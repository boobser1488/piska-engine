//! Окно редактора: winit → egui → [`EditorGpu::frame`].
//!
//! Когда рисовать кадр, решает редактор:
//! * Play — непрерывно, без вертикальной синхронизации и без ограничения частоты
//!   (или с ограничением из Preferences);
//! * Edit — только когда что-то меняется: ввод, анимация egui, сборка, новые строки
//!   в консоли. В покое — несколько кадров в секунду, чтобы подсистемы (горячая
//!   перезагрузка плагинов) продолжали работать, а видеокарта отдыхала.

use crate::boot::{Boot, LoadingScreen, Progress, SplashSpec};
use crate::i18n::tr_s;
use crate::warmup::{GpuWarmup, UiPainter, WindowGpu};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_render::RenderError;
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
    /// Общая с потоком заставки (пока редактор собирается, окно рисует тот поток).
    surface: Arc<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    format: wgpu::TextureFormat,
    /// Устройство и очередь, пока у редактора ещё нет видеочасти ([`run_with`]); потом они
    /// принадлежат `gpu`.
    dev: Option<Dev>,
    /// Сцена и интерфейс редактора; пока редактор собирается за заставкой ([`run_with`]) — `None`.
    gpu: Option<EditorGpu>,
    /// Рисует заставку, пока редактора ещё нет.
    painter: Option<UiPainter>,
    egui: egui_winit::State,
    /// Режимы показа кадра, которые умеет поверхность.
    present_modes: Vec<wgpu::PresentMode>,
}

/// Устройство wgpu и его очередь (в wgpu 23 они не клонируются: владеет кто-то один).
struct Dev {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gfx {
    fn device(&self) -> &wgpu::Device {
        match (&self.gpu, &self.dev) {
            (Some(gpu), _) => gpu.device(),
            (None, Some(dev)) => &dev.device,
            (None, None) => unreachable!("the window has either the editor GPU or the bare device"),
        }
    }
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
    /// Выравнивание времени кадра под период монитора.
    frame_time: engine_render::frame_time::FrameTimeFilter,
    /// Частота монитора известна (`min_interval` — её период, а не запасное значение).
    refresh_known: bool,
    /// Окно уже показано (после первого кадра в цветах темы).
    shown: bool,
    /// Каталог открытого проекта: Piska Hub может попросить показать окно (instance.rs).
    project: Option<std::path::PathBuf>,
    /// Когда последний раз смотрели просьбу хаба.
    focus_checked: Instant,
    /// Ровная подача кадров (`engine_render::frame_time::FramePacer`): срок начала
    /// следующего кадра и идёт ли подача сейчас (кадры один за другим).
    pacer: engine_render::frame_time::FramePacer,
    pace_next: Instant,
    pacing: bool,
    /// Замер подачи кадров без человека (`PISKA_FORCE_REPAINT=<кадров>`): рисовать кадры
    /// подряд и выйти, когда счётчик дойдёт до нуля.
    forced_frames: Option<u32>,
    /// Где курсор системы сейчас (пиксели окна).
    cursor_at: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Обзор правой кнопкой в виде Scene: курсор спрятан; здесь — куда вернуть его после.
    look_cursor: Option<Option<winit::dpi::PhysicalPosition<f64>>>,
    /// Запуск с заставкой ([`run_with`]): редактор собирается за окном. `None` — редактор
    /// уже готов ([`run`]).
    boot: Option<Boot<Editor>>,
    /// Устройство wgpu, которое готовится в фоне с начала запуска.
    warmup: Option<GpuWarmup>,
}

/// Размер окна, пока редактор ещё не собран (конфигурация известна только после сборки).
const BOOT_SIZE: (u32, u32) = (1440, 860);

impl Runner {
    fn create_gfx(&mut self, event_loop: &ActiveEventLoop) -> Result<Gfx, RenderError> {
        let config = self.editor.config();
        // Окно создаётся невидимым и показывается после первого кадра в цветах темы:
        // ни белого прямоугольника системы, ни кадра без шрифтов и цветов.
        let attributes = Window::default_attributes()
            .with_title(self.editor.window_title())
            .with_visible(false)
            .with_inner_size(LogicalSize::new(config.width, config.height));
        // Значок окна — значок движка.
        let attributes = engine_render::with_window_icon(attributes, None);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| RenderError::Window(e.to_string()))?,
        );
        // Системные окна «Открыть» / «Сохранить» / «Папка» встают над редактором.
        crate::window_kit::set_file_dialog_owner(&window);

