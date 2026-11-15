//! Видеокарта за окном: устройство wgpu готовится в отдельном потоке с первой секунды
//! работы программы, пока создаются окно и интерфейс — так окно с заставкой появляется
//! раньше (поиск адаптера и открытие устройства на Windows занимают заметное время).
//!
//! Устройство запрашивается без поверхности: окна ещё нет. Когда окно создано, поверхность
//! берётся из того же `Instance`; если адаптер её не умеет (редкая связка из двух видеокарт),
//! устройство запрашивается заново уже с поверхностью — как делали раньше.

use std::sync::Arc;
use std::thread::JoinHandle;

use engine_render::{Gpu, RenderError};
use winit::window::Window;

/// Устройство, готовящееся в фоне.
pub struct GpuWarmup {
    handle: Option<JoinHandle<Result<(wgpu::Instance, Gpu), RenderError>>>,
}

/// Поверхность окна и устройство, которое её умеет.
pub struct WindowGpu {
    pub surface: wgpu::Surface<'static>,
    pub gpu: Gpu,
}

impl GpuWarmup {
    /// Начать готовить устройство. Звать первым делом в `main`, до создания цикла событий.
    pub fn start() -> Self {
        let handle = std::thread::Builder::new()
            .name("gpu-warmup".to_string())
            .spawn(|| {
                let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
                let gpu = pollster::block_on(Gpu::request(&instance, None))?;
                Ok((instance, gpu))
            })
            .ok();
        Self { handle }
    }

    /// Поверхность для `window` и устройство; ждёт фоновую подготовку, если она ещё идёт.
    pub fn finish(self, window: Arc<Window>) -> Result<WindowGpu, RenderError> {
        if let Some(Ok((instance, gpu))) = self.handle.map(JoinHandle::join).and_then(Result::ok).map(|r| r.map_err(|e| log::warn!("early GPU start failed: {e}"))) {
            if let Ok(surface) = instance.create_surface(window.clone()) {
                if !surface.get_capabilities(&gpu.adapter).formats.is_empty() {
                    return Ok(WindowGpu { surface, gpu });
                }
            }
        }
        // Запасной путь: устройство под эту поверхность.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window).map_err(|e| RenderError::Window(e.to_string()))?;
        let gpu = pollster::block_on(Gpu::request(&instance, Some(&surface)))?;
        Ok(WindowGpu { surface, gpu })
    }
}

/// Рисует вывод egui в текстуру окна: для заставки, пока редактор ещё собирается. Устройство
/// и очередь ему дают на каждый вызов (ими владеет другой): так заставку можно рисовать из
/// потока, который занял их взаймы.
pub struct UiPainter {
    renderer: egui_wgpu::Renderer,
}

impl UiPainter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self { renderer: egui_wgpu::Renderer::new(device, format, None, 1, false) }
    }

    /// Отдать рисовальщик вместе с уже загруженными текстурами (атлас шрифтов). Тот, кто
    /// дальше рисует тем же контекстом egui, обязан продолжать этим же рисовальщиком:
    /// контекст шлёт атлас целиком один раз, потом — только изменившиеся куски.
    pub fn into_renderer(self) -> egui_wgpu::Renderer {
        self.renderer
    }

    pub fn paint(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ctx: &egui::Context, full: egui::FullOutput, target: &wgpu::TextureView, size: [u32; 2]) {
        let primitives = ctx.tessellate(full.shapes, full.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: full.pixels_per_point };
        for (id, delta) in &full.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("splash ui") });
        let extra = self.renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("splash ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(crate::gpu::theme_clear()), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &primitives, &screen);
        }
        queue.submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        for id in &full.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
