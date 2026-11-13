//! Нагрузочный стенд: воспроизводимые сцены для замеров производительности.
//!
//! ```text
//! cargo run --example stress                      # все сценарии без окна, таблица в stdout
//! cargo run --example stress -- meshes particles  # только эти сценарии
//! STRESS_WINDOW=1 ENGINE_FRAMES=600 cargo run --example stress -- meshes   # окно, средний FPS
//! ```
//!
//! Без окна каждый сценарий прогоняется `STRESS_RUNS` раз (по умолчанию 3) по
//! `STRESS_FRAMES` кадров (по умолчанию 200) с шагом 1/60 с после 30 кадров разгона;
//! печатается медиана по прогонам (внутри прогона — 10-й перцентиль по кадрам, см.
//! `STRESS_STAT`) в миллисекундах: скрипты, физика, прочие подсистемы, конец кадра,
//! рендер на CPU (из него — извлечение и запись команд), ожидание видеокарты, весь кадр.
//! `STRESS_SCALE=0.5` уменьшает число объектов во всех сценариях.

use std::path::PathBuf;

use engine::prelude::*;
use engine_render::Renderer;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

// ---------------------------------------------------------------------------
// Скрипты сценариев
// ---------------------------------------------------------------------------

component! {
    /// Двигает всех детей своего объекта по кругу (спрайты «роем»).
    pub struct Swarm {
        pub speed: f32 = 1.0,
        pub time: f32 = 0.0,
    }
}
impl Component for Swarm {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.time += dt * self.speed;
        let children = ctx.object().children().to_vec();
        for (index, child) in children.into_iter().enumerate() {
            let phase = self.time + index as f32 * 0.37;
            if let Some(object) = ctx.world.scene.get_mut(child) {
                object.transform.position.x += phase.cos() * dt * 2.0;
                object.transform.position.y += phase.sin() * dt * 2.0;
                object.transform.rotate_z(dt);
            }
        }
    }
}

component! {
    /// Покачивание: самый простой скрипт с `on_update`.
    pub struct Bobber {
        pub phase: f32 = 0.0,
    }
}
impl Component for Bobber {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.phase += dt;
        ctx.transform().position.y = self.phase.sin();
    }
}

component! {
    /// Вращает объект вокруг Z (звенья глубокой иерархии).
    pub struct Spin {
        pub speed: f32 = 0.5,
    }
}
impl Component for Spin {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let angle = self.speed * dt;
        ctx.transform().rotate_z(angle);
    }
}

component! {
    /// Медленно ведёт камеру вправо и обратно.
    pub struct Pan {
        pub amplitude: f32 = 20.0,
        pub time: f32 = 0.0,
        pub origin: Vec3 = Vec3::ZERO,
    }
}
impl Component for Pan {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.origin = ctx.transform().position;
    }
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.time += dt;
        let offset = (self.time * 0.5).sin() * self.amplitude;
        let origin = self.origin;
        ctx.transform().position = origin + Vec3::X * offset;
    }
}

component! {
    /// Короткоживущий объект сценария `churn`: летит вверх и через `life` секунд исчезает.
    pub struct Spark {
        pub life: f32 = 0.5,
    }
}
impl Component for Spark {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.life -= dt;
        ctx.transform().position.y += dt * 3.0;
        if self.life <= 0.0 {
            ctx.destroy_self();
        }
    }
}

component! {
    /// Сценарий `churn`: каждый кадр создаёт `per_frame` объектов со спрайтом и `Spark`.
    pub struct Spawner {
        pub per_frame: i32 = 100,
        pub counter: i32 = 0,
    }
}
impl Component for Spawner {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        for _ in 0..self.per_frame {
            self.counter += 1;
            let x = (self.counter % 97) as f32 * 0.4 - 19.0;
            ctx.scene_mut()
                .build("Spark")
                .at(x, -10.0)
                .with(SpriteRenderer::colored(Color::hex(0xffd166)).with_size(0.2, 0.2))
                .with(Spark::default());
        }
    }
}

// ---------------------------------------------------------------------------
// Ассеты: несколько цветных текстур во временном каталоге
// ---------------------------------------------------------------------------

