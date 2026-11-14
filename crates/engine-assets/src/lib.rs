//! `engine-assets` — ассеты как в Unity: рядом с каждым файлом лежит `<файл>.meta`
//! с постоянным GUID и настройками импорта. Сцены ссылаются на ассеты по GUID,
//! поэтому файлы можно свободно переименовывать и перекладывать.
#![forbid(unsafe_code)]

mod data;
mod formats;
mod material;
mod mesh;
mod pack;

pub use data::{parse_data, DataFormat};

pub use material::{AlphaMode, Material, MaterialParseError, Materials};
pub use mesh::{
    inspect_mesh, load_mesh, load_mesh_bytes, load_mesh_bytes_with, mesh_extras, parse_obj, MeshData, MeshExtras, MeshInfo, MeshMaterial,
    SubMesh,
};
pub use pack::{AssetPack, AssetPackBuilder};
/// Чтение сторонних форматов моделей (общий крейт движка и Piska Modeler).
pub use engine_model_formats as model_formats;
pub use formats::import_options;

use std::borrow::Cow;
use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// `std::time::SystemTime` в браузере паникует; `web-time` — тот же API, и там, и тут.
use web_time::{SystemTime, UNIX_EPOCH};

use engine_core::error::{did_you_mean, SceneError};
use engine_core::reflect::{FieldValue, Value};
use engine_core::serialize::{Prefab, PrefabSource};
use serde::{Deserialize, Serialize};

// ----------------------------------------------------------------------
// GUID и типизированные ссылки
// ----------------------------------------------------------------------

/// Постоянный идентификатор ассета (128 бит, 32 hex-символа в файлах).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Guid(pub u128);

impl Guid {
    pub const NIL: Guid = Guid(0);

    /// Новый случайный GUID.
    pub fn generate() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        // `RandomState` даёт случайные ключи SipHash на каждый экземпляр — этого
        // достаточно для уникальности идентификаторов без внешних зависимостей.
        let half = |salt: u8| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u128(nanos);
            hasher.write_u8(salt);
            hasher.finish() as u128
        };
        Guid((half(0) << 64) | half(1)).max_one()
    }

    fn max_one(self) -> Self {
        if self.0 == 0 { Guid(1) } else { self }
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text.len() == 32 {
            u128::from_str_radix(text, 16).ok().map(Guid)
        } else {
            None
        }
    }

    pub fn is_nil(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Guid({self})")
    }
}

/// Маркеры типов ассетов для [`AssetHandle<T>`].
#[derive(Debug)]
pub struct Texture;
#[derive(Debug)]
pub struct PrefabAsset;
#[derive(Debug)]
pub struct SceneAsset;
#[derive(Debug)]
pub struct AudioClip;
#[derive(Debug)]
pub struct Mesh;
/// Шрифт TrueType / OpenType (`.ttf`, `.otf`) — для текста интерфейса.
#[derive(Debug)]
pub struct Font;

/// Тип ассета: связывает маркер (`Texture`) с именем импортёра в `.meta`.
/// По нему инспектор знает, какие файлы предлагать для поля.
pub trait AssetType: 'static {
    const IMPORTER: &'static str;
}

impl AssetType for Texture {
    const IMPORTER: &'static str = "Texture";
}
impl AssetType for PrefabAsset {
    const IMPORTER: &'static str = "Prefab";
}
impl AssetType for SceneAsset {
    const IMPORTER: &'static str = "Scene";
}
impl AssetType for AudioClip {
    const IMPORTER: &'static str = "Audio";
}
impl AssetType for Mesh {
    const IMPORTER: &'static str = "Mesh";
}
impl AssetType for Font {
    const IMPORTER: &'static str = "Font";
}

/// Типизированная ссылка на ассет: GUID + тип на уровне компилятора.
/// В поле компонента сериализуется как `Asset("<guid>")`.
pub struct AssetHandle<T> {
    guid: Guid,
    _marker: PhantomData<fn() -> T>,
}

impl<T> AssetHandle<T> {
    /// «Не назначено» — как пустой слот в инспекторе Unity.
    pub const fn none() -> Self {
        Self {
            guid: Guid::NIL,
            _marker: PhantomData,
        }
    }

    pub const fn from_guid(guid: Guid) -> Self {
        Self {
            guid,
            _marker: PhantomData,
        }
    }

    pub fn guid(&self) -> Guid {
        self.guid
    }

    pub fn is_none(&self) -> bool {
        self.guid.is_nil()
    }
}

impl<T> Clone for AssetHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for AssetHandle<T> {}
impl<T> Default for AssetHandle<T> {
    fn default() -> Self {
        Self::none()
    }
}
impl<T> PartialEq for AssetHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.guid == other.guid
    }
}
impl<T> fmt::Debug for AssetHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AssetHandle({})", self.guid)
    }
}

