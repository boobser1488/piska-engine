//! Модели Piska Modeler в движке: `.pmodel` — меш с текстурой (из каталога и из пака),
//! экспорт `.glb` читается загрузчиком движка.

use engine_assets::{load_mesh_bytes, AssetDatabase, AssetPack, Guid};
use engine_modeler::{bake, export, format, samples};

fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

fn close(a: [f32; 3], b: engine_core::math::Vec3) -> bool {
    (a[0] - b.x).abs() < 1e-4 && (a[1] - b.y).abs() < 1e-4 && (a[2] - b.z).abs() < 1e-4
}

#[test]
fn pmodel_imports_as_the_baked_mesh() {
    for (name, doc) in samples::all() {
        let text = format::to_string(&doc);
        let mesh = load_mesh_bytes(&format!("{name}.pmodel"), text.as_bytes(), &no_files).expect("the engine reads .pmodel");
        let baked = bake(&doc);
        assert_eq!(mesh.vertex_count(), baked.positions.len(), "{name}");
        assert_eq!(mesh.triangle_count(), baked.triangle_count(), "{name}");
        assert_eq!(mesh.positions, baked.positions, "{name}: positions survive the text exactly");
        assert_eq!(mesh.uvs, baked.uvs, "{name}");
        let (min, max) = mesh.bounds();
        let (bmin, bmax) = baked.bounds();
        assert!(close(bmin, min) && close(bmax, max), "{name}: bounds in metres");
    }
    let err = load_mesh_bytes("x.pmodel", b"(format: \"piska-model\", version: 1, model: ())", &no_files)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Piska Modeler"), "{err}");
}

#[test]
fn glb_export_is_read_by_the_engine_loader() {
    for (name, doc) in samples::all() {
        let glb = export::to_glb(&doc);
        let mesh = load_mesh_bytes(&format!("{name}.glb"), &glb, &no_files).expect("the engine reads the exported .glb");
        let baked = bake(&doc);
        assert_eq!(mesh.vertex_count(), baked.positions.len(), "{name}: vertices");
        assert_eq!(mesh.triangle_count(), baked.triangle_count(), "{name}: triangles");
        let (min, max) = mesh.bounds();
        let (bmin, bmax) = baked.bounds();
        // Узлы групп (перенос + поворот) применены загрузчиком: границы совпадают с запеканием.
        assert!(close(bmin, min) && close(bmax, max), "{name}: {min} {max} vs {bmin:?} {bmax:?}");
    }
}

#[test]
fn database_and_pack_give_mesh_and_texture() {
    let dir = std::env::temp_dir().join(format!("pmodel-assets-{}", Guid::generate()));
    std::fs::create_dir_all(dir.join("models")).unwrap();
    let doc = samples::wooden_crate();
    format::save(&doc, dir.join("models/crate.pmodel")).unwrap();

    let db = AssetDatabase::open(&dir).unwrap();
    let guid = db.guid_of("models/crate.pmodel").unwrap();
    let meta = db.meta(guid).unwrap();
    assert_eq!(meta.importer, "Mesh", "a .pmodel is a mesh asset");
    assert!(format!("{:?}", meta.settings).contains("Nearest"), "pixel textures stay crisp");
    assert!(db.has_embedded_texture(guid));
    assert_eq!(db.load_mesh(guid).unwrap().triangle_count(), 12);
    let png = db.read_image(guid).unwrap().into_owned();
    assert_eq!(png, bake(&doc).texture_png().unwrap(), "the atlas comes from the file");

    let mut builder = AssetPack::builder();
    builder.add_dir(&dir).unwrap();
    let packed = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
    assert_eq!(packed.load_mesh(guid).unwrap().triangle_count(), 12, "loads from a pack (web, Android)");
    assert_eq!(packed.read_image(guid).unwrap().into_owned(), png);
    std::fs::remove_dir_all(dir).unwrap();
}
