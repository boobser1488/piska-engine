//! Совместимость со «старым» API скриптов: замороженные копии настоящих скриптов игр
//! (каталог `compat_scripts/`, файлы скопированы как есть 2026-11-15, до появления
//! коротких форм) обязаны компилироваться и вести себя как прежде — навсегда.
//!
//! Эти файлы **не правят**: если тест перестал собираться, сломана обратная
//! совместимость движка, а не тест.
#![allow(dead_code)]

use engine::prelude::*;

/// `games/template/src/lib.rs` целиком («Звездопад»: Basket, Star, GameManager).
#[path = "compat_scripts/template_lib.rs"]
mod template_lib;

/// lantern: слой параллакса и помощники звука.
#[path = "compat_scripts/lantern_parallax.rs"]
mod lantern_parallax;
#[path = "compat_scripts/lantern_sfx.rs"]
mod lantern_sfx;

/// marble: шестерня и «метла».
#[path = "compat_scripts/marble_cog.rs"]
mod marble_cog;
#[path = "compat_scripts/marble_sweeper.rs"]
mod marble_sweeper;

/// swarm: компонент-данные реактора.
#[path = "compat_scripts/swarm_reactor.rs"]
mod swarm_reactor;

/// tashbazar: дрожащий фонарь и подвижная платформа.
#[path = "compat_scripts/tashbazar_lantern_flicker.rs"]
mod tashbazar_lantern_flicker;
#[path = "compat_scripts/tashbazar_moving_platform.rs"]
mod tashbazar_moving_platform;

/// Скрипт из шаблона редактора «New Rust Script» (как он лежит в проекте пользователя).
#[path = "compat_scripts/app_rust.rs"]
mod app_rust;

const DT: f32 = 1.0 / 60.0;

fn app() -> App {
    let mut app = engine::app();
    app.world.resources.insert(SaveData::in_memory());
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app
}

fn angle_y_deg(app: &App, id: GameObjectId) -> f32 {
    app.world.scene.get(id).unwrap().transform.euler_deg().y
}

// ----------------------------------------------------------------------
// Шаблон «Звездопад»
// ----------------------------------------------------------------------

#[test]
fn template_basket_runs_along_the_horizontal_axis() {
    use template_lib::Basket;
    let mut app = app();
    template_lib::register(&mut app);
    let basket = app
        .spawn("Basket")
        .with(Basket::default())
        .with(Rigidbody2D { gravity_scale: 0.0, ..Rigidbody2D::kinematic() })
        .with(SpriteRenderer::default())
        .id();
    app.world.input.set_key(Key::A, true);
    app.tick(DT);
    let body = app.world.scene.get_component::<Rigidbody2D>(basket).unwrap();
    assert_eq!(body.velocity.x, -9.0, "Basket.speed = 9 to the left");
    assert!(app.world.scene.get_component::<SpriteRenderer>(basket).unwrap().flip_x);
}

#[test]
fn template_star_reports_to_the_game_manager_and_the_hud_follows() {
    use template_lib::{GameManager, Star};
    let mut app = app();
    template_lib::register(&mut app);
    app.spawn("Game").with(GameManager::default());
    let basket = app.spawn("Basket").tag("Player").id();
    let star = app.spawn("Star").at(0.0, 3.0).with(Star::default()).id();
    app.tick(DT); // on_start: HUD, рекорд

    // Звезда поймана: триггер с объектом «Player».
    app.world
        .dispatch_hook(star, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, basket));
    app.world.flush_messages();
    app.tick(DT);
    let game = app.world.scene.find_by_name("Game").unwrap();
    let manager = app.world.scene.get_component::<GameManager>(game).unwrap();
    assert_eq!(manager.score, 1);
    assert!(app.world.scene.get(star).is_none(), "the caught star is destroyed");
    let score = app.world.scene.find_by_name("Score").unwrap();
    assert_eq!(app.world.scene.get_component::<Text>(score).unwrap().text, "Score: 1");

    // Звезда упала ниже края — минус жизнь.
    app.spawn("Star").at(0.0, -9.0).with(Star::default());
    app.tick(DT);
    app.tick(DT);
    let manager = app.world.scene.get_component::<GameManager>(game).unwrap();
    assert_eq!(manager.lives, 2);
}

// ----------------------------------------------------------------------
// lantern
// ----------------------------------------------------------------------

#[test]
fn lantern_parallax_follows_the_camera_by_its_factor() {
    use lantern_parallax::Parallax;
    let mut app = app();
    app.spawn("Camera").tag("MainCamera").at(10.0, 15.0);
    let layer = app
        .spawn("Layer")
        .with(Parallax { factor: 0.2, period: 32.0, slot: 1, ..Parallax::default() })
        .id();
    app.tick(DT);
    let p = app.world.scene.get(layer).unwrap().transform.position;
    assert!((p.x - 8.0).abs() < 1e-4, "x = cam.x - cam.x·factor = 8, got {}", p.x);
    assert!((p.y - 15.0).abs() < 1e-4);
}

