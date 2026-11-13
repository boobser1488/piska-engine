//! Окно Project как файловый менеджер, часть 3: столбцы, разделитель, узкий режим,
//! тысячи файлов и снимки. Окно и GPU не нужны (кроме галереи, она помечена `#[ignore]`).
#![cfg(feature = "editor")]

mod files_support;

use std::path::PathBuf;

use engine::editor::{egui, Action, Editor};
use files_support::*;

#[test]
fn column_widths_and_the_splitter_are_remembered() {
    let sandbox = Sandbox::new("columns");
    sandbox.file("a.txt", "a").dir("d");
    let mut editor = editor_over(&sandbox.root);
    editor.set_project_list_view(true);
    let mut hands = Hands::new(&mut editor);
    hands.frames(&mut editor, 3);
    let before = editor.preferences().project_ui.clone();
    let grip = hands.center(Editor::project_column_grip_id("size"));
    hands.drag(&mut editor, grip, grip - egui::vec2(40.0, 0.0));
    let after = editor.preferences().project_ui.clone();
    assert!(
        (after.columns[2] - (before.columns[2] + 40.0)).abs() < 3.0,
        "dragging the Size border widens it: {:?} -> {:?}",
        before.columns,
        after.columns
    );

    // После перезапуска ширина та же.
    let mut fresh = editor_over(&sandbox.root);
    fresh.set_preferences(editor.preferences().clone());
    fresh.set_project_list_view(true);
    let mut hands2 = Hands::new(&mut fresh);
    hands2.frames(&mut fresh, 3);
    let header = hands2.ctx.read_response(Editor::project_sort_header_id("size")).unwrap().rect;
    assert!((header.width() - after.columns[2]).abs() < 2.0, "the saved width is applied: {} vs {}", header.width(), after.columns[2]);

    // Разделитель между деревом и содержимым.
    let row = hands.ctx.read_response(Editor::project_tree_row_id("")).unwrap().rect;
    hands.drag(&mut editor, egui::pos2(row.right() + 1.0, row.center().y), egui::pos2(row.right() + 61.0, row.center().y));
    let moved = editor.preferences().project_ui.split;
    assert!(moved > row.width() + 40.0, "the splitter followed the mouse: {} -> {moved}", row.width());
    let mut third = editor_over(&sandbox.root);
    third.set_preferences(editor.preferences().clone());
    let mut hands3 = Hands::new(&mut third);
    hands3.frames(&mut third, 3);
    let tree = hands3.ctx.read_response(Editor::project_tree_row_id("")).unwrap().rect;
    assert!((tree.width() - moved).abs() < 2.0, "the tree width is restored: {} vs {moved}", tree.width());
}

#[test]
fn a_narrow_window_drops_the_tree_and_keeps_working() {
    let sandbox = Sandbox::new("narrow");
    sandbox.dir("d").file("d/x.txt", "x").file("a.txt", "a");
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    hands.screen = egui::vec2(420.0, 700.0);
    hands.frames(&mut editor, 4);
    assert!(!hands.has(Editor::project_tree_row_id("d")), "the tree collapses in a narrow window");
    assert!(painted(&hands, &editor, "a.txt"), "the content is still there");
    let folder = hands.folder("d");
    hands.double_click(&mut editor, folder);
    assert_eq!(editor.project_folder(), "d", "folders still open");
    assert!(hands.has(Editor::project_crumb_id("")), "the breadcrumb is the way back");
}

#[test]
fn five_thousand_files_stay_fast_and_only_visible_items_are_painted() {
    let sandbox = Sandbox::new("perf");
    for n in 0..5000 {
        sandbox.file(&format!("bulk/file_{n:04}.txt"), "x");
    }
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let row = hands.tree("bulk");
    let started = std::time::Instant::now();
    hands.click(&mut editor, row);
    assert_eq!(editor.project_shown().len(), 5000);
    let first = started.elapsed();
    assert!(first.as_millis() < 3000, "opening a folder of 5000 files took {first:?}");
    assert!(editor.project_painted_count() < 400, "only visible tiles are painted: {}", editor.project_painted_count());

    // Курсор над содержимым: колесо прокручивает его, а не дерево.
    let over_content = editor.project_entry_rect("bulk/file_0005.txt").expect("a tile is painted").center();
    hands.move_to(&mut editor, over_content);
    let measure = |editor: &mut Editor, hands: &mut Hands, scroll: bool| {
        let pos = hands.pointer.unwrap();
        let started = std::time::Instant::now();
        for _ in 0..30 {
            let events = if scroll {
                vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -180.0), modifiers: egui::Modifiers::NONE }]
            } else {
                vec![egui::Event::PointerMoved(pos)]
            };
            hands.frame(editor, events);
        }
        started.elapsed().as_secs_f64() * 1000.0 / 30.0
    };
    let still = measure(&mut editor, &mut hands, false);
    let scrolling = measure(&mut editor, &mut hands, true);
    eprintln!("5000 files, tiles: {still:.1} ms/frame idle, {scrolling:.1} ms/frame scrolling");
    assert!(still < 40.0 && scrolling < 60.0, "frame budget: {still:.1} ms idle, {scrolling:.1} ms scrolling");
    assert!(editor.project_painted_count() < 400);
    assert!(!painted(&hands, &editor, "bulk/file_0000.txt"), "scrolling moved the window down the list");

    editor.set_project_list_view(true);
    hands.frames(&mut editor, 3);
    let list = measure(&mut editor, &mut hands, false);
    let list_scroll = measure(&mut editor, &mut hands, true);
    eprintln!("5000 files, list: {list:.1} ms/frame idle, {list_scroll:.1} ms/frame scrolling");
    assert!(list < 40.0 && list_scroll < 60.0, "list frame budget: {list:.1} / {list_scroll:.1} ms");
    assert!(editor.project_painted_count() < 200);
}

