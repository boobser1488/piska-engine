//! Стенд замеров движка без окна: сцены витрины, `terrain.scene`, синтетические сцены
//! (10 000 спрайтов, 5 000 мешей на 50 материалах, 200 источников света) и редактор.
//!
//! ```text
//! cargo run --example bench                       # все случаи, строки `BENCH {json}`
//! cargo run --example bench -- showcase_valley    # только выбранные (по началу имени)
//! BENCH_RUNS=1 cargo run --example bench -- --editor-idle   # редактор с level1.scene
//! BENCH_RUNS=1 cargo run --example bench -- --editor-5k     # редактор, сцена на 5 000 объектов
//! BENCH_RUNS=3 BENCH_WARMUP=30 BENCH_FRAMES=150 … # прогоны, разгон, кадры замера
//! python tools/bench_report.py out.txt [before.txt] # таблица (и сравнение «до / после»)
//! ```
//!
//! Игры меряются своими `examples/bench.rs` (тот же стенд, `harness.rs`).

#[path = "../showcase/main.rs"]
#[allow(dead_code)]
mod showcase;

mod harness;

use std::path::PathBuf;
use std::time::Instant;

use engine::prelude::*;
use harness::{orbit, place_camera, Case};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn assets() -> PathBuf {
    PathBuf::from(format!("{ROOT}/assets"))
}

/// Текстуры синтетических сцен: несколько цветных квадратов во временном каталоге.
fn synthetic_assets() -> PathBuf {
    let dir = std::env::temp_dir().join("piska_bench_assets");
    std::fs::create_dir_all(&dir).expect("temp dir");
    for index in 0..5u8 {
        let path = dir.join(format!("tex{index}.png"));
        if !path.exists() {
            let (r, g, b) = (40 + index * 40, 200 - index * 30, 90 + index * 25);
            image::RgbaImage::from_fn(32, 32, |x, y| {
                if x < 2 || y < 2 || x > 29 || y > 29 {
                    image::Rgba([20, 20, 20, 255])
                } else {
                    image::Rgba([r, g, b, 255])
                }
            })
            .save(&path)
            .expect("write texture");
        }
    }
    dir
}

fn synthetic_app() -> App {
    let mut app = engine::app();
    engine::mount_assets(&mut app, synthetic_assets()).expect("bench assets");
    app
}

fn texture(app: &App, index: usize) -> AssetHandle<Texture> {
    app.world.resources.get::<AssetDatabase>().expect("assets").handle(&format!("tex{index}.png")).expect("texture")
}

fn sun(app: &mut App) {
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0)).with(Light::default());
}

fn sprites_10k() -> App {
    let mut app = synthetic_app();
    app.spawn("Camera").with(Camera { clear_color: Color::hex(0x1b1f2a), ..Camera::orthographic(30.0) });
    let textures: Vec<_> = (0..4).map(|i| texture(&app, i)).collect();
    let side = 100;
    for i in 0..10_000 {
        let (x, y) = ((i % side) as f32, (i / side) as f32);
        app.spawn("Sprite")
            .at(x * 0.6 - 30.0, y * 0.6 - 30.0)
            .with(SpriteRenderer::textured(textures[i % 4]).with_size(0.5, 0.5));
    }
    app
}

fn sun_view() -> App {
    let mut app = synthetic_app();
    let sun = Vec3::new(0.2, -0.22, 1.0).normalize();
    app.spawn("Camera").at3(0.0, 3.0, 12.0).rotation(Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(-0.2, 0.18, -1.0).normalize())).with(Camera::perspective(60.0));
    app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, sun)).with(Light::directional(2.4));
    app.spawn("Ground").scale(300.0, 1.0, 300.0).with(MeshRenderer::plane().with_color(Color::hex(0x7f9a5a)).without_shadows());
    for i in 0..60 {
        let (x, z) = ((i % 10) as f32 * 4.0 - 18.0, (i / 10) as f32 * -6.0);
        app.spawn("Box").at3(x, 1.0, z).scale(1.5, 2.0, 1.5).with(MeshRenderer::cube().with_color(Color::hex(0xc8a070)));
    }
    app
}

fn plain_10k() -> App {
    let mut app = synthetic_app();
    app.spawn("Camera").with(Camera { clear_color: Color::hex(0x1b1f2a), ..Camera::orthographic(30.0) });
    for i in 0..10_000 {
        app.spawn("Plain").at((i % 100) as f32 * 0.6 - 30.0, (i / 100) as f32 * 0.6 - 30.0);
    }
    app
}

