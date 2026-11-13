//! Окно Project как файловый менеджер, часть 2: копирование, перенос с `.meta`,
//! переименование на месте, удаление в корзину, импорт, меню. Окно и GPU не нужны.
//! Файлы — только во временных каталогах и копиях `assets/`.
#![cfg(feature = "editor")]

mod files_support;

use engine::editor::{egui, Action, Editor};
use files_support::*;

// ----- копирование, вырезание, дубликаты -----

#[test]
fn copy_cut_paste_and_duplicate_keep_meta_files_with_conflict_names() {
    let sandbox = Sandbox::new("clipboard");
    sandbox.file("a.txt", "a").file("b.txt", "b").dir("d");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let a = hands.asset(&editor, "a.txt");
    hands.click(&mut editor, a);
    hands.ctrl(&mut editor, egui::Key::C);
    assert_eq!(editor.project_clipboard(), Some((vec!["a.txt".to_string()], false)));
    let d = hands.folder("d");
    hands.double_click(&mut editor, d);
    assert_eq!(editor.project_folder(), "d");
    hands.ctrl(&mut editor, egui::Key::V);
    assert!(sandbox.exists("d/a.txt") && sandbox.exists("d/a.txt.meta"), "paste copies the file and its .meta");
    assert_ne!(meta_guid(&sandbox.root.join("a.txt")), meta_guid(&sandbox.root.join("d/a.txt")), "a copy gets a new GUID");
    assert!(sandbox.exists("a.txt"), "copy keeps the original");
    hands.ctrl(&mut editor, egui::Key::V);
    assert!(sandbox.exists("d/a (1).txt") && sandbox.exists("d/a (1).txt.meta"), "a name clash becomes `name (1)`");
    assert_eq!(selected(&editor), ["d/a (1).txt"], "the pasted item is selected");

    // Вырезать: файл переезжает вместе с .meta, буфер пустеет.
    hands.press(&mut editor, egui::Key::Backspace);
    assert_eq!(editor.project_folder(), "");
    let b = hands.asset(&editor, "b.txt");
    hands.click(&mut editor, b);
    let guid_before = meta_guid(&sandbox.root.join("b.txt"));
    hands.ctrl(&mut editor, egui::Key::X);
    assert_eq!(editor.project_clipboard(), Some((vec!["b.txt".to_string()], true)));
    let d = hands.folder("d");
    hands.double_click(&mut editor, d);
    hands.ctrl(&mut editor, egui::Key::V);
    assert!(!sandbox.exists("b.txt") && !sandbox.exists("b.txt.meta"), "cut removes the original and its .meta");
    assert!(sandbox.exists("d/b.txt") && sandbox.exists("d/b.txt.meta"));
    assert_eq!(meta_guid(&sandbox.root.join("d/b.txt")), guid_before, "a move keeps the GUID");
    assert_eq!(editor.project_clipboard(), None, "the clipboard is empty after a cut-paste");

    // Дубликат рядом с оригиналом.
    let inner = hands.asset(&editor, "d/a.txt");
    hands.click(&mut editor, inner);
    let scans = editor.project_scan_count();
    hands.ctrl(&mut editor, egui::Key::D);
    assert!(sandbox.exists("d/a (2).txt"), "Ctrl+D duplicates next to the original (`a (1)` is taken)");
    assert_eq!(editor.project_scan_count(), scans + 1, "one operation, one rescan");
}

#[test]
fn paste_events_from_the_system_clipboard_are_trusted_only_when_they_are_ours() {
    let sandbox = Sandbox::new("paste_event");
    sandbox.file("a.txt", "a").dir("d");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let a = hands.asset(&editor, "a.txt");
    hands.click(&mut editor, a);
    hands.frame(&mut editor, vec![egui::Event::Copy]);
    hands.frames(&mut editor, 2);
    assert!(editor.project_clipboard().is_some(), "Ctrl+C arrives from the window system as Event::Copy");
    let d = hands.folder("d");
    hands.double_click(&mut editor, d);
    // Системный буфер изменили в другой программе: вставлять нечего.
    hands.frame(&mut editor, vec![egui::Event::Paste("something else".to_string())]);
    hands.frames(&mut editor, 2);
    assert!(!sandbox.exists("d/a.txt"));
    hands.frame(&mut editor, vec![egui::Event::Paste("Assets/a.txt".to_string())]);
    hands.frames(&mut editor, 2);
    assert!(sandbox.exists("d/a.txt"), "the paste of our own text works");
}

