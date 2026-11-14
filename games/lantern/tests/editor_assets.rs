//! Проект открывается в редакторе по-настоящему: каждая сцена и каждый префаб читаются,
//! иерархия заполнена, объект можно подвинуть, сцена сохраняется во временную копию, и
//! игра запускается из изменённой копии. Запуск: `cargo test -p lantern --features editor
//! --test editor_assets` (без окна, звук выключен).
#![cfg(feature = "editor")]

use engine::editor::{Action, Editor};
use engine::prelude::*;
use std::path::{Path, PathBuf};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
const DT: f32 = 1.0 / 60.0;

/// Копия каталога ассетов во временной папке — сюда редактор пишет.
fn temp_assets(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lantern_editor_test_{}_{tag}", std::process::id()));
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    copy(Path::new(ASSETS), &dir);
    dir
}

fn editor_on(assets: &Path, scene: &str) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let app = lantern::build_in(SaveData::in_memory(), assets).expect("the game builds from the assets");
    Editor::new(app, EditorConfig::default().open_scene(scene)).expect("the scene opens in the editor")
}

fn names(editor: &Editor) -> Vec<String> {
    let scene = &editor.app.world.scene;
    scene.ids().into_iter().filter_map(|id| scene.get(id).map(|o| o.name.clone())).collect()
}

