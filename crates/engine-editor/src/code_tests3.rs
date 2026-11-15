//! Меню и дерево файлов вкладки Code настоящим вводом egui (во временном каталоге):
//! правый щелчок по тексту и пункты меню, выделение мышью, дерево файлов (выделение,
//! клавиатура, переименование, удаление с вопросом, новый скрипт в `mod.rs`, копирование,
//! перетаскивание), меню вкладок и Ctrl+W.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use engine_core::prelude::App;

use crate::code_tests::{editor_with, open_and_focus, temp_dir, Hands};
use crate::state::{Action, Editor, EditorConfig};

fn text(editor: &Editor) -> String {
    editor.code_document().unwrap().1.to_string()
}

fn carets(editor: &Editor, hands: &Hands) -> Vec<(usize, usize)> {
    let id = editor.code.documents[editor.code.active].text_id();
    crate::code_keys::carets(&hands.ctx, id).sels.iter().map(|s| (s.anchor, s.caret)).collect()
}

/// Экранная точка символа `column` строки `line` текста.
fn at(editor: &Editor, line: usize, column: usize) -> Pos2 {
    let (origin, cw, lh) = editor.code.geometry.expect("the code text is laid out");
    egui::pos2(origin.x + (column as f32 + 0.1) * cw, origin.y + (line as f32 + 0.5) * lh)
}

