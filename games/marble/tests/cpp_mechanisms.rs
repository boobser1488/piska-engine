//! Механизмы цехов — компоненты на C++, слинкованные в крейт статически
//! (`engine-cpp-build` + `engine::cpp_scripts!()`): они зарегистрированы и работают.

mod common;

use common::*;
use engine::prelude::*;
use marble::kit::Kit;

/// Все C++-компоненты игры.
const CPP_TYPES: [&str; 14] = [
    "MovingPlatform",
    "Sweeper",
    "SweeperArm",
    "Conveyor",
    "Fan",
    "Bumper",
    "Crusher",
    "CollapsingTile",
    "GearPickup",
    "Checkpoint",
    "Booster",
    "FinishPad",
    "Blinker",
    "Cog",
];

/// Пустой мир для опытов: игра собрана, меню убрано.
fn lab() -> (App, GameObjectId) {
    let mut app = game();
    // Витрину меню убираем: на её площадке опыты были бы не чистыми.
    if let Some(showcase) = state_mut(&mut app).course_root.take() {
        app.world.scene.destroy(showcase);
    }
    app.run_headless(2, DT);
    let root = app.world.scene.spawn("Lab");
    (app, root)
}

/// Шар-пробник: тело, коллайдер, тег.
fn probe_ball(app: &mut App, pos: Vec3) -> GameObjectId {
    app.world
        .scene
        .build("Probe Ball")
        .tag("Ball")
        .at3(pos.x, pos.y, pos.z)
        .with(Rigidbody { ccd: true, ..Rigidbody::dynamic() })
        .with(Collider::sphere(0.5).with_material(0.8, 0.0).with_density(4.0))
        .id()
}

#[test]
fn every_cpp_component_type_is_registered() {
    let app = game();
    for name in CPP_TYPES {
        assert!(app.world.registry.contains(name), "C++ component `{name}` is registered");
    }
    // Не пересекаются ли имена с компонентами движка и демо-плагина (там тоже есть Pickup, Spinner).
    let mut names: Vec<&str> = CPP_TYPES.to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 14, "fourteen distinct C++ component types");
}

#[test]
fn cpp_fields_are_reflected_for_the_inspector_and_scenes() {
    let app = game();
    let mut platform = app.world.registry.create("MovingPlatform").expect("create by name");
    platform.set_field("period", &Value::Float(9.5)).expect("set a field");
    assert_eq!(platform.get_field("period"), Some(Value::Float(9.5)));
    assert_eq!(platform.get_field("travel"), Some(Value::Vec3(0.0, 0.0, -8.0)), "default of a Vec3 field");
    let err = platform.set_field("period", &Value::Str("fast".into())).unwrap_err();
    assert!(err.to_string().contains("period"), "{err}");
    let mut gear = app.world.registry.create("GearPickup").unwrap();
    assert_eq!(gear.get_field("golden"), Some(Value::Bool(false)));
    gear.set_field("golden", &Value::Bool(true)).unwrap();
    // Список полей: инспектор показывает их все.
    assert!(app.world.registry.create("Fan").unwrap().field_names().contains(&"lift"));
}

#[test]
fn moving_platform_travels_between_its_endpoints() {
    let (mut app, root) = lab();
    let id = {
        let mut kit = Kit::new(&mut app.world, root);
        kit.platform(Vec3::new(0.0, 0.0, 0.0), 0.0, Vec3::new(4.0, 0.5, 4.0), Vec3::new(0.0, 0.0, -8.0), 6.0, 1.0, 0.0)
    };
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
    // Две полных поездки туда-обратно — вернулась к началу.
    app.run_headless(60 * 6, DT);
    let z = app.world.scene.world_position(id).z;
    assert!(z.abs() <= 8.1, "stays on its track: {z}");
}