#[test]
fn long_non_ascii_and_read_only_names_work() {
    let sandbox = Sandbox::new("odd_names");
    let long = format!("{}.txt", "очень_длинное_имя_файла_".repeat(8));
    sandbox.file(&long, "x").file("Файл с пробелами и ёж.txt", "y").file("ro.txt", "r");
    let mut permissions = std::fs::metadata(sandbox.root.join("ro.txt")).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(sandbox.root.join("ro.txt"), permissions).unwrap();
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let tile = hands.asset(&editor, &long);
    assert!(editor.project_entry_rect(&long).unwrap().width() < 200.0, "a long name does not widen the tile");
    hands.click(&mut editor, tile);
    assert_eq!(editor.project_selection(), Some(long.as_str()));

    // Переименование файла с юникодным именем.
    let tile = hands.asset(&editor, "Файл с пробелами и ёж.txt");
    hands.wait(&mut editor, 0.5);
    hands.click(&mut editor, tile);
    hands.press(&mut editor, egui::Key::F2);
    hands.type_text(&mut editor, "новое имя");
    hands.press(&mut editor, egui::Key::Enter);
    assert!(sandbox.exists("новое имя.txt") && sandbox.exists("новое имя.txt.meta"));

    // Только для чтения: копия и дубликат работают, удаление тоже (в корзину).
    let tile = hands.asset(&editor, "ro.txt");
    hands.wait(&mut editor, 0.5);
    hands.click(&mut editor, tile);
    hands.ctrl(&mut editor, egui::Key::D);
    assert!(sandbox.exists("ro (1).txt"));
    hands.press(&mut editor, egui::Key::Delete);
    if editor.project_confirm_pending() {
        let button = hands.find_text(&mut editor, "Delete Permanently").unwrap();
        hands.click(&mut editor, button);
    }
    assert!(!sandbox.exists("ro (1).txt"), "the duplicate (it is the selected one) is gone");
    // Подчистить флаг, чтобы временный каталог удалился.
    for name in ["ro.txt", "ro (1).txt"] {
        if let Ok(meta) = std::fs::metadata(sandbox.root.join(name)) {
            let mut p = meta.permissions();
            p.set_readonly(false);
            let _ = std::fs::set_permissions(sandbox.root.join(name), p);
        }
    }
}

/// Снимки окна Project: плитки, список со столбцами, выделение, переименование,
/// перетаскивание, избранное, поиск, узкий режим.
/// `ENGINE_SHOTS_DIR=out cargo test -p engine --test editor_files_view -- --ignored gallery`
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn project_screenshot_gallery() {
    use engine::editor::{capture, capture_with_input};
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();

    let sandbox = Sandbox::copy_of_assets("gallery");
    let mut editor = editor_over(&sandbox.root);
    editor.set_project_search("");
    let shot = |editor: &mut Editor, name: &str, frames: u32| {
        capture(editor, 1440, 860, frames).unwrap().save(dir.join(name)).unwrap();
    };

    // 1. Плитки в корне.
    shot(&mut editor, "p_tiles.png", 10);
    // 2. Каталог со спрайтами, список со столбцами.
    editor.select_asset("sprites/player.png");
    editor.set_project_list_view(true);
    shot(&mut editor, "p_list_columns.png", 10);
    // 3. Выделение нескольких и сводка.
    editor.set_project_list_view(false);
    editor.select_asset("materials/checker.png");
    editor.select_project_items(&["materials/checker.png", "materials/checker.material", "materials/glass.material", "materials/leaves.png"]);
    shot(&mut editor, "p_multi_select.png", 10);
    // 4. Избранное и поиск с токенами.
    editor.select_asset("scenes/level1.scene");
    editor.apply(Action::RefreshAssets);
    editor.set_project_search("t:material");
    editor.set_project_search_scope(true);
    shot(&mut editor, "p_search_tokens.png", 10);
    editor.set_project_search("");
    // 5. Переименование на месте: щелчок по плитке и F2 — событиями внутри съёмки.
    editor.select_asset("materials/checker.png");
    let key = |key, pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::NONE };
    let image = capture_with_input(&mut editor, 1440, 860, 12, |frame, editor| {
        let tile = editor.project_entry_rect("materials/checker.png").map(|r| r.center()).unwrap_or_default();
        match frame {
            1 => vec![egui::Event::PointerMoved(tile)],
            6 => vec![key(egui::Key::F2, true), key(egui::Key::F2, false)],
            _ => vec![],
        }
    })
    .unwrap();
    image.save(dir.join("p_inline_rename.png")).unwrap();
    editor.apply(Action::RefreshAssets);
    // 6. Перетаскивание: плитка над строкой дерева, кнопка зажата.
    let image = capture_with_input(&mut editor, 1440, 860, 14, |frame, editor| {
        let tile = editor.project_entry_rect("materials/checker.png").map(|r| r.center()).unwrap_or_default();
        let target = egui::pos2(100.0, 466.0 - 24.0);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        match frame {
            1 => vec![egui::Event::PointerMoved(tile)],
            2 => vec![button(tile, true)],
            3..=10 => vec![egui::Event::PointerMoved(tile + (target - tile) * ((frame - 2) as f32 / 8.0))],
            _ => vec![],
        }
    })
    .unwrap();
    image.save(dir.join("p_drag_drop.png")).unwrap();
}
