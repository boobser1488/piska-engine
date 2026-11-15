//! Рендерер кадра: тени → небо → меши (PBR) → спрайты в HDR-цель с глубиной,
//! затем пост-обработка (блум, тонмаппинг) в цель кадра.
//!
//! Извлечение — один проход по активным объектам (без `scene.ids()` и подъёма к
//! родителям, см. `walk.rs`), с отсечением по пирамиде камеры.
//! Спрайты: сортировка → батчи по текстуре; непересекающиеся спрайты переносятся в
//! батч своей текстуры (`batch.rs`). Меши: инстансинг — пачка одинаковых мешей с одной
//! текстурой рисуется одним draw call'ом, параметры объектов идут вторым вершинным
//! буфером (`frame.rs`); в карту теней попадают только меши из объёма солнца.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use engine_assets::{AssetDatabase, Guid, MeshData, Materials};
use engine_core::prelude::*;
use wgpu::util::DeviceExt;

use crate::components::{
    Background, Camera, Environment, LightKind, MeshRenderer, Projection,
};
use crate::grid::SceneGrid;
use crate::draw2d::{
    push_quad, sprite_corners, sprite_uv, Blend, CameraBasis, DrawRun, SpriteVertex, TexRef, QUAD_VERTICES,
};
use crate::particles::{ParticleBlend, ParticleShape};
use crate::post::{PostChain, PostSettings, HDR_FORMAT};
use crate::primitives::MeshShape;
use crate::RenderError;
use engine_core::profile::Stopwatch;
use engine_core::settings::GraphicsSettings;

/// Значение из настроек, если оно конечное (файл могли поправить руками).
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

mod batch;
mod custom;
mod frame;
mod grid_pass;
mod lights_pass;
mod material;
mod overlay_pass;
mod terrain_pass;
mod variant;
mod walk;
mod water_pass;

pub use terrain_pass::TerrainStats;

use batch::{needs_rects, screen_rect, Batcher, ScreenRect};
use frame::{
    aabb_transform, build_batches, MeshInstance, MeshItem, MeshScratch, RecentCache, RenderParts,
    INSTANCE_ATTRIBUTES, INSTANCE_MODEL_ATTRIBUTES, WHOLE_MESH,
};
use material::{FrameMaterials, MaterialObject, PASS_BLEND, SHADOW_CUTOUT};
use walk::{Frustum, SceneWalk};

/// Формат цели для захвата кадра без окна.
const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;
/// Сглаживание краёв: 4 выборки на пиксель (MSAA).
const SAMPLES: u32 = 4;
const MAX_POINT_LIGHTS: usize = 4;
use crate::soft_shadows::{fit_cascades, Cascade, CascadeSetup, SoftShadows, MAX_CASCADES};
use crate::lights::{LightFrame, LightSetup, LightShadows, LocalLight, SpotLight};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

/// Параметры кадра, общие для всех шейдеров (см. `Globals` в .wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    inv_view_proj: [f32; 16],
    light_view_proj: [f32; 16],
    camera_position: [f32; 4],
    sun_direction: [f32; 4],
    sun_color: [f32; 4],
    sky_zenith: [f32; 4],
    sky_horizon: [f32; 4],
    shadow_params: [f32; 4],
    counts: [f32; 4],
    points: [[f32; 8]; MAX_POINT_LIGHTS],
    cascade_view_proj: [[f32; 16]; MAX_CASCADES],
    cascade_data: [[f32; 4]; MAX_CASCADES],
    cascade_params: [f32; 4],
    sky_sun: [f32; 4],
    sky_clouds: [f32; 4],
    sky_cloud_color: [f32; 4],
    sky_night: [f32; 4],
    cluster: [f32; 4],
    lights_info: [f32; 4],
    fill_sky: [f32; 4],
    fill_ground: [f32; 4],
}

struct GpuTexture {
    /// Для спрайтов: края не повторяются.
    bind_group: wgpu::BindGroup,
    /// Поправка UV внутрь кадра листа (полтекселя при линейной фильтрации).
    inset: Vec2,
    /// Сама текстура — для групп привязок материалов.
    view: wgpu::TextureView,
    /// Пиксель-арт: без сглаживания (материал берёт резкий сэмплер).
    nearest: bool,
    /// Пиксель-арт с мипмапами (`"filter": Enum("Pixel")`): вблизи — резкие пиксели,
    /// вдали — трилинейное уменьшение (не рябит).
    pixel: bool,
    /// Атлас модели Piska Modeler с линейной фильтрацией: без мипмапов и без анизотропии
    /// (прямоугольники граней лежат вплотную, мипы и анизотропия смешали бы соседей).
    atlas: bool,
}

struct GpuMesh {
    /// Порядковый номер — ключ сортировки пачек.
    id: u32,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    bounds: (Vec3, Vec3),
    /// Касательные (для карт нормалей) — третий вершинный буфер.
    tangents: wgpu::Buffer,
    /// Части меша: диапазон индексов и номер материала в файле модели.
    parts: Vec<(Range<u32>, Option<u32>)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MeshKey {
    Shape(u8),
    Asset(Guid),
}

fn shape_key(shape: MeshShape) -> MeshKey {
    MeshKey::Shape(match shape {
        MeshShape::Cube => 0,
        MeshShape::Sphere => 1,
        MeshShape::Plane => 2,
        MeshShape::Cylinder => 3,
    })
}

/// Статистика последнего кадра — её показывает окно Profiler редактора.
/// Лежит в `world.resources` после каждого кадра в оконном режиме.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub sprites: u32,
    pub meshes: u32,
    pub triangles: u32,
    /// Сколько текстур сейчас загружено на GPU.
    pub textures: u32,
    /// Из `draw_calls` — проход теней (пачка одинаковых мешей = один вызов).
    pub shadow_draw_calls: u32,
    /// Мешей в карте теней (только попавшие в объём солнца).
    pub shadow_casters: u32,
    /// Мешей, отброшенных отсечением по пирамиде камеры.
    pub culled_meshes: u32,
    /// Спрайтов, отброшенных отсечением по пирамиде камеры.
    pub culled_sprites: u32,
    /// Квадов тайлов (только видимые ячейки карт).
    pub tiles: u32,
    /// Сколько мировых матриц посчитано при извлечении кадра.
    pub world_matrices: u32,
    /// Время CPU на извлечение кадра (обход сцены, отсечение, сортировка), мс.
    pub extract_ms: f32,
    /// Время CPU на загрузку буферов и запись команд, мс.
    pub encode_ms: f32,
    /// Из `meshes` — полупрозрачные (рисуются после непрозрачных, от дальних к ближним).
    pub transparent_meshes: u32,
    /// Сколько разных групп привязок материалов создано (одна на набор текстур и общих
    /// параметров материала).
    pub material_bind_groups: u32,
    /// Из `extract_ms` — обход сцены: мировые матрицы, отсечение, сбор мешей и спрайтов, мс.
    pub walk_ms: f32,
    /// Отправка команд кадра в очередь GPU (`queue.submit`), мс.
    pub submit_ms: f32,
    /// Смены состояния в проходах мешей, теней и спрайтов: конвейер, группа привязок,
    /// вершинный / индексный буфер.
    pub state_changes: u32,
    /// Байт, загруженных на GPU за кадр (вершины спрайтов, экземпляры мешей, параметры
    /// кадра); без однократной загрузки текстур и мешей.
    pub upload_bytes: u64,
}

/// Откуда и как смотреть на мир в этом кадре. Обычно её даёт камера сцены
/// ([`Renderer::scene_view`]); редактор подставляет свою.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    /// Матрица «мир → clip-пространство».
    pub view_proj: Mat4,
    /// Где стоит камера (нужно для бликов).
    pub position: Vec3,
    pub clear_color: Color,
    /// Рисовать процедурное небо вместо заливки цветом.
    pub sky: bool,
    /// Пост-обработка: тонмаппинг, блум, виньетка. `None` — как задано в сцене
    /// (компонент `Environment`; без него — включена для перспективной камеры).
    pub post: Option<PostSettings>,
    /// Битовая маска видимых слоёв.
    pub culling_mask: u32,
    /// Рисовать экранный слой (игровой интерфейс, [`Overlay`](crate::Overlay)).
    /// Вид из камеры сцены — да; свободные виды редактора — нет.
    pub overlay: bool,
    /// Сетка плоскости (только виды редактора), глубина-тестируемая: модели её закрывают.
    /// У игровых камер `None`. См. [`SceneGrid`].
    pub grid: Option<SceneGrid>,
    /// Применять туман сцены (`Environment`). Вид Scene редактора по умолчанию — без
    /// тумана, как переключатель Fog в Unity; игровые камеры — всегда с ним.
    pub fog: bool,
}

impl CameraView {
    /// Ортографический вид в плоскости XY: центр и половина высоты в мировых единицах.
    pub fn orthographic_2d(center: Vec2, half_height: f32, aspect: f32, clear_color: Color) -> Self {
        let half_h = half_height.max(1e-4);
        let half_w = half_h * aspect;
        let projection = crate::depth::orthographic(-half_w, half_w, -half_h, half_h, -1000.0, 1000.0);
        let view = Mat4::from_translation(-center.extend(0.0));
        Self {
            view_proj: projection * view,
            position: center.extend(1000.0),
            clear_color,
            sky: false,
            post: Some(PostSettings::OFF),
            culling_mask: u32::MAX,
            overlay: false,
            grid: None,
            fog: true,
        }
    }

    /// Перспективный вид из точки `eye` на точку `target` (ось Y — вверх).
    pub fn look_at(eye: Vec3, target: Vec3, fov_deg: f32, aspect: f32, clear_color: Color, sky: bool) -> Self {
        let projection = crate::depth::perspective(fov_deg.to_radians(), aspect.max(1e-4), 0.05, 2000.0);
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        Self {
            view_proj: projection * view,
            position: eye,
            clear_color,
            sky,
            post: Some(PostSettings::default()),
            culling_mask: u32::MAX,
            overlay: false,
            grid: None,
            fog: true,
        }
    }
}

/// Адаптер + устройство + очередь.
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub async fn request(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                // В браузере Chrome на Windows параметр игнорирует и предупреждает об этом.
                power_preference: if cfg!(target_arch = "wasm32") {
                    wgpu::PowerPreference::None
                } else {
                    wgpu::PowerPreference::HighPerformance
                },
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RenderError::NoAdapter)?;
        check_adapter(&adapter)?;
        // WebGL2 не дотягивает даже до `downlevel_defaults` (нет storage-буферов и
        // compute) — для него свой минимум. Рендер ни того, ни другого не использует.
        let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::downlevel_defaults()
        };
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("engine device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: base_limits.using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|e| RenderError::Device(e.to_string()))?;
        Ok(Self {
            adapter,
            device,
            queue,
        })
    }
}

