//! Свой виджет кода настоящим вводом egui: несколько курсоров (Alt+щелчок,
//! Ctrl+Alt+↓), выделение столбцом (Shift+Alt+мышь, Ctrl+Shift+Alt+стрелки), следующее и
//! все вхождения, свёртка щелчком и сочетанием, поиск внутри свёрнутого, ввод IME,
//! буфер обмена с несколькими курсорами, один шаг Undo на правку всеми курсорами.

use egui::{Event, Key, Modifiers, Pos2};

use crate::code_tests::{editor_with, open_and_focus, temp_dir, Hands};
use crate::state::Editor;

fn text(editor: &Editor) -> String {
    editor.code_document().unwrap().1.to_string()
}

fn carets(editor: &Editor, hands: &Hands) -> Vec<(usize, usize)> {
    let id = editor.code.documents[editor.code.active].text_id();
    crate::code_keys::carets(&hands.ctx, id).sels.iter().map(|s| (s.anchor, s.caret)).collect()
}

/// Экранная точка символа `column` строки `line` (с нуля) — по геометрии кадра.
fn at(editor: &Editor, line: usize, column: usize) -> Pos2 {
    let (origin, cw, lh) = editor.code.geometry.expect("the code text is laid out");
    egui::pos2(origin.x + (column as f32 + 0.1) * cw, origin.y + (line as f32 + 0.5) * lh)
}

fn click_with(hands: &mut Hands, editor: &mut Editor, pos: Pos2, modifiers: Modifiers) {
    let button = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
    hands.frame(editor, vec![Event::PointerMoved(pos)]);
    hands.frame(editor, vec![button(true)]);
    hands.frame(editor, vec![button(false)]);
    hands.frame(editor, vec![]);
}

fn set(editor: &Editor, hands: &Hands, a: usize, c: usize) {
    let id = editor.code.documents[editor.code.active].text_id();
    crate::code_keys::set_selection(&hands.ctx, id, &text(editor), a, c);
}

