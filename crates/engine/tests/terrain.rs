//! Рельеф: высоты, коллайдер, персонаж на склонах, лучи, сохранение скульпта в сцене,
//! подгрузка чанков с бюджетом, отсутствие щелей между LOD и раскраска — по настоящим
//! пикселям (`Renderer::headless`, все GPU-тесты — под одним замком).

use engine::prelude::*;
use engine_render::{CameraView, PostSettings, Renderer, TerrainSampler};

const DT: f32 = 1.0 / 60.0;

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

/// Холмистый рельеф без моря и гор (чтобы шарам было где лежать).
fn hills(seed: i64, size: f32) -> Terrain {
    Terrain { seed, size, mountains: 0.0, land_bias: 2.0, ..Terrain::default() }
}

fn sampler_of(app: &App, id: GameObjectId) -> TerrainSampler {
    app.world.scene.get_component::<Terrain>(id).unwrap().sampler(None)
}

#[test]
fn balls_rest_on_the_surface_that_height_at_reports() {
    let mut app = engine::app();
    let corner = Vec3::new(-100.0, 5.0, -100.0);
    let terrain = app.spawn("Terrain").at3(corner.x, corner.y, corner.z).with(hills(21, 256.0)).id();
    let mut balls = Vec::new();
    for k in 0..25 {
        let (x, z) = ((k % 5) as f32 * 40.0 - 70.0, (k / 5) as f32 * 40.0 - 70.0);
        let ground = terrain_height(&app.world, x, z).expect("inside the terrain");
        let id = app
            .spawn(format!("Ball {k}"))
            .at3(x, ground + 2.0, z)
            .with(Rigidbody { linear_damping: 2.0, angular_damping: 2.0, ..Rigidbody::default() })
            .with(Collider::sphere(0.5).with_material(1.0, 0.0))
            .id();
        balls.push(id);
    }
    app.run_headless(240, DT);

    let sampler = sampler_of(&app, terrain);
    for id in balls {
        let p = app.world.scene.world_position(id);
        let ground = terrain_height(&app.world, p.x, p.z).unwrap();
        let normal = sampler.normal_local(p.x - corner.x, p.z - corner.z);
        // Шар на склоне касается земли не под центром: центр выше на r / cos(уклона).
        let expected = ground + 0.5 / normal.y.max(0.3);
        assert!((p.y - expected).abs() < 0.12, "ball at {p}: expected y ≈ {expected} (ground {ground})");
    }
}

#[test]
fn physics_raycast_matches_terrain_height() {
    let mut app = engine::app();
    let terrain = app.spawn("Terrain").at3(0.0, -3.0, 0.0).with(hills(5, 192.0)).id();
    // Коллайдер строится перед шагом физики (все плитки — бюджет бесконечный).
    let mut system = engine::terrain_physics::TerrainPhysicsSystem::default();
    system.sync(&mut app.world, f64::INFINITY);
    assert_eq!(system.tile_count(terrain), 9, "3 × 3 chunks of 64 m");
    let physics = app.world.resources.get::<Physics3D>().unwrap();
    for k in 0..40 {
        let (x, z) = (3.1 + k as f32 * 4.63 % 185.0, 7.7 + (k * 37 % 180) as f32);
        let hit = physics.raycast(Vec3::new(x, 500.0, z), Vec3::NEG_Y, 1000.0, None).expect("the ray hits the terrain");
        assert_eq!(hit.object, terrain);
        let expected = terrain_height(&app.world, x, z).unwrap();
        assert!((hit.point.y - expected).abs() < 0.02, "({x}, {z}): collider {} vs height {expected}", hit.point.y);
        // Луч без физики (редактор, скрипты) — то же место.
        let own = terrain_raycast(&app.world, Vec3::new(x, 500.0, z), Vec3::NEG_Y, 1000.0).unwrap();
        assert!((own.point.y - expected).abs() < 0.02);
    }
    assert!(physics.raycast(Vec3::new(-50.0, 500.0, -50.0), Vec3::NEG_Y, 1000.0, None).is_none());
    // Те же запросы доступны C++ через сервисы фасада.
    let services = engine::cpp::services_mut(&mut app.world);
    assert!(services.contains("terrain.height") && services.contains("terrain.normal"));
}

