//! Предпросмотр 3D-моделей и материалов: миниатюры в окне Project и большой вид с орбитой.
//! Материал показывается на сфере (миниатюра) или на выбранной форме (большой вид).
//!
//! Окно Project работает внутри прохода egui и GPU не видит, поэтому здесь очередь
//! запросов. Интерфейс спрашивает [`Previews::thumbnail`] («модель, нужный размер») и
//! получает готовую текстуру или заглушку; после интерфейса [`PreviewGpu::service`]
//! рисует несколько запрошенных миниатюр прямо тем рендерером и тем устройством, что
//! рисуют сцену редактора, в маленьком временном мире (модель, два источника света,
//! вид). Сцена пользователя не трогается; меши и текстуры лежат в общих кэшах
//! рендерера — модель не грузится дважды.
//!
//! Что стоит рисовать, решает интерфейс: запросы приходят только от плиток, попавших
//! в видимую часть окна, поэтому скрытое окно Project и плитки за краем прокрутки не
//! стоят ничего. Готовая миниатюра лежит в кэше по GUID и обновляется, только когда
//! файл модели или его внешние файлы (`.mtl`, текстуры, буферы) изменились на диске —
//! это проверяется при пересканировании ассетов, а не каждый кадр.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, UNIX_EPOCH};

use engine_assets::{AssetDatabase, AssetHandle, Guid, MeshExtras, MeshInfo};
use engine_core::prelude::*;
use engine_render::{CameraView, Environment, Light, LightKind, MeshRenderer, MeshShape, PostSettings, Renderer};

use crate::gpu::{ensure_target, ViewportTarget};
use crate::state::Editor;

/// Размеры миниатюр в пикселях: запрос округляется вверх до одного из них.
const BUCKETS: [u32; 5] = [32, 64, 128, 256, 512];
/// Сколько миниатюр дорисовывается за кадр: окно остаётся отзывчивым, пока папка заполняется.
const MAX_PER_FRAME: usize = 3;
/// После этого времени (мс) за кадр новые миниатюры не начинаются.
const FRAME_BUDGET_MS: f32 = 12.0;
const FOV_DEG: f32 = 32.0;
/// Фон: тёмный нейтральный, к краям темнеет виньеткой.
const BACKGROUND: u32 = 0x2b2e35;

/// На чём показывать материал в большом виде.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewShape {
    #[default]
    Sphere,
    Cube,
    Plane,
}

impl PreviewShape {
    fn mesh(self) -> MeshShape {
        match self {
            PreviewShape::Sphere => MeshShape::Sphere,
            PreviewShape::Cube => MeshShape::Cube,
            PreviewShape::Plane => MeshShape::Plane,
        }
    }
}

/// Положение камеры вокруг модели. Расстояние не хранится: модель всегда вписана в кадр,
/// `zoom` лишь приближает.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ModelOrbit {
    /// Поворот вокруг вертикали, радианы.
    pub yaw: f32,
    /// Наклон: 0 — вид сбоку, π/2 — сверху.
    pub pitch: f32,
    /// 1 — модель вписана, больше — ближе.
    pub zoom: f32,
}

impl Default for ModelOrbit {
    /// Приятный вид в три четверти: чуть сверху и сбоку.
    fn default() -> Self {
        Self { yaw: 0.75, pitch: 0.38, zoom: 1.0 }
    }
}

/// Счётчики предпросмотра — для тестов и профайлера.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PreviewStats {
    /// Сколько миниатюр нарисовано (повторная отрисовка считается).
    pub thumbnail_renders: u64,
    /// Сколько раз перерисован большой вид справа.
    pub detail_renders: u64,
    /// Модели, которые не удалось показать.
    pub failures: u64,
    /// Время CPU последней отрисовки (сбор сцены, команды, отправка), мс.
    pub last_render_ms: f32,
}

/// Что показать вместо иконки.
pub(crate) enum Thumb {
    Ready(egui::TextureId),
    /// Ещё рисуется (или окно не видно): пока иконка.
    Pending,
    /// Модель не прочиталась; текст — причина.
    Failed(String),
}

enum SlotState {
    Ready { id: egui::TextureId, bucket: u32 },
    Failed(String),
}

