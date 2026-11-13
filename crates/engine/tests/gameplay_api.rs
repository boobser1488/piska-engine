//! Геймплей-API через фасад: то, что должно работать вместе с физикой, рендером и
//! геймпадами. Без окна; картинка — через `Renderer::headless()`.

use engine::prelude::*;
use engine::render::Renderer;

const DT: f32 = 1.0 / 60.0;

fn y(app: &App, id: GameObjectId) -> f32 {
    app.world.scene.world_position(id).y
}

component! {
    /// Бежит по оси `Horizontal` — клавиатура или геймпад, скрипту всё равно.
    pub struct Runner {
        pub speed: f32 = 5.0,
    }
}
impl Component for Runner {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let run = ctx.input().axis("Horizontal") * self.speed;
        if let Some(body) = ctx.get_mut::<Rigidbody2D>() {
            body.velocity.x = run;
        }
    }
}

component! {
    pub struct Door { pub target: String = String::from("b") }
}
impl Component for Door {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if ctx.input().button_down("Jump") {
            let target = self.target.clone();
            ctx.load_scene(&target);
        }
    }
}

fn physics_app() -> App {
    let mut app = engine::app();
    app.register::<Runner>().register::<Door>();
    app
}

fn falling_box(app: &mut App, at_y: f32) -> GameObjectId {
    app.spawn("Box")
        .at(0.0, at_y)
        .with(Rigidbody2D::dynamic())
        .with(Collider2D::rect(1.0, 1.0))
        .id()
}

fn ground(app: &mut App) -> GameObjectId {
    app.spawn("Ground").at(0.0, -0.5).with(Collider2D::rect(40.0, 1.0)).id()
}

// ----------------------------------------------------------------------
// Фасад
// ----------------------------------------------------------------------

#[test]
fn the_facade_prelude_exposes_the_whole_gameplay_api() {
    // Всё нужное игре — из одного `use engine::prelude::*`.
    let mut app = engine::app();
    let id = app.spawn("X").id();
    let _ = (
        Ease::OutBack,
        Tween::position(id, Vec3::X).duration(0.1).ease(Ease::InOutQuad).yoyo().loops(2),
        Sequence::new().wait(0.1),
        Timer::once(1.0),
        Cooldown::new(1.0),
        Random::new(1),
        Rect::new(0.0, 0.0, 1.0, 1.0),
        StateMachine::new(Ease::Linear),
        SaveData::in_memory(),
        MemoryScenes::default(),
        Binding::Key(Key::Space),
        Binding::Pad(GamepadButton::South),
        AxisBindings::pair(Key::A, Key::D).with_analog(GamepadAxis::LeftStickX),
        GamepadId(0),
        lerp(0.0, 1.0, 0.5) + smoothstep(0.5) + remap(0.5, 0.0, 1.0, 0.0, 2.0) + delta_angle(0.0, 1.0),
        Color::from_hsv(0.5, 1.0, 1.0).lerp(Color::WHITE, 0.5),
    );
    app.run_headless(3, DT);
}

#[test]
fn the_gamepad_subsystem_is_wired_in_and_harmless_without_a_pad() {
    let mut app = engine::app();
    app.run_headless(10, DT);
    // Своего геймпада у CI может не быть, а у разработчика быть — проверяем только согласованность.
    assert_eq!(app.world.input.gamepads().len(), app.world.input.gamepad_count());
    assert!(app.world.input.axis("Horizontal").abs() <= 1.0);
}

// ----------------------------------------------------------------------
// Геймпад → действия → физика
// ----------------------------------------------------------------------

#[test]
fn a_gamepad_stick_drives_a_physics_body_through_the_old_axis_api() {
    let mut app = physics_app();
    let body = app
        .spawn("Runner")
        .with(Rigidbody2D { gravity_scale: 0.0, ..Rigidbody2D::dynamic() })
        .with(Collider2D::rect(1.0, 1.0))
        .with(Runner::default())
        .id();
    app.run_headless(5, DT);
    assert!(app.world.scene.world_position(body).x.abs() < 1e-3, "no input, no movement");

    let pad = GamepadId(0);
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, 1.0);
    app.run_headless(60, DT);
    let x = app.world.scene.world_position(body).x;
    assert!((4.0..6.0).contains(&x), "one second at 5 m/s: x = {x}");

    // Кнопка геймпада тоже работает в on_fixed_update, даже если она нажата на один кадр.
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, 0.0);
    app.run_headless(5, DT);
    let rest = app.world.scene.world_position(body).x;
    app.world.input.set_gamepad_button(pad, GamepadButton::DPadLeft, true);
    app.run_headless(30, DT);
    assert!(app.world.scene.world_position(body).x < rest - 1.0, "d-pad steers too");
}

// ----------------------------------------------------------------------
// Активность и физика / рендер
// ----------------------------------------------------------------------

