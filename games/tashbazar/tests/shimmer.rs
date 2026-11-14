//! Рябь на дистанции: дальние виды карты, два кадра — до и после крошечного движения
//! камеры (1 мм вбок; поворот на 0.02°). Разница кадров раскладывается на три части:
//!
//! * **края геометрии** — то, что меняется и у белой сцены без текстур и без теней (сдвиг
//!   края объекта через границу пикселя — честное движение, не рябь);
//! * **тени** — что меняется у белой сцены с тенями сверх краёв (ползущие края теней);
//! * **текстуры** — что меняется у настоящей сцены сверх краёв и теней: рябь узора
//!   (муар, мерцание тонких линий без мипмапов).
//!
//! Печатает по виду: число «прыгнувших» пикселей (яркость изменилась больше 24 из 255) в
//! каждой части и среднюю разницу яркости по текстурам. Отдельно — **вдали** (дальше
//! [`FAR`] м по лучу из камеры): там тексель 16-на-метр уже меньше пикселя, текстура
//! уменьшается, и прыгающие пиксели — это рябь. Вблизи резкие края текселей честно едут
//! вместе с поверхностью и тоже «прыгают» на сдвиге камеры — это не рябь. Измерение, а не проверка:
//! `cargo test -p tashbazar --test shimmer -- --ignored --nocapture`
//! (`TASHBAZAR_CHECK_ASSETS=<каталог>` — другая копия ассетов; `TASHBAZAR_SHOTS` — сохранить
//! пары кадров и карту ряби).

mod common;

use engine::prelude::*;
use engine::render::Renderer;
use tashbazar::scripts::game_manager::GameManager;

const DT: f32 = 1.0 / 60.0;
const W: u32 = 1280;
const H: u32 = 720;
/// Дальше этого (м) тексель 1/16 м меньше пикселя кадра 1280 × 720 при обзоре ~75°.
const FAR: f32 = 34.0;

/// Пиксели, луч которых упирается в геометрию дальше [`FAR`] (или в небо — не считаются).
fn far_mask(app: &App, player: GameObjectId) -> Vec<bool> {
    let scene = &app.world.scene;
    let cam = scene
        .find_with::<Camera>()
        .into_iter()
        .find(|id| {
            let mut cur = *id;
            while let Some(p) = scene.parent_of(cur) {
                if p == player {
                    return true;
                }
                cur = p;
            }
            false
        })
        .expect("player camera");
    let fov = scene.get_component::<Camera>(cam).unwrap().fov_deg.to_radians();
    let (_, rot, eye) = scene.world_matrix(cam).to_scale_rotation_translation();
    let physics = app.world.resources.get::<Physics3D>().unwrap();
    let t = (fov * 0.5).tan();
    let aspect = W as f32 / H as f32;
    let mut mask = vec![false; (W * H) as usize];
    for y in (0..H).step_by(2) {
        for x in (0..W).step_by(2) {
            let d = rot * Vec3::new(((x as f32 + 1.0) / W as f32 * 2.0 - 1.0) * t * aspect, (1.0 - (y as f32 + 1.0) / H as f32 * 2.0) * t, -1.0).normalize();
            let far = physics.raycast(eye, d, 400.0, Some(player)).is_some_and(|h| h.distance > FAR);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                mask[((y + dy) * W + x + dx) as usize] = far;
            }
        }
    }
    mask
}

fn luma(p: &image::Rgba<u8>) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

/// Пиксели, где кадры различаются больше порога, с расширением на `grow` пикселей.
fn changed(a: &image::RgbaImage, b: &image::RgbaImage, grow: i32) -> Vec<bool> {
    let mut raw = vec![false; (W * H) as usize];
    for (x, y, p) in a.enumerate_pixels() {
        raw[(y * W + x) as usize] = (luma(p) - luma(b.get_pixel(x, y))).abs() > 24.0;
    }
    let mut out = raw.clone();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if raw[(y as u32 * W + x as u32) as usize] {
                for dy in -grow..=grow {
                    for dx in -grow..=grow {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < W as i32 && ny < H as i32 {
                            out[(ny as u32 * W + nx as u32) as usize] = true;
                        }
                    }
                }
            }
        }
    }
    out
}

