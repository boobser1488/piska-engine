//! То, на чём стоит «Звездопад» (`games/template`), по отдельности и вместе: объявление
//! игры `engine::game!`, экран 2D (`ctx.view_rect`, `ctx.pointer_world`), готовые
//! поведения `Paddle2D`, `Spawner` (по ширине экрана, пауза, интервал на ходу) и
//! `OffScreen`, встроенные звуки `"sfx:…"` / `"music:…"`, `ctx.reload_scene()` после
//! `engine::load_scene`. Последний тест — пример «целая игра» из `docs/api/gameplay.md`.

use std::path::PathBuf;

use engine::behaviours::{Mover, OffScreen, Paddle2D, Spawner};
use engine::core::serialize::MemoryPrefabs;
use engine::prelude::*;

const DT: f32 = 1.0 / 60.0;

fn app() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app.spawn("Main Camera").with(Camera::orthographic(6.0));
    app
}

fn pos(app: &App, id: GameObjectId) -> Vec3 {
    app.world.scene.world_position(id)
}

fn with_ctx<R>(app: &mut App, id: GameObjectId, f: impl FnOnce(&mut Ctx) -> R) -> R {
    f(&mut Ctx { world: &mut app.world, id })
}

/// Половина видимой ширины при `ortho_size` 6 и экране 16 : 9.
const HALF_WIDTH: f32 = 6.0 * 1280.0 / 720.0;

component! {
    /// Записывает полученные сообщения.
    pub struct Recorder {
        pub log: Vec<String> = Vec::new(),
    }
}
impl Component for Recorder {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        self.log.push(message.name.clone());
    }
}

fn log(app: &App, id: GameObjectId) -> Vec<String> {
    app.world.scene.get_component::<Recorder>(id).unwrap().log.clone()
}

/// Каталог ассетов во временной папке.
fn assets(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-mini-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

#[test]
fn the_view_rect_and_the_pointer_follow_the_main_camera() {
    let mut app = app();
    let probe = app.spawn("Probe").id();
    let area = with_ctx(&mut app, probe, |ctx| ctx.view_rect());
    assert!((area.width - HALF_WIDTH * 2.0).abs() < 1e-3 && (area.height - 12.0).abs() < 1e-4);
    assert!(!with_ctx(&mut app, probe, |ctx| ctx.pointer_held()));
    app.world.input.set_touch(1, Vec2::new(0.0, 720.0), TouchPhase::Began);
    let (held, at) = with_ctx(&mut app, probe, |ctx| (ctx.pointer_held(), ctx.pointer_world()));
    assert!(held, "a finger on the screen holds the pointer");
    assert!((at - Vec2::new(-HALF_WIDTH, -6.0)).length() < 1e-3, "{at}");
}

#[test]
fn a_paddle_drives_by_keys_and_by_finger_and_stops_at_the_screen_edge() {
    let mut app = app();
    let paddle = app.spawn("Paddle").with(SpriteRenderer::default()).with(Paddle2D {
        speed: 6.0,
        view_margin: 1.0,
        flip_sprite: true,
        ..Paddle2D::default()
    }).id();
    app.world.input.set_key(Key::D, true);
    app.run_headless(30, DT);
    app.world.input.set_key(Key::D, false);
    assert!((pos(&app, paddle).x - 3.0).abs() < 0.05, "{}", pos(&app, paddle));
    assert_eq!(pos(&app, paddle).y, 0.0, "a paddle never moves up or down");

    // Палец у левого края: едет туда и встаёт в `view_margin` от края; спрайт развёрнут.
    app.world.input.set_touch(1, Vec2::new(0.0, 360.0), TouchPhase::Began);
    app.run_headless(240, DT);
    let x = pos(&app, paddle).x;
    assert!(x <= -HALF_WIDTH + 1.0 && x > -HALF_WIDTH + 0.85, "{x}");
    assert!(app.world.scene.get_component::<SpriteRenderer>(paddle).unwrap().flip_x);
}

#[test]
fn a_paddle_with_a_body_leaves_the_vertical_speed_to_gravity() {
    let mut app = app();
    let paddle = app.spawn("Paddle").at(0.0, 3.0).with(Rigidbody2D::default()).with(Collider2D::default()).with(Paddle2D::default()).id();
    app.world.input.set_key(Key::A, true);
    app.run_headless(30, DT);
    let at = pos(&app, paddle);
    assert!(at.x < -1.0 && at.y < 2.0, "drives left and falls: {at}");
}

#[test]
fn a_spawner_pauses_spawns_across_the_view_and_takes_a_new_interval() {
    let mut app = app();
    let template = app.spawn("Drop").tag("Drop").id();
    let prefab = Prefab::from_object(&app.world.scene, template).unwrap();
    app.world.scene.destroy(template);
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("prefabs/drop.prefab", prefab));
    let spawner = app.spawn("Clouds").at(0.0, 6.5).with(Spawner {
        enabled: false,
        prefab: "prefabs/drop.prefab".into(),
        interval: 0.1,
        across_view: true,
        view_margin: 1.0,
        ..Spawner::default()
    }).id();
    app.run_headless(60, DT);
    assert!(app.world.scene.find_all_by_tag("Drop").is_empty(), "a disabled spawner waits");

    app.world.scene.get_component_mut::<Spawner>(spawner).unwrap().enabled = true;
    app.run_headless(61, DT);
    let drops = app.world.scene.find_all_by_tag("Drop");
    assert!((9..=11).contains(&drops.len()), "{}", drops.len());
    let xs: Vec<f32> = drops.iter().map(|d| pos(&app, *d).x).collect();
    assert!(xs.iter().all(|x| x.abs() <= HALF_WIDTH - 1.0), "inside the view: {xs:?}");
    assert!(xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min) > 4.0, "spread wide: {xs:?}");
    assert!(drops.iter().all(|d| (pos(&app, *d).y - 6.5).abs() < 1e-4), "at the spawner's height");

    // Интервал поменяли на ходу — часы подхватили.
    app.world.scene.get_component_mut::<Spawner>(spawner).unwrap().interval = 0.5;
    let before = app.world.scene.find_all_by_tag("Drop").len();
    app.run_headless(61, DT);
    let added = app.world.scene.find_all_by_tag("Drop").len() - before;
    assert!((1..=3).contains(&added), "{added}");
}

