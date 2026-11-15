//! 3D-физика без окна: тела, запросы, суставы, контроллер персонажа.

use engine::prelude::*;

const DT: f32 = 1.0 / 60.0;

fn app_with_ground() -> App {
    let mut app = engine::app();
    // Земля: верхняя грань на y = 0.
    app.spawn("Ground")
        .at3(0.0, -0.5, 0.0)
        .with(Collider::cuboid(60.0, 1.0, 60.0));
    app
}

fn position(app: &App, id: GameObjectId) -> Vec3 {
    app.world.scene.world_position(id)
}

fn crate_at(app: &mut App, name: &str, x: f32, y: f32, z: f32) -> GameObjectId {
    app.spawn(name)
        .at3(x, y, z)
        .with(Rigidbody::dynamic())
        .with(Collider::cuboid(1.0, 1.0, 1.0))
        .id()
}

#[test]
fn bodies_fall_rest_and_stack() {
    let mut app = app_with_ground();
    let stack: Vec<GameObjectId> = (0..3)
        .map(|i| crate_at(&mut app, "Crate", 0.0, 0.5 + i as f32 * 1.02, 0.0))
        .collect();
    let ball = app
        .spawn("Ball")
        .at3(4.0, 5.0, 0.0)
        .with(Rigidbody::dynamic())
        .with(Collider::sphere(0.4).with_material(0.5, 0.0))
        .id();
    app.run_headless(1, DT);
    assert!(position(&app, ball).y < 5.0, "gravity pulls the ball down");
    app.run_headless(240, DT);

    assert!((position(&app, ball).y - 0.4).abs() < 0.03, "the ball rests on the ground");
    for (index, id) in stack.iter().enumerate() {
        let p = position(&app, *id);
        let expected = 0.5 + index as f32;
        assert!((p.y - expected).abs() < 0.05, "crate {index} rests at y = {} (expected {expected})", p.y);
        assert!(p.x.abs() < 0.05 && p.z.abs() < 0.05, "the stack did not slide apart: {p}");
    }
    let physics = app.world.resources.get::<Physics3D>().unwrap();
    assert_eq!(physics.body_count(), 5);
    assert!(physics.is_sleeping(stack[0]), "resting bodies go to sleep");

    // Статическая геометрия не двигается, скорость тела читается из компонента.
    let ground = app.world.scene.find_by_name("Ground").unwrap();
    assert_eq!(position(&app, ground), Vec3::new(0.0, -0.5, 0.0));
    let velocity = app.world.scene.get_component::<Rigidbody>(ball).unwrap().velocity;
    assert!(velocity.length() < 0.05, "{velocity}");
}

#[test]
fn raycast_overlap_and_explosion() {
    let mut app = app_with_ground();
    let near = crate_at(&mut app, "Near", 1.5, 0.5, 0.0);
    let far = crate_at(&mut app, "Far", 9.0, 0.5, 0.0);
    app.run_headless(30, DT);

    let physics = app.world.resources.get_mut::<Physics3D>().unwrap();
    let hit = physics
        .raycast(Vec3::new(1.5, 5.0, 0.0), Vec3::NEG_Y, 20.0, None)
        .expect("the ray hits the crate");
    assert_eq!(hit.object, near);
    assert!((hit.distance - 4.0).abs() < 0.03 && (hit.normal - Vec3::Y).length() < 1e-3, "{hit:?}");
    // `ignore` пропускает указанный объект: луч доходит до земли.
    let through = physics.raycast(Vec3::new(1.5, 5.0, 0.0), Vec3::NEG_Y, 20.0, Some(near)).unwrap();
    assert!((through.distance - 5.0).abs() < 0.03);
    assert!(physics.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::Y, 20.0, None).is_none());

    let mut inside = physics.overlap_sphere(Vec3::new(0.0, 0.5, 0.0), 2.0);
    inside.retain(|id| *id == near || *id == far);
    assert_eq!(inside, [near]);

    // Взрыв в начале координат отбрасывает ближний ящик и не трогает дальний.
    let pushed = physics.radial_impulse(Vec3::new(0.0, 0.3, 0.0), 4.0, 8.0);
    assert!(pushed.contains(&near) && !pushed.contains(&far));
    app.run_headless(30, DT);
    assert!(position(&app, near).x > 2.5, "blown away: {}", position(&app, near));
    assert!((position(&app, far).x - 9.0).abs() < 0.01);
}

component! {
    /// Считает, сколько тел вошло в триггер и сколько раз объект ударился.
    pub struct Counter {
        pub triggers: i32 = 0,
        pub collisions: i32 = 0,
    }
}

impl Component for Counter {
    fn on_trigger_enter(&mut self, _ctx: &mut Ctx, _other: GameObjectId) {
        self.triggers += 1;
    }
    fn on_collision_enter(&mut self, _ctx: &mut Ctx, _other: GameObjectId) {
        self.collisions += 1;
    }
}