// ----- перетаскивание -----

#[test]
fn dragging_a_texture_onto_a_folder_moves_the_file_and_meta_and_scenes_still_find_it() {
    let sandbox = Sandbox::copy_of_assets("drag_move");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let guid = "7f3a9c2e5b1d4e8fa6c0d2b4e6f81a3c";
    assert_eq!(meta_guid(&sandbox.root.join("sprites/player.png")), guid);

    editor.select_asset("sprites/player.png");
    hands.frames(&mut editor, 3);
    let tile = hands.asset(&editor, "sprites/player.png");
    let target = hands.tree("branding");
    hands.drag(&mut editor, tile, target);
    assert!(!sandbox.exists("sprites/player.png") && !sandbox.exists("sprites/player.png.meta"), "the file and its .meta left");
    assert!(sandbox.exists("branding/player.png") && sandbox.exists("branding/player.png.meta"), "and arrived together");
    assert_eq!(meta_guid(&sandbox.root.join("branding/player.png")), guid, "the GUID survived the move");

    // Сцена и префаб ссылаются на GUID: база находит ассет на новом месте и читает картинку.
    let db = engine::assets::AssetDatabase::open(&sandbox.root).expect("the database reopens");
    let parsed = engine::assets::Guid::parse(guid).expect("a valid GUID");
    assert_eq!(db.relative_path_of(parsed), Some("branding/player.png"));
    assert!(db.read_image(parsed).is_ok(), "the texture still loads by GUID");
    let scene = std::fs::read_to_string(sandbox.root.join("scenes/level1.scene")).unwrap();
    assert!(scene.contains(guid), "the scene text was not touched: it references the GUID");
    editor.apply(Action::OpenScene("scenes/level1.scene".to_string()));
    hands.frames(&mut editor, 2);
    assert_eq!(editor.scene_path(), Some("scenes/level1.scene"), "the scene still opens");
}

#[test]
fn several_selected_items_move_together_and_ctrl_copies() {
    let sandbox = Sandbox::new("drag_many");
    sandbox.file("a.txt", "a").file("b.txt", "b").file("c.txt", "c").dir("dest").dir("copies");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let (a, b) = (hands.asset(&editor, "a.txt"), hands.asset(&editor, "b.txt"));
    hands.click(&mut editor, a);
    hands.click_with(&mut editor, b, egui::Modifiers::COMMAND);
    let scans = editor.project_scan_count();
    let (from, to) = (hands.asset(&editor, "b.txt"), hands.folder("dest"));
    hands.drag(&mut editor, from, to);
    assert!(sandbox.exists("dest/a.txt.meta") && sandbox.exists("dest/b.txt.meta"), "both items moved with their .meta");
    assert!(!sandbox.exists("a.txt") && !sandbox.exists("b.txt") && sandbox.exists("c.txt"));
    assert_eq!(editor.project_scan_count(), scans + 1, "the whole drop is one rescan");

    // Ctrl при отпускании — копия; оригинал остаётся.
    let (from, to) = (hands.asset(&editor, "c.txt"), hands.folder("copies"));
    hands.drag_with(&mut editor, from, to, egui::Modifiers::COMMAND);
    assert!(sandbox.exists("c.txt") && sandbox.exists("copies/c.txt"), "Ctrl+drag copies");
    assert_ne!(meta_guid(&sandbox.root.join("c.txt")), meta_guid(&sandbox.root.join("copies/c.txt")));
}

#[test]
fn dropping_on_a_breadcrumb_and_never_into_itself() {
    let sandbox = Sandbox::new("drag_crumb");
    sandbox.dir("d/inner").file("d/x.txt", "x");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let row = hands.tree("d");
    hands.click(&mut editor, row);
    let (from, to) = (hands.asset(&editor, "d/x.txt"), hands.center(Editor::project_crumb_id("")));
    hands.drag(&mut editor, from, to);
    assert!(sandbox.exists("x.txt") && !sandbox.exists("d/x.txt"), "a crumb is a drop target: Assets");

    // Каталог в самого себя не переносится.
    let (from, to) = (hands.folder("d/inner"), hands.tree("d/inner"));
    hands.drag(&mut editor, from, to);
    assert!(sandbox.exists("d/inner"), "nothing happened");
}