/// Все места надписи `text` последнего кадра.
fn spots(hands: &Hands, text: &str) -> Vec<Pos2> {
    fn walk(shape: &egui::Shape, text: &str, out: &mut Vec<Pos2>) {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => out.push(t.pos + t.galley.rect.center().to_vec2()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, text, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in &hands.shapes {
        walk(&s.shape, text, &mut out);
    }
    out
}

/// Строка дерева файлов с именем `name` (под заголовком FILES).
fn tree_row(hands: &Hands, name: &str) -> Pos2 {
    let header = hands.find_text_pos("FILES").expect("the FILES header");
    spots(hands, name)
        .into_iter()
        .filter(|p| p.y > header.y + 10.0 && (p.x - header.x).abs() < 220.0)
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap_or_else(|| panic!("`{name}` in the file tree: {:?}", hands.texts()))
}

fn press(hands: &mut Hands, editor: &mut Editor, pos: Pos2, button: PointerButton, modifiers: Modifiers) {
    let event = |pressed| Event::PointerButton { pos, button, pressed, modifiers };
    hands.frame(editor, vec![Event::PointerMoved(pos)]);
    hands.frame(editor, vec![event(true)]);
    hands.frame(editor, vec![event(false)]);
    hands.frame(editor, vec![]);
}

fn right_click(hands: &mut Hands, editor: &mut Editor, pos: Pos2) {
    press(hands, editor, pos, PointerButton::Secondary, Modifiers::NONE);
}

/// Щёлкнуть пункт открытого меню.
fn menu(hands: &mut Hands, editor: &mut Editor, item: &str) {
    let pos = *spots(hands, item).last().unwrap_or_else(|| panic!("menu item `{item}`: {:?}", hands.texts()));
    hands.click(editor, pos);
}

/// Пауза, чтобы следующий щелчок не стал двойным.
fn pause(hands: &mut Hands, editor: &mut Editor) {
    hands.wait(editor, 0.6);
}

#[test]
fn right_click_menu_in_the_text_moves_the_caret_and_runs_items() {
    let dir = temp_dir("menu_text");
    let path = dir.join("m.rs");
    std::fs::write(&path, "fn speed() {}\nlet x = speed();\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    pause(&mut hands, &mut editor);
    // Правый щелчок по `speed` второй строки: курсор туда, меню у указателя.
    let pos = at(&editor, 1, 10);
    right_click(&mut hands, &mut editor, pos);
    assert_eq!(carets(&editor, &hands), vec![(24, 24)], "the caret moved to the right-click");
    let item = *spots(&hands, "Cut").last().expect("the menu");
    assert!(item.y > pos.y && item.y - pos.y < 40.0 && item.x > pos.x, "the menu opens at the pointer: {item:?} vs {pos:?}");
    assert!(hands.texts().iter().any(|t| t == "Go to Definition"));
    assert!(hands.texts().iter().any(|t| t == "F12"), "items show their shortcuts");
    menu(&mut hands, &mut editor, "Toggle Line Comment");
    hands.frame(&mut editor, vec![]);
    assert_eq!(text(&editor), "fn speed() {}\n// let x = speed();\n");
    // Правый щелчок внутри выделения его не трогает; Copy копирует его.
    pause(&mut hands, &mut editor);
    let id = editor.code.documents[0].text_id();
    crate::code_keys::set_selection(&hands.ctx, id, &text(&editor), 3, 8);
    hands.frame(&mut editor, vec![]);
    let p = at(&editor, 0, 5);
    right_click(&mut hands, &mut editor, p);
    assert_eq!(carets(&editor, &hands), vec![(3, 8)], "the selection is kept");
    menu(&mut hands, &mut editor, "Copy");
    hands.frame(&mut editor, vec![]);
    assert_eq!(hands.copied, "speed");
    // Paste из меню — текст буфера обмена.
    crate::code_menu::TEST_CLIPBOARD.with(|c| *c.borrow_mut() = Some("pace".into()));
    pause(&mut hands, &mut editor);
    let p = at(&editor, 0, 5);
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "Paste");
    hands.frame(&mut editor, vec![]);
    assert_eq!(text(&editor), "fn pace() {}\n// let x = speed();\n");
    // Select All, Copy Path, Show in File Manager.
    pause(&mut hands, &mut editor);
    let p = at(&editor, 0, 1);
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "Select All");
    hands.frame(&mut editor, vec![]);
    assert_eq!(carets(&editor, &hands), vec![(0, text(&editor).len())]);
    pause(&mut hands, &mut editor);
    let p = at(&editor, 0, 1);
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "Copy Path");
    assert_eq!(hands.copied, path.display().to_string());
    pause(&mut hands, &mut editor);
    let p = at(&editor, 0, 1);
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "Show in File Manager");
    assert_eq!(editor.code.tree.launched, vec![path.clone()]);
    // Пункт без смысла здесь выключен: Quick Fix — нет исправления у курсора.
    pause(&mut hands, &mut editor);
    let p = at(&editor, 0, 1);
    right_click(&mut hands, &mut editor, p);
    let before = text(&editor);
    menu(&mut hands, &mut editor, "Quick Fix…");
    assert!(editor.code.quick_fix.is_none() && text(&editor) == before, "a disabled item does nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mouse_selection_drag_double_triple_shift_click_and_auto_scroll() {
    let dir = temp_dir("mouse");
    let path = dir.join("s.rs");
    let body: String = (0..200).map(|i| format!("let value_{i} = {i};\n")).collect();
    std::fs::write(&path, &body).unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    pause(&mut hands, &mut editor);
    // Перетаскивание по первой строке.
    let (a, b) = (at(&editor, 0, 4), at(&editor, 0, 11));
    hands.frame(&mut editor, vec![Event::PointerMoved(a)]);
    hands.frame(&mut editor, vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    for k in 1..=3 {
        hands.frame(&mut editor, vec![Event::PointerMoved(a + (b - a) * (k as f32 / 3.0))]);
    }
    hands.frame(&mut editor, vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    assert_eq!(carets(&editor, &hands), vec![(4, 11)], "drag selects `value_0`");
    // Shift+щелчок продолжает выделение.
    pause(&mut hands, &mut editor);
    let p = at(&editor, 1, 4);
    press(&mut hands, &mut editor, p, PointerButton::Primary, Modifiers::SHIFT);
    assert_eq!(carets(&editor, &hands), vec![(4, 21)]);
    // Двойной щелчок — слово, тройной — строка.
    pause(&mut hands, &mut editor);
    let p = at(&editor, 2, 6);
    hands.double_click(&mut editor, p);
    assert_eq!(carets(&editor, &hands), vec![(38, 45)], "double-click selects `value_2`");
    pause(&mut hands, &mut editor);
    let click = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    hands.frame(&mut editor, vec![Event::PointerMoved(p)]);
    hands.frame(&mut editor, vec![click(true), click(false), click(true), click(false), click(true), click(false)]);
    hands.frame(&mut editor, vec![]);
    assert_eq!(carets(&editor, &hands), vec![(34, 51)], "triple-click selects the line");
    // Тянут за нижний край — текст прокручивается, выделение растёт.
    pause(&mut hands, &mut editor);
    let start = at(&editor, 0, 0);
    let id = editor.code.documents[0].text_id();
    let rect = hands.ctx.read_response(id).unwrap().interact_rect;
    let below = egui::pos2(start.x + 40.0, rect.bottom() + 30.0);
    hands.frame(&mut editor, vec![Event::PointerMoved(start)]);
    hands.frame(&mut editor, vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    hands.frame(&mut editor, vec![Event::PointerMoved(start + egui::vec2(10.0, 20.0))]);
    let first = editor.code.geometry.unwrap().0.y;
    for _ in 0..30 {
        hands.frame(&mut editor, vec![Event::PointerMoved(below)]);
    }
    hands.frame(&mut editor, vec![Event::PointerButton { pos: below, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    let scrolled = first - editor.code.geometry.unwrap().0.y;
    assert!(scrolled > 100.0, "dragging past the edge scrolls ({scrolled} px)");
    let (anchor, caret) = carets(&editor, &hands)[0];
    assert_eq!(anchor, 0);
    assert!(caret > 30 * 15, "the selection follows the scroll: {caret}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Временный проект: `src/scripts` (скрипты на Rust с `mod.rs`), `src/util`, пустой `src/empty`.
fn project(name: &str) -> (PathBuf, PathBuf) {
    let dir = temp_dir(name);
    let src = dir.join("src");
    std::fs::create_dir_all(sp(&src, "scripts")).unwrap();
    std::fs::create_dir_all(sp(&src, "util")).unwrap();
    std::fs::create_dir_all(sp(&src, "empty")).unwrap();
    std::fs::write(sp(&src, "main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(sp(&src, "scripts/mover.rs"), "use engine::prelude::*;\ncomponent! { pub struct Mover { pub speed: f32 = 1.0, } }\nimpl Component for Mover {}\n").unwrap();
    std::fs::write(sp(&src, "scripts/mod.rs"), "").unwrap();
    std::fs::write(sp(&src, "util/a.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(sp(&src, "util/b.rs"), "pub fn b() {}\n").unwrap();
    std::fs::write(sp(&src, "util/c.rs"), "pub fn c() {}\n").unwrap();
    (dir, src)
}

/// Путь внутри `src` с разделителями системы.
fn sp(src: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(src.to_path_buf(), |path, part| path.join(part))
}

fn project_editor(src: &Path) -> Editor {
    Editor::new(App::new(), EditorConfig::default().code_dir(src).rust_scripts(sp(src, "scripts"))).unwrap()
}

#[test]
fn file_list_selection_keyboard_and_type_ahead() {
    let (dir, src) = project("tree_select");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &sp(&src, "main.rs"));
    pause(&mut hands, &mut editor);
    let _ = tree_row(&hands, "empty");
    // Щелчок — выделить и открыть, Ctrl+щелчок — добавить, Shift+щелчок — диапазон.
    let a = tree_row(&hands, "a.rs");
    hands.click(&mut editor, a);
    assert_eq!(editor.code.tree.selected, vec![sp(&src, "util/a.rs")]);
    assert_eq!(editor.code_document().unwrap().0, sp(&src, "util/a.rs").as_path(), "a click opens the file");
    pause(&mut hands, &mut editor);
    let c = tree_row(&hands, "c.rs");
    press(&mut hands, &mut editor, c, PointerButton::Primary, Modifiers::COMMAND);
    assert_eq!(editor.code.tree.selected, vec![sp(&src, "util/a.rs"), sp(&src, "util/c.rs")], "Ctrl+click adds");
    pause(&mut hands, &mut editor);
    press(&mut hands, &mut editor, c, PointerButton::Primary, Modifiers::COMMAND);
    assert_eq!(editor.code.tree.selected, vec![sp(&src, "util/a.rs")], "Ctrl+click again removes");
    // Shift+щелчок — диапазон от якоря (простой щелчок ставит якорь).
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "a.rs");
    hands.click(&mut editor, p);
    pause(&mut hands, &mut editor);
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "c.rs");
    press(&mut hands, &mut editor, p, PointerButton::Primary, Modifiers::SHIFT);
    assert_eq!(editor.code.tree.selected.len(), 3, "Shift+click selects the range a..c");
    // Стрелки, Enter, набор имени.
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "a.rs");
    hands.click(&mut editor, p);
    hands.press(&mut editor, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(editor.code.tree.selected, vec![sp(&src, "util/b.rs")]);
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    hands.wait(&mut editor, 0.05);
    assert_eq!(editor.code_document().unwrap().0, sp(&src, "util/b.rs").as_path(), "Enter opens");
    hands.type_text(&mut editor, "m");
    hands.type_text(&mut editor, "a");
    assert_eq!(editor.code.tree.selected, vec![sp(&src, "main.rs")], "type-ahead finds `main.rs`");
    // ← сворачивает каталог, → разворачивает.
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "util");
    hands.click(&mut editor, p);
    assert!(editor.code.collapsed.iter().any(|d| d.ends_with("util")), "a click on a folder collapses it");
    hands.press(&mut editor, Key::ArrowRight, Modifiers::NONE);
    assert!(!editor.code.collapsed.iter().any(|d| d.ends_with("util")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_in_place_keeps_the_tab_and_its_undo_history() {
    let (dir, src) = project("tree_rename");
    let mut editor = project_editor(&src);
    let a = sp(&src, "util/a.rs");
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    let edited = text(&editor);
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "a.rs");
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "Rename…");
    hands.frame(&mut editor, vec![]);
    assert!(editor.code.tree.rename.is_some(), "the name is edited in place");
    hands.type_text(&mut editor, "alpha");
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    hands.frame(&mut editor, vec![]);
    let to = sp(&src, "util/alpha.rs");
    assert!(to.exists() && !a.exists(), "renamed on disk");
    assert_eq!(editor.code_document().unwrap().0, to.as_path(), "the tab follows the file");
    assert_eq!(text(&editor), edited, "unsaved text is kept");
    // История Undo осталась при вкладке.
    let id = editor.code.documents[editor.code.active].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    assert_eq!(text(&editor), "pub fn a() {}\n");
    // F2 в дереве — тоже переименование.
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "b.rs");
    hands.click(&mut editor, p);
    let tree = egui::Id::new("code_file_tree");
    hands.ctx.memory_mut(|m| m.request_focus(tree));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::F2, Modifiers::NONE);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code.tree.rename.as_ref().map(|r| r.path.clone()), Some(sp(&src, "util/b.rs")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_an_open_modified_file_asks_first() {
    let (dir, src) = project("tree_delete");
    let mut editor = project_editor(&src);
    let c = sp(&src, "util/c.rs");
    let mut hands = open_and_focus(&mut editor, &c);
    hands.type_text(&mut editor, "// unsaved\n");
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "c.rs");
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "Delete…");
    hands.frame(&mut editor, vec![]);
    assert!(c.exists(), "nothing is deleted before the answer");
    let texts = hands.texts();
    assert!(texts.iter().any(|t| t.starts_with("Unsaved changes in c.rs")), "the dialog warns about unsaved edits: {texts:?}");
    assert!(texts.iter().any(|t| t == "c.rs"), "and names the file");
    let delete = *spots(&hands, "Delete").last().expect("the Delete button");
    hands.click(&mut editor, delete);
    hands.frame(&mut editor, vec![]);
    assert!(!c.exists(), "deleted after confirming");
    assert!(!editor.code.documents.iter().any(|d| d.path == c), "its tab is closed");
    assert!(!editor.code.files.iter().any(|(_, p)| *p == c), "the list is refreshed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_rust_script_from_the_folder_menu_lands_in_mod_rs() {
    let (dir, src) = project("tree_new");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &sp(&src, "main.rs"));
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "scripts");
    right_click(&mut hands, &mut editor, p);
    menu(&mut hands, &mut editor, "New File");
    hands.wait(&mut editor, 0.1);
    menu(&mut hands, &mut editor, "Rust Script…");
    assert!(editor.dialog.is_some(), "the New Script dialog opened");
    hands.type_text(&mut editor, "Jumper");
    let create = hands.find_text_pos("Create and Build").expect("the create button");
    hands.click(&mut editor, create);
    hands.wait(&mut editor, 0.1);
    let file = sp(&src, "scripts/jumper.rs");
    assert!(file.exists(), "the script file is created");
    let mod_rs = std::fs::read_to_string(sp(&src, "scripts/mod.rs")).unwrap();
    assert!(mod_rs.contains("jumper") && mod_rs.contains("mover"), "mod.rs lists it: {mod_rs}");
    assert!(editor.code.files.iter().any(|(_, p)| *p == file), "and the list shows it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_paste_and_drag_move_between_folders() {
    let (dir, src) = project("tree_move");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &sp(&src, "main.rs"));
    pause(&mut hands, &mut editor);
    // Ctrl+C на файле, Ctrl+V в пустой каталог.
    let p = tree_row(&hands, "b.rs");
    hands.click(&mut editor, p);
    let tree = egui::Id::new("code_file_tree");
    hands.ctx.memory_mut(|m| m.request_focus(tree));
    hands.frame(&mut editor, vec![Event::Copy]);
    pause(&mut hands, &mut editor);
    let p = tree_row(&hands, "empty");
    hands.click(&mut editor, p);
    hands.ctx.memory_mut(|m| m.request_focus(tree));
    hands.frame(&mut editor, vec![Event::Paste(String::new())]);
    hands.frame(&mut editor, vec![]);
    assert!(sp(&src, "empty/b.rs").exists() && sp(&src, "util/b.rs").exists(), "copied");
    // Перетащить `a.rs` на каталог `scripts` — перенос (и `mod.rs` переписан).
    pause(&mut hands, &mut editor);
    let from = tree_row(&hands, "a.rs");
    let to = tree_row(&hands, "scripts");
    hands.frame(&mut editor, vec![Event::PointerMoved(from)]);
    hands.frame(&mut editor, vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    for k in 1..=6 {
        hands.frame(&mut editor, vec![Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
    }
    hands.frame(&mut editor, vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    hands.frame(&mut editor, vec![]);
    assert!(sp(&src, "scripts/a.rs").exists() && !sp(&src, "util/a.rs").exists(), "moved by drag");
    let mod_rs = std::fs::read_to_string(sp(&src, "scripts/mod.rs")).unwrap();
    assert!(mod_rs.contains("mod a;") || mod_rs.contains("pub mod a"), "mod.rs follows: {mod_rs}");
    // Вне проекта — отказ.
    let outside = dir.join("outside.txt");
    std::fs::write(&outside, "x").unwrap();
    crate::code_tree::run(&mut editor, &hands.ctx.clone(), crate::code_tree::TreeCmd::Delete(vec![outside.clone()]));
    crate::code_tree::run(&mut editor, &hands.ctx.clone(), crate::code_tree::TreeCmd::ConfirmDelete);
    assert!(outside.exists(), "files outside the code folders are never touched");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tab_menu_close_others_and_ctrl_w() {
    let (dir, src) = project("tabs");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &sp(&src, "util/a.rs"));
    for name in ["util/b.rs", "util/c.rs"] {
        editor.apply(Action::OpenCode(src.join(name)));
        hands.frame(&mut editor, vec![]);
    }
    assert_eq!(editor.code.documents.len(), 3);
    let tab = hands.ctx.read_response(egui::Id::new(("code_tab", 1))).expect("tab").rect.center();
    right_click(&mut hands, &mut editor, tab);
    menu(&mut hands, &mut editor, "Close Others");
    assert_eq!(editor.code.documents.len(), 1);
    assert!(editor.code_document().unwrap().0.ends_with("b.rs"));
    let id = editor.code.documents[0].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::W, Modifiers::COMMAND);
    assert!(editor.code.documents.is_empty(), "Ctrl+W closes the tab");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Снимки меню и дерева (GPU): `W22_SHOTS3=<каталог> cargo test -p engine-editor --lib
/// code_menu_screenshots -- --ignored`. Английский 100 % и русский 150 %.
#[test]
#[ignore = "GPU: снимки для просмотра"]
fn code_menu_screenshots() {
    let Ok(out) = std::env::var("W22_SHOTS3") else { return };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    for (language, scale, tag) in [(engine_i18n::Language::English, 1.0_f32, "en_100"), (engine_i18n::Language::Russian, 1.5, "ru_150")] {
        let (dir, src) = project(&format!("menu_shots_{tag}"));
        let mut editor = project_editor(&src);
        let mut prefs = editor.preferences().clone();
        prefs.ui_scale = scale;
        prefs.language = Some(language);
        editor.set_preferences(prefs);
        engine_i18n::set(language);
        editor.apply(Action::OpenCode(sp(&src, "scripts/mover.rs")));
        let (w, h) = ((1440.0 * scale) as u32, (860.0 * scale) as u32);
        let button = |pos, button, pressed| Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE };
        // 1. Меню текста.
        let shot = crate::gpu::capture_with_input(&mut editor, w, h, 40, |f, editor| {
            let Some(_) = editor.code.geometry else { return vec![] };
            let p = at(editor, 2, 30);
            match f {
                3 => vec![Event::PointerMoved(p)],
                4 => vec![button(p, PointerButton::Primary, true)],
                5 => vec![button(p, PointerButton::Primary, false)],
                30 => vec![button(p, PointerButton::Secondary, true)],
                31 => vec![button(p, PointerButton::Secondary, false)],
                _ => vec![],
            }
        })
        .unwrap();
        shot.save(out.join(format!("editor_menu_{tag}.png"))).unwrap();
        // 2. Меню каталога с подменю New File.
        let shot = crate::gpu::capture_with_input(&mut editor, w, h, 40, |f, editor| {
            let Some(row) = editor.code.tree.rows_at.iter().find(|(p, _)| p.ends_with("scripts")).map(|(_, r)| r.center()) else { return vec![] };
            match f {
                3 => vec![Event::PointerMoved(row)],
                4 => vec![button(row, PointerButton::Secondary, true)],
                5 => vec![button(row, PointerButton::Secondary, false)],
                10..=30 => vec![Event::PointerMoved(row + egui::vec2(60.0, 14.0))],
                _ => vec![],
            }
        })
        .unwrap();
        shot.save(out.join(format!("file_menu_new_{tag}.png"))).unwrap();
        // 3. Переименование на месте.
        editor.code.tree_commands.push(crate::code_tree::TreeCmd::BeginRename(sp(&src, "util/a.rs")));
        crate::gpu::capture(&mut editor, w, h, 6).unwrap().save(out.join(format!("inline_rename_{tag}.png"))).unwrap();
        editor.code.tree.rename = None;
        // 4. Подтверждение удаления.
        editor.code.tree.confirm_delete = Some(vec![sp(&src, "util/b.rs"), sp(&src, "scripts/mover.rs")]);
        crate::gpu::capture(&mut editor, w, h, 6).unwrap().save(out.join(format!("delete_confirm_{tag}.png"))).unwrap();
        editor.code.tree.confirm_delete = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
    engine_i18n::set(engine_i18n::Language::English);
}

// ----------------------------------------------------------------------------
// Закрытие вкладок с несохранёнными правками: вопрос «Save changes?» (`code_close.rs`).
// ----------------------------------------------------------------------------

/// Открыть файл, щёлкнуть в конец текста и дописать `added` (вкладка становится изменённой).
fn edit_file(hands: &mut Hands, editor: &mut Editor, path: &Path, added: &str) {
    editor.apply(Action::OpenCode(path.to_path_buf()));
    hands.frame(editor, vec![]);
    hands.frame(editor, vec![]);
    let id = editor.code.documents[editor.code.active].text_id();
    let rect = hands.ctx.read_response(id).expect("the code text field is shown").rect;
    hands.click(editor, egui::pos2(rect.left() + 200.0, rect.bottom() - 4.0));
    assert!(hands.ctx.memory(|m| m.has_focus(id)), "the text has keyboard focus");
    hands.type_text(editor, added);
    hands.frame(editor, vec![]);
}

fn tab_at(hands: &Hands, index: usize) -> egui::Rect {
    hands.ctx.read_response(egui::Id::new(("code_tab", index))).expect("a tab").rect
}

fn close_button_of(hands: &Hands, index: usize) -> Pos2 {
    hands.ctx.read_response(egui::Id::new(("code_tab_close", index))).expect("a close button").rect.center()
}

fn middle_click(hands: &mut Hands, editor: &mut Editor, pos: Pos2) {
    press(hands, editor, pos, PointerButton::Middle, Modifiers::NONE);
}

fn asks(editor: &Editor) -> bool {
    editor.code.close_ask.is_some()
}

/// Кнопка окна вопроса — самая верхняя надпись (под ней в панели инструментов есть свой «Save»).
fn dialog_button(hands: &Hands, label: &str) -> Pos2 {
    *spots(hands, label).last().unwrap_or_else(|| panic!("the `{label}` button: {:?}", hands.texts()))
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn closing_a_modified_tab_asks_through_the_button_ctrl_w_and_middle_click() {
    let (dir, src) = project("close_ask_paths");
    let mut editor = project_editor(&src);
    let a = sp(&src, "util/a.rs");
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    let edited = text(&editor);
    assert!(!asks(&editor), "typing asks nothing");

    // Крестик вкладки.
    let close = close_button_of(&hands, 0);
    hands.click(&mut editor, close);
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor), "the close button asks");
    assert_eq!(editor.code.documents.len(), 1, "the tab is still open");
    let texts = hands.texts();
    assert!(texts.iter().any(|t| t == "Save changes to “a.rs”?"), "{texts:?}");
    for label in ["Save", "Don't Save", "Cancel"] {
        assert!(!spots(&hands, label).is_empty(), "the `{label}` button: {texts:?}");
    }
    assert_eq!(bytes(&a), b"pub fn a() {}\n", "nothing is written before the answer");
    let cancel = dialog_button(&hands, "Cancel");
    hands.click(&mut editor, cancel);
    hands.wait(&mut editor, 0.1);
    assert!(!asks(&editor));

    // Ctrl+W.
    let id = editor.code.documents[0].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.wait(&mut editor, 0.1);
    hands.press(&mut editor, Key::W, Modifiers::COMMAND);
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor), "Ctrl+W asks");
    assert!(hands.texts().iter().any(|t| t == "Save changes to “a.rs”?"));
    hands.press(&mut editor, Key::Escape, Modifiers::NONE);
    hands.wait(&mut editor, 0.1);
    assert!(!asks(&editor), "Esc cancels");

    // Средняя кнопка.
    let tab = tab_at(&hands, 0).center();
    middle_click(&mut hands, &mut editor, tab);
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor), "a middle click asks");
    assert_eq!(editor.code.documents.len(), 1);
    assert_eq!(text(&editor), edited, "the text is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_writes_the_exact_text_with_the_original_line_endings_and_closes() {
    let (dir, src) = project("close_ask_save");
    let a = sp(&src, "util/a.rs");
    std::fs::write(&a, "\u{feff}pub fn a() {}\r\nlet x = 1;\r\n").unwrap();
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    let edited = text(&editor);
    assert!(!edited.contains('\r'), "the editor keeps `\\n` inside");
    let close = close_button_of(&hands, 0);
    hands.click(&mut editor, close);
    hands.wait(&mut editor, 0.1);
    let save = dialog_button(&hands, "Save");
    hands.click(&mut editor, save);
    hands.wait(&mut editor, 0.1);
    assert!(editor.code.documents.is_empty(), "the tab closed after saving");
    assert!(!asks(&editor));
    let mut expected = vec![0xEF, 0xBB, 0xBF];
    expected.extend_from_slice(edited.replace('\n', "\r\n").as_bytes());
    assert_eq!(bytes(&a), expected, "the exact text, CRLF and the BOM are kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enter_saves_and_a_failed_save_keeps_the_tab_open_with_the_error() {
    let (dir, src) = project("close_ask_enter");
    let a = sp(&src, "util/a.rs");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    let edited = text(&editor);
    // Запись невозможна: на месте файла — каталог.
    std::fs::remove_file(&a).unwrap();
    std::fs::create_dir(&a).unwrap();
    hands.press(&mut editor, Key::W, Modifiers::COMMAND);
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor));
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code.documents.len(), 1, "a failed save keeps the tab");
    assert!(asks(&editor), "and the question stays");
    assert_eq!(text(&editor), edited, "Enter did not type a new line");
    let texts = hands.texts();
    assert!(texts.iter().any(|t| t.contains("cannot save")), "the error is shown: {texts:?}");
    // Файл снова пишется — Enter сохраняет и закрывает.
    std::fs::remove_dir(&a).unwrap();
    hands.press(&mut editor, Key::Enter, Modifiers::NONE);
    hands.wait(&mut editor, 0.1);
    assert!(editor.code.documents.is_empty());
    assert_eq!(String::from_utf8(bytes(&a)).unwrap(), edited);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dont_save_closes_leaves_the_file_and_removes_the_recovery_copy() {
    let (dir, src) = project("close_ask_discard");
    let store = dir.join("recovery_store");
    let a = sp(&src, "util/a.rs");
    let mut editor = project_editor(&src);
    editor.code.recovery.dir = Some(store.clone());
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 3.5);
    assert!(crate::code_recovery::file_for(&store, &a).exists(), "the unsaved text is written aside");
    let close = close_button_of(&hands, 0);
    hands.click(&mut editor, close);
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor));
    assert!(crate::code_recovery::file_for(&store, &a).exists(), "asking does not touch the recovery copy");
    let discard = dialog_button(&hands, "Don't Save");
    hands.click(&mut editor, discard);
    hands.wait(&mut editor, 3.5);
    assert!(editor.code.documents.is_empty(), "the tab closed");
    assert_eq!(bytes(&a), b"pub fn a() {}\n", "the file on disk is untouched");
    assert!(!crate::code_recovery::file_for(&store, &a).exists(), "no recovery copy is left to be offered");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_and_esc_keep_the_tab_the_text_the_caret_and_the_undo_history() {
    let (dir, src) = project("close_ask_cancel");
    let store = dir.join("recovery_store");
    let a = sp(&src, "util/a.rs");
    let mut editor = project_editor(&src);
    editor.code.recovery.dir = Some(store.clone());
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// one\n");
    hands.wait(&mut editor, 0.1);
    hands.wait(&mut editor, 0.6);
    hands.type_text(&mut editor, "// two\n");
    hands.wait(&mut editor, 3.5);
    let (edited, caret) = (text(&editor), carets(&editor, &hands));
    for how in ["button", "esc"] {
        let close = close_button_of(&hands, 0);
        hands.click(&mut editor, close);
        hands.wait(&mut editor, 0.1);
        assert!(asks(&editor), "{how}");
        if how == "button" {
            let cancel = dialog_button(&hands, "Cancel");
            hands.click(&mut editor, cancel);
        } else {
            hands.press(&mut editor, Key::Escape, Modifiers::NONE);
        }
        hands.wait(&mut editor, 0.1);
        assert!(!asks(&editor), "{how}: the question is gone");
        assert_eq!(editor.code.documents.len(), 1, "{how}: the tab stays");
        assert_eq!(text(&editor), edited, "{how}: the text stays");
        assert_eq!(carets(&editor, &hands), caret, "{how}: the caret stays");
        assert_eq!(bytes(&a), b"pub fn a() {}\n", "{how}: nothing is written");
        assert!(crate::code_recovery::file_for(&store, &a).exists(), "{how}: the recovery copy stays");
    }
    // История Undo жива: Ctrl+Z отменяет последний набор.
    let id = editor.code.documents[0].text_id();
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.wait(&mut editor, 0.1);
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    assert!(text(&editor).len() < edited.len(), "Ctrl+Z still undoes after Cancel: {:?}", text(&editor));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn close_others_with_two_modified_tabs_asks_once_and_save_all_saves_both() {
    let (dir, src) = project("close_ask_others");
    let (a, b, c) = (sp(&src, "util/a.rs"), sp(&src, "util/b.rs"), sp(&src, "util/c.rs"));
    std::fs::write(&b, "pub fn b() {}\r\n").unwrap();
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// a\n");
    hands.wait(&mut editor, 0.1);
    let text_a = text(&editor);
    edit_file(&mut hands, &mut editor, &b, "// b\n");
    let text_b = text(&editor);
    editor.apply(Action::OpenCode(c.clone()));
    hands.wait(&mut editor, 0.6);
    assert_eq!(editor.code.documents.len(), 3);
    let tab = tab_at(&hands, 2).center();
    right_click(&mut hands, &mut editor, tab);
    menu(&mut hands, &mut editor, "Close Others");
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor), "one question");
    assert_eq!(editor.code.documents.len(), 3, "nothing closes before the answer");
    let texts = hands.texts();
    assert!(texts.iter().any(|t| t == "Save changes to 2 files?"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "a.rs") && texts.iter().any(|t| t == "b.rs"), "both files are listed: {texts:?}");
    let save_all = dialog_button(&hands, "Save All");
    hands.click(&mut editor, save_all);
    hands.wait(&mut editor, 0.1);
    assert!(!asks(&editor));
    assert_eq!(editor.code.documents.len(), 1, "only the tab the menu was opened on stays");
    assert!(editor.code_document().unwrap().0.ends_with("c.rs"));
    assert_eq!(String::from_utf8(bytes(&a)).unwrap(), text_a);
    assert_eq!(bytes(&b), text_b.replace('\n', "\r\n").as_bytes(), "b.rs keeps its CRLF");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn close_all_and_close_to_the_right_ask_dont_save_drops_them_and_pinned_tabs_stay() {
    let (dir, src) = project("close_ask_all");
    let (a, b, c) = (sp(&src, "util/a.rs"), sp(&src, "util/b.rs"), sp(&src, "util/c.rs"));
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// a\n");
    hands.wait(&mut editor, 0.1);
    edit_file(&mut hands, &mut editor, &b, "// b\n");
    editor.apply(Action::OpenCode(c.clone()));
    hands.wait(&mut editor, 0.6);
    // Закреплённая изменённая вкладка (a) не закрывается и ни о чём не спрашивает сама.
    editor.code.set_pinned(0, true);
    hands.wait(&mut editor, 0.1);
    let tab = tab_at(&hands, 2).center();
    right_click(&mut hands, &mut editor, tab);
    menu(&mut hands, &mut editor, "Close All");
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor));
    assert!(hands.texts().iter().any(|t| t == "Save changes to “b.rs”?"), "only the modified, unpinned b.rs: {:?}", hands.texts());
    let discard = dialog_button(&hands, "Don't Save");
    hands.click(&mut editor, discard);
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code.documents.len(), 1, "the pinned tab stays");
    assert!(editor.code.documents[0].path == a && editor.code.documents[0].dirty());
    assert_eq!(bytes(&b), b"pub fn b() {}\n");

    // Close to the Right: справа изменённая вкладка.
    editor.code.set_pinned(0, false);
    edit_file(&mut hands, &mut editor, &b, "// b2\n");
    hands.wait(&mut editor, 0.6);
    let tab = tab_at(&hands, 0).center();
    right_click(&mut hands, &mut editor, tab);
    menu(&mut hands, &mut editor, "Close to the Right");
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor), "to the Right asks too");
    let cancel = dialog_button(&hands, "Cancel");
    hands.click(&mut editor, cancel);
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code.documents.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unmodified_tabs_never_ask() {
    let (dir, src) = project("close_ask_clean");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &sp(&src, "util/a.rs"));
    for name in ["util/b.rs", "util/c.rs"] {
        editor.apply(Action::OpenCode(sp(&src, name)));
        hands.wait(&mut editor, 0.3);
    }
    assert_eq!(editor.code.documents.len(), 3);
    let close = close_button_of(&hands, 2);
    hands.click(&mut editor, close);
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code.documents.len(), 2, "the close button closes at once");
    let tab = tab_at(&hands, 1).center();
    middle_click(&mut hands, &mut editor, tab);
    hands.wait(&mut editor, 0.1);
    assert_eq!(editor.code.documents.len(), 1, "so does a middle click");
    let tab = tab_at(&hands, 0).center();
    right_click(&mut hands, &mut editor, tab);
    menu(&mut hands, &mut editor, "Close All");
    hands.wait(&mut editor, 0.1);
    assert!(editor.code.documents.is_empty() && !asks(&editor), "Close All with nothing modified");
    // Файл, набранный и возвращённый к исходному тексту, — не изменён.
    let a = sp(&src, "util/a.rs");
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "x");
    hands.press(&mut editor, Key::Backspace, Modifiers::NONE);
    hands.wait(&mut editor, 0.1);
    assert!(!editor.code_document().unwrap().2, "back to the saved text");
    hands.press(&mut editor, Key::W, Modifiers::COMMAND);
    hands.wait(&mut editor, 0.1);
    assert!(editor.code.documents.is_empty() && !asks(&editor), "Ctrl+W");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quit_asks_about_modified_code_and_cancel_keeps_the_editor_open() {
    let (dir, src) = project("close_ask_quit");
    let store = dir.join("recovery_store");
    let a = sp(&src, "util/a.rs");
    let mut editor = project_editor(&src);
    editor.code.recovery.dir = Some(store.clone());
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 3.5);
    let edited = text(&editor);

    // Cancel: редактор остаётся, всё как было.
    editor.apply(Action::Quit);
    hands.wait(&mut editor, 0.1);
    assert!(asks(&editor) && !editor.should_quit());
    assert!(hands.texts().iter().any(|t| t == "Save changes to “a.rs”?"), "{:?}", hands.texts());
    let cancel = dialog_button(&hands, "Cancel");
    hands.click(&mut editor, cancel);
    hands.wait(&mut editor, 0.1);
    assert!(!editor.should_quit() && !asks(&editor));
    assert_eq!(text(&editor), edited);
    assert!(crate::code_recovery::file_for(&store, &a).exists());

    // Don't Save: выходим, файл не тронут, копии восстановления нет.
    editor.apply(Action::Quit);
    hands.wait(&mut editor, 0.1);
    let discard = dialog_button(&hands, "Don't Save");
    hands.click(&mut editor, discard);
    hands.wait(&mut editor, 0.1);
    assert!(editor.should_quit());
    assert_eq!(bytes(&a), b"pub fn a() {}\n");
    assert!(!crate::code_recovery::file_for(&store, &a).exists());

    // Save: записать и выйти.
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    let edited = text(&editor);
    editor.apply(Action::Quit);
    hands.wait(&mut editor, 0.1);
    let save = dialog_button(&hands, "Save");
    hands.click(&mut editor, save);
    hands.wait(&mut editor, 0.1);
    assert!(editor.should_quit());
    assert_eq!(String::from_utf8(bytes(&a)).unwrap(), edited);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quit_with_an_unsaved_scene_and_modified_code_asks_one_combined_question() {
    let (dir, src) = project("close_ask_quit_scene");
    let a = sp(&src, "util/a.rs");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    editor.apply(Action::CreateEmpty { parent: None });
    editor.apply(Action::Quit);
    hands.wait(&mut editor, 0.1);
    assert!(editor.has_dialog() && !asks(&editor), "one question, the scene one");
    let texts = hands.texts();
    assert!(texts.iter().any(|t| t == "Unsaved code: a.rs."), "it lists the code too: {texts:?}");
    let cancel = dialog_button(&hands, "Cancel");
    hands.click(&mut editor, cancel);
    hands.wait(&mut editor, 0.1);
    assert!(!editor.should_quit() && editor.code.documents[0].dirty());
    // Don't Save — и сцена, и код.
    editor.apply(Action::Quit);
    hands.wait(&mut editor, 0.1);
    let discard = dialog_button(&hands, "Don't Save");
    hands.click(&mut editor, discard);
    hands.wait(&mut editor, 0.1);
    assert!(editor.should_quit());
    assert_eq!(bytes(&a), b"pub fn a() {}\n");
    // Save — код записан раньше выхода.
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    let edited = text(&editor);
    editor.apply(Action::CreateEmpty { parent: None });
    editor.apply(Action::Quit);
    hands.wait(&mut editor, 0.1);
    let save = dialog_button(&hands, "Save");
    hands.click(&mut editor, save);
    hands.wait(&mut editor, 0.1);
    assert_eq!(String::from_utf8(bytes(&a)).unwrap(), edited, "the code was saved by the combined question");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn play_other_editor_tabs_and_focus_loss_ask_nothing() {
    let (dir, src) = project("close_ask_quiet");
    let a = sp(&src, "util/a.rs");
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    for tab in [crate::state::ViewTab::Scene, crate::state::ViewTab::Game, crate::state::ViewTab::Code] {
        editor.view_tab = tab;
        hands.wait(&mut editor, 0.1);
        assert!(!asks(&editor) && !editor.has_dialog(), "{tab:?}");
    }
    editor.apply(Action::Play);
    hands.wait(&mut editor, 0.2);
    editor.apply(Action::Stop);
    hands.wait(&mut editor, 0.2);
    hands.frame(&mut editor, vec![Event::WindowFocused(false)]);
    hands.frame(&mut editor, vec![Event::WindowFocused(true)]);
    assert!(!asks(&editor) && !editor.has_dialog(), "Play and focus changes ask nothing");
    assert!(editor.code.documents[0].dirty() && !editor.should_quit());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_saves_modified_code_before_building() {
    let (dir, src) = project("close_ask_build");
    let a = sp(&src, "util/a.rs");
    std::fs::write(&a, "pub fn a() {}\r\n").unwrap();
    let mut editor = project_editor(&src);
    let mut hands = open_and_focus(&mut editor, &a);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 0.1);
    let edited = text(&editor);
    editor.apply(Action::Build);
    assert_eq!(bytes(&a), edited.replace('\n', "\r\n").as_bytes(), "Build writes the text first, CRLF kept");
    assert!(!editor.code.has_unsaved() && !asks(&editor), "and asks nothing");
    let _ = std::fs::remove_dir_all(&dir);
}