struct Pose {
    feet: Vec3,
    yaw: f32,
    pitch: f32,
}

fn place(app: &mut App, player: GameObjectId, pose: &Pose) {
    app.world.scene.get_mut(player).unwrap().transform.position = pose.feet;
    let c = app.world.scene.get_component_mut::<FpsController>(player).unwrap();
    c.yaw_deg = pose.yaw;
    c.pitch_deg = pose.pitch;
    c.velocity = Vec3::ZERO;
    c.noclip = true;
}

/// Пара кадров: в позе и после движения (`moved`). Время стоит — меняется только камера.
fn pair(app: &mut App, renderer: &mut Renderer, player: GameObjectId, pose: &Pose, moved: &Pose) -> (image::RgbaImage, image::RgbaImage) {
    place(app, player, pose);
    app.run_headless(2, DT);
    // Поза в кадре — после шага контроллера (он ставит камеру по `feet`).
    place(app, player, pose);
    let a = renderer.capture(&app.world, W, H).unwrap();
    // Без шага кадра (иначе мигнут фонари): позиция — переносом, поворот — поворотом
    // самого игрока (камера — его ребёнок), контроллер его до кадра не перепишет.
    let before = app.world.scene.get(player).unwrap().transform.rotation;
    place(app, player, moved);
    let turn = Quat::from_rotation_y((moved.yaw - pose.yaw).to_radians());
    app.world.scene.get_mut(player).unwrap().transform.rotation = turn * before;
    let b = renderer.capture(&app.world, W, H).unwrap();
    app.world.scene.get_mut(player).unwrap().transform.rotation = before;
    (a, b)
}

/// Материалы сцены: белые, без текстур (`true`) или как в файлах (`false`).
fn flat_materials(app: &mut App, flat: bool) {
    let handles: Vec<AssetHandle<Material>> = app
        .world
        .scene
        .find_with::<MeshRenderer>()
        .iter()
        .filter_map(|id| app.world.scene.get_component::<MeshRenderer>(*id))
        .flat_map(|r| r.materials.clone())
        .filter(|h| !h.is_none())
        .collect();
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let materials = app.world.resources.get_or_insert_with(Materials::default);
    for h in handles {
        if flat {
            if let Ok(mut m) = db.load_material(h.guid()) {
                m.base_color_texture = AssetHandle::default();
                m.normal_texture = AssetHandle::default();
                m.base_color = Color::rgb(0.8, 0.8, 0.8);
                materials.set(h, m);
            }
        } else {
            materials.remove(h);
        }
    }
}

fn sun_shadows(app: &mut App, on: bool) {
    for id in app.world.scene.find_with::<Light>() {
        if let Some(l) = app.world.scene.get_component_mut::<Light>(id) {
            if l.kind == LightKind::Directional {
                l.shadows = on;
            }
        }
    }
}