#[test]
fn lantern_sound_helpers_drive_the_audio_buses() {
    let mut app = app();
    let id = app.spawn("Any").id();
    let audio = app.world.resources.get_mut::<Audio>().expect("engine::app() installs Audio");
    audio.add_clip("blip", synth::sfx::coin());
    lantern_sfx::apply_volumes(audio, 0.25, 0.5);
    assert_eq!(audio.bus_volume(Bus::Music), 0.25);
    assert_eq!(audio.bus_volume(Bus::Sfx), 0.5);
    assert_eq!(audio.bus_volume(Bus::Ui), 0.5);
    let mut ctx = Ctx { world: &mut app.world, id };
    lantern_sfx::play(&mut ctx, "blip");
    lantern_sfx::play_ui(&mut ctx, "blip");
    lantern_sfx::music(&mut ctx, "blip", 0.5);
}

// ----------------------------------------------------------------------
// marble
// ----------------------------------------------------------------------

#[test]
fn marble_cog_turns_about_its_axis() {
    use marble_cog::Cog;
    let mut app = app();
    let cog = app.spawn("Cog").with(Cog { speed_deg: 30.0, axis: 1 }).id();
    app.run_headless(60, DT);
    assert!((angle_y_deg(&app, cog) - 30.0).abs() < 0.05, "{}", angle_y_deg(&app, cog));
}

#[test]
fn marble_sweeper_swings_from_its_start_rotation() {
    use marble_sweeper::Sweeper;
    let mut app = app();
    let sweeper = app
        .spawn("Sweeper")
        .with(Sweeper { speed_deg: 40.0, ..Sweeper::default() })
        .id();
    app.run_headless(30, DT); // старт + 29 кадров update
    let expected = 40.0 * DT * 30.0;
    assert!((angle_y_deg(&app, sweeper) - expected).abs() < 0.1, "{}", angle_y_deg(&app, sweeper));
}

// ----------------------------------------------------------------------
// swarm
// ----------------------------------------------------------------------

#[test]
fn swarm_reactor_is_reflected_data() {
    use swarm_reactor::ReactorCore;
    let mut app = app();
    app.register::<ReactorCore>();
    let mut core = ReactorCore::default();
    assert_eq!(core.pulse_speed, 2.2);
    core.set_field("pulse_speed", &Value::Float(3.0)).unwrap();
    assert_eq!(core.pulse_speed, 3.0);
    assert_eq!(core.hooks(), Hooks::ALL, "no hooks declared: the engine default");
    assert!(app.world.registry.contains("ReactorCore"));
}

// ----------------------------------------------------------------------
// tashbazar
// ----------------------------------------------------------------------

#[test]
fn tashbazar_lantern_flickers_the_light() {
    use tashbazar_lantern_flicker::LanternFlicker;
    let mut app = app();
    let lamp = app
        .spawn("Lamp")
        .with(Light::default())
        .with(LanternFlicker { intensity: 2.0, amount: 0.5, ..LanternFlicker::default() })
        .id();
    let mut seen = Vec::new();
    for _ in 0..20 {
        app.tick(0.05);
        seen.push(app.world.scene.get_component::<Light>(lamp).unwrap().intensity);
    }
    assert!(seen.iter().all(|i| (1.0..=3.0).contains(i)), "{seen:?}");
    assert!(seen.windows(2).any(|w| w[0] != w[1]), "the light changes");
    assert_eq!(LanternFlicker::default().hooks(), Hooks::UPDATE);
}

#[test]
fn tashbazar_platform_travels_with_smooth_turns() {
    use tashbazar_moving_platform::MovingPlatform;
    let mut app = app();
    let platform = app
        .spawn("Platform")
        .at3(1.0, 0.0, 0.0)
        .with(MovingPlatform { travel: Vec3::new(0.0, 0.0, 6.0), period: 2.0, ..MovingPlatform::default() })
        .id();
    // 60 шагов физики по 1/60 = 1 с = половина пути: s = 0.5, сглаженно тоже 0.5.
    app.run_headless(60, DT);
    let p = app.world.scene.get(platform).unwrap().transform.position;
    assert!((p.x - 1.0).abs() < 1e-4);
    assert!((p.z - 3.0).abs() < 0.05, "{p:?}");
}

// ----------------------------------------------------------------------
// Шаблон редактора
// ----------------------------------------------------------------------

#[test]
fn editor_template_script_spins_about_y() {
    use app_rust::Rust;
    let mut app = app();
    let id = app.spawn("Cube").with(Rust { speed: 90.0 }).id();
    app.run_headless(31, DT); // on_start в первом кадре, потом 31 update
    let expected = 90.0 * DT * 31.0;
    assert!((angle_y_deg(&app, id) - expected).abs() < 0.1, "{}", angle_y_deg(&app, id));
}
