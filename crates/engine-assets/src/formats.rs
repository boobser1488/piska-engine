//! Сторонние форматы моделей (крейт `engine-model-formats`): `.bbmodel`, модель Minecraft
//! (`.model.json` или `.json` с импортёром `Mesh`), `.vox`, `.ply`, `.stl` — меш и его
//! атлас для рендерера, настройки импорта из `.meta` (см. `docs/api/model-formats.md`).

use std::collections::BTreeMap;

use engine_core::reflect::Value;
use engine_model_formats::{Format, Imported, Options, UpAxis};

use crate::mesh::{MeshData, SubMesh};

/// Читается ли файл этим импортёром (по имени; `.json` — только `.model.json`).
pub fn is_model_format(name: &str) -> bool {
    Format::from_file_name(name).is_some()
}

/// Настройки импорта из `.meta` → [`Options`]. Неизвестные и неверные значения — по умолчанию.
pub fn import_options(settings: &BTreeMap<String, Value>) -> Options {
    let mut options = Options::default();
    let flag = |key: &str, default: bool| match settings.get(key) {
        Some(Value::Bool(b)) => *b,
        _ => default,
    };
    if let Some(scale) = match settings.get("scale") {
        Some(Value::Float(f)) => Some(*f),
        Some(Value::Int(i)) => Some(*i as f32),
        _ => None,
    } {
        options.scale = scale;
    }
    options.up = match settings.get("up_axis") {
        Some(Value::Enum(axis) | Value::Str(axis)) if axis.eq_ignore_ascii_case("z") => Some(UpAxis::Z),
        Some(Value::Enum(axis) | Value::Str(axis)) if axis.eq_ignore_ascii_case("y") => Some(UpAxis::Y),
        _ => None,
    };
    options.flip_x = flag("flip_x", false);
    options.bake_groups = flag("bake_groups", true);
    options.merge_by_texture = flag("merge_by_texture", true);
    options.double_sided = flag("double_sided", false);
    options
}

/// Настройки импорта нового файла стороннего формата: явные значения по умолчанию
/// (видны и правятся в `.meta`), «ближайший» фильтр для пиксельных текстур.
pub fn default_settings(name: &str) -> BTreeMap<String, Value> {
    let mut settings = BTreeMap::new();
    let Some(format) = Format::from_file_name(name) else { return settings };
    if format.pixel_art() {
        settings.insert("filter".to_string(), Value::Enum("Nearest".to_string()));
    }
    settings.insert("scale".to_string(), Value::Float(1.0));
    let up = match format.default_up() {
        UpAxis::Y => "Y",
        UpAxis::Z => "Z",
    };
    settings.insert("up_axis".to_string(), Value::Enum(up.to_string()));
    settings.insert("flip_x".to_string(), Value::Bool(false));
    settings.insert("bake_groups".to_string(), Value::Bool(true));
    settings.insert("merge_by_texture".to_string(), Value::Bool(true));
    settings.insert("double_sided".to_string(), Value::Bool(false));
    settings
}

fn read(name: &str, bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>, options: &Options) -> Result<Imported, String> {
    let imported = engine_model_formats::import(name, bytes, resolve, options).map_err(|e| e.0)?;
    for warning in &imported.report.warnings {
        log::warn!("{name}: {warning}");
    }
    if !imported.report.ignored.is_empty() {
        log::info!("{name}: not imported: {}", imported.report.ignored.join(", "));
    }
    Ok(imported)
}

/// Меш модели.
pub fn load(name: &str, bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>, options: &Options) -> Result<MeshData, String> {
    let imported = read(name, bytes, resolve, options)?;
    let mesh = imported.mesh;
    Ok(MeshData {
        positions: mesh.positions,
        normals: mesh.normals,
        uvs: mesh.uvs,
        indices: mesh.indices,
        tangents: Vec::new(),
        submeshes: mesh.parts.into_iter().map(|p| SubMesh { indices: p.indices, material: None, name: p.name }).collect(),
    })
}

/// Атлас модели (PNG).
pub fn texture_png(name: &str, bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>, options: &Options) -> Result<Vec<u8>, String> {
    let imported = read(name, bytes, resolve, options)?;
    imported
        .texture
        .map(|t| engine_model_formats::encode_png(&t))
        .ok_or_else(|| format!("`{name}` has no colours or texture of its own; assign a material"))
}
