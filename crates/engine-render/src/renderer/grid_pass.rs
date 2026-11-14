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
    base: [f32; 4],
    origin_x: [f32; 4],
    origin_y: [f32; 4],
    dir_0: [f32; 4],
    dir_x: [f32; 4],
    dir_y: [f32; 4],
}

/// Луч через пиксель с NDC (x, y): `base + origin_x·x + origin_y·y` и направление
/// `dir_0 + dir_x·x + dir_y·y`; `near`/`far` — границы параметра луча.
///
/// Обратная матрица вида для этого не годится: у перспективной камеры точка дальней
/// плоскости получается делением на разность близких чисел, и чем дальше камера от начала
/// координат, тем сильнее шумит направление (под острым углом к плоскости — до целой клетки
/// сетки). Направление берётся из строк самой матрицы: строка `w` — это «вперёд», строки
/// `x` и `y` — «вправо» и «вверх», умноженные на масштаб проекции; сдвиг камеры в них не
/// участвует.
struct Rays {
    base: glam::Vec3,
    origin_x: glam::Vec3,
    origin_y: glam::Vec3,
    dir_0: glam::Vec3,
    dir_x: glam::Vec3,
    dir_y: glam::Vec3,
    near: f32,
    far: f32,
}

fn rays(view_proj: glam::Mat4, position: glam::Vec3, ortho: bool) -> Rays {
    use glam::{DVec3, DVec4};
    let m = view_proj.as_dmat4();
    let inverse = m.inverse();
    let unproject = |x: f64, y: f64, z: f64| {
        let p = inverse * DVec4::new(x, y, z, 1.0);
        p.truncate() / p.w
    };
    let row = |i: usize| DVec3::new(m.x_axis[i], m.y_axis[i], m.z_axis[i]);
    if ortho {
        // Параллельные лучи: обратная матрица здесь обусловлена хорошо.
        // Обратная глубина: ближняя плоскость — z = 1, дальняя — z = 0.
        let (near_z, far_z) = (crate::depth::NEAR_DEPTH as f64, crate::depth::FAR_DEPTH as f64);
        let base = unproject(0.0, 0.0, near_z);
        return Rays {
            base: base.as_vec3(),
            origin_x: (unproject(1.0, 0.0, near_z) - base).as_vec3(),
            origin_y: (unproject(0.0, 1.0, near_z) - base).as_vec3(),
            dir_0: (unproject(0.0, 0.0, far_z) - base).as_vec3(),
            dir_x: glam::Vec3::ZERO,
            dir_y: glam::Vec3::ZERO,
            near: 0.0,
            far: 1.0,
        };
    }
    let eye = position.as_dvec3();
    let forward = row(3).normalize_or_zero();
    let (right, up) = (row(0), row(1));
    // Границы по глубине нужны только для отсечения, точность тут не важна.
    let depth = |z: f64| (unproject(0.0, 0.0, z) - eye).dot(forward);
    // Обратная глубина: ближняя плоскость — z = 1, дальняя — z = 0.
    let near = depth(crate::depth::NEAR_DEPTH as f64).max(0.0);
    let far = depth(crate::depth::FAR_DEPTH as f64);
    let far = if far.is_finite() && far > near { far } else { 1.0e9 };
    Rays {
        base: position,
        origin_x: glam::Vec3::ZERO,
        origin_y: glam::Vec3::ZERO,
        dir_0: forward.as_vec3(),
        dir_x: (right / right.length_squared().max(1e-30)).as_vec3(),
        dir_y: (up / up.length_squared().max(1e-30)).as_vec3(),
        near: near as f32,
        far: far as f32,
    }
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
                // Обратная глубина (depth.rs).
                depth_compare: wgpu::CompareFunction::GreaterEqual,
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
    pub(super) fn upload(&self, queue: &wgpu::Queue, grid: &SceneGrid, view_proj: glam::Mat4, position: glam::Vec3, ortho: bool) {
        let rays = rays(view_proj, position, ortho);
        let v4 = |v: glam::Vec3, w: f32| [v.x, v.y, v.z, w];
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
            base: v4(rays.base, rays.near),
            origin_x: v4(rays.origin_x, 0.0),
            origin_y: v4(rays.origin_y, 0.0),
            dir_0: v4(rays.dir_0, rays.far),
            dir_x: v4(rays.dir_x, 0.0),
            dir_y: v4(rays.dir_y, 0.0),
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