/// Проверить, что адаптер умеет то, на чём держится кадр: HDR-цель `Rgba16Float` с
/// MSAA ×4 и разрешением выборок, сравнивающий сэмплер для теней. На десктопе и в
/// WebGPU это есть всегда; в WebGL2 — только с расширением `EXT_color_buffer_float`.
fn check_adapter(adapter: &wgpu::Adapter) -> Result<(), RenderError> {
    let hdr = adapter.get_texture_format_features(HDR_FORMAT);
    let needed = wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4
        | wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE
        | wgpu::TextureFormatFeatureFlags::FILTERABLE;
    let renderable = hdr.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT);
    if !renderable || !hdr.flags.contains(needed) {
        return Err(RenderError::Unsupported(format!(
            "{} ({:?}) cannot render to an HDR target ({HDR_FORMAT:?} with 4x MSAA); \
             in a browser this needs WebGPU or WebGL2 with EXT_color_buffer_float",
            adapter.get_info().name,
            adapter.get_info().backend
        )));
    }
    Ok(())
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,

    sprite_pipeline: wgpu::RenderPipeline,
    /// Те же спрайты со сложением цветов (светящиеся частицы).
    sprite_additive_pipeline: wgpu::RenderPipeline,
    /// Конвейеры мешей и кэши материалов (renderer/material.rs).
    mesh_pass: material::MeshPass,
    shadow_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,

    globals_buffer: wgpu::Buffer,
    /// Только параметры кадра — для спрайтов, неба и прохода теней.
    globals_bind_group: wgpu::BindGroup,
    /// Параметры кадра + карта теней — для мешей и рельефа, по варианту шейдера
    /// (renderer/variant.rs: одна карта или каскады).
    lit_binds: Vec<Option<wgpu::BindGroup>>,
    /// Вариант шейдера мешей и рельефа этого кадра.
    frame_variant: usize,
    /// Карта теней для чтения (все слои).
    shadow_view: wgpu::TextureView,
    /// Слои карты теней — цели проходов теней (слой 0 — одна карта без каскадов).
    shadow_layers: Vec<wgpu::TextureView>,
    shadow_sampler: wgpu::Sampler,
    lit_layouts: Vec<wgpu::BindGroupLayout>,
    globals_layout: wgpu::BindGroupLayout,
    /// Параметры кадра для прохода теней каждого каскада (своя `light_view_proj`).
    cascade_globals: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    /// Параметры кадра для граней теней ламп (своя `light_view_proj`).
    face_globals: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    /// Текстуры ламп (lights.rs) — с первого кадра «с лампами».
    lights_gpu: Option<lights_pass::LightsGpu>,
    /// Лампы кадра: сбор при обходе и раскладка по кластерам (буферы переиспользуются).
    local_lights: Vec<LocalLight>,
    light_frame: LightFrame,

    /// Экземпляры мешей кадра (второй вершинный буфер).
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    mesh_scratch: MeshScratch,
    next_mesh_id: u32,
    batcher: Batcher,
    /// Буфер обхода сцены, переиспользуется между кадрами.
    walk_stack: Vec<(GameObjectId, u32)>,

    texture_layout: wgpu::BindGroupLayout,
    sampler_linear: wgpu::Sampler,
    sampler_nearest: wgpu::Sampler,
    sampler_tiled: wgpu::Sampler,
    sampler_tiled_nearest: wgpu::Sampler,
    /// Пиксель-арт с мипмапами: увеличение — ближайший тексель, уменьшение — трилинейно.
    sampler_tiled_pixel: wgpu::Sampler,
    /// Линейный атлас модели: с повторением, без мипмапов и анизотропии.
    sampler_atlas: wgpu::Sampler,
    /// Предупреждение о матрице вида с обычной глубиной — один раз.
    warned_standard_depth: bool,
    post: PostChain,
    /// Белая 1×1 — для спрайтов и мешей без текстуры (чистый цвет).
    white: GpuTexture,
    /// Пурпурная «шахматка» — заметная замена для ненайденной текстуры.
    missing: GpuTexture,
    /// Встроенные формы частиц: мягкое пятно и круг с чётким краем.
    soft_disc: GpuTexture,
    hard_disc: GpuTexture,
    textures: HashMap<Guid, GpuTexture>,
    failed: HashSet<Guid>,

    meshes: HashMap<MeshKey, GpuMesh>,
    failed_meshes: HashSet<Guid>,

    sprite_vertices: wgpu::Buffer,
    sprite_capacity: usize,
    /// Индексы квадов (0 1 2 0 2 3 со сдвигом 4 на квад), на `sprite_capacity / 4` квадов.
    quad_indices: wgpu::Buffer,
    /// Рабочие буферы спрайтов: переиспользуются от кадра к кадру без аллокаций.
    sprite_scratch: SpriteScratch,
    /// Многовыборочные цвет и глубина под размер цели.
    depth: Option<(wgpu::TextureView, wgpu::TextureView, [u32; 2])>,
    warned_no_camera: bool,
    /// Экранный слой (интерфейс) поверх кадра.
    overlay: overlay_pass::OverlayPass,
    /// Сетка вида редактора, см. grid.rs.
    grid: grid_pass::GridPass,
    /// Рельефы и вода, см. terrain_pass.rs.
    terrain: terrain_pass::TerrainPass,
    /// Вода с эффектами (`WaterEffects`), см. water_pass.rs.
    water: water_pass::WaterPass,
    /// Счётчик смен состояния кадра (`RenderStats::state_changes`); проходы получают
    /// `&self`, поэтому атомарный.
    state_changes: std::sync::atomic::AtomicU32,
    /// AO и контур (post_fx.rs) — создаются при первом кадре, где они нужны.
    post_fx: Option<crate::post_fx::PostFx>,
    fx_supported: bool,
    /// Растёт при каждом пересоздании многовыборочной глубины (группы эффектов — заново).
    depth_generation: u32,
    /// Свои шейдеры (`.shader`, renderer/custom.rs) — создаются при первом из них.
    custom: Option<Box<custom::CustomShaders>>,
}

/// Буферы, которые спрайтовая часть кадра заполняет заново, но не пересоздаёт.
#[derive(Default)]
struct SpriteScratch {
    /// Заходы: спрайт, система частиц, карта тайлов — каждый со своим куском `raw`.
    runs: Vec<DrawRun>,
    /// Вершины в порядке извлечения.
    raw: Vec<SpriteVertex>,
    batches: Vec<(TexRef, Blend, Range<u32>)>,
    /// Мировые границы каждого захода (параллельно `runs`); `NaN` — ещё не посчитаны
    /// (частицы, тайлы, следы: считаются по вершинам, только если понадобятся).
    bounds: Vec<(Vec3, Vec3)>,
    /// Прямоугольники заходов на экране — для переноса между батчами.
    rects: Vec<ScreenRect>,
}

impl SpriteScratch {
    /// Закрыть заход, начатый с `run.start`: длина и границы — по записанным вершинам.
    fn push_run(&mut self, mut run: DrawRun) {
        run.len = self.raw.len() as u32 - run.start;
        if run.len == 0 {
            return;
        }
        // Границы нужны, только если в группе захода есть другие текстуры (см.
        // `batch::needs_rects`); тогда их посчитают по вершинам. Обычно — не нужны.
        self.runs.push(run);
        self.bounds.push((Vec3::NAN, Vec3::NAN));
    }
}

/// Границы куска вершин.
fn vertex_bounds(vertices: &[SpriteVertex]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for vertex in vertices {
        let p = Vec3::from_array(vertex.position);
        min = min.min(p);
        max = max.max(p);
    }
    (min, max)
}

