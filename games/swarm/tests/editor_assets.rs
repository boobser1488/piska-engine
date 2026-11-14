//! Проект в редакторе: сцена и префабы открываются, иерархия заполнена, объект можно
//! подвинуть и сохранить, а игра запускается из изменённой копии.
//! Запуск: `cargo test -p swarm --features editor --test editor_assets`.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::{Action, Editor};
use engine::prelude::*;
use swarm::scripts::game_manager::Game;

const DT: f32 = 1.0 / 60.0;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn temp_project() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("piska_swarm_editor_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"), &dir);
    dir
}

fn editor_on(assets: &Path) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    swarm::register(&mut app);
    engine::mount_assets(&mut app, assets).expect("assets open");
    Editor::new(app, EditorConfig::default().open_scene("scenes/arena.scene")).expect("arena.scene opens in the editor")
}

#[test]
fn the_arena_scene_opens_shows_its_structure_and_survives_an_edit() {
    let dir = temp_project();
    let mut editor = editor_on(&dir);
    let scene = &editor.app.world.scene;
    assert!(scene.len() >= 30, "the hierarchy is populated: {}", scene.len());
    for name in ["Main Camera", "ArenaFloor", "Drone", "ReactorCore", "GameManager", "HUD", "MenuScreen", "UpgradeScreen"] {
        assert!(scene.find_by_name(name).is_some(), "`{name}` is in the hierarchy");
    }
    // Дрон — экземпляр префаба; у менеджера по скрипту на поведение.
    let drone = scene.find_by_name("Drone").unwrap();
    assert!(scene.get(drone).unwrap().prefab().is_some_and(|p| p.contains("drone")));
    let manager = scene.find_by_name("GameManager").unwrap();
    assert!(scene.get(manager).unwrap().components().count() >= 6, "the manager carries the scripts");

    // Выделить, подвинуть, сохранить.
    editor.apply(Action::Select(Some(drone)));
    assert_eq!(editor.selection(), Some(drone));
    editor.app.world.scene.get_mut(drone).unwrap().transform.position = Vec3::new(2.5, -1.0, 0.0);
    let space = editor.app.world.scene.find_by_name("Space").unwrap();
    editor.app.world.scene.get_mut(space).unwrap().transform.position = Vec3::new(1.5, 0.5, 0.0);
    // Правка и ещё одно действие редактора (оно помечает сцену изменённой), затем сохранение.
    editor.apply(Action::CreateEmpty { parent: None });
    assert!(editor.is_dirty());
    editor.apply(Action::Save);
    assert!(!editor.is_dirty(), "the scene was saved");

    // Игра запускается из изменённой копии проекта и видит правку.
    let saved = std::fs::read_to_string(dir.join("scenes/arena.scene")).unwrap();
    let at = saved.find("name: \"Drone\"").expect("Drone is in the saved scene");
    assert!(saved[at..at + 400].contains("2.5"), "the saved file holds the new position: {}", &saved[at..at + 400]);
    let mut app = swarm::build_from(SaveData::in_memory(), &dir).expect("the game starts from the edited copy");
    app.run_headless(5, DT);
    // Дрон и реактор двигает сама игра (их кладёт симуляция), поэтому на загрузке проверяем фон.
    let space = app.world.scene.find_by_name("Space").unwrap();
    let pos = app.world.scene.get(space).unwrap().transform.position;
    assert!((pos.x - 1.5).abs() < 1e-3 && (pos.y - 0.5).abs() < 1e-3, "the saved edit is in the scene: {pos:?}");
    assert!(app.world.resources.get::<Game>().is_some(), "the manager script built the game");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_prefab_can_be_dropped_into_the_scene_from_the_editor() {
    let dir = temp_project();
    let mut editor = editor_on(&dir);
    let mut prefabs = Vec::new();
    for folder in ["prefabs", "ui"] {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else if path.extension().is_some_and(|e| e == "prefab") {
                    out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        walk(&dir.join(folder), &dir, &mut prefabs);
    }
    assert!(prefabs.len() >= 35, "{} prefabs", prefabs.len());
    for prefab in prefabs {
        let before = editor.app.world.scene.len();
        editor.apply(Action::Instantiate { reference: prefab.clone(), position: Some(Vec3::new(0.0, 5.0, 0.0)) });
        assert!(editor.app.world.scene.len() > before, "{prefab} adds objects to the scene");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_project_folders_hold_scenes_prefabs_ui_audio_data_and_scripts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (folder, ext) in [
        ("assets/scenes", "scene"),
        ("assets/prefabs", "prefab"),
        ("assets/ui", "prefab"),
        ("assets/audio", "wav"),
        ("assets/data", "ron"),
        ("src/scripts", "rs"),
    ] {
        fn has(dir: &Path, ext: &str) -> bool {
            std::fs::read_dir(dir).unwrap().flatten().any(|e| {
                let p = e.path();
                if p.is_dir() { has(&p, ext) } else { p.extension().is_some_and(|x| x == ext) }
            })
        }
        assert!(has(&root.join(folder), ext), "{folder} has *.{ext} files");
    }
}

/// Вид игры находит детей дрона и реактора по полям-ссылкам, а не по именам: их
/// переименовывают в редакторе, сцену сохраняют — игра берёт те же объекты.
#[test]
fn renamed_prefab_children_are_still_found_after_saving() {
    let dir = temp_project();
    let mut editor = editor_on(&dir);
    let scene = &editor.app.world.scene;
    let drone = scene.find_by_name("Drone").unwrap();
    let core = scene.find_by_name("ReactorCore").unwrap();
    let renames = [
        (scene.find_child(core, "Ring").expect("reactor ring"), "Outer Ring"),
        (scene.find_child(drone, "Muzzle").expect("drone muzzle"), "Barrel Flash"),
        (scene.find_child(drone, "SlowField").expect("drone slow field"), "Stasis Bubble"),
    ];
    for (id, name) in renames {
        editor.apply(Action::Rename { id, name: name.into() });
    }
    editor.apply(Action::Save);

    let mut app = swarm::build_from(SaveData::in_memory(), &dir).expect("the game starts from the edited copy");
    app.run_headless(5, DT);
    let game = app.world.resources.get::<Game>().expect("the manager script built the game");
    let names: Vec<String> = game
        .view
        .linked_parts()
        .iter()
        .map(|id| app.world.scene.get(*id).map(|o| o.name.clone()).unwrap_or_default())
        .collect();
    assert_eq!(names, ["Outer Ring", "Barrel Flash", "Stasis Bubble"]);
    let _ = std::fs::remove_dir_all(&dir);
}
