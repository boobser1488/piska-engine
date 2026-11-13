//! Плавность: шар и камера на экране движутся ровно при любой частоте кадров.
//!
//! Игрок держит «вперёд»; после разгона видимая скорость шара и камеры от кадра к кадру
//! почти не меняется, а шар не дёргается относительно камеры. Проверяется на 60, 144 и
//! на неровных кадрах (как на загруженной машине).

mod common;

use common::*;
use engine::prelude::*;

/// Кадры с такими длительностями; возвращает (положение шара, положение камеры) по кадрам.
fn roll_forward(dts: &[f32]) -> Vec<(Vec3, Vec3)> {
    let mut app = game();
    start_course(&mut app, 0);
    let camera = app.world.scene.find_with::<Camera>()[0];
    app.world.input.set_key(Key::W, true);
    let mut frames = Vec::with_capacity(dts.len());
    for &dt in dts {
        app.run_headless(1, dt);
        frames.push((ball_pos(&app), app.world.scene.world_position(camera)));
    }
    frames
}

/// Наибольшее отклонение от среднего, в долях среднего.
fn spread(values: &[f32]) -> (f32, f32) {
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let worst = values.iter().map(|v| (v - mean).abs()).fold(0.0, f32::max);
    (mean, worst / mean.max(1e-6))
}

struct Smoothness {
    ball_speed: f32,
    ball_spread: f32,
    camera_spread: f32,
    /// Наибольший скачок шара относительно камеры за кадр, метры.
    relative_jump: f32,
}

fn measure(dts: &[f32], from: usize, to: usize) -> Smoothness {
    let frames = roll_forward(dts);
    let mut ball = Vec::new();
    let mut camera = Vec::new();
    let mut relative_jump = 0.0_f32;
    for i in from.max(1)..to.min(frames.len()) {
        let (b0, c0) = frames[i - 1];
        let (b1, c1) = frames[i];
        ball.push((b1 - b0).length() / dts[i]);
        camera.push((c1 - c0).length() / dts[i]);
        relative_jump = relative_jump.max(((b1 - c1) - (b0 - c0)).length());
    }
    let (ball_speed, ball_spread) = spread(&ball);
    let (_, camera_spread) = spread(&camera);
    Smoothness { ball_speed, ball_spread, camera_spread, relative_jump }
}

/// Неровные кадры: от 5 до 21 мс, детерминированно.
fn uneven(count: usize) -> Vec<f32> {
    let mut random = Random::new(7);
    (0..count).map(|_| random.range(0.005, 0.021)).collect()
}

fn report(name: &str, s: &Smoothness) {
    println!(
        "{name}: ball {:.2} m/s, spread {:.1}%, camera spread {:.1}%, ball-vs-camera jump {:.1} mm",
        s.ball_speed,
        s.ball_spread * 100.0,
        s.camera_spread * 100.0,
        s.relative_jump * 1000.0
    );
}

#[test]
fn the_ball_and_the_camera_move_evenly_at_any_frame_rate() {
    // Участок, где шар уже разогнался на первой прямой и ещё не доехал до поворота.
    let cases: [(&str, Vec<f32>, usize, usize); 3] = [
        ("60 fps", vec![1.0 / 60.0; 170], 110, 170),
        ("144 fps", vec![1.0 / 144.0; 400], 270, 400),
        ("uneven 48-200 fps", uneven(220), 140, 220),
    ];
    for (name, dts, from, to) in cases {
        let s = measure(&dts, from, to);
        report(name, &s);
        assert!(s.ball_speed > 4.0, "{name}: the ball is rolling ({:.2} m/s)", s.ball_speed);
        assert!(s.ball_spread < 0.12, "{name}: the ball's on-screen speed jumps by {:.0}%", s.ball_spread * 100.0);
        assert!(s.camera_spread < 0.15, "{name}: the camera's speed jumps by {:.0}%", s.camera_spread * 100.0);
        assert!(s.relative_jump < 0.03, "{name}: the ball jerks against the camera by {:.0} mm", s.relative_jump * 1000.0);
    }
}

#[test]
#[ignore = "diagnostic print"]
fn print_frames_at_144() {
    let frames = roll_forward(&vec![1.0 / 144.0; 330]);
    for i in 300..330 {
        let d = frames[i].0 - frames[i - 1].0;
        println!("{i}: ball step {:.4} m (xz {:.4}, y {:.4})", d.length(), Vec2::new(d.x, d.z).length(), d.y);
    }
}