#[test]
fn off_screen_fires_once_after_the_object_was_seen_and_destroys_it() {
    let mut app = app();
    let game = app.spawn("Game").with(Recorder::default()).id();
    // Создан над экраном и падает: сначала въезжает, потом уходит вниз.
    let star = app
        .spawn("Star")
        .at(0.0, 7.0)
        .with(Mover { velocity: Vec3::new(0.0, -6.0, 0.0), ..Mover::default() })
        .with(OffScreen { message: "Missed".into(), notify: "Game".into(), margin: -0.4, ..OffScreen::default() })
        .with(Recorder::default())
        .id();
    app.run_headless(30, DT); // y = 4: на экране
    assert!(log(&app, game).is_empty(), "spawned above the screen is not `left`");
    assert!(app.world.scene.get_component::<OffScreen>(star).unwrap().seen);
    app.run_headless(120, DT); // y = −8: ниже −5.6
    assert_eq!(log(&app, game), ["Missed"]);
    assert!(app.world.scene.get(star).is_none(), "destroyed");

    // Никогда не появлявшийся на экране — молчит.
    let far = app.spawn("Far").at(100.0, 0.0).with(OffScreen { notify: "Game".into(), ..OffScreen::default() }).id();
    app.run_headless(10, DT);
    assert!(app.world.scene.get(far).is_some());
    assert_eq!(log(&app, game).len(), 1);
}

#[test]
fn built_in_sounds_play_by_name_without_registration() {
    let mut app = app();
    let probe = app.spawn("Probe").id();
    let audio = app.world.resources.get::<Audio>().unwrap();
    assert!(audio.has_clip("sfx:coin") && audio.has_clip("music:calm") && !audio.has_clip("sfx:nope"));
    for name in engine::audio::synth::BUILTIN {
        assert!(engine::audio::synth::builtin(name).is_some_and(|c| c.duration() > 0.0), "{name}");
    }
    let coin = with_ctx(&mut app, probe, |ctx| ctx.play("sfx:coin"));
    assert_ne!(coin, VoiceId::NONE);
    let nope = with_ctx(&mut app, probe, |ctx| ctx.play("sfx:nope"));
    assert_eq!(nope, VoiceId::NONE);
    // Музыка: та же — не начинается заново.
    let first = with_ctx(&mut app, probe, |ctx| ctx.music("music:calm"));
    app.run_headless(5, DT);
    let again = with_ctx(&mut app, probe, |ctx| ctx.music("music:calm"));
    assert_ne!(first, VoiceId::NONE);
    assert_eq!(first, again);
}

#[test]
fn reload_scene_restarts_the_scene_loaded_by_engine_load_scene() {
    let dir = assets("reload", &[("scenes/main.scene", "(name: \"Main\", objects: [(name: \"Hero\")])")]);
    let mut app = app();
    engine::mount_assets(&mut app, &dir).unwrap();
    engine::load_scene(&mut app, "scenes/main.scene").unwrap();
    let hero = app.world.scene.find_by_name("Hero").unwrap();
    app.world.scene.get_mut(hero).unwrap().transform.position.x = 5.0;
    app.spawn("Extra");
    with_ctx(&mut app, hero, |ctx| ctx.reload_scene());
    app.run_headless(1, DT);
    let hero = app.world.scene.find_by_name("Hero").expect("the scene is back");
    assert_eq!(pos(&app, hero).x, 0.0);
    assert!(app.world.scene.find_by_name("Extra").is_none());
    let _ = std::fs::remove_dir_all(dir);
}