        // Устройство уже готово или почти готово: его готовил поток с начала запуска.
        let WindowGpu { surface, gpu } = self.warmup.take().unwrap_or_else(GpuWarmup::start).finish(window.clone())?;

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

        // Редактор, собираемый за заставкой, получает свою видеочасть позже (там компилируются
        // шейдеры сцены); заставку рисует лёгкий рисовальщик интерфейса.
        let (dev, editor_gpu, painter) = if self.boot.is_some() {
            let painter = UiPainter::new(&gpu.device, format);
            (Some(Dev { device: gpu.device, queue: gpu.queue }), None, Some(painter))
        } else {
            (None, Some(EditorGpu::new(gpu.device, gpu.queue, format)), None)
        };
        Ok(Gfx {
            window,
            surface: Arc::new(surface),
            config,
            format,
            dev,
            gpu: editor_gpu,
            painter,
            egui,
            present_modes: caps.present_modes,
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.boot.is_some() {
            self.boot_frame(event_loop);
            return;
        }
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let now = Instant::now();
        // Ограничиваем dt: после паузы или перетаскивания окна игра не «прыгает».
        let raw_dt = (now - self.last_frame).as_secs_f32();
        let dt = raw_dt.min(0.25);
        self.last_frame = now;
        // С vsync кадры на экране идут ровно через период монитора, а измеренное время
        // гуляет: игру и камеру двигаем на выровненное (см. `engine_render::frame_time`).
        let synced = matches!(gfx.config.present_mode, wgpu::PresentMode::AutoVsync | wgpu::PresentMode::Fifo)
            && !(self.editor.mode() == crate::Mode::Playing && self.editor.preferences().play_fps_cap > 0);
        let dt = self.frame_time.filter(dt, Some(self.min_interval.as_secs_f32()).filter(|_| synced && self.refresh_known));
        // Кадры подряд (полёт и обзор камерой, перетаскивание, режим Play) начинаются раз в
        // период монитора по своим часам: «рисуй сразу, монитор притормозит» даёт кадры
        // пачками, и движение идёт рывками (см. `FramePacer`).
        let paced = synced && self.refresh_known && engine_render::frame_time::pacing_enabled();
        self.pace_next = if paced { self.pacer.frame_started(now, self.min_interval) } else { now };
        engine_render::frame_time::trace_frame(raw_dt, dt);
        if let Some(left) = self.forced_frames.as_mut() {
            gfx.egui.egui_ctx().request_repaint();
            *left = left.saturating_sub(1);
            if *left == 0 {
                event_loop.exit();
            }
        }

        // Вертикальную синхронизацию включают и выключают в Preferences.
        let wanted = present_mode(&self.editor, &gfx.present_modes);
        if wanted != gfx.config.present_mode {
            gfx.config.present_mode = wanted;
            gfx.surface.configure(gfx.device(), &gfx.config);
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
                let Some(editor_gpu) = gfx.gpu.as_mut() else {
                    return;
                };
                let output = editor_gpu.frame(
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
                // Первый кадр, нарисованный уже с темой и шрифтами (`Editor::themed`), — окно
                // можно показать.
                if !self.shown && self.editor.themed() {
                    gfx.window.set_visible(true);
                    self.shown = true;
                    // Перезапуск после Build: прежний процесс ждёт этого кадра, чтобы
                    // закрыть своё окно (rust_scripts.rs, `window_shown`).
                    self.editor.window_shown();
                }
                self.next_frame = schedule(&self.editor, now, output.repaint_delay, self.min_interval);
                // «Сразу» при ровной подаче значит «в начале следующего периода».
                self.pacing = paced && self.next_frame <= self.pace_next;
                if self.pacing {
                    self.next_frame = self.pace_next;
                }
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(gfx.device(), &gfx.config);
                self.next_frame = now;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                self.error = Some(RenderError::Device(tr_s("out of GPU memory").to_string()));
                event_loop.exit();
            }
            Err(other) => log::warn!("skipping a frame: {other}"),
        }

        // Захват курсора для игры (ui.rs, `sync_cursor_capture`): запереть и спрятать курсор
        // системы или вернуть его. Где запереть нельзя (Windows), курсор держится в окне и
        // каждый кадр возвращается в середину вида Game — щелчки не уходят в другие окна.
        if let Some(capture) = self.editor.take_cursor_request() {
            apply_cursor_capture(&mut self.editor, &gfx.window, capture);
        }
        if self.editor.game_cursor_captured() {
            let center = self.editor.game_view_center();
            let scale = gfx.window.scale_factor() as f32 * self.editor.preferences().ui_scale.clamp(0.5, 3.0);
            let _ = gfx
                .window
                .set_cursor_position(winit::dpi::PhysicalPosition::new(f64::from(center.x * scale), f64::from(center.y * scale)));
        }
        // Обзор правой кнопкой в виде Scene (view3d.rs): курсор не нужен и только мешает —
        // прячем его, а после возвращаем туда, где кнопку нажали.
        let looking = self.editor.scene_looking();
        if looking && self.look_cursor.is_none() {
            self.look_cursor = Some(self.cursor_at);
            gfx.window.set_cursor_visible(false);
        } else if !looking {
            if let Some(back) = self.look_cursor.take() {
                if let Some(position) = back {
                    let _ = gfx.window.set_cursor_position(position);
                }
                gfx.window.set_cursor_visible(true);
            }
        }
        // Хаб открывает уже открытый проект: поднять это окно вместо второго редактора.
        if self.focus_checked.elapsed() > Duration::from_millis(250) {
            self.focus_checked = Instant::now();
            if self.project.as_deref().is_some_and(crate::instance::take_focus_request) {
                gfx.window.set_minimized(false);
                gfx.window.focus_window();
            }
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
            // Окно убирается сейчас, пока цикл событий жив. На Windows winit уничтожает окно
            // сообщением, которое разбирает цикл событий; после выхода из цикла процесс
            // ещё ждёт перезапущенный редактор (`run`), сообщения никто не разбирает — и
            // замёрзшее старое окно оставалось на экране «вторым окном движка».
            if let Some(gfx) = self.gfx.take() {
                gfx.window.set_visible(false);
                drop(gfx);
            }
            event_loop.exit();
        }
    }
}

impl Runner {
    /// Кадр запуска с заставкой: пока редактор не собран, окно рисует заставку (знак, имя,
    /// настоящий шаг, бегущая полоса), при сбое — экран ошибки. Окно показывается после первого
    /// полного кадра в цветах темы; сразу после него собирается редактор (поток окна занят
    /// этим, заставку в это время рисует поток-помощник).
    fn boot_frame(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(gfx), Some(boot)) = (self.gfx.as_mut(), self.boot.as_mut()) else {
            return;
        };
        let now = Instant::now();
        match gfx.surface.get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let raw_input = gfx.egui.take_egui_input(&gfx.window);
                let ctx = gfx.egui.egui_ctx().clone();
                let mut full = ctx.run(raw_input, |ctx| boot.splash_frame(ctx, None));
                let delay = full.viewport_output.get(&egui::ViewportId::ROOT).map_or(Duration::ZERO, |v| v.repaint_delay);
                gfx.egui.handle_platform_output(&gfx.window, std::mem::take(&mut full.platform_output));
                if let (Some(painter), Some(dev)) = (gfx.painter.as_mut(), gfx.dev.as_ref()) {
                    painter.paint(&dev.device, &dev.queue, &ctx, full, &view, [gfx.config.width, gfx.config.height]);
                }
                gfx.window.pre_present_notify();
                frame.present();
                if !self.shown && boot.themed() {
                    gfx.window.set_visible(true);
                    self.shown = true;
                }
                self.next_frame = now + delay.min(Duration::from_millis(250));
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(gfx.device(), &gfx.config);
                self.next_frame = now;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                self.error = Some(RenderError::Device(tr_s("out of GPU memory").to_string()));
                event_loop.exit();
                return;
            }
            Err(other) => log::warn!("skipping a frame: {other}"),
        }
        if boot.quit_requested() {
            event_loop.exit();
            return;
        }
        // Заставка на экране — теперь можно надолго занять поток окна.
        if self.shown && boot.should_build() {
            self.build_editor();
        }
    }

    /// Собрать редактор и его видеочасть, пока поток-помощник рисует заставку.
    fn build_editor(&mut self) {
        let (Some(gfx), Some(boot)) = (self.gfx.as_mut(), self.boot.as_mut()) else {
            return;
        };
        let Some((work, progress)) = boot.begin_local() else {
            return;
        };
        let (Some(spec), Some(dev), Some(mut painter)) = (boot.splash_spec(), gfx.dev.as_ref(), gfx.painter.take()) else {
            boot.finish_local(Err("the window has no graphics device".to_string()));
            return;
        };
        let (window, surface, ctx) = (gfx.window.clone(), gfx.surface.clone(), gfx.egui.egui_ctx().clone());
        // Пока этот поток читает ассеты и сцену, поток-помощник рисует заставку в ту же
        // поверхность тем же контекстом egui (устройство берёт взаймы: оно `Sync`).
        let stop = AtomicBool::new(false);
        let (built, returned) = std::thread::scope(|scope| {
            let helper = scope.spawn(|| splash_loop(&spec, &window, &surface, &ctx, &dev.device, &dev.queue, &mut painter, &stop));
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&progress)));
            stop.store(true, Ordering::Relaxed);
            (built, helper.join())
        });
        if returned.is_err() {
            // Поток заставки упал: рисовальщик пересоздаётся при следующем кадре.
            log::warn!("the splash thread stopped unexpectedly");
        }
        gfx.painter = Some(painter);
        let built = built.unwrap_or_else(|payload| {
            let text = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Err(format!("the editor crashed while loading: {text}"))
        });
        let editor = match built {
            Ok(editor) => editor,
            Err(message) => {
                boot.finish_local(Err(message));
                return;
            }
        };
        // Видеочасть редактора (сцена, шейдеры) принимает устройство во владение: заставка
        // перед этим рисуется ещё раз, уже с этим шагом.
        progress.set_step(tr_s("Starting the renderer…"));
        let ctx = gfx.egui.egui_ctx().clone();
        let raw_input = gfx.egui.take_egui_input(&gfx.window);
        let full = ctx.run(raw_input, |ctx| boot.splash_frame(ctx, None));
        if let (Ok(frame), Some(dev), Some(painter)) = (gfx.surface.get_current_texture(), gfx.dev.as_ref(), gfx.painter.as_mut()) {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            painter.paint(&dev.device, &dev.queue, &ctx, full, &view, [gfx.config.width, gfx.config.height]);
            frame.present();
        }
        let Some(dev) = gfx.dev.take() else {
            boot.finish_local(Err("the window has no graphics device".to_string()));
            return;
        };
        let format = gfx.format;
        // Редактор продолжает рисовать тем же контекстом egui, значит и тем же рисовальщиком,
        // что заставка: атлас шрифтов уже в нём (см. `EditorGpu::with_ui`).
        let ui = gfx.painter.take().map(UiPainter::into_renderer);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match ui {
            Some(ui) => EditorGpu::with_ui(dev.device, dev.queue, ui),
            None => EditorGpu::new(dev.device, dev.queue, format),
        }));
        match made {
            Ok(gpu) => {
                gfx.gpu = Some(gpu);
                boot.finish_local(Ok(editor));
            }
            Err(_) => boot.finish_local(Err("the renderer could not be started".to_string())),
        }
        if boot.state() == crate::boot::BootState::Ready {
            self.hand_over();
        }
    }

    /// Редактор собран: заставка запуска уступает место редактору (у него своя заставка с тем
    /// же видом, пока рисуется первая сцена).
    fn hand_over(&mut self) {
        let Some(mut boot) = self.boot.take() else {
            return;
        };
        let Some(editor) = boot.take_value() else {
            self.boot = Some(boot);
            return;
        };
        self.editor = editor;
        self.saved_prefs = prepare_editor(&mut self.editor);
        self.project = register_instance(&self.editor);
        self.title = String::new();
        if let Some(gfx) = self.gfx.as_mut() {
            let config = self.editor.config();
            if (config.width, config.height) != BOOT_SIZE {
                let _ = gfx.window.request_inner_size(LogicalSize::new(config.width, config.height));
            }
            gfx.egui.egui_ctx().set_zoom_factor(self.editor.preferences().ui_scale.clamp(0.5, 3.0));
            gfx.painter = None;
            gfx.window.request_redraw();
        }
        self.last_frame = Instant::now();
        self.next_frame = self.last_frame;
        if self.shown {
            self.editor.window_shown();
        }
    }
}