struct Slot {
    state: SlotState,
    /// Файлы, от которых зависит картинка, и их отпечаток на момент отрисовки.
    deps: Vec<PathBuf>,
    stamp: u64,
    /// Файлы изменились: показываем старую картинку, пока не нарисована новая.
    stale: bool,
}

struct InfoSlot {
    deps: Vec<PathBuf>,
    stamp: u64,
    result: Result<MeshInfo, String>,
}

/// Состояние большого вида в панели сведений.
pub(crate) struct Detail {
    pub guid: Option<Guid>,
    pub orbit: ModelOrbit,
    /// Медленное вращение, пока пользователь не взял вид в руки.
    pub auto: bool,
    pub texture: Option<egui::TextureId>,
    pub failed: Option<String>,
    /// Форма для материала (у моделей не используется).
    pub shape: PreviewShape,
    /// Что нужно нарисовать в этом кадре (ставит интерфейс, берёт GPU).
    request: Option<DetailRequest>,
    drawn: Option<DetailKey>,
}

struct DetailRequest {
    guid: Guid,
    size: [u32; 2],
}

#[derive(Clone, Copy, PartialEq)]
struct DetailKey {
    guid: Guid,
    orbit: [u32; 3],
    size: [u32; 2],
    serial: u64,
    database: u64,
    shape: PreviewShape,
}

impl Default for Detail {
    fn default() -> Self {
        Self {
            guid: None,
            orbit: ModelOrbit::default(),
            auto: true,
            texture: None,
            failed: None,
            shape: PreviewShape::Sphere,
            request: None,
            drawn: None,
        }
    }
}

/// Кэш и очередь предпросмотра; живёт в [`Editor`].
#[derive(Default)]
pub(crate) struct Previews {
    slots: HashMap<Guid, Slot>,
    infos: HashMap<Guid, InfoSlot>,
    /// Запрошенные в этом кадре миниатюры: GUID и нужный размер.
    requests: Vec<(Guid, u32)>,
    pub detail: Detail,
    pub stats: PreviewStats,
    /// Растёт при каждом пересканировании ассетов: GPU-часть заново берёт базу.
    database: u64,
    /// Растёт, когда изменился файл показанной модели.
    serial: u64,
    /// Чья GPU-часть ([`PreviewGpu`]) нарисовала готовые картинки. Идентификаторы текстур
    /// egui действительны только у неё: другая (новый захват кадра, пересозданное окно)
    /// их не знает, и картинки надо нарисовать заново, а не показывать чужие id.
    gpu: Option<u64>,
}

impl Previews {
    /// Начало кадра: какая GPU-часть будет рисовать. Если не та, что раньше, готовые
    /// картинки забываются (их id принадлежат старому рендереру egui) — до перерисовки
    /// плитки показывают иконку, а не пустое место.
    pub fn attach_gpu(&mut self, generation: u64) {
        if self.gpu == Some(generation) {
            return;
        }
        if self.gpu.is_some() {
            self.slots.retain(|_, slot| matches!(slot.state, SlotState::Failed(_)));
            self.detail.texture = None;
            self.detail.drawn = None;
        }
        self.gpu = Some(generation);
    }

    /// Миниатюра модели размером около `px` физических пикселей. Запрос уходит в очередь,
    /// пока картинки нет или она мельче нужного.
    pub fn thumbnail(&mut self, guid: Guid, px: f32) -> Thumb {
        let bucket = bucket_for(px);
        let (thumb, wanted) = match self.slots.get(&guid) {
            Some(Slot { state: SlotState::Failed(reason), stale: false, .. }) => (Thumb::Failed(reason.clone()), false),
            Some(Slot { state: SlotState::Failed(_), .. }) | None => (Thumb::Pending, true),
            Some(Slot { state: SlotState::Ready { id, bucket: have }, stale, .. }) => (Thumb::Ready(*id), *stale || *have < bucket),
        };
        if wanted {
            match self.requests.iter_mut().find(|(g, _)| *g == guid) {
                Some(request) => request.1 = request.1.max(bucket),
                None => self.requests.push((guid, bucket)),
            }
        }
        thumb
    }

    /// Выбрана другая модель — орбита возвращается к началу.
    pub fn select(&mut self, guid: Option<Guid>) {
        if self.detail.guid != guid {
            self.detail = Detail { guid, shape: self.detail.shape, ..Detail::default() };
        }
    }

