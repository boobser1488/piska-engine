//! Окно редактора: winit → egui → [`EditorGpu::frame`].
//!
//! Когда рисовать кадр, решает редактор:
//! * Play — непрерывно, без вертикальной синхронизации и без ограничения частоты
//!   (или с ограничением из Preferences);
//! * Edit — только когда что-то меняется: ввод, анимация egui, сборка, новые строки
//!   в консоли. В покое — несколько кадров в секунду, чтобы подсистемы (горячая
//!   перезагрузка плагинов) продолжали работать, а видеокарта отдыхала.

use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_render::{Gpu, RenderError};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::gpu::EditorGpu;
use crate::prefs::Preferences;
use crate::state::{Action, Editor};
use crate::ui::IDLE_REPAINT;
use crate::EditorError;

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: EditorGpu,
    egui: egui_winit::State,
    /// Режимы показа кадра, которые умеет поверхность.
    present_modes: Vec<wgpu::PresentMode>,
}

struct Runner {
    editor: Editor,
    gfx: Option<Gfx>,
    error: Option<RenderError>,
    /// Начало прошлого кадра.
    last_frame: Instant,
    /// Когда нарисовать следующий кадр (если ничего не случится раньше).
    next_frame: Instant,
    frames: u64,
    title: String,
    /// Настройки, которые уже записаны на диск.
    saved_prefs: Preferences,
    /// Самый частый кадр при работе мышью в режиме Edit — частота монитора.
    min_interval: Duration,
}