impl<T: AssetType> FieldValue for AssetHandle<T> {
    const EXPECTED: &'static str = "Asset(\"<32 hex digits>\")";

    fn to_value(&self) -> Value {
        Value::Asset(if self.is_none() {
            String::new()
        } else {
            self.guid.to_string()
        })
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Asset(text) if text.is_empty() => Some(Self::none()),
            Value::Asset(text) => Guid::parse(text).map(Self::from_guid),
            _ => None,
        }
    }

    fn meta() -> engine_core::reflect::FieldMeta {
        engine_core::reflect::FieldMeta {
            variants: &[],
            asset_kind: T::IMPORTER,
        }
    }
}

// ----------------------------------------------------------------------
// .meta-файлы
// ----------------------------------------------------------------------

/// Содержимое `<ассет>.meta`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetMeta {
    pub guid: String,
    /// Какой импортёр обрабатывает файл: `Texture`, `Prefab`, `Scene`, `Audio`, `Mesh`, `Raw`.
    pub importer: String,
    /// Настройки импорта (например, `"filter": Enum("Nearest")` для пиксель-арта).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Value>,
}

fn importer_for(path: &Path) -> &'static str {
    // Модель Minecraft — по двойному расширению; обычный `.json` остаётся данными.
    if path.to_string_lossy().to_ascii_lowercase().ends_with(model_formats::JAVA_MODEL_SUFFIX) {
        return "Mesh";
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" => "Texture",
        "prefab" => "Prefab",
        "scene" => "Scene",
        // mp3 и flac движок не декодирует (см. docs/api/audio.md) — такие файлы не звук.
        "wav" | "ogg" => "Audio",
        "gltf" | "glb" | "obj" | "pmodel" | "bbmodel" | "vox" | "ply" | "stl" => "Mesh",
        "material" => "Material",
        "ttf" | "otf" => "Font",
        // Сценарии на языке Scenario (`docs/api/scenario.md`).
        "scenario" => "Scenario",
        "rs" | "cpp" | "cc" | "cxx" | "h" | "hpp" => "Code",
        "wgsl" | "txt" | "md" | "toml" | "ron" | "json" | "cmake" => "Text",
        _ => "Raw",
    }
}

/// Настройки импорта нового файла: у моделей Piska Modeler (`.pmodel`) текстура
/// пиксельная — фильтр «ближайший».
fn default_settings_for(path: &Path, importer: &str) -> BTreeMap<String, Value> {
    let mut settings = default_settings(importer);
    if is_piska_model(&path.to_string_lossy()) {
        settings.insert("filter".to_string(), Value::Enum("Nearest".to_string()));
    }
    // Сторонние форматы: явные настройки импорта (масштаб, ось, отражение…) и «ближайший» фильтр.
    settings.extend(formats::default_settings(&path.to_string_lossy()));
    settings
}

/// Модель Piska Modeler (`.pmodel`): меш со своей текстурой внутри файла.
pub fn is_piska_model(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case("pmodel"))
}

/// Есть ли у меша-файла свой атлас (по имени): `.pmodel`, `.bbmodel`, модель Minecraft,
/// `.vox`, `.ply` (если в нём цвета — см. [`AssetDatabase::has_embedded_texture`]).
/// Рендерер грузит такой атлас без мипмапов (грани в нём лежат вплотную).
pub fn has_embedded_atlas(path: &str) -> bool {
    is_piska_model(path) || formats::is_model_format(path) && !path.to_ascii_lowercase().ends_with(".stl")
}

fn default_settings(importer: &str) -> BTreeMap<String, Value> {
    let mut settings = BTreeMap::new();
    if importer == "Texture" {
        settings.insert("filter".to_string(), Value::Enum("Linear".to_string()));
    }
    settings
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset `{path}` not found under `{root}`.{hint}")]
    NotFound {
        path: String,
        root: String,
        hint: String,
    },
    #[error("cannot access `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot import `{path}`: {reason}")]
    Import { path: String, reason: String },
    #[error("`{path}` is not a valid .meta file ({message}). Delete it to regenerate (the GUID will change)")]
    BadMeta { path: String, message: String },
    #[error("the asset pack is damaged: {reason}")]
    BadPack { reason: String },
    #[error("assets `{first}` and `{second}` share GUID {guid}. Was a .meta file copied by hand? Delete one of them")]
    DuplicateGuid {
        guid: Guid,
        first: String,
        second: String,
    },
}

// ----------------------------------------------------------------------
// База ассетов
// ----------------------------------------------------------------------