#[test]
fn alt_click_and_ctrl_alt_down_add_carets_and_typing_goes_to_each_with_one_undo() {
    let dir = temp_dir("multi");
    let path = dir.join("m.rs");
    std::fs::write(&path, "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    set(&editor, &hands, 4, 4);
    // Пауза: иначе щелчок сразу после щелчка открытия — двойной.
    hands.wait(&mut editor, 0.6);
    // Alt+щелчок перед `b` — второй курсор.
    let pos = at(&editor, 1, 4);
    click_with(&mut hands, &mut editor, pos, Modifiers::ALT);
    assert_eq!(carets(&editor, &hands), vec![(4, 4), (15, 15)]);
    // Ctrl+Alt+↓ — курсор строкой ниже последнего.
    hands.press(&mut editor, Key::ArrowDown, Modifiers::COMMAND.plus(Modifiers::ALT));
    assert_eq!(carets(&editor, &hands), vec![(4, 4), (15, 15), (26, 26)]);
    hands.type_text(&mut editor, "my_");
    assert_eq!(text(&editor), "let my_a = 1;\nlet my_b = 2;\nlet my_c = 3;\nlet d = 4;\n");
    assert_eq!(carets(&editor, &hands), vec![(7, 7), (21, 21), (35, 35)]);
    // Ctrl+/ комментирует строку каждого курсора, Esc — снова один курсор.
    hands.press(&mut editor, Key::Slash, Modifiers::COMMAND);
    assert_eq!(text(&editor), "// let my_a = 1;\n// let my_b = 2;\n// let my_c = 3;\nlet d = 4;\n");
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    hands.press(&mut editor, Key::Z, Modifiers::COMMAND);
    assert_eq!(text(&editor), "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\n", "typing at three carets is one undo step");
    // Первый Esc закрывает список подсказок (как в VS), второй — оставляет один курсор.
    if editor.code.completion.is_some() {
        hands.press(&mut editor, Key::Escape, Modifiers::NONE);
        assert!(editor.code.completion.is_none());
        assert_eq!(carets(&editor, &hands).len(), 3);
    }
    hands.press(&mut editor, Key::Escape, Modifiers::NONE);
    assert_eq!(carets(&editor, &hands).len(), 1, "Esc leaves one caret");
    assert!(hands.ctx.memory(|m| m.has_focus(editor.code.documents[0].text_id())), "and keeps the focus");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn column_selection_by_mouse_and_keyboard_then_typing_replaces_each_row() {
    let dir = temp_dir("box");
    let path = dir.join("b.rs");
    std::fs::write(&path, "x.speed = 1;\ny.speed = 2;\nz.speed = 3;\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    set(&editor, &hands, 2, 2);
    hands.frame(&mut editor, vec![]);
    // Shift+Alt+перетаскивание от `speed` первой строки до конца `speed` третьей.
    let from = at(&editor, 0, 2);
    let to = at(&editor, 2, 7);
    let mods = Modifiers::SHIFT.plus(Modifiers::ALT);
    hands.frame(&mut editor, vec![Event::PointerMoved(from)]);
    hands.frame(&mut editor, vec![Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: mods }]);
    for k in 1..=4 {
        let p = from + (to - from) * (k as f32 / 4.0);
        hands.frame(&mut editor, vec![Event::PointerMoved(p)]);
    }
    hands.frame(&mut editor, vec![Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: mods }]);
    let ranges: Vec<String> = carets(&editor, &hands).iter().map(|(a, c)| text(&editor)[*a.min(c)..*a.max(c)].to_string()).collect();
    assert_eq!(ranges, vec!["speed", "speed", "speed"], "a box over three rows");
    hands.type_text(&mut editor, "pace");
    assert_eq!(text(&editor), "x.pace = 1;\ny.pace = 2;\nz.pace = 3;\n");
    // С клавиатуры: Ctrl+Shift+Alt+↓ дважды и → дважды — столбец из двух символов.
    set(&editor, &hands, 0, 0);
    hands.frame(&mut editor, vec![]);
    let keys = Modifiers::COMMAND.plus(Modifiers::SHIFT).plus(Modifiers::ALT);
    hands.press(&mut editor, Key::ArrowDown, keys);
    hands.press(&mut editor, Key::ArrowDown, keys);
    for _ in 0..2 {
        hands.press(&mut editor, Key::ArrowRight, keys);
    }
    let ranges: Vec<String> = carets(&editor, &hands).iter().map(|(a, c)| text(&editor)[*a.min(c)..*a.max(c)].to_string()).collect();
    assert_eq!(ranges, vec!["x.", "y.", "z."]);
    hands.press(&mut editor, Key::Delete, Modifiers::NONE);
    assert_eq!(text(&editor), "pace = 1;\npace = 2;\npace = 3;\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn next_occurrence_and_select_all_occurrences_rename_in_place() {
    let dir = temp_dir("occ");
    let path = dir.join("o.rs");
    std::fs::write(&path, "let speed = 1;\nlet y = speed + speedy;\nspeed = 2;\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    set(&editor, &hands, 5, 5);
    hands.frame(&mut editor, vec![]);
    let next = Modifiers::SHIFT.plus(Modifiers::ALT);
    hands.press(&mut editor, Key::Period, next);
    assert_eq!(carets(&editor, &hands), vec![(4, 9)], "the first press selects the word");
    hands.press(&mut editor, Key::Period, next);
    assert_eq!(carets(&editor, &hands), vec![(4, 9), (23, 28)], "the next whole-word occurrence (not `speedy`)");
    hands.press(&mut editor, Key::Escape, Modifiers::NONE);
    set(&editor, &hands, 5, 5);
    hands.frame(&mut editor, vec![]);
    // Shift+Alt+; — все вхождения; набор переименовывает их разом.
    hands.press(&mut editor, Key::Semicolon, next);
    assert_eq!(carets(&editor, &hands).len(), 3);
    hands.type_text(&mut editor, "pace");
    assert_eq!(text(&editor), "let pace = 1;\nlet y = pace + speedy;\npace = 2;\n");
    // Ctrl+Shift+L — то же.
    set(&editor, &hands, 5, 5);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::L, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    assert_eq!(carets(&editor, &hands).len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clipboard_with_several_carets_copies_lines_and_pastes_one_per_caret() {
    let dir = temp_dir("clip");
    let path = dir.join("c.rs");
    std::fs::write(&path, "aa\nbb\ncc\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    set(&editor, &hands, 0, 2);
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::ArrowDown, Modifiers::COMMAND.plus(Modifiers::ALT));
    hands.press(&mut editor, Key::ArrowDown, Modifiers::COMMAND.plus(Modifiers::ALT));
    hands.press(&mut editor, Key::End, Modifiers::NONE);
    hands.press(&mut editor, Key::Home, Modifiers::SHIFT);
    assert_eq!(carets(&editor, &hands).len(), 3);
    hands.frame(&mut editor, vec![Event::Copy]);
    assert_eq!(hands.copied, "aa\nbb\ncc", "one line per caret");
    // Вставка трёх строк тремя курсорами — по строке каждому.
    hands.frame(&mut editor, vec![Event::Paste("1\n2\n3".into())]);
    assert_eq!(text(&editor), "1\n2\n3\n");
    // Без выделения Ctrl+X вырезает строку целиком, как в Visual Studio.
    hands.press(&mut editor, Key::Escape, Modifiers::NONE);
    set(&editor, &hands, 2, 2);
    hands.frame(&mut editor, vec![]);
    hands.frame(&mut editor, vec![Event::Cut]);
    assert_eq!(text(&editor), "1\n3\n");
    assert_eq!(hands.copied, "2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ime_commits_cyrillic_text_and_shows_the_composition_first() {
    let dir = temp_dir("ime");
    let path = dir.join("i.rs");
    std::fs::write(&path, "// \n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    set(&editor, &hands, 3, 3);
    hands.frame(&mut editor, vec![]);
    hands.frame(&mut editor, vec![Event::Ime(egui::ImeEvent::Enabled), Event::Ime(egui::ImeEvent::Preedit("при".into()))]);
    assert_eq!(text(&editor), "// \n", "composition is not in the text yet");
    assert!(hands.texts().iter().any(|t| t == "при"), "but it is drawn at the caret");
    hands.frame(&mut editor, vec![Event::Ime(egui::ImeEvent::Commit("привет".into()))]);
    assert_eq!(text(&editor), "// привет\n");
    assert_eq!(carets(&editor, &hands), vec![(15, 15)]);
    // Обычный набор кириллицы и Backspace по символу (не по байту).
    hands.type_text(&mut editor, "ж");
    hands.press(&mut editor, Key::Backspace, Modifiers::NONE);
    hands.press(&mut editor, Key::ArrowLeft, Modifiers::COMMAND);
    assert_eq!(text(&editor), "// привет\n");
    assert_eq!(carets(&editor, &hands), vec![(3, 3)], "Ctrl+Left goes to the start of the word");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn folding_by_gutter_click_and_chords_and_find_unfolds() {
    let dir = temp_dir("fold");
    let path = dir.join("f.rs");
    let body: String = (0..10).map(|i| format!("    step({i});\n")).collect();
    std::fs::write(&path, format!("fn a() {{\n{body}}}\nfn b() {{\n    hidden_target();\n}}\nfn c() {{}}\n")).unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &path);
    set(&editor, &hands, 0, 0);
    hands.frame(&mut editor, vec![]);
    let id = editor.code.documents[0].text_id();
    let hidden = |hands: &Hands| crate::code_widget::with_state(&hands.ctx, id, |st| st.hidden());
    // Щелчок по стрелке в колонке номеров строки 1.
    let (origin, _, lh) = editor.code.geometry.unwrap();
    let arrow = egui::pos2(origin.x - crate::theme::SP_6 - 7.0, origin.y + lh * 0.5);
    click_with(&mut hands, &mut editor, arrow, Modifiers::NONE);
    assert_eq!(hidden(&hands), vec![1..12], "fn a body is folded");
    assert!(hands.texts().iter().any(|t| t == "… }"), "the folded block shows `… }}`: {:?}", hands.texts());
    // Строки ниже свёртки поднялись: «fn b» — сразу под «fn a».
    let b_line = hands.find_text_pos("13").expect("line number 13 is shown");
    assert!((b_line.y - (origin.y + lh * 1.5)).abs() < 2.0, "line 13 right below line 1");
    // Ctrl+M Ctrl+M разворачивает.
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.press(&mut editor, Key::M, Modifiers::COMMAND);
    hands.press(&mut editor, Key::M, Modifiers::COMMAND);
    assert!(hidden(&hands).is_empty());
    // Ctrl+M Ctrl+O — свернуть определения (fn a и fn b), Ctrl+M Ctrl+L — развернуть всё.
    hands.press(&mut editor, Key::M, Modifiers::COMMAND);
    hands.press(&mut editor, Key::O, Modifiers::COMMAND);
    assert_eq!(hidden(&hands), vec![1..12, 13..15]);
    // Поиск находит текст в свёрнутом — блок разворачивается.
    hands.press(&mut editor, Key::F, Modifiers::COMMAND);
    hands.type_text(&mut editor, "hidden_target");
    hands.frame(&mut editor, vec![]);
    assert_eq!(hidden(&hands), vec![1..12], "fn b unfolded for the match");
    hands.ctx.memory_mut(|m| m.request_focus(id));
    hands.frame(&mut editor, vec![]);
    hands.press(&mut editor, Key::M, Modifiers::COMMAND);
    hands.press(&mut editor, Key::L, Modifiers::COMMAND);
    assert!(hidden(&hands).is_empty(), "toggle all expands everything");
    // Свёрнутое живёт при переключении вкладок.
    hands.press(&mut editor, Key::M, Modifiers::COMMAND);
    hands.press(&mut editor, Key::L, Modifiers::COMMAND);
    let folded = hidden(&hands);
    assert!(!folded.is_empty());
    let other = dir.join("other.rs");
    std::fs::write(&other, "fn z() {}\n").unwrap();
    editor.apply(crate::state::Action::OpenCode(other));
    hands.frame(&mut editor, vec![]);
    editor.apply(crate::state::Action::OpenCode(path.clone()));
    hands.frame(&mut editor, vec![]);
    assert_eq!(hidden(&hands), folded);
    let _ = std::fs::remove_dir_all(&dir);
}

fn key(key: Key, modifiers: Modifiers, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn press_at(pos: Pos2, modifiers: Modifiers, pressed: bool) -> Event {
    Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
}

/// Снимки своего виджета без окна (GPU): `W22_SHOTS=<каталог> cargo test -p engine-editor
/// --lib code_widget_screenshots -- --ignored`. Несколько курсоров, столбец, свёрнутые блоки
/// с ошибкой; английский 100 % и русский 150 %.
#[test]
#[ignore = "GPU: снимки для просмотра"]
fn code_widget_screenshots() {
    let Ok(out) = std::env::var("W22_SHOTS") else { return };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let source = "use engine::prelude::*;\n\ncomponent! {\n    pub struct Mover {\n        pub speed: f32 = 4.0,\n        pub jump: f32 = 6.0,\n        pub dash: f32 = 2.0,\n    }\n}\n\n// region Helpers\nfn clamp_speed(v: f32) -> f32 {\n    v.min(10.0)\n}\n// endregion\n\nimpl Component for Mover {\n    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {\n        let x = self.speed * dt\n        let y = self.jump * dt;\n        let z = self.dash * dt;\n        ctx.log(\"моё сообщение\");\n    }\n}\n";
    for (language, scale, tag) in [(engine_i18n::Language::English, 1.0_f32, "en_100"), (engine_i18n::Language::Russian, 1.5, "ru_150")] {
        let dir = temp_dir(&format!("w22_{tag}"));
        let path = dir.join("mover.rs");
        std::fs::write(&path, source).unwrap();
        let mut editor = editor_with(&dir);
        let mut prefs = editor.preferences().clone();
        prefs.ui_scale = scale;
        prefs.language = Some(language);
        editor.set_preferences(prefs);
        engine_i18n::set(language);
        editor.apply(crate::state::Action::OpenCode(path.clone()));
        let (w, h) = ((1440.0 * scale) as u32, (860.0 * scale) as u32);
        // 1. Несколько курсоров: щелчок в `speed` строки 19, Alt+щелчок в 20, Ctrl+Alt+↓, набор.
        let shot = crate::gpu::capture_with_input(&mut editor, w, h, 90, |f, editor| {
            let Some(_) = editor.code.geometry else { return vec![] };
            let p = |l, c| at(editor, l, c);
            match f {
                3 => vec![Event::PointerMoved(p(18, 21))],
                4 => vec![press_at(p(18, 21), Modifiers::NONE, true)],
                5 => vec![press_at(p(18, 21), Modifiers::NONE, false)],
                40 => vec![Event::PointerMoved(p(19, 21))],
                41 => vec![press_at(p(19, 21), Modifiers::ALT, true)],
                42 => vec![press_at(p(19, 21), Modifiers::ALT, false)],
                60 => vec![key(Key::ArrowDown, Modifiers::COMMAND.plus(Modifiers::ALT), true)],
                61 => vec![key(Key::ArrowDown, Modifiers::COMMAND.plus(Modifiers::ALT), false)],
                62 => vec![key(Key::ArrowRight, Modifiers::SHIFT, true), key(Key::ArrowRight, Modifiers::SHIFT, true), key(Key::ArrowRight, Modifiers::SHIFT, true)],
                _ => vec![],
            }
        })
        .unwrap();
        shot.save(out.join(format!("multi_caret_{tag}.png"))).unwrap();
        // 2. Столбец: Shift+Alt+перетаскивание по полям `component!`.
        editor.apply(crate::state::Action::OpenCode(path.clone()));
        let shot = crate::gpu::capture_with_input(&mut editor, w, h, 60, |f, editor| {
            let Some(_) = editor.code.geometry else { return vec![] };
            let mods = Modifiers::SHIFT.plus(Modifiers::ALT);
            let (a, b) = (at(editor, 4, 12), at(editor, 6, 17));
            match f {
                3 => vec![Event::PointerMoved(a)],
                4 => vec![press_at(a, Modifiers::NONE, true)],
                5 => vec![press_at(a, Modifiers::NONE, false)],
                40 => vec![press_at(a, mods, true)],
                41..=45 => vec![Event::PointerMoved(a + (b - a) * ((f - 40) as f32 / 5.0))],
                46 => vec![press_at(b, mods, false)],
                _ => vec![],
            }
        })
        .unwrap();
        shot.save(out.join(format!("column_select_{tag}.png"))).unwrap();
        // 3. Свёрнутые блоки и ошибка: Ctrl+M Ctrl+L по component!, наведение на «… }».
        editor.apply(crate::state::Action::OpenCode(path.clone()));
        let shot = crate::gpu::capture_with_input(&mut editor, w, h, 70, |f, editor| {
            let Some((origin, _, lh)) = editor.code.geometry else { return vec![] };
            let p = at(editor, 2, 3);
            // После свёртки component! (строки 4–9 скрыты) метка region — пятая видимая строка.
            let arrow = egui::pos2(origin.x - crate::theme::SP_6 - 7.0, origin.y + lh * 4.5);
            match f {
                3 => vec![Event::PointerMoved(p)],
                4 => vec![press_at(p, Modifiers::NONE, true)],
                5 => vec![press_at(p, Modifiers::NONE, false)],
                30 => vec![key(Key::M, Modifiers::COMMAND, true)],
                31 => vec![key(Key::M, Modifiers::COMMAND, true)],
                40 => vec![Event::PointerMoved(arrow)],
                41 => vec![press_at(arrow, Modifiers::NONE, true)],
                42 => vec![press_at(arrow, Modifiers::NONE, false)],
                50 => editor.code.first_diagnostic_at.map(Event::PointerMoved).into_iter().collect(),
                _ => vec![],
            }
        })
        .unwrap();
        shot.save(out.join(format!("folded_errors_{tag}.png"))).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
    engine_i18n::set(engine_i18n::Language::English);
}

/// Несохранённые правки пишутся в файл восстановления; после «сбоя» (новый редактор)
/// полоса предлагает их вернуть, Restore возвращает текст (Ctrl+Z — как на диске).
#[test]
fn unsaved_edits_survive_a_crash_through_the_recovery_file() {
    let dir = temp_dir("recovery");
    let store = dir.join("recovery_store");
    let path = dir.join("r.rs");
    std::fs::write(&path, "fn a() {}\n").unwrap();
    let mut editor = editor_with(&dir);
    editor.code.recovery.dir = Some(store.clone());
    let mut hands = open_and_focus(&mut editor, &path);
    hands.type_text(&mut editor, "// edit\n");
    hands.wait(&mut editor, 3.5);
    assert!(crate::code_recovery::file_for(&store, &path).exists(), "the unsaved text is written aside");
    let expected = text(&editor);
    drop(editor);

    // «Сбой»: новый редактор, файл на диске прежний.
    let mut editor = editor_with(&dir);
    editor.code.recovery.dir = Some(store.clone());
    editor.apply(crate::state::Action::OpenCode(path.clone()));
    let mut hands = Hands::new(&mut editor);
    hands.frame(&mut editor, vec![]);
    assert_eq!(text(&editor), "fn a() {}\n");
    let restore = hands.find_text_pos("Restore").expect("the restore bar is shown");
    hands.click(&mut editor, restore);
    assert_eq!(text(&editor), expected);
    assert!(editor.code_document().unwrap().2, "restored text is unsaved");
    // Сохранили — файл восстановления больше не нужен.
    editor.apply(crate::state::Action::SaveCode);
    hands.wait(&mut editor, 3.5);
    assert!(!crate::code_recovery::file_for(&store, &path).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pinned_tabs_stay_first_and_ignore_middle_click() {
    let dir = temp_dir("pin");
    let (a, b) = (dir.join("a.rs"), dir.join("b.rs"));
    std::fs::write(&a, "fn a() {}\n").unwrap();
    std::fs::write(&b, "fn b() {}\n").unwrap();
    let mut editor = editor_with(&dir);
    let mut hands = open_and_focus(&mut editor, &a);
    editor.apply(crate::state::Action::OpenCode(b.clone()));
    hands.frame(&mut editor, vec![]);
    editor.code.set_pinned(1, true);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code.documents[0].path, b, "the pinned tab moves first");
    assert_eq!(editor.code_document().unwrap().0, b.as_path(), "and stays active");
    let tab = hands.ctx.read_response(egui::Id::new(("code_tab", 0))).expect("tab").rect;
    let button = |pressed| Event::PointerButton { pos: tab.center(), button: egui::PointerButton::Middle, pressed, modifiers: Modifiers::NONE };
    hands.frame(&mut editor, vec![Event::PointerMoved(tab.center())]);
    hands.frame(&mut editor, vec![button(true)]);
    hands.frame(&mut editor, vec![button(false)]);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.code.documents.len(), 2, "middle click does not close a pinned tab");
    let _ = std::fs::remove_dir_all(&dir);
}