fn meshes_5k() -> App {
    let mut app = synthetic_app();
    app.spawn("Camera").at3(0.0, 25.0, 60.0).with(Camera::perspective(60.0));
    sun(&mut app);
    app.spawn("Ground").scale(300.0, 1.0, 300.0).with(MeshRenderer::plane().with_color(Color::hex(0x8a8f7c)).without_shadows());
    let textures: Vec<_> = (0..5).map(|i| texture(&app, i)).collect();
    if app.world.resources.get::<Materials>().is_none() {
        app.world.resources.insert(Materials::default());
    }
    let handles: Vec<AssetHandle<Material>> = {
        let materials = app.world.resources.get_mut::<Materials>().expect("materials");
        (0..50)
            .map(|i| {
                let shade = i as f32 / 49.0;
                materials.add(
                    Material::new()
                        .color(Color::rgb(0.3 + 0.7 * shade, 0.8 - 0.5 * shade, 0.5))
                        .texture(textures[i % 5])
                        .roughness(0.3 + 0.6 * ((i / 5) as f32 / 9.0))
                        .metallic(if i % 7 == 0 { 0.8 } else { 0.0 }),
                )
            })
            .collect()
    };
    let side = 71;
    for i in 0..5_000 {
        let (x, z) = ((i % side) as f32, (i / side) as f32);
        let mesh = match i % 3 {
            0 => MeshRenderer::cube(),
            1 => MeshRenderer::sphere(),
            _ => MeshRenderer::cylinder(),
        };
        app.spawn("Prop")
            .at3(x * 2.5 - side as f32 * 1.25, 0.5, z * -2.5 + 40.0)
            .with(mesh.with_material_asset(handles[i % 50]));
    }
    app
}

fn lights_200() -> App {
    let mut app = synthetic_app();
    app.spawn("Camera").at3(0.0, 20.0, 40.0).with(Camera::perspective(60.0));
    app.spawn("Moon").rotation(Quat::from_euler(EulerRot::YXZ, 0.3, -0.6, 0.0)).with(Light::directional(0.15));
    app.spawn("Ground").scale(120.0, 1.0, 120.0).with(MeshRenderer::plane().with_color(Color::hex(0x70757a)).without_shadows());
    for i in 0..400 {
        let (x, z) = ((i % 20) as f32, (i / 20) as f32);
        app.spawn("Pillar")
            .at3(x * 5.0 - 47.5, 1.0, z * 5.0 - 47.5)
            .scale(0.8, 2.0, 0.8)
            .with(MeshRenderer::cube().with_color(Color::hex(0xc8c0b0)));
    }
    let colors = [0xffb347, 0x6fcf97, 0x4c8dff, 0xeb5757, 0xf2c94c];
    for i in 0..200 {
        let (x, z) = ((i % 20) as f32, (i / 20) as f32);
        app.spawn("Lamp")
            .at3(x * 5.0 - 45.0, 1.5, z * 10.0 - 45.0)
            .with(Light::point(Color::hex(colors[i % 5]), 6.0, 7.0));
    }
    app
}

fn showcase_case(scene: &'static str) -> impl Fn() -> App {
    move || {
        std::env::set_var("SHOWCASE_FRAMES", "0");
        showcase::load_showcase(&assets(), scene, "hero").expect("showcase scene")
    }
}

fn terrain_scene() -> App {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets()).expect("assets");
    engine::load_scene(&mut app, "scenes/terrain.scene").expect("terrain.scene");
    app
}

fn cases() -> Vec<Case> {
    vec![
        Case::new("showcase_island", "showcase island, hero shot", showcase_case("island")),
        Case::new("showcase_clearing", "showcase clearing (night), hero shot", showcase_case("clearing")),
        Case::new("showcase_workshop", "showcase workshop diorama, hero shot", showcase_case("workshop")),
        Case::new("showcase_valley", "showcase valley (thousands of trees), hero shot", showcase_case("valley")),
        Case::new("terrain_scene", "assets/scenes/terrain.scene, scene camera", terrain_scene),
        Case::new("sprites_10k", "10 000 sprites, 4 textures, panning camera", sprites_10k).with_step(|app, frame| {
            let x = (frame as f32 * 0.02).sin() * 10.0;
            place_camera(app, Vec3::new(x, 0.0, 10.0), Vec3::new(x, 0.0, 0.0));
        }),
        // Солнце в кадре: снимки неба (`BENCH_SHOT`, `BENCH_GFX=sky`).
        Case::new("sun_view", "low sun in frame over a field of boxes", sun_view),
        // Разбор цены обхода спрайтов: пустые объекты (только обход) и те же 10 000 спрайтов,
        // целиком за кадром (матрица, углы, отсечение — без квадов).
        Case::new("walk_plain_10k", "10 000 objects without components (scene walk only)", plain_10k),
        Case::new("sprites_10k_offscreen", "10 000 sprites, all outside the camera", sprites_10k).with_step(|app, _| {
            place_camera(app, Vec3::new(500.0, 500.0, 10.0), Vec3::new(500.0, 500.0, 0.0));
        }),
        Case::new("meshes_5k_50mat", "5 000 meshes (3 shapes), 50 materials, sun shadows, orbit", meshes_5k)
            .with_step(|app, frame| orbit(app, frame, Vec3::new(0.0, 0.0, -45.0), 70.0, 30.0, 600)),
        Case::new("lights_200", "200 point lights, 400 pillars, orbit", lights_200)
            .with_step(|app, frame| orbit(app, frame, Vec3::ZERO, 45.0, 20.0, 600)),
    ]
}

