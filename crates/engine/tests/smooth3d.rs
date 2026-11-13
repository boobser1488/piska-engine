//! Сглаживание 3D-физики между фиксированными шагами: тело, которое движется с постоянной
//! скоростью, на экране сдвигается на одинаковое расстояние в секунду при любой частоте
//! кадров — и тогда, когда шаг физики случается не в каждом кадре (144 Гц), и на неровных
//! кадрах.

use engine::prelude::*;

/// Шар летит вдоль X с постоянной скоростью (без гравитации и трения).
fn flying_ball(speed: f32) -> (App, GameObjectId) {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    if let Some(physics) = app.world.resources.get_mut::<Physics3D>() {
        physics.gravity = Vec3::ZERO;
    }
    let ball = app
        .spawn("Ball")
        .at3(0.0, 5.0, 0.0)
        .with(Collider::sphere(0.5))
        .with(Rigidbody { velocity: Vec3::new(speed, 0.0, 0.0), linear_damping: 0.0, ..Rigidbody::dynamic() })
        .id();
    (app, ball)
}

/// Видимая скорость по кадрам (после нескольких разгонных кадров).
fn seen_speeds(dts: &[f32], speed: f32) -> Vec<f32> {
    let (mut app, ball) = flying_ball(speed);
    let mut last = app.world.scene.world_position(ball);
    let mut speeds = Vec::new();
    for (index, &dt) in dts.iter().enumerate() {
        app.run_headless(1, dt);
        let now = app.world.scene.world_position(ball);
        if index >= 10 {
            speeds.push((now - last).length() / dt);
        }
        last = now;
    }
    speeds
}

fn worst_error(speeds: &[f32], speed: f32) -> f32 {
    speeds.iter().map(|v| (v - speed).abs() / speed).fold(0.0, f32::max)
}

#[test]
fn a_body_moves_evenly_on_screen_at_60_hz() {
    let speeds = seen_speeds(&[1.0 / 60.0; 120], 6.0);
    assert!(worst_error(&speeds, 6.0) < 0.02, "worst error {:.1}%", worst_error(&speeds, 6.0) * 100.0);
}

#[test]
fn a_body_moves_evenly_on_screen_at_144_hz() {
    let speeds = seen_speeds(&[1.0 / 144.0; 300], 6.0);
    let stalled = speeds.iter().filter(|v| **v < 0.6).count();
    assert_eq!(stalled, 0, "the body must not stand still between physics steps ({stalled} of {} frames)", speeds.len());
    assert!(worst_error(&speeds, 6.0) < 0.05, "worst error {:.1}%", worst_error(&speeds, 6.0) * 100.0);
}

#[test]
fn a_body_moves_evenly_on_screen_with_uneven_frames() {
    let mut random = Random::new(11);
    let dts: Vec<f32> = (0..300).map(|_| random.range(0.004, 0.022)).collect();
    let speeds = seen_speeds(&dts, 6.0);
    assert!(worst_error(&speeds, 6.0) < 0.05, "worst error {:.1}%", worst_error(&speeds, 6.0) * 100.0);
}

/// Видимая скорость тела `ball` в уже собранном приложении.
fn seen_speeds_of(app: &mut App, ball: GameObjectId, dts: &[f32]) -> Vec<f32> {
    let mut last = app.world.scene.world_position(ball);
    let mut speeds = Vec::new();
    for (index, &dt) in dts.iter().enumerate() {
        app.run_headless(1, dt);
        let now = app.world.scene.world_position(ball);
        if index >= 10 {
            speeds.push((now - last).length() / dt);
        }
        last = now;
    }
    speeds
}

fn stalled(speeds: &[f32]) -> usize {
    speeds.iter().filter(|v| **v < 0.6).count()
}

#[test]
fn a_child_body_with_children_moves_evenly_at_144_hz() {
    // Как шар в игре: лежит внутри объекта-уровня и несёт на себе дочерние украшения.
    let (mut app, ball) = flying_ball(6.0);
    let level = app.spawn("Level").at3(3.0, 1.0, -2.0).id();
    app.world.scene.set_parent(ball, Some(level)).unwrap();
    let stripe = app.spawn("Stripe").with(MeshRenderer::cube()).id();
    app.world.scene.set_parent(stripe, Some(ball)).unwrap();

    let speeds = seen_speeds_of(&mut app, ball, &[1.0 / 144.0; 300]);
    assert_eq!(stalled(&speeds), 0, "stalled in {} of {} frames", stalled(&speeds), speeds.len());
    assert!(worst_error(&speeds, 6.0) < 0.05, "worst error {:.1}%", worst_error(&speeds, 6.0) * 100.0);
}

#[test]
fn a_rolling_ball_moves_evenly_at_144_hz() {
    // Шар катится по полу: вращается, трётся, включён CCD.
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    app.spawn("Floor").at3(0.0, -0.5, 0.0).scale(400.0, 1.0, 20.0).with(Collider::cuboid(1.0, 1.0, 1.0));
    let ball = app
        .spawn("Ball")
        .at3(0.0, 0.5, 0.0)
        .with(Collider::sphere(0.5))
        .with(Rigidbody {
            velocity: Vec3::new(6.0, 0.0, 0.0),
            angular_velocity: Vec3::new(0.0, 0.0, -12.0),
            ccd: true,
            linear_damping: 0.0,
            angular_damping: 0.0,
            ..Rigidbody::dynamic()
        })
        .id();
    let speeds = seen_speeds_of(&mut app, ball, &[1.0 / 144.0; 300]);
    let mean = speeds.iter().sum::<f32>() / speeds.len() as f32;
    assert!(mean > 4.0, "the ball rolls: {mean:.2} m/s");
    assert_eq!(stalled(&speeds), 0, "stalled in {} of {} frames", stalled(&speeds), speeds.len());
    assert!(worst_error(&speeds, mean) < 0.08, "worst error {:.1}%", worst_error(&speeds, mean) * 100.0);
}
