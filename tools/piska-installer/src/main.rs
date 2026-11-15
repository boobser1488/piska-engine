//! Окно установщика: `piska-installer` (справка — `--help`).
//!
//! На Windows программа — с подсистемой «GUI» всегда (и в отладочной сборке): окно консоли не
//! появляется никогда; к консоли терминала программа подключается только для `--help`,
//! `--version`, ошибки аргументов и `--unattended`. Окно создаётся невидимым и показывается
//! после первого кадра в цветах темы (с заставкой) — без белой вспышки.
//! `ENGINE_FRAMES=N` закрывает окно после N кадров (проверки без человека).

#![windows_subsystem = "windows"]

use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_editor::kit::i18n::{brand, trf, Language};
use piska_installer::capture::Painter;
use piska_installer::cli::{self, Args};
use engine_editor::kit::boot::Boot;
use engine_editor::GpuWarmup;
use piska_installer::instance::Instance;
use piska_installer::{Action, Installer, InstallerOptions};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Как часто забирать события скриптов, когда ничего не происходит.
const POLL: Duration = Duration::from_millis(100);
const IDLE: Duration = Duration::from_secs(3600);

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    egui: egui_winit::State,
}

struct Runner {
    /// Запуск установщика: заставка, потом мастер (собирается в фоне, `startup.rs`).
    boot: Boot<Installer>,
    /// Устройство wgpu готовится в фоне с первой секунды.
    warmup: Option<GpuWarmup>,
    /// Сохранённый размер окна уже применён (он известен, когда установщик собран).
    size_applied: bool,
    instance: Option<Instance>,
    gfx: Option<Gfx>,
    error: Option<String>,
    frames: u64,
    max_frames: Option<u64>,
    shown: bool,
    next_frame: Instant,
}