fn assets_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("piska_stress_assets");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let colors = [[230, 80, 70], [80, 200, 120], [70, 120, 230], [240, 200, 60]];
    for (index, [r, g, b]) in colors.into_iter().enumerate() {
        let path = dir.join(format!("tex{index}.png"));
        if !path.exists() {
            let image = image::RgbaImage::from_fn(32, 32, |x, y| {
                let edge = x < 2 || y < 2 || x > 29 || y > 29;
                if edge {
                    image::Rgba([20, 20, 20, 255])
                } else {
                    image::Rgba([r, g, b, 255])
                }
            });
            image.save(&path).expect("write texture");
        }
    }
    // Тайлсет 4×4 ячейки.
    let tiles = dir.join("tiles.png");
    if !tiles.exists() {
        let image = image::RgbaImage::from_fn(64, 64, |x, y| {
            let cell = (y / 16) * 4 + x / 16;
            let shade = 60 + (cell * 12) as u8;
            image::Rgba([shade, 255 - shade, 120, 255])
        });
        image.save(&tiles).expect("write tileset");
    }
    dir
}

fn texture(app: &App, name: &str) -> AssetHandle<Texture> {
    app.world
        .resources
        .get::<AssetDatabase>()
        .expect("assets mounted")
        .handle(name)
        .expect("texture exists")
}