    /// Ассет изменился (материал сохранён из инспектора): миниатюра и большой вид
    /// перерисуются, старая картинка видна до тех пор.
    pub fn invalidate(&mut self, guid: Guid) {
        if let Some(slot) = self.slots.get_mut(&guid) {
            slot.stale = true;
        }
        self.serial += 1;
    }

    /// Большой вид нужного размера в пикселях; возвращает текстуру, если она уже есть.
    pub fn request_detail(&mut self, guid: Guid, size: [u32; 2]) {
        self.detail.request = Some(DetailRequest { guid, size });
    }

    /// Сведения о модели для панели справа (вершины, части, материалы), с кэшем.
    pub fn info(&mut self, guid: Guid, database: &AssetDatabase) -> &Result<MeshInfo, String> {
        let slot = self.infos.entry(guid).or_insert_with(|| {
            let result = database.inspect_mesh(guid).map_err(|e| e.to_string());
            let deps = dependencies(database, guid, result.as_ref().map(|i| i.files.as_slice()).unwrap_or(&[]));
            InfoSlot { stamp: stamp_of(&deps), deps, result }
        });
        &slot.result
    }

    /// Модель не удалось показать: запомнить причину (повторных попыток не будет, пока файл
    /// не изменится) и один раз сказать об этом в консоли.
    fn fail(&mut self, database: &AssetDatabase, guid: Guid, reason: String) {
        if matches!(self.slots.get(&guid), Some(Slot { state: SlotState::Failed(_), stale: false, .. })) {
            return;
        }
        self.stats.failures += 1;
        log::warn!("cannot preview `Assets/{}`: {reason}", database.relative_path_of(guid).unwrap_or("?"));
        let deps = dependencies(database, guid, &[]);
        self.slots.insert(guid, Slot { state: SlotState::Failed(reason), stamp: stamp_of(&deps), deps, stale: false });
    }

    /// Ассеты перечитаны: у изменившихся на диске файлов кэш объявляется устаревшим,
    /// у исчезнувших ассетов — удаляется.
    pub fn assets_changed(&mut self, database: Option<&AssetDatabase>) {
        self.database += 1;
        let alive = |guid: &Guid| database.is_some_and(|db| db.relative_path_of(*guid).is_some());
        self.slots.retain(|guid, _| alive(guid));
        self.infos.retain(|guid, info| alive(guid) && stamp_of(&info.deps) == info.stamp);
        let mut changed = false;
        for slot in self.slots.values_mut() {
            if !slot.stale && stamp_of(&slot.deps) != slot.stamp {
                slot.stale = true;
                changed = true;
            }
        }
        if changed {
            self.serial += 1;
        }
    }
}

fn bucket_for(px: f32) -> u32 {
    let px = px.max(1.0);
    BUCKETS.iter().copied().find(|b| *b as f32 >= px).unwrap_or(BUCKETS[BUCKETS.len() - 1])
}

/// Файлы модели на диске: сама модель и всё, что она подтягивает.
fn dependencies(database: &AssetDatabase, guid: Guid, files: &[String]) -> Vec<PathBuf> {
    let Some(path) = database.path_of(guid) else {
        return Vec::new();
    };
    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
    let mut deps = vec![path];
    deps.extend(files.iter().map(|f| dir.join(f)));
    deps
}

/// Отпечаток файлов (время изменения и размер): другой — значит, файлы менялись.
fn stamp_of(files: &[PathBuf]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    };
    for file in files {
        if let Ok(meta) = std::fs::metadata(file) {
            let nanos = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as u64);
            mix(nanos);
            mix(meta.len());
        } else {
            mix(1);
        }
    }
    hash
}

// ----------------------------------------------------------------------------------
// Временный мир и камера
// ----------------------------------------------------------------------------------

/// Вид на модель: куда смотрит камера и откуда светят два источника.
struct Framing {
    camera: CameraView,
    key_direction: Vec3,
    fill_position: Vec3,
    fill_range: f32,
}

/// Расстояние, на котором коробка с половинными размерами `half` (вокруг центра) целиком
/// в кадре при взгляде под углами `yaw`/`pitch`.
fn fit_distance(half: Vec3, yaw: f32, pitch: f32, tan_x: f32, tan_y: f32) -> f32 {
    let toward_camera = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
    let right = (-toward_camera).cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = right.cross(-toward_camera);
    let mut needed = 0.0_f32;
    for corner in 0..8 {
        let sign = |bit: u32| if corner & bit == 0 { -1.0 } else { 1.0 };
        let rel = Vec3::new(half.x * sign(1), half.y * sign(2), half.z * sign(4));
        needed = needed.max(rel.dot(toward_camera) + (rel.dot(right).abs() / tan_x).max(rel.dot(up).abs() / tan_y));
    }
    needed
}

