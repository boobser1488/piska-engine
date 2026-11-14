//! Артефакты рендера по настоящим пикселям: одиночные «горячие» (белые) пиксели на
//! рельефе и в сценах. Все GPU-тесты — под одним замком.

use engine::prelude::*;
use engine_render::{CameraView, Renderer};

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn luma(p: &image::Rgba<u8>) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

/// Одиночные горячие пиксели: намного ярче медианы окрестности 5 × 5 и почти без таких же
/// ярких соседей (блик на ребре, «светлячок»). Возвращает координаты.
fn hot_pixels(image: &image::RgbaImage) -> Vec<(u32, u32)> {
    let (w, h) = image.dimensions();
    let lum: Vec<f32> = image.pixels().map(luma).collect();
    let at = |x: u32, y: u32| lum[(y * w + x) as usize];
    let mut out = Vec::new();
    let mut around = Vec::with_capacity(24);
    for y in 2..h.saturating_sub(2) {
        for x in 2..w.saturating_sub(2) {
            let l = at(x, y);
            if l < 150.0 {
                continue;
            }
            around.clear();
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    if dx != 0 || dy != 0 {
                        around.push(at((x as i32 + dx) as u32, (y as i32 + dy) as u32));
                    }
                }
            }
            around.sort_by(|a, b| a.total_cmp(b));
            let median = around[12];
            if l - median < 70.0 {
                continue;
            }
            let mut bright = 0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if (dx != 0 || dy != 0) && at((x as i32 + dx) as u32, (y as i32 + dy) as u32) > median + 35.0 {
                        bright += 1;
                    }
                }
            }
            // Одиночный пиксель или пара; линия (тонкая полоска воды, край) — не «светлячок».
            let mut brightest = 0.0f32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx != 0 || dy != 0 {
                        brightest = brightest.max(at((x as i32 + dx) as u32, (y as i32 + dy) as u32));
                    }
                }
            }
            if bright <= 1 && l >= brightest {
                out.push((x, y));
            }
        }
    }
    out
}

fn dump_dir() -> Option<String> {
    std::env::var("RENDER_FIX_SHOTS").ok()
}

/// Сохранить кадр и увеличенные вырезки вокруг найденных пикселей (для разбора).
fn dump(image: &image::RgbaImage, name: &str, hot: &[(u32, u32)]) {
    let Some(dir) = dump_dir() else { return };
    image.save(format!("{dir}/{name}.png")).unwrap();
    for (k, &(x, y)) in hot.iter().take(4).enumerate() {
        let (x0, y0) = (x.saturating_sub(12).min(image.width() - 25), y.saturating_sub(12).min(image.height() - 25));
        let crop = image::imageops::crop_imm(image, x0, y0, 25, 25).to_image();
        let big = image::imageops::resize(&crop, 200, 200, image::imageops::FilterType::Nearest);
        big.save(format!("{dir}/{name}_hot{k}.png")).unwrap();
    }
}

fn perspective(eye: Vec3, target: Vec3, w: u32, h: u32) -> CameraView {
    let mut view = CameraView::look_at(eye, target, 60.0, w as f32 / h as f32, Color::BLACK, true);
    view.view_proj = engine_render::perspective(60f32.to_radians(), w as f32 / h as f32, 0.1, 4000.0) * Mat4::look_at_rh(eye, target, Vec3::Y);
    // Пост-обработка — как в игре (из Environment сцены).
    view.post = None;
    view
}

fn set_sun(app: &mut App, pitch_deg: f32, yaw_deg: f32) {
    let id = app.world.scene.find_by_name("Sun").expect("scene has a Sun");
    if let Some(sun) = app.world.scene.get_mut(id) {
        sun.transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw_deg.to_radians(), pitch_deg.to_radians(), 0.0);
    }
}