fn scaled(count: usize) -> usize {
    let scale: f32 = std::env::var("STRESS_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    ((count as f32 * scale).round() as usize).max(1)
}

fn camera_2d(app: &mut App, half_height: f32) {
    app.spawn("Camera").with(Camera {
        clear_color: Color::hex(0x1b1f2a),
        ..Camera::orthographic(half_height)
    });
}

fn camera_3d(app: &mut App, eye: Vec3, target: Vec3) {
    let mut transform = Transform::from_xyz(eye.x, eye.y, eye.z);
    transform.look_at(target, Vec3::Y);
    app.spawn("Camera")
        .at3(eye.x, eye.y, eye.z)
        .rotation(transform.rotation)
        .with(Camera::perspective(60.0));
}

// ---------------------------------------------------------------------------
// Сценарии
// ---------------------------------------------------------------------------

struct Scenario {
    name: &'static str,
    about: &'static str,
    build: fn(&mut App),
}

const SCENARIOS: &[Scenario] = &[
    Scenario { name: "sprites_static", about: "20k static sprites, 4 textures", build: sprites_static },
    Scenario { name: "sprites_moving", about: "10k moving sprites, 4 textures", build: sprites_moving },
    Scenario { name: "tilemap", about: "400x400 tilemap, panning camera", build: tilemap },
    Scenario { name: "particles", about: "300 particle systems", build: particles },
    Scenario { name: "meshes", about: "10k static meshes (3 kinds) + shadows, most off-screen", build: meshes },
    Scenario { name: "rigidbodies", about: "1500 dynamic 3D rigidbodies", build: rigidbodies },
    Scenario { name: "hierarchy", about: "100 chains x 30 deep, every link spins", build: hierarchy },
    Scenario { name: "scripts", about: "20k objects with on_update", build: scripts },
    Scenario { name: "ui", about: "UI canvas with 400 widgets", build: ui },
    Scenario { name: "plain", about: "20k objects without components", build: plain },
    Scenario { name: "churn", about: "100 sprites spawned and destroyed per frame, 20k static", build: churn },
];

fn sprites_static(app: &mut App) {
    camera_2d(app, 30.0);
    let textures: Vec<_> = (0..4).map(|i| texture(app, &format!("tex{i}.png"))).collect();
    let n = scaled(20_000);
    let side = (n as f32).sqrt().ceil() as usize;
    for i in 0..n {
        let (x, y) = ((i % side) as f32, (i / side) as f32);
        app.spawn("Sprite")
            .at(x * 0.6 - side as f32 * 0.3, y * 0.6 - side as f32 * 0.3)
            .with(SpriteRenderer::textured(textures[i % 4]).with_size(0.5, 0.5));
    }
}

fn sprites_moving(app: &mut App) {
    camera_2d(app, 30.0);
    let textures: Vec<_> = (0..4).map(|i| texture(app, &format!("tex{i}.png"))).collect();
    let n = scaled(10_000);
    let side = (n as f32).sqrt().ceil() as usize;
    let swarm = app.spawn("Swarm").with(Swarm::default()).id();
    for i in 0..n {
        let (x, y) = ((i % side) as f32, (i / side) as f32);
        app.spawn("Sprite")
            .at(x * 0.5 - side as f32 * 0.25, y * 0.5 - side as f32 * 0.25)
            .with(SpriteRenderer::textured(textures[i % 4]).with_size(0.4, 0.4).with_order(0, (i % 7) as i32))
            .child_of(swarm);
    }
}

fn tilemap(app: &mut App) {
    app.spawn("Camera")
        .at(200.0, 200.0)
        .with(Camera {
            clear_color: Color::hex(0x1b1f2a),
            ..Camera::orthographic(12.0)
        })
        .with(Pan::default());
    let tileset = texture(app, "tiles.png");
    let side = scaled(400).max(16);
    let rows: Vec<String> = (0..side)
        .map(|y| {
            (0..side)
                .map(|x| if (x * 7 + y * 3) % 11 == 0 { ".".to_string() } else { (1 + (x + y) % 16).to_string() })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    app.spawn("Tilemap").with(Tilemap::from_rows(&rows).with_tileset(tileset, 4, 4));
}

fn particles(app: &mut App) {
    camera_2d(app, 30.0);
    let n = scaled(300);
    let side = (n as f32).sqrt().ceil() as usize;
    for i in 0..n {
        let (x, y) = ((i % side) as f32, (i / side) as f32);
        let system = match i % 3 {
            0 => ParticleSystem::fire(),
            1 => ParticleSystem::smoke(),
            _ => ParticleSystem::sparks(),
        };
        app.spawn("Particles")
            .at(x * 3.0 - side as f32 * 1.5, y * 3.0 - side as f32 * 1.5)
            .with(system.with_seed(i as i32));
    }
}

fn meshes(app: &mut App) {
    camera_3d(app, Vec3::new(0.0, 12.0, 30.0), Vec3::new(0.0, 0.0, 0.0));
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0))
        .with(Light::default());
    app.spawn("Ground")
        .scale(400.0, 1.0, 400.0)
        .with(MeshRenderer::plane().with_color(Color::hex(0x8a8f7c)).without_shadows());
    let n = scaled(10_000);
    let side = (n as f32).sqrt().ceil() as usize;
    let colors = [0x4c8dff, 0x6fcf97, 0xf2c94c, 0xeb5757];
    for i in 0..n {
        let (x, z) = ((i % side) as f32, (i / side) as f32);
        let mesh = match i % 3 {
            0 => MeshRenderer::cube(),
            1 => MeshRenderer::sphere(),
            _ => MeshRenderer::cylinder(),
        };
        app.spawn("Prop")
            .at3(x * 2.5 - side as f32 * 1.25, 0.5, z * -2.5 + 20.0)
            .with(mesh.with_color(Color::hex(colors[i % 4])).with_material(0.0, 0.6));
    }
}

fn rigidbodies(app: &mut App) {
    camera_3d(app, Vec3::new(0.0, 15.0, 35.0), Vec3::new(0.0, 3.0, 0.0));
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0))
        .with(Light::default());
    app.spawn("Ground")
        .scale(200.0, 1.0, 200.0)
        .with(MeshRenderer::plane().with_color(Color::hex(0x8a8f7c)).without_shadows());
    app.spawn("Ground Collider")
        .at3(0.0, -0.5, 0.0)
        .with(Collider::cuboid(200.0, 1.0, 200.0));
    let n = scaled(1500);
    for i in 0..n {
        let (x, y, z) = ((i % 15) as f32, (i / 225) as f32, ((i / 15) % 15) as f32);
        app.spawn("Crate")
            .at3(x * 1.3 - 9.0, 1.0 + y * 1.3, z * 1.3 - 9.0)
            .with(MeshRenderer::cube().with_color(Color::hex(0x4c8dff)))
            .with(Rigidbody::dynamic())
            .with(Collider::cuboid(1.0, 1.0, 1.0));
    }
}