// ----- переименование на месте -----

#[test]
fn inline_rename_keeps_the_extension_and_validates_the_name() {
    let sandbox = Sandbox::new("rename");
    sandbox.file("notes.txt", "n").file("other.txt", "o");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let tile = hands.asset(&editor, "notes.txt");
    hands.click(&mut editor, tile);
    let guid = meta_guid(&sandbox.root.join("notes.txt"));

    hands.press(&mut editor, egui::Key::F2);
    assert_eq!(editor.project_rename_text(), Some("notes.txt"), "F2 starts the rename in place");
    assert!(hands.has(Editor::project_rename_field_id("notes.txt")));
    // Выделено имя без расширения: набранное заменяет его.
    hands.type_text(&mut editor, "memo");
    assert_eq!(editor.project_rename_text(), Some("memo.txt"));
    hands.press(&mut editor, egui::Key::Enter);
    assert_eq!(editor.project_rename_text(), None);
    assert!(sandbox.exists("memo.txt") && sandbox.exists("memo.txt.meta") && !sandbox.exists("notes.txt"));
    assert_eq!(meta_guid(&sandbox.root.join("memo.txt")), guid, "the GUID is kept");
    assert_eq!(editor.project_selection(), Some("memo.txt"), "the selection follows the new name");

    // Esc отменяет.
    hands.press(&mut editor, egui::Key::F2);
    hands.type_text(&mut editor, "zzz");
    hands.press(&mut editor, egui::Key::Escape);
    assert_eq!(editor.project_rename_text(), None);
    assert!(sandbox.exists("memo.txt") && !sandbox.exists("zzz.txt"));

    // Занятое и неверное имя не принимаются: поле остаётся.
    hands.press(&mut editor, egui::Key::F2);
    hands.type_text(&mut editor, "other");
    hands.press(&mut editor, egui::Key::Enter);
    assert_eq!(editor.project_rename_text(), Some("other.txt"), "a name clash keeps the field open");
    assert!(sandbox.exists("memo.txt") && sandbox.exists("other.txt"));
    hands.press(&mut editor, egui::Key::Escape);
    hands.press(&mut editor, egui::Key::F2);
    hands.type_text(&mut editor, "a?b");
    hands.press(&mut editor, egui::Key::Enter);
    assert!(editor.project_rename_text().is_some(), "`?` is not allowed in a name");
    hands.press(&mut editor, egui::Key::Escape);
    assert!(sandbox.exists("memo.txt"));
}

#[test]
fn a_slow_second_click_renames_but_a_double_click_does_not() {
    let sandbox = Sandbox::new("slow_click");
    sandbox.file("a.txt", "a").dir("d");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let tile = hands.asset(&editor, "a.txt");
    hands.click(&mut editor, tile);
    hands.wait(&mut editor, 0.4);
    hands.click(&mut editor, tile);
    assert_eq!(editor.project_rename_text(), None, "a second click after 0.4 s is neither a double click nor a slow one");
    hands.wait(&mut editor, 1.0);
    hands.click(&mut editor, tile);
    assert_eq!(editor.project_rename_text(), Some("a.txt"), "a slow second click starts the rename");
    hands.press(&mut editor, egui::Key::Escape);
}

// ----- удаление -----

#[test]
fn delete_sends_files_to_the_recycle_bin_and_the_folder_you_are_in_falls_back() {
    let sandbox = Sandbox::new("delete");
    sandbox.file("gone.txt", "g").file("keep.txt", "k").dir("d/e").file("d/e/f.txt", "f");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let tile = hands.asset(&editor, "gone.txt");
    hands.click(&mut editor, tile);
    hands.press(&mut editor, egui::Key::Delete);
    if editor.project_confirm_pending() {
        // Система без корзины: удаление спрашивает и говорит, что оно насовсем.
        let button = hands.find_text(&mut editor, "Delete Permanently").expect("the dialog offers a permanent delete");
        hands.click(&mut editor, button);
    }
    assert!(!sandbox.exists("gone.txt") && !sandbox.exists("gone.txt.meta"), "the file and its .meta left the folder");
    assert!(sandbox.exists("keep.txt"), "only the selection is deleted");
    assert!(selected(&editor).is_empty());
    assert!(!editor.project_toasts().is_empty(), "the result is reported");

    // Каталог, в котором стоишь, удалён: окно поднимается к существующему.
    let row = hands.tree("d/e");
    hands.click(&mut editor, row);
    assert_eq!(editor.project_folder(), "d/e");
    hands.right_click(&mut editor, row);
    let delete = hands.find_text(&mut editor, "Delete").expect("the folder menu has Delete");
    hands.click(&mut editor, delete);
    if editor.project_confirm_pending() {
        let button = hands.find_text(&mut editor, "Delete Permanently").unwrap();
        hands.click(&mut editor, button);
    }
    assert!(!sandbox.exists("d/e"));
    assert_eq!(editor.project_folder(), "d", "the open folder vanished: the window moves to its parent");
}

