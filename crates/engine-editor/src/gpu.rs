//! GPU-часть кадра редактора: сцена → текстура вьюпорта, затем интерфейс egui → цель.
//!
//! Один и тот же код рисует и в окно ([`crate::run`]), и в картинку ([`capture`]).

use std::time::Instant;

use egui_wgpu::ScreenDescriptor;
use engine_render::{read_texture, Gpu, Renderer};

use crate::profiler::FrameTimings;
use crate::state::Editor;
use crate::EditorError;

/// Формат текстуры вьюпорта: его требует egui для «родных» текстур wgpu.
const VIEWPORT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub(crate) struct ViewportTarget {
    pub view: wgpu::TextureView,
    size: [u32; 2],
    pub id: egui::TextureId,
}

/// Что кадр сообщает окну: вывод egui для платформы и когда рисовать следующий кадр.
pub(crate) struct FrameOutput {
    pub platform: egui::PlatformOutput,
    /// Через сколько egui просит следующий кадр (`Duration::MAX` — пока нет ввода, не нужно).
    pub repaint_delay: std::time::Duration,
}

pub(crate) struct EditorGpu {
    /// Рендерер движка; владеет устройством и очередью.
    scene: Renderer,
    egui: egui_wgpu::Renderer,
    /// Текстура окна Scene.
    target: Option<ViewportTarget>,
    /// Текстура окна Game (Scene и Game можно поставить рядом).
    game_target: Option<ViewportTarget>,
    /// Миниатюры и большой вид моделей окна Project.
    previews: crate::preview::PreviewGpu,
}

impl EditorGpu {
    /// `output_format` — формат цели, в которую рисуется интерфейс (окно или картинка).
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, output_format: wgpu::TextureFormat) -> Self {
        let egui = egui_wgpu::Renderer::new(&device, output_format, None, 1, false);
        Self {
            scene: Renderer::new(device, queue, VIEWPORT_FORMAT),
            egui,
            target: None,
            game_target: None,
            previews: crate::preview::PreviewGpu::default(),
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        self.scene.device()
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.scene.queue()
    }

    /// Текстура вьюпорта нужного размера. При смене размера текстура создаётся
    /// заново, но её идентификатор в egui остаётся прежним.
    fn viewport_target(&mut self, size: [u32; 2]) -> egui::TextureId {
        Self::ensure_target(&self.scene, &mut self.egui, &mut self.target, size)
    }

    /// То же для окна Game.
    fn game_target(&mut self, size: [u32; 2]) -> egui::TextureId {
        Self::ensure_target(&self.scene, &mut self.egui, &mut self.game_target, size)
    }

    fn ensure_target(
        scene: &Renderer,
        egui: &mut egui_wgpu::Renderer,
        slot: &mut Option<ViewportTarget>,
        size: [u32; 2],
    ) -> egui::TextureId {
        ensure_target(scene, egui, slot, size)
    }
}

