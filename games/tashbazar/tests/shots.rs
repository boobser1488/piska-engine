//! Кадры карты с именованных точек (для проверки глазами) и замер кадра 1920×1080.
//! Нужна видеокарта, окно не открывается, звук выключен:
//! `TASHBAZAR_SHOTS=<каталог> cargo test -p tashbazar --test shots -- --ignored --nocapture`.

use std::path::PathBuf;
use std::time::Instant;

use engine::prelude::*;
use engine::render::Renderer;
use tashbazar::scripts::game_manager::GameManager;

const DT: f32 = 1.0 / 60.0;

fn game(scene: &str) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    tashbazar::build_in(SaveData::in_memory(), common::assets(), scene).expect("the game builds")
}

/// Нажать «Прогулка по карте» и вернуть игрока.
fn start_playing(app: &mut App) -> GameObjectId {
    let game = app.world.scene.find_by_name("Game").expect("Game");
    app.world.send_message(game, "play", Value::Nil);
    app.run_headless(5, DT);
    let manager = app.world.scene.get_component::<GameManager>(game).unwrap();
    assert_eq!(manager.screen, tashbazar::scripts::game_manager::Screen::Playing);
    app.world.scene.find_by_name("Player").expect("Player")
}

/// Поставить игрока (в полёте, чтобы стоял где поставили) с поворотом и наклоном.
fn place(app: &mut App, player: GameObjectId, feet: Vec3, yaw: f32, pitch: f32) {
    app.world.scene.get_mut(player).unwrap().transform.position = feet;
    {
        let c = app.world.scene.get_component_mut::<FpsController>(player).unwrap();
        c.yaw_deg = yaw;
        c.pitch_deg = pitch;
        c.velocity = Vec3::ZERO;
        c.noclip = true;
    }
    // Подсказка по клавишам гаснет, стоп-кадр в покое.
    app.run_headless(12, DT);
}

