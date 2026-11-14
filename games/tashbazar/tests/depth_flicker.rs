//! Мерцание карты вдали («как от наложения текстур»): кадр с длинной точки обзора и тот
//! же кадр после сдвига камеры на 1 мм (время не идёт — анимаций нет). Пиксели, сменившие
//! цвет вдали от резких краёв геометрии, — это борьба глубин близких поверхностей
//! (стекло за рамой, плитка на мостовой, карнизы). Нужна видеокарта, окно не открывается:
//! `cargo test -p tashbazar --test depth_flicker -- --ignored --nocapture`
//! (`TASHBAZAR_SHOTS=<каталог>` — сохранить кадры).

use engine::prelude::*;
use engine::render::Renderer;

const DT: f32 = 1.0 / 60.0;

/// Пиксели, чей цвет во втором кадре не объясняется сдвигом краёв: хотя бы один канал
/// вышел за диапазон соседей 3 × 3 первого кадра больше чем на 24. Подпиксельный сдвиг
/// края даёт смесь соседних цветов (внутри диапазона); борьба глубин — цвет другой
/// поверхности, которого рядом не было.
fn flips(a: &image::RgbaImage, b: &image::RgbaImage) -> usize {
    let (w, h) = a.dimensions();
    let mut count = 0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let q = b.get_pixel(x, y);
            let outside = (0..3).any(|c| {
                let (mut lo, mut hi) = (255u8, 0u8);
                for dy in 0..3 {
                    for dx in 0..3 {
                        let v = a.get_pixel(x + dx - 1, y + dy - 1)[c];
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
                q[c] as i32 > hi as i32 + 24 || (q[c] as i32) < lo as i32 - 24
            });
            if outside {
                count += 1;
            }
        }
    }
    count
}

/// Замер, а не проверка: на этих точках (дальность до ~120 м) почти всё, что меняется
/// при сдвиге на 1 мм, — честный сдвиг краёв у близких предметов; до обратной глубины
/// было 66 пикселей на шесть кадров, после — 53. Точность глубины проверяет
/// `crates/engine/tests/depth_precision.rs`.
#[test]
#[ignore = "needs a GPU; a measurement: prints flipping pixels per viewpoint"]
fn long_sightlines_do_not_flicker() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = tashbazar::build_in(SaveData::in_memory(), concat!(env!("CARGO_MANIFEST_DIR"), "/assets"), tashbazar::MAP_SCENE)
        .expect("the game builds");
    let game = app.world.scene.find_by_name("Game").expect("Game");
    app.world.send_message(game, "play", Value::Nil);
    app.run_headless(5, DT);
    let player = app.world.scene.find_by_name("Player").expect("Player");
    let mut renderer = Renderer::headless().expect("a GPU");
    let out = std::env::var("TASHBAZAR_SHOTS").ok();
    let (w, h) = (1280, 720);
    // Длинные линии взгляда: «Длинная стена», через A и B, с крыш и акведука на всю карту.
    let views: [(&str, Vec3, f32, f32); 6] = [
        ("long_wall", Vec3::new(-46.0, 0.0, -45.0), 180.0, 0.0),
        ("long_wall_back", Vec3::new(-46.0, 0.0, 2.0), 0.0, 0.0),
        ("across_a", Vec3::new(-46.0, 0.0, -2.0), 200.0, -2.0),
        ("across_b", Vec3::new(42.5, 0.0, 3.0), 130.0, -2.0),
        ("aqueduct", Vec3::new(-15.0, 3.0, -18.5), 88.0, -6.0),
        ("roofs", Vec3::new(-50.0, 14.0, -60.0), 135.0, -10.0),
    ];
    let mut report = Vec::new();
    let mut total = 0;
    for (name, feet, yaw, pitch) in views {
        app.world.scene.get_mut(player).unwrap().transform.position = feet;
        {
            let c = app.world.scene.get_component_mut::<FpsController>(player).unwrap();
            c.yaw_deg = yaw;
            c.pitch_deg = pitch;
            c.velocity = Vec3::ZERO;
            c.noclip = true;
        }
        app.run_headless(12, DT);
        let rest = app.world.scene.get(player).unwrap().transform.position;
        let a = renderer.capture(&app.world, w, h).unwrap();
        // 1 мм вбок и вверх; время стоит — меняется только камера.
        let side = Quat::from_rotation_y(yaw.to_radians()) * Vec3::X;
        app.world.scene.get_mut(player).unwrap().transform.position = rest + side * 0.001 + Vec3::Y * 0.001;
        let b = renderer.capture(&app.world, w, h).unwrap();
        app.world.scene.get_mut(player).unwrap().transform.position = rest;
        let n = flips(&a, &b);
        total += n;
        report.push(format!("{name}: {n}"));
        if let Some(dir) = &out {
            std::fs::create_dir_all(dir).unwrap();
            a.save(format!("{dir}/depth_{name}.png")).unwrap();
            // Где мерцает: отличающиеся пиксели — белым поверх затемнённого кадра.
            let mut marked = a.clone();
            for (x, y, p) in marked.enumerate_pixels_mut() {
                let q = b.get_pixel(x, y);
                let o = a.get_pixel(x, y);
                if (0..3).any(|c| o[c].abs_diff(q[c]) > 24) {
                    *p = image::Rgba([255, 0, 255, 255]);
                } else {
                    *p = image::Rgba([p[0] / 3, p[1] / 3, p[2] / 3, 255]);
                }
            }
            marked.save(format!("{dir}/depth_{name}_flips.png")).unwrap();
        }
    }
    println!("flipping pixels per view (1280x720, 1 mm camera move): {report:?}, total {total}");
    // Почти ноль: остаются единичные пиксели у тонких деталей, которые сдвиг камеры
    // честно переводит через край пикселя.
    assert!(total <= 60, "the map flickers at a distance: {report:?}");
}