#[test]
fn inactive_objects_do_not_collide_and_do_not_fall() {
    let mut app = physics_app();
    let floor = ground(&mut app);
    let boxed = falling_box(&mut app, 3.0);
    app.run_headless(150, DT);
    assert!((y(&app, boxed) - 0.5).abs() < 0.1, "rests on the ground: {}", y(&app, boxed));

    // Пол выключили — коробка проваливается; включили — ловит то, что осталось.
    app.world.scene.set_active(floor, false);
    app.run_headless(60, DT);
    let fallen = y(&app, boxed);
    assert!(fallen < -1.0, "no floor, no support: {fallen}");

    // Выключенное тело стоит на месте, включённое — падает.
    let sleeper = falling_box(&mut app, 50.0);
    app.world.scene.set_active(sleeper, false);
    app.run_headless(60, DT);
    assert_eq!(y(&app, sleeper), 50.0);
    app.world.scene.set_active(sleeper, true);
    app.run_headless(30, DT);
    assert!(y(&app, sleeper) < 49.0, "falls once active: {}", y(&app, sleeper));

    // Выключенный родитель выключает и детей.
    let parent = app.spawn("Parent").at(20.0, 30.0).id();
    let child = app
        .spawn("Child")
        .child_of(parent)
        .with(Rigidbody2D::dynamic())
        .with(Collider2D::rect(1.0, 1.0))
        .id();
    app.world.scene.set_active(parent, false);
    app.run_headless(30, DT);
    assert_eq!(y(&app, child), 30.0, "a child follows its parent's activity");
}

#[test]
fn inactive_objects_are_not_drawn() {
    let mut app = engine::app();
    app.spawn("Camera").with(Camera { ortho_size: 2.0, ..Camera::default() });
    let sprite = app
        .spawn("Square")
        .with(SpriteRenderer::colored(Color::WHITE))
        .scale(2.0, 2.0, 1.0)
        .id();
    app.run_headless(2, DT);

    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let center = |app: &App, renderer: &mut Renderer| {
        let image = renderer.capture(&app.world, 32, 32).expect("capture");
        image.get_pixel(16, 16).0
    };
    let visible = center(&app, &mut renderer);
    assert!(visible[0] > 200 && visible[1] > 200, "white square drawn: {visible:?}");

    app.world.scene.set_active(sprite, false);
    let hidden = center(&app, &mut renderer);
    assert!(hidden[0] < 100, "the inactive square is gone: {hidden:?}");
    app.world.scene.set_active(sprite, true);
    assert_eq!(center(&app, &mut renderer), visible);
}

// ----------------------------------------------------------------------
// Твины по путям реальных компонентов
// ----------------------------------------------------------------------

#[test]
fn tweens_reach_fields_of_engine_components_by_path() {
    let mut app = engine::app();
    let id = app
        .spawn("Sprite")
        .with(SpriteRenderer::colored(Color::WHITE))
        .with(Rigidbody2D { gravity_scale: 0.0, ..Rigidbody2D::dynamic() })
        .id();
    app.world.start_tween(Tween::field(id, "SpriteRenderer.color", Color::RED).duration(0.5).ease(Ease::OutQuad));
    app.world.start_tween(Tween::field(id, "SpriteRenderer.color.a", 0.25f32).duration(0.5));
    app.run_headless(10, DT);
    let mid = app.world.scene.get_component::<SpriteRenderer>(id).unwrap().color;
    assert!(mid.g < 1.0 && mid.g > 0.0 && mid.a < 1.0, "{mid:?}");
    app.run_headless(40, DT);
    assert_eq!(
        app.world.scene.get_component::<SpriteRenderer>(id).unwrap().color,
        Color::RED.with_alpha(0.25)
    );
}

// ----------------------------------------------------------------------
// Смена сцены из скрипта с физикой
// ----------------------------------------------------------------------

fn level_scenes() -> MemoryScenes {
    let body = r#"components: [
        (kind: "Rigidbody2D", fields: {"gravity_scale": Float(0.0)}),
        (kind: "Collider2D", fields: {}),
    ]"#;
    MemoryScenes::default()
        .with("a", format!(r#"(name: "A", objects: [(name: "A1", {body}), (name: "Door", components: [(kind: "Door", fields: {{}})])])"#))
        .with(
            "b",
            format!(r#"(name: "B", objects: [(name: "B1", {body}), (name: "B2", {body}), (name: "B3", {body})])"#),
        )
}

#[test]
fn loading_a_scene_from_a_script_swaps_physics_bodies_cleanly() {
    let mut app = physics_app();
    app.world.scenes().set_source(level_scenes());
    app.world.scenes().load("a");
    app.run_headless(3, DT);
    assert_eq!(app.world.resources.get::<Physics2D>().unwrap().body_count(), 1);
    assert!(app.world.scene.find_by_name("Door").is_some());

    // Нажатие кнопки читает скрипт в on_update: сцена заменится в конце этого кадра.
    app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::South, true);
    app.tick(DT);
    app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::South, false);
    assert_eq!(app.world.scene.name, "B");
    assert!(app.world.scene.find_by_name("Door").is_none() && app.world.scene.find_by_name("B3").is_some());
    app.run_headless(3, DT);
    assert_eq!(
        app.world.resources.get::<Physics2D>().unwrap().body_count(),
        3,
        "old bodies are gone, the new scene's bodies exist"
    );
}

#[test]
fn time_scale_zero_freezes_physics_and_resumes() {
    let mut app = physics_app();
    ground(&mut app);
    let boxed = falling_box(&mut app, 10.0);
    app.run_headless(10, DT);
    app.world.time.time_scale = 0.0;
    let frozen = y(&app, boxed);
    app.run_headless(120, DT);
    assert_eq!(y(&app, boxed), frozen, "paused physics does not move a thing");
    app.world.time.time_scale = 1.0;
    app.run_headless(30, DT);
    assert!(y(&app, boxed) < frozen - 0.5);
}