impl Renderer {
    /// Рендерер без окна — для захвата кадров в тестах и на CI. В браузере его нет:
    /// поток страницы нельзя блокировать ожиданием GPU (там — [`Gpu::request`] + `.await`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let gpu = pollster::block_on(Gpu::request(&instance, None))?;
        let mut renderer = Self::new(gpu.device, gpu.queue, CAPTURE_FORMAT);
        // Снимок без окна — один кадр: рельеф строится сразу, целиком.
        renderer.terrain.blocking = true;
        Ok(renderer)
    }

    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });
        // Вариант 0 (одна карта теней); остальные — при первом кадре, которому нужны.
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh shader"),
            source: wgpu::ShaderSource::Wgsl(variant::mesh_source(0).into()),
        });
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky and shadow shader"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("common.wgsl"), include_str!("sky.wgsl")).into(),
            ),
        });

        let uniform_entry = |binding, dynamic| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: None,
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[uniform_entry(0, false)],
        });
        let lit_layouts: Vec<wgpu::BindGroupLayout> = (0..variant::COUNT).map(|v| variant::lit_layout(&device, v)).collect();
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        // Карта теней солнца: массив слоёв (каскады), пока каскадов нет — один слой.
        let (shadow_view, shadow_layers) = create_shadow_maps(&device, 1);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..wgpu::SamplerDescriptor::default()
        });
        let shadow_views = variant::ShadowViews { all: &shadow_view, first: &shadow_layers[0], sampler: &shadow_sampler, lights: None };
        let lit_binds: Vec<Option<wgpu::BindGroup>> =
            (0..variant::COUNT).map(|v| variant::lit_bind_group(&device, &lit_layouts[v], v, &globals_buffer, &shadow_views)).collect();

        let depth_state = |write: bool, compare: wgpu::CompareFunction| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: write,
            depth_compare: compare,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        // Вся сцена рисуется в HDR; в формат цели её переводит пост-обработка.
        let color_target = |blend| {
            [Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        // Экземпляры мешей: второй буфер, шаг — экземпляр.
        let instance_layout = |attributes| wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        };
        let mesh_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attributes,
        };

        // --- спрайты: глубина проверяется (меш может закрыть спрайт), но не пишется ---
        let sprite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite pipeline layout"),
            bind_group_layouts: &[&globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let make_sprite_pipeline = |label: &'static str, blend: wgpu::BlendState| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&sprite_layout),
            vertex: wgpu::VertexState {
                module: &sprite_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SpriteVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader,
                entry_point: Some("fs_main"),
                targets: &color_target(Some(blend)),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, wgpu::CompareFunction::GreaterEqual)),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..wgpu::MultisampleState::default()
            },
            multiview: None,
            cache: None,
        });
        let sprite_pipeline = make_sprite_pipeline("sprite pipeline", wgpu::BlendState::ALPHA_BLENDING);
        // Сложение: цвет × альфа добавляется к кадру; в HDR свет накапливается и даёт блум.
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let sprite_additive_pipeline = make_sprite_pipeline("additive sprite pipeline", additive);

        // --- меши: четыре конвейера и тени с вырезом, см. renderer/material.rs ---
        let mesh_pass = material::MeshPass::new(material::PipelineInputs {
            device: &device,
            mesh_shader: &mesh_shader,
            sky_shader: &sky_shader,
            lit_layout: &lit_layouts[0],
            globals_layout: &globals_layout,
            vertex: mesh_vertex_layout.clone(),
            instance_model: instance_layout(&INSTANCE_MODEL_ATTRIBUTES),
            instance_cutout: instance_layout(&material::INSTANCE_CUTOUT_ATTRIBUTES),
            depth_format: DEPTH_FORMAT,
        });

        // --- тени: только глубина, со смещением против «акне» ---
        let unlit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow and sky pipeline layout"),
            bind_group_layouts: &[&globals_layout],
            push_constant_ranges: &[],
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow pipeline"),
            layout: Some(&unlit_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs_shadow"),
                buffers: &[mesh_vertex_layout, instance_layout(&INSTANCE_MODEL_ATTRIBUTES)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 1.5,
                    clamp: 0.0,
                },
                ..depth_state(true, wgpu::CompareFunction::Less)
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- небо: треугольник на весь экран позади всего ---
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky pipeline"),
            layout: Some(&unlit_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs_sky"),
                targets: &color_target(None),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, wgpu::CompareFunction::Always)),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..wgpu::MultisampleState::default()
            },
            multiview: None,
            cache: None,
        });

        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("texture sampler"),
                mag_filter: filter,
                min_filter: filter,
                ..wgpu::SamplerDescriptor::default()
            })
        };
        let sampler_linear = sampler(wgpu::FilterMode::Linear);
        let sampler_nearest = sampler(wgpu::FilterMode::Nearest);
        // Для мешей: повторение, трилинейная фильтрация по мипам, анизотропия 16x.
        let sampler_tiled = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tiled texture sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..wgpu::SamplerDescriptor::default()
        });
        let sampler_tiled_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tiled pixel-art sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..wgpu::SamplerDescriptor::default()
        });
        let sampler_tiled_pixel = device.create_sampler(&pixel_sampler_descriptor());
        let sampler_atlas = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model atlas sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });

        let solid = |pixels: &[u8], side: u32| {
            upload_texture(
                &device,
                &queue,
                &texture_layout,
                &sampler_nearest,
                &image::RgbaImage::from_raw(side, side, pixels.to_vec()).expect("size matches"),
                false,
            )
        };
        let white = solid(&[255; 4], 1);
        let magenta = [255, 0, 255, 255];
        let black = [0, 0, 0, 255];
        let missing = solid(&[magenta, black, black, magenta].concat(), 2);
        // Формы частиц: белый круг, прозрачность задаёт альфа.
        let disc = |edge: &dyn Fn(f32) -> f32| {
            const SIDE: u32 = 64;
            let image = image::RgbaImage::from_fn(SIDE, SIDE, |x, y| {
                let dx = (x as f32 + 0.5) / SIDE as f32 * 2.0 - 1.0;
                let dy = (y as f32 + 0.5) / SIDE as f32 * 2.0 - 1.0;
                let alpha = edge((dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                image::Rgba([255, 255, 255, (alpha * 255.0).round() as u8])
            });
            upload_texture(&device, &queue, &texture_layout, &sampler_linear, &image, false)
        };
        let soft_disc = disc(&|d| (1.0 - d * d).max(0.0).powi(2));
        let hard_disc = disc(&|d| (1.0 - d) * 16.0);
        let post = PostChain::new(&device, &queue, format);
        // Эффекты по глубине читают многовыборочную глубину — в WebGL2 так нельзя (там лимиты
        // `downlevel_webgl2_defaults`, без storage-буферов): эффекты молча выключены.
        let fx_supported = device.limits().max_storage_buffers_per_shader_stage > 0;
        let overlay = overlay_pass::OverlayPass::new(&device, format, &texture_layout);
        let grid = grid_pass::GridPass::new(&device, &globals_layout);
        let terrain = terrain_pass::TerrainPass::new(&device, &lit_layouts[0], &globals_layout);
        let water = water_pass::WaterPass::new(&device);

        let sprite_capacity = QUAD_VERTICES * 256;
        let sprite_vertices = create_sprite_buffer(&device, sprite_capacity);
        let quad_indices = create_quad_indices(&device, sprite_capacity / QUAD_VERTICES);

        let instance_capacity = 256;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);

        Self {
            device,
            queue,
            format,
            sprite_pipeline,
            sprite_additive_pipeline,
            mesh_pass,
            shadow_pipeline,
            sky_pipeline,
            globals_buffer,
            globals_bind_group,
            lit_binds,
            frame_variant: 0,
            shadow_view,
            shadow_layers,
            shadow_sampler,
            lit_layouts,
            globals_layout,
            cascade_globals: Vec::new(),
            face_globals: Vec::new(),
            lights_gpu: None,
            local_lights: Vec::new(),
            light_frame: LightFrame::default(),
            instance_buffer,
            instance_capacity,
            mesh_scratch: MeshScratch::default(),
            next_mesh_id: 0,
            batcher: Batcher::default(),
            walk_stack: Vec::new(),
            texture_layout,
            sampler_linear,
            sampler_nearest,
            sampler_tiled,
            sampler_tiled_nearest,
            sampler_tiled_pixel,
            sampler_atlas,
            warned_standard_depth: false,
            post,
            white,
            missing,
            soft_disc,
            hard_disc,
            textures: HashMap::new(),
            failed: HashSet::new(),
            meshes: HashMap::new(),
            failed_meshes: HashSet::new(),
            sprite_vertices,
            sprite_capacity,
            quad_indices,
            sprite_scratch: SpriteScratch::default(),
            depth: None,
            warned_no_camera: false,
            overlay,
            grid,
            terrain,
            water,
            state_changes: std::sync::atomic::AtomicU32::new(0),
            post_fx: None,
            fx_supported,
            depth_generation: 0,
            custom: None,
        }
    }

    /// Карта теней не меньше чем из `layers` слоёв (каскады); пересоздаётся один раз.
    fn ensure_shadow_layers(&mut self, layers: usize) {
        if self.shadow_layers.len() >= layers {
            return;
        }
        let (view, each) = create_shadow_maps(&self.device, MAX_CASCADES as u32);
        self.shadow_view = view;
        self.shadow_layers = each;
        self.rebuild_lit_binds();
    }

    /// Группы привязок освещённых шейдеров всех вариантов (после смены карты теней или
    /// текстур ламп).
    fn rebuild_lit_binds(&mut self) {
        let views = variant::ShadowViews {
            all: &self.shadow_view,
            first: &self.shadow_layers[0],
            sampler: &self.shadow_sampler,
            lights: self.lights_gpu.as_ref().map(|l| l.views()),
        };
        self.lit_binds = (0..variant::COUNT)
            .map(|v| variant::lit_bind_group(&self.device, &self.lit_layouts[v], v, &self.globals_buffer, &views))
            .collect();
    }

    /// Собрать конвейеры варианта шейдера мешей и рельефа, если их ещё нет
    /// (renderer/variant.rs).
    fn ensure_variant(&mut self, variant: usize) {
        if self.mesh_pass.pipelines[variant].is_none() {
            let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mesh shader variant"),
                source: wgpu::ShaderSource::Wgsl(variant::mesh_source(variant).into()),
            });
            self.mesh_pass.pipelines[variant] =
                Some(material::main_pipelines(&self.device, &module, &self.lit_layouts[variant], &self.mesh_pass.layout));
        }
        self.terrain.ensure_variant(&self.device, variant, &self.lit_layouts[variant]);
        self.frame_variant = variant;
    }

    /// Учесть `n` смен состояния в статистике кадра.
    fn count_state(&self, n: u32) {
        self.state_changes.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Формат цели, под которую собран конвейер.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Забыть загруженные с диска текстуры и меши: при следующем кадре они будут
    /// прочитаны заново. Редактор вызывает это, когда файлы ассетов изменились.
    pub fn reload_assets(&mut self) {
        self.textures.clear();
        self.failed.clear();
        self.meshes.retain(|key, _| matches!(key, MeshKey::Shape(_)));
        self.failed_meshes.clear();
        self.mesh_pass.clear();
        self.forget_custom_binds();
    }

    /// Вид из камеры сцены: активная камера с наибольшим приоритетом.
    pub fn scene_view(&mut self, world: &World, aspect: f32) -> CameraView {
        // Активная камера с наибольшим приоритетом (при равенстве — последняя в иерархии,
        // как у `max_by_key`) и есть ли в сцене активный `Environment` — по индексу сцены,
        // без обхода всех объектов.
        let scene = &world.scene;
        let mut best_id: Option<(GameObjectId, _)> = None;
        scene.visit_active_with::<Camera>(|id, camera| {
            if best_id.is_none_or(|(_, priority)| camera.priority >= priority) {
                best_id = Some((id, camera.priority));
            }
        });
        let mut has_environment = false;
        if scene.count_with::<Environment>() > 0 {
            scene.visit_active_with::<Environment>(|_, _| has_environment = true);
        }
        let best = best_id.and_then(|(id, _)| Some((scene.get_component::<Camera>(id)?, scene.world_matrix(id))));
        match best {
            Some((camera, matrix)) => {
                CameraView {
                    view_proj: camera.view_projection(matrix, aspect),
                    position: matrix.w_axis.truncate(),
                    clear_color: camera.clear_color,
                    sky: camera.background == Background::Sky,
                    // 2D-камера без `Environment` выводит цвета как есть.
                    post: (camera.projection == Projection::Orthographic && !has_environment)
                    .then_some(PostSettings::OFF),
                    culling_mask: camera.culling_mask,
                    overlay: true,
                    grid: None,
                    fog: true,
                }
            }
            None => {
                if !self.warned_no_camera {
                    self.warned_no_camera = true;
                    log::warn!(
                        "no active Camera in the scene; using a default orthographic one. \
                         Add it: `app.spawn(\"Camera\").with(Camera::default())`"
                    );
                }
                let camera = Camera::default();
                CameraView {
                    view_proj: camera.view_projection(Mat4::IDENTITY, aspect),
                    position: Vec3::ZERO,
                    clear_color: camera.clear_color,
                    sky: false,
                    post: Some(PostSettings::OFF),
                    culling_mask: camera.culling_mask,
                    overlay: true,
                    grid: None,
                    fog: true,
                }
            }
        }
    }

    /// Загрузить текстуру по GUID, если её ещё нет на GPU.
    fn ensure_texture(&mut self, guid: Guid, assets: Option<&AssetDatabase>) {
        if self.textures.contains_key(&guid) || self.failed.contains(&guid) {
            return;
        }
        let loaded = (|| {
            let assets = assets.ok_or_else(|| {
                "no AssetDatabase in world.resources; open one with \
                 `AssetDatabase::open(\"assets\")` and insert it"
                    .to_string()
            })?;
            let name = assets.relative_path_of(guid).unwrap_or_default().to_string();
            // У модели Piska Modeler (`.pmodel`) картинка — её атлас.
            let bytes = assets.read_image(guid).map_err(|e| e.to_string())?;
            let format = image::ImageFormat::from_path(&name).ok();
            let decoded = match format {
                Some(format) => image::load_from_memory_with_format(&bytes, format),
                None => image::load_from_memory(&bytes),
            };
            let image = decoded
                .map_err(|e| format!("cannot decode `{name}`: {e}"))?
                .to_rgba8();
            let filter = TextureFilter::from_setting(assets.meta(guid).and_then(|meta| meta.settings.get("filter")));
            Ok::<_, String>((image, filter, engine_assets::has_embedded_atlas(&name)))
        })();

        match loaded {
            Ok((image, filter, atlas)) => {
                let nearest = filter == TextureFilter::Nearest;
                let pixel = filter == TextureFilter::Pixel;
                // Пиксель-арт остаётся резким: без сглаживания и без мипмапов. Атлас
                // модели (`.pmodel`) — тоже без мипмапов: грани в нём лежат вплотную, и
                // уменьшенные копии смешали бы их цвета на швах (см. `inset_atlas_uvs`).
                // Сэмплер с повторением для мешей выбирает материал (по `nearest`).
                // `Pixel` — резкие пиксели вблизи, но с мипмапами: вдали не рябит.
                let sampler = if nearest || pixel { &self.sampler_nearest } else { &self.sampler_linear };
                let mut texture = upload_texture(
                    &self.device,
                    &self.queue,
                    &self.texture_layout,
                    sampler,
                    &image,
                    (!nearest && !atlas) || pixel,
                );
                texture.nearest = nearest;
                texture.pixel = pixel;
                texture.atlas = atlas && !nearest && !pixel;
                self.textures.insert(guid, texture);
            }
            Err(reason) => {
                // Один раз в лог, дальше — заметная «шахматка» вместо падения.
                log::error!("texture {guid}: {reason}");
                self.failed.insert(guid);
            }
        }
    }

    /// Загрузить меш на GPU, если его там ещё нет. `None` — меш загрузить не удалось
    /// (причина один раз уходит в лог), рисовать нечего.
    fn ensure_mesh(&mut self, mesh: &MeshRenderer, assets: Option<&AssetDatabase>) -> Option<MeshKey> {
        let key = if mesh.mesh.is_none() {
            shape_key(mesh.shape)
        } else {
            MeshKey::Asset(mesh.mesh.guid())
        };
        if self.meshes.contains_key(&key) {
            return Some(key);
        }
        let data = match key {
            MeshKey::Shape(_) => mesh.shape.mesh(),
            MeshKey::Asset(guid) => {
                if self.failed_meshes.contains(&guid) {
                    return None;
                }
                let loaded = assets
                    .ok_or_else(|| "no AssetDatabase in world.resources".to_string())
                    .and_then(|assets| {
                        let mut data = assets.load_mesh(guid).map_err(|e| e.to_string())?;
                        inset_atlas_uvs(&mut data, guid, assets);
                        Ok(data)
                    });
                match loaded {
                    Ok(data) => data,
                    Err(reason) => {
                        log::error!("mesh {guid}: {reason}");
                        self.failed_meshes.insert(guid);
                        return None;
                    }
                }
            }
        };
        let mut gpu = upload_mesh(&self.device, &data);
        gpu.id = self.next_mesh_id;
        self.next_mesh_id += 1;
        self.meshes.insert(key, gpu);
        Some(key)
    }

    /// Загрузить меш ассета на GPU заранее (если его там ещё нет) и вернуть его границы.
    /// В отличие от кадра, ошибку не пишет в лог и не запоминает: вызывающий (предпросмотр
    /// в редакторе) сам решает, что сказать пользователю. Загруженный меш общий с кадром.
    pub fn preload_mesh(&mut self, guid: Guid, assets: &AssetDatabase) -> Result<(Vec3, Vec3), String> {
        let key = MeshKey::Asset(guid);
        if let Some(mesh) = self.meshes.get(&key) {
            return Ok(mesh.bounds);
        }
        let mut data = assets.load_mesh(guid).map_err(|e| e.to_string())?;
        inset_atlas_uvs(&mut data, guid, assets);
        let mut gpu = upload_mesh(&self.device, &data);
        gpu.id = self.next_mesh_id;
        self.next_mesh_id += 1;
        let bounds = gpu.bounds;
        self.meshes.insert(key, gpu);
        Ok(bounds)
    }

    /// Поправка UV для текстуры спрайта (загружает её, если нужно); без текстуры — нулевая.
    fn texture_inset(&mut self, guid: Option<Guid>, assets: Option<&AssetDatabase>) -> Vec2 {
        let Some(guid) = guid else {
            return Vec2::ZERO;
        };
        self.ensure_texture(guid, assets);
        self.textures.get(&guid).map_or(Vec2::ZERO, |t| t.inset)
    }

    /// Многовыборочные цвет и глубина под размер цели (пересоздаются при смене размера).
    fn ensure_targets(&mut self, width: u32, height: u32) {
        let size = [width.max(1), height.max(1)];
        if self.depth.as_ref().is_some_and(|(_, _, existing)| *existing == size) {
            return;
        }
        let texture = |label, format, extra: wgpu::TextureUsages| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | extra,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let readable = if self.fx_supported { wgpu::TextureUsages::TEXTURE_BINDING } else { wgpu::TextureUsages::empty() };
        let color = texture("msaa color", HDR_FORMAT, wgpu::TextureUsages::empty());
        let depth = texture("msaa depth", DEPTH_FORMAT, readable);
        self.depth = Some((color, depth, size));
        self.depth_generation = self.depth_generation.wrapping_add(1);
    }

    /// Нарисовать мир в `target` глазами камеры сцены. Мир только читается.
    pub fn render(
        &mut self,
        world: &World,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> RenderStats {
        let aspect = width.max(1) as f32 / height.max(1) as f32;
        let view = self.scene_view(world, aspect);
        self.render_with(world, target, width, height, &view)
    }

    /// Нарисовать мир в `target` (размером `width` × `height`) с заданным видом —
    /// так рисует вьюпорт редактора.
    pub fn render_with(
        &mut self,
        world: &World,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        view: &CameraView,
    ) -> RenderStats {
        // Сцена рисуется с обратной глубиной (depth.rs). Свою матрицу с обычной глубиной
        // (`Mat4::perspective_rh` …) переводим здесь: иначе тест «больше» оставил бы в
        // кадре самое дальнее.
        let converted;
        let view = if crate::depth::uses_reversed_depth(view.view_proj, view.position) {
            view
        } else {
            if !self.warned_standard_depth {
                self.warned_standard_depth = true;
                log::warn!(
                    "CameraView::view_proj uses standard 0..1 depth; converted to reversed-Z. \
                     Build projections with engine_render::perspective / orthographic for full depth precision"
                );
            }
            converted = CameraView { view_proj: crate::depth::to_reversed_depth(view.view_proj), ..*view };
            &converted
        };
        // Свои шейдеры: файлы, изменённые на диске, — перечитать (горячая перезагрузка).
        self.poll_custom_shaders();
        let extract_clock = Stopwatch::start();
        let assets = world.resources.get::<AssetDatabase>();
        let scene = &world.scene;
        let mask = view.culling_mask;
        let frustum = Frustum::from_view_proj(view.view_proj);
        let mut stats = RenderStats::default();

        // ---------------- 1. Извлечение: один проход по активным объектам ----------------
        let mut scratch = std::mem::take(&mut self.sprite_scratch);
        scratch.runs.clear();
        scratch.raw.clear();
        scratch.bounds.clear();
        let mut frame = std::mem::take(&mut self.mesh_scratch);
        frame.items.clear();
        let camera_basis = CameraBasis::from_view_proj(view.view_proj);
        let mut sun: Option<(Vec3, Vec3, bool)> = None; // направление, цвет × яркость, тени
        let mut points: Vec<(f32, [f32; 8])> = Vec::new();
        let mut environment: Option<Environment> = None;
        let mut insets = RecentCache::<Guid, Vec2>::default();
        let mut mesh_cache = RecentCache::<MeshKey, (u32, (Vec3, Vec3))>::default();
        let runtime_materials = world.resources.get::<Materials>();
        let mut frame_materials = FrameMaterials::default();

        // Лампы (lights.rs): нужен ли вариант «с лампами» и сколько ламп брать.
        let light_setup = {
            let g = world.resources.get::<GraphicsSettings>();
            LightSetup::of(scene, g.map_or(255, |g| g.max_lights), g.map_or(2, |g| g.light_shadows))
        };
        let mut local_lights = std::mem::take(&mut self.local_lights);
        local_lights.clear();
        // Свои шейдеры спрайтов: без компонента `SpriteShader` в сцене спрайты их не ищут.
        let sprite_shaders = scene.count_with::<crate::SpriteShader>() > 0;
        // Значения своего шейдера у отдельных объектов (`MaterialParams`) — так же.
        let material_params = scene.count_with::<crate::MaterialParams>() > 0;
        let mut walk = SceneWalk::new(scene, std::mem::take(&mut self.walk_stack));
        while let Some((_, object)) = walk.next() {
            let parts = RenderParts::of(object);
            if parts.is_empty() {
                continue;
            }
            if let (Some(env), None) = (parts.environment, &environment) {
                environment = Some(env.clone());
            }

            if let Some(light) = parts.light {
                let matrix = walk.world();
                let color = Vec3::new(light.color.r, light.color.g, light.color.b) * light.intensity.max(0.0);
                match light.kind {
                    // Солнце — первый направленный источник в порядке иерархии.
                    LightKind::Directional if sun.is_none() => {
                        let (_, rotation, _) = matrix.to_scale_rotation_translation();
                        sun = Some((rotation * Vec3::NEG_Z, color, light.shadows));
                    }
                    LightKind::Directional => {}
                    LightKind::Point => {
                        let position = matrix.w_axis.truncate();
                        if light_setup.extended {
                            let spot = object.get::<SpotLight>().map(LocalLight::spot_cone);
                            let shadows = object.get::<LightShadows>().filter(|s| s.enabled).map(|s| finite_or(s.bias, 0.03).clamp(0.0, 1.0));
                            local_lights.push(LocalLight {
                                position,
                                range: light.range.max(0.01),
                                color,
                                direction: matrix.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z),
                                spot,
                                shadows,
                                distance2: position.distance_squared(view.position),
                            });
                        }
                        points.push((
                            position.distance_squared(view.position),
                            [
                                position.x,
                                position.y,
                                position.z,
                                light.range.max(0.01),
                                color.x,
                                color.y,
                                color.z,
                                0.0,
                            ],
                        ));
                    }
                }
            }
            if mask & (1u32 << object.layer.min(31)) == 0 {
                continue;
            }

            if let Some(sprite) = parts.sprite {
                let matrix = walk.world();
                let corners = sprite_corners(sprite, matrix);
                let min = corners[0].min(corners[1]).min(corners[2].min(corners[3]));
                let max = corners[0].max(corners[1]).max(corners[2].max(corners[3]));
                // Спрайт целиком вне кадра не даёт ни одного пикселя — не рисуем.
                if frustum.intersects_aabb(min, max) {
                    let guid = (!sprite.texture.is_none()).then(|| sprite.texture.guid());
                    let inset = self.cached_inset(&mut insets, guid, assets);
                    let start = scratch.raw.len() as u32;
                    push_quad(&mut scratch.raw, corners, sprite_uv(sprite, inset), sprite.color.to_array());
                    let blend = if sprite_shaders {
                        object.get::<crate::SpriteShader>().and_then(|s| self.sprite_shader_bind(s, assets)).map_or(Blend::Alpha, Blend::Custom)
                    } else {
                        Blend::Alpha
                    };
                    scratch.runs.push(DrawRun {
                        sort_key: (sprite.sorting_layer, sprite.order),
                        depth: corners[0].z,
                        texture: guid.map_or(TexRef::White, TexRef::Asset),
                        blend,
                        start,
                        len: QUAD_VERTICES as u32,
                    });
                    scratch.bounds.push((min, max));
                } else {
                    stats.culled_sprites += 1;
                }
            }

            if let Some(particles) = parts.particles {
                if particles.alive_count() > 0 {
                    let matrix = walk.world();
                    let guid = (!particles.texture.is_none()).then(|| particles.texture.guid());
                    let inset = self.cached_inset(&mut insets, guid, assets);
                    let start = scratch.raw.len() as u32;
                    particles.write_vertices(matrix, camera_basis, inset, &mut scratch.raw);
                    scratch.push_run(
                        DrawRun {
                            sort_key: (particles.sorting_layer, particles.order),
                            depth: matrix.w_axis.z,
                            texture: match guid {
                                Some(guid) => TexRef::Asset(guid),
                                None => match particles.particle_shape {
                                    ParticleShape::Soft => TexRef::Soft,
                                    ParticleShape::Circle => TexRef::Disc,
                                    ParticleShape::Square => TexRef::White,
                                },
                            },
                            blend: match particles.blend {
                                ParticleBlend::Alpha => Blend::Alpha,
                                ParticleBlend::Additive => Blend::Additive,
                            },
                            start,
                            len: 0,
                        },
                    );
                }
            }

            if let Some(map) = parts.tilemap {
                let matrix = walk.world();
                let guid = (!map.tileset.is_none()).then(|| map.tileset.guid());
                let inset = self.cached_inset(&mut insets, guid, assets);
                let start = scratch.raw.len() as u32;
                stats.tiles += map.write_vertices(matrix, view.view_proj, inset, &mut scratch.raw) as u32;
                scratch.push_run(
                    DrawRun {
                        sort_key: (map.sorting_layer, map.order),
                        depth: matrix.w_axis.z,
                        texture: guid.map_or(TexRef::White, TexRef::Asset),
                        blend: Blend::Alpha,
                        start,
                        len: 0,
                    },
                );
            }

            if let Some(trail) = parts.trail {
                let start = scratch.raw.len() as u32;
                trail.write_vertices(camera_basis, &mut scratch.raw);
                if scratch.raw.len() as u32 > start {
                    let depth = walk.world().w_axis.z;
                    scratch.push_run(
                        DrawRun {
                            sort_key: (trail.sorting_layer, trail.order),
                            depth,
                            texture: TexRef::White,
                            blend: match trail.blend {
                                ParticleBlend::Alpha => Blend::Alpha,
                                ParticleBlend::Additive => Blend::Additive,
                            },
                            start,
                            len: 0,
                        },
                    );
                }
            }

            if let Some(mesh) = parts.mesh {
                let key = if mesh.mesh.is_none() {
                    shape_key(mesh.shape)
                } else {
                    MeshKey::Asset(mesh.mesh.guid())
                };
                let found = match mesh_cache.get(&key) {
                    Some(found) => Some(found),
                    None => self.ensure_mesh(mesh, assets).and_then(|key| {
                        let gpu = self.meshes.get(&key)?;
                        let found = (gpu.id, gpu.bounds);
                        mesh_cache.put(key, found);
                        Some(found)
                    }),
                };
                let Some((mesh_id, local_bounds)) = found else {
                    continue;
                };
                let matrix = walk.world();
                let world_bounds = aabb_transform(matrix, local_bounds);
                let visible = frustum.intersects_aabb(world_bounds.0, world_bounds.1);
                if !visible {
                    stats.culled_meshes += 1;
                    if !mesh.cast_shadows {
                        continue;
                    }
                }
                if mesh.uses_material_assets() {
                    let params = if material_params { Self::object_params(object) } else { None };
                    let object = MaterialObject { mesh, key, mesh_id, matrix, visible, world_bounds, params };
                    self.push_material_parts(&mut frame.items, &mut frame_materials, object, assets, runtime_materials);
                    continue;
                }
                // Встроенный материал: как до появления ассетов, весь меш одним вызовом.
                let inline = self.inline_material(&mut frame_materials, mesh, assets);
                let tint = mesh.tint;
                frame.items.push(MeshItem {
                    key,
                    mesh_id,
                    part: WHOLE_MESH,
                    bind: inline.bind,
                    pass: inline.pass,
                    shadow: inline.shadow,
                    matrix,
                    base_color: [
                        mesh.color.r * tint.r,
                        mesh.color.g * tint.g,
                        mesh.color.b * tint.b,
                        mesh.color.a * tint.a,
                    ],
                    emissive: inline.emissive,
                    params: inline.params,
                    casts_shadows: mesh.cast_shadows,
                    visible,
                    world_bounds,
                });
            }
        }
        stats.world_matrices = walk.computed;
        stats.walk_ms = extract_clock.ms();
        self.walk_stack = walk.into_stack();
        let mut environment = environment.unwrap_or_default();
        // Качество графики (Project Settings → Graphics, `ctx.set_quality`): ресурс читается
        // каждый кадр. Нет ресурса — всё как задано в `Environment`.
        let graphics = world.resources.get::<GraphicsSettings>();
        let quality_shadows = graphics.is_none_or(|g| g.shadows);
        let bloom_threshold = graphics.map_or(1.0, |g| finite_or(g.bloom_threshold, 1.0).max(0.0));
        if let Some(g) = graphics {
            environment.shadow_distance = environment.shadow_distance.min(finite_or(g.shadow_distance, f32::MAX).max(1.0));
            environment.fog_density *= finite_or(g.fog_scale, 1.0).max(0.0);
            environment.bloom = if g.bloom { environment.bloom * finite_or(g.bloom_intensity, 1.0).max(0.0) } else { 0.0 };
        }
        if !view.fog {
            environment.fog_density = 0.0;
        }
        // Форма тумана (компонент `Fog`, fog.rs): начало, спад с высотой; выключатель.
        let (fog_on, fog_shape) = crate::fog::Fog::shader_params(&world.scene);
        if !fog_on {
            environment.fog_density = 0.0;
        }
        // Рельефы: подгрузка чанков вокруг камеры и отбор видимых (terrain_pass.rs).
        self.terrain.prepare(&self.device, &self.queue, world, view.position, &frustum, mask, assets);

        // ---------------- 2. Свет, небо и тени ----------------
        // Меши без единого источника света не должны быть чёрными: включаем солнце
        // по умолчанию — как камеру по умолчанию, когда её нет в сцене.
        let (sun_direction, sun_color, sun_shadows) = match sun {
            Some((direction, color, shadows)) => (direction, color, shadows),
            None if points.is_empty() => (
                Vec3::new(-0.45, -1.0, -0.35).normalize(),
                Vec3::new(1.0, 0.95, 0.88) * 2.4,
                true,
            ),
            None => (Vec3::NEG_Y, Vec3::ZERO, false),
        };
        let has_sun = sun_color.max_element() > 0.0;

        // Цвета неба зависят от высоты солнца: день, закат, ночь.
        let elevation = if has_sun { -sun_direction.y } else { -1.0 };
        let day = smoothstep(-0.12, 0.25, elevation);
        let sunset = (1.0 - elevation.abs() * 3.5).clamp(0.0, 1.0) * smoothstep(-0.25, 0.0, elevation);
        let sky_zenith = Vec3::new(0.004, 0.006, 0.02).lerp(Vec3::new(0.12, 0.30, 0.75), day);
        let sky_horizon = Vec3::new(0.015, 0.02, 0.04)
            .lerp(Vec3::new(0.55, 0.70, 0.90), day)
            .lerp(Vec3::new(0.95, 0.50, 0.25), sunset * 0.7);
        // Свои цвета неба сцены (компонент `SkyColors`, sky_colors.rs).
        let (sky_zenith, sky_horizon) = crate::sky_colors::SkyColors::apply(scene, sky_zenith, sky_horizon);
        // Небо `Sky` (atmosphere.rs): солнце, рассеяние, облака, луна; без компонента — нули.
        let sky = crate::atmosphere::Sky::shader_params(scene, world.time.elapsed, true);

        let caster_bounds = frame
            .items
            .iter()
            .filter(|m| m.casts_shadows)
            .map(|m| m.world_bounds)
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
        let caster_bounds = match (caster_bounds, self.terrain.shadow_bounds(view.position, environment.shadow_distance)) {
            (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
            (a, b) => a.or(b),
        };
        let shadows = quality_shadows && sun_shadows && has_sun && caster_bounds.is_some();
        let mut light_view_proj = Mat4::IDENTITY;
        let mut shadow_params = [0.0; 4];
        if shadows {
            let (min, max) = caster_bounds.unwrap_or((Vec3::ZERO, Vec3::ZERO));
            // Объём солнца накрывает все отбрасывающие тень объекты; в больших сценах —
            // область вокруг камеры, чтобы тени рядом оставались чёткими.
            let mut center = (min + max) * 0.5;
            let mut radius = ((max - min).length() * 0.5).max(1.0);
            let limit = environment.shadow_distance.max(1.0);
            if radius > limit {
                radius = limit;
                center = view.position + (center - view.position).clamp_length_max(limit * 0.25);
            }
            let up = if sun_direction.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
            let rotation = Mat4::look_to_rh(Vec3::ZERO, sun_direction, up);
            // Привязка к текселям карты теней: при движении камеры края теней не «плывут».
            let texel = radius * 2.0 / SHADOW_SIZE as f32;
            let mut in_light = rotation.transform_point3(center);
            in_light.x = (in_light.x / texel).round() * texel;
            in_light.y = (in_light.y / texel).round() * texel;
            // Запас по глубине: всё, что стоит между солнцем и областью, тоже даёт тень.
            let depth = radius.max((max - min).length() * 0.5) + 50.0;
            let projection = Mat4::orthographic_rh(
                in_light.x - radius,
                in_light.x + radius,
                in_light.y - radius,
                in_light.y + radius,
                -in_light.z - depth,
                -in_light.z + depth,
            );
            light_view_proj = projection * rotation;
            shadow_params = [1.0, 1.0 / SHADOW_SIZE as f32, texel * 1.6 + 0.01, 0.0006];
        }

        // Каскады (`SoftShadows`): только для перспективной камеры; иначе — одна карта.
        let (max_cascades, max_filter) = quality_shadow_caps(graphics);
        let cascades: Option<(CascadeSetup, Vec<Cascade>)> = if shadows {
            SoftShadows::setup(scene, max_cascades, max_filter).and_then(|setup| {
                let casters = caster_bounds.unwrap_or((Vec3::ZERO, Vec3::ZERO));
                fit_cascades(view.view_proj, view.position, sun_direction, &setup, environment.shadow_distance, casters, SHADOW_SIZE)
                    .map(|list| (setup, list))
            })
        } else {
            None
        };
        if let Some((_, list)) = &cascades {
            self.ensure_shadow_layers(list.len());
        }
        // Лампы: раскладка по кластерам, тени ламп, текстуры на GPU.
        if light_setup.extended {
            self.light_frame.build(&mut local_lights, view.view_proj, &light_setup);
            let mut rebuild = false;
            if self.lights_gpu.is_none() {
                self.lights_gpu = Some(lights_pass::LightsGpu::new(&self.device));
                rebuild = true;
            }
            if !self.light_frame.shadow_faces.is_empty() {
                rebuild |= self.lights_gpu.as_mut().is_some_and(|l| l.ensure_atlas(&self.device));
            }
            if rebuild {
                self.rebuild_lit_binds();
            }
        } else {
            self.light_frame.shadow_faces.clear();
            self.light_frame.light_count = 0;
        }
        self.local_lights = local_lights;
        // Вариант шейдера: без каскадов — одна карта (дешевле, как до `SoftShadows`);
        // лампы — только если сцена их просит.
        let cascade_bit = if cascades.is_some() { variant::CASCADES } else { 0 };
        let lights_bit = if light_setup.extended { variant::LIGHTS } else { 0 };
        self.ensure_variant(cascade_bit | lights_bit);

        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut point_data = [[0.0; 8]; MAX_POINT_LIGHTS];
        // С лампами (lights.rs) все точечные идут через кластеры, четыре uniform-лампы не нужны.
        let point_count = if light_setup.extended { 0 } else { points.len().min(MAX_POINT_LIGHTS) };
        for (slot, (_, data)) in point_data.iter_mut().zip(&points) {
            *slot = *data;
        }

        // ---------------- 3. Меши: сортировка по конвейеру, мешу и материалу, инстансы ----------------
        // Видимые части одного меша с одной группой материала рисуются одним draw call'ом;
        // полупрозрачные — отдельно, от дальних к ближним.
        frame.order.clear();
        frame.blend.clear();
        for (index, item) in frame.items.iter().enumerate() {
            if item.visible {
                if item.pass >= PASS_BLEND {
                    frame.blend.push(index as u32);
                } else {
                    frame.order.push(index as u32);
                }
            }
        }
        {
            let items = &frame.items;
            // Устойчивая сортировка: внутри пачки сохраняется порядок иерархии.
            frame.order.sort_by_key(|&i| {
                let item = &items[i as usize];
                (item.pass, item.mesh_id, item.part, item.bind)
            });
            let eye = view.position;
            let distance = |i: u32| {
                let (min, max) = items[i as usize].world_bounds;
                ((min + max) * 0.5).distance_squared(eye)
            };
            frame.blend.sort_by(|&a, &b| distance(b).total_cmp(&distance(a)));
        }
        frame.instances.clear();
        frame.batches.clear();
        frame.blend_batches.clear();
        build_batches(&frame.items, &frame.order, &mut frame.instances, &mut frame.batches, false);
        build_batches(&frame.items, &frame.blend, &mut frame.instances, &mut frame.blend_batches, false);
        stats.meshes = (frame.order.len() + frame.blend.len()) as u32;
        stats.transparent_meshes = frame.blend.len() as u32;

        // Тени: только те, кто попадает в объём солнца (остальные в карту не пишут).
        frame.shadow_batches.clear();
        for batches in &mut frame.cascade_batches {
            batches.clear();
        }
        if let Some((_, list)) = &cascades {
            while frame.cascade_batches.len() < list.len() {
                frame.cascade_batches.push(Vec::new());
            }
            for (index, cascade) in list.iter().enumerate() {
                let light_frustum = Frustum::from_view_proj(cascade.view_proj);
                frame.order.clear();
                frame.order.extend((0..frame.items.len() as u32).filter(|&i| {
                    let item = &frame.items[i as usize];
                    item.casts_shadows && light_frustum.intersects_aabb(item.world_bounds.0, item.world_bounds.1)
                }));
                {
                    let items = &frame.items;
                    frame.order.sort_by_key(|&i| {
                        let item = &items[i as usize];
                        let bind = if item.shadow == SHADOW_CUTOUT { item.bind } else { 0 };
                        (item.shadow, item.mesh_id, item.part, bind)
                    });
                }
                build_batches(&frame.items, &frame.order, &mut frame.instances, &mut frame.cascade_batches[index], true);
                stats.shadow_casters += frame.order.len() as u32;
            }
        }
        // Грани теней ламп: заслонители в пирамиде грани и в шаре лампы.
        for batches in &mut frame.face_batches {
            batches.clear();
        }
        while frame.face_batches.len() < self.light_frame.shadow_faces.len() {
            frame.face_batches.push(Vec::new());
        }
        for (index, face) in self.light_frame.shadow_faces.iter().enumerate() {
            let face_frustum = Frustum::from_view_proj(face.view_proj);
            let (center, radius) = (face.position, face.range);
            frame.order.clear();
            frame.order.extend((0..frame.items.len() as u32).filter(|&i| {
                let item = &frame.items[i as usize];
                let (min, max) = item.world_bounds;
                item.casts_shadows
                    && center.clamp(min, max).distance_squared(center) <= radius * radius
                    && face_frustum.intersects_aabb(min, max)
            }));
            {
                let items = &frame.items;
                frame.order.sort_by_key(|&i| {
                    let item = &items[i as usize];
                    let bind = if item.shadow == SHADOW_CUTOUT { item.bind } else { 0 };
                    (item.shadow, item.mesh_id, item.part, bind)
                });
            }
            build_batches(&frame.items, &frame.order, &mut frame.instances, &mut frame.face_batches[index], true);
            stats.shadow_casters += frame.order.len() as u32;
        }
        if cascades.is_none() && shadows {
            let light_frustum = Frustum::from_view_proj(light_view_proj);
            frame.order.clear();
            frame.order.extend((0..frame.items.len() as u32).filter(|&i| {
                let item = &frame.items[i as usize];
                item.casts_shadows && light_frustum.intersects_aabb(item.world_bounds.0, item.world_bounds.1)
            }));
            {
                let items = &frame.items;
                frame.order.sort_by_key(|&i| {
                    let item = &items[i as usize];
                    let bind = if item.shadow == SHADOW_CUTOUT { item.bind } else { 0 };
                    (item.shadow, item.mesh_id, item.part, bind)
                });
            }
            build_batches(&frame.items, &frame.order, &mut frame.instances, &mut frame.shadow_batches, true);
            stats.shadow_casters = frame.order.len() as u32;
        }

        // ---------------- 4. Спрайты: сортировка и склейка в батчи ----------------
        // Слой → порядок → глубина; сортировка устойчивая, порядок иерархии сохраняется.
        let runs_sorted = scratch
            .runs
            .windows(2)
            .all(|w| w[0].sort_key.cmp(&w[1].sort_key).then(w[0].depth.total_cmp(&w[1].depth)).is_le());
        if !runs_sorted {
            // Сортируем номера заходов вместе с их границами.
            let mut keyed: Vec<(DrawRun, (Vec3, Vec3))> =
                scratch.runs.iter().copied().zip(scratch.bounds.iter().copied()).collect();
            keyed.sort_by(|a, b| a.0.sort_key.cmp(&b.0.sort_key).then(a.0.depth.total_cmp(&b.0.depth)));
            scratch.runs.clear();
            scratch.bounds.clear();
            for (run, bounds) in keyed {
                scratch.runs.push(run);
                scratch.bounds.push(bounds);
            }
        }
        scratch.rects.clear();
        if needs_rects(&scratch.runs) {
            let view_proj = view.view_proj;
            let raw = &scratch.raw;
            scratch
                .rects
                .extend(scratch.runs.iter().zip(&scratch.bounds).map(|(run, &(min, max))| {
                    let (min, max) = if min.is_nan() {
                        vertex_bounds(&raw[run.start as usize..(run.start + run.len) as usize])
                    } else {
                        (min, max)
                    };
                    screen_rect(view_proj, min, max)
                }));
        }
        let order = self.batcher.build(&scratch.runs, &scratch.rects, &mut scratch.batches);
        let vertex_count: usize = scratch.runs.iter().map(|run| run.len as usize).sum();
        let quad_count = (vertex_count / QUAD_VERTICES) as u32;
        stats.extract_ms = extract_clock.ms();

        // ---------------- 5. Данные на GPU ----------------
        let encode_clock = Stopwatch::start();
        // Вершины спрайтов пишутся в порядке отрисовки прямо в буфер загрузки — без
        // промежуточной отсортированной копии.
        if vertex_count > self.sprite_capacity {
            self.sprite_capacity = vertex_count.next_power_of_two();
            self.sprite_vertices = create_sprite_buffer(&self.device, self.sprite_capacity);
            self.quad_indices = create_quad_indices(&self.device, self.sprite_capacity / QUAD_VERTICES);
        }
        let byte_count = (vertex_count * std::mem::size_of::<SpriteVertex>()) as u64;
        if let Some(size) = wgpu::BufferSize::new(byte_count) {
            if let Some(mut staging) = self.queue.write_buffer_with(&self.sprite_vertices, 0, size) {
                let mut offset = 0;
                for &index in order {
                    let run = &scratch.runs[index as usize];
                    let bytes: &[u8] =
                        bytemuck::cast_slice(&scratch.raw[run.start as usize..(run.start + run.len) as usize]);
                    staging[offset..offset + bytes.len()].copy_from_slice(bytes);
                    offset += bytes.len();
                }
            }
        }
        // Масштаб разрешения сцены (`GraphicsSettings::render_scale`): сцена — в меньшую
        // (или большую) цель, сведение кадра растягивает её до `target`.
        let scale = graphics.map_or(1.0, |g| finite_or(g.render_scale, 1.0).clamp(0.25, 2.0));
        let (scene_width, scene_height) = if (scale - 1.0).abs() < 1e-3 {
            (width, height)
        } else {
            (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1))
        };
        let (fill_sky, fill_ground) = match &light_setup.fill {
            Some(fill) => {
                let (up, down) = crate::lights::fill_colors(fill, sky_zenith, sky_horizon, sun_direction, sun_color);
                (up.extend(1.0).to_array(), down.extend(0.0).to_array())
            }
            None => ([0.0; 4], [0.0; 4]),
        };
        let globals = Globals {
            view_proj: view.view_proj.to_cols_array(),
            inv_view_proj: view.view_proj.inverse().to_cols_array(),
            light_view_proj: light_view_proj.to_cols_array(),
            camera_position: view.position.extend(1.0).to_array(),
            sun_direction: sun_direction.extend(0.0).to_array(),
            sun_color: sun_color.extend(if has_sun { 1.0 } else { 0.0 }).to_array(),
            sky_zenith: sky_zenith.extend(environment.ambient.max(0.0)).to_array(),
            sky_horizon: sky_horizon.extend(environment.fog_density.max(0.0)).to_array(),
            shadow_params,
            counts: [point_count as f32, fog_shape[0], fog_shape[1], fog_shape[2]],
            points: point_data,
            cascade_view_proj: [[0.0; 16]; MAX_CASCADES],
            cascade_data: [[0.0; 4]; MAX_CASCADES],
            cascade_params: [0.0; 4],
            sky_sun: sky.sun,
            sky_clouds: sky.clouds,
            sky_cloud_color: sky.cloud_color,
            sky_night: [
                sky.night[0],
                sky.night[1],
                // Дешёвый рельеф на пресете Low (terrain.wgsl).
                if graphics.is_some_and(|g| !g.terrain_detail) { 1.0 } else { 0.0 },
                sky.night[3],
            ],
            cluster: crate::lights::cluster_params(scene_width, scene_height),
            lights_info: [self.light_frame.light_count as f32, 1.0 / crate::lights::ATLAS_SIZE as f32, 0.0, 0.0],
            fill_sky,
            fill_ground,
        };
        let mut globals = globals;
        if let Some((setup, list)) = &cascades {
            for (i, cascade) in list.iter().enumerate() {
                globals.cascade_view_proj[i] = cascade.view_proj.to_cols_array();
                globals.cascade_data[i] = cascade.data;
            }
            globals.cascade_params = [list.len() as f32, setup.blend, setup.filter as f32, setup.softness];
        }
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        // Проход теней каждого каскада читает свою матрицу из `light_view_proj`.
        if let Some((_, list)) = &cascades {
            while self.cascade_globals.len() < list.len() {
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("cascade globals"),
                    size: std::mem::size_of::<Globals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("cascade globals"),
                    layout: &self.globals_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
                });
                self.cascade_globals.push((buffer, bind));
            }
            for (cascade, (buffer, _)) in list.iter().zip(&self.cascade_globals) {
                let own = Globals { light_view_proj: cascade.view_proj.to_cols_array(), ..globals };
                self.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&own));
                stats.upload_bytes += std::mem::size_of::<Globals>() as u64;
            }
        }
        let post_settings = view.post.unwrap_or(PostSettings {
            enabled: true,
            exposure: environment.exposure,
            bloom: environment.bloom,
            bloom_threshold,
            vignette: environment.vignette,
        });

        if frame.instances.len() > self.instance_capacity {
            self.instance_capacity = frame.instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        if !frame.instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&frame.instances));
        }
        stats.upload_bytes += byte_count
            + std::mem::size_of::<Globals>() as u64
            + std::mem::size_of_val(frame.instances.as_slice()) as u64;
        // Лампы: текстуры кадра и параметры кадра каждой грани тени.
        if light_setup.extended {
            if let Some(gpu) = &self.lights_gpu {
                stats.upload_bytes += gpu.upload(&self.queue, &self.light_frame);
            }
            while self.face_globals.len() < self.light_frame.shadow_faces.len() {
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("light shadow face globals"),
                    size: std::mem::size_of::<Globals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("light shadow face globals"),
                    layout: &self.globals_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
                });
                self.face_globals.push((buffer, bind));
            }
            for (face, (buffer, _)) in self.light_frame.shadow_faces.iter().zip(&self.face_globals) {
                let own = Globals { light_view_proj: face.view_proj.to_cols_array(), ..globals };
                self.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&own));
                stats.upload_bytes += std::mem::size_of::<Globals>() as u64;
            }
        }
        self.state_changes.store(0, std::sync::atomic::Ordering::Relaxed);

        // Свои шейдеры: время кадра, конвейеры частей мешей для варианта кадра.
        self.prepare_custom(world, &frame.items, scene_width, scene_height);

        // ---------------- 6. Проходы ----------------
        self.ensure_targets(scene_width, scene_height);
        self.post.resize(&self.device, scene_width, scene_height);
        // AO и контур (компоненты `AmbientOcclusion`, `Outline`) — только для вида из камеры
        // сцены; пресет Low затенение не делает, Medium — 8 выборок, High — 12.
        let quality = graphics.map(|g| g.quality);
        let ao_samples = match (graphics.is_none_or(|g| g.ambient_occlusion), quality) {
            (false, _) => 0,
            (true, Some(engine_core::settings::QualityLevel::Low | engine_core::settings::QualityLevel::Medium)) => 8,
            _ => 12,
        };
        let scene_post = view.post.is_none();
        let fx = if self.fx_supported && scene_post {
            crate::post_fx::FxSettings::from_scene(scene, ao_samples)
        } else {
            crate::post_fx::FxSettings::default()
        };
        let (fxaa, hq_bloom) = if scene_post {
            let (fxaa, hq) = crate::post_fx::PostQuality::settings(scene, graphics.is_none_or(|g| g.high_quality_bloom));
            (fxaa && graphics.is_none_or(|g| g.fxaa), hq)
        } else {
            (false, false)
        };
        // Вода с эффектами: с кадром под водой (не WebGL2, детализация не Low) проход сцены
        // делится на два (water_pass.rs).
        let water_fx = !self.terrain.water_fx.is_empty();
        let water_scene = water_fx && self.fx_supported && self.terrain.water_fx.iter().any(|w| w.detail[0] > 0.5);
        if water_fx {
            self.water.ensure_pipeline(&self.device, &self.globals_layout, water_scene);
        }
        if water_scene {
            if let Some((_, depth, _)) = self.depth.as_ref() {
                self.water.scene_copy(&self.device, [scene_width, scene_height], depth, self.depth_generation);
            }
        }
        let (Some((color_view, depth_view, _)), Some(hdr_view)) =
            (self.depth.as_ref(), self.post.scene_target())
        else {
            self.sprite_scratch = scratch;
            self.mesh_scratch = frame;
            return RenderStats::default();
        };
        let mut draw_calls = 0;
        let mut triangles = 0;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        if let Some((_, list)) = &cascades {
            for (index, cascade) in list.iter().enumerate() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sun shadow cascade"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow_layers[index],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                let globals_bind = &self.cascade_globals[index].1;
                let draws = self.draw_shadow_batches_with(&mut pass, globals_bind, &frame.items, &frame.cascade_batches[index]);
                draw_calls += draws;
                stats.shadow_draw_calls += draws;
                let terrain_calls = self.terrain.draw_shadows(&mut pass, globals_bind, cascade.view_proj);
                draw_calls += terrain_calls;
                stats.shadow_draw_calls += terrain_calls;
            }
        } else if shadows {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sun shadows"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_layers[0],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let draws = self.draw_shadow_batches(&mut pass, &frame.items, &frame.shadow_batches);
            draw_calls += draws;
            stats.shadow_draw_calls += draws;
            let terrain_calls = self.terrain.draw_shadows(&mut pass, &self.globals_bind_group, light_view_proj);
            draw_calls += terrain_calls;
            stats.shadow_draw_calls += terrain_calls;
        }

        // Тени ламп: все грани — плитки одного атласа, один проход.
        if let (false, Some(atlas)) = (self.light_frame.shadow_faces.is_empty(), self.lights_gpu.as_ref().and_then(|l| l.atlas())) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("light shadows"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: atlas,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let tile = crate::lights::TILE_SIZE as f32;
            for (index, face) in self.light_frame.shadow_faces.iter().enumerate() {
                pass.set_viewport(face.origin.0 as f32, face.origin.1 as f32, tile, tile, 0.0, 1.0);
                let globals_bind = &self.face_globals[index].1;
                let draws = self.draw_shadow_batches_with(&mut pass, globals_bind, &frame.items, &frame.face_batches[index]);
                draw_calls += draws;
                stats.shadow_draw_calls += draws;
                let terrain_calls = self.terrain.draw_shadows(&mut pass, globals_bind, face.view_proj);
                draw_calls += terrain_calls;
                stats.shadow_draw_calls += terrain_calls;
            }
        }

        {
            let clear = view.clear_color;
            // С водой, читающей кадр, первая часть прохода сводится в копию кадра и
            // сохраняет многовыборочный цвет — вторая продолжит его.
            let split = water_scene.then(|| self.water.scene_view()).flatten();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    // Рисуем в многовыборочную текстуру, итог сводится в `target`.
                    view: color_view,
                    resolve_target: Some(split.unwrap_or(hdr_view)),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.r as f64,
                            g: clear.g as f64,
                            b: clear.b as f64,
                            a: clear.a as f64,
                        }),
                        store: if split.is_some() { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // Обратная глубина: «дальше всего» — ноль.
                        load: wgpu::LoadOp::Clear(crate::depth::FAR_DEPTH),
                        // Эффекты по глубине и вода читают её после прохода.
                        store: if fx.any() || split.is_some() { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if view.sky {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.draw(0..3, 0..1);
                draw_calls += 1;
            }

            let (draws, tris) = self.draw_mesh_batches(&mut pass, &frame.items, &frame.batches);
            draw_calls += draws;
            triangles += tris;

            let (terrain_calls, terrain_triangles) = match &self.lit_binds[self.frame_variant] {
                Some(lit) => self.terrain.draw(&mut pass, lit, self.frame_variant),
                None => (0, 0),
            };
            draw_calls += terrain_calls;
            triangles += terrain_triangles.min(u32::MAX as u64) as u32;

            // Сетка редактора: после непрозрачных мешей (глубина проверяется), до спрайтов.
            if let Some(grid) = &view.grid {
                // Ортографический вид: у матрицы нет перспективного деления (clip.w не зависит от точки).
                let ortho = view.view_proj.x_axis.w == 0.0 && view.view_proj.y_axis.w == 0.0 && view.view_proj.z_axis.w == 0.0;
                self.grid.upload(&self.queue, grid, view.view_proj, view.position, ortho);
                self.grid.draw(&mut pass, &self.globals_bind_group);
                draw_calls += 1;
            }

            // Вторая часть прохода: многовыборочный цвет продолжается, глубина — только чтение
            // (вода читает её как текстуру).
            if split.is_some() {
                drop(pass);
                pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("scene after water copy"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_view,
                        resolve_target: Some(hdr_view),
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: None,
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }

            // Полупрозрачные меши: после непрозрачных и сетки, от дальних к ближним.
            let (draws, tris) = self.draw_mesh_batches(&mut pass, &frame.items, &frame.blend_batches);
            draw_calls += draws;
            triangles += tris;

            // Вода: прозрачная, после всего непрозрачного.
            draw_calls += self.terrain.draw_water(&self.device, &self.queue, &mut pass, &self.globals_bind_group);
            if water_fx {
                draw_calls += self.water.draw(&self.device, &self.queue, &mut pass, &self.globals_bind_group, &self.terrain.water_fx, water_scene);
            }

            if !scratch.batches.is_empty() {
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_vertex_buffer(0, self.sprite_vertices.slice(..));
                pass.set_index_buffer(self.quad_indices.slice(..), wgpu::IndexFormat::Uint32);
                self.count_state(3);
                let mut current: Option<Blend> = None;
                for (texture, blend, range) in &scratch.batches {
                    if current != Some(*blend) {
                        current = Some(*blend);
                        match blend {
                            Blend::Alpha => pass.set_pipeline(&self.sprite_pipeline),
                            Blend::Additive => pass.set_pipeline(&self.sprite_additive_pipeline),
                            // Свой шейдер (renderer/custom.rs); не собран — обычный спрайт.
                            Blend::Custom(id) => {
                                if !self.bind_custom_sprites(&mut pass, *id) {
                                    pass.set_pipeline(&self.sprite_pipeline);
                                }
                            }
                        }
                        self.count_state(1);
                    }
                    self.count_state(1);
                    let gpu_texture = match texture {
                        TexRef::White => &self.white,
                        TexRef::Soft => &self.soft_disc,
                        TexRef::Disc => &self.hard_disc,
                        TexRef::Asset(guid) => self.textures.get(guid).unwrap_or(&self.missing),
                    };
                    pass.set_bind_group(1, &gpu_texture.bind_group, &[]);
                    // Вершины квадов по 4 → индексы по 6.
                    let quads = range.start / QUAD_VERTICES as u32..range.end / QUAD_VERTICES as u32;
                    pass.draw_indexed(quads.start * 6..quads.end * 6, 0, 0..1);
                    draw_calls += 1;
                }
            }
        }
        // Эффекты по глубине: AO (половинное разрешение) и контур — в текстуру для сведения.
        let compose = crate::post::ComposeFx {
            ao: fx.ao[1] > 0.0,
            outline: fx.outline_color,
            outline_strength: if fx.outline[3] > 0.5 { fx.outline_strength } else { 0.0 },
            high_quality_bloom: hq_bloom,
            fxaa,
        };
        if fx.any() {
            if let Some((_, depth_view, _)) = self.depth.as_ref() {
                let post_fx = self.post_fx.get_or_insert_with(|| crate::post_fx::PostFx::new(&self.device, &self.queue));
                post_fx.resize(&self.device, scene_width, scene_height, depth_view, self.depth_generation);
                post_fx.run(&self.queue, &mut encoder, &fx, view.view_proj, view.position);
            }
            let output = self.post_fx.as_ref().and_then(|p| p.output());
            self.post.set_fx(&self.device, compose, output.map(|v| (v, self.depth_generation)));
        } else {
            self.post.set_fx(&self.device, compose, None);
        }
        // Цветокоррекция сцены (компонент `ColorGrading`, grading.rs) — только для вида,
        // который берёт пост-обработку из сцены.
        self.post.set_grading(match view.post {
            None => crate::grading::ColorGrading::scene_params(scene),
            Some(_) => crate::grading::GradingParams::OFF,
        });
        // Свои эффекты камеры (`PostEffect`, renderer/custom.rs) — вокруг сведения кадра.
        let effects = view.overlay
            && self.run_post_effects(world, &mut encoder, target, [width, height], [scene_width, scene_height], post_settings);
        if !effects {
            self.post.run(&self.queue, &mut encoder, target, post_settings);
        }
        // Экранный слой (игровой интерфейс) — поверх пост-обработки, см. overlay.rs.
        if view.overlay {
            draw_calls += self.draw_overlay(world, &mut encoder, target, width, height);
        }
        let submit_clock = Stopwatch::start();
        self.queue.submit(Some(encoder.finish()));
        stats.submit_ms = submit_clock.ms();
        stats.state_changes = self.state_changes.load(std::sync::atomic::Ordering::Relaxed);

        self.sprite_scratch = scratch;
        self.mesh_scratch = frame;
        stats.draw_calls = draw_calls;
        stats.sprites = quad_count;
        stats.triangles = triangles + quad_count * 2;
        stats.textures = self.textures.len() as u32;
        stats.material_bind_groups = self.mesh_pass.bind_count() as u32;
        stats.encode_ms = encode_clock.ms();
        stats
    }

    /// Поправка UV текстуры через маленький кэш последних текстур кадра: без него
    /// каждый спрайт делал бы по три поиска в хеш-таблицах.
    fn cached_inset(
        &mut self,
        cache: &mut RecentCache<Guid, Vec2>,
        guid: Option<Guid>,
        assets: Option<&AssetDatabase>,
    ) -> Vec2 {
        let Some(guid) = guid else {
            return Vec2::ZERO;
        };
        if let Some(inset) = cache.get(&guid) {
            return inset;
        }
        let inset = self.texture_inset(Some(guid), assets);
        cache.put(guid, inset);
        inset
    }

    /// Нарисовать кадр в память и вернуть картинку. Работает без окна —
    /// для скриншот-тестов и CI. Требует рендерер из [`Renderer::headless`].
    pub fn capture(
        &mut self,
        world: &World,
        width: u32,
        height: u32,
    ) -> Result<image::RgbaImage, RenderError> {
        self.capture_view(world, width, height, None).map(|(image, _)| image)
    }

    /// Как [`capture`](Self::capture), но с заданным видом (`None` — камера сцены) и
    /// со статистикой кадра: сколько квадов и draw call'ов ушло на спрайты, тайлы и частицы.
    pub fn capture_view(
        &mut self,
        world: &World,
        width: u32,
        height: u32,
        view: Option<&CameraView>,
    ) -> Result<(image::RgbaImage, RenderStats), RenderError> {
        if self.format != CAPTURE_FORMAT {
            return Err(RenderError::Capture(
                "this renderer draws to a window surface; create a separate one with \
                 `Renderer::headless()` for captures"
                    .to_string(),
            ));
        }
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let stats = match view {
            Some(view) => self.render_with(world, &target_view, width, height, view),
            None => self.render(world, &target_view, width, height),
        };
        read_texture(&self.device, &self.queue, &target, width, height).map(|image| (image, stats))
    }
}

