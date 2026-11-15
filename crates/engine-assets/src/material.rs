//! Материалы: ассет `.material` (RON) и материалы, созданные из кода.
//!
//! Материал — описание поверхности в модели metallic-roughness (как в glTF): базовый
//! цвет и текстура, металличность и шероховатость (и их общая текстура), карта нормалей,
//! свечение, ambient occlusion, тайлинг, режим прозрачности, двусторонность, тени,
//! «без освещения». Текстуры — ссылки по GUID, как у компонентов: файлы можно
//! переименовывать.

use std::collections::HashMap;

use engine_core::math::{Color, Vec2};
use serde::{Deserialize, Serialize};

use crate::{AssetHandle, AssetType, Guid, Shader, ShaderParams, Texture};

/// Как материал обращается с прозрачностью (альфой базового цвета × текстуры).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlphaMode {
    /// Непрозрачный: альфа не учитывается.
    #[default]
    Opaque,
    /// Вырез: пиксели с альфой ниже порога не рисуются (листва, решётки). Дырки видны
    /// и в тени.
    Cutout,
    /// Полупрозрачный: смешивается с тем, что позади (стекло). Рисуется после
    /// непрозрачных, от дальних к ближним.
    Blend,
}

/// Материал поверхности. Он же — тип ассета для [`AssetHandle<Material>`].
///
/// ```ignore
/// let gold = Material::new().color(Color::hex(0xffc933)).metallic(1.0).roughness(0.25);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Базовый цвет (линейный); умножается на текстуру. Альфа — прозрачность.
    pub base_color: Color,
    pub base_color_texture: AssetHandle<Texture>,
    /// 0 — диэлектрик, 1 — металл. Умножается на синий канал `metallic_roughness_texture`.
    pub metallic: f32,
    /// 0 — зеркало, 1 — матовая. Умножается на зелёный канал `metallic_roughness_texture`.
    pub roughness: f32,
    /// Текстура как в glTF: G — шероховатость, B — металличность (линейные данные).
    pub metallic_roughness_texture: AssetHandle<Texture>,
    /// Карта нормалей в касательном пространстве (OpenGL-соглашение, +Y вверх).
    pub normal_texture: AssetHandle<Texture>,
    /// Сила карты нормалей: 0 — плоско, 1 — как в текстуре.
    pub normal_strength: f32,
    /// Цвет свечения; умножается на `emissive_texture` и `emissive_intensity`.
    pub emissive: Color,
    pub emissive_texture: AssetHandle<Texture>,
    /// Яркость свечения: больше единицы — блум.
    pub emissive_intensity: f32,
    /// Ambient occlusion: красный канал затеняет рассеянный свет в щелях.
    pub occlusion_texture: AssetHandle<Texture>,
    pub occlusion_strength: f32,
    /// Повторение текстур по UV меша.
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    /// Шаг повторения базовой текстуры в метрах в мировых координатах (0 — по UV меша).
    pub world_tiling: f32,
    pub alpha_mode: AlphaMode,
    /// Порог для [`AlphaMode::Cutout`].
    pub alpha_cutoff: f32,
    /// Рисовать обе стороны граней (нормаль на обратной стороне переворачивается).
    pub double_sided: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// Без освещения: цвет как есть (плюс свечение).
    pub unlit: bool,
    /// Dev-сетка в метрах поверх материала — для прототипов уровней.
    pub grid: bool,
    /// Свой шейдер (`.shader`, вид `surface` или `unlit`; `docs/api/shaders.md`). Пусто —
    /// стандартный материал.
    pub shader: AssetHandle<Shader>,
    /// Значения параметров своего шейдера; чего нет — по умолчанию из `.shader`.
    pub shader_params: ShaderParams,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            base_color_texture: AssetHandle::none(),
            metallic: 0.0,
            roughness: 0.6,
            metallic_roughness_texture: AssetHandle::none(),
            normal_texture: AssetHandle::none(),
            normal_strength: 1.0,
            emissive: Color::BLACK,
            emissive_texture: AssetHandle::none(),
            emissive_intensity: 1.0,
            occlusion_texture: AssetHandle::none(),
            occlusion_strength: 1.0,
            uv_scale: Vec2::ONE,
            uv_offset: Vec2::ZERO,
            world_tiling: 0.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            cast_shadows: true,
            receive_shadows: true,
            unlit: false,
            grid: false,
            shader: AssetHandle::none(),
            shader_params: ShaderParams::default(),
        }
    }
}

