//! Вкладка Code с настоящим вводом egui (без окна и GPU): набор ошибки → волна и строка
//! Error List после паузы; исправление убирает их; F8; Quick Fix одним шагом Undo;
//! переводы строк и BOM при сохранении.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Pos2};
use engine_core::prelude::App;

use crate::code_cargo::{RawDiagnostic, RawSpan};
use crate::code_diagnostics::Severity;
use crate::dock::Panel;
use crate::state::{Action, Editor, EditorConfig};
use crate::theme;

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);

pub(crate) struct Hands {
    pub ctx: egui::Context,
    pub time: f64,
    pub shapes: Vec<egui::epaint::ClippedShape>,
    /// Что последним положили в буфер обмена.
    pub copied: String,
}

impl Hands {
    pub fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new(), copied: String::new() };
        hands.frame(editor, vec![]);
        hands.frame(editor, vec![]);
        hands
    }

    pub fn frame(&mut self, editor: &mut Editor, events: Vec<Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let output = self.ctx.run(input, |ctx| editor.ui(ctx));
        #[allow(deprecated)]
        if !output.platform_output.copied_text.is_empty() {
            self.copied = output.platform_output.copied_text.clone();
        }
        self.shapes = output.shapes;
    }

    /// Подождать `seconds` (кадры по 1/60 с).
    pub fn wait(&mut self, editor: &mut Editor, seconds: f64) {
        let frames = (seconds * 60.0).ceil() as usize;
        for _ in 0..frames {
            self.frame(editor, vec![]);
        }
    }

    pub fn click(&mut self, editor: &mut Editor, pos: Pos2) {
        let button = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(editor, vec![Event::PointerMoved(pos)]);
        self.frame(editor, vec![button(true)]);
        self.frame(editor, vec![button(false)]);
        self.frame(editor, vec![]);
    }

    pub fn double_click(&mut self, editor: &mut Editor, pos: Pos2) {
        let button = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(editor, vec![Event::PointerMoved(pos)]);
        self.frame(editor, vec![button(true), button(false), button(true), button(false)]);
        self.frame(editor, vec![]);
    }

    pub fn hover(&mut self, editor: &mut Editor, pos: Pos2) {
        self.frame(editor, vec![Event::PointerMoved(pos)]);
        self.frame(editor, vec![]);
    }

    pub fn press(&mut self, editor: &mut Editor, key: Key, modifiers: Modifiers) {
        let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
    }

    pub fn type_text(&mut self, editor: &mut Editor, text: &str) {
        self.frame(editor, vec![Event::Text(text.to_string())]);
    }

    /// Центр надписи `text` последнего кадра (самая верхняя из одинаковых).
    pub fn find_text_pos(&self, text: &str) -> Option<Pos2> {
        fn walk(shape: &egui::Shape, text: &str, out: &mut Vec<Pos2>) {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => out.push(t.pos + t.galley.rect.center().to_vec2()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, text, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for s in &self.shapes {
            walk(&s.shape, text, &mut out);
        }
        out.into_iter().min_by(|a, b| a.y.total_cmp(&b.y))
    }

    /// Надписи последнего кадра (галереи текста).
    pub fn texts(&self) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for s in &self.shapes {
            walk(&s.shape, &mut out);
        }
        out
    }

    /// Нарисованы ли волнистые линии цвета `color` (подчёркивание диагностики).
    pub fn squiggles(&self, color: egui::Color32) -> usize {
        fn walk(shape: &egui::Shape, color: egui::Color32, n: &mut usize) {
            match shape {
                egui::Shape::Path(p) if p.points.len() >= 3 && !p.closed && p.stroke.color == egui::epaint::ColorMode::Solid(color) => *n += 1,
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, color, n)),
                _ => {}
            }
        }
        let mut n = 0;
        for s in &self.shapes {
            walk(&s.shape, color, &mut n);
        }
        n
    }
}

pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_code_tab_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn editor_with(dir: &Path) -> Editor {
    Editor::new(App::new(), EditorConfig::default().code_dir(dir)).unwrap()
}

/// Открыть файл, щёлкнуть в конец текста (фокус в поле), вернуть руки.
pub(crate) fn open_and_focus(editor: &mut Editor, path: &Path) -> Hands {
    editor.apply(Action::OpenCode(path.to_path_buf()));
    let mut hands = Hands::new(editor);
    hands.frame(editor, vec![]);
    let id = editor.code.documents[editor.code.active].text_id();
    let rect = hands.ctx.read_response(id).expect("the code text field is shown").rect;
    // Ниже последней строки — курсор в конец текста (у длинного файла — внизу экрана).
    let y = if rect.height() > 300.0 { rect.top() + 30.0 } else { rect.bottom() - 4.0 };
    hands.click(editor, egui::pos2(rect.left() + 200.0, y));
    assert!(hands.ctx.memory(|m| m.has_focus(id)), "the code text has keyboard focus");
    hands
}