/// Мировые границы объекта по локальным границам меша и матрице.
pub fn transform_bounds(matrix: Mat4, (min, max): (Vec3, Vec3)) -> (Vec3, Vec3) {
    let mut world_min = Vec3::splat(f32::INFINITY);
    let mut world_max = Vec3::splat(f32::NEG_INFINITY);
    for corner in 0..8 {
        let local = Vec3::new(
            if corner & 1 == 0 { min.x } else { max.x },
            if corner & 2 == 0 { min.y } else { max.y },
            if corner & 4 == 0 { min.z } else { max.z },
        );
        let world = matrix.transform_point3(local);
        world_min = world_min.min(world);
        world_max = world_max.max(world);
    }
    (world_min, world_max)
}

/// Скопировать RGBA8-текстуру (с `COPY_SRC`) с GPU в картинку. На этом стоят
/// скриншот-тесты и рендерера, и редактора.
pub fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<image::RgbaImage, RenderError> {
    // Строки в буфере копирования выравниваются по 256 байт.
    let row_bytes = 4 * width;
    let padded_row = row_bytes.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture readback"),
        size: padded_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("capture"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|e| RenderError::Capture(e.to_string()))?
        .map_err(|e| RenderError::Capture(e.to_string()))?;

    let mut pixels = Vec::with_capacity((row_bytes * height) as usize);
    {
        let data = slice.get_mapped_range();
        for row in 0..height as usize {
            let start = row * padded_row as usize;
            pixels.extend_from_slice(&data[start..start + row_bytes as usize]);
        }
    }
    buffer.unmap();
    image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| RenderError::Capture("readback size mismatch".to_string()))
}

