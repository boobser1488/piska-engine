//! GPU-часть экранного слоя (данные — в `crate::overlay`): свой конвейер без глубины
//! и MSAA, рисует прямо в цель кадра после пост-обработки.

use std::collections::HashMap;

use engine_assets::AssetDatabase;
use engine_core::prelude::*;

use super::Renderer;
use crate::overlay::{Overlay, OverlayImage, OverlayTexture, OverlayVertex};

/// См. `Screen` в overlay.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ScreenUniform {
    target: [f32; 2],
    scale: [f32; 2],
    srgb_target: f32,
    pad: [f32; 3],
}

struct GpuImage {
    version: u64,
    size: [u32; 2],
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

pub(super) struct OverlayPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    vertices: wgpu::Buffer,
    vertex_capacity: usize,
    indices: wgpu::Buffer,
    index_capacity: usize,
    images: HashMap<u32, GpuImage>,
}

impl OverlayPass {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../overlay.wgsl").into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay screen layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay screen"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay screen"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay pipeline layout"),
            bind_group_layouts: &[&uniform_layout, texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<OverlayVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4,
                        3 => Float32x4,
                        4 => Float32x4,
                        5 => Float32x4
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let vertex_capacity = 4 * 1024;
        let index_capacity = 6 * 1024;
        Self {
            pipeline,
            uniform,
            uniform_bind_group,
            sampler,
            vertices: vertex_buffer(device, vertex_capacity),
            vertex_capacity,
            indices: index_buffer(device, index_capacity),
            index_capacity,
            images: HashMap::new(),
        }
    }

    /// Загрузить картинку на GPU или обновить изменившуюся часть.
    fn sync_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        image: &OverlayImage,
    ) {
        let size = [image.width.max(1), image.height.max(1)];
        if image.rgba.len() != (size[0] * size[1] * 4) as usize {
            return;
        }
        let cached = self.images.get(&image.id);
        if cached.is_some_and(|c| c.version == image.version && c.size == size) {
            return;
        }
        let partial = match (cached, image.dirty) {
            (Some(c), Some(rect)) if c.size == size && c.version + 1 == image.version => Some(rect),
            _ => None,
        };
        if cached.is_none_or(|c| c.size != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("overlay image"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("overlay image"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.images.insert(
                image.id,
                GpuImage {
                    version: image.version,
                    size,
                    texture,
                    bind_group,
                },
            );
        }
        let Some(gpu) = self.images.get_mut(&image.id) else {
            return;
        };
        gpu.version = image.version;
        let [x, y, w, h] = match partial {
            Some([x, y, w, h]) if x + w <= size[0] && y + h <= size[1] && w > 0 && h > 0 => [x, y, w, h],
            Some(_) => return,
            None => [0, 0, size[0], size[1]],
        };
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &gpu.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::ImageDataLayout {
                offset: ((y * size[0] + x) * 4) as u64,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
}

impl Renderer {
    /// Нарисовать [`Overlay`] из `world.resources` поверх `target`. Возвращает число
    /// вызовов отрисовки.
    pub(super) fn draw_overlay(
        &mut self,
        world: &World,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> u32 {
        let Some(overlay) = world.resources.get::<Overlay>() else {
            return 0;
        };
        if overlay.is_empty() || width == 0 || height == 0 {
            return 0;
        }
        let assets = world.resources.get::<AssetDatabase>();
        for image in &overlay.images {
            self.overlay
                .sync_image(&self.device, &self.queue, &self.texture_layout, image);
        }
        for batch in &overlay.batches {
            if let OverlayTexture::Asset(guid) = batch.texture {
                self.ensure_texture(guid, assets);
            }
        }

        let pass_data = &mut self.overlay;
        if overlay.vertices.len() > pass_data.vertex_capacity {
            pass_data.vertex_capacity = overlay.vertices.len().next_power_of_two();
            pass_data.vertices = vertex_buffer(&self.device, pass_data.vertex_capacity);
        }
        if overlay.indices.len() > pass_data.index_capacity {
            pass_data.index_capacity = overlay.indices.len().next_power_of_two();
            pass_data.indices = index_buffer(&self.device, pass_data.index_capacity);
        }
        self.queue
            .write_buffer(&pass_data.vertices, 0, bytemuck::cast_slice(&overlay.vertices));
        self.queue
            .write_buffer(&pass_data.indices, 0, bytemuck::cast_slice(&overlay.indices));

        let (w, h) = (width as f32, height as f32);
        let source = if overlay.size.x > 0.0 && overlay.size.y > 0.0 {
            overlay.size
        } else {
            Vec2::new(w, h)
        };
        let scale = Vec2::new(w / source.x, h / source.y);
        self.queue.write_buffer(
            &pass_data.uniform,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                target: [w, h],
                scale: scale.to_array(),
                srgb_target: if self.format.is_srgb() { 1.0 } else { 0.0 },
                pad: [0.0; 3],
            }),
        );

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pass_data.pipeline);
        pass.set_bind_group(0, &pass_data.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, pass_data.vertices.slice(..));
        pass.set_index_buffer(pass_data.indices.slice(..), wgpu::IndexFormat::Uint32);

        let mut draw_calls = 0;
        for batch in &overlay.batches {
            let bind_group = match batch.texture {
                OverlayTexture::Image(id) => match pass_data.images.get(&id) {
                    Some(image) => &image.bind_group,
                    None => continue,
                },
                OverlayTexture::Asset(guid) => self
                    .textures
                    .get(&guid)
                    .map_or(&self.missing.bind_group, |t| &t.bind_group),
            };
            let scissor = match batch.clip {
                Some([x, y, cw, ch]) => {
                    let x0 = (x * scale.x).round().clamp(0.0, w) as u32;
                    let y0 = (y * scale.y).round().clamp(0.0, h) as u32;
                    let x1 = ((x + cw) * scale.x).round().clamp(0.0, w) as u32;
                    let y1 = ((y + ch) * scale.y).round().clamp(0.0, h) as u32;
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    [x0, y0, x1 - x0, y1 - y0]
                }
                None => [0, 0, width, height],
            };
            pass.set_scissor_rect(scissor[0], scissor[1], scissor[2], scissor[3]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw_indexed(batch.indices.clone(), 0, 0..1);
            draw_calls += 1;
        }
        draw_calls
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay vertices"),
        size: (capacity * std::mem::size_of::<OverlayVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn index_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay indices"),
        size: (capacity * std::mem::size_of::<u32>()) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