#[test]
fn typing_a_scenario_error_shows_a_squiggle_and_an_error_list_row_after_the_pause() {
    let dir = temp_dir("scenario");
    let path = dir.join("intro.scenario");
    std::fs::write(&path, "let x = 1\nprint(x)\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code_problem_counts(), (0, 0, 0), "a valid file has no problems");

    hands.type_text(&mut editor, "let y = (1 +");
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code_problem_counts().0, 0, "not before the pause");
    hands.wait(&mut editor, 0.35);
    assert_eq!(editor.code_problem_counts().0, 1, "the compiler error after the pause");
    hands.frame(&mut editor, vec![]);
    assert!(hands.squiggles(theme::ERROR) >= 1, "a red wavy underline is painted");
    let rows = editor.error_list_rows();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("intro.scenario:3:"), "{rows:?}");
    // Status bar of the Code tab shows the count.
    assert!(hands.texts().iter().any(|t| t == "1 error"), "status bar: {:?}", hands.texts());

    // Fix it: close the expression.
    hands.type_text(&mut editor, " 2)");
    hands.wait(&mut editor, 0.45);
    assert_eq!(editor.code_problem_counts(), (0, 0, 0));
    assert_eq!(hands.squiggles(theme::ERROR), 0);
    assert!(hands.texts().iter().any(|t| t == "No issues"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rust_missing_semicolon_quick_fix_is_one_undo_step() {
    let dir = temp_dir("rust_fix");
    let path = dir.join("mover.rs");
    std::fs::write(&path, "fn a() {\n    let x = 5\n    let y = 6;\n}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    hands.wait(&mut editor, 0.1);
    let entries = crate::code_checks::entries(&editor.code);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert!(entries[0].diagnostic.message.starts_with("expected `;`"));
    assert_eq!(entries[0].diagnostic.line, 2);
    // F8 puts the caret on the error, Ctrl+. opens the fixes, Enter applies the first.
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F8, Modifiers::NONE);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code.cursor.0, 2, "F8 moved the caret to line 2");
    hands.press(&mut editor, Key::Period, Modifiers::COMMAND);
    hands.frame(&mut editor, vec![]);
    assert!(editor.code.quick_fix.is_some(), "the quick fix menu is open");
    assert!(hands.texts().iter().any(|t| t == "Insert `;`"));
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    hands.wait(&mut editor, 0.45);
    assert_eq!(editor.code_document().unwrap().1, "fn a() {\n    let x = 5;\n    let y = 6;\n}\n");
    assert_eq!(editor.code_problem_counts(), (0, 0, 0));
    // One Ctrl+Z brings the old text back.
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code_document().unwrap().1, "fn a() {\n    let x = 5\n    let y = 6;\n}\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compiler_errors_in_closed_files_open_with_f8_and_from_the_error_list() {
    let dir = temp_dir("compiler");
    let a = dir.join("a.rs");
    let b = dir.join("b.rs");
    std::fs::write(&a, "fn a() {}\n").unwrap();
    std::fs::write(&b, "fn b() {\n    let v = sped;\n}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &a);
    // Recorded result of `cargo check` (no real cargo run here): an error in b.rs, not open.
    let raw = RawDiagnostic {
        severity: Severity::Error,
        code: Some("E0425".into()),
        message: "cannot find value `sped` in this scope".into(),
        span: RawSpan { file: b.clone(), line_start: 2, col_start: 13, line_end: 2, col_end: 17 },
        hint: None,
        notes: vec![],
        fixes: vec![],
    };
    let documents = std::mem::take(&mut editor.code.documents);
    editor.code.checks.store_compiler(vec![raw], &documents, false);
    editor.code.documents = documents;
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code_problem_counts(), (1, 0, 0));
    hands.press(&mut editor, Key::F8, Modifiers::NONE);
    hands.wait(&mut editor, 0.1);
    let (path, text, _) = editor.code_document().unwrap();
    assert_eq!(path, b.as_path(), "F8 opened the file with the error");
    let text = text.to_string();
    assert_eq!(editor.code.cursor, (2, 13));
    let d = &crate::code_checks::entries(&editor.code)[0].diagnostic;
    assert_eq!(&text[d.range.clone()], "sped");

    // Error List: the tab shows the count; double-click goes back to the place.
    editor.apply(Action::OpenCode(a.clone()));
    editor.apply(Action::Layout(crate::dock::LayoutAction::Focus(Panel::ErrorList)));
    hands.wait(&mut editor, 0.05);
    assert!(editor.is_panel_visible(Panel::ErrorList));
    assert!(hands.texts().iter().any(|t| t.starts_with("Error List") && t.ends_with('1')), "tab badge: {:?}", hands.texts());
    let row = hands.ctx.read_response(Editor::error_list_row_id(0)).expect("a row").rect;
    hands.double_click(&mut editor, row.center());
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code_document().unwrap().0, b.as_path());
    // Editing above the error shifts it and marks it stale (dimmed) until the next check.
    editor.set_code_text(&format!("// top\n{text}"));
    hands.frame(&mut editor, vec![]);
    let d = crate::code_checks::entries(&editor.code).into_iter().find(|e| e.diagnostic.origin == crate::code_diagnostics::Origin::Compiler).unwrap().diagnostic;
    assert!(d.stale && d.line == 3, "{d:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn crlf_and_bom_are_kept_on_save() {
    let dir = temp_dir("crlf");
    let path = dir.join("spin.cpp");
    std::fs::write(&path, b"\xEF\xBB\xBFint a;\r\nint b;\r\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    hands.type_text(&mut editor, "int c;\n");
    hands.frame(&mut editor, vec![]);
    assert!(hands.texts().iter().any(|t| t == "CRLF") && hands.texts().iter().any(|t| t == "UTF-8 with BOM"));
    editor.apply(Action::SaveCode);
    assert_eq!(std::fs::read(&path).unwrap(), b"\xEF\xBB\xBFint a;\r\nint b;\r\nint c;\n".replace_crlf());
    let _ = std::fs::remove_dir_all(&dir);
}

trait Crlf {
    fn replace_crlf(&self) -> Vec<u8>;
}

impl Crlf for [u8] {
    /// `\n`, перед которым нет `\r`, → `\r\n`.
    fn replace_crlf(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, b) in self.iter().enumerate() {
            if *b == b'\n' && (i == 0 || self[i - 1] != b'\r') {
                out.push(b'\r');
            }
            out.push(*b);
        }
        out
    }
}

/// Корпус настоящих файлов репозитория: проверка не имеет права найти в них ни одной
/// проблемы (иначе неправа проверка). Скрипты игр — все диагностики (и проверки движка);
/// исходники движка — ни одной ошибки синтаксиса.
#[test]
fn the_repository_corpus_has_zero_false_positives() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !matches!(name.as_str(), "target" | "dist" | "build" | ".git" | "node_modules" | "vendor") && !name.starts_with('.') {
                    walk(&path, out);
                }
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join("games"), &mut files);
    walk(&root.join("crates"), &mut files);
    walk(&root.join("assets"), &mut files);
    walk(&root.join("plugins"), &mut files);
    walk(&root.join("tools"), &mut files);
    let schema = crate::code_check_data::Schema::default();
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut problems = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let script = rel.starts_with("games/") && rel.contains("/src/scripts/") && ext == "rs";
        let kind = match ext {
            "rs" if script => "game script",
            "rs" if rel.starts_with("crates/") => "engine rust",
            "scenario" => "scenario",
            "cpp" | "hpp" | "h" | "cc" => "c++",
            "wgsl" if rel.starts_with("crates/") => "wgsl",
            // Свои шейдеры: примеры, шаблоны — собираются так же, как в игре.
            "shader" => "shader",
            "scene" | "prefab" | "material" => "assets",
            _ => continue,
        };
        // Тестовые данные с намеренно сломанными файлами.
        if rel.contains("/tests/data/") || rel.contains("broken") {
            continue;
        }
        let Ok((text, _)) = crate::code_files::read(path) else { continue };
        *counts.entry(kind).or_default() += 1;
        let found = crate::code_check_data::check_file(path, &text, &schema);
        let bad: Vec<_> = if kind == "engine rust" { found.iter().filter(|d| d.severity == Severity::Error).collect() } else { found.iter().collect() };
        for d in bad {
            problems.push(format!("{rel}:{}:{}: {}", d.line, d.column, d.message));
        }
    }
    eprintln!("corpus: {counts:?}");
    assert!(counts.get("game script").copied().unwrap_or(0) >= 30, "{counts:?}");
    assert!(counts.get("scenario").copied().unwrap_or(0) >= 5, "{counts:?}");
    assert!(problems.is_empty(), "{} false positives:\n{}", problems.len(), problems.join("\n"));
}

