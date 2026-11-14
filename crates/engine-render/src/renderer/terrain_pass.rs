//! Проход рельефа и воды: подгрузка чанков вокруг камеры, уровни детализации,
//! пулы вершин на GPU, отсечение, карта теней, вода.
//!
//! Как в образце: меши чанков строятся в фоновых потоках (в браузере — по кусочку
//! времени в основном потоке), основной поток только копирует готовые в пулы, не
//! больше `MAX_UPLOADS_PER_FRAME` за кадр. Нужное ищется заново, только когда камера
//! сдвинулась на несколько метров, что-то ещё в пути или изменились правки. Чанки
//! одного LOD лежат в общих буферах-страницах одинаковыми слотами. Дальние чанки
//! выгружаются. Пока чанк перестраивается (другой LOD, скульпт), рисуется старый.

use std::collections::HashMap;

use engine_assets::{AssetDatabase, Guid};
use engine_core::prelude::*;

use super::walk::Frustum;
use super::{Renderer, DEPTH_FORMAT, SAMPLES};
use crate::post::HDR_FORMAT;
use crate::terrain::{Terrain, Water};
use crate::terrain_data::TerrainSampler;
use crate::terrain_mesh::{build_chunk, build_indices, lod_cells, vertex_count, ChunkMesh, TerrainVertex};
use crate::terrain_textures::{builtin_layers, downsample, from_image, LayerPixels, LAYER_TOTAL, TEX_SIZE};

/// Сколько готовых чанков копировать на GPU за кадр (кадр не проседает).
const MAX_UPLOADS_PER_FRAME: usize = 16;
/// Сдвиг камеры (м), после которого заново ищется, чего не хватает.
const RESCAN_DISTANCE: f32 = 4.0;
/// Размер страницы пула вершин.
const PAGE_BYTES: u64 = 4 << 20;
/// Без фоновых потоков (браузер): столько миллисекунд за кадр на построение мешей.
#[cfg(target_arch = "wasm32")]
const INLINE_BUDGET_MS: f64 = 3.0;
/// Слоёв в GPU-массиве: 6 нужных + запасной. Квадратный массив ровно из 6 слоёв
/// бэкенд GL (WebGL2) может принять за кубическую карту.
const ARRAY_LAYERS: u32 = LAYER_TOTAL + 1;