/// Виды на рельеф `terrain.scene`: разные места, высоты, направления и высоты солнца.
fn terrain_views(app: &App, w: u32, h: u32) -> Vec<(String, CameraView)> {
    let g = |x: f32, z: f32| terrain_height(&app.world, x, z).unwrap_or(0.0).max(0.0);
    let mut views = Vec::new();
    let spots = [(-60.0, 160.0), (300.0, -250.0), (-500.0, -400.0), (520.0, 480.0), (40.0, -40.0), (-750.0, 300.0)];
    for (k, &(x, z)) in spots.iter().enumerate() {
        for (j, &(above, pitch)) in [(2.5f32, -0.08f32), (25.0, -0.25), (140.0, -0.35)].iter().enumerate() {
            let yaw = k as f32 * 1.7 + j as f32 * 2.3;
            let eye = Vec3::new(x, g(x, z) + above, z);
            let dir = Vec3::new(yaw.cos(), pitch, yaw.sin()).normalize();
            views.push((format!("t{k}_{j}"), perspective(eye, eye + dir, w, h)));
        }
    }
    views
}

fn terrain_app() -> App {
    let mut app = engine::app();
    app.world.load_scene_str(include_str!("../../../assets/scenes/terrain.scene"), "scenes/terrain.scene").unwrap();
    app
}


/// Низкие виды: у берега и на вершинах, четыре стороны света. Глаз выше всего рельефа в
/// радиусе 6 м — камера не оказывается внутри склона.
fn low_views(app: &App, w: u32, h: u32) -> Vec<(String, CameraView)> {
    let g = |x: f32, z: f32| terrain_height(&app.world, x, z).unwrap_or(-50.0);
    let mut low = Vec::new();
    let mut k = 0;
    'scan: for iz in 0..40 {
        for ix in 0..40 {
            let (x, z) = (-950.0 + ix as f32 * 48.7, -950.0 + iz as f32 * 47.3);
            let y = g(x, z);
            let shore = (0.2..2.5).contains(&y);
            let high = y > 180.0;
            if (shore && k % 3 == 0) || (high && k % 5 == 0) {
                for (j, yaw) in [0.0f32, 1.6, 3.14, 4.7].iter().enumerate() {
                    let mut top = y.max(0.0);
                    for a in 0..8 {
                        let t = a as f32 * 0.785;
                        top = top.max(g(x + 6.0 * t.cos(), z + 6.0 * t.sin()));
                    }
                    let eye = Vec3::new(x, top + 1.7, z);
                    let dir = Vec3::new(yaw.cos(), -0.12, yaw.sin()).normalize();
                    low.push((format!("low{k}_{j}"), perspective(eye, eye + dir, w, h)));
                }
            }
            if shore || high {
                k += 1;
            }
            if low.len() >= 64 {
                break 'scan;
            }
        }
    }
    low
}

fn describe(image: &image::RgbaImage, hot: &[(u32, u32)]) -> Vec<String> {
    hot.iter()
        .take(3)
        .map(|&(x, y)| {
            let mut n: Vec<u8> = Vec::new();
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    n.push(luma(image.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)) as u8);
                }
            }
            format!("({x},{y}) {n:?}")
        })
        .collect()
}

/// Белые точки на рельефе против низкого солнца: мокрый берег и склоны с картой деталей.
/// Без `facing_normal` (common.wgsl) и поправки Токсвига (terrain.wgsl) восемь кадров
/// «берег против солнца» дают 55 одиночных белых пикселей (до 22 в одном кадре).
#[test]
fn terrain_has_no_isolated_white_pixels_against_a_low_sun() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = terrain_app();
    let (w, h) = (1280, 720);
    let views = low_views(&app, w, h);
    let picked = ["low0_3", "low18_3", "low21_3", "low27_3", "low39_3", "low45_3", "low40_1", "low24_2"];
    let mut report = Vec::new();
    let mut total = 0;
    for &(pitch, yaw) in &[(-3.0f32, 200.0f32), (-5.0, 20.0)] {
        set_sun(&mut app, pitch, yaw);
        for (name, view) in views.iter().filter(|(n, _)| picked.contains(&n.as_str())) {
            let (image, _) = renderer.capture_view(&app.world, w, h, Some(view)).unwrap();
            let hot = hot_pixels(&image);
            let tag = format!("{name}_sun{}", pitch.abs() as i32);
            dump(&image, &tag, &hot);
            total += hot.len();
            // Вблизи мокрый песок против солнца честно искрится бугорками карты деталей
            // (несколько пикселей на блик) — до трёх на кадр допускается.
            if hot.len() > 3 {
                report.push(format!("{tag}: {} {:?}", hot.len(), describe(&image, &hot)));
            }
        }
    }
    assert!(report.is_empty() && total <= 8, "isolated white pixels on the terrain ({total}): {report:#?}");
}

