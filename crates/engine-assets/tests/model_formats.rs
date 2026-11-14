//! Сторонние форматы моделей в базе ассетов: `.bbmodel`, модель Minecraft, `.vox`, `.ply`,
//! `.stl` — импортёр `Mesh`, свой атлас для рендерера, настройки импорта в `.meta`, пак,
//! и никаких чтений вне каталога ассетов. Образцы — из `crates/engine-model-formats/tests/fixtures`.

use std::path::{Path, PathBuf};

use engine_assets::{load_mesh, AssetDatabase, AssetMeta, AssetPack, Guid};
use engine_core::reflect::Value;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine-model-formats/tests/fixtures");

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("model-formats-{}", Guid::generate()));
        std::fs::create_dir_all(dir.join("assets/models/block")).unwrap();
        Self(dir)
    }

    fn assets(&self) -> PathBuf {
        self.0.join("assets")
    }

    fn copy(&self, fixture: &str, to: &str) {
        std::fs::copy(Path::new(FIXTURES).join(fixture), self.assets().join(to)).unwrap();
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn third_party_models_are_mesh_assets_with_their_own_atlas() {
    let temp = Temp::new();
    temp.copy("asym.bbmodel", "models/asym.bbmodel");
    temp.copy("slab.model.json", "models/slab.model.json");
    temp.copy("block/faces.png", "models/block/faces.png");
    temp.copy("quad_ascii.ply", "models/quad.ply");
    temp.copy("wedge_binary.stl", "models/wedge.stl");
    std::fs::write(temp.assets().join("models/stats.json"), br#"{"elements": [1, 2], "hp": 3}"#).unwrap();

    let db = AssetDatabase::open(temp.assets()).unwrap();
    let guid = |p: &str| db.guid_of(p).unwrap_or_else(|| panic!("{p}"));
    for path in ["models/asym.bbmodel", "models/slab.model.json", "models/quad.ply", "models/wedge.stl"] {
        let meta = db.meta(guid(path)).unwrap();
        assert_eq!(meta.importer, "Mesh", "{path}");
        assert_eq!(meta.settings.get("scale"), Some(&Value::Float(1.0)), "{path}: settings are explicit in .meta");
    }
    assert_eq!(db.meta(guid("models/stats.json")).unwrap().importer, "Text", "data .json files stay data");
    assert_eq!(db.meta(guid("models/asym.bbmodel")).unwrap().settings.get("filter"), Some(&Value::Enum("Nearest".into())));
    assert_eq!(db.meta(guid("models/quad.ply")).unwrap().settings.get("up_axis"), Some(&Value::Enum("Z".into())));

    let asym = guid("models/asym.bbmodel");
    let mesh = db.load_mesh(asym).unwrap();
    assert!(mesh.triangle_count() > 40);
    assert!(db.has_embedded_texture(asym));
    let png = db.read_image(asym).unwrap().into_owned();
    assert_eq!(&png[1..4], b"PNG");

    // Модель Minecraft берёт внешнюю текстуру рядом с собой (через базу ассетов).
    let slab = guid("models/slab.model.json");
    assert_eq!(db.load_mesh(slab).unwrap().bounds().1.y, 1.0);
    let info = db.inspect_mesh(slab).unwrap();
    assert_eq!(info.files, ["block/faces.png"]);

    assert!(db.has_embedded_texture(guid("models/quad.ply")), "a PLY with colours has an atlas");
    assert!(!db.has_embedded_texture(guid("models/wedge.stl")), "an STL has none: the MeshRenderer texture is used");
    assert!(engine_assets::has_embedded_atlas("a/b.bbmodel") && !engine_assets::has_embedded_atlas("a/b.stl"));

    // Тот же результат из пака (веб, Android).
    let mut builder = AssetPack::builder();
    builder.add_dir(temp.assets()).unwrap();
    let packed = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
    assert_eq!(packed.load_mesh(asym).unwrap(), mesh);
    assert_eq!(packed.read_image(asym).unwrap().into_owned(), png);
    assert_eq!(packed.load_mesh(slab).unwrap(), db.load_mesh(slab).unwrap());
}

#[test]
fn import_settings_from_meta_change_the_mesh() {
    let temp = Temp::new();
    temp.copy("asym.bbmodel", "models/asym.bbmodel");
    let db = AssetDatabase::open(temp.assets()).unwrap();
    let guid = db.guid_of("models/asym.bbmodel").unwrap();
    let plain = db.load_mesh(guid).unwrap();

    let meta_path = temp.assets().join("models/asym.bbmodel.meta");
    let mut meta: AssetMeta = ron::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    meta.settings.insert("scale".into(), Value::Float(2.0));
    meta.settings.insert("double_sided".into(), Value::Bool(true));
    meta.settings.insert("merge_by_texture".into(), Value::Bool(false));
    std::fs::write(&meta_path, ron::to_string(&meta).unwrap()).unwrap();
    let db = AssetDatabase::open(temp.assets()).unwrap();
    let scaled = db.load_mesh(guid).unwrap();
    assert_eq!(scaled.triangle_count(), 2 * plain.triangle_count());
    assert!((scaled.bounds().1 - plain.bounds().1 * 2.0).length() < 1e-4);
    let names: Vec<&str> = scaled.submeshes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["faces.png", "second.png", "anim.png"]);
}

#[test]
fn texture_paths_never_leave_the_assets_folder() {
    let temp = Temp::new();
    // Картинка вне каталога ассетов — рядом с ним.
    std::fs::copy(Path::new(FIXTURES).join("block/faces.png"), temp.0.join("secret.png")).unwrap();
    let model = br#"{"meta":{"format_version":"4.5"},
        "elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"uv":[0,0,16,16],"texture":0}}}],
        "textures":[{"name":"secret","relative_path":"../../secret.png"}]}"#;
    std::fs::write(temp.assets().join("models/escape.bbmodel"), model).unwrap();
    let db = AssetDatabase::open(temp.assets()).unwrap();
    let guid = db.guid_of("models/escape.bbmodel").unwrap();
    let png = db.read_image(guid).unwrap();
    let atlas = engine_assets::model_formats::decode_png(&png).unwrap();
    let secret = engine_assets::model_formats::decode_png(&std::fs::read(temp.0.join("secret.png")).unwrap()).unwrap();
    assert_ne!(atlas, secret, "the file outside assets/ must not be read; a checkerboard is used");
    // Без базы (путь на диске) — тоже только рядом с моделью.
    assert!(load_mesh(temp.assets().join("models/escape.bbmodel")).is_ok());
}

#[test]
fn unsupported_formats_say_what_to_do() {
    let temp = Temp::new();
    std::fs::write(temp.assets().join("models/robot.fbx"), b"Kaydara FBX Binary").unwrap();
    let err = load_mesh(temp.assets().join("models/robot.fbx")).unwrap_err().to_string();
    assert!(err.contains("FBX is not supported") && err.contains("glTF"), "{err}");
    let err = engine_assets::load_mesh_bytes("x.blend", b"BLENDER", &|_| None).unwrap_err().to_string();
    assert!(err.contains("Blender"), "{err}");
}
