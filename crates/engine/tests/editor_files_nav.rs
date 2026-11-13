//! Окно Project как файловый менеджер, часть 1: история, путь, клавиатура, выделение,
//! избранное и поиск. Окно и GPU не нужны. Файлы — только во временных каталогах.
#![cfg(feature = "editor")]

mod files_support;

use engine::editor::{egui, Action, Editor};
use files_support::*;

// ----- навигация -----

#[test]
fn history_back_forward_and_up_work_with_keys_and_mouse_buttons() {
    let sandbox = Sandbox::new("history");
    sandbox.dir("one/two").dir("three").file("one/x.txt", "x");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    assert_eq!(editor.project_history(), (false, false));

    for folder in ["one", "one/two", "three"] {
        let row = hands.tree(folder);
        hands.click(&mut editor, row);
        assert_eq!(editor.project_folder(), folder);
    }
    assert_eq!(editor.project_history(), (true, false));
    hands.key(&mut editor, egui::Key::ArrowLeft, alt());
    assert_eq!(editor.project_folder(), "one/two", "Alt+Left goes back");
    hands.key(&mut editor, egui::Key::ArrowLeft, alt());
    assert_eq!(editor.project_folder(), "one");
    assert_eq!(editor.project_history(), (true, true));
    hands.key(&mut editor, egui::Key::ArrowRight, alt());
    assert_eq!(editor.project_folder(), "one/two", "Alt+Right goes forward");

    hands.key(&mut editor, egui::Key::ArrowUp, alt());
    assert_eq!(editor.project_folder(), "one", "Alt+Up goes to the parent");
    hands.press(&mut editor, egui::Key::Backspace);
    assert_eq!(editor.project_folder(), "", "Backspace goes up too");
    hands.press(&mut editor, egui::Key::Backspace);
    assert_eq!(editor.project_folder(), "", "there is nothing above the root");

    // Кнопки мыши 4 и 5.
    let pos = hands.pointer.unwrap();
    let extra = |button, pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE };
    hands.frame(&mut editor, vec![extra(egui::PointerButton::Extra1, true), extra(egui::PointerButton::Extra1, false)]);
    hands.frames(&mut editor, 2);
    assert_eq!(editor.project_folder(), "one", "mouse button 4 goes back");
    hands.frame(&mut editor, vec![extra(egui::PointerButton::Extra2, true), extra(egui::PointerButton::Extra2, false)]);
    hands.frames(&mut editor, 2);
    assert_eq!(editor.project_folder(), "", "mouse button 5 goes forward");
}

#[test]
fn history_skips_a_folder_that_was_deleted_meanwhile() {
    let sandbox = Sandbox::new("history_gone");
    sandbox.dir("keep").dir("gone");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    for folder in ["gone", "keep"] {
        let row = hands.tree(folder);
        hands.click(&mut editor, row);
    }
    std::fs::remove_dir_all(sandbox.root.join("gone")).unwrap();
    editor.apply(Action::RefreshAssets);
    hands.frames(&mut editor, 3);
    hands.key(&mut editor, egui::Key::ArrowLeft, alt());
    assert_eq!(editor.project_folder(), "", "the vanished folder is skipped");
}

#[test]
fn breadcrumbs_navigate_and_the_path_field_takes_a_typed_path() {
    let sandbox = Sandbox::new("crumbs");
    sandbox.dir("a/b/c").dir("other");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let row = hands.tree("a/b/c");
    hands.click(&mut editor, row);
    assert_eq!(editor.project_folder(), "a/b/c");
    let crumb = hands.center(Editor::project_crumb_id("a"));
    hands.click(&mut editor, crumb);
    assert_eq!(editor.project_folder(), "a", "a click on a crumb opens that folder");
    let root = hands.center(Editor::project_crumb_id(""));
    hands.click(&mut editor, root);
    assert_eq!(editor.project_folder(), "");

    // Щелчок в пустое место строки пути — поле для ввода.
    let bar = hands.ctx.read_response(Editor::project_path_bar_id()).expect("the path bar is drawn").rect;
    hands.click(&mut editor, egui::pos2(bar.right() - 8.0, bar.center().y));
    assert!(hands.has(Editor::project_path_field_id()), "the path turns into an editable field");
    hands.type_text(&mut editor, "Assets/other");
    hands.press(&mut editor, egui::Key::Enter);
    assert_eq!(editor.project_folder(), "other", "Enter goes to the typed path");
    assert!(!hands.has(Editor::project_path_field_id()), "the field closes");

    // Неверный путь: остаёмся на месте, поле не закрывается.
    let bar = hands.ctx.read_response(Editor::project_path_bar_id()).unwrap().rect;
    hands.click(&mut editor, egui::pos2(bar.right() - 8.0, bar.center().y));
    hands.type_text(&mut editor, "Assets/nowhere");
    hands.press(&mut editor, egui::Key::Enter);
    assert_eq!(editor.project_folder(), "other");
    assert!(hands.has(Editor::project_path_field_id()), "a wrong path keeps the field open");
    hands.press(&mut editor, egui::Key::Escape);
    assert!(!hands.has(Editor::project_path_field_id()), "Esc closes the field");
}

