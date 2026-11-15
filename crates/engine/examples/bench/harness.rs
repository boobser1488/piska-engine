//! Общий стенд замеров кадра (без окна): им пользуются `examples/bench` движка и
//! `examples/bench.rs` игр (подключают этот файл через `#[path]`).
//!
//! Каждый случай (`Case`) строится заново `BENCH_RUNS` раз (по умолчанию 3): загрузка,
//! `BENCH_WARMUP` кадров разгона (по умолчанию 30), затем `BENCH_FRAMES` кадров замера
//! (по умолчанию 150) с шагом 1/60 с в цель 1280×720. Камера — по фиксированному пути
//! случая (или своя у игры), поэтому прогоны повторяемы.
//!
//! На каждый прогон печатается строка `BENCH {json}`: медиана / p95 / p99 времени кадра,
//! медианы фаз (скрипты, физика, прочие подсистемы, конец кадра, обход сцены, извлечение,
//! запись команд, отправка, ожидание GPU), draw call'ы, треугольники, смены состояния,
//! байты загрузки и аллокации за кадр, пик кучи, время загрузки и первого кадра.
//! Сводку и сравнение строит `tools/bench_report.py`.

#![allow(dead_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use engine::prelude::*;
use engine::render::{wgpu, RenderStats, Renderer};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Размер кадра: `BENCH_SIZE=640x360` — для опыта «упирается ли кадр в пиксели».
pub fn size() -> (u32, u32) {
    std::env::var("BENCH_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((WIDTH, HEIGHT))
}

// ---------------------------------------------------------------------------
// Счётчик аллокаций: число вызовов и текущий / пиковый объём кучи
// ---------------------------------------------------------------------------

pub struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: всё делегируется системному аллокатору; счётчики — только атомарные числа.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let out = System.realloc(ptr, layout, new_size);
        if !out.is_null() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            if new_size >= layout.size() {
                let live = LIVE_BYTES.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size - layout.size();
                PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        out
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

pub fn allocations() -> u64 {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// Сбросить пик кучи до текущего объёма (перед новым прогоном).
pub fn reset_peak() {
    PEAK_BYTES.store(LIVE_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn peak_bytes() -> usize {
    PEAK_BYTES.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Случаи
// ---------------------------------------------------------------------------

/// Один замеряемый случай: как собрать игру и что делать перед каждым кадром.
pub struct Case {
    pub name: &'static str,
    pub about: &'static str,
    /// Собрать приложение (загрузка сцены входит в `load_ms`).
    pub build: Box<dyn Fn() -> App>,
    /// Перед каждым кадром (номер кадра от начала прогона, разгон включён):
    /// путь камеры, нажатия бота. `None` — ничего.
    pub step: Option<Box<dyn Fn(&mut App, u32)>>,
}

impl Case {
    pub fn new(name: &'static str, about: &'static str, build: impl Fn() -> App + 'static) -> Self {
        Self { name, about, build: Box::new(build), step: None }
    }

    pub fn with_step(mut self, step: impl Fn(&mut App, u32) + 'static) -> Self {
        self.step = Some(Box::new(step));
        self
    }
}

/// Поставить активную камеру сцены (первую найденную) в `eye`, взглядом на `target`.
pub fn place_camera(app: &mut App, eye: Vec3, target: Vec3) {
    let mut found = None;
    app.world.scene.visit_active_with::<Camera>(|id, _| {
        if found.is_none() {
            found = Some(id);
        }
    });
    if let Some(id) = found {
        if let Some(object) = app.world.scene.get_mut(id) {
            object.transform.position = eye;
            object.transform.look_at(target, Vec3::Y);
        }
    }
}

/// Круговой облёт: за `period` кадров камера обходит точку `center` по кругу радиуса
/// `radius` на высоте `height`, глядя в центр.
pub fn orbit(app: &mut App, frame: u32, center: Vec3, radius: f32, height: f32, period: u32) {
    let angle = frame as f32 / period.max(1) as f32 * std::f32::consts::TAU;
    let eye = center + Vec3::new(angle.cos() * radius, height, angle.sin() * radius);
    place_camera(app, eye, center);
}

// ---------------------------------------------------------------------------
// Замер
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Frame {
    total: f32,
    tick: f32,
    scripts: f32,
    physics: f32,
    other: f32,
    end_frame: f32,
    render: f32,
    walk: f32,
    extract: f32,
    encode: f32,
    submit: f32,
    gpu: f32,
    allocs: f32,
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn percentile(values: &[f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let index = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// Цель кадра 1280×720 в формате рендерера.
pub fn target(renderer: &Renderer) -> wgpu::TextureView {
    renderer
        .device()
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("bench target"),
            size: wgpu::Extent3d { width: size().0, height: size().1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Один прогон случая; возвращает строку JSON для `BENCH`.
fn run_once(case: &Case, renderer: &mut Renderer, view: &wgpu::TextureView, run: u32, warmup: u32, frames: u32) -> String {
    reset_peak();
    let load_clock = Instant::now();
    let mut app = (case.build)();
    // Опыты: `BENCH_QUALITY=low|medium|high` — пресет графики, `BENCH_NO_SHADOWS=1` — без теней.
    if let Ok(q) = std::env::var("BENCH_QUALITY") {
        let level = match q.as_str() {
            "low" => QualityLevel::Low,
            "medium" => QualityLevel::Medium,
            _ => QualityLevel::High,
        };
        app.world.resources.insert(GraphicsSettings::preset(level));
    }
    if let Some(scale) = std::env::var("BENCH_SCALE").ok().and_then(|v| v.parse::<f32>().ok()) {
        let mut g = app.world.resources.get::<GraphicsSettings>().cloned().unwrap_or_default();
        g.render_scale = scale;
        app.world.resources.insert(g);
    }
    // `BENCH_GFX=shadows,sky,ao,outline,lights,fill,lampshadows,water,flatwater` — включить новые эффекты в сцене случая (для замера
    // их цены и для снимков «до / после»).
    if let Ok(list) = std::env::var("BENCH_GFX") {
        let mut look = app.world.scene.build("Bench Look");
        for name in list.split(',') {
            look = match name.trim() {
                "shadows" => look.with(SoftShadows::default()),
                "sky" => look.with(Sky::default()),
                "ao" => look.with(AmbientOcclusion::default()),
                "outline" => look.with(Outline::default()),
                "lights" => look.with(ManyLights::default()),
                "fill" => look.with(FillLight::default()),
                _ => look,
            };
        }
        // `water` / `flatwater` — эффекты (или плоский стиль) у всей воды сцены.
        for (name, stylized) in [("water", false), ("flatwater", true)] {
            if list.split(',').any(|n| n.trim() == name) {
                let mut waters = Vec::new();
                app.world.scene.visit_active_with::<Water>(|id, _| waters.push(id));
                for id in waters {
                    if let Some(object) = app.world.scene.get_mut(id) {
                        object.add(WaterEffects { stylized, ..Default::default() });
                    }
                }
            }
        }
        // `lampshadows` — тени всех точечных ламп (сколько рисуется — решает пресет).
        if list.split(',').any(|n| n.trim() == "lampshadows") {
            let mut lamps = Vec::new();
            app.world.scene.visit_active_with::<Light>(|id, l| {
                if l.kind == LightKind::Point {
                    lamps.push(id);
                }
            });
            for id in lamps {
                if let Some(object) = app.world.scene.get_mut(id) {
                    object.add(LightShadows::default());
                }
            }
        }
    }
    if std::env::var("BENCH_NO_SHADOWS").is_ok() {
        let mut g = app.world.resources.get::<GraphicsSettings>().cloned().unwrap_or_default();
        g.shadows = false;
        app.world.resources.insert(g);
    }
    let load_ms = load_clock.elapsed().as_secs_f32() * 1000.0;
    let mut first_frame_ms = 0.0;
    let mut samples: Vec<Frame> = Vec::with_capacity(frames as usize);
    let mut last = RenderStats::default();
    let dt = 1.0 / 60.0;
    for frame in 0..warmup + frames {
        if let Some(step) = &case.step {
            step(&mut app, frame);
        }
        let allocs_before = allocations();
        let clock = Instant::now();
        app.tick(dt);
        let tick = clock.elapsed().as_secs_f32() * 1000.0;
        let profile = app.profile().clone();
        let render_clock = Instant::now();
        let stats = renderer.render(&app.world, view, size().0, size().1);
        let render = render_clock.elapsed().as_secs_f32() * 1000.0;
        let allocs = (allocations() - allocs_before) as f32;
        let gpu_clock = Instant::now();
        renderer.device().poll(wgpu::Maintain::Wait);
        let gpu = gpu_clock.elapsed().as_secs_f32() * 1000.0;
        let total = clock.elapsed().as_secs_f32() * 1000.0;
        if frame == 0 {
            first_frame_ms = total;
        }
        last = stats;
        if frame < warmup {
            continue;
        }
        let physics = profile.subsystem_ms("physics2d") + profile.subsystem_ms("physics3d");
        let subsystems: f32 = profile.subsystems.iter().map(|(_, ms)| ms).sum();
        samples.push(Frame {
            total,
            tick,
            scripts: profile.scripts_ms,
            physics,
            other: subsystems - physics,
            end_frame: profile.end_frame_ms,
            render,
            walk: stats.walk_ms,
            extract: stats.extract_ms,
            encode: stats.encode_ms,
            submit: stats.submit_ms,
            gpu,
            allocs,
        });
    }
    // `BENCH_SHOT=<каталог>` — снимок последнего кадра (сравнение картинок до / после).
    if let Ok(dir) = std::env::var("BENCH_SHOT") {
        let (w, h) = size();
        if let Ok((image, _)) = renderer.capture_view(&app.world, w, h, None) {
            let _ = std::fs::create_dir_all(&dir);
            let _ = image.save(std::path::Path::new(&dir).join(format!("{}_{run}.png", case.name)));
        }
    }
    let col = |f: fn(&Frame) -> f32| samples.iter().map(f).collect::<Vec<f32>>();
    let med = |f: fn(&Frame) -> f32| percentile(&col(f), 0.5);
    let totals = col(|s| s.total);
    format!(
        concat!(
            "{{\"case\":\"{}\",\"run\":{},\"frames\":{},\"median\":{:.3},\"p95\":{:.3},\"p99\":{:.3},",
            "\"tick\":{:.3},\"scripts\":{:.3},\"physics\":{:.3},\"other\":{:.3},\"end\":{:.3},",
            "\"render\":{:.3},\"walk\":{:.3},\"extract\":{:.3},\"encode\":{:.3},\"submit\":{:.3},\"gpu\":{:.3},",
            "\"draws\":{},\"shadow_draws\":{},\"tris\":{},\"meshes\":{},\"sprites\":{},\"culled\":{},",
            "\"state_changes\":{},\"upload_kb\":{:.1},\"allocs\":{:.0},\"peak_mb\":{:.1},",
            "\"load_ms\":{:.1},\"first_frame_ms\":{:.1},\"objects\":{}}}"
        ),
        case.name,
        run,
        samples.len(),
        percentile(&totals, 0.5),
        percentile(&totals, 0.95),
        percentile(&totals, 0.99),
        med(|s| s.tick),
        med(|s| s.scripts),
        med(|s| s.physics),
        med(|s| s.other),
        med(|s| s.end_frame),
        med(|s| s.render),
        med(|s| s.walk),
        med(|s| s.extract),
        med(|s| s.encode),
        med(|s| s.submit),
        med(|s| s.gpu),
        last.draw_calls,
        last.shadow_draw_calls,
        last.triangles,
        last.meshes,
        last.sprites,
        last.culled_meshes + last.culled_sprites,
        last.state_changes,
        last.upload_bytes as f32 / 1024.0,
        med(|s| s.allocs),
        peak_bytes() as f32 / (1024.0 * 1024.0),
        load_ms,
        first_frame_ms,
        app.world.scene.len(),
    )
}

/// Прогнать выбранные случаи (аргументы командной строки — имена или их начала; пусто —
/// все) на одном устройстве GPU.
pub fn main_with(cases: Vec<Case>) {
    std::env::set_var("ENGINE_AUDIO", "off");
    let wanted: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("--")).collect();
    let selected: Vec<&Case> = cases
        .iter()
        .filter(|c| wanted.is_empty() || wanted.iter().any(|w| c.name.starts_with(w.as_str())))
        .collect();
    if selected.is_empty() {
        eprintln!("unknown case; available:");
        for c in &cases {
            eprintln!("  {:<22} {}", c.name, c.about);
        }
        std::process::exit(2);
    }
    let runs = env_u32("BENCH_RUNS", 3);
    let warmup = env_u32("BENCH_WARMUP", 30);
    let frames = env_u32("BENCH_FRAMES", 150);
    let mut renderer = Renderer::headless().expect("GPU");
    let view = target(&renderer);
    for case in selected {
        for run in 0..runs {
            let line = run_once(case, &mut renderer, &view, run, warmup, frames);
            println!("BENCH {line}");
        }
        eprintln!("bench: {} done", case.name);
    }
}

// ---------------------------------------------------------------------------
// Загрузка сцены по частям (`--load-split`)
// ---------------------------------------------------------------------------

/// Где уходит время до первого кадра: приложение с ассетами (`make_app`: движок,
/// регистрация компонентов, `mount_assets`), чтение текста сцены, разбор RON, создание
/// объектов (полная загрузка минус разбор), устройство GPU с конвейерами, первый кадр
/// (загрузка мешей и текстур на GPU, сборка рельефа) и второй кадр для сравнения.
/// Печатает строки `LOAD {json}` (`BENCH_RUNS` прогонов, свежий процесс — холодный диск
/// кэша ОС уже тёплый после первого прогона).
pub fn load_split(name: &str, make_app: impl Fn() -> App, scene: &str) {
    let runs = env_u32("BENCH_RUNS", 3);
    let ms = |clock: Instant| clock.elapsed().as_secs_f64() * 1000.0;
    let clock = Instant::now();
    let mut renderer = Renderer::headless().expect("GPU");
    let device_ms = ms(clock);
    let view = target(&renderer);
    for run in 0..runs {
        let clock = Instant::now();
        let mut app = make_app();
        let app_ms = ms(clock);
        let clock = Instant::now();
        let text = {
            let db = app.world.resources.get::<AssetDatabase>().expect("assets");
            let guid = db.handle::<engine::assets::SceneAsset>(scene).expect("scene").guid();
            db.read_text(guid).expect("scene text")
        };
        let read_ms = ms(clock);
        let clock = Instant::now();
        let parsed = SceneData::from_ron(&text, scene).expect("scene parses");
        let parse_ms = ms(clock);
        drop(parsed);
        let clock = Instant::now();
        app.world.load_scene_str(&text, scene).expect("scene loads");
        let load_ms = ms(clock);
        let clock = Instant::now();
        app.tick(1.0 / 60.0);
        let tick_ms = ms(clock);
        let clock = Instant::now();
        renderer.render(&app.world, &view, size().0, size().1);
        renderer.device().poll(wgpu::Maintain::Wait);
        let first_ms = ms(clock);
        let clock = Instant::now();
        renderer.render(&app.world, &view, size().0, size().1);
        renderer.device().poll(wgpu::Maintain::Wait);
        let second_ms = ms(clock);
        println!(
            "LOAD {{\"case\":\"{name}\",\"run\":{run},\"device_ms\":{device_ms:.1},\"app_ms\":{app_ms:.1},\"read_ms\":{read_ms:.1},\"parse_ms\":{parse_ms:.1},\"instantiate_ms\":{:.1},\"first_tick_ms\":{tick_ms:.1},\"first_frame_ms\":{first_ms:.1},\"second_frame_ms\":{second_ms:.1},\"objects\":{}}}",
            load_ms - parse_ms,
            app.world.scene.len()
        );
    }
}