impl Runner {
    fn create_gfx(&self, event_loop: &ActiveEventLoop) -> Result<Gfx, RenderError> {
        let config = self.editor.config();
        let attributes = Window::default_attributes()
            .with_title(self.editor.window_title())
            .with_inner_size(LogicalSize::new(config.width, config.height));
        // Значок окна — значок движка.
        let attributes = engine_render::with_window_icon(attributes, None);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| RenderError::Window(e.to_string()))?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| RenderError::Window(e.to_string()))?;
        let gpu = pollster::block_on(Gpu::request(&instance, Some(&surface)))?;

        // egui рисует в гамма-пространстве, поэтому поверхность берём не-sRGB.
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| RenderError::Window("surface reports no formats".to_string()))?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: present_mode(&self.editor, &caps.present_modes),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&gpu.device, &config);
        log::info!(
            "editor renderer: {} ({:?}), present mode {:?}",
            gpu.adapter.get_info().name,
            gpu.adapter.get_info().backend,
            config.present_mode
        );

        let max_texture_side = gpu.device.limits().max_texture_dimension_2d as usize;
        let context = egui::Context::default();
        context.set_zoom_factor(self.editor.preferences().ui_scale.clamp(0.5, 3.0));
        let egui = egui_winit::State::new(
            context,
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );

        Ok(Gfx {
            window,
            surface,
            config,
            gpu: EditorGpu::new(gpu.device, gpu.queue, format),
            egui,
            present_modes: caps.present_modes,
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let now = Instant::now();
        // Ограничиваем dt: после паузы или перетаскивания окна игра не «прыгает».
        let dt = (now - self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;

        // Вертикальную синхронизацию включают и выключают в Preferences.
        let wanted = present_mode(&self.editor, &gfx.present_modes);
        if wanted != gfx.config.present_mode {
            gfx.config.present_mode = wanted;
            gfx.surface.configure(gfx.gpu.device(), &gfx.config);
        }

        let acquire = Instant::now();
        match gfx.surface.get_current_texture() {
            Ok(frame) => {
                let wait_ms = acquire.elapsed().as_secs_f32() * 1000.0;
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let raw_input = gfx.egui.take_egui_input(&gfx.window);
                let ctx = gfx.egui.egui_ctx().clone();
                let output = gfx.gpu.frame(
                    &mut self.editor,
                    &ctx,
                    raw_input,
                    dt,
                    wait_ms,
                    &view,
                    [gfx.config.width, gfx.config.height],
                );
                gfx.egui.handle_platform_output(&gfx.window, output.platform);
                gfx.window.pre_present_notify();
                frame.present();
                self.next_frame = schedule(&self.editor, now, output.repaint_delay, self.min_interval);
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(gfx.gpu.device(), &gfx.config);
                self.next_frame = now;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                self.error = Some(RenderError::Device("out of GPU memory".to_string()));
                event_loop.exit();
            }
            Err(other) => log::warn!("skipping a frame: {other}"),
        }

        let title = self.editor.window_title();
        if title != self.title {
            gfx.window.set_title(&title);
            self.title = title;
        }
        if *self.editor.preferences() != self.saved_prefs {
            self.saved_prefs = self.editor.preferences().clone();
            self.saved_prefs.save();
        }

        self.frames += 1;
        let limit_reached = self
            .editor
            .config()
            .max_frames
            .is_some_and(|max| self.frames >= max);
        if self.editor.should_quit() || limit_reached {
            event_loop.exit();
        }
    }
}

/// Какой режим показа нужен сейчас: с синхронизацией или без.
fn present_mode(editor: &Editor, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if editor.preferences().vsync {
        return wgpu::PresentMode::AutoVsync;
    }
    // Mailbox не рвёт кадр и не ждёт монитор; Immediate — запасной вариант.
    [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
        .into_iter()
        .find(|mode| supported.contains(mode))
        .unwrap_or(wgpu::PresentMode::AutoVsync)
}

/// Когда рисовать следующий кадр, если до тех пор ничего не случится.
fn schedule(editor: &Editor, started: Instant, egui_delay: Duration, min_interval: Duration) -> Instant {
    if editor.mode() == crate::Mode::Playing {
        let cap = editor.preferences().play_fps_cap;
        return if cap > 0 {
            started + Duration::from_secs_f64(1.0 / f64::from(cap))
        } else {
            started
        };
    }
    if !editor.preferences().redraw_on_demand {
        return started + min_interval;
    }
    // Режим Edit: egui сам скажет, когда нужен кадр (анимация, тултип, курсор);
    // в покое — не чаще IDLE_REPAINT.
    started + egui_delay.max(min_interval).min(IDLE_REPAINT)
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match self.create_gfx(event_loop) {
            Ok(gfx) => {
                if let Some(millihertz) = gfx.window.current_monitor().and_then(|m| m.refresh_rate_millihertz()) {
                    self.min_interval = Duration::from_secs_f64(1000.0 / f64::from(millihertz.max(30_000)));
                }
                self.last_frame = Instant::now();
                self.next_frame = self.last_frame;
                gfx.window.request_redraw();
                self.gfx = Some(gfx);
            }
            Err(err) => {
                self.error = Some(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        // egui видит все события; игра в режиме Play получает ввод уже из него.
        let response = gfx.egui.on_window_event(&gfx.window, &event);
        if response.repaint {
            // Ввод — кадр как можно скорее, но не чаще частоты монитора.
            let soonest = self.last_frame + self.min_interval;
            self.next_frame = self.next_frame.min(soonest);
        }
        match event {
            // Закрытие окна — тоже действие: при несохранённой сцене появится вопрос.
            WindowEvent::CloseRequested => {
                self.editor.apply(Action::Quit);
                self.next_frame = Instant::now();
            }
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(gfx.gpu.device(), &gfx.config);
                self.next_frame = Instant::now();
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = &self.gfx else {
            return;
        };
        if Instant::now() >= self.next_frame {
            gfx.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        }
    }
}

/// Открыть окно редактора и работать, пока его не закроют.
///
/// Настройки (масштаб, акцент, синхронизация, ограничение кадров) читаются из файла
/// Preferences и сохраняются при изменении. `ENGINE_VSYNC=1` / `0` переопределяет
/// синхронизацию — для замеров.
pub fn run(mut editor: Editor) -> Result<Editor, EditorError> {
    // Прежние версии программы, отодвинутые при пересборке с перезапуском.
    crate::rust_scripts::remove_stale_executables();
    let mut prefs = Preferences::load();
    if let Some(scale) = editor.config().ui_scale {
        prefs.ui_scale = scale;
    }
    if let Ok(value) = std::env::var("ENGINE_VSYNC") {
        prefs.vsync = value != "0";
    }
    // Переопределение из окружения не записывается в файл настроек.
    let saved_prefs = prefs.clone();
    editor.set_preferences(prefs);

    let event_loop = EventLoop::new().map_err(|e| RenderError::Window(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut runner = Runner {
        title: editor.window_title(),
        editor,
        gfx: None,
        error: None,
        last_frame: Instant::now(),
        next_frame: Instant::now(),
        frames: 0,
        saved_prefs,
        min_interval: Duration::from_secs_f64(1.0 / 120.0),
    };
    event_loop
        .run_app(&mut runner)
        .map_err(|e| RenderError::Window(e.to_string()))?;
    if let Some(err) = runner.error.take() {
        return Err(err.into());
    }
    // Build перезапустил редактор новой версией программы: окно и игра этого процесса
    // освобождаются, а сам процесс ждёт новый и выходит с его кодом. Ждать нужно:
    // `cargo run` на Windows держит дочерние процессы в job-объекте и закрывает их
    // всех, когда завершается сам.
    if let Some(mut successor) = runner.editor.take_successor() {
        drop(runner);
        let code = match successor.wait() {
            Ok(status) => status.code().unwrap_or(0),
            Err(err) => {
                log::error!("lost track of the rebuilt editor: {err}");
                0
            }
        };
        std::process::exit(code);
    }
    let timings = runner.editor.frame_timings();
    log::info!(
        "editor closed after {} frames; last seconds: {:.2} ms/frame ({:.0} FPS), of which work {:.2} ms: \
         game {:.2}, scene render {:.2}, ui {:.2}, tessellate {:.2}, submit {:.2}, waiting for the GPU {:.2}",
        runner.frames,
        timings.frame_ms,
        if timings.frame_ms > 0.0 { 1000.0 / timings.frame_ms } else { 0.0 },
        timings.work_ms(),
        timings.update_ms,
        timings.render_ms,
        timings.ui_ms,
        timings.tessellate_ms,
        timings.submit_ms,
        timings.wait_ms
    );
    Ok(runner.editor)
}