// ----- клавиатура и выделение -----

fn thirty_files() -> Sandbox {
    let sandbox = Sandbox::new("grid");
    for n in 0..30 {
        sandbox.file(&format!("f{n:02}.txt"), "x");
    }
    sandbox
}

#[test]
fn arrow_keys_walk_tiles_in_two_dimensions() {
    let sandbox = thirty_files();
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let first = hands.asset(&editor, "f00.txt");
    hands.click(&mut editor, first);
    assert_eq!(editor.project_selection(), Some("f00.txt"));

    hands.press(&mut editor, egui::Key::ArrowRight);
    assert_eq!(editor.project_selection(), Some("f01.txt"));
    hands.press(&mut editor, egui::Key::ArrowLeft);
    assert_eq!(editor.project_selection(), Some("f00.txt"));

    // Сколько плиток в ряду — по раскладке.
    let top = editor.project_entry_rect("f00.txt").unwrap().top();
    let cols = (0..30)
        .filter(|n| editor.project_entry_rect(&format!("f{n:02}.txt")).is_some_and(|r| (r.top() - top).abs() < 1.0))
        .count();
    assert!((2..30).contains(&cols), "the fixture wraps into several rows: {cols} per row");
    hands.press(&mut editor, egui::Key::ArrowDown);
    assert_eq!(editor.project_selection(), Some(format!("f{cols:02}.txt").as_str()), "Down moves a whole row");
    hands.press(&mut editor, egui::Key::ArrowUp);
    assert_eq!(editor.project_selection(), Some("f00.txt"));

    hands.press(&mut editor, egui::Key::End);
    assert_eq!(editor.project_selection(), Some("f29.txt"));
    hands.press(&mut editor, egui::Key::Home);
    assert_eq!(editor.project_selection(), Some("f00.txt"));
    hands.press(&mut editor, egui::Key::PageDown);
    let after_page = editor.project_selection().unwrap().to_string();
    assert!(after_page.as_str() > "f00.txt", "PageDown moves down: {after_page}");

    // Shift + стрелка растягивает выделение.
    hands.press(&mut editor, egui::Key::Home);
    hands.key(&mut editor, egui::Key::ArrowRight, egui::Modifiers::SHIFT);
    hands.key(&mut editor, egui::Key::ArrowRight, egui::Modifiers::SHIFT);
    assert_eq!(selected(&editor), ["f00.txt", "f01.txt", "f02.txt"]);
    hands.ctrl(&mut editor, egui::Key::A);
    assert_eq!(selected(&editor).len(), 30, "Ctrl+A selects everything");
    hands.press(&mut editor, egui::Key::Escape);
    assert!(selected(&editor).is_empty(), "Esc clears the selection");
}

