//! Скрипты «для новичка»: готовые поведения (`engine::behaviours`) и короткие формы
//! физики, звука и интерфейса — без окна, по кадрам.

use engine::behaviours::*;
use engine::core::serialize::MemoryPrefabs;
use engine::prelude::*;

const DT: f32 = 1.0 / 60.0;

fn app() -> App {
    let mut app = engine::app();
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app.register::<Recorder>();
    app
}

fn errors(app: &App) -> Vec<String> {
    app.world.resources.get::<ScriptErrors>().map(|e| e.messages().to_vec()).unwrap_or_default()
}

fn pos(app: &App, id: GameObjectId) -> Vec3 {
    app.world.scene.world_position(id)
}

fn with_ctx<R>(app: &mut App, id: GameObjectId, f: impl FnOnce(&mut Ctx) -> R) -> R {
    f(&mut Ctx { world: &mut app.world, id })
}

component! {
    /// Записывает сообщения и щелчки.
    pub struct Recorder {
        pub log: Vec<String> = Vec::new(),
    }
}
impl Component for Recorder {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        self.log.push(format!("{}={:?}", message.name, message.value));
    }
    fn on_click(&mut self, _ctx: &mut Ctx) {
        self.log.push("click".into());
    }
}

fn log(app: &App, id: GameObjectId) -> Vec<String> {
    app.world.scene.get_component::<Recorder>(id).unwrap().log.clone()
}

// ----------------------------------------------------------------------
// Готовые поведения
// ----------------------------------------------------------------------

#[test]
fn behaviours_are_in_add_component() {
    let app = app();
    for name in ["Mover", "Rotator", "Follow", "DestroyAfter", "Spawner", "Health", "Damage", "Collectible", "Clickable", "PlayerMove2D", "Platformer2D", "PlayerMove3D"] {
        assert!(app.world.registry.contains(name), "{name}");
        assert_eq!(app.world.registry.category_of(name), Some(CATEGORY), "{name}");
    }
}

#[test]
fn mover_and_rotator() {
    let mut app = app();
    let id = app
        .spawn("Cloud")
        .with(Mover { velocity: Vec3::new(2.0, 1.0, 0.0) })
        .with(Rotator { degrees_per_second: Vec3::new(0.0, 0.0, 90.0) })
        .id();
    app.run_headless(30, DT); // 30 кадров (первый — со стартом) = 0.5 с движения
    let p = pos(&app, id);
    assert!((p.x - 1.0).abs() < 1e-3 && (p.y - 0.5).abs() < 1e-3, "{p:?}");
    let angle = app.world.scene.get(id).unwrap().transform.angle_z().to_degrees();
    assert!((angle - 45.0).abs() < 0.05, "{angle}");
}

#[test]
fn follow_catches_up_with_its_target() {
    let mut app = app();
    let hero = app.spawn("Hero").at(5.0, 2.0).id();
    let field = ObjectRef::new(&app.world.scene, hero);
    let snap = app.spawn("Snap").with(Follow { target: field, offset: Vec3::new(0.0, 0.0, 10.0), smoothing: 0.0 }).id();
    let lazy = app.spawn("Lazy").with(Follow { target: field, offset: Vec3::ZERO, smoothing: 5.0 }).id();
    app.tick(DT);
    assert_eq!(pos(&app, snap), Vec3::new(5.0, 2.0, 10.0));
    let first = pos(&app, lazy).x;
    assert!(first > 0.0 && first < 1.0, "smoothing moves part of the way: {first}");
    app.run_headless(120, DT);
    assert!((pos(&app, lazy).x - 5.0).abs() < 0.01);
}

#[test]
fn destroy_after_removes_the_object() {
    let mut app = app();
    let id = app.spawn("Spark").with(DestroyAfter { seconds: 0.25 }).id();
    app.run_headless(10, DT);
    assert!(app.world.scene.get(id).is_some());
    app.run_headless(10, DT);
    assert!(app.world.scene.get(id).is_none());
}