impl AssetType for Material {
    const IMPORTER: &'static str = "Material";
}

/// Ошибка разбора `.material`.
#[derive(Debug, thiserror::Error)]
#[error("not a valid .material file: {0}. Fix the text or recreate the material from the editor (Create → Material)")]
pub struct MaterialParseError(pub String);

impl Material {
    /// Белый диэлектрик с шероховатостью 0.6.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn color(mut self, color: Color) -> Self {
        self.base_color = color;
        self
    }

    pub fn texture(mut self, texture: AssetHandle<Texture>) -> Self {
        self.base_color_texture = texture;
        self
    }

    pub fn metallic(mut self, metallic: f32) -> Self {
        self.metallic = metallic;
        self
    }

    pub fn roughness(mut self, roughness: f32) -> Self {
        self.roughness = roughness;
        self
    }

    pub fn metallic_roughness_texture(mut self, texture: AssetHandle<Texture>) -> Self {
        self.metallic_roughness_texture = texture;
        self
    }

    /// Карта нормалей и её сила.
    pub fn normal_map(mut self, texture: AssetHandle<Texture>, strength: f32) -> Self {
        self.normal_texture = texture;
        self.normal_strength = strength;
        self
    }

    /// Свечение: цвет × яркость (больше единицы — блум).
    pub fn emissive(mut self, color: Color, intensity: f32) -> Self {
        self.emissive = color;
        self.emissive_intensity = intensity;
        self
    }

    pub fn emissive_texture(mut self, texture: AssetHandle<Texture>) -> Self {
        self.emissive_texture = texture;
        self
    }

    pub fn occlusion(mut self, texture: AssetHandle<Texture>, strength: f32) -> Self {
        self.occlusion_texture = texture;
        self.occlusion_strength = strength;
        self
    }

    /// Повторение текстур: `tiling(4.0, 4.0)` — четыре раза по каждой оси UV.
    pub fn tiling(mut self, x: f32, y: f32) -> Self {
        self.uv_scale = Vec2::new(x, y);
        self
    }

    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.uv_offset = Vec2::new(x, y);
        self
    }

    /// Базовая текстура в мировых координатах с шагом `metres` (как `MeshRenderer::tiling`).
    pub fn world_tiling(mut self, metres: f32) -> Self {
        self.world_tiling = metres;
        self
    }

    /// Вырез по альфе с порогом `threshold`.
    pub fn cutout(mut self, threshold: f32) -> Self {
        self.alpha_mode = AlphaMode::Cutout;
        self.alpha_cutoff = threshold;
        self
    }

    /// Полупрозрачный (альфа базового цвета × текстуры).
    pub fn transparent(mut self) -> Self {
        self.alpha_mode = AlphaMode::Blend;
        self
    }

    pub fn double_sided(mut self, double_sided: bool) -> Self {
        self.double_sided = double_sided;
        self
    }

    pub fn unlit(mut self, unlit: bool) -> Self {
        self.unlit = unlit;
        self
    }

    pub fn shadows(mut self, cast: bool, receive: bool) -> Self {
        self.cast_shadows = cast;
        self.receive_shadows = receive;
        self
    }

    /// Свой шейдер (`.shader`) — см. `docs/api/shaders.md`.
    pub fn shader(mut self, shader: AssetHandle<Shader>) -> Self {
        self.shader = shader;
        self
    }

    /// Значение параметра своего шейдера: `.param("amount", 0.5)`.
    pub fn param(mut self, name: &str, value: impl Into<crate::ParamValue>) -> Self {
        self.shader_params.set(name, value);
        self
    }

    /// Все текстуры материала (пустые слоты пропускаются), включая параметры шейдера.
    pub fn textures(&self) -> impl Iterator<Item = Guid> + '_ {
        [
            self.base_color_texture,
            self.metallic_roughness_texture,
            self.normal_texture,
            self.emissive_texture,
            self.occlusion_texture,
        ]
        .into_iter()
        .filter(|t| !t.is_none())
        .map(|t| t.guid())
        .chain(self.shader_params.textures())
    }

    /// Разобрать текст `.material`. Отсутствующие поля получают значения по умолчанию.
    pub fn from_ron(text: &str) -> Result<Self, MaterialParseError> {
        let file: MaterialFile = ron::from_str(text).map_err(|e| MaterialParseError(e.to_string()))?;
        Ok(file.into())
    }

    /// Текст `.material` (RON, с отступами).
    pub fn to_ron(&self) -> String {
        let file = MaterialFile::from(self);
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::new()).expect("material is always serializable");
        format!("// Material (Piska Engine). Textures are referenced by GUID.\n{text}\n")
    }
}

