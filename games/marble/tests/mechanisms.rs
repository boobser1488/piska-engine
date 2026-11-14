//! Механизмы цехов — компоненты-скрипты на Rust из `src/scripts/`, собранные в префабы
//! (`assets/prefabs/mech/*`): каждый зарегистрирован, берётся из префаба и работает.

mod common;

use common::*;
use engine::prelude::*;
use marble::scripts::{
    booster::Booster, conveyor::Conveyor, crusher::Crusher, moving_platform::MovingPlatform, sweeper::Sweeper,
};

/// Все компоненты игры.
const TYPES: [&str; 18] = [
    "Ball",
    "Blinker",
    "Booster",
    "Bumper",
    "CameraRig",
    "Checkpoint",
    "Cog",
    "CollapsingTile",
    "Conveyor",
    "Crusher",
    "Fan",
    "FinishPad",
    "GameManager",
    "GearPickup",
    "MovingPlatform",
    "SeesawRig",
    "Sweeper",
    "SweeperArm",
];

#[test]
fn every_script_component_is_registered() {
    let app = game();
    for name in TYPES {
        assert!(app.world.registry.contains(name), "script component `{name}` is registered");
    }
}

#[test]
fn script_fields_are_reflected_for_the_inspector_and_scenes() {
    let app = game();
    let mut platform = app.world.registry.create("MovingPlatform").expect("create by name");
    platform.set_field("period", &Value::Float(9.5)).expect("set a field");
    assert_eq!(platform.get_field("period"), Some(Value::Float(9.5)));
    assert_eq!(platform.get_field("travel"), Some(Value::Vec3(0.0, 0.0, -8.0)), "default of a Vec3 field");
    let err = platform.set_field("period", &Value::Str("fast".into())).unwrap_err();
    assert!(err.to_string().contains("period"), "{err}");
    assert!(app.world.registry.create("Fan").unwrap().field_names().contains(&"lift"));
}

// Редактор тянет `cpp` через объединение фич, поэтому проверка — только без него.
#[cfg(not(any(feature = "editor", feature = "cpp-example")))]
#[test]
fn the_cpp_engine_feature_is_off_by_default() {
    // Игра целиком на Rust: C++ выключен, поэтому она собирается и для веба.
    assert!(!engine::enabled_features().contains(&"cpp"), "the `cpp` engine feature is off: {:?}", engine::enabled_features());
}

#[test]
fn moving_platform_travels_between_its_endpoints() {
    let mut app = lab();
    let id = spawn_prefab(&mut app, "mech/moving_platform", Vec3::new(0.0, -0.25, 0.0));
    {
        let p = app.world.scene.get_component_mut::<MovingPlatform>(id).unwrap();
        p.travel = Vec3::new(0.0, 0.0, -8.0);
        p.period = 6.0;
        p.pause = 1.0;
    }
    app.run_headless(2, DT);
    let start = app.world.scene.world_position(id);
    let (mut min_z, mut max_z) = (start.z, start.z);
    for _ in 0..(60 * 13) {
        app.run_headless(1, DT);
        let z = app.world.scene.world_position(id).z;
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    assert!(max_z - start.z < 0.3, "the platform never goes behind its start: {max_z} vs {}", start.z);
    assert!(start.z - min_z > 7.7, "the platform reaches the far endpoint (8 m): travelled {}", start.z - min_z);
}

#[test]
fn a_ball_rides_the_platform() {
    let mut app = lab();
    let platform = spawn_prefab(&mut app, "mech/moving_platform", Vec3::new(0.0, -0.25, 0.0));
    {
        let p = app.world.scene.get_component_mut::<MovingPlatform>(platform).unwrap();
        p.travel = Vec3::new(0.0, 0.0, -8.0);
        p.pause = 0.5;
    }
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.7, 0.0));
    app.run_headless(75, DT);
    let p = app.world.scene.world_position(platform);
    let b = app.world.scene.world_position(ball);
    assert!(p.z < -1.5, "the platform moved: {}", p.z);
    assert!(b.z < -0.4 && b.y > 0.3, "the ball is carried along by friction instead of staying behind: ball {b:?}, platform {p:?}");
}

#[test]
fn conveyor_pushes_a_body_along_the_belt() {
    let mut app = lab();
    let belt = spawn_prefab(&mut app, "mech/conveyor", Vec3::new(0.0, -0.2, 0.0));
    app.world.scene.get_component_mut::<Conveyor>(belt).unwrap().size = Vec3::new(4.0, 0.4, 10.0);
    let crate_id = spawn_prefab(&mut app, "track/crate", Vec3::new(0.0, 0.7, 3.0));
    app.run_headless(30, DT);
    let z0 = app.world.scene.world_position(crate_id).z;
    app.run_headless(90, DT);
    let z1 = app.world.scene.world_position(crate_id).z;
    assert!(z0 - z1 > 2.5, "the belt carries the crate forward (-Z): {z0} -> {z1}");
    let speed = app.world.scene.get_component::<Rigidbody>(crate_id).unwrap().velocity.z;
    assert!((-4.6..-2.0).contains(&speed), "the crate settles near the belt speed (4 m/s): {speed}");

    // Обратный ход: та же лента с минусом везёт назад.
    let mut back = lab();
    let belt = spawn_prefab(&mut back, "mech/conveyor", Vec3::new(0.0, -0.2, 0.0));
    back.world.scene.get_component_mut::<Conveyor>(belt).unwrap().speed = -4.0;
    let reverse = spawn_prefab(&mut back, "track/crate", Vec3::new(0.0, 0.7, -3.0));
    back.run_headless(90, DT);
    assert!(back.world.scene.world_position(reverse).z > -1.0, "a negative speed carries the crate back");
}