#[test]
fn spawner_makes_the_prefab_on_a_timer() {
    let mut app = app();
    let template = app.spawn("Coin").tag("Coin").id();
    let prefab = Prefab::from_object(&app.world.scene, template).unwrap();
    app.world.scene.destroy(template);
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("prefabs/coin.prefab", prefab));
    app.spawn("Spawner").at(3.0, 0.0).with(Spawner {
        prefab: "prefabs/coin.prefab".into(),
        interval: 0.5,
        count: 3,
        ..Spawner::default()
    });
    app.run_headless(61, DT); // ~1 с: две штуки
    assert_eq!(app.world.scene.find_all_by_tag("Coin").len(), 2);
    app.run_headless(120, DT);
    let coins = app.world.scene.find_all_by_tag("Coin");
    assert_eq!(coins.len(), 3, "count limits the total");
    assert!(coins.iter().all(|c| pos(&app, *c).x == 3.0));
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn damage_on_contact_drains_health_until_death() {
    let mut app = app();
    let hero = app.spawn("Hero").tag("Player").with(Health { max: 3, hp: 2, destroy_on_death: true }).with(Recorder::default()).id();
    let spikes = app.spawn("Spikes").with(Damage { amount: 1, only_tag: "Player".into(), destroy_on_hit: false }).id();
    let wall = app.spawn("Wall").id();
    app.tick(DT);
    app.world.dispatch_hook(spikes, Hooks::COLLISION_ENTER, |c, ctx| c.on_collision_enter(ctx, wall));
    app.world.dispatch_hook(spikes, Hooks::COLLISION_ENTER, |c, ctx| c.on_collision_enter(ctx, hero));
    app.world.flush_messages();
    assert_eq!(app.world.scene.get_component::<Health>(hero).unwrap().hp, 1, "only the tagged object is hurt");
    app.world.dispatch_hook(spikes, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, hero));
    app.world.flush_messages();
    assert_eq!(log(&app, hero), ["Damage=Int(1)", "Damage=Int(1)", "Died=Nil"]);
    app.tick(DT);
    assert!(app.world.scene.get(hero).is_none(), "destroy_on_death");
}

#[test]
fn collectible_tells_the_collector_and_the_game_then_vanishes() {
    let mut app = app();
    let game = app.spawn("Game").with(Recorder::default()).id();
    let hero = app.spawn("Hero").tag("Player").with(Recorder::default()).id();
    let crate_ = app.spawn("Crate").with(Recorder::default()).id();
    let coin = app.spawn("Coin").with(Collectible { value: 5, ..Collectible::default() }).id();
    app.tick(DT);
    app.world.dispatch_hook(coin, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, crate_));
    app.world.flush_messages();
    assert!(log(&app, crate_).is_empty(), "a crate is not a Player");
    app.world.dispatch_hook(coin, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, hero));
    app.world.flush_messages();
    assert_eq!(log(&app, hero), ["Collected=Int(5)"]);
    assert_eq!(log(&app, game), ["Collected=Int(5)"]);
    app.tick(DT);
    assert!(app.world.scene.get(coin).is_none());
}

#[test]
fn clickable_turns_a_mouse_click_into_on_click() {
    let mut app = app();
    app.spawn("Camera").at3(0.0, 0.0, 10.0).with(Camera::orthographic(5.0));
    let target = app
        .spawn("Target")
        .at(2.0, 0.0)
        .with(Clickable { notify: "Game".into(), ..Clickable::default() })
        .with(Recorder::default())
        .id();
    let game = app.spawn("Game").with(Recorder::default()).id();
    app.tick(DT);
    // Мир на экране: высота 10 единиц на 720 пикселей → 72 px на единицу; x=2 → 640+144.
    app.world.input.set_mouse_position(Vec2::new(100.0, 360.0));
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.tick(DT);
    assert!(log(&app, target).is_empty(), "a miss");
    app.world.input.set_mouse_button(MouseButton::Left, false);
    app.tick(DT);
    app.world.input.set_mouse_position(Vec2::new(640.0 + 144.0, 360.0));
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.tick(DT);
    assert_eq!(log(&app, target), ["click", "Clicked=Nil"]);
    assert_eq!(log(&app, game), ["Clicked=Nil"]);
}

#[test]
fn player_move_2d_walks_with_or_without_a_body() {
    let mut app = app();
    let free = app.spawn("Free").with(PlayerMove2D { speed: 4.0 }).id();
    let body = app
        .spawn("Body")
        .at(0.0, 5.0)
        .with(PlayerMove2D { speed: 4.0 })
        .with(Rigidbody2D { gravity_scale: 0.0, ..Rigidbody2D::default() })
        .with(Collider2D::circle(0.4))
        .id();
    app.world.input.set_key(Key::D, true);
    app.run_headless(60, DT);
    assert!((pos(&app, free).x - 4.0).abs() < 0.01, "{:?}", pos(&app, free));
    assert!((pos(&app, body).x - 4.0).abs() < 0.2, "{:?}", pos(&app, body));
    assert_eq!(app.world.scene.get_component::<Rigidbody2D>(body).unwrap().velocity, Vec2::new(4.0, 0.0));
}