#[test]
fn triggers_and_collisions_call_component_hooks() {
    let mut app = app_with_ground();
    let zone = app
        .spawn("Zone")
        .at3(0.0, 2.0, 0.0)
        .with(Collider::cuboid(2.0, 0.5, 2.0).trigger())
        .with(Counter::default())
        .id();
    let ball = app
        .spawn("Ball")
        .at3(0.0, 5.0, 0.0)
        .with(Rigidbody::dynamic())
        .with(Collider::sphere(0.3))
        .with(Counter::default())
        .id();
    app.run_headless(150, DT);

    let zone_counter = app.world.scene.get_component::<Counter>(zone).unwrap();
    assert_eq!(zone_counter.triggers, 1, "the ball passed through the zone once");
    let ball_counter = app.world.scene.get_component::<Counter>(ball).unwrap();
    assert_eq!(ball_counter.triggers, 1);
    assert!(ball_counter.collisions >= 1, "and then hit the ground");
    assert!((position(&app, ball).y - 0.3).abs() < 0.03, "a trigger does not stop bodies");
}

#[test]
fn joints_weld_hang_and_spin() {
    let mut app = app_with_ground();
    let welded = crate_at(&mut app, "Welded", -4.0, 5.0, 0.0);
    let bob = app
        .spawn("Bob")
        .at3(2.0, 3.0, 0.0)
        .with(Rigidbody::dynamic())
        .with(Collider::sphere(0.3))
        .id();
    let wheel = app
        .spawn("Wheel")
        .at3(6.0, 3.0, 0.0)
        .with(Rigidbody::dynamic())
        .with(Collider::cylinder(0.5, 0.3))
        .id();
    app.run_headless(1, DT); // тела созданы

    let anchor = Vec3::new(0.0, 5.0, 0.0);
    let physics = app.world.resources.get_mut::<Physics3D>().unwrap();
    physics.fixed_joint(welded, None);
    physics.rope_joint(bob, Vec3::new(2.0, 3.0, 0.0), None, anchor, 0.0);
    let hinge = physics.hinge_joint(wheel, None, Vec3::new(6.0, 3.0, 0.0), Vec3::Z);
    physics.set_hinge_motor(hinge, 4.0, 1.0e4);
    let rope = anchor.distance(Vec3::new(2.0, 3.0, 0.0));
    app.run_headless(180, DT);

    // Сварка с миром держит ящик в воздухе.
    assert!((position(&app, welded) - Vec3::new(-4.0, 5.0, 0.0)).length() < 0.05, "{}", position(&app, welded));
    // Маятник качается, но не дальше длины верёвки, и не падает на землю.
    let swing = position(&app, bob);
    assert!(anchor.distance(swing) <= rope + 0.05, "rope length is kept: {}", anchor.distance(swing));
    assert!(swing.y > 1.5 && (swing.x - 2.0).abs() > 0.05, "it hangs and swings: {swing}");
    // Петля держит колесо на месте, мотор крутит его вокруг оси Z.
    assert!((position(&app, wheel) - Vec3::new(6.0, 3.0, 0.0)).length() < 0.05);
    let spin = app.world.scene.get_component::<Rigidbody>(wheel).unwrap().angular_velocity;
    assert!((spin.z.abs() - 4.0).abs() < 0.6 && spin.x.abs() < 0.2, "motor speed: {spin}");

    // Сустав можно убрать — колесо падает.
    app.world.resources.get_mut::<Physics3D>().unwrap().remove_joint(hinge);
    app.run_headless(120, DT);
    assert!(position(&app, wheel).y < 1.0);
}

component! {
    /// Ведёт персонажа с постоянной скоростью; прыгает по кнопке Jump.
    pub struct Walker {
        pub speed: Vec3 = Vec3::new(3.0, 0.0, 0.0),
        pub jump_speed: f32 = 5.0,
    }
}

impl Component for Walker {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let jump = ctx.input().button_down("Jump");
        let (speed, jump_speed) = (self.speed, self.jump_speed);
        if let Some(character) = ctx.get_mut::<CharacterController>() {
            character.velocity.x = speed.x;
            character.velocity.z = speed.z;
            if jump && character.grounded {
                character.velocity.y = jump_speed;
            }
        }
    }
}