/// Тело потока-помощника: рисует заставку из этого потока, пока поток окна занят сборкой
/// редактора. Окно к этому моменту уже показано.
#[allow(clippy::too_many_arguments)]
fn splash_loop(
    spec: &SplashSpec,
    window: &Window,
    surface: &wgpu::Surface<'static>,
    ctx: &egui::Context,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &mut UiPainter,
    stop: &AtomicBool,
) {
    let mut screen = LoadingScreen::default();
    let started = Instant::now();
    let base_time = ctx.input(|i| i.time);
    while !stop.load(Ordering::Relaxed) {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let scale = window.scale_factor() as f32;
        let points = scale * ctx.zoom_factor();
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.width as f32, size.height as f32) / points)),
            time: Some(base_time + started.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            ..egui::RawInput::default()
        };
        raw.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo { native_pixels_per_point: Some(scale), ..egui::ViewportInfo::default() });
        let full = ctx.run(raw, |ctx| {
            spec.frame(ctx, &mut screen);
        });
        match surface.get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                painter.paint(device, queue, ctx, full, &view, [size.width, size.height]);
                frame.present();
            }
            Err(_) => std::thread::sleep(Duration::from_millis(16)),
        }
    }
}

/// Запереть (`capture`) или вернуть курсор системы. Не вышло запереть — игре отказ:
/// `Input::set_cursor_capture_state(false)` и просьба снимается (иначе повтор каждый кадр).
fn apply_cursor_capture(editor: &mut Editor, window: &Window, capture: bool) {
    use winit::window::CursorGrabMode;
    if capture {
        let grabbed = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
        match grabbed {
            Ok(()) => window.set_cursor_visible(false),
            Err(error) => {
                log::warn!("cursor capture is not available here: {error}");
                let input = &mut editor.app.world.input;
                input.set_cursor_capture_state(false);
                input.set_cursor_captured(false);
            }
        }
    } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
    }
}

