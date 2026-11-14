//! Проект открывается в редакторе по-настоящему: обе сцены и каждый префаб читаются,
//! иерархия заполнена, объект можно подвинуть, сцена сохраняется во временную копию, и
//! игра запускается из изменённой копии. `cargo test -p tashbazar --features editor --test assets`
//! (без окна, звук выключен).
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::{Action, Editor};
use engine::prelude::*;
use tashbazar::scripts::game_manager::{GameManager, Screen};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
const DT: f32 = 1.0 / 64.0;

/// Копия каталога ассетов во временной папке — сюда редактор пишет.
fn temp_assets(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tashbazar_editor_test_{}_{tag}", std::process::id()));
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
    let app = tashbazar::build_in(SaveData::in_memory(), assets, scene).expect("the game builds from the assets");
    Editor::new(app, EditorConfig::default().open_scene(scene)).expect("the scene opens in the editor")
}

fn names(editor: &Editor) -> Vec<String> {
    let scene = &editor.app.world.scene;
    scene.ids().into_iter().filter_map(|id| scene.get(id).map(|o| o.name.clone())).collect()
}

#[test]
fn both_scenes_open_in_the_editor_are_populated_editable_and_save() {
    let cases: [(&str, &str, usize, &[&str]); 2] = [
        (
            tashbazar::MAP_SCENE,
            "Crate Long",
            700,
            &["Ground", "Buildings", "Camel Bridge", "Aqueduct", "Site A — Cistern", "Site B — Caravanserai", "Player", "Game", "HUD", "Main Menu", "Movement Course", "Menu Camera", "Sun", "Environment"],
        ),
        (tashbazar::GYM_SCENE, "Moving Platform", 120, &["Gym", "Steps", "Jump Boxes", "Slopes", "Player", "Game", "HUD", "Pause"]),
    ];
    let project = temp_assets("scenes");
    for (scene, mover, min_objects, must_have) in cases {
        let mut editor = editor_on(&project, scene);
        let all = names(&editor);
        assert!(all.len() >= min_objects, "{scene}: only {} objects", all.len());
        for name in must_have {
            assert!(all.iter().any(|n| n == name), "{scene}: no object `{name}` in the hierarchy");
        }
        // Выделить и подвинуть (ящик полосы / платформу полигона), сохранить.
        let id = editor
            .app
            .world
            .scene
            .ids()
            .into_iter()
            .find(|id| editor.app.world.scene.get(*id).is_some_and(|o| o.name == mover || o.name == "crate_long"))
            .unwrap_or_else(|| panic!("{scene}: no `{mover}`"));
        editor.apply(Action::Select(Some(id)));
        let moved = editor.app.world.scene.world_position(id) + Vec3::new(0.0, 0.0, 0.5);
        editor.app.world.scene.get_mut(id).unwrap().transform.position = moved;
        editor.apply(Action::Save);

        // Игра стартует из изменённой копии и видит сдвиг.
        let mut game = tashbazar::build_in(SaveData::in_memory(), &project, scene).expect("the game builds from the modified copy");
        game.run_headless(4, DT);
        let found = game
            .world
            .scene
            .ids()
            .into_iter()
            .filter(|id| game.world.scene.get(*id).is_some_and(|o| o.name == mover || o.name == "crate_long"))
            .map(|id| game.world.scene.world_position(id))
            .any(|p| (p - moved).length() < 0.05);
        assert!(found, "{scene}: the moved object is at {moved} after reloading");
    }
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn every_prefab_instantiates_in_the_editor() {
    let project = temp_assets("prefabs");
    let mut editor = editor_on(&project, tashbazar::GYM_SCENE);
    let mut prefabs: Vec<String> = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "prefab") {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    for dir in ["prefabs", "ui"] {
        walk(&project.join(dir), &project, &mut prefabs);
    }
    assert!(prefabs.len() >= 50, "{} prefabs: {prefabs:?}", prefabs.len());
    for reference in prefabs {
        let before = editor.app.world.scene.len();
        editor.apply(Action::Instantiate { reference: reference.clone(), position: None });
        let after = editor.app.world.scene.len();
        assert!(after > before, "`{reference}` did not instantiate");
    }
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn the_map_plays_from_the_editor_play_button() {
    let project = temp_assets("play");
    let mut editor = editor_on(&project, tashbazar::MAP_SCENE);
    editor.apply(Action::Play);
    for _ in 0..10 {
        editor.app.tick(DT);
    }
    let game = editor.app.world.scene.find_by_name("Game").expect("Game");
    assert_eq!(editor.app.world.scene.get_component::<GameManager>(game).unwrap().screen, Screen::Menu);
    editor.app.world.send_message(game, "play", Value::Nil);
    for _ in 0..30 {
        editor.app.tick(DT);
    }
    let player = editor.app.world.scene.find_by_name("Player").unwrap();
    assert!(editor.app.world.scene.is_active(player), "the player is spawned");
    let p = editor.app.world.scene.world_position(player);
    assert!(p.z < -45.0 && p.y.abs() < 0.05, "at the attackers' spawn, standing: {p}");
    assert!(editor.app.world.input.cursor_capture_requested(), "the game asks for the cursor");
    let _ = std::fs::remove_dir_all(project);
}