#[test]
fn arrow_keys_walk_a_list_and_enter_opens_a_folder() {
    let sandbox = Sandbox::new("list_keys");
    sandbox.dir("dir").file("dir/inside.txt", "x").file("a.txt", "a").file("b.txt", "b");
    let mut editor = editor_over(&sandbox.root);
    editor.set_project_list_view(true);
    let mut hands = Hands::new(&mut editor);
    hands.frames(&mut editor, 3);
    let a = hands.asset(&editor, "a.txt");
    hands.click(&mut editor, a);
    hands.press(&mut editor, egui::Key::ArrowDown);
    assert_eq!(editor.project_selection(), Some("b.txt"));
    hands.press(&mut editor, egui::Key::ArrowRight);
    assert_eq!(editor.project_selection(), Some("b.txt"), "Right does nothing in a list");
    hands.press(&mut editor, egui::Key::ArrowUp);
    hands.press(&mut editor, egui::Key::ArrowUp);
    assert_eq!(editor.project_selection(), Some("dir"), "folders come first");
    hands.press(&mut editor, egui::Key::Enter);
    assert_eq!(editor.project_folder(), "dir", "Enter opens a folder");
    assert!(painted(&hands, &editor, "dir/inside.txt"));
}

#[test]
fn typing_letters_jumps_to_the_matching_item() {
    let sandbox = Sandbox::new("typeahead");
    for name in ["apple.txt", "banana.txt", "blue.txt", "cherry.txt"] {
        sandbox.file(name, "x");
    }
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let apple = hands.asset(&editor, "apple.txt");
    hands.click(&mut editor, apple);
    hands.type_text(&mut editor, "c");
    assert_eq!(editor.project_selection(), Some("cherry.txt"));
    hands.wait(&mut editor, 2.0);
    hands.type_text(&mut editor, "b");
    assert_eq!(editor.project_selection(), Some("banana.txt"));
    hands.type_text(&mut editor, "l");
    assert_eq!(editor.project_selection(), Some("blue.txt"), "`bl` quickly after `b` is one word");
}

#[test]
fn ctrl_shift_and_box_selection() {
    let sandbox = Sandbox::new("select");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        sandbox.file(name, "x");
    }
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);

    let a = hands.asset(&editor, "a.txt");
    hands.click(&mut editor, a);
    let c = hands.asset(&editor, "c.txt");
    hands.click_with(&mut editor, c, egui::Modifiers::COMMAND);
    assert_eq!(selected(&editor), ["a.txt", "c.txt"], "Ctrl adds");
    hands.click_with(&mut editor, c, egui::Modifiers::COMMAND);
    assert_eq!(selected(&editor), ["a.txt"], "Ctrl on a selected item removes it");
    let e = hands.asset(&editor, "e.txt");
    hands.click_with(&mut editor, e, egui::Modifiers::SHIFT);
    // Якорь — последний щёлкнутый с Ctrl элемент (c).
    assert_eq!(selected(&editor), ["c.txt", "d.txt", "e.txt"], "Shift takes the range from the anchor");
    let b = hands.asset(&editor, "b.txt");
    hands.wait(&mut editor, 0.5);
    hands.click(&mut editor, b);
    assert_eq!(selected(&editor), ["b.txt"], "a plain click selects one");

    // Резинка: от пустого места под рядом плиток через c, d, e.
    let (rb, re) = (editor.project_entry_rect("b.txt").unwrap(), editor.project_entry_rect("e.txt").unwrap());
    let start = egui::pos2(re.right() + 120.0, re.bottom() + 24.0);
    let end = egui::pos2(rb.center().x + 4.0, rb.center().y);
    hands.drag(&mut editor, start, end);
    assert_eq!(selected(&editor), ["b.txt", "c.txt", "d.txt", "e.txt"], "the rubber band selects what it touches");
    let (left, _) = editor.project_status();
    assert!(left.contains("4 items selected"), "the status line counts the selection: {left}");
    // Клик в пустое место снимает выделение.
    hands.click(&mut editor, egui::pos2(re.right() + 120.0, re.bottom() + 24.0));
    assert!(selected(&editor).is_empty());
}

// ----- избранное и поиск -----

