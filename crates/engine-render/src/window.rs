//! Оконный цикл на winit: события → `Input`, кадр → `App::tick` + `Renderer::render`.
//!
//! Один код для всех платформ:
//! * **десктоп** — окно ОС, GPU запрашивается синхронно (`pollster`);
//! * **браузер** — окно — это `<canvas id="piska-canvas">` (или новый canvas в `<body>`),
//!   GPU запрашивается асинхронно (блокировать поток страницы нельзя), WebGPU, а без
//!   него — WebGL2; размер холста следует за CSS-размером с учётом `devicePixelRatio`;
//! * **Android** — поверхность живёт между `resumed` и `suspended`: при сворачивании
//!   она уничтожается, а устройство, рендерер с текстурами и сам мир остаются.

use std::sync::Arc;

use engine_core::prelude::*;
use engine_core::settings::{FullscreenMode, PlayerSettings, WindowFocus};
// `std::time::Instant` в браузере паникует; `web_time` — тот же API на всех платформах.
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::renderer::{Gpu, RenderStats, Renderer};
use crate::RenderError;

/// `id` холста, к которому веб-сборка привязывает игру (если его нет — холст создаётся).
pub const WEB_CANVAS_ID: &str = "piska-canvas";

#[derive(Clone, Debug)]
pub struct WindowConfig {
    pub title: String,
    /// Размер окна в логических пикселях. В браузере не используется: размер холста
    /// задаёт страница (CSS).
    pub width: u32,
    pub height: u32,
    /// Синхронизация с частотой монитора. По умолчанию выключена: кадры идут так
    /// быстро, как успевает машина, и задержка ввода минимальна (возможны разрывы
    /// картинки). Включите, чтобы не грузить видеокарту впустую. Браузер всегда
    /// рисует с частотой монитора.
    pub vsync: bool,
    /// Закрыть окно после стольких кадров (для смоук-тестов).
    pub max_frames: Option<u64>,
    /// Значок окна — PNG (лучше 256×256). `None` — значок движка. Игра, у которой
    /// есть `assets/icon.png`, получает его автоматически (`engine::game_main!`).
    /// Не используется в браузере, на Android и macOS.
    pub icon: Option<Vec<u8>>,
    /// Можно ли менять размер окна мышью (десктоп).
    pub resizable: bool,
    /// Окно или полный экран без рамки при старте (десктоп; в браузере и на Android
    /// размер задаёт платформа).
    pub fullscreen: FullscreenMode,
    /// Не чаще стольких кадров в секунду; 0 — без ограничения. В браузере кадры
    /// идут с частотой монитора, ограничение не действует.
    pub fps_cap: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Piska Engine".to_string(),
            width: 1280,
            height: 720,
            vsync: false,
            max_frames: None,
            icon: None,
            resizable: true,
            fullscreen: FullscreenMode::Windowed,
            fps_cap: 0,
        }
    }
}

impl WindowConfig {
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Свой значок окна: байты PNG (например, `include_bytes!("../assets/icon.png")`).
    pub fn with_icon_png(mut self, png: impl Into<Vec<u8>>) -> Self {
        self.icon = Some(png.into());
        self
    }

    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn with_fullscreen(mut self, fullscreen: FullscreenMode) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Ограничение частоты кадров; 0 — без ограничения.
    pub fn with_fps_cap(mut self, fps_cap: u32) -> Self {
        self.fps_cap = fps_cap;
        self
    }

    pub fn with_vsync(mut self, vsync: bool) -> Self {
        self.vsync = vsync;
        self
    }

    /// Наложить раздел Player настроек проекта на конфиг из кода игры. Основа — код;
    /// поле настроек побеждает, только если его изменили (оно отличается от значения
    /// по умолчанию в [`PlayerSettings`]); заголовок — только непустой. Поэтому старый
    /// `project.settings` без раздела `player` оставляет окно таким, как его задал код.
    /// Значок (`PlayerSettings::icon`) здесь не читается — это делает `engine::run`
    /// через подключённую базу ассетов.
    pub fn with_player_settings(mut self, player: &PlayerSettings) -> Self {
        let default = PlayerSettings::default();
        if !player.title.trim().is_empty() {
            self.title = player.title.clone();
        }
        if (player.width, player.height) != (default.width, default.height) && player.width > 0 && player.height > 0 {
            self.width = player.width;
            self.height = player.height;
        }
        if player.resizable != default.resizable {
            self.resizable = player.resizable;
        }
        if player.fullscreen != default.fullscreen {
            self.fullscreen = player.fullscreen;
        }
        if player.vsync != default.vsync {
            self.vsync = player.vsync;
        }
        if player.fps_cap != default.fps_cap {
            self.fps_cap = player.fps_cap;
        }
        self
    }

