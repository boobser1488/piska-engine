//! Piska Modeler — отдельное окно редактора моделей.
//!
//! ```text
//! cargo run -p piska-modeler -- [файлы.pmodel…]
//! cargo run -p piska-modeler -- assets/models/rover.pmodel --screenshot shot.png --size 1440x860
//! ```
//!
//! Ключи для снимков без окна: `--screenshot файл.png`, `--size ШxВ`, `--frames N`,
//! `--sample crate|lantern|pine|rover` (образец вместо файла), `--paint` (режим рисования),
//! `--tool <имя>`, `--view front|back|left|right|top|bottom`, `--ortho`, `--select <узел>`,
//! `--face north|east|south|west|up|down`, `--pointer X,Y` (курсор над окном), `--scale 1.25`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_modeler::ui::{Command, Mode, ModelerApp, Tool};
use engine_modeler::{FaceDir, ViewPreset};
use engine_render::{read_texture, Gpu};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Default)]
struct Options {
    files: Vec<String>,
    screenshot: Option<String>,
    size: (u32, u32),
    frames: u32,
    sample: Option<String>,
    paint: bool,
    tool: Option<String>,
    view: Option<String>,
    ortho: bool,
    select: Option<String>,
    face: Option<String>,
    pointer: Option<(f32, f32)>,
    scale: f32,
}

fn parse_args() -> Result<Options, String> {
    let mut o = Options { size: (1440, 860), frames: 6, scale: 1.0, ..Options::default() };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("`{name}` needs a value"));
        match arg.as_str() {
            "--screenshot" => o.screenshot = Some(value("--screenshot")?),
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v.split_once('x').ok_or("--size is WIDTHxHEIGHT")?;
                o.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--frames" => o.frames = value("--frames")?.parse().map_err(|_| "--frames is a number")?,
            "--sample" => o.sample = Some(value("--sample")?),
            "--paint" => o.paint = true,
            "--tool" => o.tool = Some(value("--tool")?),
            "--view" => o.view = Some(value("--view")?),
            "--ortho" => o.ortho = true,
            "--select" => o.select = Some(value("--select")?),
            "--face" => o.face = Some(value("--face")?),
            "--scale" => o.scale = value("--scale")?.parse().map_err(|_| "--scale is a number")?,
            "--pointer" => {
                let v = value("--pointer")?;
                let (x, y) = v.split_once(',').ok_or("--pointer is X,Y")?;
                o.pointer = Some((x.parse().map_err(|_| "bad X")?, y.parse().map_err(|_| "bad Y")?));
            }
            "--help" | "-h" => {
                return Err("usage: piska-modeler [model.pmodel…] [--screenshot out.png --size 1440x860 --frames 6 \
                     --sample crate|lantern|pine|rover --paint --tool NAME --view front|top|… --ortho \
                     --select NODE --face north|… --pointer X,Y --scale 1.0]"
                    .to_string())
            }
            other if other.starts_with("--") => return Err(format!("unknown option `{other}` (see --help)")),
            file => o.files.push(file.to_string()),
        }
    }
    Ok(o)
}

/// Окно моделей с документами из аргументов и настройками для снимков.
fn build_app(o: &Options) -> ModelerApp {
    let mut app = ModelerApp::new();
    for file in &o.files {
        if let Err(e) = app.open(file) {
            eprintln!("piska-modeler: {e}");
        }
    }
    if let Some(name) = &o.sample {
        match engine_modeler::samples::all().into_iter().find(|(n, _)| n == name) {
            Some((_, doc)) => app.open_document(doc, None),
            None => eprintln!("piska-modeler: no sample `{name}` (crate, lantern, pine, rover)"),
        }
    }
    if let Some(name) = &o.select {
        let id = app.document().find_by_name(name);
        if id.is_none() {
            eprintln!("piska-modeler: no node `{name}` in the model");
        }
        app.session_mut().select(id);
    }
    if let Some(face) = &o.face {
        let face = FaceDir::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(face));
        app.session_mut().face = face;
    }
    if let Some(view) = &o.view {
        let preset = match view.as_str() {
            "front" => Some(ViewPreset::Front),
            "back" => Some(ViewPreset::Back),
            "left" => Some(ViewPreset::Left),
            "right" => Some(ViewPreset::Right),
            "top" => Some(ViewPreset::Top),
            "bottom" => Some(ViewPreset::Bottom),
            _ => None,
        };
        if let Some(preset) = preset {
            app.run(Command::View(preset));
            app.run(Command::Frame);
        }
    }
    if o.ortho {
        app.run(Command::ToggleOrthographic);
    }
    if o.paint {
        app.run(Command::SetMode(Mode::Paint));
    }
    if let Some(name) = &o.tool {
        let all = Tool::EDIT.iter().chain(Tool::PAINT.iter());
        match all.into_iter().find(|t| t.name().eq_ignore_ascii_case(name)) {
            Some(tool) => app.run(Command::Tool(*tool)),
            None => eprintln!("piska-modeler: unknown tool `{name}`"),
        }
    }
    app.view.supersample = 2;
    app
}

fn frame_ui(app: &mut ModelerApp, ctx: &egui::Context, input: egui::RawInput) -> egui::FullOutput {
    ctx.run(input, |ctx| {
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| app.show(ui));
    })
}

/// Нарисовать интерфейс egui в текстуру.
fn paint(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut egui_wgpu::Renderer,
    ctx: &egui::Context,
    full: egui::FullOutput,
    target: &wgpu::TextureView,
    size: [u32; 2],
) {
    let primitives = ctx.tessellate(full.shapes, full.pixels_per_point);
    let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: full.pixels_per_point };
    for (id, delta) in &full.textures_delta.set {
        renderer.update_texture(device, queue, *id, delta);
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("modeler ui") });
    let extra = renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("modeler ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &primitives, &screen);
    }
    queue.submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
    for id in &full.textures_delta.free {
        renderer.free_texture(id);
    }
}