#[test]
fn favorites_hold_pinned_folders_and_saved_searches() {
    let sandbox = Sandbox::new("favorites");
    sandbox.dir("alpha").dir("beta").file("level.scene", "()").file("alpha/coin.txt", "c");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    for query in ["t:script", "t:scene", "t:model", "t:material", "t:prefab"] {
        assert!(hands.has(Editor::project_saved_search_id(query)), "`{query}` is a built-in saved search");
    }
    let scenes = hands.center(Editor::project_saved_search_id("t:scene"));
    hands.click(&mut editor, scenes);
    assert_eq!(editor.project_search(), "t:scene");
    assert_eq!(editor.project_shown(), ["level.scene"], "a saved search shows its results");

    // Закрепить каталог из меню строки дерева.
    let row = hands.tree("alpha");
    hands.right_click(&mut editor, row);
    let pin = hands.find_text(&mut editor, "Add to Favorites").expect("the tree menu can pin a folder");
    hands.click(&mut editor, pin);
    assert_eq!(editor.preferences().project_ui.favorites, ["alpha"]);
    hands.frames(&mut editor, 2);
    let favorite = hands.center(Editor::project_favorite_id("alpha"));
    hands.click(&mut editor, favorite);
    assert_eq!(editor.project_folder(), "alpha", "a pinned folder opens by a click");
    assert_eq!(editor.project_search(), "");

    // Каталог, брошенный на «Favorites», тоже закрепляется.
    let root = hands.tree("");
    hands.click(&mut editor, root);
    let (from, to) = (hands.folder("beta"), hands.center(Editor::project_favorites_header_id()));
    hands.drag(&mut editor, from, to);
    assert_eq!(editor.preferences().project_ui.favorites, ["alpha", "beta"]);

    // Свой поиск запоминается, переживает перезапуск (настройки) и удаляется из меню.
    editor.set_project_search("coin");
    editor.save_project_search();
    let saved = editor.preferences().clone();
    assert_eq!(saved.project_ui.searches, [("coin".to_string(), "coin".to_string())]);
    let mut fresh = editor_over(&sandbox.root);
    fresh.set_preferences(saved);
    let mut hands = Hands::new(&mut fresh);
    assert!(hands.has(Editor::project_saved_search_id("coin")), "the saved search is back after a restart");
    assert!(hands.has(Editor::project_favorite_id("beta")), "so are the pinned folders");
    let search = hands.center(Editor::project_saved_search_id("coin"));
    hands.right_click(&mut fresh, search);
    let delete = hands.find_text(&mut fresh, "Delete Saved Search").unwrap();
    hands.click(&mut fresh, delete);
    assert!(fresh.preferences().project_ui.searches.is_empty());
}

#[test]
fn search_has_scope_type_tokens_and_a_result_count() {
    let sandbox = Sandbox::new("search");
    sandbox
        .file("x/coin.txt", "1")
        .file("x/deep/coin2.txt", "2")
        .file("y/coin3.scene", "()")
        .file("level.scene", "()")
        .file("x/plain.txt", "p");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let row = hands.tree("x");
    hands.click(&mut editor, row);
    let sorted = |editor: &Editor| {
        let mut shown = editor.project_shown().to_vec();
        shown.sort();
        shown
    };
    editor.set_project_search("coin");
    hands.frames(&mut editor, 2);
    assert_eq!(sorted(&editor), ["coin.txt", "coin2.txt"], "the default scope is This Folder and everything below it");
    assert_eq!(editor.project_status().1, "2 results");
    editor.set_project_search_scope(true);
    hands.frames(&mut editor, 2);
    assert_eq!(sorted(&editor), ["coin.txt", "coin2.txt", "coin3.scene"], "All Assets looks everywhere");
    editor.set_project_search("t:scene");
    hands.frames(&mut editor, 2);
    assert_eq!(sorted(&editor), ["coin3.scene", "level.scene"], "t:scene filters by type");
    editor.set_project_search("coin t:scene");
    hands.frames(&mut editor, 2);
    assert_eq!(sorted(&editor), ["coin3.scene"], "words and tokens combine");
    editor.set_project_search("t:banana");
    hands.frames(&mut editor, 2);
    assert!(sorted(&editor).is_empty(), "an unknown type finds nothing");
    assert_eq!(editor.project_status().1, "0 results");
    editor.set_project_search("");
    hands.frames(&mut editor, 2);
    assert_eq!(editor.project_status().1, "3 items", "without a query the folder is listed again: deep, coin.txt, plain.txt");

    // Ctrl+F переводит фокус в поле поиска: набранное попадает в него.
    hands.ctrl(&mut editor, egui::Key::F);
    hands.type_text(&mut editor, "plain");
    assert_eq!(editor.project_search(), "plain", "Ctrl+F focuses the search field");
}