/// Вписать ограничивающий параллелепипед `bounds` в кадр с отношением сторон `aspect`.
/// `tight` — ровно под нынешний угол (миниатюры: модель заполняет кадр); иначе берётся
/// худший случай по всем поворотам вокруг вертикали: расстояние зависит только от наклона,
/// и вид не «дышит», пока модель вращается.
fn frame_model(bounds: (Vec3, Vec3), orbit: ModelOrbit, aspect: f32, tight: bool) -> Framing {
    let (min, max) = bounds;
    let center = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    let radius = half.length().max(1e-3);
    let toward_camera = Vec3::new(
        orbit.yaw.sin() * orbit.pitch.cos(),
        orbit.pitch.sin(),
        orbit.yaw.cos() * orbit.pitch.cos(),
    );
    let forward = -toward_camera;
    let right = forward.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = right.cross(forward);
    let tan_y = (FOV_DEG.to_radians() * 0.5).tan();
    let tan_x = tan_y * aspect.max(0.1);
    let mut distance = if tight {
        fit_distance(half, orbit.yaw, orbit.pitch, tan_x, tan_y) * 1.07
    } else {
        let worst = (0..24)
            .map(|step| fit_distance(half, step as f32 * std::f32::consts::TAU / 24.0, orbit.pitch, tan_x, tan_y))
            .fold(0.0_f32, f32::max);
        worst * 1.08
    };
    distance = (distance / orbit.zoom.max(0.05)).max(radius * 0.05);
    let eye = center + toward_camera * distance;
    let background = Color::hex(BACKGROUND);
    let mut camera = CameraView::look_at(eye, center, FOV_DEG, aspect, background, false);
    camera.post = Some(PostSettings { enabled: true, exposure: 1.0, bloom: 0.0, bloom_threshold: 1.0, vignette: 0.55 });
    // Свет привязан к камере: при вращении модель не уходит в тень.
    let key_from = -forward * 0.75 - right * 0.55 + up * 0.7;
    let fill_position = center + (-forward * 0.15 + right * 0.9 - up * 0.1).normalize() * radius * 2.4;
    Framing { camera, key_direction: -key_from.normalize(), fill_position, fill_range: radius * 7.0 }
}

/// Временный мир: модель и свет. Камера — значение [`CameraView`], объекта-камеры нет.
struct PreviewScene {
    world: World,
    model: GameObjectId,
    key: GameObjectId,
    fill: GameObjectId,
}

impl PreviewScene {
    fn new() -> Self {
        let mut world = World::new();
        let model = world.scene.build("model").with(MeshRenderer::default()).id();
        let key = world
            .scene
            .build("key light")
            .with(Light { color: Color::rgb(1.0, 0.96, 0.9), intensity: 2.4, shadows: false, ..Light::default() })
            .id();
        let fill = world
            .scene
            .build("fill light")
            .with(Light::point(Color::rgb(0.62, 0.72, 1.0), 1.7, 10.0))
            .id();
        world
            .scene
            .build("environment")
            .with(Environment { ambient: 0.4, fog_density: 0.0, exposure: 1.0, bloom: 0.0, vignette: 0.0, shadow_distance: 10.0 });
        Self { world, model, key, fill }
    }

    /// Материалы модели из её файла (каждая часть — своим); модель без материалов —
    /// светло-серая.
    fn dress(&mut self, database: &AssetDatabase, guid: Guid, extras: &MeshExtras) {
        let mut renderer = MeshRenderer::asset(engine_assets::AssetHandle::from_guid(guid));
        renderer.model_materials = !extras.materials.is_empty();
        renderer.color = Color::hex(0xcfd2d8);
        renderer.roughness = 0.55;
        if let Some(material) = extras.materials.first() {
            renderer.color = Color::rgba(material.color[0], material.color[1], material.color[2], 1.0);
            if let (Some(texture), Some(model)) = (&material.texture, database.relative_path_of(guid)) {
                let dir = model.rsplit_once('/').map_or("", |(dir, _)| dir);
                if let Some(texture) = database.guid_of(&join_normalized(dir, texture)) {
                    renderer.texture = engine_assets::AssetHandle::from_guid(texture);
                }
            }
        }
        if let Some(slot) = self.world.scene.get_mut(self.model).and_then(|o| o.get_mut::<MeshRenderer>()) {
            *slot = renderer;
        }
    }