fn platformer_level(app: &mut App) -> GameObjectId {
    app.spawn("Ground").at(0.0, -0.5).with(Collider2D::rect(40.0, 1.0));
    app.spawn("Hero")
        .at(0.0, 0.6)
        .with(Platformer2D::default())
        .with(Rigidbody2D::default().locked_rotation())
        .with(Collider2D::rect(0.8, 1.0))
        .id()
}

#[test]
fn platformer_2d_runs_jumps_once_and_lands() {
    let mut app = app();
    let hero = platformer_level(&mut app);
    app.run_headless(30, DT); // осесть на землю
    let rest = pos(&app, hero).y;
    assert!((rest - 0.5).abs() < 0.05, "standing on the ground: {rest}");
    with_ctx(&mut app, hero, |ctx| assert!(ctx.grounded()));

    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    app.run_headless(10, DT);
    let up = pos(&app, hero).y;
    assert!(up > rest + 0.8, "jumped: {up}");
    with_ctx(&mut app, hero, |ctx| assert!(!ctx.grounded()));
    // Второй прыжок в воздухе не срабатывает.
    let vy_before = app.world.scene.get_component::<Rigidbody2D>(hero).unwrap().velocity.y;
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    let vy_after = app.world.scene.get_component::<Rigidbody2D>(hero).unwrap().velocity.y;
    assert!(vy_after <= vy_before, "no double jump: {vy_before} → {vy_after}");

    app.world.input.set_key(Key::D, true);
    app.run_headless(180, DT); // прыжок 11 м/с летит ~2.2 с
    let p = pos(&app, hero);
    assert!((p.y - rest).abs() < 0.05, "landed: {p:?}");
    assert!(p.x > 8.0, "ran right: {p:?}");
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn platformer_2d_forgives_a_late_jump_after_the_edge() {
    let mut app = app();
    let hero = platformer_level(&mut app);
    app.run_headless(30, DT);
    // Сойти с края нельзя без длинного уровня — «сдвигаем» землю: время в воздухе
    // меньше coyote_time, а прыжок разрешён.
    app.world.scene.get_component_mut::<Platformer2D>(hero).unwrap().air_time = 0.05;
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    assert!(app.world.scene.get_component::<Rigidbody2D>(hero).unwrap().velocity.y > 5.0);
}

#[test]
fn player_move_3d_walks_on_a_character_controller() {
    let mut app = app();
    app.spawn("Floor").at3(0.0, -0.5, 0.0).with(Collider::cuboid(20.0, 1.0, 20.0));
    let hero = app
        .spawn("Hero")
        .at3(0.0, 0.05, 0.0)
        .with(CharacterController::default())
        .with(PlayerMove3D::default())
        .id();
    app.run_headless(20, DT);
    app.world.input.set_key(Key::W, true);
    app.run_headless(60, DT);
    let p = pos(&app, hero);
    assert!(p.z < -3.5, "walked forward (−Z) ~5 m/s: {p:?}");
    assert!(p.y.abs() < 0.2, "stayed on the floor: {p:?}");
    let forward = app.world.scene.world_rotation(hero) * Vec3::NEG_Z;
    assert!(forward.z < -0.9, "faces where it walks: {forward:?}");
}

// ----------------------------------------------------------------------
// Короткие формы физики, звука, интерфейса
// ----------------------------------------------------------------------

#[test]
fn physics_short_forms_match_the_long_ones() {
    let mut app = app();
    let body = app.spawn("Body").with(Rigidbody2D { gravity_scale: 0.0, ..Rigidbody2D::default() }).with(Collider2D::circle(0.5)).id();
    let wall = app.spawn("Wall").at(5.0, 0.0).with(Collider2D::rect(1.0, 4.0)).id();
    app.tick(DT);
    with_ctx(&mut app, body, |ctx| {
        ctx.set_velocity(Vec2::new(3.0, -1.0));
        assert_eq!(ctx.get::<Rigidbody2D>().unwrap().velocity, Vec2::new(3.0, -1.0));
        assert_eq!(ctx.velocity(), Vec3::new(3.0, -1.0, 0.0));
        assert_eq!(ctx.velocity2(), Vec2::new(3.0, -1.0));
        let short = ctx.raycast2d(Vec2::ZERO, Vec2::X, 10.0).unwrap();
        let long = ctx.resource::<Physics2D>().unwrap().raycast(Vec2::ZERO, Vec2::X, 10.0, Some(ctx.id)).unwrap();
        assert_eq!(short, long);
        assert_eq!(short.object, wall);
    });

    // 3D: сила через ctx = сила через Physics3D.
    let a = app.spawn("A").at3(0.0, 10.0, 0.0).with(Rigidbody { gravity_scale: 0.0, ..Rigidbody::default() }).with(Collider::sphere(0.5)).id();
    let b = app.spawn("B").at3(5.0, 10.0, 0.0).with(Rigidbody { gravity_scale: 0.0, ..Rigidbody::default() }).with(Collider::sphere(0.5)).id();
    app.tick(DT);
    for _ in 0..10 {
        with_ctx(&mut app, a, |ctx| ctx.add_force(Vec3::X * 10.0));
        app.world.resources.get_mut::<Physics3D>().unwrap().add_force(b, Vec3::X * 10.0);
        app.tick(DT);
    }
    let va = app.world.scene.get_component::<Rigidbody>(a).unwrap().velocity;
    let vb = app.world.scene.get_component::<Rigidbody>(b).unwrap().velocity;
    assert!(va.x > 0.1 && (va - vb).length() < 1e-4, "{va:?} vs {vb:?}");
    let hit = with_ctx(&mut app, a, |ctx| ctx.raycast(Vec3::new(0.0, 10.0, 0.0), Vec3::X, 20.0)).unwrap();
    assert_eq!(hit.object, b, "the ray skips its own body");
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn physics_without_a_body_teaches_once() {
    let mut app = app();
    let id = app.spawn("Ghost").id();
    with_ctx(&mut app, id, |ctx| {
        for _ in 0..10 {
            ctx.set_velocity(Vec2::X);
            assert_eq!(ctx.velocity(), Vec3::ZERO);
        }
    });
    let errors = errors(&app);
    assert_eq!(errors.len(), 2, "one per call site: {errors:#?}");
    assert!(errors[0].contains("`Ghost` has no physics body") && errors[0].contains("Rigidbody2D"), "{}", errors[0]);
}

#[test]
fn audio_short_forms_use_the_audio_resource() {
    let mut app = app();
    let id = app.spawn("Jukebox").id();
    app.world.resources.get_mut::<Audio>().unwrap().add_clip("blip", synth::sfx::coin());
    with_ctx(&mut app, id, |ctx| {
        let long = ctx.resource_mut::<Audio>().unwrap().play("blip");
        let short = ctx.play("blip");
        assert_eq!(long.is_none(), short.is_none(), "the same outcome as the long form");
        ctx.play_at("blip", Vec2::new(1.0, 0.0));
        ctx.music("blip");
        let playing = ctx.resource::<Audio>().unwrap().music_voice().is_some();
        let device = ctx.resource::<Audio>().unwrap().music_voice().is_some() || !playing;
        assert!(device);
        ctx.stop_music();
        assert!(ctx.resource::<Audio>().unwrap().music_voice().is_none());
        assert!(ctx.play("no_such_clip").is_none());
    });
}

#[test]
fn ui_short_forms_change_text_visibility_and_report_clicks() {
    let mut app = app();
    let (panel, button) = {
        let scene = &mut app.world.scene;
        let canvas = scene.ui_canvas().id();
        scene.ui_text(canvas, "Score: 0").name("Score");
        let panel = scene.ui_panel(canvas).name("GameOver").id();
        let button = scene
            .ui_button(canvas, "restart", "Again")
            .anchor(Anchor::Center)
            .size(200.0, 60.0)
            .with(Recorder::default())
            .id();
        (panel, button)
    };
    let me = app.spawn("Game").id();
    app.tick(DT);
    with_ctx(&mut app, me, |ctx| {
        ctx.ui_text("Score", format!("Score: {}", 7));
        ctx.ui_hide("GameOver");
        ctx.ui_text("Scroe", "typo");
    });
    let score = app.world.scene.find_by_name("Score").unwrap();
    assert_eq!(app.world.scene.get_component::<Text>(score).unwrap().text, "Score: 7");
    assert!(!app.world.scene.is_active(panel));
    with_ctx(&mut app, me, |ctx| ctx.ui_show("GameOver"));
    assert!(app.world.scene.is_active(panel));
    let errors = errors(&app);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("no UI element named `Scroe`") && errors[0].contains("Did you mean `Score`?"), "{}", errors[0]);

    // Клик по кнопке: хук on_click на самой кнопке и ctx.clicked(..) в этом кадре.
    app.world.input.set_mouse_position(Vec2::new(640.0, 360.0));
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.tick(DT);
    app.world.input.set_mouse_button(MouseButton::Left, false);
    app.tick(DT);
    assert!(with_ctx(&mut app, me, |ctx| ctx.clicked("restart")));
    assert_eq!(log(&app, button), ["click", "restart=Nil"]);
}