impl Runner {
    fn create_gfx(&mut self, event_loop: &ActiveEventLoop) -> Result<Gfx, String> {
        // Сохранённый размер окна известен только собранному установщику: он применяется позже.
        let attributes = Window::default_attributes()
            .with_title(trf!("{engine} Setup", engine = brand::ENGINE_NAME))
            .with_visible(false)
            .with_inner_size(LogicalSize::new(1060.0, 700.0))
            .with_min_inner_size(LogicalSize::new(900.0, 600.0));
        let attributes = engine_render::with_window_icon(attributes, None);
        let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
        // Устройство готово или почти готово: его готовил отдельный поток.
        let engine_editor::WindowGpu { surface, gpu } =
            self.warmup.take().unwrap_or_else(GpuWarmup::start).finish(window.clone()).map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&gpu.adapter);
        // egui рисует в гамма-пространстве: поверхность не-sRGB.
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or_else(|| caps.formats.first().copied()).ok_or("the window surface reports no formats")?;
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
        let egui = egui_winit::State::new(egui::Context::default(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, Some(max_texture));
        Ok(Gfx { window, surface, config, painter: Painter::new(gpu.device, gpu.queue, format), egui })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let now = Instant::now();
        let raw = gfx.egui.take_egui_input(&gfx.window);
        let ctx = gfx.egui.egui_ctx().clone();
        let boot = &mut self.boot;
        let mut full = ctx.run(raw, |ctx| piska_installer::startup::frame(boot, ctx));
        let repaint = full.viewport_output.get(&egui::ViewportId::ROOT).map_or(Duration::ZERO, |v| v.repaint_delay);
        gfx.egui.handle_platform_output(&gfx.window, std::mem::take(&mut full.platform_output));
        match gfx.surface.get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                gfx.painter.paint(&ctx, full, &view, [gfx.config.width, gfx.config.height]);
                gfx.window.pre_present_notify();
                frame.present();
                // Первый кадр, нарисованный с темой и шрифтами (первый только ставит шрифты).
                if !self.shown && self.boot.themed() {
                    gfx.window.set_visible(true);
                    self.shown = true;
                }
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => gfx.surface.configure(&gfx.painter.device, &gfx.config),
            Err(wgpu::SurfaceError::OutOfMemory) => {
                self.error = Some("out of GPU memory".into());
                event_loop.exit();
            }
            Err(other) => log::warn!("skipping a frame: {other}"),
        }
        self.next_frame = now.checked_add(repaint).unwrap_or(now + IDLE);
        self.frames += 1;
        if self.max_frames.is_some_and(|max| self.frames >= max) {
            event_loop.exit();
        }
        if self.boot.quit_requested() || self.boot.value().is_some_and(|i| i.quit) {
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
        if gfx.egui.on_window_event(&gfx.window, &event).repaint {
            self.next_frame = Instant::now();
        }
        match event {
            WindowEvent::CloseRequested => match self.boot.value_mut() {
                Some(installer) => {
                    installer.apply(Action::Close);
                    if installer.quit {
                        event_loop.exit();
                    } else {
                        self.next_frame = Instant::now();
                    }
                }
                // Установщик ещё собирается (или запуск не удался): спрашивать не о чем.
                None => event_loop.exit(),
            },
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.painter.device, &gfx.config);
                let logical = size.to_logical::<f32>(gfx.window.scale_factor());
                if let Some(installer) = self.boot.value_mut() {
                    installer.window_size = Some((logical.width, logical.height));
                }
                self.next_frame = Instant::now();
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Невидимое окно не получает RedrawRequested (Windows не шлёт WM_PAINT спрятанному
        // окну): пока окно не показано, кадры рисуются прямо отсюда — иначе первый кадр не
        // наступит никогда и окно так и останется невидимым.
        self.boot.poll();
        if self.gfx.is_some() && !self.shown {
            if let Some(installer) = self.boot.value_mut() {
                installer.update();
            }
            self.frame(event_loop);
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        // Установщик собран: сохранённый размер окна.
        if !self.size_applied {
            if let (Some(installer), Some(gfx)) = (self.boot.value(), self.gfx.as_ref()) {
                self.size_applied = true;
                if let Some((w, h)) = installer.window_size {
                    let _ = gfx.window.request_inner_size(LogicalSize::new(w.max(900.0), h.max(600.0)));
                }
            }
        }
        // Пока установщик собирается или заставка растворяется, кадры идут сами.
        let animating = self.boot.state() != engine_editor::kit::boot::BootState::Ready || self.boot.splash_visible();
        let changed = self.boot.value_mut().is_some_and(|installer| installer.update()) || animating;
        let focus = self.instance.as_mut().is_some_and(Instance::tick);
        let Some(gfx) = &self.gfx else {
            return;
        };
        if focus {
            gfx.window.set_minimized(false);
            gfx.window.focus_window();
        }
        let now = Instant::now();
        if changed || focus || now >= self.next_frame {
            gfx.window.request_redraw();
            self.next_frame = now + IDLE;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame.min(now + POLL)));
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse(&raw) {
        Ok(args) => args,
        Err(err) => {
            engine_build::host::gui_startup("piska-installer", brand::CONFIG_DIR_NAME);
            eprintln!("piska-installer: {err}");
            std::process::exit(cli::EXIT_BAD_ARGS);
        }
    };
    if args.help || args.version || args.unattended {
        // Только здесь — к консоли терминала (окна консоли у программы нет).
        engine_build::host::gui_startup("piska-installer", brand::CONFIG_DIR_NAME);
    }
    if args.help {
        print!("{}", cli::usage());
        return;
    }
    if args.version {
        println!("{} Setup {}", brand::ENGINE_NAME, env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.unattended {
        std::process::exit(cli::unattended(&args));
    }
    std::process::exit(run_window(args));
}

fn run_window(args: Args) -> i32 {
    let options = InstallerOptions {
        dry_run: args.dry_run,
        components: args.components.clone(),
        install_dir: args.install_dir.clone(),
        source: args.source.clone(),
        language: args.lang.as_deref().and_then(Language::from_code),
        uninstall: args.uninstall,
        prefer_pwsh: args.prefer_pwsh,
        ..InstallerOptions::default()
    };
    // Второй запуск — показать первый и выйти.
    let config_dir = piska_installer::settings::config_dir();
    let instance = match config_dir.as_deref().map(Instance::claim) {
        Some(Err(_)) => return cli::EXIT_OK,
        Some(Ok(instance)) => Some(instance),
        None => None,
    };
    // Видеокарта готовится с первой секунды, пока создаются цикл событий и окно.
    let warmup = GpuWarmup::start();
    let event_loop = match EventLoop::new() {
        Ok(e) => e,
        Err(err) => {
            engine_build::host::gui_startup("piska-installer", brand::CONFIG_DIR_NAME);
            eprintln!("piska-installer: cannot open a window: {err}");
            return cli::EXIT_CANNOT_START;
        }
    };
    let mut runner = Runner {
        boot: piska_installer::startup::start(options),
        warmup: Some(warmup),
        size_applied: false,
        instance,
        gfx: None,
        error: None,
        frames: 0,
        max_frames: std::env::var("ENGINE_FRAMES").ok().and_then(|v| v.parse().ok()),
        shown: false,
        next_frame: Instant::now(),
    };
    if let Err(err) = event_loop.run_app(&mut runner) {
        runner.error = Some(err.to_string());
    }
    if let Some(message) = runner.boot.error() {
        engine_build::host::gui_startup("piska-installer", brand::CONFIG_DIR_NAME);
        eprintln!("piska-installer: {message}");
        return cli::EXIT_CANNOT_START;
    }
    let Some(installer) = runner.boot.value_mut() else {
        // Окно закрыли, пока установщик собирался: ничего не начато.
        return cli::EXIT_OK;
    };
    installer.save_settings();
    let code = cli::exit_code_of(installer.run.as_ref().map(|r| &r.outcome));
    if let Some(err) = runner.error {
        engine_build::host::gui_startup("piska-installer", brand::CONFIG_DIR_NAME);
        eprintln!("piska-installer: {err}");
        return cli::EXIT_CANNOT_START;
    }
    if installer.backend_error.is_some() {
        return cli::EXIT_CANNOT_START;
    }
    code
}