#[test]
#[ignore = "needs a GPU: writes PNG files into $TASHBAZAR_SHOTS"]
fn viewpoints_and_frame_time() {
    let out = PathBuf::from(std::env::var("TASHBAZAR_SHOTS").expect("TASHBAZAR_SHOTS"));
    std::fs::create_dir_all(&out).unwrap();
    let mut renderer = Renderer::headless().expect("a GPU");
    let (w, h) = (1280, 720);

    // Главное меню поверх облёта.
    let mut app = game(tashbazar::MAP_SCENE);
    app.run_headless(30, DT);
    renderer.capture(&app.world, w, h).unwrap().save(out.join("00_menu.png")).unwrap();

    let player = start_playing(&mut app);
    let views: [(&str, Vec3, f32, f32); 12] = [
        ("01_attackers_spawn", Vec3::new(-2.0, 0.0, -56.0), 160.0, -4.0),
        ("02_ropewalk", Vec3::new(-15.0, 0.0, -50.0), 90.0, 0.0),
        ("03_long_wall", Vec3::new(-46.0, 0.0, -45.0), 180.0, 0.0),
        ("04_site_a_from_short", Vec3::new(-29.5, 0.0, -2.0), 145.0, -6.0),
        ("05_site_a_from_arch", Vec3::new(-46.0, 0.0, -2.0), 200.0, -6.0),
        ("06_aqueduct", Vec3::new(-15.0, 3.0, -18.5), 88.0, -8.0),
        ("07_canal", Vec3::new(0.0, -1.5, -32.0), 180.0, 2.0),
        ("08_bridge", Vec3::new(-13.0, 0.0, -6.0), -90.0, -2.0),
        ("09_spice_market", Vec3::new(15.0, 0.0, -51.0), -90.0, 0.0),
        ("10_souk", Vec3::new(41.0, 0.0, -47.0), 180.0, 0.0),
        ("11_site_b", Vec3::new(42.5, 0.0, 3.0), 130.0, -6.0),
        ("12_well_court", Vec3::new(-8.0, 0.0, 47.0), 15.0, 0.0),
    ];
    for (name, feet, yaw, pitch) in views {
        place(&mut app, player, feet, yaw, pitch);
        let (image, stats) = renderer.capture_view(&app.world, w, h, None).unwrap();
        image.save(out.join(format!("{name}.png"))).unwrap();
        println!("{name}: {} draw calls ({} shadow), {} meshes, {} triangles", stats.draw_calls, stats.shadow_draw_calls, stats.meshes, stats.triangles);
    }

    // План сверху: ортографическая камера над серединой.
    let top = app
        .spawn("Plan Camera")
        .at3(-2.0, 120.0, -5.0)
        .rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
        .with(Camera { projection: Projection::Orthographic, ortho_size: 62.0, near: 1.0, far: 400.0, priority: 100, ..Camera::default() })
        .id();
    app.run_headless(2, DT);
    renderer.capture(&app.world, 1200, 1200).unwrap().save(out.join("13_plan.png")).unwrap();
    app.world.scene.destroy(top);
    app.run_headless(2, DT);

    // Замер кадра 1920×1080 (рендер + чтение кадра в память) с точки над серединой.
    place(&mut app, player, Vec3::new(-6.0, 0.0, -38.0), 180.0, -3.0);
    let (fw, fh) = (1920, 1080);
    let _ = renderer.capture_view(&app.world, fw, fh, None).unwrap();
    let frames = 20;
    let start = Instant::now();
    let mut stats = RenderStats::default();
    for _ in 0..frames {
        app.run_headless(1, DT);
        stats = renderer.capture_view(&app.world, fw, fh, None).unwrap().1;
    }
    let ms = start.elapsed().as_secs_f32() * 1000.0 / frames as f32;
    println!(
        "frame 1920x1080: {ms:.1} ms (incl. readback), {} draw calls ({} shadow), {} meshes, {} triangles, {} culled",
        stats.draw_calls, stats.shadow_draw_calls, stats.meshes, stats.triangles, stats.culled_meshes
    );
    let profile = app.profile().clone();
    println!("cpu tick: {:.2} ms (scripts {:.2} ms)", profile.total_ms, profile.scripts_ms);

    // Полигон.
    let mut gym = game(tashbazar::GYM_SCENE);
    gym.run_headless(10, DT);
    let p = gym.world.scene.find_by_name("Player").unwrap();
    place(&mut gym, p, Vec3::new(-4.0, 6.0, 22.5), 0.0, -22.0);
    renderer.capture(&gym.world, w, h).unwrap().save(out.join("14_gym.png")).unwrap();
}

mod common;

