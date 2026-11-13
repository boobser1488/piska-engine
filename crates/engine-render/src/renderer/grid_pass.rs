//! GPU-часть сетки вида (данные — в `crate::grid`): один draw call, полноэкранный
//! треугольник, глубина проверяется по MSAA-глубине сцены, но не пишется.

use super::{DEPTH_FORMAT, HDR_FORMAT, SAMPLES};
use crate::grid::{GridPlane, SceneGrid};

/// См. `Grid` в grid.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniform {
    params: [f32; 4],
    color: [f32; 4],
    axis_u: [f32; 4],
    axis_v: [f32; 4],
}

pub(super) struct GridPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl GridPass {
    pub(super) fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid shader"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("../common.wgsl"), include_str!("../grid.wgsl")).into(),
            ),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grid params"),
            size: std::mem::size_of::<GridUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid params"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid pipeline layout"),
            bind_group_layouts: &[globals_layout, &layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grid"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_grid"),
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
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..wgpu::MultisampleState::default()
            },
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            uniform,
            bind_group,
        }
    }

    /// Загрузить параметры кадра.
    pub(super) fn upload(&self, queue: &wgpu::Queue, grid: &SceneGrid, ortho: bool) {
        let (plane, axis_u, axis_v) = match grid.plane {
            // Линия x = 0 идёт вдоль Z — синяя; линия z = 0 идёт вдоль X — красная.
            GridPlane::Ground => (0.0, [0.28, 0.45, 1.0, 1.0], [1.0, 0.27, 0.27, 1.0]),
            // Линия x = 0 идёт вдоль Y — зелёная; линия y = 0 — красная.
            GridPlane::Wall => (1.0, [0.35, 0.85, 0.4, 1.0], [1.0, 0.27, 0.27, 1.0]),
        };
        let uniform = GridUniform {
            params: [plane, grid.height, grid.fade_distance.max(1.0), grid.opacity.clamp(0.0, 1.0)],
            color: [grid.color[0], grid.color[1], grid.color[2], if grid.axes { 2.2 } else { 0.0 }],
            axis_u,
            axis_v: [axis_v[0], axis_v[1], axis_v[2], if ortho { 1.0 } else { 0.0 }],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// Нарисовать сетку: один draw call.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, globals: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
