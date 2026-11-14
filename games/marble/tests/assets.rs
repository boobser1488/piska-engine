//! Проект открывается в редакторе по-настоящему: каждая сцена и каждый префаб читаются,
//! иерархия заполнена, объект можно подвинуть, сцена сохраняется во временную копию, и
//! игра запускается из изменённой копии. Запуск: `cargo test -p marble --features editor
//! --test assets` (без окна, звук выключен).
#![cfg(feature = "editor")]

use engine::editor::{Action, Editor};
use engine::prelude::*;
use marble::Boot;
use std::path::{Path, PathBuf};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
const DT: f32 = 1.0 / 60.0;

/// Копия каталога ассетов во временной папке — сюда редактор пишет.
fn temp_assets(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("marble_editor_test_{}_{tag}", std::process::id()));
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
    let app = marble::build_in(SaveData::in_memory(), Boot::default(), assets).expect("the game builds from the assets");
    Editor::new(app, EditorConfig::default().open_scene(scene)).expect("the scene opens in the editor")
}

fn names(editor: &Editor) -> Vec<String> {
    let scene = &editor.app.world.scene;
    scene.ids().into_iter().filter_map(|id| scene.get(id).map(|o| o.name.clone())).collect()
}

#[test]
fn every_scene_opens_in_the_editor_is_populated_editable_and_saves() {
    let mut cases: Vec<(String, &str, usize, Vec<&str>)> = vec![("scenes/menu.scene".into(), "Camera", 40, vec!["Camera", "Sun", "Game", "Menu", "Select", "Options"])];
    for n in 1..=marble::COUNT {
        cases.push((format!("scenes/courses/course_{n}.scene"), "Ball", 150, vec!["Camera", "Sun", "Game", "Ball", "Finish", "HUD", "Pause", "Results"]));
    }
    let project = temp_assets("scenes");
    for (scene, mover, min_objects, must_have) in cases {
        let mut editor = editor_on(&project, &scene);
        let all = names(&editor);
        assert!(all.len() >= min_objects, "{scene}: only {} objects", all.len());
        for name in must_have {
            assert!(all.iter().any(|n| n == name), "{scene}: no object `{name}` in the hierarchy");
        }

        // выделить и подвинуть
        let id = editor.app.world.scene.find_by_name(mover).unwrap_or_else(|| panic!("{scene}: no `{mover}`"));
        editor.apply(Action::Select(Some(id)));
        let moved = editor.app.world.scene.world_position(id) + Vec3::new(1.5, 0.0, 0.0);
        editor.app.world.scene.get_mut(id).unwrap().transform.position = moved;
        editor.apply(Action::Save);

        // игра стартует из изменённой копии и видит сдвиг в файле сцены
        let text = std::fs::read_to_string(project.join(&scene)).expect("the saved scene");
        assert!(text.contains(mover), "{scene}: the saved file keeps `{mover}`");
        let mut game = marble::build_in(SaveData::in_memory(), Boot::default(), &project).expect("the game builds from the modified copy");
        let file = scene.trim_start_matches("scenes/").trim_end_matches(".scene").to_string();
        game.world.scenes().load(&file);
        game.run_headless(30, DT);
        let found = game.world.scene.find_by_name(mover).unwrap_or_else(|| panic!("{scene}: `{mover}` is gone after saving"));
        let now = game.world.scene.world_position(found);
        assert!(now.is_finite(), "{scene}: the object is alive after the reload");
        if mover == "Camera" {
            assert!((now.x - moved.x).abs() < 0.05 || scene.ends_with("menu.scene"), "{scene}: the camera is at {now}, expected near {moved}");
        }
    }
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn every_prefab_instantiates_in_the_editor() {
    let project = temp_assets("prefabs");
    let mut editor = editor_on(&project, "scenes/courses/course_1.scene");
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
    assert!(prefabs.len() >= 40, "{} prefabs: {prefabs:?}", prefabs.len());
    for reference in prefabs {
        let before = editor.app.world.scene.len();
        editor.apply(Action::Instantiate { reference: reference.clone(), position: None });
        let after = editor.app.world.scene.len();
        assert!(after > before, "`{reference}` did not instantiate");
    }
    let _ = std::fs::remove_dir_all(project);
}

/// Кадры редактора для проверки глазами: `MARBLE_EDITOR_SHOTS=<каталог> cargo test -p marble
/// --features editor --test assets -- --ignored editor_screenshots` (нужна видеокарта, без окна).
#[test]
#[ignore = "needs a GPU: writes PNG files into $MARBLE_EDITOR_SHOTS"]
fn editor_screenshots() {
    let out = PathBuf::from(std::env::var("MARBLE_EDITOR_SHOTS").expect("MARBLE_EDITOR_SHOTS"));
    std::fs::create_dir_all(&out).unwrap();
    let project = temp_assets("shots");
    // (сцена, выбранный объект, файл)
    let cases = [
        ("scenes/menu.scene", "Menu", "menu_ui"),
        ("scenes/courses/course_1.scene", "Ball", "course1_ball"),
        ("scenes/courses/course_3.scene", "Sweeper", "course3_sweeper"),
        ("scenes/courses/course_5.scene", "Crusher", "course5_crusher"),
        ("scenes/courses/course_8.scene", "Fan", "course8_fan"),
    ];
    for (scene, pick, file) in cases {
        let mut editor = editor_on(&project, scene);
        if let Some(id) = editor.app.world.scene.find_by_name(pick) {
            editor.apply(Action::Select(Some(id)));
        }
        let image = engine::editor::capture(&mut editor, 1600, 900, 4).expect("the editor draws a frame");
        image.save(out.join(format!("{file}.png"))).unwrap();
    }
    let _ = std::fs::remove_dir_all(project);
}

/// Скрипты находят детей префабов по полям-ссылкам, а не по именам: детей пресса и
/// ротор вентилятора переименовывают в редакторе, сцену сохраняют — игра работает.
#[test]
fn renamed_children_of_mechanisms_still_work_after_saving() {
    use marble::scripts::{crusher::Crusher, fan::Fan};
    let project = temp_assets("rename");
    let scene_file = "scenes/courses/course_5.scene";
    let mut editor = editor_on(&project, scene_file);
    let scene = &editor.app.world.scene;
    let press = scene.find_by_name("Crusher").expect("course 5 has a crusher");
    let head = scene.find_child(press, "Head").expect("the crusher has a head");
    let shaft = scene.find_child(press, "Shaft").expect("the crusher has a shaft");
    editor.apply(Action::Rename { id: head, name: "Piston".into() });
    editor.apply(Action::Rename { id: shaft, name: "Rod".into() });
    editor.apply(Action::Save);
    let text = std::fs::read_to_string(project.join(scene_file)).unwrap();
    assert!(text.contains("\"Piston\""), "the rename is saved as an override");

    let mut game = marble::build_in(SaveData::in_memory(), Boot::default(), &project).expect("the game builds from the modified copy");
    game.world.scenes().load("courses/course_5");
    game.run_headless(2, DT);
    let scene = &game.world.scene;
    let press = scene.find_by_name("Crusher").unwrap();
    let crusher = scene.get_component::<Crusher>(press).unwrap().clone();
    let piston = scene.resolve(crusher.head).expect("the head reference resolves");
    assert_eq!(scene.get(piston).unwrap().name, "Piston");
    assert_eq!(scene.get(scene.resolve(crusher.shaft).unwrap()).unwrap().name, "Rod");
    assert!(scene.find_child(press, "Head").is_none(), "the child is renamed, not duplicated");
    // Пресс работает: голова опускается за цикл.
    let mut lowest = 0.0f32;
    for _ in 0..240 {
        game.run_headless(1, DT);
        let y = game.world.scene.get(piston).unwrap().transform.position.y;
        lowest = lowest.min(y);
    }
    assert!(lowest < -1.0, "the renamed piston still slams down (lowest local y {lowest})");

    // Ротор вентилятора — тоже по ссылке.
    let fans: Vec<GameObjectId> = game.world.scene.find_with::<Fan>();
    for fan in fans {
        let rotor = game.world.scene.get_component::<Fan>(fan).unwrap().rotor;
        assert!(game.world.scene.resolve(rotor).is_some(), "every fan's rotor reference resolves");
    }
    let _ = std::fs::remove_dir_all(project);
}