    /// Длительность кадра при ограничении `fps_cap` (`None` — без ограничения).
    pub fn frame_period(&self) -> Option<std::time::Duration> {
        (self.fps_cap > 0).then(|| std::time::Duration::from_secs_f64(1.0 / self.fps_cap as f64))
    }
}

/// Готовый к работе GPU: то, что нужно, чтобы (пере)создавать поверхность.
struct Gfx {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// `None` между `suspended` и `resumed` (Android сворачивает приложение).
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    /// Формат, в котором рисует рендерер: sRGB-вид формата поверхности.
    view_format: wgpu::TextureFormat,
    renderer: Renderer,
}

/// Результат асинхронной инициализации GPU (в браузере она завершается позже `resumed`).
struct GpuInit {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    gpu: Gpu,
}

enum UserEvent {
    GpuReady(Result<GpuInit, RenderError>),
}

struct Runner {
    app: App,
    config: WindowConfig,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    /// GPU запрошен, ответа ещё нет (только в браузере).
    gpu_pending: bool,
    proxy: EventLoopProxy<UserEvent>,
    error: Option<RenderError>,
    last_frame: Instant,
    frames: u64,
    fps_window_start: Instant,
    fps_window_frames: u32,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // итог FPS печатается только на десктопе
    started: Instant,
}

/// Ограничить размер поверхности тем, что умеет устройство (WebGL2 — часто 4096 или 8192).
fn clamp_size(width: u32, height: u32, device: &wgpu::Device) -> (u32, u32) {
    let max = device.limits().max_texture_dimension_2d.max(1);
    (width.clamp(1, max), height.clamp(1, max))
}