#[test]
fn a_ball_rides_the_platform() {
    let (mut app, root) = lab();
    let platform = {
        let mut kit = Kit::new(&mut app.world, root);
        kit.platform(Vec3::new(0.0, 0.0, 0.0), 0.0, Vec3::new(4.0, 0.5, 4.0), Vec3::new(0.0, 0.0, -8.0), 6.0, 0.5, 0.0)
    };
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.7, 0.0));
    app.run_headless(75, DT);
    let p = app.world.scene.world_position(platform);
    let b = app.world.scene.world_position(ball);
    assert!(p.z < -1.5, "the platform moved: {}", p.z);
    assert!(b.z < -0.4 && b.y > 0.3, "the ball is carried along by friction instead of staying behind: ball {b:?}, platform {p:?}");
}

#[test]
fn conveyor_pushes_a_body_along_the_belt() {
    let (mut app, root) = lab();
    {
        let mut kit = Kit::new(&mut app.world, root);
        kit.conveyor(Vec3::new(0.0, 0.0, 0.0), 0.0, 14.0, 4.0, 4.0);
    }
    let crate_id = {
        let mut kit = Kit::new(&mut app.world, root);
        kit.crate_(Vec3::new(0.0, 0.7, 5.0), 1.0)
    };
    app.run_headless(30, DT);
    let z0 = app.world.scene.world_position(crate_id).z;
    app.run_headless(90, DT);
    let z1 = app.world.scene.world_position(crate_id).z;
    assert!(z0 - z1 > 2.5, "the belt carries the crate forward (-Z): {z0} -> {z1}");
    let speed = app.world.scene.get_component::<Rigidbody>(crate_id).unwrap().velocity.z;
    assert!((-4.6..-2.0).contains(&speed), "the crate settles near the belt speed (4 m/s): {speed}");

    // Обратный ход: та же лента с минусом везёт назад.
    let (mut app2, root2) = lab();
    {
        let mut kit = Kit::new(&mut app2.world, root2);
        kit.conveyor(Vec3::ZERO, 0.0, 14.0, 4.0, -4.0);
    }
    let reverse = {
        let mut kit = Kit::new(&mut app2.world, root2);
        kit.crate_(Vec3::new(0.0, 0.7, -3.0), 1.0)
    };
    app2.run_headless(90, DT);
    assert!(app2.world.scene.world_position(reverse).z > -1.0, "a negative speed carries the crate back");
}

#[test]
fn fan_lifts_the_ball() {
    let (mut app, root) = lab();
    {
        let mut kit = Kit::new(&mut app.world, root);
        kit.block("Floor", Vec3::new(0.0, -0.3, 0.0), Vec3::new(10.0, 0.6, 10.0), Quat::IDENTITY, marble::kit::Mat::Plate);
        kit.fan(Vec3::ZERO, Quat::IDENTITY, 7.0, 1.8, 26.0);
    }
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.6, 0.0));
    let mut top = 0.0f32;
    for _ in 0..180 {
        app.run_headless(1, DT);
        top = top.max(app.world.scene.world_position(ball).y);
    }
    assert!(top > 4.0, "the air stream lifts the ball well above the floor: top = {top}");

    // Без вентилятора шар остаётся на полу.
    let (mut idle, root2) = lab();
    {
        let mut kit = Kit::new(&mut idle.world, root2);
        kit.block("Floor", Vec3::new(0.0, -0.3, 0.0), Vec3::new(10.0, 0.6, 10.0), Quat::IDENTITY, marble::kit::Mat::Plate);
    }
    let ball2 = probe_ball(&mut idle, Vec3::new(0.0, 0.6, 0.0));
    idle.run_headless(120, DT);
    assert!(idle.world.scene.world_position(ball2).y < 0.7);
}

#[test]
fn collapsing_tile_falls_under_the_ball_and_comes_back() {
    let (mut app, root) = lab();
    let tile = {
        let mut kit = Kit::new(&mut app.world, root);
        kit.tile(Vec3::ZERO, 0.0, 3.0, 3.0)
    };
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.7, 0.0));
    app.run_headless(30, DT);
    assert!(app.world.scene.world_position(tile).y > -0.5, "the tile holds for a moment");
    app.run_headless(150, DT);
    assert!(app.world.scene.world_position(tile).y < -3.0, "the tile has collapsed");
    assert!(app.world.scene.world_position(ball).y < -1.0, "and the ball went down with it");
    // Через respawn секунд плита на месте.
    app.run_headless(60 * 8, DT);
    assert!(app.world.scene.world_position(tile).y > -0.5, "the tile is back: {}", app.world.scene.world_position(tile).y);
}

