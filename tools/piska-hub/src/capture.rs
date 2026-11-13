//! Хаб в картинку без окна (снимки экрана в тестах): тот же интерфейс, нарисованный
//! egui-wgpu в текстуру, затем прочитанный с видеокарты.

use egui_wgpu::ScreenDescriptor;
use engine_render::{read_texture, Gpu};

use crate::hub::Hub;

/// Отрисовщик хаба на видеокарте.
pub struct HubGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
}

impl HubGpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
        Self { device, queue, renderer }
    }

    /// Нарисовать вывод egui в `target`.
    pub fn paint(&mut self, ctx: &egui::Context, output: egui::FullOutput, target: &wgpu::TextureView, size: [u32; 2]) {
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: output.pixels_per_point,
        };
        for (id, delta) in &output.textures_delta.set {
            self.renderer.update_texture(&self.device, &self.queue, *id, delta);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hub ui") });
        let extra = self
            .renderer
            .update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("hub ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &primitives, &screen);
        }
        self.queue.submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}

/// Нарисовать хаб в картинку `width × height`: `frames` кадров (не меньше трёх — шрифты
/// и раскладка устанавливаются за первые кадры); перед каждым кадром `input(номер, хаб)`
/// даёт события egui (координаты в точках интерфейса).
pub fn capture(
    hub: &mut Hub,
    width: u32,
    height: u32,
    frames: u32,
    mut input: impl FnMut(u32, &mut Hub) -> Vec<egui::Event>,
) -> Result<image::RgbaImage, String> {
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let gpu = pollster::block_on(Gpu::request(&instance, None)).map_err(|e| e.to_string())?;
    let mut painter = HubGpu::new(gpu.device, gpu.queue, FORMAT);
    let output = painter.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hub capture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&wgpu::TextureViewDescriptor::default());
    let ctx = egui::Context::default();
    let dt = 1.0 / 60.0;
    for frame in 0..frames.max(3) {
        let events = input(frame, hub);
        hub.update();
        let zoom = hub.preferences().ui_scale.clamp(0.5, 3.0);
        ctx.set_zoom_factor(zoom);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32) / zoom,
            )),
            time: Some(f64::from(frame) * dt),
            predicted_dt: dt as f32,
            events,
            ..egui::RawInput::default()
        };
        let full = ctx.run(raw, |ctx| hub.ui(ctx));
        painter.paint(&ctx, full, &view, [width, height]);
    }
    read_texture(&painter.device, &painter.queue, &output, width, height).map_err(|e| e.to_string())
}