/// Индекс ассетов: GUID ↔ путь. Хранится в `world.resources`.
///
/// Источник файлов — каталог на диске ([`open`](Self::open), с созданием `.meta`) или
/// пак в памяти ([`from_pack`](Self::from_pack): браузер, APK). Читайте содержимое
/// через [`read`](Self::read) — тогда код работает с обоими источниками.
#[derive(Clone, Debug, Default)]
pub struct AssetDatabase {
    root: PathBuf,
    /// Путь относительно корня, всегда с `/`.
    by_guid: HashMap<Guid, String>,
    by_path: HashMap<String, Guid>,
    metas: HashMap<Guid, AssetMeta>,
    /// `Some` — ассеты из пака, а не из каталога.
    pack: Option<Arc<AssetPack>>,
}

/// Корень, которым база из пака представляется в сообщениях и в [`AssetDatabase::path_of`].
pub const PACK_ROOT: &str = "<asset pack>";

/// Пак, установленный для всего процесса (см. [`install_pack`]).
static INSTALLED_PACK: Mutex<Option<Arc<AssetPack>>> = Mutex::new(None);

/// Установить пак ассетов для процесса: после этого [`AssetDatabase::mount`] берёт
/// ассеты из него, а не из каталога. Так делает веб-сборка (пак скачан до старта игры)
/// и Android (пак из APK); игровой код не меняется.
pub fn install_pack(pack: AssetPack) {
    *INSTALLED_PACK.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(pack));
}