/// Разведка по многим видам и солнцам (печатает, сохраняет кадры в `RENDER_FIX_SHOTS`):
/// `cargo test -p engine --test render_artifacts explore -- --ignored --nocapture`.
#[test]
#[ignore]
fn explore_terrain_hot_pixels() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = terrain_app();
    let (w, h) = (1280, 720);
    let sets = [terrain_views(&app, w, h), low_views(&app, w, h)];
    let only: Option<usize> = std::env::var("EXPLORE_SET").ok().and_then(|s| s.parse().ok());
    let mut total = 0;
    for (index, views) in sets.iter().enumerate() {
        if only.is_some_and(|o| o != index) {
            continue;
        }
        for &(pitch, yaw) in &[(-40.0f32, -28.0f32), (-12.0, 150.0), (-70.0, 60.0), (-5.0, 20.0), (-3.0, 200.0)] {
            set_sun(&mut app, pitch, yaw);
            for (name, view) in views {
                let (image, _) = renderer.capture_view(&app.world, w, h, Some(view)).unwrap();
                let hot = hot_pixels(&image);
                total += hot.len();
                let tag = format!("{name}_sun{}", pitch.abs() as i32);
                if !hot.is_empty() {
                    println!("{tag}: {} hot {:?}", hot.len(), describe(&image, &hot));
                    dump(&image, &tag, &hot);
                }
            }
        }
    }
    println!("total {total}");
}

/// Отладка: те же виды без неба на пурпурном фоне — сколько пурпурных пикселей видно
/// сквозь рельеф ниже линии горизонта.
#[test]
#[ignore]
fn explore_terrain_cracks() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = terrain_app();
    let (w, h) = (1280, 720);
    set_sun(&mut app, -3.0, 200.0);
    for (name, view) in low_views(&app, w, h) {
        let mut view = view;
        view.sky = false;
        view.clear_color = Color::rgb(1.0, 0.0, 1.0);
        view.post = Some(engine_render::PostSettings::OFF);
        let (image, _) = renderer.capture_view(&app.world, w, h, Some(&view)).unwrap();
        // Пурпурный пиксель, под которым (ниже по экрану) снова рельеф — дырка.
        let mut holes = Vec::new();
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let p = image.get_pixel(x, y);
                let magenta = p[0] > 150 && p[1] < 90 && p[2] > 150;
                let below = image.get_pixel(x, y + 1);
                let above = image.get_pixel(x, y - 1);
                let solid = |q: &image::Rgba<u8>| !(q[0] > 150 && q[1] < 90 && q[2] > 150);
                if magenta && solid(below) && solid(above) {
                    holes.push((x, y));
                }
            }
        }
        if !holes.is_empty() {
            println!("{name}: {} holes {:?}", holes.len(), &holes[..holes.len().min(5)]);
            dump(&image, &format!("{name}_holes"), &holes);
        }
    }
}


