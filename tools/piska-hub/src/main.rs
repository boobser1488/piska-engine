//! Окно Piska Hub: `cargo run -p piska-hub` (или `-- --open <каталог проекта>`).
//!
//! `ENGINE_FRAMES=N` закрывает окно после N кадров (проверки без человека).

// Подсистема «окно» (без консоли и в отладочной сборке) — build.rs: engine_build::windows_app().

use engine_editor::kit::i18n::{tr_s};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use piska_hub::capture::HubGpu;
use piska_hub::{Hub, HubOptions};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Как часто проверять сборку и запущенные программы, когда ничего не происходит
/// (проверка дешёвая и без перерисовки).
const POLL: Duration = Duration::from_millis(200);
/// «Никогда» для следующего кадра в покое.
const IDLE: Duration = Duration::from_secs(3600);

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    painter: HubGpu,
    egui: egui_winit::State,
}

struct Runner {
    hub: Hub,
    gfx: Option<Gfx>,
    error: Option<String>,
    frames: u64,
    max_frames: Option<u64>,
    visible: bool,
    /// Следующий кадр (если раньше ничего не случится).
    next_frame: Instant,
    /// Окно уже показано: оно создаётся невидимым и показывается после первого кадра в
    /// цветах темы (с заставкой) — ни белого, ни серого кадра системы.
    shown: bool,
}

