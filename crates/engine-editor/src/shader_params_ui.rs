//! Параметры своих шейдеров в инспекторе (docs/api/shaders.md): виджет на каждый
//! `//! param` из заголовка `.shader` — число с ползунком для диапазона, цвет, текстура,
//! вектор, флажок; подсказка — из комментариев `///`. Значение пишется в `ShaderParams`
//! материала или компонента, только когда его меняют (иначе действует значение из файла).

use std::cell::RefCell;
use std::collections::HashMap;

use engine_assets::{AssetDatabase, Guid, ParamValue, ShaderInfo, ShaderParams};
use engine_core::reflect::FieldMeta;

use crate::i18n::{n_, tr_s, trf};
use crate::inspector::{asset_widget, axes, color_widget, property};
use crate::state::AssetEntry;
use crate::theme;

const TEXTURE: FieldMeta = FieldMeta { variants: &[], asset_kind: n_("Texture") };

/// Заголовок шейдера: разобран из файла, пока файл не изменится.
type Cached = (Option<(std::time::SystemTime, u64)>, Result<ShaderInfo, String>);

thread_local! {
    static HEADERS: RefCell<HashMap<Guid, Cached>> = RefCell::new(HashMap::new());
}

/// Заголовок `.shader` по GUID (кэш по времени изменения файла). `None` — шейдер не назначен
/// или его нет в базе.
pub(crate) fn shader_info(database: Option<&AssetDatabase>, guid: Guid) -> Option<Result<ShaderInfo, String>> {
    if guid.is_nil() {
        return None;
    }
    let database = database?;
    database.relative_path_of(guid)?;
    let stamp = database.path_of(guid).and_then(|p| std::fs::metadata(p).ok()).and_then(|m| Some((m.modified().ok()?, m.len())));
    HEADERS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached, info)) = cache.get(&guid) {
            if *cached == stamp && stamp.is_some() {
                return Some(info.clone());
            }
        }
        let info = database
            .read_text(guid)
            .map_err(|e| e.to_string())
            .and_then(|text| engine_assets::parse_header(&text).map_err(|e| e.to_string()));
        cache.insert(guid, (stamp, info.clone()));
        Some(info)
    })
}

/// Шейдер из поля-ссылки (`Value::Asset`).
pub(crate) fn guid_of(value: Option<engine_core::reflect::Value>) -> Guid {
    match value {
        Some(engine_core::reflect::Value::Asset(text)) => Guid::parse(&text).unwrap_or(Guid::NIL),
        _ => Guid::NIL,
    }
}

/// Строки параметров шейдера. Возвращает `true`, если значение изменили.
pub(crate) fn params_ui(
    ui: &mut egui::Ui,
    info: &ShaderInfo,
    params: &mut ShaderParams,
    assets: &[AssetEntry],
    filter: &mut String,
    salt: impl std::hash::Hash + Copy,
) -> bool {
    let mut changed = false;
    if info.params.is_empty() {
        crate::controls::muted(ui, tr_s("This shader has no parameters"));
        return false;
    }
    for decl in &info.params {
        let mut value = info.value(decl, &[params]);
        let label = theme::pretty_name(&decl.name);
        let edited = property(ui, &label, &decl.doc, |ui| {
            let width = ui.available_width();
            match &mut value {
                ParamValue::Float(x) => match decl.range {
                    Some((min, max)) => {
                        ui.spacing_mut().slider_width = (width - 70.0).max(60.0);
                        ui.add(egui::Slider::new(x, min..=max).max_decimals(3)).changed()
                    }
                    None => crate::controls::number_field(ui, x, 0.05, width).changed(),
                },
                ParamValue::Int(i) => match decl.range {
                    Some((min, max)) => {
                        ui.spacing_mut().slider_width = (width - 70.0).max(60.0);
                        ui.add(egui::Slider::new(i, min.round() as i32..=max.round() as i32)).changed()
                    }
                    None => crate::controls::number_field(ui, i, 0.1, width).changed(),
                },
                ParamValue::Bool(b) => crate::controls::checkbox(ui, b, "").changed(),
                ParamValue::Vec2(x, y) => {
                    let mut v = [*x, *y];
                    let c = axes(ui, &mut v, 0.01);
                    (*x, *y) = (v[0], v[1]);
                    c
                }
                ParamValue::Vec3(x, y, z) => {
                    let mut v = [*x, *y, *z];
                    let c = axes(ui, &mut v, 0.01);
                    (*x, *y, *z) = (v[0], v[1], v[2]);
                    c
                }
                ParamValue::Vec4(x, y, z, w) => {
                    let mut v = [*x, *y, *z, *w];
                    let c = axes(ui, &mut v, 0.01);
                    (*x, *y, *z, *w) = (v[0], v[1], v[2], v[3]);
                    c
                }
                ParamValue::Color(r, g, b, a) => color_widget(ui, r, g, b, a, ("shader_param_color", salt, decl.name.as_str())),
                ParamValue::Texture(text) => asset_widget(ui, text, TEXTURE, assets, filter, ("shader_param_texture", salt, decl.name.as_str())),
            }
        });
        if edited {
            params.set(&decl.name, value);
            changed = true;
        }
    }
    changed
}