/// Пак, установленный [`install_pack`], если он есть.
pub fn installed_pack() -> Option<Arc<AssetPack>> {
    INSTALLED_PACK.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

impl AssetDatabase {
    /// Ассеты игры: из установленного пака ([`install_pack`]), если он есть, иначе из
    /// каталога `root` (как [`open`](Self::open)).
    pub fn mount(root: impl AsRef<Path>) -> Result<Self, AssetError> {
        match installed_pack() {
            Some(pack) => Self::from_shared_pack(pack),
            None => Self::open(root),
        }
    }

    /// База поверх пака. У каждого файла в паке должен быть `.meta` (их создаёт
    /// `AssetDatabase::open` при экспорте), иначе GUID был бы случайным при каждом запуске.
    pub fn from_pack(pack: AssetPack) -> Result<Self, AssetError> {
        Self::from_shared_pack(Arc::new(pack))
    }

    fn from_shared_pack(pack: Arc<AssetPack>) -> Result<Self, AssetError> {
        let mut db = Self {
            root: PathBuf::from(PACK_ROOT),
            ..Self::default()
        };
        for relative in pack.paths() {
            if relative.ends_with(".meta") {
                continue;
            }
            let meta_path = format!("{relative}.meta");
            let bytes = pack.get(&meta_path).ok_or_else(|| AssetError::BadPack {
                reason: format!(
                    "`{relative}` has no `{meta_path}`; open the assets directory once \
                     (the editor or `AssetDatabase::open`) and export again"
                ),
            })?;
            let text = std::str::from_utf8(bytes).map_err(|_| AssetError::BadMeta {
                path: meta_path.clone(),
                message: "not UTF-8".to_string(),
            })?;
            let meta = parse_meta(text, &meta_path)?;
            db.insert(relative.to_string(), meta, &meta_path)?;
        }
        db.pack = Some(pack);
        Ok(db)
    }

    /// Ассеты берутся из пака, а не из каталога.
    pub fn is_packed(&self) -> bool {
        self.pack.is_some()
    }

    /// Пак, из которого читает база (если она из пака).
    pub fn pack(&self) -> Option<&AssetPack> {
        self.pack.as_deref()
    }

    /// Содержимое файла ассета — одинаково для каталога и пака.
    pub fn read(&self, guid: Guid) -> Result<Cow<'_, [u8]>, AssetError> {
        let relative = self.by_guid.get(&guid).ok_or_else(|| self.unknown_guid(guid))?;
        self.read_path(relative)
    }

    fn unknown_guid(&self, guid: Guid) -> AssetError {
        AssetError::NotFound {
            path: format!("GUID {guid}"),
            root: self.root.display().to_string(),
            hint: " Was the .meta file deleted?".to_string(),
        }
    }

    /// Содержимое файла по пути относительно корня (любого, не только ассета с `.meta`:
    /// например, буфер `.bin` рядом с `.gltf`).
    pub fn read_path(&self, relative_path: &str) -> Result<Cow<'_, [u8]>, AssetError> {
        let relative = relative_path.replace('\\', "/");
        match &self.pack {
            Some(pack) => pack.get(&relative).map(Cow::Borrowed).ok_or_else(|| AssetError::NotFound {
                hint: did_you_mean(&relative, pack.paths()),
                path: relative,
                root: PACK_ROOT.to_string(),
            }),
            None => {
                let path = self.root.join(&relative);
                std::fs::read(&path).map(Cow::Owned).map_err(|source| AssetError::Io {
                    path: path.display().to_string(),
                    source,
                })
            }
        }
    }

    /// Байты картинки ассета для рендерера: у текстуры — сам файл, у модели Piska Modeler
    /// (`.pmodel`) — её атлас (PNG).
    pub fn read_image(&self, guid: Guid) -> Result<Cow<'_, [u8]>, AssetError> {
        let relative = self.by_guid.get(&guid).ok_or_else(|| self.unknown_guid(guid))?.clone();
        if self.third_party_model(guid) {
            let settings = self.meta(guid).map(|m| m.settings.clone()).unwrap_or_default();
            let options = formats::import_options(&settings);
            return self
                .with_mesh_bytes(guid, |name, bytes, resolve| {
                    formats::texture_png(name, bytes, resolve, &options).map_err(|reason| AssetError::Import { path: name.to_string(), reason })
                })
                .map(Cow::Owned);
        }
        if !is_piska_model(&relative) {
            return self.read(guid);
        }
        let bytes = self.read(guid)?;
        mesh::piska_model_texture(&bytes)
            .map(Cow::Owned)
            .map_err(|reason| AssetError::Import { path: relative, reason })
    }

    /// Есть ли у меша-ассета своя текстура внутри файла (`.pmodel`): рендерер берёт её,
    /// если у `MeshRenderer` не назначена другая.
    pub fn has_embedded_texture(&self, guid: Guid) -> bool {
        let Some(path) = self.by_guid.get(&guid) else { return false };
        if is_piska_model(path) {
            return true;
        }
        // `.ply` — только с цветами или текстурой, `.stl` — никогда (заголовок дёшев).
        self.third_party_model(guid) && self.read(guid).is_ok_and(|bytes| model_formats::has_texture(path, &bytes))
    }

    /// Меш стороннего формата (`.bbmodel`, `.vox`, `.ply`, `.stl`, `.model.json`, или
    /// `.json` с импортёром `Mesh` в `.meta`).
    fn third_party_model(&self, guid: Guid) -> bool {
        let Some(path) = self.by_guid.get(&guid) else { return false };
        formats::is_model_format(path)
            || path.to_ascii_lowercase().ends_with(".json") && self.meta(guid).is_some_and(|m| m.importer == "Mesh")
    }

    /// Текст ассета (сцена, префаб, RON).
    pub fn read_text(&self, guid: Guid) -> Result<String, AssetError> {
        let bytes = self.read(guid)?;
        let name = self.relative_path_of(guid).unwrap_or_default().to_string();
        String::from_utf8(bytes.into_owned()).map_err(|_| AssetError::Import {
            path: name,
            reason: "the file is not UTF-8 text".to_string(),
        })
    }

    /// Загрузить меш ассета (`.obj`, `.gltf`, `.glb`) — из каталога или из пака.
    pub fn load_mesh(&self, guid: Guid) -> Result<MeshData, AssetError> {
        if self.third_party_model(guid) {
            // Через базу: внешние текстуры — только из каталога ассетов, с настройками из `.meta`.
            let settings = self.meta(guid).map(|m| m.settings.clone()).unwrap_or_default();
            return self.with_mesh_bytes(guid, |name, bytes, resolve| load_mesh_bytes_with(name, bytes, resolve, &settings));
        }
        if self.pack.is_none() {
            let path = self.path_of(guid).ok_or_else(|| self.unknown_guid(guid))?;
            return load_mesh(path);
        }
        let relative = self
            .relative_path_of(guid)
            .ok_or_else(|| self.unknown_guid(guid))?
            .to_string();
        let bytes = self.read(guid)?;
        let dir = relative
            .rsplit_once('/')
            .map(|(dir, _)| format!("{dir}/"))
            .unwrap_or_default();
        let resolve = |uri: &str| self.read_path(&format!("{dir}{uri}")).ok().map(Cow::into_owned);
        load_mesh_bytes(&relative, &bytes, &resolve)
    }

    /// Сведения о меше ассета (вершины, части, материалы, границы) — для окна Project.
    pub fn inspect_mesh(&self, guid: Guid) -> Result<MeshInfo, AssetError> {
        self.with_mesh_bytes(guid, inspect_mesh)
    }

    /// Материал из файла `.material` (RON). Из каталога — всегда с диска (свежая версия).
    pub fn load_material(&self, guid: Guid) -> Result<Material, AssetError> {
        let text = self.read_text(guid)?;
        Material::from_ron(&text).map_err(|e| AssetError::Import {
            path: self.relative_path_of(guid).unwrap_or("?").to_string(),
            reason: e.to_string(),
        })
    }

    /// Записать материал в файл `relative_path` (каталог ассетов, не пак) и добавить его
    /// в базу (с `.meta`; у существующего файла GUID сохраняется). Возвращает ссылку.
    pub fn save_material(&mut self, relative_path: &str, material: &Material) -> Result<AssetHandle<Material>, AssetError> {
        let relative = relative_path.replace('\\', "/");
        if self.pack.is_some() {
            return Err(AssetError::Import {
                path: relative,
                reason: "assets come from a pack and are read-only; save materials in the editor".to_string(),
            });
        }
        let path = self.root.join(&relative);
        let io = |source| AssetError::Io { path: path.display().to_string(), source };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(&path, material.to_ron()).map_err(io)?;
        if let Some(guid) = self.guid_of(&relative) {
            return Ok(AssetHandle::from_guid(guid));
        }
        let meta = AssetMeta {
            guid: Guid::generate().to_string(),
            importer: "Material".to_string(),
            settings: BTreeMap::new(),
        };
        let meta_path = format!("{}.meta", path.display());
        let text = ron::ser::to_string_pretty(&meta, ron::ser::PrettyConfig::new()).expect("meta is always serializable");
        std::fs::write(&meta_path, text).map_err(|source| AssetError::Io { path: meta_path.clone(), source })?;
        self.insert(relative.clone(), meta, &meta_path)?;
        self.handle(&relative)
    }

    /// Материалы, описанные в файле модели (`.gltf`/`.glb`, `.mtl` рядом с `.obj`), в
    /// виде [`Material`]: текстуры находятся по путям рядом с моделью среди ассетов
    /// (картинки, встроенные в `.glb`, и файлы вне каталога ассетов пропускаются).
    /// Порядок — как в файле; на него ссылается [`SubMesh::material`].
    pub fn model_materials(&self, mesh: Guid) -> Result<Vec<Material>, AssetError> {
        let extras = self.mesh_extras(mesh)?;
        let model = self.relative_path_of(mesh).unwrap_or("");
        let dir = model.rsplit_once('/').map_or("", |(dir, _)| dir);
        let texture = |path: &Option<String>| -> AssetHandle<Texture> {
            path.as_ref()
                .and_then(|p| self.guid_of(&join_normalized(dir, p)))
                .map_or(AssetHandle::none(), AssetHandle::from_guid)
        };
        Ok(extras
            .materials
            .iter()
            .map(|m| Material {
                base_color: engine_core::math::Color::rgba(m.color[0], m.color[1], m.color[2], m.color[3]),
                base_color_texture: texture(&m.texture),
                metallic: m.metallic,
                roughness: m.roughness,
                metallic_roughness_texture: texture(&m.metallic_roughness_texture),
                normal_texture: texture(&m.normal_texture),
                normal_strength: m.normal_scale,
                emissive: engine_core::math::Color::rgb(m.emissive[0], m.emissive[1], m.emissive[2]),
                emissive_texture: texture(&m.emissive_texture),
                occlusion_texture: texture(&m.occlusion_texture),
                alpha_mode: m.alpha_mode,
                alpha_cutoff: m.alpha_cutoff,
                double_sided: m.double_sided,
                ..Material::default()
            })
            .collect())
    }

    /// Только части, материалы и внешние файлы меша — без разбора вершин (см. [`mesh_extras`]).
    pub fn mesh_extras(&self, guid: Guid) -> Result<MeshExtras, AssetError> {
        self.with_mesh_bytes(guid, |name, bytes, resolve| Ok(mesh_extras(name, bytes, resolve)))
    }

    fn with_mesh_bytes<T>(
        &self,
        guid: Guid,
        f: impl FnOnce(&str, &[u8], &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<T, AssetError>,
    ) -> Result<T, AssetError> {
        let relative = self
            .relative_path_of(guid)
            .ok_or_else(|| self.unknown_guid(guid))?
            .to_string();
        let bytes = self.read(guid)?;
        let dir = relative
            .rsplit_once('/')
            .map(|(dir, _)| format!("{dir}/"))
            .unwrap_or_default();
        let resolve = |uri: &str| {
            // Ни абсолютных путей, ни выхода выше корня ассетов (`..` обрезается корнем).
            if uri.contains(':') || uri.starts_with('/') || uri.starts_with('\\') {
                return None;
            }
            self.read_path(&join_normalized(&dir, uri)).ok().map(Cow::into_owned)
        };
        f(&relative, &bytes, &resolve)
    }

    fn insert(&mut self, relative: String, meta: AssetMeta, meta_path: &str) -> Result<(), AssetError> {
        let guid = Guid::parse(&meta.guid).ok_or_else(|| AssetError::BadMeta {
            path: meta_path.to_string(),
            message: format!("guid `{}` is not 32 hex digits", meta.guid),
        })?;
        if let Some(first) = self.by_guid.get(&guid) {
            return Err(AssetError::DuplicateGuid {
                guid,
                first: first.clone(),
                second: relative,
            });
        }
        self.by_guid.insert(guid, relative.clone());
        self.by_path.insert(relative, guid);
        self.metas.insert(guid, meta);
        Ok(())
    }

    /// Просканировать каталог. Для файлов без `.meta` он создаётся (новый GUID).
    pub fn open(root: impl AsRef<Path>) -> Result<Self, AssetError> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(AssetError::NotFound {
                path: ".".to_string(),
                root: root.display().to_string(),
                hint: " The assets directory itself is missing; create it or fix the path.".to_string(),
            });
        }
        let mut db = Self {
            root: root.clone(),
            ..Self::default()
        };
        db.scan_dir(&root)?;
        Ok(db)
    }

    fn scan_dir(&mut self, dir: &Path) -> Result<(), AssetError> {
        let io = |path: &Path, source| AssetError::Io {
            path: path.display().to_string(),
            source,
        };
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| io(dir, e))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .collect();
        entries.sort();

        for path in entries {
            if path.is_dir() {
                self.scan_dir(&path)?;
                continue;
            }
            if path.extension().is_some_and(|e| e == "meta") {
                continue;
            }
            let relative = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let meta_path = PathBuf::from(format!("{}.meta", path.display()));

            let meta = if meta_path.is_file() {
                let text = std::fs::read_to_string(&meta_path).map_err(|e| io(&meta_path, e))?;
                parse_meta(&text, &meta_path.display().to_string())?
            } else {
                let importer = importer_for(&path);
                let meta = AssetMeta {
                    guid: Guid::generate().to_string(),
                    importer: importer.to_string(),
                    settings: default_settings_for(&path, importer),
                };
                let text = ron::ser::to_string_pretty(&meta, ron::ser::PrettyConfig::new())
                    .expect("meta is always serializable");
                std::fs::write(&meta_path, text).map_err(|e| io(&meta_path, e))?;
                log::info!("created {}", meta_path.display());
                meta
            };

            self.insert(relative, meta, &meta_path.display().to_string())?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn len(&self) -> usize {
        self.by_guid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_guid.is_empty()
    }

    pub fn guid_of(&self, relative_path: &str) -> Option<Guid> {
        self.by_path.get(&relative_path.replace('\\', "/")).copied()
    }

    /// Абсолютный путь к файлу ассета. У базы из пака путь условный (`<asset pack>/…`),
    /// файла на диске нет — читайте содержимое через [`read`](Self::read).
    pub fn path_of(&self, guid: Guid) -> Option<PathBuf> {
        self.by_guid.get(&guid).map(|rel| self.root.join(rel))
    }

    pub fn relative_path_of(&self, guid: Guid) -> Option<&str> {
        self.by_guid.get(&guid).map(String::as_str)
    }

    pub fn meta(&self, guid: Guid) -> Option<&AssetMeta> {
        self.metas.get(&guid)
    }

    /// Все ассеты: (GUID, путь относительно корня) — основа окна Project в редакторе.
    pub fn assets(&self) -> impl Iterator<Item = (Guid, &str)> {
        self.by_guid.iter().map(|(g, p)| (*g, p.as_str()))
    }

    /// Типизированная ссылка по пути: `db.handle::<Texture>("sprites/player.png")?`.
    pub fn handle<T>(&self, relative_path: &str) -> Result<AssetHandle<T>, AssetError> {
        self.guid_of(relative_path)
            .map(AssetHandle::from_guid)
            .ok_or_else(|| AssetError::NotFound {
                path: relative_path.to_string(),
                root: self.root.display().to_string(),
                hint: did_you_mean(relative_path, self.by_path.keys().map(String::as_str)),
            })
    }
}

