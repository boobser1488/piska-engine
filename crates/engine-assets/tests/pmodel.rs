//! Модели Piska Modeler в движке: `.pmodel` — меш с текстурой (из каталога и из пака),
//! экспорт `.glb` читается загрузчиком движка.
//!
//! Piska Modeler — отдельный репозиторий (`modeler/`), поэтому тест не зависит от его кода:
//! образцы `assets/models/*.pmodel` и `tests/fixtures/rover.glb` записал сам редактор моделей
//! (`cargo run --manifest-path modeler/Cargo.toml -p engine-modeler --example gen_samples --
//! assets/models --glb crates/engine-assets/tests/fixtures`).

use std::path::Path;

use engine_assets::{load_mesh_bytes, AssetDatabase, AssetPack, Guid};

const MODELS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models");

fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(MODELS).join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn pmodel_samples_import_as_meshes() {
    // Треугольники каждого образца (печатает генератор образцов).
    for (name, triangles) in [("crate", 12), ("lantern", 134), ("pine", 56), ("rover", 192)] {
        let file = format!("{name}.pmodel");
        let mesh = load_mesh_bytes(&file, &read(&file), &no_files).expect("the engine reads .pmodel");
        assert_eq!(mesh.triangle_count(), triangles, "{name}");
        assert_eq!(mesh.vertex_count(), mesh.normals.len(), "{name}");
        assert_eq!(mesh.vertex_count(), mesh.uvs.len(), "{name}");
        assert!(mesh.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])), "{name}: UVs in the atlas");
        let (min, max) = mesh.bounds();
        // Метры: модели размером с предмет (16 единиц модели = 1 м).
        assert!(min.y >= -0.01 && max.y < 2.0 && max.y > 0.3, "{name}: {min} {max}");
    }
    let err = load_mesh_bytes("x.pmodel", b"(format: \"piska-model\", version: 1, model: ())", &no_files)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Piska Modeler"), "{err}");
}

#[cfg(feature = "gltf")]
#[test]
fn exported_glb_is_read_by_the_engine_loader() {
    let glb = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/rover.glb")).unwrap();
    let from_glb = load_mesh_bytes("rover.glb", &glb, &no_files).expect("the engine reads the exported .glb");
    let from_pmodel = load_mesh_bytes("rover.pmodel", &read("rover.pmodel"), &no_files).unwrap();
    assert_eq!(from_glb.vertex_count(), from_pmodel.vertex_count());
    assert_eq!(from_glb.triangle_count(), from_pmodel.triangle_count());
    // Узлы групп (перенос + поворот) применены загрузчиком: границы совпадают с запеканием.
    let (a, b) = (from_glb.bounds(), from_pmodel.bounds());
    assert!((a.0 - b.0).length() < 1e-4 && (a.1 - b.1).length() < 1e-4, "{a:?} vs {b:?}");
}

#[test]
fn database_and_pack_give_mesh_and_texture() {
    let dir = std::env::temp_dir().join(format!("pmodel-assets-{}", Guid::generate()));
    std::fs::create_dir_all(dir.join("models")).unwrap();
    std::fs::write(dir.join("models/crate.pmodel"), read("crate.pmodel")).unwrap();

    let db = AssetDatabase::open(&dir).unwrap();
    let guid = db.guid_of("models/crate.pmodel").unwrap();
    let meta = db.meta(guid).unwrap();
    assert_eq!(meta.importer, "Mesh", "a .pmodel is a mesh asset");
    assert!(format!("{:?}", meta.settings).contains("Nearest"), "pixel textures stay crisp");
    assert!(db.has_embedded_texture(guid));
    assert_eq!(db.load_mesh(guid).unwrap().triangle_count(), 12);
    let png = db.read_image(guid).unwrap().into_owned();
    assert_eq!(&png[1..4], b"PNG", "the atlas comes from the file as a PNG");

    let mut builder = AssetPack::builder();
    builder.add_dir(&dir).unwrap();
    let packed = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
    assert_eq!(packed.load_mesh(guid).unwrap().triangle_count(), 12, "loads from a pack (web, Android)");
    assert_eq!(packed.read_image(guid).unwrap().into_owned(), png);
    std::fs::remove_dir_all(dir).unwrap();
}