impl Runner {
    fn create_window(&self, event_loop: &ActiveEventLoop) -> Result<Arc<Window>, RenderError> {
        let attributes = Window::default_attributes().with_title(self.config.title.clone());
        let attributes = crate::appicon::with_window_icon(attributes, self.config.icon.as_deref());
        // В браузере размер холста задаёт CSS страницы: явный размер winit записал бы
        // в style холста и сломал бы «на всё окно».
        #[cfg(not(target_arch = "wasm32"))]
        let attributes = attributes
            .with_inner_size(winit::dpi::LogicalSize::new(self.config.width, self.config.height))
            .with_resizable(self.config.resizable)
            .with_fullscreen(match self.config.fullscreen {
                FullscreenMode::Windowed => None,
                FullscreenMode::Borderless => Some(winit::window::Fullscreen::Borderless(None)),
            });
        #[cfg(target_arch = "wasm32")]
        let attributes = web::canvas_attributes(attributes);
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| RenderError::Window(e.to_string()))?;
        Ok(Arc::new(window))
    }

    /// Запросить GPU для окна. На десктопе ответ готов сразу, в браузере приходит
    /// событием [`UserEvent::GpuReady`].
    fn request_gpu(&mut self, window: Arc<Window>) {
        let request = async move {
            let instance = new_instance().await;
            let surface = instance
                .create_surface(window)
                .map_err(|e| RenderError::Window(e.to_string()))?;
            let gpu = Gpu::request(&instance, Some(&surface)).await?;
            Ok(GpuInit { instance, surface, gpu })
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = pollster::block_on(request);
            // Десктоп: событие тут же доставит winit в этом же цикле.
            let _ = self.proxy.send_event(UserEvent::GpuReady(result));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = proxy.send_event(UserEvent::GpuReady(request.await));
            });
        }
        self.gpu_pending = true;
    }

    fn finish_gpu(&mut self, init: GpuInit) -> Result<(), RenderError> {
        let Some(window) = self.window.clone() else {
            return Err(RenderError::Window("the window is gone".to_string()));
        };
        let GpuInit { instance, surface, gpu } = init;
        let caps = surface.get_capabilities(&gpu.adapter);
        // Рендер пишет линейный цвет и рассчитывает на sRGB-кодирование при записи.
        // WebGPU не даёт sRGB-форматов поверхности — тогда берём обычный формат и
        // sRGB-вид на него (view_formats).
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| RenderError::Window("surface reports no formats".to_string()))?;
        let view_format = format.add_srgb_suffix();
        let size = window.inner_size();
        let (width, height) = clamp_size(size.width, size.height, &gpu.device);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: if self.config.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            alpha_mode: caps.alpha_modes[0],
            view_formats: if view_format == format { vec![] } else { vec![view_format] },
            // Один кадр в очереди: меньше задержка между вводом и картинкой.
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&gpu.device, &config);
        let info = gpu.adapter.get_info();
        log::info!("renderer: {} ({:?}), surface {format:?}", info.name, info.backend);
        #[cfg(target_arch = "wasm32")]
        web::report_backend(info.backend);

        self.gfx = Some(Gfx {
            instance,
            adapter: gpu.adapter,
            surface: Some(surface),
            config,
            view_format,
            renderer: Renderer::new(gpu.device, gpu.queue, view_format),
        });
        self.app
            .world
            .input
            .set_screen_size(Vec2::new(width as f32, height as f32));
        self.last_frame = Instant::now();
        self.fps_window_start = self.last_frame;
        window.request_redraw();
        Ok(())
    }

    /// После `suspended` (Android) — новая поверхность для того же устройства.
    fn recreate_surface(&mut self) -> Result<(), RenderError> {
        let (Some(window), Some(gfx)) = (self.window.clone(), self.gfx.as_mut()) else {
            return Ok(());
        };
        if gfx.surface.is_some() {
            return Ok(());
        }
        let surface = gfx
            .instance
            .create_surface(window.clone())
            .map_err(|e| RenderError::Window(e.to_string()))?;
        let caps = surface.get_capabilities(&gfx.adapter);
        if !caps.formats.contains(&gfx.config.format) {
            return Err(RenderError::Window(format!(
                "the new surface does not support {:?}, the format the renderer was created for",
                gfx.config.format
            )));
        }
        let size = window.inner_size();
        let (width, height) = clamp_size(size.width, size.height, gfx.renderer.device());
        gfx.config.width = width;
        gfx.config.height = height;
        surface.configure(gfx.renderer.device(), &gfx.config);
        gfx.surface = Some(surface);
        self.app
            .world
            .input
            .set_screen_size(Vec2::new(width as f32, height as f32));
        log::info!("surface recreated ({width}x{height})");
        window.request_redraw();
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: RenderError) {
        log::error!("{error}");
        #[cfg(target_arch = "wasm32")]
        web::report_error(&error.to_string());
        self.error = Some(error);
        event_loop.exit();
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let Some(surface) = gfx.surface.as_ref() else {
            return; // свёрнуто: мир стоит, пока поверхности нет
        };
        let now = Instant::now();
        // Ограничиваем dt: после паузы/перетаскивания окна игра не «прыгает».
        let dt = (now - self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;

        self.app.tick(dt);

        let mut stats = RenderStats::default();
        match surface.get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(gfx.view_format),
                    ..wgpu::TextureViewDescriptor::default()
                });
                stats = gfx.renderer.render(
                    &self.app.world,
                    &view,
                    gfx.config.width,
                    gfx.config.height,
                );
                frame.present();
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                surface.configure(gfx.renderer.device(), &gfx.config);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                self.fail(event_loop, RenderError::Device("out of GPU memory".to_string()));
                return;
            }
            Err(other) => log::warn!("skipping a frame: {other}"),
        }
        self.app.world.resources.insert(stats);

        // Мини-профайлер в заголовке окна (в браузере заголовок вкладки не трогаем).
        self.frames += 1;
        self.fps_window_frames += 1;
        let elapsed = (now - self.fps_window_start).as_secs_f32();
        if elapsed >= 0.5 {
            let fps = self.fps_window_frames as f32 / elapsed;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(window) = &self.window {
                window.set_title(&format!(
                    "{} — {fps:.0} FPS · {} draw calls · {} sprites",
                    self.config.title, stats.draw_calls, stats.sprites,
                ));
            }
            #[cfg(target_arch = "wasm32")]
            web::report_fps(fps);
            self.fps_window_start = now;
            self.fps_window_frames = 0;
        }
        let finished = self.config.max_frames.is_some_and(|max| self.frames >= max);
        if finished || self.app.world.quit_requested() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = match &self.window {
            Some(window) => window.clone(),
            None => match self.create_window(event_loop) {
                Ok(window) => {
                    self.window = Some(window.clone());
                    window
                }
                Err(err) => return self.fail(event_loop, err),
            },
        };
        if self.gfx.is_some() {
            if let Err(err) = self.recreate_surface() {
                self.fail(event_loop, err);
            }
        } else if !self.gpu_pending {
            self.request_gpu(window);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Android уничтожает окно при сворачивании: отпускаем поверхность, но
        // устройство, текстуры на GPU и мир остаются — возврат мгновенный.
        if let Some(gfx) = self.gfx.as_mut() {
            if gfx.surface.take().is_some() {
                log::info!("surface released (suspended)");
            }
        }
        self.app.world.input.release_all();
        // Игра в фоне стоит: подсистемы узнают об этом (звук замирает) и оживают
        // на первом кадре после `resumed`.
        self.app.idle();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::GpuReady(result) => {
                self.gpu_pending = false;
                if let Err(err) = result.and_then(|init| self.finish_gpu(init)) {
                    self.fail(event_loop, err);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let input = &mut self.app.world.input;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            // Окно потеряло фокус: отпускания клавиш до нас уже не дойдут, поэтому
            // отпускаем всё сами — иначе персонаж продолжит бежать.
            WindowEvent::Focused(focused) => {
                if !focused {
                    input.release_all();
                }
                // Звук глушится по `AudioSettings::mute_when_unfocused` (engine-audio).
                self.app.world.resources.insert(WindowFocus { focused });
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    let (width, height) = clamp_size(size.width, size.height, gfx.renderer.device());
                    gfx.config.width = width;
                    gfx.config.height = height;
                    if let Some(surface) = &gfx.surface {
                        surface.configure(gfx.renderer.device(), &gfx.config);
                    }
                    input.set_screen_size(Vec2::new(width as f32, height as f32));
                } else {
                    input.set_screen_size(Vec2::new(size.width as f32, size.height as f32));
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(key) = map_key(code) {
                        input.set_key(key, event.state == ElementState::Pressed);
                    }
                }
                // Android: системная кнопка «Назад» — это Escape (как в Unity). Выйти
                // по ней игра может сама: `ctx.quit()`.
                #[cfg(target_os = "android")]
                if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::BrowserBack) {
                    input.set_key(Key::Escape, event.state == ElementState::Pressed);
                }
                if event.state == ElementState::Pressed {
                    if let Some(text) = &event.text {
                        input.add_text(text);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Строки колеса; у тачпада и в браузере — пиксели, приводим к строкам.
                let lines = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
                    winit::event::MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32) / 40.0,
                };
                input.add_scroll(lines);
            }
            WindowEvent::Touch(touch) => {
                let phase = match touch.phase {
                    winit::event::TouchPhase::Started => TouchPhase::Began,
                    winit::event::TouchPhase::Moved => TouchPhase::Moved,
                    winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => TouchPhase::Ended,
                };
                let position = Vec2::new(touch.location.x as f32, touch.location.y as f32);
                input.set_touch(touch.id, position, phase);
            }
            WindowEvent::CursorMoved { position, .. } => {
                input.set_mouse_position(Vec2::new(position.x as f32, position.y as f32));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    winit::event::MouseButton::Left => Some(MouseButton::Left),
                    winit::event::MouseButton::Right => Some(MouseButton::Right),
                    winit::event::MouseButton::Middle => Some(MouseButton::Middle),
                    _ => None,
                };
                if let Some(button) = button {
                    input.set_mouse_button(button, state == ElementState::Pressed);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(window) = &self.window else {
            return;
        };
        // Ограничение частоты кадров (`fps_cap`): до срока следующего кадра цикл спит.
        // В браузере кадры задаёт `requestAnimationFrame`.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(period) = self.config.frame_period() {
            let next = self.last_frame + period;
            if Instant::now() < next {
                _event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                return;
            }
            _event_loop.set_control_flow(ControlFlow::Poll);
        }
        window.request_redraw();
    }
}