impl Runner {
    fn create_gfx(&self, event_loop: &ActiveEventLoop) -> Result<Gfx, String> {
        let attributes = Window::default_attributes()
            .with_title(tr_s("Piska Hub"))
            .with_visible(false)
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(960.0, 600.0));
        // Значок движка, зашитый в программу: и в заголовке, и на панели задач / в Alt-Tab.
        // (Раньше он читался с диска из папки движка и ставился только в заголовок: без
        // найденной папки значка не было вовсе, а панель задач показывала пустой.)
        let attributes = engine_render::with_window_icon(attributes, None);
        let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
        // Системные окна «Папка» / «Открыть» встают над хабом и блокируют его.
        engine_editor::kit::window::set_file_dialog_owner(&window);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let gpu = pollster::block_on(engine_render::Gpu::request(&instance, Some(&surface))).map_err(|e| e.to_string())?;
        // egui рисует в гамма-пространстве: поверхность не-sRGB.
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or("the window surface reports no formats")?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&gpu.device, &config);
        let max_texture = gpu.device.limits().max_texture_dimension_2d as usize;
        let egui = egui_winit::State::new(
            egui::Context::default(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture),
        );
        Ok(Gfx {
            window,
            surface,
            config,
            painter: HubGpu::new(gpu.device, gpu.queue, format),
            egui,
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let now = Instant::now();
        let raw = gfx.egui.take_egui_input(&gfx.window);
        let ctx = gfx.egui.egui_ctx().clone();
        let mut full = ctx.run(raw, |ctx| self.hub.ui(ctx));
        let repaint = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::ZERO, |v| v.repaint_delay);
        gfx.egui
            .handle_platform_output(&gfx.window, std::mem::take(&mut full.platform_output));
        match gfx.surface.get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                gfx.painter.paint(&ctx, full, &view, [gfx.config.width, gfx.config.height]);
                gfx.window.pre_present_notify();
                frame.present();
                // Первый кадр уже в цветах темы (с заставкой) — окно можно показать.
                if !self.shown && self.hub.themed() {
                    self.shown = true;
                    gfx.window.set_visible(self.visible);
                }
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.painter.device, &gfx.config);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                self.error = Some(tr_s("out of GPU memory").to_string());
                event_loop.exit();
            }
            Err(other) => log::warn!("skipping a frame: {other}"),
        }
        // В покое egui не просит кадров (`Duration::MAX`): следующий — только по вводу,
        // по просьбе egui или когда хаб сообщит об изменении.
        self.next_frame = now.checked_add(repaint).unwrap_or(now + IDLE);
        self.frames += 1;
        if self.max_frames.is_some_and(|max| self.frames >= max) {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match self.create_gfx(event_loop) {
            Ok(gfx) => {
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
        let response = gfx.egui.on_window_event(&gfx.window, &event);
        if response.repaint {
            self.next_frame = Instant::now();
        }
        match event {
            WindowEvent::CloseRequested => {
                if self.hub.close_requested() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.painter.device, &gfx.config);
                self.next_frame = Instant::now();
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Невидимое окно не получает RedrawRequested: первые кадры рисуются отсюда.
        if self.gfx.is_some() && !self.shown {
            self.frame(event_loop);
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        // Сборка, запущенные программы, просьбы редактора — и когда окно спрятано.
        // Перерисовка — только если что-то поменялось.
        let changed = self.hub.update();
        let request = self.hub.window_request();
        if request.quit {
            event_loop.exit();
            return;
        }
        let Some(gfx) = &self.gfx else {
            return;
        };
        if request.visible != self.visible && self.shown {
            self.visible = request.visible;
            gfx.window.set_visible(request.visible);
            if request.visible {
                self.next_frame = Instant::now();
            }
        }
        if request.focus && self.visible {
            gfx.window.set_minimized(false);
            gfx.window.focus_window();
            self.next_frame = Instant::now();
        }
        let now = Instant::now();
        if self.visible && (changed || now >= self.next_frame) {
            gfx.window.request_redraw();
            self.next_frame = now + IDLE;
        }
        let wake = if self.visible { self.next_frame.min(now + POLL) } else { now + POLL };
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

/// Лог в stderr (уровень — `RUST_LOG=debug|info|warn`, по умолчанию info).
struct StderrLog(log::LevelFilter);

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.0 && metadata.target().starts_with("piska_hub")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[hub {}] {}", record.level().as_str().to_lowercase(), record.args());
        }
    }

    fn flush(&self) {}
}

fn main() {
    // Своей консоли нет: подключиться к терминалу или писать журнал в файл.
    engine_build::host::gui_startup("piska-hub", engine_editor::kit::i18n::brand::CONFIG_DIR_NAME);
    let level = match std::env::var("RUST_LOG").unwrap_or_default().as_str() {
        "debug" | "trace" => log::LevelFilter::Debug,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    };
    let logger: &'static StderrLog = Box::leak(Box::new(StderrLog(level)));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }

    let mut options = HubOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--open" => options.open = args.next().map(PathBuf::from),
            "--help" | "-h" => {
                let ext = engine_editor::kit::i18n::brand::PROJECT_EXTENSION;
                println!(
                    "piska-hub [--open <project folder>] [<project folder or .{ext} file>...]\n\nThe {} start screen: projects, new project, import, samples.\nA folder or a .{ext} file opens the Import Project page.",
                    engine_editor::kit::i18n::brand::ENGINE_NAME
                );
                return;
            }
            other if !other.starts_with('-') => options.import.push(PathBuf::from(other)),
            other => {
                eprintln!("piska-hub: unknown argument `{other}` (try --help)");
                std::process::exit(2);
            }
        }
    }
    // Двойной щелчок по файлу проекта при уже запущенном хабе: просьба уходит ему, второе окно не открывается.
    if let Some(dir) = piska_hub::paths::config_dir() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if !options.import.is_empty() && options.open.is_none() && piska_hub::link::other_hub_alive(&dir, now) {
            let sent = options
                .import
                .iter()
                .all(|path| piska_hub::link::send_request(&dir, &piska_hub::link::Request::Import(path.clone())).is_ok());
            if sent {
                return;
            }
        }
    }
    let max_frames = std::env::var("ENGINE_FRAMES").ok().and_then(|v| v.parse().ok());
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            eprintln!("piska-hub: cannot open a window: {err}");
            std::process::exit(1);
        }
    };
    let mut runner = Runner {
        hub: Hub::new(options),
        gfx: None,
        error: None,
        frames: 0,
        max_frames,
        visible: true,
        next_frame: Instant::now(),
        shown: false,
    };
    runner.hub.enable_splash();
    // Системные окна выбора папки и файла — только у настоящего окна (не в тестах).
    engine_build::file_dialog::enable_native();
    if let Err(err) = event_loop.run_app(&mut runner) {
        eprintln!("piska-hub: {err}");
        std::process::exit(1);
    }
    runner.hub.shutdown();
    if let Some(err) = runner.error {
        eprintln!("piska-hub: {err}");
        std::process::exit(1);
    }
}