/// Статистика рельефа за последний кадр ([`Renderer::terrain_stats`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TerrainStats {
    /// Активных рельефов.
    pub terrains: u32,
    /// Чанков на GPU.
    pub chunks_loaded: u32,
    /// Нарисовано чанков (после отсечения).
    pub chunks_drawn: u32,
    pub triangles: u64,
    pub draw_calls: u32,
    /// Чанков в карте теней.
    pub shadow_chunks: u32,
    /// Скопировано на GPU в этом кадре.
    pub uploads: u32,
    /// Ждут построения в фоне.
    pub pending: u32,
    /// Память пулов вершин на GPU, байт.
    pub gpu_bytes: u64,
    /// Нарисовано чанков по уровням детализации (0 — самый детальный).
    pub lod_counts: [u32; 5],
    /// Время построения мешей, принятых в этом кадре (сумма по потокам), мс.
    pub build_ms: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TerrainUniform {
    origin: [f32; 4],
    params: [f32; 4],
    tints: [[f32; 4]; 5],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WaterUniform {
    center: [f32; 4],
    deep: [f32; 4],
    wave: [f32; 4],
}

/// Слоты одинакового размера в нескольких больших буферах.
struct VertexPool {
    slot_bytes: u64,
    slots_per_page: u32,
    pages: Vec<wgpu::Buffer>,
    free: Vec<(u32, u32)>,
}

impl VertexPool {
    fn new(cells: i32) -> Self {
        let slot_bytes = (vertex_count(cells) * std::mem::size_of::<TerrainVertex>()) as u64;
        let slots_per_page = (PAGE_BYTES / slot_bytes).clamp(1, 4096) as u32;
        Self { slot_bytes, slots_per_page, pages: Vec::new(), free: Vec::new() }
    }

    fn alloc(&mut self, device: &wgpu::Device) -> (u32, u32) {
        if let Some(slot) = self.free.pop() {
            return slot;
        }
        let page = self.pages.len() as u32;
        self.pages.push(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain vertex pool"),
            size: self.slot_bytes * self.slots_per_page as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.free.extend((0..self.slots_per_page).rev().map(|s| (page, s)));
        self.free.pop().expect("a fresh page has free slots")
    }

    fn release(&mut self, page: u32, slot: u32) {
        self.free.push((page, slot));
    }

    fn slice(&self, page: u32, slot: u32) -> wgpu::BufferSlice<'_> {
        let start = slot as u64 * self.slot_bytes;
        self.pages[page as usize].slice(start..start + self.slot_bytes)
    }

    fn bytes(&self) -> u64 {
        self.pages.len() as u64 * self.slot_bytes * self.slots_per_page as u64
    }
}

struct GpuChunk {
    lod: u32,
    page: u32,
    slot: u32,
    signature: u64,
    min_y: f32,
    max_y: f32,
}

/// Состояние одного рельефа на GPU.
struct TerrainState {
    id: GameObjectId,
    /// Уникальный номер — по нему находят своё готовые меши из потоков.
    token: u64,
    shape: u64,
    seen: u64,
    sampler: TerrainSampler,
    /// Отпечаток правок, с которыми сверялись чанки.
    edits: u64,
    origin: Vec3,
    layer: u32,
    cast_shadows: bool,
    view_distance: f32,
    lod_distance: f32,
    chunks: HashMap<(i32, i32), GpuChunk>,
    /// Что строится: ключ → (LOD, подпись).
    pending: HashMap<(i32, i32), (u32, u64)>,
    /// Чанки, которые надо перестроить из-за правок.
    dirty: bool,
    /// Зрители (камеры в локальных координатах) при последнем поиске нужного.
    last_scan: Vec<Vec3>,
    outstanding: usize,
    pools: Vec<VertexPool>,
    /// Индексы всех LOD подряд: (первый индекс, число) на уровень.
    index_buffer: wgpu::Buffer,
    lod_ranges: Vec<(u32, u32)>,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    textures_key: [Guid; 5],
    /// Свои массивы текстур (если назначены свои слои).
    own_textures: Option<(wgpu::Texture, wgpu::Texture)>,
}

impl TerrainState {
    fn release_chunk(&mut self, key: (i32, i32)) {
        if let Some(chunk) = self.chunks.remove(&key) {
            self.pools[chunk.lod as usize].release(chunk.page, chunk.slot);
        }
    }

    /// Расстояние (м) по горизонтали от камеры до чанка.
    fn chunk_distance(&self, key: (i32, i32), camera: Vec3) -> f32 {
        let size = self.sampler.layout().chunk_size;
        let (min_x, min_z) = (key.0 as f32 * size, key.1 as f32 * size);
        let dx = (min_x - camera.x).max(camera.x - (min_x + size)).max(0.0);
        let dz = (min_z - camera.z).max(camera.z - (min_z + size)).max(0.0);
        (dx * dx + dz * dz).sqrt()
    }

    fn lod_for(&self, distance: f32) -> u32 {
        let layout = self.sampler.layout();
        let d = distance / (layout.chunk_size * self.lod_distance.max(0.05));
        let lod = match d {
            d if d < 2.0 => 0,
            d if d < 5.0 => 1,
            d if d < 10.0 => 2,
            d if d < 20.0 => 3,
            _ => 4,
        };
        lod.min(layout.lod_count() - 1)
    }

    /// Расстояние до ближайшего зрителя.
    fn near(&self, key: (i32, i32), viewers: &[Vec3]) -> f32 {
        viewers.iter().map(|v| self.chunk_distance(key, *v)).fold(f32::INFINITY, f32::min)
    }

    /// Что строить для чанка при этих зрителях: уровень (самый детальный из нужных),
    /// подпись, грубые соседи.
    fn wanted(&self, key: (i32, i32), viewers: &[Vec3]) -> (u32, u64, [bool; 4]) {
        let lod = self.lod_for(self.near(key, viewers));
        let chunks = self.sampler.layout().chunks;
        let mut coarser = [false; 4];
        let mut mask = 0u64;
        for (e, (dx, dz)) in [(0, -1), (0, 1), (-1, 0), (1, 0)].into_iter().enumerate() {
            let next = (key.0 + dx, key.1 + dz);
            if next.0 < 0 || next.1 < 0 || next.0 >= chunks || next.1 >= chunks {
                continue;
            }
            if self.lod_for(self.near(next, viewers)) > lod {
                coarser[e] = true;
                mask |= 1 << e;
            }
        }
        (lod, self.signature(key, lod) ^ (mask << 59), coarser)
    }

    /// Подпись чанка на уровне `lod`: учитывает правки в рамке, которая читается
    /// при построении (нормали и AO).
    fn signature(&self, key: (i32, i32), lod: u32) -> u64 {
        let layout = self.sampler.layout();
        let cells = lod_cells(layout.resolution, lod);
        let step = layout.resolution / cells;
        let step_m = layout.spacing() * step as f32;
        let border = ((16.0 / step_m).ceil() as i32).clamp(1, 24);
        self.sampler.chunk_signature(key.0, key.1, (border + 1) * step) ^ lod as u64
    }
}

/// Нужный чанк: приоритет, ключ, LOD, подпись, грубые соседи.
type Wanted = (f32, (i32, i32), u32, u64, [bool; 4]);

struct Job {
    owner: u64,
    key: (i32, i32),
    lod: u32,
    signature: u64,
    coarser: [bool; 4],
    sampler: TerrainSampler,
}

struct Built {
    owner: u64,
    mesh: ChunkMesh,
    ms: f32,
}

fn run_job(job: Job) -> Built {
    let clock = web_time::Instant::now();
    let mesh = build_chunk(&job.sampler, job.key, job.lod, job.signature, job.coarser);
    Built { owner: job.owner, mesh, ms: clock.elapsed().as_secs_f32() * 1000.0 }
}

/// Фоновые потоки построения мешей (на десктопе и Android). Потоки завершаются,
/// когда рендерер уничтожен (канал закрыт).
struct Workers {
    #[cfg(not(target_arch = "wasm32"))]
    jobs: std::sync::mpsc::Sender<Job>,
    #[cfg(not(target_arch = "wasm32"))]
    results: std::sync::mpsc::Receiver<Built>,
    #[cfg(target_arch = "wasm32")]
    queue: std::collections::VecDeque<Job>,
    count: usize,
}

impl Workers {
    #[cfg(not(target_arch = "wasm32"))]
    fn new() -> Self {
        use std::sync::{mpsc, Arc, Mutex};
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (result_tx, results) = mpsc::channel::<Built>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let count = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 3);
        let mut started = 0;
        for i in 0..count {
            let rx = job_rx.clone();
            let tx = result_tx.clone();
            let spawned = std::thread::Builder::new().name(format!("terrain-chunks-{i}")).spawn(move || loop {
                let job = rx.lock().map(|rx| rx.recv());
                let Ok(Ok(job)) = job else { break };
                if tx.send(run_job(job)).is_err() {
                    break;
                }
            });
            if spawned.is_ok() {
                started += 1;
            }
        }
        if started == 0 {
            log::warn!("terrain: no worker threads could be started; chunks are built on the main thread");
        }
        Self { jobs, results, count: started }
    }

    #[cfg(target_arch = "wasm32")]
    fn new() -> Self {
        Self { queue: std::collections::VecDeque::new(), count: 0 }
    }

    fn submit(&mut self, job: Job, inline: &mut Vec<Built>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.count == 0 {
                inline.push(run_job(job));
            } else if let Err(err) = self.jobs.send(job) {
                inline.push(run_job(err.0));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = inline;
            self.queue.push_back(job);
        }
    }

    /// Готовые меши. `wait` — дождаться хотя бы одного (если что-то в пути).
    fn collect(&mut self, out: &mut Vec<Built>, wait: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if wait {
                if let Ok(built) = self.results.recv_timeout(std::time::Duration::from_secs(30)) {
                    out.push(built);
                }
            }
            while let Ok(built) = self.results.try_recv() {
                out.push(built);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Без потоков: строим здесь, пока не кончится бюджет кадра.
            let clock = web_time::Instant::now();
            while let Some(job) = self.queue.pop_front() {
                out.push(run_job(job));
                if !wait && clock.elapsed().as_secs_f64() * 1000.0 > INLINE_BUDGET_MS {
                    break;
                }
            }
        }
    }
}