/// Экземпляр wgpu. В браузере — WebGPU, если он есть и выдаёт адаптер, иначе WebGL2.
async fn new_instance() -> wgpu::Instance {
    #[cfg(target_arch = "wasm32")]
    {
        let mut descriptor = wgpu::InstanceDescriptor::default();
        if web::force_webgl() {
            // Без возможности `webgl` в wasm нет GL-бэкенда: `?backend=webgl` игнорируем.
            #[cfg(feature = "webgl")]
            {
                descriptor.backends = wgpu::Backends::GL;
            }
            #[cfg(not(feature = "webgl"))]
            log::warn!("`?backend=webgl` ignored: this build has no WebGL2 fallback (built with `--no-webgl`)");
        }
        wgpu::util::new_instance_with_webgpu_detection(descriptor).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Instance::new(wgpu::InstanceDescriptor::default())
    }
}

/// Android: приложение NativeActivity, которое передал `android_main`.
#[cfg(target_os = "android")]
pub use winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::Mutex<Option<AndroidApp>> = std::sync::Mutex::new(None);

/// Android: запомнить приложение из `android_main` до вызова [`run`] (без него на
/// Android окно не создать). Это делает `engine::game_main!`.
#[cfg(target_os = "android")]
pub fn set_android_app(app: AndroidApp) {
    *ANDROID_APP.lock().unwrap_or_else(|p| p.into_inner()) = Some(app);
}