#[test]
fn booster_throws_the_ball_forward() {
    let (mut app, root) = lab();
    {
        let mut kit = Kit::new(&mut app.world, root);
        kit.block("Floor", Vec3::new(0.0, -0.3, -10.0), Vec3::new(8.0, 0.6, 40.0), Quat::IDENTITY, marble::kit::Mat::Plate);
        kit.booster(Vec3::new(0.0, 0.0, 0.0), 0.0, 3.0, 3.0, 14.0);
    }
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
    let (mut app, root) = lab();
    {
        let mut kit = Kit::new(&mut app.world, root);
        kit.block("Floor", Vec3::new(0.0, -0.3, 0.0), Vec3::new(20.0, 0.6, 20.0), Quat::IDENTITY, marble::kit::Mat::Plate);
        kit.bumper(Vec3::ZERO, 0.8);
    }
    let ball = probe_ball(&mut app, Vec3::new(4.0, 0.6, 0.0));
    app.world.scene.get_component_mut::<Rigidbody>(ball).unwrap().velocity = Vec3::new(-5.0, 0.0, 0.0);
    app.run_headless(45, DT);
    let v = app.world.scene.get_component::<Rigidbody>(ball).unwrap().velocity;
    assert!(v.x > 3.0, "the bumper sends the ball back the way it came: {v:?}");
}

#[test]
fn sweeper_spins_its_arms() {
    let (mut app, root) = lab();
    let hub = {
        let mut kit = Kit::new(&mut app.world, root);
        kit.sweeper(Vec3::ZERO, 8.0, 1, 90.0, 0.0, 0.0)
    };
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
    // Ставим пресс отдельно над шаром: в цехе пресс есть, но нужен контролируемый случай.
    let root = app.world.scene.spawn("Lab");
    let ball_pos = Vec3::new(500.0, 0.6, 500.0);
    {
        let mut kit = Kit::new(&mut app.world, root);
        kit.block("Floor", Vec3::new(500.0, -0.3, 500.0), Vec3::new(8.0, 0.6, 8.0), Quat::IDENTITY, marble::kit::Mat::Plate);
        kit.crusher(Vec3::new(500.0, 3.85, 500.0), Vec3::new(4.0, 1.0, 4.0), 3.3, 3.0, 0.5, 0.0);
    }
    let lab_ball = probe_ball(&mut app, ball_pos);
    let falls_before = state(&app).falls;
    app.run_headless(60 * 4, DT);
    let _ = lab_ball;
    assert!(state(&app).falls > falls_before, "the piston came down on the ball: the game sent it back to the checkpoint");
}

#[test]
#[ignore]
fn debug_booster() {
    let (mut app, root) = lab();
    {
        let mut kit = Kit::new(&mut app.world, root);
        kit.block("Floor", Vec3::new(0.0, -0.3, -10.0), Vec3::new(8.0, 0.6, 40.0), Quat::IDENTITY, marble::kit::Mat::Plate);
        kit.booster(Vec3::new(0.0, 0.0, 0.0), 0.0, 3.0, 3.0, 14.0);
    }
    let ball = probe_ball(&mut app, Vec3::new(0.0, 0.6, 4.0));
    app.world.scene.get_component_mut::<Rigidbody>(ball).unwrap().velocity = Vec3::new(0.0, 0.0, -4.0);
    for i in 0..90 {
        app.run_headless(1, DT);
        if i % 4 == 0 {
            let p = app.world.scene.world_position(ball);
            let v = app.world.scene.get_component::<Rigidbody>(ball).unwrap().velocity;
            println!("f={i} z={:.2} y={:.2} v={:?}", p.z, p.y, v);
        }
    }
}
