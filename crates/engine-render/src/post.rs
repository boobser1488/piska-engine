//! Пост-обработка: сцена рисуется в HDR-текстуру, затем блум и сведение в цель кадра.

use crate::grading::GradingParams;

/// Формат, в котором рисуется сцена до пост-обработки.
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const BLOOM_LEVELS: usize = 5;

/// Настройки сведения кадра.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSettings {
    /// Тонмаппинг ACES, блум и виньетка. Выключено — кадр выводится как есть (2D).
    pub enabled: bool,
    pub exposure: f32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub vignette: f32,
}

impl PostSettings {
    /// Вывод без обработки — для 2D, где цвета спрайтов должны совпадать с макетом.
    pub const OFF: PostSettings = PostSettings {
        enabled: false,
        exposure: 1.0,
        bloom: 0.0,
        bloom_threshold: 1.0,
        vignette: 0.0,
    };
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            exposure: 1.0,
            bloom: 0.06,
            bloom_threshold: 1.0,
            vignette: 0.18,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostParams {
    exposure: f32,
    bloom_strength: f32,
    bloom_threshold: f32,
    vignette: f32,
    enabled: f32,
    pad: [f32; 3],
    // Цветокоррекция (grading.rs): баланс, тон, тени, света.
    balance: [f32; 4],
    tone: [f32; 4],
    lift: [f32; 4],
    gain: [f32; 4],
    /// x — затенение (AO) включено, y — сила контура, z — блум «13 выборок», w — FXAA.
    fx: [f32; 4],
    /// rgb — цвет контура.
    outline_color: [f32; 4],
}

/// Эффекты сведения кадра из сцены (post_fx.rs, `PostQuality`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ComposeFx {
    pub ao: bool,
    /// rgb — цвет, a — 1: затемнение цвета под линией.
    pub outline: [f32; 4],
    pub outline_strength: f32,
    pub high_quality_bloom: bool,
    pub fxaa: bool,
}

/// Текстуры под конкретный размер кадра.
struct Sized {
    size: [u32; 2],
    /// Сведённый из MSAA HDR-кадр сцены.
    hdr_view: wgpu::TextureView,
    bloom_views: Vec<wgpu::TextureView>,
    prefilter: wgpu::BindGroup,
    /// `down[i]` читает уровень `i`, пишет в `i + 1`.
    down: Vec<wgpu::BindGroup>,
    /// `up[i]` читает уровень `i + 1`, добавляет к уровню `i`.
    up: Vec<wgpu::BindGroup>,
    composite: wgpu::BindGroup,
    /// Сведение с текстурой эффектов по глубине (AO, контур), если она есть.
    composite_fx: Option<(u32, wgpu::BindGroup)>,
    /// Промежуточный кадр для FXAA (в формате цели) и его группа.
    ldr: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

pub(crate) struct PostChain {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    prefilter_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    fxaa_pipeline: wgpu::RenderPipeline,
    down13_pipeline: wgpu::RenderPipeline,
    prefilter13_pipeline: wgpu::RenderPipeline,
    output_format: wgpu::TextureFormat,
    /// 1×1 «без эффектов» (r = 1 — без затенения, g = 0 — без контура).
    no_fx: wgpu::TextureView,
    fx: ComposeFx,
    sized: Option<Sized>,
    /// Цветокоррекция следующего `run` (рендерер ставит её из сцены каждый кадр).
    grading: GradingParams,
}

impl PostChain {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, output_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post layout"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &'static str, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };

        let no_fx = {
            use wgpu::util::DeviceExt;
            device
                .create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some("no post fx"),
                        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &[255, 0, 0, 255],
                )
                .create_view(&Default::default())
        };
        Self {
            fxaa_pipeline: pipeline("fs_fxaa", output_format, None),
            down13_pipeline: pipeline("fs_down13", HDR_FORMAT, None),
            prefilter13_pipeline: pipeline("fs_prefilter13", HDR_FORMAT, None),
            output_format,
            no_fx,
            fx: ComposeFx::default(),
            prefilter_pipeline: pipeline("fs_prefilter", HDR_FORMAT, None),
            down_pipeline: pipeline("fs_down", HDR_FORMAT, None),
            up_pipeline: pipeline("fs_up", HDR_FORMAT, Some(additive)),
            composite_pipeline: pipeline("fs_composite", output_format, None),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..wgpu::SamplerDescriptor::default()
            }),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post params"),
                size: std::mem::size_of::<PostParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            layout,
            sized: None,
            grading: GradingParams::OFF,
        }
    }

    /// Эффекты следующего сведения: AO / контур из `fx_view` (текстура post_fx.rs с
    /// номером поколения `generation`; `None` — без них), блум «13 выборок», FXAA.
    pub(crate) fn set_fx(&mut self, device: &wgpu::Device, fx: ComposeFx, fx_view: Option<(&wgpu::TextureView, u32)>) {
        self.fx = fx;
        let Some(sized) = &mut self.sized else {
            return;
        };
        match fx_view {
            Some((view, generation)) => {
                if sized.composite_fx.as_ref().map(|(g, _)| *g) != Some(generation) {
                    let bind = Self::bind_with(device, &self.layout, &self.sampler, &self.params, &sized.hdr_view, &sized.bloom_views[0], view);
                    sized.composite_fx = Some((generation, bind));
                }
            }
            None => sized.composite_fx = None,
        }
        if fx.fxaa && sized.ldr.is_none() {
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("fxaa source"),
                    size: wgpu::Extent3d { width: sized.size[0], height: sized.size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.output_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let bind = Self::bind_with(device, &self.layout, &self.sampler, &self.params, &view, &view, &self.no_fx);
            sized.ldr = Some((view, bind));
        }
    }

    fn bind_with(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
        source: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
        fx: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(bloom) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(fx) },
            ],
        })
    }

    /// Цветокоррекция для следующих сведений кадра (`GradingParams::OFF` — без неё).
    pub(crate) fn set_grading(&mut self, grading: GradingParams) {
        self.grading = grading;
    }

    /// Подготовить текстуры под размер кадра (пересоздаются при его смене).
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let size = [width.max(1), height.max(1)];
        if self.sized.as_ref().is_some_and(|sized| sized.size == size) {
            return;
        }
        let texture = |label, width: u32, height: u32| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: width.max(1),
                        height: height.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let hdr_view = texture("hdr scene", size[0], size[1]);
        let bloom_views: Vec<wgpu::TextureView> = (0..BLOOM_LEVELS)
            .map(|level| texture("bloom level", size[0] >> (level + 1), size[1] >> (level + 1)))
            .collect();

        let bind = |source: &wgpu::TextureView, bloom: &wgpu::TextureView| {
            Self::bind_with(device, &self.layout, &self.sampler, &self.params, source, bloom, &self.no_fx)
        };
        // Проходы блума не читают `bloom_texture`, поэтому туда идёт тот же источник.
        let prefilter = bind(&hdr_view, &hdr_view);
        let down = (0..BLOOM_LEVELS - 1)
            .map(|level| bind(&bloom_views[level], &bloom_views[level]))
            .collect();
        let up = (0..BLOOM_LEVELS - 1)
            .map(|level| bind(&bloom_views[level + 1], &bloom_views[level + 1]))
            .collect();
        let composite = bind(&hdr_view, &bloom_views[0]);

        self.sized = Some(Sized {
            size,
            hdr_view,
            bloom_views,
            prefilter,
            down,
            up,
            composite,
            composite_fx: None,
            ldr: None,
        });
    }

    /// Куда сводится (resolve) многовыборочный кадр сцены. Вызывать после `resize`.
    pub fn scene_target(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|sized| &sized.hdr_view)
    }

    /// Блум и сведение HDR-кадра в `target`.
    pub fn run(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        settings: PostSettings,
    ) {
        let Some(sized) = &self.sized else {
            return;
        };
        let bloom = settings.enabled && settings.bloom > 0.0;
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&PostParams {
                exposure: settings.exposure,
                bloom_strength: if bloom { settings.bloom } else { 0.0 },
                bloom_threshold: settings.bloom_threshold,
                vignette: settings.vignette,
                enabled: if settings.enabled { 1.0 } else { 0.0 },
                pad: [0.0; 3],
                balance: self.grading.balance,
                tone: self.grading.tone,
                lift: self.grading.lift,
                gain: self.grading.gain,
                fx: [
                    if self.fx.ao && sized.composite_fx.is_some() { 1.0 } else { 0.0 },
                    if sized.composite_fx.is_some() { self.fx.outline_strength } else { 0.0 },
                    if self.fx.high_quality_bloom { 1.0 } else { 0.0 },
                    if self.fx.fxaa { 1.0 } else { 0.0 },
                ],
                outline_color: self.fx.outline,
            }),
        );

        let pass = |encoder: &mut wgpu::CommandEncoder,
                    pipeline: &wgpu::RenderPipeline,
                    bind_group: &wgpu::BindGroup,
                    view: &wgpu::TextureView,
                    load: wgpu::LoadOp<wgpu::Color>| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);

        if bloom {
            // Блум «13 выборок» (`PostQuality`): уменьшение фильтром из 13 выборок, на первом
            // шаге — среднее Кариса (одиночные яркие пиксели не мерцают).
            let (prefilter, down) = if self.fx.high_quality_bloom {
                (&self.prefilter13_pipeline, &self.down13_pipeline)
            } else {
                (&self.prefilter_pipeline, &self.down_pipeline)
            };
            pass(encoder, prefilter, &sized.prefilter, &sized.bloom_views[0], clear);
            for level in 0..BLOOM_LEVELS - 1 {
                pass(encoder, down, &sized.down[level], &sized.bloom_views[level + 1], clear);
            }
            for level in (0..BLOOM_LEVELS - 1).rev() {
                pass(encoder, &self.up_pipeline, &sized.up[level], &sized.bloom_views[level], wgpu::LoadOp::Load);
            }
        }
        let composite = sized.composite_fx.as_ref().map_or(&sized.composite, |(_, bind)| bind);
        match (&sized.ldr, self.fx.fxaa) {
            (Some((ldr, ldr_bind)), true) => {
                pass(encoder, &self.composite_pipeline, composite, ldr, clear);
                pass(encoder, &self.fxaa_pipeline, ldr_bind, target, clear);
            }
            _ => pass(encoder, &self.composite_pipeline, composite, target, clear),
        }
    }
}