/// Лист модели: спереди, сбоку, сзади и сверху, на земле с сеткой 1 м.
#[test]
#[ignore = "needs a GPU: writes PNG files into $TASHBAZAR_SHOTS/models"]
fn model_sheets() {
    let out = PathBuf::from(std::env::var("TASHBAZAR_SHOTS").expect("TASHBAZAR_SHOTS")).join("models");
    std::fs::create_dir_all(&out).unwrap();
    let mut renderer = Renderer::headless().expect("a GPU");
    let assets = PathBuf::from(common::assets());
    let mut list = Vec::new();
    for dir in ["props", "facade"] {
        for entry in std::fs::read_dir(assets.join("prefabs").join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "prefab") {
                list.push(format!("prefabs/{dir}/{}", path.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    let size = 480u32;
    for prefab in list {
        std::env::set_var("ENGINE_AUDIO", "off");
        let mut app = engine::app();
        tashbazar::register(&mut app);
        engine::mount_assets(&mut app, &assets).unwrap();
        app.spawn("Ground").at3(0.0, -0.05, 0.0).scale(12.0, 0.1, 12.0).with(MeshRenderer { grid: true, color: Color::hex(0xb9ad98), ..MeshRenderer::cube() });
        app.spawn("Sun").rotation(Quat::from_rotation_y(-0.6) * Quat::from_rotation_x(-0.9)).with(Light::directional(3.0));
        app.spawn("Environment").with(Environment { ambient: 0.6, fog_density: 0.0, ..Environment::default() });
        // Стена за настенными вещами (у них корень на поверхности стены, наружу +Z).
        let wall_mounted = prefab.contains("facade/") || prefab.contains("wall_lantern") || prefab.contains("awning");
        if wall_mounted {
            app.spawn("Wall").at3(0.0, 2.0, -0.25).scale(5.0, 4.0, 0.5).with(MeshRenderer { color: Color::hex(0xd8c4a0), ..MeshRenderer::cube() });
        }
        let y = if prefab.contains("window") || prefab.contains("wall_lantern") { 1.5 } else { 0.0 };
        let guid = app.world.resources.get::<AssetDatabase>().unwrap().guid_of(&prefab).unwrap().to_string();
        let id = app.world.instantiate_at(&guid, Vec3::new(0.0, y, 0.0), Quat::IDENTITY, None).unwrap();
        app.run_headless(2, DT);
        let pieces = common::check::pieces(&app.world);
        let own: Vec<_> = pieces.iter().filter(|p| p.assembly == id).collect();
        let lo = own.iter().map(|p| p.min).fold(Vec3::splat(f32::MAX), Vec3::min);
        let hi = own.iter().map(|p| p.max).fold(Vec3::splat(f32::MIN), Vec3::max);
        let center = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(0.3);
        let mut sheet = image::RgbaImage::new(size * 2, size * 2);
        let views = [(Vec3::Z, 0), (Vec3::X, 1), (Vec3::NEG_Z, 2), (Vec3::Y, 3)];
        for (dir, slot) in views {
            let eye = center + if dir == Vec3::Y { Vec3::new(0.0, 1.0, 0.001) * radius * 3.0 } else { (dir + Vec3::Y * 0.35).normalize() * radius * 3.0 };
            let look = (center - eye).normalize();
            let yaw = (-look.x).atan2(-look.z);
            let pitch = look.y.clamp(-1.0, 1.0).asin();
            let cam = app
                .spawn("View")
                .at3(eye.x, eye.y, eye.z)
                .rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch))
                .with(Camera { priority: 100, near: 0.02, far: 100.0, ..Camera::perspective(40.0) })
                .id();
            app.run_headless(1, DT);
            let image = renderer.capture(&app.world, size, size).unwrap();
            image::imageops::replace(&mut sheet, &image, ((slot % 2) * size) as i64, ((slot / 2) * size) as i64);
            app.world.scene.destroy(cam);
            app.run_headless(1, DT);
        }
        let name = prefab.trim_start_matches("prefabs/").replace(['/', '.'], "_");
        sheet.save(out.join(format!("{name}.png"))).unwrap();
        // Ящики и телега — ещё крупный план (развёртка и атлас видны вблизи).
        if prefab.contains("crate") || prefab.contains("cart") {
            let eye = center + Vec3::new(0.75, 0.35, 1.0).normalize() * radius * 1.25;
            let look = (center - eye).normalize();
            let cam = app
                .spawn("Close")
                .at3(eye.x, eye.y, eye.z)
                .rotation(Quat::from_rotation_y((-look.x).atan2(-look.z)) * Quat::from_rotation_x(look.y.clamp(-1.0, 1.0).asin()))
                .with(Camera { priority: 100, near: 0.02, far: 100.0, ..Camera::perspective(40.0) })
                .id();
            app.run_headless(1, DT);
            renderer.capture(&app.world, 960, 640).unwrap().save(out.join(format!("{name}_closeup.png"))).unwrap();
            app.world.scene.destroy(cam);
        }
        println!("{prefab}: bounds {lo} .. {hi}");
    }
}

/// Прогулка: кадры на уровне глаз вдоль каждого маршрута и крупно у реквизита.
/// `TASHBAZAR_DEBUG_OFFENDERS=1` — нарушители проверки подсвечены красным.
#[test]
#[ignore = "needs a GPU: writes PNG files into $TASHBAZAR_SHOTS/walk"]
fn walk_every_route() {
    let out = PathBuf::from(std::env::var("TASHBAZAR_SHOTS").expect("TASHBAZAR_SHOTS")).join("walk");
    std::fs::create_dir_all(&out).unwrap();
    let mut renderer = Renderer::headless().expect("a GPU");
    let mut app = game(tashbazar::MAP_SCENE);
    let player = start_playing(&mut app);
    if std::env::var("TASHBAZAR_DEBUG_OFFENDERS").is_ok() {
        for o in common::check::check(&app.world) {
            let mut stack = vec![o.id];
            while let Some(id) = stack.pop() {
                if let Some(obj) = app.world.scene.get_mut(id) {
                    stack.extend(obj.children().iter().copied());
                    if let Some(r) = obj.get_mut::<MeshRenderer>() {
                        r.emissive = Color::rgb(3.0, 0.0, 0.0);
                    }
                }
            }
        }
    }
    let table = app.world.resources.get::<AssetDatabase>().unwrap().load_data::<tashbazar::RouteTable>("data/routes.ron").unwrap();
    let mut n = 0;
    for route in &table.routes {
        for (i, w) in route.points.windows(2).enumerate() {
            let (a, b) = (w[0].position(), w[1].position());
            if route.id == "course" && i % 2 == 1 {
                continue;
            }
            let dir = Vec2::new(b.x - a.x, b.z - a.z).normalize_or_zero();
            let yaw = (-dir.x).atan2(-dir.y).to_degrees();
            place(&mut app, player, a, yaw, -3.0);
            renderer.capture(&app.world, 960, 540).unwrap().save(out.join(format!("{:03}_{}_{i:02}.png", n, route.id))).unwrap();
            n += 1;
        }
    }
    // Крупно у реквизита: камера в 2.5 м, взгляд чуть вниз.
    let props: Vec<(String, Vec3)> = common::check::pieces(&app.world)
        .iter()
        .filter(|p| common::check::RESTING.contains(&common::check::assembly_name(&app.world, p.assembly).as_str()))
        .map(|p| (common::check::assembly_name(&app.world, p.assembly), app.world.scene.world_position(p.assembly)))
        .collect();
    let mut seen: Vec<Vec3> = Vec::new();
    for (name, at) in props {
        if seen.iter().any(|s| s.distance(at) < 4.0) {
            continue;
        }
        seen.push(at);
        // Две точки обзора из восьми направлений, откуда реквизит виден (луч от глаз до него
        // не упирается в стену и сами глаза не в стене).
        let mut views = Vec::new();
        for k in 0..8 {
            let a = (k as f32 * 45.0 + 20.0).to_radians();
            let feet = at + Vec3::new(a.cos(), 0.0, a.sin()) * 2.6;
            let eye = feet + Vec3::Y * 1.22;
            let target = at + Vec3::Y * 0.6;
            let physics = app.world.resources.get::<Physics3D>().unwrap();
            let inside = !physics.overlap_box(eye, Vec3::splat(0.1), QueryFilter3D::solid()).is_empty();
            let blocked = physics.raycast(eye, target - eye, (target - eye).length() - 0.7, None).is_some();
            if !inside && !blocked {
                views.push(feet - at);
            }
            if views.len() == 2 {
                break;
            }
        }
        for (k, offset) in views.iter().enumerate() {
            let eye_feet = at + *offset;
            let to = at - eye_feet;
            let yaw = (-to.x).atan2(-to.z).to_degrees();
            place(&mut app, player, eye_feet, yaw, -18.0);
            renderer.capture(&app.world, 960, 540).unwrap().save(out.join(format!("{:03}_prop_{name}_{k}.png", n))).unwrap();
            n += 1;
        }
    }
    println!("{n} shots");
}

/// Фасады с окнами: каждый прямо и под углом 40°, с высоты глаз (`$TASHBAZAR_SHOTS/facades`,
/// подкаталог — `$TASHBAZAR_FACADES`, если задан: «до» и «после»).
#[test]
#[ignore = "needs a GPU: writes PNG files into $TASHBAZAR_SHOTS/facades"]
fn facades() {
    let sub = std::env::var("TASHBAZAR_FACADES").unwrap_or_else(|_| "facades".to_string());
    let out = PathBuf::from(std::env::var("TASHBAZAR_SHOTS").expect("TASHBAZAR_SHOTS")).join(sub);
    std::fs::create_dir_all(&out).unwrap();
    let mut renderer = Renderer::headless().expect("a GPU");
    let mut app = game(tashbazar::MAP_SCENE);
    let player = start_playing(&mut app);
    // Окна: корень и «наружу»; фасад — окна на одной плоскости стены не дальше 4 м друг от друга.
    let mut windows: Vec<(Vec3, Vec3)> = Vec::new();
    for id in app.world.scene.find_with::<MeshRenderer>() {
        let asm = {
            let mut cur = id;
            while let Some(p) = app.world.scene.parent_of(cur) {
                if app.world.scene.get(cur).is_some_and(|o| o.prefab().is_some()) {
                    break;
                }
                cur = p;
            }
            cur
        };
        if app.world.scene.get(asm).is_none_or(|o| o.name != "window") {
            continue;
        }
        let (_, rot, at) = app.world.scene.world_matrix(asm).to_scale_rotation_translation();
        if !windows.iter().any(|(p, _)| p.distance(at) < 0.01) {
            windows.push((at, rot * Vec3::Z));
        }
    }
    let mut facades: Vec<Vec<(Vec3, Vec3)>> = Vec::new();
    for w in windows {
        let same = |f: &Vec<(Vec3, Vec3)>| {
            f.iter().any(|(p, n)| n.dot(w.1) > 0.99 && (w.0 - *p).dot(*n).abs() < 0.05 && Vec2::new(w.0.x - p.x, w.0.z - p.z).length() < 4.0)
        };
        match facades.iter_mut().find(|f| same(f)) {
            Some(f) => f.push(w),
            None => facades.push(vec![w]),
        }
    }
    let mut n = 0;
    for (i, f) in facades.iter().enumerate() {
        let out_dir = f[0].1;
        let along = Vec3::Y.cross(out_dir).normalize();
        let center = f.iter().map(|(p, _)| *p).sum::<Vec3>() / f.len() as f32;
        let target = center + Vec3::Y * 0.6;
        let (span0, span1) = f.iter().map(|(p, _)| (*p - center).dot(along)).fold((0.0f32, 0.0f32), |a, u| (a.0.min(u), a.1.max(u)));
        let wanted = ((span1 - span0) * 0.9 + 4.0).clamp(4.0, 9.0);
        for (k, angle) in [(0, 0.0f32), (1, 40.0), (1, -40.0)] {
            let dir = (out_dir * angle.to_radians().cos() + along * angle.to_radians().sin()).normalize();
            let physics = app.world.resources.get::<Physics3D>().unwrap();
            let start = Vec3::new(target.x, 1.0f32.max(center.y - 0.4), target.z) + out_dir * 0.2;
            let free = physics.raycast(start, dir, wanted, None).map(|h| h.distance - 0.6).unwrap_or(wanted).max(1.2);
            let mut feet = start + dir * free;
            // Пол под камерой.
            if let Some(hit) = physics.raycast(feet + Vec3::Y * 0.5, Vec3::NEG_Y, 6.0, None) {
                feet.y = feet.y + 0.5 - hit.distance;
            }
            let eye = feet + Vec3::Y * 1.62;
            let inside = !physics.overlap_box(eye, Vec3::splat(0.1), QueryFilter3D::solid()).is_empty();
            if inside {
                continue;
            }
            let to = target - eye;
            let yaw = (-to.x).atan2(-to.z).to_degrees();
            let pitch = (to.y / Vec2::new(to.x, to.z).length()).atan().to_degrees();
            place(&mut app, player, feet, yaw, pitch);
            let tag = if k == 0 { "straight" } else { "angle" };
            renderer
                .capture(&app.world, 960, 540)
                .unwrap()
                .save(out.join(format!("{i:03}_{}_{}_{tag}.png", center.x.round(), center.z.round())))
                .unwrap();
            n += 1;
            if k == 1 {
                break;
            }
        }
    }
    println!("{} facades, {n} shots", facades.len());
}

/// Вплотную (0.5 м): стена, мостовая, дверь, телега, ящик, кувшин, лоток — пиксели должны
/// быть резкими и одного размера у соседних объектов (`$TASHBAZAR_SHOTS/closeups`).
#[test]
#[ignore = "needs a GPU: writes PNG files into $TASHBAZAR_SHOTS/closeups"]
fn closeups() {
    let out = PathBuf::from(std::env::var("TASHBAZAR_SHOTS").expect("TASHBAZAR_SHOTS")).join("closeups");
    std::fs::create_dir_all(&out).unwrap();
    let mut renderer = Renderer::headless().expect("a GPU");
    let mut app = game(tashbazar::MAP_SCENE);
    let _ = start_playing(&mut app);
    let pose = |app: &App, name: &str| {
        let id = app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("{name} on the map"));
        let (_, rot, at) = app.world.scene.world_matrix(id).to_scale_rotation_translation();
        (at, rot * Vec3::Z, rot * Vec3::X)
    };
    let mut views: Vec<(&str, Vec3, Vec3)> = Vec::new();
    let (w, out_w, along) = pose(&app, "window");
    views.push(("wall", w + along * 1.5 + Vec3::Y * 0.3, out_w));
    let (d, out_d, _) = pose(&app, "door");
    views.push(("door", d + Vec3::Y * 1.3 + out_d * 0.05, out_d));
    views.push(("paving", Vec3::new(-4.0, 0.0, -50.0), Vec3::new(0.0, 1.0, 0.45).normalize()));
    let (c, _, _) = pose(&app, "cart");
    views.push(("cart", c + Vec3::new(0.2, 0.85, 0.6), Vec3::new(0.25, 0.2, 1.0).normalize()));
    let (k, _, _) = pose(&app, "crate_small");
    views.push(("crate", k + Vec3::new(0.0, 0.6, 0.45), Vec3::new(0.3, 0.25, 1.0).normalize()));
    let (j, _, _) = pose(&app, "jar");
    views.push(("jar", j + Vec3::Y * 0.25, Vec3::new(0.4, 0.3, 1.0).normalize()));
    let (m, out_m, _) = pose(&app, "market_stall");
    views.push(("stall", m + Vec3::Y * 0.7 + out_m * 1.0, (out_m + Vec3::Y * 0.3).normalize()));
    for (name, target, normal) in views {
        let eye = target + normal * 0.5;
        let look = (target - eye).normalize();
        let cam = app
            .spawn("Close")
            .at3(eye.x, eye.y, eye.z)
            .rotation(Quat::from_rotation_y((-look.x).atan2(-look.z)) * Quat::from_rotation_x(look.y.clamp(-1.0, 1.0).asin()))
            .with(Camera { priority: 100, near: 0.02, far: 300.0, ..Camera::perspective(60.0) })
            .id();
        app.run_headless(1, DT);
        renderer.capture(&app.world, 960, 640).unwrap().save(out.join(format!("{name}.png"))).unwrap();
        app.world.scene.destroy(cam);
        app.run_headless(1, DT);
    }
}