/// Текстура-цель нужного размера в слоте; при смене размера создаётся заново, а её
/// идентификатор в egui остаётся прежним. Так делают вьюпорты и предпросмотр моделей.
pub(crate) fn ensure_target(
    scene: &Renderer,
    egui: &mut egui_wgpu::Renderer,
    slot: &mut Option<ViewportTarget>,
    size: [u32; 2],
) -> egui::TextureId {
    if let Some(target) = slot.as_ref() {
        if target.size == size {
            return target.id;
        }
    }
    let device = scene.device();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("editor viewport"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: VIEWPORT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let id = match slot.take() {
        Some(old) => {
            egui.update_egui_texture_from_wgpu_texture(device, &view, wgpu::FilterMode::Linear, old.id);
            old.id
        }
        None => egui.register_native_texture(device, &view, wgpu::FilterMode::Linear),
    };
    *slot = Some(ViewportTarget { view, size, id });
    id
}

impl EditorGpu {
    /// Один кадр редактора: ввод → игра → интерфейс → сцена в текстуру → всё в `output`.
    ///
    /// Сцена рисуется *после* интерфейса: камера, которую только что сдвинули, и
    /// объект, который только что перетащили гизмо, попадают в этот же кадр.
    /// `wait_ms` — сколько окно ждало свободный кадр (для профайлера).
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        editor: &mut Editor,
        ctx: &egui::Context,
        raw_input: egui::RawInput,
        dt: f32,
        wait_ms: f32,
        output: &wgpu::TextureView,
        output_size: [u32; 2],
    ) -> FrameOutput {
        let started = Instant::now();
        // Окно снова в фокусе — вероятно, файлы меняли в другой программе (как в Unity).
        if raw_input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::WindowFocused(true)))
            && editor.assets_root().is_some()
        {
            editor.apply(crate::state::Action::RefreshAssets);
        }
        if std::mem::take(&mut editor.gpu_assets_stale) {
            self.scene.reload_assets();
        }
        // Материалы, сохранённые инспектором: рендерер перечитает их файлы.
        for guid in editor.material_editor.stale.drain(..) {
            self.scene.reload_material(guid);
        }
        // Картинки предпросмотра рисует эта GPU-часть: чужие id egui не показываются.
        editor.previews.attach_gpu(self.previews.generation());
        editor.forward_game_input(ctx, &raw_input);
        editor.advance(dt);
        let update_ms = started.elapsed().as_secs_f32() * 1000.0;

        // Интерфейс. Идентификатор текстуры вьюпорта известен заранее и не меняется.
        let ui_started = Instant::now();
        let texture = self.viewport_target(editor.viewport_pixels());
        editor.set_viewport_texture(texture);
        let texture = self.game_target(editor.game_view_pixels());
        editor.set_game_texture(texture);
        let full = ctx.run(raw_input, |ctx| editor.ui(ctx));
        let ui_ms = ui_started.elapsed().as_secs_f32() * 1000.0;
        let tessellate_started = Instant::now();
        let primitives = ctx.tessellate(full.shapes, full.pixels_per_point);
        let tessellate_ms = tessellate_started.elapsed().as_secs_f32() * 1000.0;

        // Сцена — уже с размером окна и камерой этого кадра. Рисуются только видимые
        // окна Scene и Game: скрытая вкладка (или открытый Code) не стоит ничего.
        let render_started = Instant::now();
        let mut stats = editor.profiler.stats;
        let [scene_visible, game_visible] = editor.visible_views();
        let mut rendered = [None, None];
        if scene_visible {
            let size = editor.viewport_pixels();
            self.viewport_target(size);
            let view = editor.camera_view_for(&mut self.scene, size, crate::state::ViewTab::Scene);
            if let Some(target) = &self.target {
                rendered[0] = Some(self.scene.render_with(&editor.app.world, &target.view, size[0], size[1], &view));
            }
        }
        if game_visible {
            let size = editor.game_view_pixels();
            self.game_target(size);
            let view = editor.camera_view_for(&mut self.scene, size, crate::state::ViewTab::Game);
            if let Some(target) = &self.game_target {
                rendered[1] = Some(self.scene.render_with(&editor.app.world, &target.view, size[0], size[1], &view));
            }
        }
        // В профайлер — статистика вида, выбранного последним (если он нарисован).
        let preferred = usize::from(editor.view_tab() == crate::state::ViewTab::Game);
        if let Some(view_stats) = rendered[preferred].or(rendered[1 - preferred]) {
            stats = view_stats;
        }
        // Предпросмотр моделей окна Project: только то, что интерфейс запросил в этом кадре.
        self.previews.service(editor, &mut self.scene, &mut self.egui, ctx);
        // Рельеф догружается в фоне: в режиме Edit кадры по требованию — просим ещё.
        if self.scene.terrain_busy() {
            ctx.request_repaint();
        }
        let render_ms = render_started.elapsed().as_secs_f32() * 1000.0;
        let submit_started = Instant::now();

        let screen = ScreenDescriptor {
            size_in_pixels: output_size,
            pixels_per_point: full.pixels_per_point,
        };
        let (device, queue) = (self.scene.device(), self.scene.queue());
        for (id, delta) in &full.textures_delta.set {
            self.egui.update_texture(device, queue, *id, delta);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("editor ui"),
        });
        let extra = self
            .egui
            .update_buffers(device, queue, &mut encoder, &primitives, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("editor ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: output,
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
            self.egui.render(&mut pass, &primitives, &screen);
        }
        queue.submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        for id in &full.textures_delta.free {
            self.egui.free_texture(id);
        }
        let submit_ms = submit_started.elapsed().as_secs_f32() * 1000.0;

        editor.frame_rendered(
            stats,
            FrameTimings {
                frame_ms: dt * 1000.0,
                update_ms,
                render_ms,
                ui_ms,
                tessellate_ms,
                submit_ms,
                wait_ms,
            },
        );
        let repaint_delay = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(std::time::Duration::ZERO, |viewport| viewport.repaint_delay);
        FrameOutput {
            platform: full.platform_output,
            repaint_delay,
        }
    }
}

/// Нарисовать редактор в картинку без окна — для скриншот-тестов и CI.
///
/// Прогоняет `frames` кадров (минимум два: в первом раскладка только узнаёт размер
/// вьюпорта) с шагом 1/60 с и возвращает последний. Масштаб интерфейса берётся из
/// [`Editor::preferences`].
pub fn capture(
    editor: &mut Editor,
    width: u32,
    height: u32,
    frames: u32,
) -> Result<image::RgbaImage, EditorError> {
    capture_with_input(editor, width, height, frames, |_, _| Vec::new())
}

/// То же, что [`capture`], но перед каждым кадром `input(номер кадра, редактор)`
/// возвращает события egui (координаты — в точках интерфейса): так снимаются открытые
/// меню, всплывающие окна и наведённые элементы.
pub fn capture_with_input(
    editor: &mut Editor,
    width: u32,
    height: u32,
    frames: u32,
    mut input: impl FnMut(u32, &mut Editor) -> Vec<egui::Event>,
) -> Result<image::RgbaImage, EditorError> {
    // Интерфейс egui рассчитан на цель в гамма-пространстве.
    const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let gpu = pollster::block_on(Gpu::request(&instance, None))?;
    let mut editor_gpu = EditorGpu::new(gpu.device, gpu.queue, OUTPUT_FORMAT);
    // Снимок: рельеф строится сразу, а не по кусочку за кадр.
    editor_gpu.scene.set_terrain_blocking(true);

    let output = editor_gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("editor capture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&wgpu::TextureViewDescriptor::default());

    let ctx = egui::Context::default();
    let zoom = editor.preferences().ui_scale.clamp(0.5, 3.0);
    ctx.set_zoom_factor(zoom);
    let dt = 1.0 / 60.0;
    for frame in 0..frames.max(2) {
        let events = input(frame, editor);
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32) / zoom,
            )),
            time: Some(frame as f64 * dt as f64),
            predicted_dt: dt,
            events,
            ..egui::RawInput::default()
        };
        editor_gpu.frame(editor, &ctx, raw_input, dt, 0.0, &view, [width, height]);
    }
    Ok(read_texture(
        editor_gpu.device(),
        editor_gpu.queue(),
        &output,
        width,
        height,
    )?)
}