    /// Поставить в сцену то, что показываем: модель или форму с материалом. Возвращает
    /// границы и внешние файлы (пути относительно ассета), от которых зависит картинка.
    fn prepare(&mut self, scene: &mut Renderer, database: &AssetDatabase, guid: Guid, shape: PreviewShape) -> Result<((Vec3, Vec3), Vec<String>), String> {
        if database.meta(guid).is_some_and(|meta| meta.importer == "Material") {
            let material = database.load_material(guid).map_err(|e| e.to_string())?;
            // Изменения файла рендерер должен увидеть и тогда, когда их сделали снаружи.
            scene.reload_material(guid);
            let dir = database.relative_path_of(guid).and_then(|p| p.rsplit_once('/')).map_or("", |(dir, _)| dir);
            let files = material
                .textures()
                .filter_map(|texture| database.relative_path_of(texture))
                .map(|path| relative_to(dir, path))
                .collect();
            let renderer = MeshRenderer::shape(shape.mesh()).with_material_asset(AssetHandle::from_guid(guid));
            if let Some(slot) = self.world.scene.get_mut(self.model).and_then(|o| o.get_mut::<MeshRenderer>()) {
                *slot = renderer;
            }
            return Ok((shape.mesh().bounds(), files));
        }
        let bounds = scene.preload_mesh(guid, database)?;
        let extras = database.mesh_extras(guid).unwrap_or_default();
        self.dress(database, guid, &extras);
        Ok((bounds, extras.files))
    }

    fn aim(&mut self, framing: &Framing) {
        let scene = &mut self.world.scene;
        if let Some(key) = scene.get_mut(self.key) {
            key.transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, framing.key_direction);
        }
        if let Some(fill) = scene.get_mut(self.fill) {
            fill.transform.position = framing.fill_position;
            if let Some(light) = fill.get_mut::<Light>() {
                light.kind = LightKind::Point;
                light.range = framing.fill_range;
            }
        }
    }
}

/// Путь `path` (от корня ассетов) относительно каталога `dir` — так [`dependencies`]
/// ищет файлы рядом с ассетом.
fn relative_to(dir: &str, path: &str) -> String {
    let dir_parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    let path_parts: Vec<&str> = path.split('/').collect();
    let common = dir_parts.iter().zip(&path_parts).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = vec![".."; dir_parts.len() - common];
    out.extend(&path_parts[common..]);
    out.join("/")
}

/// `dir` + `relative` с разбором `.` и `..`, разделители — `/`.
fn join_normalized(dir: &str, relative: &str) -> String {
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

// ----------------------------------------------------------------------------------
// GPU-часть
// ----------------------------------------------------------------------------------

/// Ресурсы GPU предпросмотра: временный мир и текстуры миниатюр; живёт в `EditorGpu`.
pub(crate) struct PreviewGpu {
    scene: Option<PreviewScene>,
    thumbs: HashMap<Guid, Option<ViewportTarget>>,
    detail: Option<ViewportTarget>,
    /// С какой версией базы ассетов синхронизирован временный мир.
    synced: Option<u64>,
    /// Номер этой GPU-части (см. [`Previews::attach_gpu`]).
    generation: u64,
    /// Текстуры миниатюр исчезнувших ассетов: освобождаются кадром позже, когда
    /// интерфейс их уже точно не рисует.
    retired: Vec<egui::TextureId>,
}

impl Default for PreviewGpu {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            scene: None,
            thumbs: HashMap::new(),
            detail: None,
            synced: None,
            generation: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            retired: Vec::new(),
        }
    }
}