/// Текстура «в метрах» (`tiling` / `world_tiling`) на объекте, который вращается вокруг
/// своей точки (метла в marble): узор едет вместе с объектом, а не «плывёт» по нему, и
/// у повёрнутого бруса не смешивается из двух проекций. Вид строго сверху: одна и та же
/// точка верхней грани (в осях объекта) должна давать один и тот же цвет клетки при любом
/// повороте.
#[test]
fn world_tiled_texture_sticks_to_a_rotating_object() {
    let _gpu = gpu_lock();
    let dir = std::env::temp_dir().join(format!("piska_tiling_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Шахматка 2 × 2 клетки: на 1 м узора — клетки по 0.5 м.
    image::RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 { image::Rgba([240, 240, 240, 255]) } else { image::Rgba([20, 20, 20, 255]) }
    })
    .save(dir.join("checker.png"))
    .unwrap();
    std::fs::write(
        dir.join("checker.png.meta"),
        "(\n    guid: \"7e1100000000000000000000000000aa\",\n    importer: \"Texture\",\n    settings: {\n        \"filter\": Enum(\"Nearest\"),\n    },\n)",
    )
    .unwrap();
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).expect("temp assets open");
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let checker = db.handle::<engine_assets::Texture>("checker.png").unwrap();
    let origin = Vec3::new(3.0, 0.0, -2.0);
    let scale = Vec3::new(4.0, 0.5, 1.0);
    let arm = app
        .spawn("Arm")
        .at3(origin.x, origin.y, origin.z)
        .scale(scale.x, scale.y, scale.z)
        .with(MeshRenderer::cube().with_texture(checker, 1.0).with_color(Color::WHITE))
        .id();
    app.spawn("Sun").rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)).with(Light { shadows: false, ..Light::default() });
    let (w, h) = (400u32, 400u32);
    let eye = origin + Vec3::Y * 12.0;
    let view_proj = engine_render::perspective(30f32.to_radians(), 1.0, 0.5, 100.0) * Mat4::look_at_rh(eye, origin, Vec3::NEG_Z);
    let view = CameraView {
        view_proj,
        position: eye,
        clear_color: Color::rgb(0.0, 0.0, 1.0),
        sky: false,
        post: Some(engine_render::PostSettings::OFF),
        culling_mask: u32::MAX,
        overlay: false,
        grid: None,
        fog: false,
    };
    // Центры клеток верхней грани (в метрах вдоль осей объекта от его точки).
    let mut points = Vec::new();
    for i in 0..7 {
        for j in 0..2 {
            points.push(Vec3::new(-1.75 + i as f32 * 0.5, scale.y * 0.5, -0.25 + j as f32 * 0.5));
        }
    }
    let shade = |angle: f32, app: &mut App, renderer: &mut Renderer| -> Vec<bool> {
        let rotation = Quat::from_rotation_y(angle.to_radians());
        app.world.scene.get_mut(arm).unwrap().transform.rotation = rotation;
        let (image, _) = renderer.capture_view(&app.world, w, h, Some(&view)).unwrap();
        points
            .iter()
            .map(|&p| {
                let clip = view_proj * (origin + rotation * p).extend(1.0);
                let ndc = clip.truncate() / clip.w;
                let (x, y) = (((ndc.x * 0.5 + 0.5) * w as f32) as u32, ((0.5 - ndc.y * 0.5) * h as f32) as u32);
                let px = image.get_pixel(x, y);
                assert!(px[2] < 200 || px[0] > 100, "point {p} is on the object");
                luma(px) > 100.0
            })
            .collect()
    };
    let rest = shade(0.0, &mut app, &mut renderer);
    let mut moved = Vec::new();
    for angle in [23.0f32, 40.0, 90.0, 137.0] {
        let turned = shade(angle, &mut app, &mut renderer);
        let differ = rest.iter().zip(&turned).filter(|(a, b)| a != b).count();
        if differ > 0 {
            moved.push(format!("{angle}°: {differ} of {} cells changed colour", rest.len()));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(rest.iter().any(|&b| b) && rest.iter().any(|&b| !b), "the checker is visible: {rest:?}");
    assert!(moved.is_empty(), "the tiled texture swims on the rotating object: {moved:?}");
}