/// Какой режим показа нужен сейчас: с синхронизацией или без.
fn present_mode(editor: &Editor, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    // Вне игры кадры всегда идут в такт монитору: редактору не нужна частота выше, а
    // таймер системы (им приходилось отмерять кадры без vsync) на Windows грубый — камера
    // вида Scene двигалась рывками. В режиме Play действует настройка: без vsync виден
    // настоящий FPS игры.
    if editor.preferences().vsync || editor.mode() != crate::Mode::Playing {
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
    // в покое — не чаще IDLE_REPAINT. Непрерывное движение (полёт камеры, перетаскивание,
    // анимация) — кадр сразу: темп задаёт монитор (vsync), а не таймер.
    if egui_delay.is_zero() {
        return started;
    }
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
                    self.refresh_known = true;
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
            // Ввод — кадр как можно скорее. С vsync частоту ограничивает сам монитор;
            // без него (режим Play с выключенным vsync) — не чаще частоты монитора.
            let vsync = matches!(gfx.config.present_mode, wgpu::PresentMode::AutoVsync | wgpu::PresentMode::Fifo);
            // С vsync — в начале следующего периода (после простоя этот срок уже прошёл, и
            // кадр рисуется сразу).
            let soonest = if vsync { self.pace_next } else { self.last_frame + self.min_interval };
            if soonest < self.next_frame {
                self.next_frame = soonest;
                self.pacing = vsync;
            }
        }
        match event {
            // Закрытие окна — тоже действие: при несохранённой сцене появится вопрос.
            WindowEvent::CloseRequested => {
                if self.boot.is_some() {
                    // Редактора ещё нет: спрашивать о несохранённом нечего.
                    event_loop.exit();
                } else {
                    self.editor.apply(Action::Quit);
                    self.next_frame = Instant::now();
                }
            }
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(gfx.device(), &gfx.config);
                self.next_frame = Instant::now();
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor_at = Some(position),
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    /// Сырое движение мыши — игре, пока курсор захвачен (взгляд в играх от первого лица).
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        if let winit::event::DeviceEvent::MouseMotion { delta } = event {
            self.editor.add_raw_mouse_motion(delta.0 as f32, delta.1 as f32);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Невидимое окно не получает RedrawRequested: пока оно не показано, кадры
        // рисуются прямо отсюда.
        if self.gfx.is_some() && !self.shown {
            self.frame(event_loop);
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        let Some(gfx) = &self.gfx else {
            return;
        };
        let now = Instant::now();
        if now >= self.next_frame {
            gfx.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.pacing && self.next_frame - now <= Duration::from_millis(3) {
            // Ровная подача: последние миллисекунды до срока — точное ожидание (таймер
            // системы на столько может ошибиться).
            engine_render::frame_time::wait_until(self.next_frame);
            gfx.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.pacing {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame - Duration::from_millis(2)));
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
pub fn run(editor: Editor) -> Result<Editor, EditorError> {
    run_loop(Start::Ready(editor))
}

/// То же, что [`run`], но редактор собирается уже за окном: сначала на экране заставка (знак,
/// имя, что происходит сейчас, бегущая полоса), потом `build` читает ассеты и сцену, а
/// редактор заменяет заставку. Не успел `build` — окно всё равно не белое и не пустое; не
/// удался — экран ошибки с текстом и кнопками «Close» / «Copy details», а не зависшее окно.
///
/// `build` выполняется в потоке окна (приложение игры не `Send`), поэтому заставку в это время
/// рисует поток-помощник. `Err` из `build` и паника в нём показываются на экране ошибки, а
/// из `run_with` возвращается тот же текст в [`EditorError::Other`].
///
/// ```no_run
/// use engine_core::prelude::*;
/// use engine_editor::{Editor, EditorConfig};
///
/// engine_editor::run_with(|| {
///     let mut app = App::new();
///     engine_render::register_components(&mut app.world.registry);
///     Editor::new(app, EditorConfig::titled("My Game"))
/// })
/// .unwrap();
/// ```
pub fn run_with(build: impl FnOnce() -> Result<Editor, EditorError> + 'static) -> Result<Editor, EditorError> {
    run_with_progress(move |_| build())
}

/// [`run_with`], где `build` сообщает ход: `progress.set_step("Scanning assets…")` — текст
/// на заставке, `progress.set_fraction(Some(0.4))` — доля на её полосе.
pub fn run_with_progress(build: impl FnOnce(&Progress) -> Result<Editor, EditorError> + 'static) -> Result<Editor, EditorError> {
    run_loop(Start::Deferred(Box::new(move |progress| build(progress).map_err(|e| e.to_string()))))
}

/// Запуск редактора с заставкой, как его ведёт [`run_with`], но без окна: тесты и снимки гоняют
/// кадры `boot.frame(ctx, |ctx, editor| { editor.ui(ctx); None })` и видят то же, что окно.
pub fn boot_editor(build: impl FnOnce(&Progress) -> Result<Editor, EditorError> + 'static) -> Boot<Editor> {
    boot_with(Box::new(move |progress| build(progress).map_err(|e| e.to_string())))
}

fn boot_with(work: Box<dyn FnOnce(&Progress) -> Result<Editor, String>>) -> Boot<Editor> {
    // Заставка растворяется сразу, как редактор собран: дальше у редактора своя заставка с тем
    // же видом (первый кадр сцены, миниатюры).
    Boot::start_local(
        tr_s("Piska Engine"),
        env!("CARGO_PKG_VERSION"),
        crate::window_kit::paint_engine_mark,
        tr_s("Loading the project…"),
        work,
    )
    .hand_over_when_ready()
}

/// Откуда берётся редактор.
enum Start {
    /// Уже собран ([`run`]).
    Ready(Editor),
    /// Соберётся за заставкой ([`run_with`]).
    Deferred(Box<dyn FnOnce(&Progress) -> Result<Editor, String>>),
}

/// Настройки в редактор: файл Preferences, масштаб из конфигурации, `ENGINE_VSYNC`. Возвращает
/// то, что считается уже записанным на диск.
fn prepare_editor(editor: &mut Editor) -> Preferences {
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
    editor.enable_splash();
    saved_prefs
}

/// Один редактор на проект: метка для Piska Hub (instance.rs).
fn register_instance(editor: &Editor) -> Option<std::path::PathBuf> {
    let project = editor.assets_root().map(|root| crate::instance::project_dir_of_assets(&root));
    if let Some(project) = &project {
        if let Some(other) = crate::instance::running_editor(project) {
            log::warn!(
                "this project is already open in another editor (process {other}); saving the same scene from both \n                 overwrites one with the other"
            );
        }
        if let Err(err) = crate::instance::acquire(project, std::process::id()) {
            log::warn!("cannot mark the project as open for Piska Hub: {err}");
        }
    }
    project
}

fn run_loop(start: Start) -> Result<Editor, EditorError> {
    // Видеокарта готовится с первой секунды, пока создаются цикл событий и окно.
    let warmup = GpuWarmup::start();
    // Прежние версии программы, отодвинутые при пересборке с перезапуском.
    crate::rust_scripts::remove_stale_executables();
    let (mut editor, boot) = match start {
        Start::Ready(editor) => (editor, None),
        Start::Deferred(work) => {
            // Пока настоящего редактора нет, роль редактора играет пустой: он держит настройки
            // окна и ничего не рисует (кадры рисует заставка).
            let placeholder = Editor::new(engine_core::app::App::new(), crate::EditorConfig::default())?;
            (placeholder, Some(boot_with(work)))
        }
    };
    let (saved_prefs, project) = if boot.is_none() {
        (prepare_editor(&mut editor), register_instance(&editor))
    } else {
        // Масштаб и синхронизация нужны уже окну заставки; метка проекта — когда он откроется.
        let prefs = Preferences::load();
        editor.set_preferences(prefs.clone());
        (prefs, None)
    };

    let event_loop = EventLoop::new().map_err(|e| RenderError::Window(e.to_string()))?;
    // Системные окна выбора файлов и папок — только у настоящего окна (тесты их не открывают).
    engine_build::file_dialog::enable_native();
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
        frame_time: engine_render::frame_time::FrameTimeFilter::new(),
        refresh_known: false,
        shown: false,
        project,
        focus_checked: Instant::now(),
        pacer: engine_render::frame_time::FramePacer::new(),
        pace_next: Instant::now(),
        pacing: false,
        forced_frames: std::env::var("PISKA_FORCE_REPAINT").ok().and_then(|v| v.parse().ok()),
        cursor_at: None,
        look_cursor: None,
        boot,
        warmup: Some(warmup),
    };
    event_loop
        .run_app(&mut runner)
        .map_err(|e| RenderError::Window(e.to_string()))?;
    if let Some(err) = runner.error.take() {
        return Err(err.into());
    }
    if let Some(boot) = &runner.boot {
        // Сбой запуска (экран ошибки закрыли) или окно закрыли, пока редактор собирался.
        return Err(EditorError::Other(boot.error().map_or_else(|| "the window was closed before the editor finished loading".to_string(), str::to_string)));
    }
    let project = runner.project.clone();
    // Build перезапустил редактор новой версией программы: окно и игра этого процесса
    // освобождаются, а сам процесс ждёт новый и выходит с его кодом. Ждать нужно:
    // `cargo run` на Windows держит дочерние процессы в job-объекте и закрывает их
    // всех, когда завершается сам.
    if let Some(project) = &project {
        // После перезапуска метка уже у нового процесса — тогда она остаётся.
        crate::instance::release(project, std::process::id());
    }
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