fn hierarchy(app: &mut App) {
    camera_2d(app, 40.0);
    let chains = scaled(100);
    for c in 0..chains {
        let mut parent = app
            .spawn("Root")
            .at((c % 10) as f32 * 8.0 - 36.0, (c / 10) as f32 * 8.0 - 36.0)
            .with(Spin { speed: 0.3 })
            .id();
        for depth in 0..30 {
            parent = app
                .spawn("Link")
                .at(0.3, 0.0)
                .with(Spin { speed: 0.1 + depth as f32 * 0.01 })
                .with(SpriteRenderer::colored(Color::hex(0xf2c94c)).with_size(0.25, 0.25))
                .child_of(parent)
                .id();
        }
    }
}

fn scripts(app: &mut App) {
    camera_2d(app, 10.0);
    for i in 0..scaled(20_000) {
        app.spawn("Scripted").with(Bobber { phase: i as f32 * 0.01 });
    }
}

fn plain(app: &mut App) {
    camera_2d(app, 10.0);
    for i in 0..scaled(20_000) {
        app.spawn("Plain").at((i % 200) as f32 * 0.1, (i / 200) as f32 * 0.1);
    }
}

fn churn(app: &mut App) {
    camera_2d(app, 20.0);
    for i in 0..scaled(20_000) {
        app.spawn("Static").at((i % 200) as f32 * 0.2 - 20.0, (i / 200) as f32 * 0.2 - 10.0);
    }
    app.spawn("Spawner").with(Spawner { per_frame: scaled(100) as i32, counter: 0 });
}

