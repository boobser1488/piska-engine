//! Эффекты по глубине кадра: экранное затенение [`AmbientOcclusion`] и контур
//! [`Outline`] — необязательные компоненты на любом объекте сцены.
//!
//! Оба читают буфер глубины сцены после основного прохода (post_fx.wgsl) и пишут одну
//! текстуру полного размера (r — доля рассеянного света, g — сила контура), которую
//! подмешивает сведение кадра. Затенение считается в половинном разрешении и размывается
//! с учётом глубины (без ореолов у краёв); контур — по разрывам расстояния и изломам
//! нормалей: для низкополигональных сцен это «рисованная» обводка граней и силуэтов.
//!
//! Без компонентов этих проходов нет вовсе. В браузере на WebGL2 многовыборочную глубину
//! читать нельзя — там эффекты молча выключены (кадр как без них).

use engine_core::prelude::*;
use wgpu::util::DeviceExt;

component! {
    /// Экранное затенение (SSAO): щели, углы и места под объектами темнее. Действует на
    /// пресетах Medium и выше.
    pub struct AmbientOcclusion {
        /// Включено. `false` — как без компонента.
        pub enabled: bool = true,
        /// Радиус поиска заслонителей, м.
        pub radius: f32 = 0.6,
        /// Сила затенения: 0 — нет, 1 — полная.
        pub intensity: f32 = 0.6,
        /// Смещение против самозатенения, м.
        pub bias: f32 = 0.03,
    }
}

impl Component for AmbientOcclusion {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Контур по разрывам глубины и нормалей — обводка силуэтов и рёбер граней.
    pub struct Outline {
        /// Включено. `false` — как без компонента.
        pub enabled: bool = true,
        /// Цвет контура. При `tint` — множитель цвета под линией (0.3 — линия на 70 % темнее
        /// того, что под ней), иначе — сам цвет линии (линейный).
        pub color: Color = Color::rgb(0.3, 0.3, 0.3),
        /// Линия — затемнённый цвет под ней (`true`) или один цвет `color` (`false`).
        pub tint: bool = true,
        /// Непрозрачность контура, 0…1.
        pub strength: f32 = 1.0,
        /// Толщина в пикселях при 720p (1…3); с разрешением растёт.
        pub thickness: f32 = 1.0,
        /// Порог разрыва расстояния (доля расстояния до камеры): меньше — больше линий.
        pub depth_threshold: f32 = 0.04,
        /// Порог излома нормали (1 − cos угла): 0.3 ≈ 45°. Больше — только острые рёбра.
        pub normal_threshold: f32 = 0.3,
    }
}

impl Component for Outline {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Качество сведения кадра: FXAA (сглаживание краёв, которые не берёт MSAA: вырезы,
    /// текстуры, тонкие линии контура) и блум «13 выборок» (ровнее и без мерцания ярких
    /// точек). Блум 13 выборок — на пресетах Medium и выше.
    pub struct PostQuality {
        /// Включено. `false` — как без компонента.
        pub enabled: bool = true,
        pub fxaa: bool = true,
        pub high_quality_bloom: bool = true,
    }
}

impl Component for PostQuality {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl PostQuality {
    /// (FXAA, блум 13 выборок) первого активного включённого компонента сцены.
    pub(crate) fn settings(scene: &Scene, hq_bloom_allowed: bool) -> (bool, bool) {
        let mut out = (false, false);
        if scene.count_with::<PostQuality>() > 0 {
            let mut done = false;
            scene.visit_active_with::<PostQuality>(|_, q| {
                if !done && q.enabled {
                    done = true;
                    out = (q.fxaa, q.high_quality_bloom && hq_bloom_allowed);
                }
            });
        }
        out
    }
}

/// Параметры эффектов кадра из сцены (после ограничений пресета).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FxSettings {
    /// x — радиус, y — сила (0 — выкл.), z — смещение, w — выборок.
    pub ao: [f32; 4],
    /// x — порог глубины, y — порог нормалей, z — толщина, w — включён.
    pub outline: [f32; 4],
    /// rgb — цвет, a — 1: линия — затемнение цвета под ней (для сведения кадра).
    pub outline_color: [f32; 4],
    /// Сила контура (для сведения кадра).
    pub outline_strength: f32,
}

impl FxSettings {
    pub fn any(&self) -> bool {
        self.ao[1] > 0.0 || self.outline[3] > 0.5
    }

