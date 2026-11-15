//! Вода с эффектами (`WaterEffects`, water_fx.rs / water_fx.wgsl) на GPU.
//!
//! Если в кадре есть такая вода и кадр можно читать (не WebGL2, пресет не Low), основной
//! проход делится на два: непрозрачное сводится в копию кадра ([`WaterPass::scene_copy`]),
//! затем полупрозрачное, вода и спрайты дорисовываются поверх (глубина — только чтение);
//! шейдер воды читает копию и глубину под собой (преломление, цвет по глубине, пена,
//! отражения сцены). Без такой воды проход один, как раньше.

use super::{DEPTH_FORMAT, SAMPLES};
use crate::post::HDR_FORMAT;
use crate::water_fx::WaterFxUniform;

const COMMON: &str = include_str!("../common.wgsl");

pub(super) struct WaterPass {
    uniform_layout: wgpu::BindGroupLayout,
    /// Группа 2: копия кадра, сэмплер, глубина (только с чтением кадра).
    scene_layout: Option<wgpu::BindGroupLayout>,
    /// Конвейеры: [без кадра, с кадром].
    pipelines: [Option<wgpu::RenderPipeline>; 2],
    draws: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    /// Копия кадра до воды и её группа привязок (под размер и поколение глубины).
    scene: Option<SceneCopy>,
    sampler: Option<wgpu::Sampler>,
}

struct SceneCopy {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    size: [u32; 2],
    generation: u32,
}

impl WaterPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water fx layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        Self { uniform_layout, scene_layout: None, pipelines: [None, None], draws: Vec::new(), scene: None, sampler: None }
    }

    fn scene_layout(&mut self, device: &wgpu::Device) -> &wgpu::BindGroupLayout {
        self.scene_layout.get_or_insert_with(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("water scene layout"),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: true,
                        },
                        count: None,
                    },
                ],
            })
        })
    }

    /// Конвейер воды с эффектами (с кадром под водой или без), если его ещё нет.
    pub fn ensure_pipeline(&mut self, device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout, with_scene: bool) {
        let slot = with_scene as usize;
        if self.pipelines[slot].is_some() {
            return;
        }
        let part = if with_scene { include_str!("../water_scene.wgsl") } else { include_str!("../water_noscene.wgsl") };
        let source = [COMMON, part, include_str!("../water_fx.wgsl")].concat();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water fx shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        if with_scene {
            self.scene_layout(device);
        }
        let mut layouts: Vec<&wgpu::BindGroupLayout> = vec![globals_layout, &self.uniform_layout];
        if let (true, Some(scene)) = (with_scene, self.scene_layout.as_ref()) {
            layouts.push(scene);
        }
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water fx pipeline layout"),
            bind_group_layouts: &layouts,
            push_constant_ranges: &[],
        });
        self.pipelines[slot] = Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water fx pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..wgpu::MultisampleState::default() },
            multiview: None,
            cache: None,
        }));
    }

    /// Копия кадра под размер сцены; группа привязок — заново при смене размера или
    /// многовыборочной глубины. Возвращает вид копии (цель сведения первой части прохода).
    pub fn scene_copy(&mut self, device: &wgpu::Device, size: [u32; 2], depth: &wgpu::TextureView, generation: u32) -> &wgpu::TextureView {
        let stale = self.scene.as_ref().is_none_or(|s| s.size != size || s.generation != generation);
        if stale {
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("scene before water"),
                    size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            if self.sampler.is_none() {
                self.sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("water scene sampler"),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                }));
            }
            self.scene_layout(device);
            let (Some(sampler), Some(layout)) = (self.sampler.as_ref(), self.scene_layout.as_ref()) else {
                unreachable!("sampler and layout were just created");
            };
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("water scene"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
                ],
            });
            self.scene = Some(SceneCopy { view, bind, size, generation });
        }
        &self.scene.as_ref().expect("scene copy was just created").view
    }

    /// Копия кадра, если создана.
    pub fn scene_view(&self) -> Option<&wgpu::TextureView> {
        self.scene.as_ref().map(|s| &s.view)
    }

    /// Нарисовать воду с эффектами: `waters` — параметры, `with_scene` — с кадром под водой.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass,
        globals: &wgpu::BindGroup,
        waters: &[WaterFxUniform],
        with_scene: bool,
    ) -> u32 {
        let slot = with_scene as usize;
        let Some(pipeline) = self.pipelines[slot].as_ref() else {
            return 0;
        };
        let scene_bind = if with_scene {
            match self.scene.as_ref() {
                Some(scene) => Some(&scene.bind),
                None => return 0,
            }
        } else {
            None
        };
        while self.draws.len() < waters.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("water fx params"),
                size: std::mem::size_of::<WaterFxUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("water fx"),
                layout: &self.uniform_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.draws.push((buffer, bind));
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, globals, &[]);
        if let Some(scene) = scene_bind {
            pass.set_bind_group(2, scene, &[]);
        }
        for (water, (buffer, bind)) in waters.iter().zip(&self.draws) {
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(water));
            pass.set_bind_group(1, bind, &[]);
            pass.draw(0..6, 0..1);
        }
        waters.len() as u32
    }
}