#[test]
fn character_climbs_a_gentle_slope_and_stops_at_a_steep_one() {
    let mut app = engine::app();
    // Плоская земля (высоты × 0) и два склона, сделанных правкой высот.
    let mut terrain = Terrain { size: 128.0, height_scale: 0.0, ..Terrain::default() };
    // x 40..80: подъём 20°; z ≥ 64 — там же стена 65°.
    terrain.modify_heights(Vec2::ZERO, Vec2::splat(128.0), None, |p, _| {
        let ramp = (p.x - 40.0).clamp(0.0, 40.0) * 20f32.to_radians().tan();
        if p.y < 64.0 {
            ramp
        } else {
            (p.x - 40.0).clamp(0.0, 20.0) * 65f32.to_radians().tan()
        }
    });
    app.spawn("Terrain").with(terrain);
    let walker = |app: &mut App, z: f32| {
        app.spawn("Walker")
            .at3(30.0, 0.1, z)
            .with(CharacterController { velocity: Vec3::new(4.0, 0.0, 0.0), ..CharacterController::default() })
            .id()
    };
    let gentle = walker(&mut app, 32.0);
    let steep = walker(&mut app, 96.0);
    for _ in 0..600 {
        for id in [gentle, steep] {
            if let Some(c) = app.world.scene.get_component_mut::<CharacterController>(id) {
                c.velocity.x = 4.0;
            }
        }
        app.run_headless(1, DT);
    }
    let (a, b) = (app.world.scene.world_position(gentle), app.world.scene.world_position(steep));
    assert!(a.x > 60.0 && a.y > 6.0, "walked up the 20° slope: {a}");
    let on_slope = terrain_height(&app.world, a.x, a.z).unwrap();
    assert!((a.y - on_slope).abs() < 0.4, "stays on the ground: {a} vs {on_slope}");
    assert!(b.x < 45.0 && b.y < 3.0, "a 65° wall stops the character: {b}");
}

#[test]
fn sculpting_survives_a_scene_round_trip() {
    let mut app = engine::app();
    let mut terrain = hills(8, 128.0);
    let brush = TerrainBrush { tool: BrushTool::Raise, radius: 9.0, strength: 1.0, falloff: 0.5, ..TerrainBrush::default() };
    for _ in 0..20 {
        terrain.apply_brush(&brush, 60.0, 70.0, 0.05, None);
    }
    let paint = TerrainBrush { tool: BrushTool::Paint, layer: Some(2), ..brush };
    terrain.apply_brush(&paint, 30.0, 30.0, 0.5, None);
    let raised = terrain.height_local(60.0, 70.0);
    app.spawn("Terrain").with(terrain);
    let text = app.world.scene_to_string();
    assert!(text.contains("Terrain") && text.contains("edits"), "the terrain is in the scene file");

    let mut loaded = engine::app();
    loaded.world.load_scene_str(&text, "terrain.scene").unwrap();
    let id = loaded.world.scene.find_by_name("Terrain").unwrap();
    let back = loaded.world.scene.get_component::<Terrain>(id).unwrap();
    assert!((back.height_local(60.0, 70.0) - raised).abs() < 2e-3);
    let sampler = back.sampler(None);
    let sp = back.layout().spacing();
    let p = sampler.paint_at_sample((30.0 / sp) as i32, (30.0 / sp) as i32);
    assert!(p[2] > 100, "painted rock survives: {p:?}");
}

#[test]
fn the_shipped_scene_loads_and_its_ball_lands_on_the_plaza() {
    let mut app = engine::app();
    let text = include_str!("../../../assets/scenes/terrain.scene");
    app.world.load_scene_str(text, "scenes/terrain.scene").unwrap();
    let id = app.world.scene.find_by_name("Terrain").unwrap();
    assert_eq!(app.world.scene.get_component::<Terrain>(id).unwrap().extent(), 2048.0);
    let ball = app.world.scene.find_by_name("Ball").unwrap();
    app.run_headless(540, DT);
    let p = app.world.scene.world_position(ball);
    let ground = terrain_height(&app.world, p.x, p.z).unwrap();
    assert!((p.y - 0.5 - ground).abs() < 0.05, "the ball rests on the flat plaza: {p}, ground {ground}");
}

/// Вид прямо сверху на точку `at` с высоты `height`.
fn top_view(at: Vec3, height: f32, clear: Color) -> CameraView {
    let eye = at + Vec3::Y * height;
    let projection = Mat4::perspective_rh(50f32.to_radians(), 1.0, 1.0, 5000.0);
    let view = Mat4::look_at_rh(eye, at, Vec3::NEG_Z);
    CameraView {
        view_proj: projection * view,
        position: eye,
        clear_color: clear,
        sky: false,
        post: Some(PostSettings::OFF),
        culling_mask: u32::MAX,
        overlay: false,
        grid: None,
    }
}

/// Пиксели цвета фона.
fn background_pixels(image: &image::RgbaImage) -> usize {
    image.pixels().filter(|p| p[0] > 220 && p[1] < 40 && p[2] > 220).count()
}