// ----- импорт -----

#[test]
fn files_dropped_from_the_os_are_copied_into_the_open_folder() {
    let sandbox = Sandbox::new("import_dst");
    sandbox.dir("target");
    let outside = Sandbox::new("import_src");
    outside.file("pic.txt", "p").file("pack/inner.txt", "i").file("pack/deeper/two.txt", "2").file("pic.txt.meta", "alien");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let row = hands.tree("target");
    hands.click(&mut editor, row);

    let dropped = ["pic.txt", "pack"]
        .iter()
        .map(|n| egui::DroppedFile { path: Some(outside.root.join(n)), ..Default::default() })
        .collect::<Vec<_>>();
    hands.frame_with(&mut editor, vec![], dropped);
    for _ in 0..400 {
        hands.frames(&mut editor, 1);
        if !editor.project_import_active() && sandbox.exists("target/pack/deeper/two.txt.meta") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    hands.frames(&mut editor, 3);
    assert!(sandbox.exists("target/pic.txt") && sandbox.exists("target/pack/inner.txt") && sandbox.exists("target/pack/deeper/two.txt"));
    assert!(sandbox.exists("target/pic.txt.meta") && sandbox.exists("target/pack/inner.txt.meta"), "the database made .meta files");
    assert_ne!(std::fs::read_to_string(sandbox.root.join("target/pic.txt.meta")).unwrap(), "alien", "a foreign .meta is not imported");
    assert!(outside.exists("pic.txt") && outside.exists("pack/inner.txt"), "the originals stay where they were");
    assert!(editor.project_toasts().iter().any(|t| t.starts_with("Imported")), "the result is reported: {:?}", editor.project_toasts());
    assert!(selected(&editor).contains(&"target/pic.txt".to_string()), "the imported items are selected");
}

#[test]
fn the_import_browser_draws_and_closes_with_escape() {
    let sandbox = Sandbox::new("browser");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    editor.show_import_browser();
    hands.frames(&mut editor, 3);
    assert!(editor.project_browser_open());
    assert!(hands.find_text(&mut editor, "Cancel").is_some(), "the in-app browser draws its buttons");
    hands.press(&mut editor, egui::Key::Escape);
    assert!(!editor.project_browser_open());
}

// ----- контекстные меню -----

#[test]
fn every_item_of_the_asset_menu_does_something() {
    let sandbox = Sandbox::new("menu_items");
    sandbox.file("notes.txt", "n").dir("d");
    let mut editor = editor_over(&sandbox.root);
    editor.set_project_list_view(true);
    let mut hands = Hands::new(&mut editor);
    let open = |hands: &mut Hands, editor: &mut Editor, label: &str| {
        let tile = hands.asset(editor, "notes.txt");
        hands.right_click(editor, tile);
        let item = hands.find_text(editor, label).unwrap_or_else(|| panic!("the menu has `{label}`"));
        hands.click(editor, item);
    };

    open(&mut hands, &mut editor, "Copy");
    assert_eq!(editor.project_clipboard(), Some((vec!["notes.txt".to_string()], false)));
    open(&mut hands, &mut editor, "Cut");
    assert_eq!(editor.project_clipboard(), Some((vec!["notes.txt".to_string()], true)));
    open(&mut hands, &mut editor, "Copy");
    open(&mut hands, &mut editor, "Paste");
    assert!(sandbox.exists("notes (1).txt"), "Paste copies next to the original");
    open(&mut hands, &mut editor, "Duplicate");
    assert!(sandbox.exists("notes (2).txt"), "Duplicate makes a numbered copy");
    open(&mut hands, &mut editor, "Copy GUID");
    assert!(editor.project_toasts().iter().any(|t| t.contains("GUID")), "Copy GUID says so");
    open(&mut hands, &mut editor, "Copy Path");
    assert!(editor.project_toasts().iter().any(|t| t.contains("Assets/notes.txt")), "Copy Path says what it copied");
    open(&mut hands, &mut editor, "Reimport");
    assert!(editor.project_toasts().iter().any(|t| t.starts_with("Reimported")));
    open(&mut hands, &mut editor, "Find References In Scene");
    assert!(editor.project_toasts().iter().any(|t| t.contains("No object in the open scene")), "{:?}", editor.project_toasts());
    open(&mut hands, &mut editor, "Properties");
    assert!(editor.preferences().project_ui.details);
    open(&mut hands, &mut editor, "Rename");
    assert_eq!(editor.project_rename_text(), Some("notes.txt"), "Rename starts the in-place field");
    hands.press(&mut editor, egui::Key::Escape);
    let scans = editor.project_scan_count();
    open(&mut hands, &mut editor, "Delete");
    if editor.project_confirm_pending() {
        let button = hands.find_text(&mut editor, "Delete Permanently").unwrap();
        hands.click(&mut editor, button);
    }
    assert!(!sandbox.exists("notes.txt") && !sandbox.exists("notes.txt.meta"), "Delete removes the file");
    assert!(editor.project_scan_count() > scans);
    // «Show in File Manager» открыл бы настоящий проводник: здесь проверяется только наличие.
    let tile = hands.asset(&editor, "notes (1).txt");
    hands.right_click(&mut editor, tile);
    assert!(hands.find_text(&mut editor, "Show in File Manager").is_some());
    hands.press(&mut editor, egui::Key::Escape);
    // Последним: открытие в Code уводит на другую вкладку, окна Project больше не видно.
    let tile = hands.asset(&editor, "notes (1).txt");
    hands.right_click(&mut editor, tile);
    let item = hands.find_text(&mut editor, "Open in Code Tab").expect("the menu has Open in Code Tab");
    hands.click(&mut editor, item);
    assert_eq!(editor.view_tab(), engine::editor::ViewTab::Code, "Open in Code Tab switches to the Code view");
}

#[test]
fn the_empty_space_menu_creates_things_and_selects_all() {
    let sandbox = Sandbox::new("menu_space");
    sandbox.file("a.txt", "a").file("b.txt", "b");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let rect = editor.project_entry_rect("b.txt").unwrap();
    let empty = egui::pos2(rect.right() + 200.0, rect.center().y);
    hands.right_click(&mut editor, empty);
    let item = hands.find_text(&mut editor, "Select All").expect("the menu has Select All");
    hands.click(&mut editor, item);
    assert_eq!(selected(&editor), ["a.txt", "b.txt"]);

    hands.right_click(&mut editor, empty);
    let create = hands.find_text(&mut editor, "Create").expect("the menu has Create");
    hands.click(&mut editor, create);
    let folder = hands.find_text(&mut editor, "Folder").expect("Create has Folder");
    hands.click(&mut editor, folder);
    assert!(sandbox.exists("New Folder"), "Create → Folder makes a folder");
    assert_eq!(editor.project_rename_text(), Some("New Folder"), "and starts naming it in place");
    hands.type_text(&mut editor, "Props");
    hands.press(&mut editor, egui::Key::Enter);
    assert!(sandbox.exists("Props") && !sandbox.exists("New Folder"), "the typed name is the folder name");

    hands.right_click(&mut editor, empty);
    let create = hands.find_text(&mut editor, "Create").unwrap();
    hands.click(&mut editor, create);
    let scene = hands.find_text(&mut editor, "Scene").expect("Create has Scene");
    hands.click(&mut editor, scene);
    assert!(sandbox.exists("New Scene.scene") && sandbox.exists("New Scene.scene.meta"), "a new scene file with its .meta");
    hands.press(&mut editor, egui::Key::Escape);

    hands.right_click(&mut editor, empty);
    let import = hands.find_text(&mut editor, "Import New Asset…").expect("the menu has Import New Asset");
    hands.click(&mut editor, import);
    assert!(editor.project_browser_open());
}