/// Снимок без окна.
fn screenshot(o: &Options, path: &str) -> Result<(), String> {
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    let mut app = build_app(o);
    app.autosave_seconds = 0.0;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let gpu = pollster::block_on(Gpu::request(&instance, None)).map_err(|e| e.to_string())?;
    let mut renderer = egui_wgpu::Renderer::new(&gpu.device, FORMAT, None, 1, false);
    let (w, h) = o.size;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modeler capture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let ctx = egui::Context::default();
    engine_editor::kit::theme::apply(&ctx, true);
    ctx.set_zoom_factor(o.scale);
    let dt = 1.0 / 60.0;
    for frame in 0..o.frames.max(3) {
        let events = o
            .pointer
            .map(|(x, y)| vec![egui::Event::PointerMoved(egui::pos2(x, y))])
            .unwrap_or_default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32) / o.scale)),
            time: Some(frame as f64 * dt),
            predicted_dt: dt as f32,
            events,
            ..Default::default()
        };
        let full = frame_ui(&mut app, &ctx, input);
        paint(&gpu.device, &gpu.queue, &mut renderer, &ctx, full, &view, [w, h]);
    }
    let image = read_texture(&gpu.device, &gpu.queue, &texture, w, h).map_err(|e| e.to_string())?;
    image.save(path).map_err(|e| format!("cannot write `{path}`: {e}"))?;
    println!("saved {path} ({w}x{h})");
    Ok(())
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
    egui: egui_winit::State,
}

struct Runner {
    app: ModelerApp,
    gfx: Option<Gfx>,
    error: Option<String>,
    next_frame: Instant,
    frames: u64,
    max_frames: Option<u64>,
    title: String,
}

impl Runner {
    fn create(&self, event_loop: &ActiveEventLoop) -> Result<Gfx, String> {
        let attributes = Window::default_attributes()
            .with_title("Piska Modeler")
            .with_inner_size(LogicalSize::new(1440.0, 880.0));
        let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let gpu = pollster::block_on(Gpu::request(&instance, Some(&surface))).map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&gpu.adapter);
        // egui рисует в гамма-пространстве: поверхность не-sRGB.
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
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&gpu.device, &config);
        let ctx = egui::Context::default();
        engine_editor::kit::theme::apply(&ctx, true);
        let max_side = gpu.device.limits().max_texture_dimension_2d as usize;
        let egui = egui_winit::State::new(ctx, egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, Some(max_side));
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, None, 1, false);
        Ok(Gfx { window, surface, config, device: gpu.device, queue: gpu.queue, renderer, egui })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        match gfx.surface.get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let input = gfx.egui.take_egui_input(&gfx.window);
                let ctx = gfx.egui.egui_ctx().clone();
                let full = frame_ui(&mut self.app, &ctx, input);
                let platform = full.platform_output.clone();
                let delay = full
                    .viewport_output
                    .get(&egui::ViewportId::ROOT)
                    .map_or(Duration::ZERO, |v| v.repaint_delay);
                paint(&gfx.device, &gfx.queue, &mut gfx.renderer, &ctx, full, &view, [gfx.config.width, gfx.config.height]);
                gfx.egui.handle_platform_output(&gfx.window, platform);
                gfx.window.pre_present_notify();
                frame.present();
                // Перерисовка по требованию; в покое — раз в секунду (файл восстановления,
                // перечитывание текстур).
                self.next_frame = Instant::now() + delay.max(Duration::from_millis(8)).min(Duration::from_secs(1));
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                self.next_frame = Instant::now();
            }
            Err(e) => log::warn!("skipping a frame: {e}"),
        }
        let title = match self.app.path() {
            Some(p) => format!("{}{} — Piska Modeler", p.display(), if self.app.has_unsaved_changes() { " ●" } else { "" }),
            None => "Piska Modeler".to_string(),
        };
        if title != self.title {
            gfx.window.set_title(&title);
            self.title = title;
        }
        self.frames += 1;
        if self.max_frames.is_some_and(|m| self.frames >= m) {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match self.create(event_loop) {
            Ok(gfx) => {
                gfx.window.request_redraw();
                self.gfx = Some(gfx);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let response = gfx.egui.on_window_event(&gfx.window, &event);
        if response.repaint {
            self.next_frame = Instant::now();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.device, &gfx.config);
                self.next_frame = Instant::now();
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = &self.gfx else { return };
        if Instant::now() >= self.next_frame {
            gfx.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        }
    }
}

fn main() {
    let options = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Some(path) = options.screenshot.clone() {
        if let Err(e) = screenshot(&options, &path) {
            eprintln!("piska-modeler: {e}");
            std::process::exit(1);
        }
        return;
    }
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("piska-modeler: cannot open a window: {e}");
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut runner = Runner {
        app: build_app(&options),
        gfx: None,
        error: None,
        next_frame: Instant::now(),
        frames: 0,
        max_frames: std::env::var("ENGINE_FRAMES").ok().and_then(|v| v.parse().ok()),
        title: String::new(),
    };
    if let Err(e) = event_loop.run_app(&mut runner) {
        eprintln!("piska-modeler: {e}");
    }
    if let Some(e) = runner.error {
        eprintln!("piska-modeler: {e}");
        std::process::exit(1);
    }
    if runner.app.has_unsaved_changes() {
        eprintln!("piska-modeler: closed with unsaved changes; they stay in the recovery file and are offered on the next start");
    }
}