// ----------------------------------------------------------------------
// Пример «Целая игра: звёзды в корзину» из docs/api/gameplay.md — код дословно.
// ----------------------------------------------------------------------

mod mini {
    pub mod scripts {
        pub fn register(_app: &mut engine::prelude::App) {}
    }
    use engine::prelude::*;

    engine::game! {
        name: "Mini Starfall",
        scene: "scenes/main.scene",
        components: [Score],
    }

    component! {
        /// Счёт: каждая пойманная звезда — очко.
        pub struct Score {
            pub points: i32 = 0,
        }
    }

    impl Component for Score {
        fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
            if message.is("Caught") {
                self.points += 1;
                ctx.ui_text("ScoreText", format!("Score: {}", self.points));
            }
        }
    }
}

const MINI_SCENE: &str = r#"(
    name: "Main",
    objects: [
        (name: "Main Camera", components: [(kind: "Camera", fields: { "ortho_size": Float(6.0) })]),
        (
            name: "Player",
            tag: "Player",
            transform: (position: (0.0, -5.0, 0.0)),
            components: [
                (kind: "SpriteRenderer", fields: { "size": Vec2(1.6, 1.0) }),
                (kind: "Rigidbody2D", fields: { "body_type": Enum("Dynamic"), "lock_rotation": Bool(true) }),
                (kind: "Collider2D", fields: { "size": Vec2(1.5, 0.9) }),
                (kind: "Paddle2D", fields: {}),
            ],
        ),
        (
            name: "Ground",
            transform: (position: (0.0, -6.5, 0.0)),
            components: [(kind: "Collider2D", fields: { "size": Vec2(40.0, 2.0) })],
        ),
        (
            name: "Game",
            transform: (position: (0.0, 6.8, 0.0)),
            components: [
                (kind: "Score", fields: {}),
                (kind: "Spawner", fields: { "prefab": Str("prefabs/star.prefab"), "interval": Float(1.0), "across_view": Bool(true) }),
            ],
        ),
        (
            name: "Canvas",
            components: [(kind: "Canvas", fields: {}), (kind: "RectTransform", fields: {})],
            children: [(name: "ScoreText", components: [(kind: "RectTransform", fields: {}), (kind: "Text", fields: { "text": Str("Score: 0") })])],
        ),
    ],
)"#;

const MINI_STAR: &str = r#"(
    name: "Star",
    tag: "Star",
    components: [
        (kind: "SpriteRenderer", fields: { "size": Vec2(0.9, 0.9) }),
        (kind: "Rigidbody2D", fields: { "body_type": Enum("Dynamic"), "gravity_scale": Float(0.25) }),
        (kind: "Collider2D", fields: { "shape": Enum("Circle"), "radius": Float(0.35), "is_trigger": Bool(true) }),
        (kind: "Collectible", fields: { "message": Str("Caught"), "notify": Str("Game"), "sound": Str("sfx:coin") }),
    ],
)"#;

#[test]
fn the_tutorial_star_game_catches_a_star_and_counts_it() {
    assert_eq!(mini::GAME_NAME, "Mini Starfall");
    let dir = assets("game", &[("scenes/main.scene", MINI_SCENE), ("prefabs/star.prefab", MINI_STAR)]);
    let mut app = app();
    mini::register(&mut app);
    let mut app = engine::build_game(app, SaveData::in_memory(), &dir, "scenes/main.scene").unwrap();
    app.run_headless(2, DT);
    assert!(app.world.scene.find_by_name("Player").is_some());
    let game = app.world.scene.find_by_name("Game").unwrap();
    app.world.scene.get_component_mut::<Spawner>(game).unwrap().enabled = false;
    app.world.instantiate_at("prefabs/star.prefab", Vec3::new(0.0, -2.5, 0.0), Quat::IDENTITY, None).unwrap();
    app.run_headless(90, DT);
    let score = app.world.scene.find_by_name("ScoreText").unwrap();
    assert_eq!(app.world.scene.get_component::<Text>(score).unwrap().text, "Score: 1");
    assert_eq!(app.world.scene.get_component::<mini::Score>(game).unwrap().points, 1);
    let errors = app.world.resources.get::<ScriptErrors>().map(|e| e.messages().to_vec()).unwrap_or_default();
    assert!(errors.is_empty(), "{errors:?}");

    // И звёзды идут сами.
    app.world.scene.get_component_mut::<Spawner>(game).unwrap().enabled = true;
    app.run_headless(130, DT);
    assert!(!app.world.scene.find_all_by_tag("Star").is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