// ---------------------------------------------------------------------------
// Редактор: время кадра интерфейса + сцены (без окна), отдельным процессом
// ---------------------------------------------------------------------------

fn editor_case(name: &str, objects: usize) {
    use engine::editor::{Editor, EditorConfig};
    let runs = std::env::var("BENCH_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(3u32);
    let frames = std::env::var("BENCH_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(150u32);
    let warmup = 30u32;
    for run in 0..runs {
        harness::reset_peak();
        let load = Instant::now();
        let mut app = engine::app();
        engine::mount_assets(&mut app, assets()).expect("assets");
        let mut editor = if objects == 0 {
            Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).expect("editor")
        } else {
            app.spawn("Camera").at3(0.0, 30.0, 80.0).with(Camera::perspective(60.0));
            sun(&mut app);
            let parent = app.spawn("Props").id();
            for i in 0..objects {
                app.spawn(&format!("Prop {i}"))
                    .at3((i % 70) as f32 * 2.0 - 70.0, 0.5, (i / 70) as f32 * -2.0)
                    .with(MeshRenderer::cube().with_color(Color::hex(0x6fcf97)))
                    .child_of(parent);
            }
            Editor::new(app, EditorConfig::default()).expect("editor")
        };
        let load_ms = load.elapsed().as_secs_f32() * 1000.0;
        let mut times: Vec<f32> = Vec::new();
        let mut allocs: Vec<f32> = Vec::new();
        let mut last = Instant::now();
        let mut last_allocs = harness::allocations();
        let _ = engine::editor::capture_with_input(&mut editor, harness::WIDTH, harness::HEIGHT, warmup + frames, |frame, _editor| {
            let now = Instant::now();
            let a = harness::allocations();
            if frame > warmup {
                times.push(now.duration_since(last).as_secs_f32() * 1000.0);
                allocs.push((a - last_allocs) as f32);
            }
            last = now;
            last_allocs = a;
            Vec::new()
        });
        // Средние по последним кадрам (профайлер редактора): игра, сцена, интерфейс, тесселяция, отправка.
        let t = editor.frame_timings();
        let pct = |v: &[f32], p: f32| {
            let mut s = v.to_vec();
            s.sort_by(f32::total_cmp);
            s.get(((s.len().max(1) - 1) as f32 * p).round() as usize).copied().unwrap_or(0.0)
        };
        println!(
            "BENCH {{\"case\":\"{name}\",\"run\":{run},\"frames\":{},\"median\":{:.3},\"p95\":{:.3},\"p99\":{:.3},\"tick\":{:.3},\"render\":{:.3},\"other\":{:.3},\"encode\":{:.3},\"submit\":{:.3},\"allocs\":{:.0},\"peak_mb\":{:.1},\"load_ms\":{:.1},\"objects\":{}}}",
            times.len(),
            pct(&times, 0.5),
            pct(&times, 0.95),
            pct(&times, 0.99),
            t.update_ms,
            t.render_ms,
            t.ui_ms,
            t.tessellate_ms,
            t.submit_ms,
            pct(&allocs, 0.5),
            harness::peak_bytes() as f32 / (1024.0 * 1024.0),
            load_ms,
            editor.app.world.scene.len(),
        );
    }
    eprintln!("bench: {name} done");
}

fn main() {
    std::env::set_var("ENGINE_AUDIO", "off");
    // Редактор создаёт своё устройство GPU на каждый прогон: по одному случаю на процесс
    // (драйвер не любит много устройств в одном процессе), прогоны — `BENCH_RUNS`.
    if std::env::args().any(|a| a == "--editor-idle") {
        editor_case("editor_idle", 0);
        return;
    }
    if std::env::args().any(|a| a == "--editor-5k") {
        editor_case("editor_5k", 5_000);
        return;
    }
    // Загрузка сцены по частям: `-- --load-split` (витрина «долина» и `terrain.scene`).
    if std::env::args().any(|a| a == "--load-split") {
        std::env::set_var("SHOWCASE_FRAMES", "0");
        let showcase_app = || {
            let mut app = engine::app();
            showcase::tour::register(&mut app);
            engine::mount_assets(&mut app, assets()).expect("assets");
            app
        };
        harness::load_split("showcase_valley", showcase_app, &showcase::scenes::scene_path("valley"));
        let terrain_app = || {
            let mut app = engine::app();
            engine::mount_assets(&mut app, assets()).expect("assets");
            app
        };
        harness::load_split("terrain_scene", terrain_app, "scenes/terrain.scene");
        return;
    }
    harness::main_with(cases());
}