/// Вид `.material` на диске: цвета — четвёрки чисел, текстуры — GUID строкой
/// (пустая строка — нет текстуры).
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct MaterialFile {
    base_color: (f32, f32, f32, f32),
    base_color_texture: String,
    metallic: f32,
    roughness: f32,
    metallic_roughness_texture: String,
    normal_texture: String,
    normal_strength: f32,
    emissive: (f32, f32, f32),
    emissive_texture: String,
    emissive_intensity: f32,
    occlusion_texture: String,
    occlusion_strength: f32,
    uv_scale: (f32, f32),
    uv_offset: (f32, f32),
    world_tiling: f32,
    alpha_mode: AlphaMode,
    alpha_cutoff: f32,
    double_sided: bool,
    cast_shadows: bool,
    receive_shadows: bool,
    unlit: bool,
    grid: bool,
    /// GUID `.shader` строкой (пусто — стандартный материал).
    shader: String,
    shader_params: ShaderParams,
}

impl Default for MaterialFile {
    fn default() -> Self {
        MaterialFile::from(&Material::default())
    }
}

fn handle_text(handle: AssetHandle<Texture>) -> String {
    if handle.is_none() {
        String::new()
    } else {
        handle.guid().to_string()
    }
}

fn text_handle(text: &str) -> AssetHandle<Texture> {
    Guid::parse(text.trim()).map_or(AssetHandle::none(), AssetHandle::from_guid)
}

impl From<&Material> for MaterialFile {
    fn from(m: &Material) -> Self {
        let c = m.base_color;
        Self {
            base_color: (c.r, c.g, c.b, c.a),
            base_color_texture: handle_text(m.base_color_texture),
            metallic: m.metallic,
            roughness: m.roughness,
            metallic_roughness_texture: handle_text(m.metallic_roughness_texture),
            normal_texture: handle_text(m.normal_texture),
            normal_strength: m.normal_strength,
            emissive: (m.emissive.r, m.emissive.g, m.emissive.b),
            emissive_texture: handle_text(m.emissive_texture),
            emissive_intensity: m.emissive_intensity,
            occlusion_texture: handle_text(m.occlusion_texture),
            occlusion_strength: m.occlusion_strength,
            uv_scale: (m.uv_scale.x, m.uv_scale.y),
            uv_offset: (m.uv_offset.x, m.uv_offset.y),
            world_tiling: m.world_tiling,
            alpha_mode: m.alpha_mode,
            alpha_cutoff: m.alpha_cutoff,
            double_sided: m.double_sided,
            cast_shadows: m.cast_shadows,
            receive_shadows: m.receive_shadows,
            unlit: m.unlit,
            grid: m.grid,
            shader: if m.shader.is_none() { String::new() } else { m.shader.guid().to_string() },
            shader_params: m.shader_params.clone(),
        }
    }
}

impl From<MaterialFile> for Material {
    fn from(f: MaterialFile) -> Self {
        Self {
            base_color: Color::rgba(f.base_color.0, f.base_color.1, f.base_color.2, f.base_color.3),
            base_color_texture: text_handle(&f.base_color_texture),
            metallic: f.metallic,
            roughness: f.roughness,
            metallic_roughness_texture: text_handle(&f.metallic_roughness_texture),
            normal_texture: text_handle(&f.normal_texture),
            normal_strength: f.normal_strength,
            emissive: Color::rgb(f.emissive.0, f.emissive.1, f.emissive.2),
            emissive_texture: text_handle(&f.emissive_texture),
            emissive_intensity: f.emissive_intensity,
            occlusion_texture: text_handle(&f.occlusion_texture),
            occlusion_strength: f.occlusion_strength,
            uv_scale: Vec2::new(f.uv_scale.0, f.uv_scale.1),
            uv_offset: Vec2::new(f.uv_offset.0, f.uv_offset.1),
            world_tiling: f.world_tiling,
            alpha_mode: f.alpha_mode,
            alpha_cutoff: f.alpha_cutoff,
            double_sided: f.double_sided,
            cast_shadows: f.cast_shadows,
            receive_shadows: f.receive_shadows,
            unlit: f.unlit,
            grid: f.grid,
            shader: Guid::parse(f.shader.trim()).map_or(AssetHandle::none(), AssetHandle::from_guid),
            shader_params: f.shader_params,
        }
    }
}