#[test]
fn no_cracks_between_chunks_and_lod_levels() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = engine::app();
    // Горы — крутые склоны дают самые большие перепады между уровнями.
    let terrain = Terrain { seed: 123, size: 2048.0, lod_distance: 0.25, ..Terrain::default() };
    app.spawn("Terrain").at3(-1024.0, 0.0, -1024.0).with(terrain);
    let magenta = Color::rgb(1.0, 0.0, 1.0);
    for (at, height) in [(Vec3::new(0.0, 0.0, 0.0), 900.0), (Vec3::new(300.0, 0.0, -200.0), 600.0)] {
        let view = top_view(at, height, magenta);
        let (image, _) = renderer.capture_view(&app.world, 320, 320, Some(&view)).unwrap();
        let stats = renderer.terrain_stats();
        let levels = stats.lod_counts.iter().filter(|&&n| n > 0).count();
        assert!(levels >= 3, "the view crosses several LOD boundaries: {:?}", stats.lod_counts);
        assert_eq!(background_pixels(&image), 0, "no background shows through the terrain ({:?})", stats.lod_counts);
    }
}

#[test]
fn streaming_loads_around_a_moving_camera_within_budget() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    renderer.set_terrain_blocking(false);
    let mut app = engine::app();
    let terrain = Terrain { seed: 2, size: 2048.0, view_distance: 300.0, ..Terrain::default() };
    app.spawn("Terrain").at3(-1024.0, 0.0, -1024.0).with(terrain);
    let clear = Color::rgb(0.2, 0.2, 0.2);

    let mut max_uploads = 0;
    let mut frames_to_settle = 0;
    // Камера стоит: всё нужное догружается за несколько кадров, не больше 16 за кадр.
    for frame in 0..400 {
        renderer.capture_view(&app.world, 64, 64, Some(&top_view(Vec3::ZERO, 400.0, clear))).unwrap();
        let stats = renderer.terrain_stats();
        max_uploads = max_uploads.max(stats.uploads);
        if stats.pending == 0 && frame > 2 && stats.uploads == 0 {
            frames_to_settle = frame;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let settled = renderer.terrain_stats();
    assert!(frames_to_settle > 1, "chunks arrive over several frames");
    assert!(max_uploads <= 16, "per-frame upload budget: {max_uploads}");
    // Чанки 64 м в радиусе 300 м: круг ≈ 70–90 чанков.
    assert!((50..=110).contains(&settled.chunks_loaded), "{settled:?}");

    // Полёт на 1.2 км: дальние чанки выгружаются, рядом всё снова загружено.
    for step in 0..120 {
        let at = Vec3::new(step as f32 * 10.0, 0.0, 0.0);
        renderer.capture_view(&app.world, 64, 64, Some(&top_view(at, 400.0, clear))).unwrap();
        let stats = renderer.terrain_stats();
        assert!(stats.uploads <= 16);
        assert!(stats.chunks_loaded <= 130, "far chunks are released: {stats:?}");
    }
    for _ in 0..400 {
        renderer.capture_view(&app.world, 64, 64, Some(&top_view(Vec3::new(1190.0, 0.0, 0.0), 400.0, clear))).unwrap();
        if renderer.terrain_stats().pending == 0 && renderer.terrain_stats().uploads == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let end = renderer.terrain_stats();
    // У края рельефа (x до 1024) видна только часть круга.
    assert!(end.pending == 0 && (15..=110).contains(&end.chunks_loaded), "{end:?}");
}

#[test]
fn painting_changes_the_rendered_colour() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = engine::app();
    let id = app.spawn("Terrain").with(hills(4, 256.0)).id();
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.3, -1.2, 0.0)).with(Light::default());
    let at = Vec3::new(128.0, terrain_height(&app.world, 128.0, 128.0).unwrap(), 128.0);
    let view = top_view(at, 60.0, Color::BLACK);
    let mean = |image: &image::RgbaImage| {
        let (mut sum, mut n) = ([0.0f32; 3], 0.0);
        for y in 24..40 {
            for x in 24..40 {
                let p = image.get_pixel(x, y);
                for k in 0..3 {
                    sum[k] += p[k] as f32;
                }
                n += 1.0;
            }
        }
        sum.map(|s| s / n)
    };
    let (before, _) = renderer.capture_view(&app.world, 64, 64, Some(&view)).unwrap();
    let snow = TerrainBrush { tool: BrushTool::Paint, radius: 30.0, strength: 1.0, falloff: 0.2, layer: Some(4), ..TerrainBrush::default() };
    if let Some(terrain) = app.world.scene.get_component_mut::<Terrain>(id) {
        for _ in 0..10 {
            terrain.apply_brush(&snow, 128.0, 128.0, 0.2, None);
        }
    }
    let (after, _) = renderer.capture_view(&app.world, 64, 64, Some(&view)).unwrap();
    if let Ok(dir) = std::env::var("TERRAIN_SHOTS") {
        before.save(format!("{dir}/paint_before.png")).unwrap();
        after.save(format!("{dir}/paint_after.png")).unwrap();
    }
    let (a, b) = (mean(&before), mean(&after));
    let change: f32 = (0..3).map(|k| (a[k] - b[k]).abs()).sum();
    // Снег светлее и голубее песка и травы.
    assert!(change > 60.0 && b[2] - a[2] > 30.0 && b[2] > b[1], "painted snow changes the colour: {a:?} → {b:?}");
}