#[test]
fn fan_lifts_the_ball() {
    let mut app = lab();
    spawn_block(&mut app, Vec3::new(0.0, -0.3, 0.0), Vec3::new(10.0, 0.6, 10.0));
    spawn_prefab(&mut app, "mech/fan", Vec3::ZERO);
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.6, 0.0));
    let mut top = 0.0f32;
    for _ in 0..180 {
        app.run_headless(1, DT);
        top = top.max(app.world.scene.world_position(ball).y);
    }
    assert!(top > 4.0, "the air stream lifts the ball well above the floor: top = {top}");

    // Без вентилятора шар остаётся на полу.
    let mut idle = lab();
    spawn_block(&mut idle, Vec3::new(0.0, -0.3, 0.0), Vec3::new(10.0, 0.6, 10.0));
    let ball2 = probe_ball(&mut idle, Vec3::new(0.0, 0.6, 0.0));
    idle.run_headless(120, DT);
    assert!(idle.world.scene.world_position(ball2).y < 0.7);
}

#[test]
fn collapsing_tile_falls_under_the_ball_and_comes_back() {
    let mut app = lab();
    let tile = spawn_prefab(&mut app, "mech/tile", Vec3::new(0.0, -0.225, 0.0));
    app.world.scene.get_mut(tile).unwrap().transform.scale = Vec3::new(3.0, 0.45, 3.0);
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.7, 0.0));
    app.run_headless(30, DT);
    assert!(app.world.scene.world_position(tile).y > -0.5, "the tile holds for a moment");
    app.run_headless(150, DT);
    assert!(app.world.scene.world_position(tile).y < -3.0, "the tile has collapsed");
    assert!(app.world.scene.world_position(ball).y < -1.0, "and the ball went down with it");
    app.run_headless(60 * 8, DT);
    assert!(app.world.scene.world_position(tile).y > -0.5, "the tile is back: {}", app.world.scene.world_position(tile).y);
}

#[test]
fn booster_throws_the_ball_forward() {
    let mut app = lab();
    spawn_block(&mut app, Vec3::new(0.0, -0.3, -10.0), Vec3::new(8.0, 0.6, 40.0));
    let pad = spawn_prefab(&mut app, "mech/booster", Vec3::ZERO);
    app.world.scene.get_component_mut::<Booster>(pad).unwrap().speed = 14.0;
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.6, 4.0));
    app.world.scene.get_component_mut::<Rigidbody>(ball).unwrap().velocity = Vec3::new(0.0, 0.0, -4.0);
    let mut fastest = 0.0f32;
    for _ in 0..80 {
        app.run_headless(1, DT);
        fastest = fastest.max(-app.world.scene.get_component::<Rigidbody>(ball).unwrap().velocity.z);
    }
    assert!(fastest > 12.0, "the pad accelerates the ball to ~14 m/s along its arrow: {fastest}");
}

#[test]
fn bumper_kicks_the_ball_away() {
    let mut app = lab();
    spawn_block(&mut app, Vec3::new(0.0, -0.3, 0.0), Vec3::new(20.0, 0.6, 20.0));
    spawn_prefab(&mut app, "mech/bumper", Vec3::new(0.0, 0.45, 0.0));
    let ball = probe_ball(&mut app, Vec3::new(4.0, 0.6, 0.0));
    app.world.scene.get_component_mut::<Rigidbody>(ball).unwrap().velocity = Vec3::new(-5.0, 0.0, 0.0);
    app.run_headless(45, DT);
    let v = app.world.scene.get_component::<Rigidbody>(ball).unwrap().velocity;
    assert!(v.x > 3.0, "the bumper sends the ball back the way it came: {v:?}");
}

#[test]
fn sweeper_spins_its_arms() {
    let mut app = lab();
    let hub = spawn_prefab(&mut app, "mech/sweeper", Vec3::ZERO);
    app.world.scene.get_component_mut::<Sweeper>(hub).unwrap().speed_deg = 90.0;
    app.run_headless(2, DT);
    let before = app.world.scene.get(hub).unwrap().transform.rotation;
    app.run_headless(30, DT);
    let after = app.world.scene.get(hub).unwrap().transform.rotation;
    let angle = before.angle_between(after).to_degrees();
    assert!((40.0..50.0).contains(&angle), "90 deg/s for half a second turns the hub 45 deg, got {angle}");
}

#[test]
fn crusher_squashes_a_ball_under_the_piston() {
    let mut app = game();
    start_course(&mut app, 4);
    // Пресс отдельно над шаром далеко в стороне: в цехе прессы есть, но нужен контролируемый случай.
    let at = Vec3::new(500.0, 0.0, 500.0);
    spawn_block(&mut app, at + Vec3::new(0.0, -0.3, 0.0), Vec3::new(8.0, 0.6, 8.0));
    let press = spawn_prefab(&mut app, "mech/crusher", at + Vec3::new(0.0, 3.85, 0.0));
    {
        let c = app.world.scene.get_component_mut::<Crusher>(press).unwrap();
        c.stroke = 3.3;
        c.period = 3.0;
        c.wait_up = 0.5;
    }
    probe_ball(&mut app, at + Vec3::new(0.0, 0.6, 0.0));
    let falls_before = state(&app).falls;
    app.run_headless(60 * 4, DT);
    assert!(state(&app).falls > falls_before, "the piston came down on the ball: the game sent it back to the checkpoint");
}