struct WaterDraw {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// Всё GPU-состояние рельефов и воды одного рендерера.
pub(crate) struct TerrainPass {
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    water_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Встроенные массивы текстур (создаются при первом рельефе).
    builtin: Option<(wgpu::Texture, wgpu::Texture)>,
    states: Vec<TerrainState>,
    workers: Option<Workers>,
    /// Камеры последних кадров (позиция, кадр): подгрузка — для всех сразу.
    viewers: Vec<(Vec3, u64)>,
    /// Готовые меши сверх бюджета кадра — ждут следующего кадра.
    backlog: Vec<Built>,
    in_flight: usize,
    next_token: u64,
    frame: u64,
    /// Строить всё нужное сразу, в этом же кадре (скриншоты и тесты).
    pub blocking: bool,
    /// Чанки к отрисовке в этом кадре: (состояние, ключ).
    visible: Vec<(usize, (i32, i32), f32)>,
    waters: Vec<WaterUniform>,
    water_draws: Vec<WaterDraw>,
    stats: TerrainStats,
}

impl TerrainPass {
    pub fn new(device: &wgpu::Device, lit_layout: &wgpu::BindGroupLayout, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain shader"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("../common.wgsl"), include_str!("../terrain.wgsl")).into()),
        });
        let water_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water shader"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("../common.wgsl"), include_str!("../water.wgsl")).into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let uniform_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform_entry(3),
            ],
        });
        let water_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water layout"),
            entries: &[uniform_entry(0)],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Snorm8x4, 2 => Unorm8x4, 3 => Unorm8x4],
        };
        let depth = |write: bool, compare| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: write,
            depth_compare: compare,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let main_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain pipeline layout"),
            bind_group_layouts: &[lit_layout, &layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain pipeline"),
            layout: Some(&main_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&vertex_layout),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            // Юбки видны с обеих сторон.
            primitive: wgpu::PrimitiveState::default(),
            // Обратная глубина (depth.rs): ближе — больше.
            depth_stencil: Some(depth(true, wgpu::CompareFunction::Greater)),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..wgpu::MultisampleState::default() },
            multiview: None,
            cache: None,
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain shadow layout"),
            bind_group_layouts: &[globals_layout, &layout],
            push_constant_ranges: &[],
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain shadow pipeline"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                bias: wgpu::DepthBiasState { constant: 0, slope_scale: 1.5, clamp: 0.0 },
                ..depth(true, wgpu::CompareFunction::Less)
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let water_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water pipeline layout"),
            bind_group_layouts: &[globals_layout, &water_layout],
            push_constant_ranges: &[],
        });
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water pipeline"),
            layout: Some(&water_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &water_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &water_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth(false, wgpu::CompareFunction::Greater)),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..wgpu::MultisampleState::default() },
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("terrain sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..wgpu::SamplerDescriptor::default()
        });
        Self {
            pipeline,
            shadow_pipeline,
            water_pipeline,
            layout,
            water_layout,
            sampler,
            builtin: None,
            states: Vec::new(),
            workers: None,
            viewers: Vec::new(),
            backlog: Vec::new(),
            in_flight: 0,
            next_token: 1,
            frame: 0,
            blocking: false,
            visible: Vec::new(),
            waters: Vec::new(),
            water_draws: Vec::new(),
            stats: TerrainStats::default(),
        }
    }

    pub fn stats(&self) -> TerrainStats {
        self.stats
    }

    /// Массивы текстур слоёв с мипмапами: `layers[i]` или встроенный слой.
    fn create_arrays(device: &wgpu::Device, queue: &wgpu::Queue, own: &[Option<LayerPixels>]) -> (wgpu::Texture, wgpu::Texture) {
        let builtin = builtin_layers();
        let mips = TEX_SIZE.ilog2() + 1;
        let create = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: TEX_SIZE, height: TEX_SIZE, depth_or_array_layers: ARRAY_LAYERS },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let albedo = create("terrain albedo", wgpu::TextureFormat::Rgba8UnormSrgb);
        let normal = create("terrain normals", wgpu::TextureFormat::Rgba8Unorm);
        for layer in 0..ARRAY_LAYERS {
            let source = (layer as usize) % LAYER_TOTAL as usize;
            let pixels = own.get(source).and_then(Option::as_ref).unwrap_or(&builtin[source]);
            for (texture, data, is_normal) in [(&albedo, &pixels.albedo, false), (&normal, &pixels.normal, true)] {
                let mut level_data = data.clone();
                let mut size = TEX_SIZE as usize;
                for mip in 0..mips {
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture,
                            mip_level: mip,
                            origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &level_data,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(4 * size as u32),
                            rows_per_image: Some(size as u32),
                        },
                        wgpu::Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
                    );
                    if size > 1 {
                        level_data = downsample(&level_data, size, is_normal);
                        size /= 2;
                    }
                }
            }
        }
        (albedo, normal)
    }

    fn bind_group(&self, device: &wgpu::Device, arrays: &(wgpu::Texture, wgpu::Texture), uniform: &wgpu::Buffer) -> wgpu::BindGroup {
        let view = |texture: &wgpu::Texture| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..wgpu::TextureViewDescriptor::default()
            })
        };
        let (albedo, normal) = (view(&arrays.0), view(&arrays.1));
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&albedo) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&normal) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: uniform.as_entire_binding() },
            ],
        })
    }

    /// Свои слои рельефа: картинки по GUID (пустой GUID — встроенный слой).
    fn load_own_layers(terrain_key: &[Guid; 5], assets: Option<&AssetDatabase>) -> Vec<Option<LayerPixels>> {
        terrain_key
            .iter()
            .enumerate()
            .map(|(layer, guid)| {
                if guid.0 == 0 {
                    return None;
                }
                let loaded = (|| {
                    let assets = assets.ok_or("no AssetDatabase")?;
                    let bytes = assets.read(*guid).map_err(|e| e.to_string())?;
                    let image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
                    Ok::<_, String>(from_image(&image, layer as u32))
                })();
                match loaded {
                    Ok(pixels) => Some(pixels),
                    Err(reason) => {
                        log::error!("Terrain layer texture {guid}: {reason}; using the built-in one");
                        None
                    }
                }
            })
            .collect()
    }

    fn texture_key(terrain: &Terrain) -> [Guid; 5] {
        let guid = |h: &engine_assets::AssetHandle<engine_assets::Texture>| if h.is_none() { Guid(0) } else { h.guid() };
        [
            guid(&terrain.grass_texture),
            guid(&terrain.dirt_texture),
            guid(&terrain.rock_texture),
            guid(&terrain.sand_texture),
            guid(&terrain.snow_texture),
        ]
    }

    fn uniform_of(terrain: &Terrain, origin: Vec3) -> TerrainUniform {
        let c = |c: Color| [c.r, c.g, c.b, c.a];
        TerrainUniform {
            origin: [origin.x, origin.y, origin.z, terrain.texture_scale.max(0.01)],
            params: [terrain.sea_level, terrain.desert.clamp(0.0, 1.0), terrain.snow_height, 0.0],
            tints: [
                c(terrain.grass_color),
                c(terrain.dirt_color),
                c(terrain.rock_color),
                c(terrain.sand_color),
                c(terrain.snow_color),
            ],
        }
    }

    fn new_state(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: GameObjectId, terrain: &Terrain, sampler: TerrainSampler, assets: Option<&AssetDatabase>) -> TerrainState {
        let layout = sampler.layout();
        let mut lod_ranges = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        let mut pools = Vec::new();
        for lod in 0..layout.lod_count() {
            let cells = lod_cells(layout.resolution, lod);
            let list = build_indices(cells);
            lod_ranges.push((indices.len() as u32, list.len() as u32));
            indices.extend_from_slice(&list);
            pools.push(VertexPool::new(cells));
        }
        if indices.len() % 2 == 1 {
            indices.push(0); // размер буфера кратен 4 байтам
        }
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain indices"),
            size: (indices.len() * 2) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(&indices));
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain params"),
            size: std::mem::size_of::<TerrainUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let textures_key = Self::texture_key(terrain);
        let own_textures = textures_key.iter().any(|g| g.0 != 0).then(|| {
            let own = Self::load_own_layers(&textures_key, assets);
            Self::create_arrays(device, queue, &own)
        });
        if self.builtin.is_none() {
            self.builtin = Some(Self::create_arrays(device, queue, &[]));
        }
        let arrays = own_textures.as_ref().or(self.builtin.as_ref()).expect("built-in arrays exist");
        let bind_group = self.bind_group(device, arrays, &uniform);
        let token = self.next_token;
        self.next_token += 1;
        TerrainState {
            id,
            token,
            shape: sampler.shared.hash,
            seen: self.frame,
            edits: sampler.edits.fingerprint(),
            sampler,
            origin: Vec3::ZERO,
            layer: 0,
            cast_shadows: terrain.cast_shadows,
            view_distance: terrain.view_distance,
            lod_distance: terrain.lod_distance,
            chunks: HashMap::new(),
            pending: HashMap::new(),
            dirty: false,
            last_scan: Vec::new(),
            outstanding: 0,
            pools,
            index_buffer,
            lod_ranges,
            uniform,
            bind_group,
            textures_key,
            own_textures,
        }
    }

    /// Подготовка кадра: найти рельефы и воду, подгрузить чанки, отобрать видимые.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world: &World,
        camera: Vec3,
        frustum: &Frustum,
        culling_mask: u32,
        assets: Option<&AssetDatabase>,
    ) {
        self.frame += 1;
        let frame = self.frame;
        let scene = &world.scene;
        self.visible.clear();
        self.waters.clear();
        let mut stats = TerrainStats::default();

        // --- Вода ---
        let time = world.time.elapsed as f32;
        scene.visit_active_with::<Water>(|id, water| {
            let layer = scene.get(id).map_or(0, |o| o.layer.min(31));
            if culling_mask & (1u32 << layer) == 0 {
                return;
            }
            let p = scene.world_matrix(id).w_axis.truncate();
            let c = water.deep_color;
            self.waters.push(WaterUniform {
                center: [p.x, p.y, p.z, (water.size * 0.5).max(0.0)],
                deep: [c.r, c.g, c.b, water.opacity.clamp(0.0, 1.0)],
                wave: [time % 10_000.0, water.waves.max(0.0), water.wave_speed, 0.0],
            });
        });

        // --- Рельефы ---
        let mut found: Vec<(GameObjectId, Vec3, u32)> = Vec::new();
        scene.visit_active_with::<Terrain>(|id, _| {
            let layer = scene.get(id).map_or(0, |o| o.layer.min(31));
            found.push((id, scene.world_matrix(id).w_axis.truncate(), layer));
        });
        if found.is_empty() && self.states.is_empty() {
            self.stats = stats;
            return;
        }
        if self.workers.is_none() && !found.is_empty() {
            self.workers = Some(Workers::new());
        }

        for (id, origin, layer) in found {
            let Some(terrain) = scene.get_component::<Terrain>(id) else { continue };
            let sampler = terrain.sampler(assets);
            // Своё состояние; после Undo / перезагрузки объект новый — подхватываем
            // состояние рельефа той же формы, которое в этом кадре никто не взял.
            let index = self
                .states
                .iter()
                .position(|s| s.id == id && s.seen < frame)
                .or_else(|| self.states.iter().position(|s| s.seen < frame && s.shape == sampler.shared.hash && !found_id_alive(scene, s.id)));
            let index = match index {
                Some(index) if self.states[index].shape == sampler.shared.hash => index,
                Some(index) => {
                    // Форма изменилась: всё заново.
                    let state = self.new_state(device, queue, id, terrain, sampler.clone(), assets);
                    self.states[index] = state;
                    index
                }
                None => {
                    let state = self.new_state(device, queue, id, terrain, sampler.clone(), assets);
                    self.states.push(state);
                    self.states.len() - 1
                }
            };
            // Свои текстуры слоёв сменились — пересоздать массивы.
            let key = Self::texture_key(terrain);
            if key != self.states[index].textures_key {
                let own = key.iter().any(|g| g.0 != 0).then(|| {
                    let layers = Self::load_own_layers(&key, assets);
                    Self::create_arrays(device, queue, &layers)
                });
                let arrays = own.as_ref().or(self.builtin.as_ref()).expect("built-in arrays exist");
                let bind_group = self.bind_group(device, arrays, &self.states[index].uniform);
                let state = &mut self.states[index];
                state.own_textures = own;
                state.bind_group = bind_group;
                state.textures_key = key;
            }
            let state = &mut self.states[index];
            state.id = id;
            state.seen = frame;
            state.origin = origin;
            state.layer = layer;
            state.cast_shadows = terrain.cast_shadows;
            state.view_distance = terrain.view_distance.max(terrain.chunk_size);
            state.lod_distance = terrain.lod_distance;
            queue.write_buffer(&state.uniform, 0, bytemuck::bytes_of(&Self::uniform_of(terrain, origin)));
            let edits = sampler.edits.fingerprint();
            if edits != state.edits {
                state.edits = edits;
                state.dirty = true;
            }
            state.sampler = sampler;
        }

        // Рельефы, которых больше нет, отпускают GPU-память (с запасом в пару секунд —
        // объект могли пересоздать Undo'м).
        let before = self.states.len();
        self.states.retain(|s| frame - s.seen < 120);
        if self.states.len() != before {
            self.in_flight = self.states.iter().map(|s| s.pending.len()).sum();
        }

        // Зрители: этот рендерер может рисовать несколько видов (Scene и Game в редакторе) —
        // подгрузка учитывает все камеры последних кадров, а не прыгает между ними.
        if self.states.iter().any(|s| s.seen == frame) {
            match self.viewers.iter_mut().find(|v| v.0.distance(camera) < 64.0) {
                Some(viewer) => *viewer = (camera, frame),
                None => self.viewers.push((camera, frame)),
            }
        }
        self.viewers.retain(|v| frame - v.1 <= 8);
        let viewers: Vec<Vec3> = self.viewers.iter().map(|v| v.0).collect();
        self.stream(device, queue, &viewers, &mut stats);

        // --- Видимые чанки ---
        for (index, state) in self.states.iter().enumerate() {
            if state.seen != frame {
                continue;
            }
            stats.terrains += 1;
            stats.chunks_loaded += state.chunks.len() as u32;
            stats.pending += state.pending.len() as u32;
            stats.gpu_bytes += state.pools.iter().map(VertexPool::bytes).sum::<u64>();
            if culling_mask & (1u32 << state.layer) == 0 {
                continue;
            }
            let size = state.sampler.layout().chunk_size;
            for (&key, chunk) in &state.chunks {
                let min = state.origin + Vec3::new(key.0 as f32 * size, chunk.min_y, key.1 as f32 * size);
                let max = state.origin + Vec3::new((key.0 + 1) as f32 * size, chunk.max_y, (key.1 + 1) as f32 * size);
                if frustum.intersects_aabb(min, max) {
                    let distance = state.chunk_distance(key, camera - state.origin);
                    self.visible.push((index, key, distance));
                }
            }
        }
        // Спереди назад: ранний тест глубины отбрасывает закрытое.
        self.visible.sort_by(|a, b| a.2.total_cmp(&b.2));
        self.stats = stats;
    }

    /// Подгрузка: принять готовое, выгрузить дальнее, заказать недостающее.
    fn stream(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, viewers: &[Vec3], stats: &mut TerrainStats) {
        let Some(mut workers) = self.workers.take() else { return };
        let frame = self.frame;
        let mut built = std::mem::take(&mut self.backlog);
        let mut rounds = 0;
        let budget = if self.blocking { usize::MAX } else { MAX_UPLOADS_PER_FRAME };
        let mut uploaded = 0usize;
        loop {
            // 1. Готовые меши — в пулы (не больше бюджета за кадр, остальное — в запас).
            let wait = self.blocking && self.in_flight > 0 && built.is_empty();
            workers.collect(&mut built, wait);
            for result in built.drain(..) {
                if uploaded >= budget {
                    self.backlog.push(result);
                    continue;
                }
                self.in_flight = self.in_flight.saturating_sub(1);
                stats.build_ms += result.ms;
                let Some(state) = self.states.iter_mut().find(|s| s.token == result.owner) else { continue };
                let mesh = result.mesh;
                if state.pending.get(&mesh.key) == Some(&(mesh.lod, mesh.signature)) {
                    state.pending.remove(&mesh.key);
                }
                let local: Vec<Vec3> = viewers.iter().map(|v| *v - state.origin).collect();
                let distance = state.near(mesh.key, &local);
                if distance > state.view_distance + state.sampler.layout().chunk_size {
                    continue;
                }
                let (wanted_lod, current, _) = state.wanted(mesh.key, &local);
                let has = state.chunks.contains_key(&mesh.key);
                // Устаревший (другой LOD, соседи или правки) годится только чтобы закрыть дыру.
                if has && (wanted_lod != mesh.lod || current != mesh.signature) {
                    continue;
                }
                state.release_chunk(mesh.key);
                let pool = &mut state.pools[mesh.lod as usize];
                let (page, slot) = pool.alloc(device);
                queue.write_buffer(&pool.pages[page as usize], slot as u64 * pool.slot_bytes, bytemuck::cast_slice(&mesh.vertices));
                state.chunks.insert(
                    mesh.key,
                    GpuChunk { lod: mesh.lod, page, slot, signature: mesh.signature, min_y: mesh.min_y, max_y: mesh.max_y },
                );
                uploaded += 1;
                state.last_scan.clear(); // пересмотреть нужное
            }

            // 2–3. Выгрузить дальнее, заказать недостающее.
            let max_in_flight = if self.blocking { usize::MAX } else { (workers.count.max(1)) * 3 };
            let mut inline = Vec::new();
            for state in self.states.iter_mut().filter(|s| s.seen == frame) {
                let local: Vec<Vec3> = viewers.iter().map(|v| *v - state.origin).collect();
                let moved = local.len() != state.last_scan.len()
                    || local.iter().zip(&state.last_scan).any(|(a, b)| a.distance_squared(*b) > RESCAN_DISTANCE * RESCAN_DISTANCE);
                if !moved && state.outstanding == 0 && state.pending.is_empty() && !state.dirty {
                    continue;
                }
                state.last_scan = local.clone();
                let layout = state.sampler.layout();
                let size = layout.chunk_size;
                let radius = state.view_distance;
                let far: Vec<(i32, i32)> = state
                    .chunks
                    .keys()
                    .copied()
                    .filter(|k| state.near(*k, &local) > radius + size)
                    .collect();
                for key in far {
                    state.release_chunk(key);
                }

                let mut wanted: Vec<Wanted> = Vec::new();
                // Чанки вокруг каждого зрителя (прямоугольник, охватывающий всех).
                let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
                for v in &local {
                    lo = lo.min(*v - Vec3::splat(radius + size));
                    hi = hi.max(*v + Vec3::splat(radius + size));
                }
                let clamp = |v: f32| ((v / size).floor() as i64).clamp(0, layout.chunks as i64 - 1) as i32;
                for cz in clamp(lo.z)..=clamp(hi.z) {
                    for cx in clamp(lo.x)..=clamp(hi.x) {
                        let key = (cx, cz);
                        let d = state.near(key, &local);
                        if d > radius {
                            continue;
                        }
                        let (lod, signature, coarser) = state.wanted(key, &local);
                        let have = state.chunks.get(&key).map(|c| (c.lod, c.signature));
                        if have == Some((lod, signature)) || state.pending.get(&key) == Some(&(lod, signature)) {
                            continue;
                        }
                        // Сначала дыры, потом правки (скульпт — сразу), потом уточнение LOD.
                        let priority = match have {
                            None => d,
                            Some((have_lod, _)) if have_lod == lod => d - 1e6,
                            Some(_) => d + 8.0 * size,
                        };
                        wanted.push((priority, key, lod, signature, coarser));
                    }
                }
                state.dirty = false;
                state.outstanding = wanted.len();
                let free = max_in_flight.saturating_sub(self.in_flight);
                if free == 0 || wanted.is_empty() {
                    continue;
                }
                if wanted.len() > free {
                    wanted.select_nth_unstable_by(free, |a, b| a.0.total_cmp(&b.0));
                    wanted.truncate(free);
                }
                wanted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
                for (_, key, lod, signature, coarser) in wanted {
                    state.pending.insert(key, (lod, signature));
                    self.in_flight += 1;
                    workers.submit(Job { owner: state.token, key, lod, signature, coarser, sampler: state.sampler.clone() }, &mut inline);
                }
            }
            built.append(&mut inline);
            rounds += 1;
            let busy = self.in_flight > 0 || !built.is_empty() || self.states.iter().any(|s| s.seen == frame && s.outstanding > 0);
            if !self.blocking || !busy || rounds > 10_000 {
                break;
            }
        }
        self.backlog.append(&mut built);
        stats.uploads = uploaded as u32;
        self.workers = Some(workers);
    }

    /// Границы рельефа рядом с камерой (для объёма теней солнца).
    pub fn shadow_bounds(&self, camera: Vec3, distance: f32) -> Option<(Vec3, Vec3)> {
        let mut bounds: Option<(Vec3, Vec3)> = None;
        for state in self.states.iter().filter(|s| s.seen == self.frame && s.cast_shadows) {
            let size = state.sampler.layout().chunk_size;
            let local = camera - state.origin;
            for (&key, chunk) in &state.chunks {
                if state.chunk_distance(key, local) > distance {
                    continue;
                }
                let min = state.origin + Vec3::new(key.0 as f32 * size, chunk.min_y, key.1 as f32 * size);
                let max = state.origin + Vec3::new((key.0 + 1) as f32 * size, chunk.max_y, (key.1 + 1) as f32 * size);
                bounds = Some(match bounds {
                    Some((a, b)) => (a.min(min), b.max(max)),
                    None => (min, max),
                });
            }
        }
        bounds
    }

    /// Рельеф в карту теней. Возвращает число вызовов отрисовки.
    pub fn draw_shadows(&mut self, pass: &mut wgpu::RenderPass, globals: &wgpu::BindGroup, light_view_proj: Mat4) -> u32 {
        let light = Frustum::from_view_proj(light_view_proj);
        let mut calls = 0;
        let frame = self.frame;
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, globals, &[]);
        for state in self.states.iter().filter(|s| s.seen == frame && s.cast_shadows && !s.chunks.is_empty()) {
            let size = state.sampler.layout().chunk_size;
            pass.set_bind_group(1, &state.bind_group, &[]);
            pass.set_index_buffer(state.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            for (&key, chunk) in &state.chunks {
                let min = state.origin + Vec3::new(key.0 as f32 * size, chunk.min_y, key.1 as f32 * size);
                let max = state.origin + Vec3::new((key.0 + 1) as f32 * size, chunk.max_y, (key.1 + 1) as f32 * size);
                if !light.intersects_aabb(min, max) {
                    continue;
                }
                let (first, count) = state.lod_ranges[chunk.lod as usize];
                pass.set_vertex_buffer(0, state.pools[chunk.lod as usize].slice(chunk.page, chunk.slot));
                pass.draw_indexed(first..first + count, 0, 0..1);
                calls += 1;
            }
        }
        self.stats.shadow_chunks = calls;
        self.stats.draw_calls += calls;
        calls
    }

    /// Рельеф в кадр. Возвращает (вызовы, треугольники).
    pub fn draw(&mut self, pass: &mut wgpu::RenderPass, lit: &wgpu::BindGroup) -> (u32, u64) {
        if self.visible.is_empty() {
            return (0, 0);
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, lit, &[]);
        let (mut calls, mut triangles) = (0u32, 0u64);
        let mut current: Option<usize> = None;
        for &(index, key, _) in &self.visible {
            let state = &self.states[index];
            let Some(chunk) = state.chunks.get(&key) else { continue };
            if current != Some(index) {
                current = Some(index);
                pass.set_bind_group(1, &state.bind_group, &[]);
                pass.set_index_buffer(state.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            }
            let (first, count) = state.lod_ranges[chunk.lod as usize];
            pass.set_vertex_buffer(0, state.pools[chunk.lod as usize].slice(chunk.page, chunk.slot));
            pass.draw_indexed(first..first + count, 0, 0..1);
            calls += 1;
            triangles += (count / 3) as u64;
            if let Some(slot) = self.stats.lod_counts.get_mut(chunk.lod as usize) {
                *slot += 1;
            }
        }
        self.stats.chunks_drawn = calls;
        self.stats.triangles = triangles;
        self.stats.draw_calls += calls;
        (calls, triangles)
    }

    /// Вода — после непрозрачного (прозрачная, глубину не пишет).
    pub fn draw_water(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass, globals: &wgpu::BindGroup) -> u32 {
        if self.waters.is_empty() {
            return 0;
        }
        while self.water_draws.len() < self.waters.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("water params"),
                size: std::mem::size_of::<WaterUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("water"),
                layout: &self.water_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.water_draws.push(WaterDraw { buffer, bind_group });
        }
        pass.set_pipeline(&self.water_pipeline);
        pass.set_bind_group(0, globals, &[]);
        for (water, draw) in self.waters.iter().zip(&self.water_draws) {
            queue.write_buffer(&draw.buffer, 0, bytemuck::bytes_of(water));
            pass.set_bind_group(1, &draw.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        self.waters.len() as u32
    }
}

fn found_id_alive(scene: &Scene, id: GameObjectId) -> bool {
    scene.get_component::<Terrain>(id).is_some() && scene.is_active(id)
}

impl Renderer {
    /// Рельеф ещё догружается: стоит нарисовать следующий кадр (редактор в режиме Edit
    /// рисует по требованию и спрашивает об этом).
    pub fn terrain_busy(&self) -> bool {
        let pass = &self.terrain;
        pass.in_flight > 0 || !pass.backlog.is_empty() || pass.states.iter().any(|s| s.outstanding > 0 || !s.pending.is_empty())
    }

    /// Статистика рельефов за последний кадр: чанки, треугольники, вызовы, подгрузка.
    pub fn terrain_stats(&self) -> TerrainStats {
        self.terrain.stats()
    }

    /// Строить чанки рельефа сразу, в том же кадре (`true`), или в фоне с бюджетом на
    /// кадр (`false`, по умолчанию в окне и редакторе). Рендерер без окна
    /// ([`Renderer::headless`]) строит сразу — скриншоты сразу полные.
    pub fn set_terrain_blocking(&mut self, blocking: bool) {
        self.terrain.blocking = blocking;
    }
}