    /// Первые активные включённые `AmbientOcclusion` и `Outline` сцены. `ao_samples` — 0,
    /// если пресет затенение не разрешает.
    pub fn from_scene(scene: &Scene, ao_samples: u32) -> Self {
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let mut out = FxSettings::default();
        if ao_samples > 0 && scene.count_with::<AmbientOcclusion>() > 0 {
            let mut done = false;
            scene.visit_active_with::<AmbientOcclusion>(|_, ao| {
                if !done && ao.enabled {
                    done = true;
                    out.ao = [
                        finite(ao.radius, 0.6).clamp(0.05, 10.0),
                        finite(ao.intensity, 0.8).clamp(0.0, 1.0),
                        finite(ao.bias, 0.03).clamp(0.0, 1.0),
                        ao_samples as f32,
                    ];
                }
            });
        }
        if scene.count_with::<Outline>() > 0 {
            let mut done = false;
            scene.visit_active_with::<Outline>(|_, o| {
                if !done && o.enabled {
                    done = true;
                    let strength = finite(o.strength, 0.8).clamp(0.0, 1.0);
                    out.outline = [
                        finite(o.depth_threshold, 0.04).clamp(0.001, 10.0),
                        finite(o.normal_threshold, 0.3).clamp(0.0, 2.0),
                        finite(o.thickness, 1.0).clamp(0.5, 3.0),
                        if strength > 0.0 { 1.0 } else { 0.0 },
                    ];
                    let c = o.color;
                    out.outline_color = [finite(c.r, 0.0).max(0.0), finite(c.g, 0.0).max(0.0), finite(c.b, 0.0).max(0.0), if o.tint { 1.0 } else { 0.0 }];
                    out.outline_strength = strength;
                }
            });
        }
        out
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FxParams {
    view_proj: [f32; 16],
    inv_view_proj: [f32; 16],
    camera: [f32; 4],
    ao: [f32; 4],
    outline: [f32; 4],
    size: [f32; 4],
}

/// Текстуры эффектов под размер кадра.
struct Sized {
    size: [u32; 2],
    ao_view: wgpu::TextureView,
    fx_view: wgpu::TextureView,
    ao_bind: wgpu::BindGroup,
    resolve_bind: wgpu::BindGroup,
}

pub(crate) struct PostFx {
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    sampler: wgpu::Sampler,
    ao_pipeline: wgpu::RenderPipeline,
    resolve_pipeline: wgpu::RenderPipeline,
    /// 1×1 «без затенения» на месте AO, пока затенение выключено.
    no_ao: wgpu::TextureView,
    sized: Option<Sized>,
    /// Поколение буфера глубины, под который собраны группы привязок.
    generation: u32,
}

/// Формат текстур эффектов: два канала по 8 бит хватает с запасом.
const FX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl PostFx {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post fx shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post_fx.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post fx layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post fx pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &'static str| {
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
                    targets: &[Some(wgpu::ColorTargetState { format: FX_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let no_ao = device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("no ao"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FX_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[255, 255, 255, 255],
            )
            .create_view(&Default::default());
        Self {
            ao_pipeline: pipeline("fs_ao"),
            resolve_pipeline: pipeline("fs_resolve"),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post fx params"),
                size: std::mem::size_of::<FxParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post fx sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            }),
            layout,
            no_ao,
            sized: None,
            generation: 0,
        }
    }

    /// Текстуры под размер кадра и буфер глубины сцены (пересоздаются при смене размера
    /// или буфера глубины: `generation` растёт при каждом пересоздании глубины).
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32, depth: &wgpu::TextureView, generation: u32) {
        let size = [width.max(1), height.max(1)];
        if self.sized.as_ref().is_some_and(|s| s.size == size) && self.generation == generation {
            return;
        }
        self.generation = generation;
        let texture = |label, w: u32, h: u32| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FX_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let ao_view = texture("ssao", size[0].div_ceil(2), size[1].div_ceil(2));
        let fx_view = texture("post fx", size[0], size[1]);
        let bind = |ao: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post fx"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 1, resource: self.params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(ao) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        // Проход AO не читает `ao_texture`: туда идёт заглушка.
        let ao_bind = bind(&self.no_ao);
        let resolve_bind = bind(&ao_view);
        self.sized = Some(Sized { size, ao_view, fx_view, ao_bind, resolve_bind });
    }

    /// Текстура эффектов для сведения кадра (после `resize`).
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.fx_view)
    }

    /// Записать проходы эффектов. `view_proj` — матрица камеры (обратная глубина).
    pub fn run(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, settings: &FxSettings, view_proj: Mat4, camera: Vec3) {
        let Some(sized) = &self.sized else {
            return;
        };
        let ortho = view_proj.x_axis.w == 0.0 && view_proj.y_axis.w == 0.0 && view_proj.z_axis.w == 0.0;
        let params = FxParams {
            view_proj: view_proj.to_cols_array(),
            inv_view_proj: view_proj.inverse().to_cols_array(),
            camera: [camera.x, camera.y, camera.z, if ortho { 0.0 } else { 1.0 }],
            ao: settings.ao,
            outline: settings.outline,
            size: [sized.size[0] as f32, sized.size[1] as f32, 1.0 / sized.size[0] as f32, 1.0 / sized.size[1] as f32],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let pass = |encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline, bind: &wgpu::BindGroup, view: &wgpu::TextureView| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post fx"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        };
        if settings.ao[1] > 0.0 {
            pass(encoder, &self.ao_pipeline, &sized.ao_bind, &sized.ao_view);
        }
        pass(encoder, &self.resolve_pipeline, &sized.resolve_bind, &sized.fx_view);
    }
}