impl PreviewGpu {
    /// Номер этой GPU-части — для [`Previews::attach_gpu`] перед интерфейсом кадра.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Нарисовать то, что запросил интерфейс в этом кадре: несколько миниатюр и большой
    /// вид. Вызывается после интерфейса и до его отрисовки, поэтому большой вид,
    /// повёрнутый в этом кадре, попадает в этот же кадр.
    pub fn service(&mut self, editor: &mut Editor, scene: &mut Renderer, egui: &mut egui_wgpu::Renderer, ctx: &egui::Context) {
        let previews = &mut editor.previews;
        // Миниатюры, чьи ассеты исчезли, освобождаются — но кадром позже: в этом кадре
        // интерфейс мог успеть взять их id до того, как ассет исчез.
        for id in self.retired.drain(..) {
            egui.free_texture(&id);
        }
        let retired = &mut self.retired;
        self.thumbs.retain(|guid, target| {
            let keep = previews.slots.contains_key(guid);
            if let (false, Some(target)) = (keep, target) {
                retired.push(target.id);
            }
            keep
        });
        if previews.requests.is_empty() && previews.detail.request.is_none() {
            return;
        }
        let Some(database) = editor.app.world.resources.get::<AssetDatabase>() else {
            previews.requests.clear();
            previews.detail.request = None;
            return;
        };
        let preview_scene = self.scene.get_or_insert_with(PreviewScene::new);
        if self.synced != Some(previews.database) {
            self.synced = Some(previews.database);
            preview_scene.world.resources.insert(database.clone());
        }

        let started = Instant::now();
        let requests = std::mem::take(&mut previews.requests);
        let mut done = 0;
        for (guid, bucket) in &requests {
            if done >= MAX_PER_FRAME || (done > 0 && started.elapsed().as_secs_f32() * 1000.0 > FRAME_BUDGET_MS) {
                break;
            }
            done += 1;
            let clock = Instant::now();
            let outcome = render_thumbnail(preview_scene, scene, egui, &mut self.thumbs, database, *guid, *bucket);
            previews.stats.last_render_ms = clock.elapsed().as_secs_f32() * 1000.0;
            match outcome {
                Ok((id, deps)) => {
                    previews.stats.thumbnail_renders += 1;
                    let stamp = stamp_of(&deps);
                    previews.slots.insert(*guid, Slot { state: SlotState::Ready { id, bucket: *bucket }, deps, stamp, stale: false });
                }
                Err(reason) => previews.fail(database, *guid, reason),
            }
        }
        if done > 0 {
            // Только что нарисованные миниатюры интерфейс покажет со следующего кадра;
            // остальные из очереди дорисуются тоже там: интерфейс спросит их снова.
            ctx.request_repaint();
        }

        if let Some(request) = previews.detail.request.take() {
            let key = DetailKey {
                guid: request.guid,
                orbit: [previews.detail.orbit.yaw.to_bits(), previews.detail.orbit.pitch.to_bits(), previews.detail.orbit.zoom.to_bits()],
                size: request.size,
                serial: previews.serial,
                database: previews.database,
                shape: previews.detail.shape,
            };
            // Модель, которую уже не удалось прочитать, не пробуется снова, пока не изменится файл.
            let known_failure = match previews.slots.get(&request.guid) {
                Some(Slot { state: SlotState::Failed(reason), stale: false, .. }) => Some(reason.clone()),
                _ => None,
            };
            if let Some(reason) = known_failure {
                previews.detail.failed = Some(reason);
            } else if previews.detail.drawn != Some(key) || self.detail.is_none() {
                let clock = Instant::now();
                match render_detail(preview_scene, scene, egui, &mut self.detail, database, &request, previews.detail.orbit, previews.detail.shape) {
                    Ok(id) => {
                        if previews.detail.texture.is_none() {
                            ctx.request_repaint();
                        }
                        previews.detail.texture = Some(id);
                        previews.detail.failed = None;
                        previews.stats.detail_renders += 1;
                        previews.stats.last_render_ms = clock.elapsed().as_secs_f32() * 1000.0;
                    }
                    Err(reason) => {
                        // Запоминаем в кэше: плитка той же модели не будет пробовать снова,
                        // а в консоль уйдёт одно предупреждение на модель.
                        previews.fail(database, request.guid, reason.clone());
                        previews.detail.failed = Some(reason);
                    }
                }
                previews.detail.drawn = Some(key);
            }
        }
    }
}

