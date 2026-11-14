//! Пункты «Edit in Modeler» / «Edit a Copy in Modeler»: какой пункт у какого файла, активен
//! ли он без программы Piska Modeler, модель объекта для меню Hierarchy и поля Mesh,
//! «Edit a Copy» без программы ничего не создаёт и объясняет, что её нет.
//! Настоящая программа здесь не запускается.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use engine::editor::{Action, Editor, EditorConfig};
use engine_assets::AssetDatabase;
use engine_render::MeshRenderer;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
/// Тесты меняют `PISKA_MODELER` — по одному.
static ENV: Mutex<()> = Mutex::new(());

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap()
}

/// `PISKA_MODELER`: существующий файл — «программа есть» (её не запускают), несуществующий —
/// «нет».
fn modeler(installed: bool) -> PathBuf {
    let path = if installed {
        std::env::current_exe().unwrap()
    } else {
        std::env::temp_dir().join("no-such-piska-modeler.exe")
    };
    std::env::set_var("PISKA_MODELER", &path);
    path
}

#[test]
fn models_get_edit_and_foreign_meshes_get_edit_a_copy() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    for installed in [true, false] {
        modeler(installed);
        assert_eq!(Editor::modeler_menu_entry(Path::new("models/crate.pmodel")), Some(("Edit in Modeler", installed)));
        for mesh in ["a.obj", "b.GLB", "c.gltf", "d.bbmodel"] {
            assert_eq!(Editor::modeler_menu_entry(Path::new(mesh)), Some(("Edit a Copy in Modeler", installed)), "{mesh}");
        }
        for other in ["e.png", "f.scene", "g"] {
            assert_eq!(Editor::modeler_menu_entry(Path::new(other)), None, "{other}");
        }
    }
    std::env::remove_var("PISKA_MODELER");
}

#[test]
fn objects_with_a_model_mesh_offer_it_to_the_modeler() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let mut editor = editor();
    let (model, foreign) = {
        let db = editor.app.world.resources.get::<AssetDatabase>().expect("assets are mounted");
        (db.handle::<engine_assets::Mesh>("models/crate.pmodel").unwrap(), db.handle::<engine_assets::Mesh>("models/torus.obj").unwrap())
    };
    let scene = &mut editor.app.world.scene;
    let crate_object = scene.spawn("Crate");
    scene.get_mut(crate_object).unwrap().add(MeshRenderer::asset(model));
    let torus = scene.spawn("Torus");
    scene.get_mut(torus).unwrap().add(MeshRenderer::asset(foreign));
    let cube = scene.spawn("Cube");
    scene.get_mut(cube).unwrap().add(MeshRenderer::cube());

    let file = editor.object_model_file(crate_object).expect("the crate draws a .pmodel");
    assert!(file.ends_with("models/crate.pmodel") || file.ends_with("models\\crate.pmodel"), "{}", file.display());
    let file = editor.object_model_file(torus).expect("the torus draws an .obj");
    assert_eq!(file.extension().and_then(|e| e.to_str()), Some("obj"));
    assert_eq!(editor.object_model_file(cube), None, "a built-in shape has no model file");
}

#[test]
fn edit_a_copy_without_the_modeler_creates_nothing_and_explains() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("piska-edit-copy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("ящик с пробелом.obj");
    std::fs::write(&source, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let mut editor = editor();
    modeler(false);
    editor.apply(Action::EditCopyInModeler(source.clone()));
    assert!(editor.is_modeler_missing_shown(), "no program: the explanation window, not silence");
    assert!(!dir.join("ящик с пробелом.pmodel").exists(), "nothing is created without the program");
    std::env::remove_var("PISKA_MODELER");
    let _ = std::fs::remove_dir_all(&dir);
}