#[test]
fn character_controller_walks_climbs_steps_and_stops_at_walls() {
    let mut app = app_with_ground();
    app.register::<Walker>();
    // Ступенька высотой 0.25 м на пути и стена дальше.
    app.spawn("Step").at3(2.0, 0.125, 0.0).with(Collider::cuboid(1.0, 0.25, 4.0));
    app.spawn("Wall").at3(6.0, 1.5, 0.0).with(Collider::cuboid(1.0, 3.0, 8.0));
    let hero = app
        .spawn("Hero")
        .at3(-1.0, 0.0, 0.0)
        .with(CharacterController::default())
        .with(Walker::default())
        .id();

    app.run_headless(18, DT);
    let character = app.world.scene.get_component::<CharacterController>(hero).unwrap();
    assert!(character.grounded, "landed");
    let p = position(&app, hero);
    assert!(p.y.abs() < 0.06, "feet on the ground: {p}");
    assert!((p.x - (-0.1)).abs() < 0.2, "walking at 3 m/s: {p}");

    // Через ступеньку персонаж проходит сам.
    let mut highest = 0.0_f32;
    for _ in 0..50 {
        app.tick(DT);
        highest = highest.max(position(&app, hero).y);
    }
    assert!((highest - 0.25).abs() < 0.08, "stepped onto the 0.25 m ledge: {highest}");

    // В стену он упирается: дальше x = 5.5 − радиус не пройти.
    app.run_headless(180, DT);
    let p = position(&app, hero);
    assert!((p.x - (5.5 - 0.35)).abs() < 0.08, "stopped by the wall: {p}");
    assert!(p.y.abs() < 0.06);

    // Прыжок: v² / 2g = 25 / 19.62 ≈ 1.27 м.
    app.world.scene.get_component_mut::<Walker>(hero).unwrap().speed = Vec3::ZERO;
    app.run_headless(10, DT);
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    let mut peak = 0.0_f32;
    for _ in 0..120 {
        app.tick(DT);
        peak = peak.max(position(&app, hero).y);
    }
    assert!((peak - 1.27).abs() < 0.15, "jump height: {peak}");
    assert!(position(&app, hero).y.abs() < 0.06, "and back on the ground");
}

#[test]
fn motion_is_smooth_between_physics_steps() {
    // Монитор 144 Гц, физика 60 Гц: персонаж должен сдвигаться каждый кадр одинаково.
    let mut app = app_with_ground();
    app.register::<Walker>();
    let hero = app
        .spawn("Hero")
        .at3(0.0, 0.0, 0.0)
        .with(CharacterController::default())
        .with(Walker::default())
        .id();
    let dt = 1.0 / 144.0;
    app.run_headless(100, dt);
    let expected = 3.0 * dt;
    let mut previous = position(&app, hero).x;
    for frame in 0..72 {
        app.tick(dt);
        let x = position(&app, hero).x;
        assert!(
            ((x - previous) - expected).abs() < expected * 0.15,
            "frame {frame}: moved {:.5} instead of {expected:.5}",
            x - previous
        );
        previous = x;
    }
}

#[test]
fn physics_components_are_saved_with_the_scene() {
    let mut app = app_with_ground();
    crate_at(&mut app, "Crate", 0.0, 3.0, 0.0);
    app.spawn("Hero").at3(8.0, 0.0, 0.0).with(CharacterController::default());
    let saved = app.world.scene_to_string();
    assert!(saved.contains(r#"kind: "Rigidbody""#) && saved.contains(r#"kind: "CharacterController""#), "{saved}");

    app.world.load_scene_str(&saved, "saved.scene").unwrap();
    let crate_id = app.world.scene.find_by_name("Crate").unwrap();
    app.run_headless(180, DT);
    assert!((position(&app, crate_id).y - 0.5).abs() < 0.05, "the loaded scene simulates");
}

/// Запросы от имени объекта (`ignore` / `QueryFilter3D::ignoring`) соблюдают матрицу слоёв:
/// стрелок слоя 1 не видит стену слоя 2, если слои 1 и 2 не сталкиваются. Без запрета —
/// как раньше.
#[test]
fn queries_from_an_object_honour_the_layer_matrix() {
    let mut app = app_with_ground();
    let shooter = app.spawn("Shooter").at3(0.0, 1.0, 0.0).layer(1).with(Rigidbody::kinematic()).with(Collider::sphere(0.3)).id();
    let wall = app.spawn("Wall").at3(5.0, 1.0, 0.0).layer(2).with(Collider::cuboid(1.0, 2.0, 4.0)).id();
    let back = app.spawn("Back").at3(10.0, 1.0, 0.0).with(Collider::cuboid(1.0, 2.0, 4.0)).id();
    app.run_headless(2, DT);
    let shoot = |app: &App| {
        let physics = app.world.resources.get::<Physics3D>().unwrap();
        let ray = physics.raycast(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 30.0, Some(shooter)).map(|h| h.object);
        let cast = physics
            .cast_shape(CastShape3D::Sphere { radius: 0.2 }, Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY, Vec3::X * 30.0, QueryFilter3D::ignoring(shooter))
            .map(|h| h.object);
        // Без объекта (из-за края шара стрелка, чтобы не попасть в него самого).
        let anonymous = physics.raycast(Vec3::new(0.5, 1.0, 0.0), Vec3::X, 30.0, None).map(|h| h.object);
        (ray, cast, anonymous)
    };
    assert_eq!(shoot(&app), (Some(wall), Some(wall), Some(wall)), "default matrix: the wall blocks");
    let mut settings = engine::core::settings::Physics3dSettings::default();
    settings.layer_matrix.set(1, 2, false);
    app.world.resources.get_mut::<Physics3D>().unwrap().apply_settings(&settings);
    // От имени стрелка стена не видна; запрос без объекта не знает слоя — видит всё.
    assert_eq!(shoot(&app), (Some(back), Some(back), Some(wall)));
}