/// Нарисовать миниатюру: меш на GPU → вид 3/4 → своя текстура. Возвращает идентификатор
/// текстуры egui и файлы, от которых зависит картинка.
fn render_thumbnail(
    preview: &mut PreviewScene,
    scene: &mut Renderer,
    egui: &mut egui_wgpu::Renderer,
    thumbs: &mut HashMap<Guid, Option<ViewportTarget>>,
    database: &AssetDatabase,
    guid: Guid,
    bucket: u32,
) -> Result<(egui::TextureId, Vec<PathBuf>), String> {
    let (bounds, files) = preview.prepare(scene, database, guid, PreviewShape::Sphere)?;
    let framing = frame_model(bounds, ModelOrbit::default(), 1.0, true);
    preview.aim(&framing);
    let slot = thumbs.entry(guid).or_insert(None);
    let size = [bucket, bucket];
    let id = ensure_target(scene, egui, slot, size);
    let target = slot.as_ref().ok_or("no preview target")?;
    scene.render_with(&preview.world, &target.view, size[0], size[1], &framing.camera);
    Ok((id, dependencies(database, guid, &files)))
}

/// Большой вид: та же сцена, камера по орбите пользователя, размер панели.
#[allow(clippy::too_many_arguments)]
fn render_detail(
    preview: &mut PreviewScene,
    scene: &mut Renderer,
    egui: &mut egui_wgpu::Renderer,
    slot: &mut Option<ViewportTarget>,
    database: &AssetDatabase,
    request: &DetailRequest,
    orbit: ModelOrbit,
    shape: PreviewShape,
) -> Result<egui::TextureId, String> {
    let (bounds, _) = preview.prepare(scene, database, request.guid, shape)?;
    let size = request.size;
    let framing = frame_model(bounds, orbit, size[0] as f32 / size[1].max(1) as f32, false);
    preview.aim(&framing);
    let id = ensure_target(scene, egui, slot, size);
    let target = slot.as_ref().ok_or("no preview target")?;
    scene.render_with(&preview.world, &target.view, size[0], size[1], &framing.camera);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_paths_are_relative_to_the_material() {
        assert_eq!(relative_to("materials", "textures/a.png"), "../textures/a.png");
        assert_eq!(relative_to("materials", "materials/a.png"), "a.png");
        assert_eq!(relative_to("", "a/b.png"), "a/b.png");
        assert_eq!(join_normalized("materials", &relative_to("materials", "textures/a.png")), "textures/a.png");
    }

    #[test]
    fn requests_round_up_to_a_bucket() {
        assert_eq!(bucket_for(16.0), 32);
        assert_eq!(bucket_for(72.0), 128);
        assert_eq!(bucket_for(432.0), 512);
        assert_eq!(bucket_for(5000.0), 512);
    }

    #[test]
    fn paths_next_to_a_model_are_normalized() {
        assert_eq!(join_normalized("models", "tex/a.png"), "models/tex/a.png");
        assert_eq!(join_normalized("models/sub", "../a.png"), "models/a.png");
        assert_eq!(join_normalized("", "./a.png"), "a.png");
    }

    #[test]
    fn a_cube_fills_the_frame_tightly_without_leaving_it() {
        let bounds = (Vec3::splat(-1.0), Vec3::splat(1.0));
        let orbit = ModelOrbit::default();
        let framing = frame_model(bounds, orbit, 1.0, true);
        // Все углы коробки внутри кадра, и хотя бы один рядом с краем.
        let mut widest = 0.0_f32;
        for corner in 0..8 {
            let p = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            let clip = framing.camera.view_proj * p.extend(1.0);
            let ndc = clip.truncate() / clip.w;
            widest = widest.max(ndc.x.abs()).max(ndc.y.abs());
        }
        assert!(widest <= 1.0, "the model leaves the frame: {widest}");
        assert!(widest > 0.85, "the model does not fill the frame: {widest}");
    }

    #[test]
    fn the_big_view_does_not_breathe_while_the_model_turns() {
        let bounds = (Vec3::new(-1.0, -0.2, -0.4), Vec3::new(1.0, 0.2, 0.4));
        let distance = |yaw: f32| {
            let orbit = ModelOrbit { yaw, ..ModelOrbit::default() };
            let framing = frame_model(bounds, orbit, 1.2, false);
            framing.camera.position.length()
        };
        let first = distance(0.0);
        for step in 1..12 {
            let other = distance(step as f32 * 0.5);
            assert!((other - first).abs() < 1e-3, "distance changed with yaw: {first} vs {other}");
        }
    }
}