fn ui(app: &mut App) {
    camera_2d(app, 10.0);
    let scene = &mut app.world.scene;
    let canvas = scene.ui_canvas().id();
    let grid = scene.ui_node(canvas, "Grid").stretch().row(4.0).padding(8.0).id();
    let columns = scaled(10);
    for c in 0..columns {
        let column = scene.ui_panel(grid).width(120.0).column(2.0).padding(4.0).id();
        for r in 0..10 {
            scene.ui_text(column, &format!("Item {c}-{r}")).font_size(12.0);
            scene.ui_button(column, &format!("b{c}_{r}"), "Press");
            scene.ui_progress(column, (r as f32) / 10.0);
            scene.ui_toggle(column, &format!("t{c}_{r}"), "On", r % 2 == 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Замер
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Sample {
    scripts: f32,
    physics: f32,
    other: f32,
    end_frame: f32,
    render: f32,
    extract: f32,
    encode: f32,
    gpu: f32,
    total: f32,
}

fn new_app() -> App {
    let mut app = engine::app();
    app.register::<Swarm>()
        .register::<Bobber>()
        .register::<Spin>()
        .register::<Pan>()
        .register::<Spark>()
        .register::<Spawner>();
    engine::mount_assets(&mut app, assets_dir()).expect("stress assets");
    app
}

/// Сводка по кадрам одного прогона: `STRESS_STAT=mean` — среднее, `median` — медиана,
/// по умолчанию `p10` — 10-й перцентиль. На машине, где параллельно идёт сборка или
/// другая нагрузка, среднее «плавает» в разы; быстрые кадры показывают цену самой работы.
fn frame_stat(mut values: Vec<f32>) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    match std::env::var("STRESS_STAT").as_deref() {
        Ok("mean") => values.iter().sum::<f32>() / values.len() as f32,
        Ok("median") => median(values),
        _ => {
            values.sort_by(f32::total_cmp);
            values[values.len() / 10]
        }
    }
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

fn measure(scenario: &Scenario, renderer: &mut Renderer, frames: u32, runs: u32) -> (Sample, engine_render::RenderStats) {
    let target = renderer.device().create_texture(&engine_render::wgpu::TextureDescriptor {
        label: Some("stress target"),
        size: engine_render::wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: engine_render::wgpu::TextureDimension::D2,
        format: renderer.format(),
        usage: engine_render::wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut per_run = Vec::new();
    let mut last_stats = engine_render::RenderStats::default();
    for _ in 0..runs {
        let mut app = new_app();
        (scenario.build)(&mut app);
        let mut samples: Vec<Sample> = Vec::with_capacity(frames as usize);
        for frame in 0..frames + 30 {
            let clock = std::time::Instant::now();
            app.tick(1.0 / 60.0);
            let profile = app.profile().clone();
            let render_clock = std::time::Instant::now();
            let stats = renderer.render(&app.world, &view, WIDTH, HEIGHT);
            let render = render_clock.elapsed().as_secs_f32() * 1000.0;
            let gpu_clock = std::time::Instant::now();
            renderer.device().poll(engine_render::wgpu::Maintain::Wait);
            let gpu = gpu_clock.elapsed().as_secs_f32() * 1000.0;
            let total = clock.elapsed().as_secs_f32() * 1000.0;
            last_stats = stats;
            if frame < 30 {
                continue;
            }
            let physics = profile.subsystem_ms("physics2d") + profile.subsystem_ms("physics3d");
            let all_subsystems: f32 = profile.subsystems.iter().map(|(_, ms)| ms).sum();
            samples.push(Sample {
                scripts: profile.scripts_ms,
                physics,
                other: all_subsystems - physics,
                end_frame: profile.end_frame_ms,
                render,
                extract: stats.extract_ms,
                encode: stats.encode_ms,
                gpu,
                total,
            });
        }
        let reduce = |f: fn(&Sample) -> f32| frame_stat(samples.iter().map(f).collect());
        per_run.push(Sample {
            scripts: reduce(|s| s.scripts),
            physics: reduce(|s| s.physics),
            other: reduce(|s| s.other),
            end_frame: reduce(|s| s.end_frame),
            render: reduce(|s| s.render),
            extract: reduce(|s| s.extract),
            encode: reduce(|s| s.encode),
            gpu: reduce(|s| s.gpu),
            total: reduce(|s| s.total),
        });
    }
    let pick = |f: fn(&Sample) -> f32| median(per_run.iter().map(f).collect());
    (
        Sample {
            scripts: pick(|s| s.scripts),
            physics: pick(|s| s.physics),
            other: pick(|s| s.other),
            end_frame: pick(|s| s.end_frame),
            render: pick(|s| s.render),
            extract: pick(|s| s.extract),
            encode: pick(|s| s.encode),
            gpu: pick(|s| s.gpu),
            total: pick(|s| s.total),
        },
        last_stats,
    )
}

fn main() -> Result<(), EngineError> {
    let wanted: Vec<String> = std::env::args().skip(1).collect();
    let selected: Vec<&Scenario> = SCENARIOS
        .iter()
        .filter(|s| wanted.is_empty() || wanted.iter().any(|w| w == s.name))
        .collect();
    if selected.is_empty() {
        eprintln!("unknown scenario; available:");
        for s in SCENARIOS {
            eprintln!("  {:<15} {}", s.name, s.about);
        }
        std::process::exit(2);
    }

    if std::env::var("STRESS_WINDOW").is_ok_and(|v| v != "0") {
        // Окно: один сценарий, средний FPS печатает сам оконный цикл при закрытии.
        let scenario = selected[0];
        let mut app = new_app();
        (scenario.build)(&mut app);
        log::info!("stress: window run of `{}` ({})", scenario.name, scenario.about);
        return engine::run(app, WindowConfig::titled(format!("stress: {}", scenario.name)));
    }

    std::env::set_var("ENGINE_AUDIO", std::env::var("ENGINE_AUDIO").unwrap_or_else(|_| "off".into()));
    let frames = std::env::var("STRESS_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let runs = std::env::var("STRESS_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut renderer = Renderer::headless()?;
    println!(
        "{:<15} {:>7} {:>7} {:>7} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} | {:>6} {:>7} {:>6} {:>8} {:>7}",
        "scenario", "scripts", "physics", "other", "end", "render", "extract", "encode", "gpu", "total",
        "draws", "sprites", "meshes", "tris", "culled"
    );
    for scenario in selected {
        let (s, stats) = measure(scenario, &mut renderer, frames, runs);
        println!(
            "{:<15} {:>7.3} {:>7.3} {:>7.3} {:>6.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} | {:>6} {:>7} {:>6} {:>8} {:>7}",
            scenario.name, s.scripts, s.physics, s.other, s.end_frame, s.render, s.extract, s.encode, s.gpu, s.total,
            stats.draw_calls, stats.sprites, stats.meshes, stats.triangles,
            stats.culled_meshes + stats.culled_sprites
        );
    }
    Ok(())
}