#[test]
#[ignore = "needs a GPU; a measurement: prints texture shimmer, shadow crawl and edge pixels per far view"]
fn far_views_do_not_shimmer() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut renderer = Renderer::headless().expect("a GPU");
    let mut app = tashbazar::build_in(SaveData::in_memory(), common::assets(), tashbazar::MAP_SCENE).expect("the game builds");
    let game = app.world.scene.find_by_name("Game").expect("Game");
    app.world.send_message(game, "play", Value::Nil);
    app.run_headless(5, DT);
    assert_eq!(app.world.scene.get_component::<GameManager>(game).unwrap().screen, tashbazar::scripts::game_manager::Screen::Playing);
    let player = app.world.scene.find_by_name("Player").expect("Player");
    let out = std::env::var("TASHBAZAR_SHOTS").ok().map(|d| std::path::PathBuf::from(d).join("shimmer"));
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).unwrap();
    }
    // Длинная улица у Длинной стены в обе стороны, через обе площадки, с крыш, вдоль канала.
    let views: [(&str, Vec3, f32, f32); 6] = [
        ("long_wall", Vec3::new(-46.0, 0.0, -45.0), 180.0, -2.0),
        ("long_wall_back", Vec3::new(-46.0, 0.0, 2.0), 0.0, -2.0),
        ("across_a", Vec3::new(-46.0, 0.0, -2.0), 200.0, -3.0),
        ("across_b", Vec3::new(42.5, 0.0, 3.0), 130.0, -3.0),
        ("canal", Vec3::new(0.0, -1.5, -32.0), 180.0, -2.0),
        ("roofs", Vec3::new(-50.0, 14.0, -60.0), 135.0, -10.0),
    ];
    let (mut tex_total, mut shadow_total, mut edge_total, mut mean_total, mut far_total) = (0usize, 0usize, 0usize, 0.0f32, 0usize);
    for (name, feet, yaw, pitch) in views {
        let pose = Pose { feet, yaw, pitch };
        let side = Quat::from_rotation_y(yaw.to_radians()) * Vec3::X;
        for (motion, moved) in [
            ("move 1 mm", Pose { feet: feet + side * 0.001 + Vec3::Y * 0.001, yaw, pitch }),
            ("turn 0.02°", Pose { feet, yaw: yaw + 0.02, pitch }),
        ] {
            flat_materials(&mut app, true);
            sun_shadows(&mut app, false);
            let (ga, gb) = pair(&mut app, &mut renderer, player, &pose, &moved);
            let edges = changed(&ga, &gb, 1);
            sun_shadows(&mut app, true);
            let (sa, sb) = pair(&mut app, &mut renderer, player, &pose, &moved);
            let shadow_raw = changed(&sa, &sb, 0);
            let shadow_mask = changed(&sa, &sb, 1);
            flat_materials(&mut app, false);
            let (ta, tb) = pair(&mut app, &mut renderer, player, &pose, &moved);
            let tex_raw = changed(&ta, &tb, 0);
            place(&mut app, player, &pose);
            let far = far_mask(&app, player);
            let (mut far_px, mut far_flips) = (0usize, 0usize);
            let (mut tex, mut shadow, mut edge, mut sum, mut count) = (0usize, 0usize, 0usize, 0.0f32, 0usize);
            for i in 0..(W * H) as usize {
                if edges[i] {
                    edge += tex_raw[i] as usize;
                    continue;
                }
                if shadow_raw[i] {
                    shadow += 1;
                }
                if shadow_mask[i] {
                    continue;
                }
                let (x, y) = ((i as u32) % W, (i as u32) / W);
                if far[i] {
                    far_px += 1;
                    far_flips += tex_raw[i] as usize;
                }
                sum += (luma(ta.get_pixel(x, y)) - luma(tb.get_pixel(x, y))).abs();
                count += 1;
                tex += tex_raw[i] as usize;
            }
            let mean = sum / count.max(1) as f32;
            println!(
                "{name:15} {motion:11}: far texture shimmer {far_flips:6} of {far_px:6} far px; all texture flips {tex:6} px (mean |dL| {mean:.3}), shadow crawl {shadow:6} px, geometry edges {edge:6} px"
            );
            far_total += far_flips;
            tex_total += tex;
            shadow_total += shadow;
            edge_total += edge;
            mean_total += mean;
            if let Some(dir) = &out {
                let tag = motion.replace([' ', '°'], "_");
                ta.save(dir.join(format!("{name}_{tag}a.png"))).unwrap();
                tb.save(dir.join(format!("{name}_{tag}b.png"))).unwrap();
                let mut map = ta.clone();
                for (x, y, p) in map.enumerate_pixels_mut() {
                    let i = (y * W + x) as usize;
                    *p = if edges[i] {
                        image::Rgba([p[0] / 4, p[1] / 4, p[2] / 4, 255])
                    } else if shadow_raw[i] {
                        image::Rgba([0, 255, 255, 255])
                    } else if tex_raw[i] {
                        image::Rgba([255, 0, 255, 255])
                    } else {
                        image::Rgba([p[0] / 2, p[1] / 2, p[2] / 2, 255])
                    };
                }
                map.save(dir.join(format!("{name}_{tag}map.png"))).unwrap();
            }
        }
    }
    println!("total: far texture shimmer {far_total} px; all texture flips {tex_total} px, mean |dL| sum {mean_total:.3}, shadow crawl {shadow_total} px, geometry edges {edge_total} px");
}