/// Замер: рельеф 2 × 2 км из примера, дальность по умолчанию, 1280 × 720, камера летит
/// 30 м/с. Время кадра — с ожиданием GPU. Запуск:
/// `cargo test -p engine --test terrain perf_terrain -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf_terrain() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = engine::app();
    let terrain = Terrain { seed: 99, size: 2048.0, view_distance: 1600.0, ..Terrain::default() };
    app.spawn("Terrain").at3(-1024.0, 0.0, -1024.0).with(terrain);
    app.spawn("Sea").with(Water::default());
    app.spawn("Environment").with(Environment { fog_density: 0.0005, shadow_distance: 180.0, ..Environment::default() });
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, -0.5, -0.7, 0.0)).with(Light::default());
    let (w, h) = (1280, 720);
    let target = renderer.device().create_texture(&engine_render::wgpu::TextureDescriptor {
        label: Some("perf target"),
        size: engine_render::wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: engine_render::wgpu::TextureDimension::D2,
        format: engine_render::wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: engine_render::wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = target.create_view(&Default::default());
    let view_at = |t: f32| {
        let eye = Vec3::new(-260.0 + t * 30.0, 95.0, 420.0 - t * 12.0);
        CameraView::look_at(eye, eye + Vec3::new(0.62, -0.12, -0.78), 60.0, w as f32 / h as f32, Color::BLACK, true)
    };
    let frame = |renderer: &mut Renderer, t: f32| {
        let mut view = view_at(t);
        view.view_proj = Mat4::perspective_rh(60f32.to_radians(), w as f32 / h as f32, 0.1, 4000.0)
            * Mat4::look_at_rh(view.position, view.position + Vec3::new(0.62, -0.12, -0.78), Vec3::Y);
        let clock = std::time::Instant::now();
        let stats = renderer.render_with(&app.world, &target, w, h, &view);
        renderer.device().poll(engine_render::wgpu::Maintain::Wait);
        (clock.elapsed().as_secs_f64() * 1000.0, stats)
    };

    // Первая загрузка всего видимого (сразу, в одном кадре).
    renderer.set_terrain_blocking(true);
    let (first_ms, _) = frame(&mut renderer, 0.0);
    let loaded = renderer.terrain_stats();
    println!(
        "initial load: {first_ms:.0} ms for {} chunks ({:.2} ms build per chunk summed over threads), {:.1} MB vertex pools",
        loaded.chunks_loaded,
        loaded.build_ms / loaded.chunks_loaded.max(1) as f32,
        loaded.gpu_bytes as f64 / 1048576.0
    );

    // Полёт: подгрузка в фоне, бюджет на кадр.
    renderer.set_terrain_blocking(false);
    let mut times = Vec::new();
    let (mut tris, mut calls, mut drawn, mut uploads) = (0u64, 0u64, 0u64, 0u32);
    for i in 0..900 {
        let (ms, stats) = frame(&mut renderer, i as f32 / 60.0);
        let t = renderer.terrain_stats();
        times.push(ms);
        tris += t.triangles;
        calls += stats.draw_calls as u64;
        drawn += t.chunks_drawn as u64;
        uploads = uploads.max(t.uploads);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let n = times.len() as f64;
    let mean = times.iter().sum::<f64>() / n;
    let end = renderer.terrain_stats();
    println!(
        "flight 900 frames: mean {mean:.2} ms ({:.0} FPS), median {:.2} ms, p99 {:.2} ms, worst {:.2} ms",
        1000.0 / mean,
        times[times.len() / 2],
        times[(n * 0.99) as usize],
        times[times.len() - 1]
    );
    println!(
        "per frame: {:.0} terrain triangles, {:.0} draw calls (all passes), {:.0} chunks drawn; max uploads {uploads}; at end {} chunks loaded, {:.1} MB",
        tris as f64 / n,
        calls as f64 / n,
        drawn as f64 / n,
        end.chunks_loaded,
        end.gpu_bytes as f64 / 1048576.0
    );
}