/// Материалы, созданные или изменённые из кода, — ресурс `world.resources`.
///
/// Рендерер ищет материал сначала здесь, потом в базе ассетов. Поэтому сюда можно
/// положить и материал с GUID ассета `.material`: он подменит файл, пока лежит здесь.
/// Изменения через [`get_mut`](Self::get_mut) рендерер увидит в следующем кадре.
#[derive(Default)]
pub struct Materials {
    entries: HashMap<Guid, (Material, u64)>,
    revision: u64,
}

impl Materials {
    /// Добавить материал и получить ссылку для `MeshRenderer::with_material_asset`.
    pub fn add(&mut self, material: Material) -> AssetHandle<Material> {
        let guid = Guid::generate();
        self.set(AssetHandle::from_guid(guid), material);
        AssetHandle::from_guid(guid)
    }

    /// Положить материал под заданной ссылкой (например, GUID ассета — подмена файла).
    pub fn set(&mut self, handle: AssetHandle<Material>, material: Material) {
        self.revision += 1;
        self.entries.insert(handle.guid(), (material, self.revision));
    }

    pub fn get(&self, handle: AssetHandle<Material>) -> Option<&Material> {
        self.entries.get(&handle.guid()).map(|(m, _)| m)
    }

    /// Изменить материал: все объекты с ним увидят изменение в следующем кадре.
    pub fn get_mut(&mut self, handle: AssetHandle<Material>) -> Option<&mut Material> {
        self.revision += 1;
        let revision = self.revision;
        self.entries.get_mut(&handle.guid()).map(|(m, r)| {
            *r = revision;
            m
        })
    }

    pub fn remove(&mut self, handle: AssetHandle<Material>) -> Option<Material> {
        self.revision += 1;
        self.entries.remove(&handle.guid()).map(|(m, _)| m)
    }

    /// Материал и номер его последнего изменения — для кэша рендерера.
    pub fn entry(&self, guid: Guid) -> Option<(&Material, u64)> {
        self.entries.get(&guid).map(|(m, r)| (m, *r))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_ron_round_trip_keeps_every_field() {
        let texture = AssetHandle::<Texture>::from_guid(Guid(0x1234));
        let material = Material::new()
            .color(Color::rgba(0.1, 0.2, 0.3, 0.4))
            .texture(texture)
            .metallic(0.7)
            .roughness(0.15)
            .normal_map(AssetHandle::from_guid(Guid(0x55)), 0.6)
            .emissive(Color::rgb(1.0, 0.5, 0.0), 3.0)
            .occlusion(AssetHandle::from_guid(Guid(0x77)), 0.8)
            .tiling(4.0, 2.0)
            .offset(0.25, 0.5)
            .cutout(0.3)
            .double_sided(true)
            .unlit(true)
            .shadows(false, false)
            .shader(AssetHandle::from_guid(Guid(0x5ade)))
            .param("amount", 0.25);
        let text = material.to_ron();
        assert!(text.contains("alpha_mode: Cutout"), "{text}");
        assert!(text.contains(&Guid(0x1234).to_string()), "{text}");
        assert_eq!(Material::from_ron(&text).unwrap(), material);
    }

    #[test]
    fn missing_fields_get_defaults_and_errors_explain() {
        let material = Material::from_ron("(metallic: 1.0, alpha_mode: Blend)").unwrap();
        assert_eq!(material.metallic, 1.0);
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
        assert_eq!(material.roughness, Material::default().roughness);
        assert!(material.base_color_texture.is_none());
        let err = Material::from_ron("(metallic: \"shiny\")").unwrap_err().to_string();
        assert!(err.contains(".material") && err.contains("Create"), "{err}");
    }

    #[test]
    fn runtime_materials_track_revisions() {
        let mut materials = Materials::default();
        let handle = materials.add(Material::new().metallic(1.0));
        let (_, first) = materials.entry(handle.guid()).unwrap();
        materials.get_mut(handle).unwrap().roughness = 0.1;
        let (material, second) = materials.entry(handle.guid()).unwrap();
        assert!(second > first);
        assert_eq!(material.roughness, 0.1);
        assert_eq!(materials.len(), 1);
    }
}