/// Открыть окно и крутить главный цикл, пока его не закроют.
///
/// В браузере функция возвращается сразу: цикл продолжает жить в событиях страницы
/// (`requestAnimationFrame`), а ошибки GPU показываются на странице и в консоли.
pub fn run(app: App, config: WindowConfig) -> Result<(), RenderError> {
    #[allow(unused_mut)] // меняется только на Android
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        let app = ANDROID_APP
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or_else(|| RenderError::Window("call `set_android_app` from `android_main` first".to_string()))?;
        builder.with_android_app(app);
    }
    let event_loop = builder.build().map_err(|e| RenderError::Window(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let now = Instant::now();
    let runner = Runner {
        app,
        config,
        window: None,
        gfx: None,
        gpu_pending: false,
        proxy: event_loop.create_proxy(),
        error: None,
        last_frame: now,
        frames: 0,
        fps_window_start: now,
        fps_window_frames: 0,
        started: now,
    };

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(runner);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut runner = runner;
        event_loop
            .run_app(&mut runner)
            .map_err(|e| RenderError::Window(e.to_string()))?;
        if let Some(err) = runner.error.take() {
            return Err(err);
        }
        let seconds = runner.started.elapsed().as_secs_f32();
        if runner.frames > 0 && seconds > 0.0 {
            log::info!(
                "window closed after {} frames in {seconds:.1} s: {:.0} FPS on average",
                runner.frames,
                runner.frames as f32 / seconds
            );
        }
        Ok(())
    }
}

/// Связь с веб-страницей: холст, выбранный бэкенд, ошибки и FPS для `index.html`.
#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::JsCast;
    use winit::platform::web::WindowAttributesExtWebSys;
    use winit::window::WindowAttributes;

    use super::WEB_CANVAS_ID;

    pub(super) fn canvas_attributes(attributes: WindowAttributes) -> WindowAttributes {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(WEB_CANVAS_ID))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        let append = canvas.is_none();
        attributes
            .with_canvas(canvas)
            .with_append(append)
            .with_prevent_default(true)
            .with_focusable(true)
    }

    /// `?backend=webgl` в адресе страницы — принудительно WebGL2 (для проверки запасного пути).
    pub(super) fn force_webgl() -> bool {
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .is_some_and(|q| q.contains("backend=webgl"))
    }

    fn set_body_data(name: &str, value: &str) {
        if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
            let _ = body.set_attribute(name, value);
        }
    }

    /// Страница узнаёт, на чём рисуем (`<body data-piska-backend="BrowserWebGpu">`), и
    /// убирает индикатор загрузки.
    pub(super) fn report_backend(backend: wgpu::Backend) {
        set_body_data("data-piska-backend", &format!("{backend:?}"));
        set_body_data("data-piska-state", "running");
    }

    pub(super) fn report_error(message: &str) {
        set_body_data("data-piska-error", message);
        set_body_data("data-piska-state", "error");
    }

    pub(super) fn report_fps(fps: f32) {
        set_body_data("data-piska-fps", &format!("{fps:.0}"));
    }
}

fn map_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        KeyCode::Digit0 => Key::Digit0,
        KeyCode::Digit1 => Key::Digit1,
        KeyCode::Digit2 => Key::Digit2,
        KeyCode::Digit3 => Key::Digit3,
        KeyCode::Digit4 => Key::Digit4,
        KeyCode::Digit5 => Key::Digit5,
        KeyCode::Digit6 => Key::Digit6,
        KeyCode::Digit7 => Key::Digit7,
        KeyCode::Digit8 => Key::Digit8,
        KeyCode::Digit9 => Key::Digit9,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::Space => Key::Space,
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::ShiftLeft => Key::LShift,
        KeyCode::ShiftRight => Key::RShift,
        KeyCode::ControlLeft => Key::LCtrl,
        KeyCode::ControlRight => Key::RCtrl,
        KeyCode::AltLeft => Key::LAlt,
        KeyCode::AltRight => Key::RAlt,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F4 => Key::F4,
        KeyCode::F5 => Key::F5,
        KeyCode::F6 => Key::F6,
        KeyCode::F7 => Key::F7,
        KeyCode::F8 => Key::F8,
        KeyCode::F9 => Key::F9,
        KeyCode::F10 => Key::F10,
        KeyCode::F11 => Key::F11,
        KeyCode::F12 => Key::F12,
        _ => return None,
    })
}