/// Параметры шейдера `shader` в поле `ShaderParams` (компоненты `PostEffect`, `SpriteShader`,
/// `MaterialParams`). Шейдер не назначен или с ошибкой — пояснение вместо виджетов.
pub(crate) fn field_ui(
    ui: &mut egui::Ui,
    database: Option<&AssetDatabase>,
    shader: Guid,
    params: &mut ShaderParams,
    assets: &[AssetEntry],
    filter: &mut String,
    salt: impl std::hash::Hash + Copy,
) -> bool {
    match shader_info(database, shader) {
        None => {
            crate::controls::muted(ui, tr_s("Choose a shader to see its parameters"));
            false
        }
        Some(Err(error)) => {
            ui.label(egui::RichText::new(trf!("The shader header has an error: {error}", error = error)).color(theme::ERROR));
            false
        }
        Some(Ok(info)) => {
            theme::section_label(ui, tr_s("Shader Parameters"));
            params_ui(ui, &info, params, assets, filter, salt)
        }
    }
}

impl crate::state::Editor {
    /// Создать шейдер вида `kind` из шаблона в каталоге `folder` (относительно каталога
    /// ассетов): `New Surface Shader.shader`, `New Surface Shader 1.shader`… Возвращает путь.
    pub(crate) fn create_shader(&mut self, folder: &str, kind: engine_assets::ShaderKind) -> Result<String, crate::EditorError> {
        let root = self.require_assets_root("create a shader")?;
        let base = match kind {
            engine_assets::ShaderKind::Surface => "New Surface Shader", // i18n-skip: имя файла
            engine_assets::ShaderKind::Unlit => "New Unlit Shader", // i18n-skip: имя файла
            engine_assets::ShaderKind::Sprite => "New Sprite Shader", // i18n-skip: имя файла
            engine_assets::ShaderKind::Post => "New Post Effect", // i18n-skip: имя файла
        };
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        let mut relative = format!("{prefix}{base}.shader");
        let mut suffix = 1;
        while root.join(&relative).exists() {
            relative = format!("{prefix}{base} {suffix}.shader"); // i18n-skip: имя файла
            suffix += 1;
        }
        let path = root.join(&relative);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| crate::EditorError::Other(trf!("Cannot create `{path}`: {error}", path = dir.display(), error = e)))?;
        }
        std::fs::write(&path, engine_render::shaders::template(kind))
            .map_err(|e| crate::EditorError::Other(trf!("Cannot write `{path}`: {error}", path = path.display(), error = e)))?;
        log::info!("created `Assets/{relative}`");
        Ok(relative)
    }
}