/// Сколько каскадов и какой фильтр теней разрешают настройки графики
/// (`GraphicsSettings::shadow_cascades` / `shadow_filter`; без ресурса — как High).
fn quality_shadow_caps(graphics: Option<&GraphicsSettings>) -> (u32, u32) {
    graphics.map_or((4, 2), |g| (g.shadow_cascades.clamp(1, MAX_CASCADES as u32), g.shadow_filter.min(2)))
}

/// Карта теней из `layers` слоёв: вид для чтения шейдерами и виды слоёв для записи.
fn create_shadow_maps(device: &wgpu::Device, layers: u32) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow map"),
        size: wgpu::Extent3d {
            width: SHADOW_SIZE,
            height: SHADOW_SIZE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let all = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let each = (0..layers)
        .map(|layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    (all, each)
}

/// Индексный буфер на `quads` квадов: (0 1 2) (0 2 3) для каждых четырёх вершин.
fn create_quad_indices(device: &wgpu::Device, quads: usize) -> wgpu::Buffer {
    let indices: Vec<u32> = (0..quads.max(1) as u32)
        .flat_map(|q| {
            let b = q * QUAD_VERTICES as u32;
            [b, b + 1, b + 2, b, b + 2, b + 3]
        })
        .collect();
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("quad indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    })
}

fn create_sprite_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sprite vertices"),
        size: (capacity * std::mem::size_of::<SpriteVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mesh instances"),
        size: (capacity * std::mem::size_of::<MeshInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn upload_mesh(device: &wgpu::Device, data: &MeshData) -> GpuMesh {
    let vertices: Vec<MeshVertex> = (0..data.vertex_count())
        .map(|i| MeshVertex {
            position: data.positions[i],
            normal: data.normals[i],
            uv: data.uvs[i],
        })
        .collect();
    // Касательные из файла или посчитанные по UV (для карт нормалей).
    let generated;
    let tangents: &[[f32; 4]] = if data.tangents.len() == data.vertex_count() {
        &data.tangents
    } else {
        let mut copy = data.clone();
        copy.compute_tangents();
        generated = copy.tangents;
        &generated
    };
    GpuMesh {
        tangents: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh tangents"),
            contents: bytemuck::cast_slice(tangents),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        parts: data
            .submeshes
            .iter()
            .map(|part| (part.indices.clone(), part.material.map(|m| m as u32)))
            .collect(),
        id: 0,
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: data.indices.len() as u32,
        bounds: data.bounds(),
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Загрузить картинку на GPU. С `mipmaps` строится полная цепочка уменьшенных копий:
/// без неё текстура на удалённых и наклонных поверхностях рябит.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    image: &image::RgbaImage,
    mipmaps: bool,
) -> GpuTexture {
    upload_texture_as(device, queue, layout, sampler, image, mipmaps, wgpu::TextureFormat::Rgba8UnormSrgb)
}

/// Как [`upload_texture`], но в заданном формате: `Rgba8Unorm` — для данных (нормали,
/// шероховатость), которые нельзя переводить из sRGB.
fn upload_texture_as(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    image: &image::RgbaImage,
    mipmaps: bool,
    format: wgpu::TextureFormat,
) -> GpuTexture {
    let (width, height) = image.dimensions();
    let levels = if mipmaps {
        32 - width.max(height).leading_zeros()
    } else {
        1
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut level_image = image.clone();
    for level in 0..levels {
        let (w, h) = ((width >> level).max(1), (height >> level).max(1));
        if level > 0 {
            level_image = image::imageops::resize(&level_image, w, h, image::imageops::FilterType::Triangle);
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            level_image.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
    GpuTexture {
        bind_group,
        // Линейная фильтрация подмешивает соседние кадры листа — отступаем на полтекселя.
        inset: if mipmaps {
            Vec2::new(0.5 / width as f32, 0.5 / height as f32)
        } else {
            Vec2::ZERO
        },
        view,
        nearest: !mipmaps,
        pixel: false,
        atlas: false,
    }
}

/// Фильтр текстуры из настройки импорта `filter` в `.meta`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureFilter {
    /// Сглаживание и мипмапы (по умолчанию).
    Linear,
    /// Пиксель-арт без мипмапов (спрайты, интерфейс).
    Nearest,
    /// Пиксель-арт на поверхностях мира: вблизи — ближайший тексель (резко), вдали —
    /// мипмапы и трилинейное уменьшение (без ряби на мостовой и стенах).
    Pixel,
}

impl TextureFilter {
    pub(crate) fn from_setting(value: Option<&Value>) -> Self {
        match value {
            Some(Value::Enum(name)) if name == "Nearest" => Self::Nearest,
            Some(Value::Enum(name)) if name == "Pixel" => Self::Pixel,
            _ => Self::Linear,
        }
    }
}

/// Сэмплер `Pixel` для мешей: повторение, увеличение — ближайший тексель, уменьшение и
/// переход между мипами — линейно (анизотропия в wgpu требует линейного увеличения).
pub(crate) fn pixel_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("tiled pixel-art sampler with mipmaps"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..wgpu::SamplerDescriptor::default()
    }
}

#[cfg(test)]
mod pixel_filter_tests {
    use super::*;

    #[test]
    fn the_filter_setting_picks_linear_nearest_or_pixel() {
        assert_eq!(TextureFilter::from_setting(None), TextureFilter::Linear);
        assert_eq!(TextureFilter::from_setting(Some(&Value::Enum("Linear".into()))), TextureFilter::Linear);
        assert_eq!(TextureFilter::from_setting(Some(&Value::Enum("Nearest".into()))), TextureFilter::Nearest);
        assert_eq!(TextureFilter::from_setting(Some(&Value::Enum("Pixel".into()))), TextureFilter::Pixel);
    }

    #[test]
    fn pixel_textures_are_sharp_up_close_and_mipmapped_far_away() {
        let d = pixel_sampler_descriptor();
        assert_eq!(d.mag_filter, wgpu::FilterMode::Nearest);
        assert_eq!(d.min_filter, wgpu::FilterMode::Linear);
        assert_eq!(d.mipmap_filter, wgpu::FilterMode::Linear);
        assert_eq!(d.address_mode_u, wgpu::AddressMode::Repeat);
    }
}

/// Защита атласа модели Piska Modeler от смешивания граней при линейной фильтрации:
/// UV каждого треугольника сдвигаются внутрь его прямоугольника в атласе на полтекселя
/// (билинейная выборка у края грани тогда не задевает соседний прямоугольник). Для
/// «ближайшего» фильтра (по умолчанию у `.pmodel`) не нужно — UV не меняются, крайние
/// пиксели граней остаются полной ширины. Вершины на стыке разных прямоугольников
/// (у моделера их нет) ограничиваются пересечением прямоугольников.
fn inset_atlas_uvs(data: &mut MeshData, guid: Guid, assets: &AssetDatabase) {
    let is_atlas = assets.relative_path_of(guid).is_some_and(engine_assets::has_embedded_atlas);
    let linear = assets
        .meta(guid)
        .and_then(|meta| meta.settings.get("filter"))
        .is_some_and(|v| *v == Value::Enum("Linear".to_string()));
    if !is_atlas || !linear || data.uvs.len() != data.positions.len() {
        return;
    }
    let Ok(bytes) = assets.read_image(guid) else { return };
    let Ok((width, height)) = image::ImageReader::new(std::io::Cursor::new(&bytes[..]))
        .with_guessed_format()
        .map_err(|_| ())
        .and_then(|r| r.into_dimensions().map_err(|_| ()))
    else {
        return;
    };
    let half = Vec2::new(0.5 / width.max(1) as f32, 0.5 / height.max(1) as f32);
    let mut ranges = vec![(Vec2::splat(f32::NEG_INFINITY), Vec2::splat(f32::INFINITY)); data.uvs.len()];
    for triangle in data.indices.chunks_exact(3) {
        let corners = [triangle[0], triangle[1], triangle[2]].map(|i| Vec2::from(data.uvs[i as usize]));
        let min = corners[0].min(corners[1]).min(corners[2]);
        let max = corners[0].max(corners[1]).max(corners[2]);
        // Внутрь на полтекселя, но не дальше середины (грань уже тексела).
        let middle = (min + max) * 0.5;
        let low = (min + half).min(middle);
        let high = (max - half).max(middle);
        for &i in triangle {
            let range = &mut ranges[i as usize];
            range.0 = range.0.max(low);
            range.1 = range.1.min(high);
        }
    }
    for (uv, (low, high)) in data.uvs.iter_mut().zip(ranges) {
        if low.x <= high.x && low.y <= high.y {
            *uv = Vec2::from(*uv).clamp(low, high).to_array();
        }
    }
}