#[test]
fn every_scene_opens_in_the_editor_is_populated_editable_and_saves() {
    // (сцена, объект, который двигаем, минимум объектов, обязательные имена)
    let cases: [(&str, &str, usize, &[&str]); 7] = [
        ("scenes/menu.scene", "Fireflies", 60, &["Menu", "MenuCanvas", "ScreenMain", "ScreenLevels", "level_0", "music_vol", "Main Camera"]),
        ("scenes/level1.scene", "Player", 100, &["Tiles", "Deco", "Level", "Hud", "PauseOverlay", "ResultsOverlay", "Enemies", "Fireflies", "Exit", "Checkpoints"]),
        ("scenes/level2.scene", "Player", 100, &["Tiles", "Level", "Hud", "Mechanisms"]),
        ("scenes/level3.scene", "Player", 100, &["Tiles", "Level", "Hud", "Gate"]),
        ("scenes/level4.scene", "Player", 100, &["Tiles", "Level", "Hud", "MovingPlatform"]),
        ("scenes/level5.scene", "Player", 100, &["Tiles", "Level", "Hud", "Brute"]),
        ("scenes/level6.scene", "Player", 100, &["Tiles", "Level", "Hud", "Boss"]),
    ];
    let project = temp_assets("scenes");
    for (scene, mover, min_objects, must_have) in cases {
        let mut editor = editor_on(&project, scene);
        let all = names(&editor);
        assert!(all.len() >= min_objects, "{scene}: only {} objects", all.len());
        for name in must_have {
            assert!(all.iter().any(|n| n == name), "{scene}: no object `{name}` in the hierarchy");
        }

        // выделить и подвинуть
        let id = editor.app.world.scene.find_by_name(mover).unwrap_or_else(|| panic!("{scene}: no `{mover}`"));
        editor.apply(Action::Select(Some(id)));
        let before = editor.app.world.scene.world_position(id);
        let moved = before + Vec3::new(1.5, 0.0, 0.0);
        editor.app.world.scene.get_mut(id).unwrap().transform.position = moved;
        editor.apply(Action::Save);

        // игра стартует из изменённой копии
        let file = scene.trim_start_matches("scenes/").trim_end_matches(".scene").to_string();
        let mut game = lantern::build_in(SaveData::in_memory(), &project).expect("the game builds from the modified copy");
        game.world.scenes().load(&file);
        game.run_headless(30, DT);
        let found = game.world.scene.find_by_name(mover).unwrap_or_else(|| panic!("{scene}: `{mover}` is gone after saving"));
        let now = game.world.scene.world_position(found);
        if mover == "Player" {
            assert!((now.x - moved.x).abs() < 1.0, "{scene}: the moved player is at {now}, expected near {moved}");
        } else {
            assert!((now.x - moved.x).abs() < 0.01, "{scene}: the moved object is at {now}, expected {moved}");
        }
        if scene.contains("level") {
            assert!(game.world.scene.find_by_name("Level").is_some());
        }
    }
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn every_prefab_instantiates_in_the_editor() {
    let project = temp_assets("prefabs");
    let mut editor = editor_on(&project, "scenes/level1.scene");
    let mut prefabs: Vec<String> = Vec::new();
    for dir in ["prefabs", "ui"] {
        for entry in std::fs::read_dir(project.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "prefab") {
                prefabs.push(format!("{dir}/{}", path.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    assert!(prefabs.len() >= 25, "{prefabs:?}");
    for reference in prefabs {
        let before = editor.app.world.scene.len();
        editor.apply(Action::Instantiate { reference: reference.clone(), position: None });
        let after = editor.app.world.scene.len();
        assert!(after > before, "`{reference}` did not instantiate");
    }
    let _ = std::fs::remove_dir_all(project);
}

/// Снимки проекта в редакторе (нужна видеокарта, поэтому `#[ignore]`):
/// `LANTERN_SHOTS=<каталог> cargo test -p lantern --features editor --test editor_assets -- --ignored`.
#[test]
#[ignore]
fn editor_screenshots_of_the_project() {
    let Ok(out) = std::env::var("LANTERN_SHOTS") else { return };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    for (file, scene, select) in [
        ("level_player.png", "scenes/level3.scene", "Player"),
        ("level_enemy.png", "scenes/level5.scene", "Brute"),
        ("menu_button.png", "scenes/menu.scene", "options"),
        ("boss.png", "scenes/level6.scene", "Boss"),
    ] {
        let mut editor = editor_on(Path::new(ASSETS), scene);
        let id = editor.app.world.scene.find_by_name(select).unwrap_or_else(|| panic!("no `{select}`"));
        editor.apply(Action::Select(Some(id)));
        editor.apply(Action::FrameSelected);
        let image = engine::editor::capture(&mut editor, 1600, 900, 6).expect("capture");
        image.save(out.join(file)).unwrap();
    }
}

/// Менеджер уровня и босс находят тайлмап по полю-ссылке, а не по имени «Tiles»:
/// объект переименовывают в редакторе, сцену сохраняют — уровень по-прежнему строит
/// сетку столкновений из него.
#[test]
fn renamed_tilemap_still_drives_the_level_after_saving() {
    use lantern::phys::Shared;
    use lantern::scripts::{boss::Boss, level_manager::LevelManager};
    let project = temp_assets("rename");
    for (scene, file) in [("scenes/level1.scene", "level1"), ("scenes/level6.scene", "level6")] {
        let mut editor = editor_on(&project, scene);
        let tiles = editor.app.world.scene.find_by_name("Tiles").unwrap_or_else(|| panic!("{scene}: Tiles"));
        editor.apply(Action::Rename { id: tiles, name: "Cave Walls".into() });
        editor.apply(Action::Save);
        let text = std::fs::read_to_string(project.join(scene)).unwrap();
        assert!(text.contains("Cave Walls") && !text.contains("\"Tiles\""), "{scene}: the rename is saved");

        let mut game = lantern::build_in(SaveData::in_memory(), &project).expect("the game builds from the modified copy");
        game.world.scenes().load(file);
        game.run_headless(10, DT);
        let level = game.world.scene.find_by_name("Level").expect("the level manager");
        let manager = game.world.scene.get_component::<LevelManager>(level).unwrap();
        let walls = manager.tiles.get(&game.world.scene).expect("the tiles reference resolves");
        assert_eq!(game.world.scene.get(walls).unwrap().name, "Cave Walls");
        let shared = game.world.resources.get::<Shared>().expect("the collision grid is built");
        let width = shared.grid.borrow().width;
        assert!(width > 20, "{scene}: the grid comes from the level tilemap (width {width})");
        if file == "level6" {
            let boss = game.world.scene.find_with::<Boss>()[0];
            let boss = game.world.scene.get_component::<Boss>(boss).unwrap();
            assert_eq!(boss.tiles.get(&game.world.scene), Some(walls), "the boss reference resolves too");
        }
    }
    let _ = std::fs::remove_dir_all(project);
}