/// `dir` + `relative` с разбором `.` и `..`, разделители — `/`.
pub fn join_normalized(dir: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn parse_meta(text: &str, path: &str) -> Result<AssetMeta, AssetError> {
    ron::from_str::<AssetMeta>(text).map_err(|e| AssetError::BadMeta {
        path: path.to_string(),
        message: e.to_string(),
    })
}

/// База ассетов умеет отдавать префабы загрузчику сцен: ссылка — GUID или путь.
impl PrefabSource for AssetDatabase {
    fn load(&self, reference: &str) -> Result<Prefab, SceneError> {
        let guid = match Guid::parse(reference) {
            Some(guid) => Some(guid).filter(|g| self.by_guid.contains_key(g)),
            None => self.guid_of(reference),
        };
        let guid = guid.ok_or_else(|| SceneError::PrefabNotFound {
            reference: reference.to_string(),
            reason: format!(
                "no such asset under `{}`.{}",
                self.root.display(),
                did_you_mean(
                    reference,
                    self.by_path
                        .keys()
                        .filter(|p| p.ends_with(".prefab"))
                        .map(String::as_str)
                )
            ),
        })?;
        let origin = self.relative_path_of(guid).unwrap_or(reference).to_string();
        let text = self.read_text(guid).map_err(|e| SceneError::PrefabNotFound {
            reference: reference.to_string(),
            reason: e.to_string(),
        })?;
        Prefab::from_ron(&text, &origin)
    }
}

/// Сцены по имени из базы ассетов — для `ctx.load_scene("level2")`: ищется
/// `level2`, `level2.scene`, `scenes/level2`, `scenes/level2.scene` (путь относительно
/// корня ассетов). Работает и с каталогом, и с паком. Его ставит `engine::mount_assets`.
pub struct AssetScenes {
    database: AssetDatabase,
}

impl AssetScenes {
    pub fn new(database: AssetDatabase) -> Self {
        Self { database }
    }

    /// GUID сцены по имени из `load_scene`.
    pub fn resolve(&self, name: &str) -> Option<Guid> {
        let name = name.replace('\\', "/");
        [
            name.clone(),
            format!("{name}.scene"),
            format!("scenes/{name}"),
            format!("scenes/{name}.scene"),
        ]
        .iter()
        .find_map(|candidate| self.database.guid_of(candidate))
    }
}

impl engine_core::scenes::SceneSource for AssetScenes {
    fn load(&self, name: &str) -> Result<String, SceneError> {
        let not_found = |reason: String| SceneError::Io {
            path: format!("scene `{name}`"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, reason),
        };
        let Some(guid) = self.resolve(name) else {
            // Как раньше (`DirScenes` от текущего каталога): путь к файлу на диске.
            if !self.database.is_packed() {
                let direct = Path::new(name);
                let with_ext = PathBuf::from(format!("{name}.scene"));
                for path in [direct, with_ext.as_path()] {
                    if path.is_file() {
                        return std::fs::read_to_string(path).map_err(|source| SceneError::Io {
                            path: path.display().to_string(),
                            source,
                        });
                    }
                }
            }
            return Err(not_found(format!(
                "no such scene among the assets (tried `{name}`, `{name}.scene`, `scenes/{name}.scene`).{}",
                did_you_mean(
                    name,
                    self.database
                        .by_path
                        .keys()
                        .filter(|p| p.ends_with(".scene"))
                        .map(String::as_str)
                )
            )));
        };
        self.database.read_text(guid).map_err(|e| not_found(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_roundtrips_and_handles_serialize_as_asset_values() {
        let guid = Guid::generate();
        assert!(!guid.is_nil());
        assert_ne!(guid, Guid::generate());
        assert_eq!(Guid::parse(&guid.to_string()), Some(guid));
        assert_eq!(Guid::parse("xyz"), None);

        let handle = AssetHandle::<Texture>::from_guid(guid);
        assert_eq!(handle.to_value(), Value::Asset(guid.to_string()));
        assert_eq!(AssetHandle::<Texture>::from_value(&handle.to_value()), Some(handle));
        assert!(AssetHandle::<Texture>::from_value(&Value::Asset(String::new())).unwrap().is_none());
        assert_eq!(AssetHandle::<Texture>::from_value(&Value::Str("x".into())), None);
    }

    #[test]
    fn database_creates_meta_once_and_keeps_guid_stable() {
        let dir = std::env::temp_dir().join(format!("engine-assets-test-{}", Guid::generate()));
        std::fs::create_dir_all(dir.join("sprites")).unwrap();
        std::fs::write(dir.join("sprites/hero.png"), b"not really a png").unwrap();

        let db = AssetDatabase::open(&dir).unwrap();
        assert_eq!(db.len(), 1);
        let guid = db.guid_of("sprites/hero.png").unwrap();
        assert!(dir.join("sprites/hero.png.meta").is_file());
        assert_eq!(db.meta(guid).unwrap().importer, "Texture");

        // Повторное открытие читает тот же GUID из .meta.
        let again = AssetDatabase::open(&dir).unwrap();
        assert_eq!(again.guid_of("sprites/hero.png"), Some(guid));
        assert_eq!(again.handle::<Texture>("sprites\\hero.png").unwrap().guid(), guid);

        let err = again.handle::<Texture>("sprites/hero.pn").unwrap_err().to_string();
        assert!(err.contains("Did you mean `sprites/hero.png`?"), "{err}");

        std::fs::remove_dir_all(dir).unwrap();
    }

    const PREFAB: &str = "(name: \"Coin\", components: [], children: [])";
    const QUAD_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nf 1 2 3\n";

    /// Каталог с ассетами → пак → база из пака: те же GUID, те же байты.
    #[test]
    fn pack_round_trip_keeps_guids_bytes_and_settings() {
        let dir = std::env::temp_dir().join(format!("engine-pack-test-{}", Guid::generate()));
        std::fs::create_dir_all(dir.join("prefabs")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::create_dir_all(dir.join("scenes")).unwrap();
        std::fs::write(dir.join("hero.png"), [1u8, 2, 3, 250]).unwrap();
        std::fs::write(dir.join("prefabs/coin.prefab"), PREFAB).unwrap();
        std::fs::write(dir.join("models/tri.obj"), QUAD_OBJ).unwrap();
        std::fs::write(dir.join("scenes/level2.scene"), "scene text").unwrap();
        let disk = AssetDatabase::open(&dir).unwrap();
        // Настройка импорта из .meta должна доехать до пака.
        let hero = disk.guid_of("hero.png").unwrap();

        let mut builder = AssetPack::builder();
        builder.add_dir(&dir).unwrap();
        assert_eq!(builder.len(), 8, "4 files + 4 .meta");
        let bytes = builder.to_bytes();
        let pack = AssetPack::from_bytes(bytes).unwrap();
        assert!(pack.contains("prefabs/coin.prefab.meta"));

        let packed = AssetDatabase::from_pack(pack).unwrap();
        assert!(packed.is_packed() && !disk.is_packed());
        assert_eq!(packed.len(), 4);
        for (guid, path) in disk.assets() {
            assert_eq!(packed.guid_of(path), Some(guid), "{path}");
            assert_eq!(packed.read(guid).unwrap(), disk.read(guid).unwrap(), "{path}");
        }
        assert_eq!(&packed.read(hero).unwrap()[..], &[1, 2, 3, 250]);
        assert_eq!(packed.meta(hero), disk.meta(hero));

        // Префабы, меши и сцены грузятся без файловой системы.
        let prefab = PrefabSource::load(&packed, "prefabs/coin.prefab").unwrap();
        assert_eq!(prefab.root.name, "Coin");
        let by_guid = packed.guid_of("prefabs/coin.prefab").unwrap().to_string();
        assert!(PrefabSource::load(&packed, &by_guid).is_ok());
        let mesh = packed.load_mesh(packed.guid_of("models/tri.obj").unwrap()).unwrap();
        assert_eq!(mesh.triangle_count(), 1);
        let scenes = AssetScenes::new(packed.clone());
        use engine_core::scenes::SceneSource;
        assert_eq!(scenes.load("level2").unwrap(), "scene text");
        assert_eq!(scenes.load("scenes/level2.scene").unwrap(), "scene text");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pack_reports_missing_files_meta_and_damage() {
        let mut builder = AssetPack::builder();
        builder.add("a.png", vec![1]);
        builder.add("a.png.meta", b"(guid: \"0000000000000000000000000000002a\", importer: \"Texture\")".to_vec());
        let pack = AssetPack::from_bytes(builder.to_bytes()).unwrap();
        let db = AssetDatabase::from_pack(pack).unwrap();
        assert_eq!(db.guid_of("a.png"), Some(Guid(42)));

        // Нет файла: ошибка с подсказкой.
        let err = db.read_path("b.png").unwrap_err().to_string();
        assert!(err.contains("`b.png` not found") && err.contains("a.png"), "{err}");
        let err = db.read(Guid(7)).unwrap_err().to_string();
        assert!(err.contains("GUID"), "{err}");
        let err = PrefabSource::load(&db, "nope.prefab").unwrap_err().to_string();
        assert!(err.contains("nope.prefab"), "{err}");

        // Файл без .meta в паке — понятная ошибка, а не случайный GUID.
        let mut builder = AssetPack::builder();
        builder.add("lonely.png", vec![1]);
        let err = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("lonely.png.meta"), "{err}");

        // Испорченный и обрезанный пак.
        assert!(AssetPack::from_bytes(b"not a pack".to_vec()).is_err());
        let mut builder = AssetPack::builder();
        builder.add("x", vec![0; 100]);
        let mut bytes = builder.to_bytes();
        bytes.truncate(bytes.len() - 10);
        let err = AssetPack::from_bytes(bytes).unwrap_err().to_string();
        assert!(err.contains("truncated"), "{err}");
    }

    #[test]
    fn installed_pack_is_used_by_mount() {
        // Пак процесса — глобальный; этот тест единственный, кто его ставит.
        let mut builder = AssetPack::builder();
        builder.add("only.txt", b"hi".to_vec());
        builder.add("only.txt.meta", b"(guid: \"000000000000000000000000000000aa\", importer: \"Text\")".to_vec());
        install_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap());
        let db = AssetDatabase::mount("this/directory/does/not/exist").unwrap();
        assert!(db.is_packed());
        assert_eq!(&db.read(Guid(0xaa)).unwrap()[..], b"hi");
        *INSTALLED_PACK.lock().unwrap() = None;
    }
}