/// Правка строк и комментарии — настоящими нажатиями (сочетания Visual Studio).
#[test]
fn visual_studio_line_operations_and_comments_work_from_the_keyboard() {
    let dir = temp_dir("line_ops");
    let path = dir.join("ops.rs");
    std::fs::write(&path, "fn a() {\n    one();\n    two();\n}").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    let id = editor.code.documents[editor.code.active].text_id();
    // Курсор на `one();` (строка 2).
    let text = editor.code_document().unwrap().1.to_string();
    let at = text.find("one").unwrap();
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    let doc = |editor: &Editor| editor.code_document().unwrap().1.to_string();

    hands.press(&mut editor, Key::D, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    one();\n    two();\n}", "Ctrl+D duplicates the line");
    hands.press(&mut editor, Key::K, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    two();\n}", "Ctrl+Shift+K deletes it");
    // Как в Visual Studio: курсор остаётся на строке, которая встала на место удалённой.
    assert_eq!(editor.code.cursor.0, 3);
    let text = doc(&editor);
    let at = text.find("one").unwrap();
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::ArrowDown, Modifiers::ALT);
    assert_eq!(doc(&editor), "fn a() {\n    two();\n    one();\n}", "Alt+Down moves it");
    hands.press(&mut editor, Key::ArrowUp, Modifiers::ALT);
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    two();\n}", "Alt+Up moves it back");
    hands.press(&mut editor, Key::ArrowDown, Modifiers::ALT.plus(Modifiers::SHIFT));
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    one();\n    two();\n}", "Shift+Alt+Down copies it");
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    two();\n}", "each operation is one undo step");
    hands.press(&mut editor, Key::Slash, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    // one();\n    two();\n}", "Ctrl+/ comments the line");
    hands.press(&mut editor, Key::Slash, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    two();\n}", "and uncomments it");
    // Ctrl+K, Ctrl+C — сочетание из двух нажатий.
    hands.press(&mut editor, Key::K, Modifiers::COMMAND);
    hands.press(&mut editor, Key::C, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    // one();\n    two();\n}", "Ctrl+K Ctrl+C comments");
    hands.press(&mut editor, Key::K, Modifiers::COMMAND);
    hands.press(&mut editor, Key::U, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    two();\n}", "Ctrl+K Ctrl+U uncomments");
    hands.press(&mut editor, Key::U, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    assert_eq!(doc(&editor), "fn a() {\n    ONE();\n    two();\n}", "Ctrl+Shift+U upper-cases the word");
    hands.press(&mut editor, Key::U, Modifiers::COMMAND);
    assert_eq!(doc(&editor), "fn a() {\n    one();\n    two();\n}", "Ctrl+U lower-cases it");
    let help = editor.shortcut_text("comment-lines");
    assert_eq!(help, "Ctrl+K, Ctrl+C");

    // Набор: парная скобка, перешагивание, Enter с отступом.
    let text = doc(&editor);
    let at = text.find("one();").unwrap() + "one();".len();
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    for c in "if x {".chars() {
        hands.type_text(&mut editor, &c.to_string());
    }
    assert!(doc(&editor).contains("    if x {}\n"), "the brace pair: {:?}", doc(&editor));
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    for c in "go(".chars() {
        hands.type_text(&mut editor, &c.to_string());
    }
    for c in ")".chars() {
        hands.type_text(&mut editor, &c.to_string());
    }
    for c in ";".chars() {
        hands.type_text(&mut editor, &c.to_string());
    }
    assert!(doc(&editor).contains("    if x {\n        go();\n    }\n"), "{:?}", doc(&editor));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Поиск с параметрами, F3 / Shift+F3, замена с группами — одним шагом Undo.
#[test]
fn find_options_f3_and_regex_replace() {
    let dir = temp_dir("find");
    let path = dir.join("f.rs");
    std::fs::write(&path, "let speed = 1;\nlet Speed = 2;\nlet speedy = 3;\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    hands.press(&mut editor, Key::F, Modifiers::COMMAND);
    hands.type_text(&mut editor, "speed");
    hands.frame(&mut editor, vec![]);
    assert!(hands.texts().iter().any(|t| t == "1 of 3"), "{:?}", hands.texts());
    editor.code.find.options.match_case = true;
    editor.code.find.options.whole_word = true;
    hands.frame(&mut editor, vec![]);
    assert!(hands.texts().iter().any(|t| t == "1 of 1"), "{:?}", hands.texts());
    editor.code.find.options = Default::default();
    // F3 по тексту: фокус обратно в код.
    let id = editor.code.documents[0].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    crate::code_keys::set_selection(&hands.ctx, id, &editor.code.documents[0].text.clone(), 0, 0);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F3, Modifiers::NONE);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code.find.current, 0);
    hands.press(&mut editor, Key::F3, Modifiers::NONE);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code.find.current, 1, "F3 goes to the next match");
    hands.press(&mut editor, Key::F3, Modifiers::SHIFT);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code.find.current, 0, "Shift+F3 goes back");
    // Регулярное выражение с группами.
    editor.code.find.options = crate::code_find::FindOptions { regex: true, match_case: true, ..Default::default() };
    editor.code.find.query = r"let (\w+) = (\d)".into();
    editor.code.find.replace = true;
    editor.code.find.replacement = "const $1: i32 = $2".into();
    hands.frame(&mut editor, vec![]);
    let pos = hands.find_text_pos("Replace All").expect("Replace All button");
    hands.click(&mut editor, pos);
    assert_eq!(editor.code_document().unwrap().1, "const speed: i32 = 1;\nconst Speed: i32 = 2;\nconst speedy: i32 = 3;\n");
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    assert_eq!(editor.code_document().unwrap().1, "let speed = 1;\nlet Speed = 2;\nlet speedy = 3;\n", "replace all is one undo step");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Go to File (Ctrl+P), Go to Definition (F12) на символ проекта и на символ движка,
/// назад (Ctrl+−), вкладки по недавности (Ctrl+Tab) и открыть закрытую (Ctrl+Shift+T).
#[test]
fn navigation_go_to_file_definition_history_and_tabs() {
    let dir = temp_dir("nav");
    let a = dir.join("player_move.rs");
    let b = dir.join("helpers.rs");
    std::fs::write(&a, "fn tick(ctx: &mut Ctx) {\n    let t = Timer::default();\n    jump_height(2.0);\n}\n").unwrap();
    std::fs::write(&b, "/// How high.\npub fn jump_height(scale: f32) -> f32 {\n    scale * 3.0\n}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.press(&mut editor, Key::P, Modifiers::COMMAND);
    assert!(editor.code.nav.picker.is_some(), "Ctrl+P opens Go to File");
    hands.type_text(&mut editor, "hlp");
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code_document().unwrap().0, b.as_path());
    // Обратно в a.rs, курсор на `jump_height`, F12.
    hands.press(&mut editor, Key::Tab, Modifiers::COMMAND);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code_document().unwrap().0, a.as_path(), "Ctrl+Tab: the previous tab");
    let id = editor.code.documents[editor.code.active].text_id();
    let text = editor.code_document().unwrap().1.to_string();
    let at = text.find("jump_height").unwrap() + 2;
    hands.ctx.memory_mut(|m| m.request_focus(id));
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F12, Modifiers::NONE);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code_document().unwrap().0, b.as_path(), "F12 opens the definition");
    assert_eq!(editor.code.cursor.0, 2);
    hands.press(&mut editor, Key::Minus, Modifiers::COMMAND);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code_document().unwrap().0, a.as_path(), "Ctrl+- goes back");
    // Символ движка: `Timer` — исходник движка.
    let id = editor.code.documents[editor.code.active].text_id();
    let at = text.find("Timer").unwrap() + 1;
    hands.ctx.memory_mut(|m| m.request_focus(id));
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F12, Modifiers::NONE);
    hands.wait(&mut editor, 0.05);
    let (path, shown, _) = editor.code_document().unwrap();
    assert!(path.ends_with("timer.rs"), "{path:?}");
    assert!(shown.contains("pub struct Timer"));
    // Закрыть вкладку и открыть снова.
    let index = editor.code.active;
    editor.code.close(index);
    hands.frame(&mut editor, vec![]);
    let id = editor.code.documents[editor.code.active].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::T, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    hands.wait(&mut editor, 0.05);
    assert!(editor.code_document().unwrap().0.ends_with("timer.rs"), "Ctrl+Shift+T reopens it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Подсказки после `ctx.`: методы `Ctx` из индекса движка; Enter вставляет.
#[test]
fn completion_after_ctx_lists_ctx_methods() {
    let dir = temp_dir("complete");
    let path = dir.join("c.rs");
    std::fs::write(&path, "fn f(ctx: &mut Ctx) {\n    \n}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    let id = editor.code.documents[0].text_id();
    let text = editor.code_document().unwrap().1.to_string();
    let at = text.find("    \n").unwrap() + 4;
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    for c in "ctx.".chars() {
        hands.type_text(&mut editor, &c.to_string());
    }
    hands.frame(&mut editor, vec![]);
    let popup = editor.code.completion.as_ref().expect("the completion list is open");
    assert!(popup.items.len() > 5 && popup.items.iter().all(|i| i.kind == "method"), "{:?}", popup.items.iter().map(|i| &i.label).collect::<Vec<_>>());
    let first = popup.items[0].label.clone();
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    assert!(editor.code_document().unwrap().1.contains(&format!("ctx.{first}(")), "{}", editor.code_document().unwrap().1);
    assert!(editor.code.completion.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Find in Files и Find All References (по тексту): результаты по файлам, щелчок — место.
#[test]
fn find_in_files_and_references() {
    let dir = temp_dir("fif");
    std::fs::write(dir.join("a.rs"), "fn speed() {}\n// speed\n").unwrap();
    std::fs::write(dir.join("b.rs"), "fn x() { speed(); let s = \"speed\"; }\n").unwrap();
    std::fs::write(dir.join("c.scenario"), "speed = 2\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &dir.join("a.rs"));
    editor.code.search.query = "speed".into();
    editor.find_in_files();
    assert_eq!(editor.find_in_files_counts(), (3, 5));
    // Ссылки: без комментария и строки.
    let id = editor.code.documents[0].text_id();
    let text = editor.code_document().unwrap().1.to_string();
    crate::code_keys::set_selection(&hands.ctx, id, &text, 4, 4);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F12, Modifiers::SHIFT);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.find_in_files_counts(), (3, 3), "{:?}", editor.code.search.results);
    assert!(editor.is_panel_visible(Panel::FindResults));
    let row = hands.ctx.read_response(Editor::find_result_row_id(1, 0)).expect("a result row").rect;
    hands.click(&mut editor, row.center());
    hands.wait(&mut editor, 0.05);
    assert!(editor.code_document().unwrap().0.ends_with("b.rs"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rename (F2) в файле; Format Document для Scenario; перечитать файл, изменённый снаружи.
#[test]
fn rename_format_and_external_reload() {
    let dir = temp_dir("rename");
    let path = dir.join("r.rs");
    std::fs::write(&path, "fn f() {\n    let speed = 1; // speed\n    let x = speed + 1;\n}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    let id = editor.code.documents[0].text_id();
    let text = editor.code_document().unwrap().1.to_string();
    let at = text.find("speed").unwrap() + 1;
    crate::code_keys::set_selection(&hands.ctx, id, &text, at, at);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F2, Modifiers::NONE);
    hands.frame(&mut editor, vec![]);
    let rename = editor.code.rename.as_mut().expect("the rename dialog");
    assert_eq!(rename.here, 2);
    rename.new = "pace".into();
    hands.frame(&mut editor, vec![]);
    // Enter в поле нового имени — Rename.
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    assert!(editor.code.rename.is_none(), "the dialog closed");
    assert_eq!(editor.code_document().unwrap().1, "fn f() {\n    let pace = 1; // speed\n    let x = pace + 1;\n}\n");

    let s = dir.join("s.scenario");
    std::fs::write(&s, "function go()\nif x then\nprint(1)\nend\nend\n").unwrap();
    editor.apply(Action::OpenCode(s.clone()));
    hands.frame(&mut editor, vec![]);
    let id = editor.code.documents[editor.code.active].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::K, Modifiers::COMMAND);
    hands.press(&mut editor, Key::D, Modifiers::COMMAND);
    assert_eq!(editor.code_document().unwrap().1, "function go()\n    if x then\n        print(1)\n    end\nend\n", "Ctrl+K Ctrl+D");

    // Чистый файл, изменённый снаружи, перечитывается сам.
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&path, "fn g() {}\n").unwrap();
    editor.apply(Action::OpenCode(path.clone()));
    editor.apply(Action::SaveCode);
    std::fs::write(&path, "fn changed() {}\n").unwrap();
    hands.wait(&mut editor, 1.2);
    assert_eq!(editor.code_document().unwrap().1, "fn changed() {}\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Задержка нажатия клавиши в файле на 20 000 строк (без окна, CPU) и время проверки.
#[test]
fn keystroke_latency_on_a_20000_line_file() {
    let dir = temp_dir("perf");
    let path = dir.join("big.rs");
    let mut text = String::new();
    for i in 0..20_000 {
        text.push_str(&format!("fn f{i}(x: f32) -> f32 {{ x * {i}.0 + 1.0 }}\n"));
    }
    std::fs::write(&path, &text).unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    hands.wait(&mut editor, 0.5);
    let mut times = Vec::new();
    for _ in 0..5 {
        let started = std::time::Instant::now();
        hands.type_text(&mut editor, "x");
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    hands.wait(&mut editor, 1.0);
    let t = std::time::Instant::now();
    for _ in 0..3 {
        hands.frame(&mut editor, vec![]);
    }
    let idle_ms = t.elapsed().as_secs_f64() * 1000.0 / 3.0;
    eprintln!("20000 lines: idle frame {idle_ms:.1} ms");
    let big = editor.code_document().unwrap().1.to_string();
    let t = std::time::Instant::now();
    let job = crate::code::highlight(&big, crate::code::Language::Rust, crate::theme::mono());
    let highlight_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let galley = hands.ctx.fonts(|f| f.layout_job(job));
    let layout_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let symbols = crate::code_symbols::symbols(&path, &big);
    let symbols_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let _ = crate::code_complete::complete(&path, &big, 100, crate::code::Language::Rust, &symbols, false);
    let complete_ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("20000 lines parts: highlight {highlight_ms:.1} ms, layout {layout_ms:.1} ms ({} rows), symbols {symbols_ms:.1} ms, completion {complete_ms:.1} ms", galley.rows.len());
    let started = std::time::Instant::now();
    let found = crate::code_check_data::check_file(&path, editor.code_document().unwrap().1, &Default::default());
    let check_ms = started.elapsed().as_secs_f64() * 1000.0;
    eprintln!("20000 lines: keystroke frame median {:.1} ms (min {:.1}, max {:.1}); Rust check {:.1} ms; {} diagnostics", times[2], times[0], times[4], check_ms, found.len());
    // Свой виджет раскладывает только видимые строки: было 0,3–0,6 с, теперь единицы мс
    // (запас на загруженную машину — сборки пользователя идут параллельно).
    assert!(times[2] < 30.0, "a keystroke takes {:.0} ms", times[2]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Таблица задержек для файлов на 200, 2 000 и 20 000 строк: открытие (команда и первый
/// кадр), нажатие клавиши, кадр прокрутки колесом. Печатает числа (`--nocapture`).
#[test]
fn latency_table_for_200_2000_and_20000_lines() {
    let dir = temp_dir("perf_table");
    for lines in [200usize, 2_000, 20_000] {
        let path = dir.join(format!("big{lines}.rs"));
        let mut text = String::new();
        for i in 0..lines {
            text.push_str(&format!("fn f{i}(x: f32) -> f32 {{ x * {i}.0 + 1.0 }} // строка {i}\n"));
        }
        std::fs::write(&path, &text).unwrap();
        let mut editor = editor_with(&dir);
        let mut hands = Hands::new(&mut editor);
        let started = std::time::Instant::now();
        editor.apply(Action::OpenCode(path.clone()));
        hands.frame(&mut editor, vec![]);
        let open_ms = started.elapsed().as_secs_f64() * 1000.0;
        let id = editor.code.documents[editor.code.active].text_id();
        let rect = hands.ctx.read_response(id).expect("the code text is shown").rect;
        let at = egui::pos2(rect.left() + 200.0, rect.top() + 30.0);
        hands.click(&mut editor, at);
        hands.wait(&mut editor, 0.5);
        let mut keys = Vec::new();
        for _ in 0..9 {
            let t = std::time::Instant::now();
            hands.type_text(&mut editor, "x");
            keys.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        keys.sort_by(f64::total_cmp);
        let mut scrolls = Vec::new();
        for _ in 0..9 {
            let t = std::time::Instant::now();
            hands.frame(&mut editor, vec![Event::PointerMoved(at), Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, -3.0), modifiers: Modifiers::NONE }]);
            scrolls.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        scrolls.sort_by(f64::total_cmp);
        eprintln!("latency {lines:>6} lines: open {open_ms:.1} ms, keystroke median {:.1} ms (max {:.1}), scroll frame median {:.1} ms (max {:.1})", keys[4], keys[8], scrolls[4], scrolls[8]);
        assert_eq!(editor.code_document().unwrap().1.matches('x').count(), text.matches('x').count() + 9);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Снимки вкладки Code без окна (GPU): `W15_SHOTS=<каталог> cargo test -p engine-editor
/// --lib code_tab_screenshots -- --ignored`. Английский 100 % и русский 150 %.
#[test]
#[ignore = "GPU: снимки для просмотра"]
fn code_tab_screenshots() {
    let Ok(out) = std::env::var("W15_SHOTS") else { return };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    for (language, scale, tag) in [(engine_i18n::Language::English, 1.0_f32, "en_100"), (engine_i18n::Language::Russian, 1.5, "ru_150")] {
        let dir = temp_dir(&format!("shots_{tag}"));
        let path = dir.join("player_move.rs");
        std::fs::write(&path, "use engine::prelude::*;\n\ncomponent! {\n    /// Moves the player.\n    pub struct PlayerMove {\n        pub speed: f64 = 4.0,\n        pub jump: f32 = 6.0,\n    }\n}\n\nimpl Component for PlayerMove {\n    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {\n        let x = 5\n        let v = (1.0, 2.0];\n    }\n}\n").unwrap();
        std::fs::write(dir.join("helpers.rs"), "pub fn jump_height(scale: f32) -> f32 {\n    scale * 3.0\n}\n").unwrap();
        let mut editor = editor_with(&dir);
        let mut prefs = editor.preferences().clone();
        prefs.ui_scale = scale;
        prefs.language = Some(language);
        editor.set_preferences(prefs);
        engine_i18n::set(language);
        editor.apply(Action::OpenCode(path.clone()));
        let (w, h) = ((1440.0 * scale) as u32, (860.0 * scale) as u32);
        // 1. Волны и подсказка при наведении.
        let shot = crate::gpu::capture_with_input(&mut editor, w, h, 8, |frame, editor| match (frame, editor.code.first_diagnostic_at) {
            (f, Some(at)) if f >= 4 => vec![Event::PointerMoved(at)],
            _ => vec![],
        })
        .unwrap();
        shot.save(out.join(format!("code_squiggles_hover_{tag}.png"))).unwrap();
        // 2. Error List.
        editor.apply(Action::Layout(crate::dock::LayoutAction::Focus(Panel::ErrorList)));
        crate::gpu::capture(&mut editor, w, h, 4).unwrap().save(out.join(format!("error_list_{tag}.png"))).unwrap();
        // 3. Подсказки после `ctx.`
        editor.apply(Action::OpenCode(path.clone()));
        let text = editor.code_document().unwrap().1.to_string();
        // Подсказки — состояние ставится напрямую (фокус в снимке не нужен), затем снимок.
        let caret = text.find("let x").unwrap();
        let (anchor, items) = crate::code_complete::complete(&path, &format!("{}ctx.{}", &text[..caret], &text[caret..]), caret + 4, crate::code::Language::Rust, &[], false);
        editor.set_code_text(&format!("{}ctx.{}", &text[..caret], &text[caret..]));
        let at = editor.code.first_diagnostic_at.unwrap_or(egui::pos2(400.0, 300.0));
        editor.code.completion = Some(crate::code_complete::Popup { path: path.clone(), anchor, items, selected: 0, at: at + egui::vec2(0.0, 40.0) });
        crate::gpu::capture(&mut editor, w, h, 3).unwrap().save(out.join(format!("completion_{tag}.png"))).unwrap();
        editor.code.completion = None;
        editor.set_code_text(&text);
        // 4. Find Results.
        editor.code.search.query = "jump".into();
        editor.find_in_files();
        editor.apply(Action::Layout(crate::dock::LayoutAction::Focus(Panel::FindResults)));
        crate::gpu::capture(&mut editor, w, h, 4).unwrap().save(out.join(format!("find_results_{tag}.png"))).unwrap();
        // 5. Go to File.
        editor.apply(Action::OpenCode(path.clone()));
        editor.open_picker(crate::code_nav::PickerKind::Files);
        editor.code.nav.picker.as_mut().unwrap().query = "pm".into();
        crate::gpu::capture(&mut editor, w, h, 4).unwrap().save(out.join(format!("go_to_file_{tag}.png"))).unwrap();
        editor.code.nav.picker = None;
        // 6. Preferences → Code Editor.
        editor.show_settings_page(crate::settings::SettingsScope::Preferences, "code-editor");
        crate::gpu::capture(&mut editor, w, h, 4).unwrap().save(out.join(format!("prefs_code_editor_{tag}.png"))).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
    engine_i18n::set(engine_i18n::Language::English);
}

/// Наведение на волну — подсказка с текстом ошибки и «Quick Fix… (Ctrl+.)».
#[test]
fn hovering_a_squiggle_shows_the_message_and_the_quick_fix_hint() {
    let dir = temp_dir("hover");
    let path = dir.join("h.rs");
    std::fs::write(&path, "fn a() {\n    let x = 5\n    let y = 6;\n}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    hands.wait(&mut editor, 0.1);
    let at = editor.code.first_diagnostic_at.expect("the error is on screen");
    hands.hover(&mut editor, at);
    hands.wait(&mut editor, 0.6);
    let texts = hands.texts();
    assert!(texts.iter().any(|t| t.starts_with("expected `;`")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Quick Fix… (Ctrl+.)"), "{texts:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Сборка C++: строки MSVC / GCC из вывода команды сборки становятся диагностиками файла.
#[test]
fn cpp_build_output_lands_in_the_error_list() {
    let dir = temp_dir("cpp_build");
    let cpp = dir.join("spin.cpp");
    std::fs::write(&cpp, "int a;\nint b = x;\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &cpp);
    let line = "spin.cpp(2,9): error C2065: 'x': undeclared identifier";
    let command = if cfg!(windows) {
        crate::code::BuildCommand::new("C++ plugin", "cmd").args(["/C", &format!("echo {line}& exit 1")]).dir(&dir)
    } else {
        crate::code::BuildCommand::new("C++ plugin", "sh").args(["-c", &format!("echo \"{line}\"; exit 1")]).dir(&dir)
    };
    editor.code.start_build(vec![command]);
    for _ in 0..300 {
        hands.frame(&mut editor, vec![]);
        if editor.code_problem_counts().0 > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let entries = crate::code_checks::entries(&editor.code);
    let d = entries.iter().find(|e| e.diagnostic.code.as_deref() == Some("C2065")).expect("the MSVC error is listed");
    assert_eq!((d.diagnostic.line, d.diagnostic.column), (2, 9));
    assert!(crate::code_diagnostics::same_path(&d.path, &cpp));
    let _ = std::fs::remove_dir_all(&dir);
}
