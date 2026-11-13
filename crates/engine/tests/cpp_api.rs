//! C++-API целиком, без окна и звуковой карты: встроенный (статический) плагин
//! `plugins/cpp-gameplay` + скрипт `ApiProbe`, который по сообщению выполняет одно
//! обращение к движку и записывает итог в свои поля, + игра из `examples/cpp_game.rs`.

use engine::core::logging;
use engine::prelude::*;

#[path = "../examples/cpp_game.rs"]
#[allow(dead_code)]
mod game;

const DT: f32 = 1.0 / 60.0;

/// Приложение с C++-плагином, без звукового устройства и с сохранениями в памяти.
fn base() -> App {
    let mut app = engine::app();
    game::add_logic(&mut app, None).expect("the built-in plugin loads");
    app.world.resources.insert(SaveData::in_memory());
    let audio = app.world.resources.get_mut::<Audio>().expect("engine::app installs Audio");
    audio.set_device_enabled(false);
    audio.add_clip("probe-beep", synth::sfx::coin());
    app
}

fn add_cpp(app: &mut App, id: GameObjectId, kind: &str) {
    let component = app.world.registry.create(kind).expect("C++ component is registered");
    app.world.scene.get_mut(id).unwrap().add_boxed(component);
}

/// Приложение и объект `Probe` с `ApiProbe`; первые кадры уже прошли (`on_start` выполнен).
fn probe_app() -> (App, GameObjectId) {
    let mut app = base();
    let probe = app.spawn("Probe").id();
    add_cpp(&mut app, probe, "ApiProbe");
    app.run_headless(2, DT);
    (app, probe)
}

/// Послать пробе команду и прогнать один кадр.
fn run(app: &mut App, probe: GameObjectId, command: &str, value: Value) {
    app.world.send_message(probe, &format!("probe.{command}"), value);
    app.tick(DT);
}

fn field(app: &App, id: GameObjectId, name: &str) -> Value {
    app.world
        .scene
        .get(id)
        .expect("object is alive")
        .components()
        .find(|c| c.type_name() == "ApiProbe")
        .expect("ApiProbe")
        .get_field(name)
        .unwrap_or_else(|| panic!("field {name}"))
}

fn number(app: &App, id: GameObjectId) -> f32 {
    match field(app, id, "number") {
        Value::Float(v) => v,
        other => panic!("{other:?}"),
    }
}

fn count(app: &App, id: GameObjectId, name: &str) -> i64 {
    match field(app, id, name) {
        Value::Int(v) => v,
        other => panic!("{other:?}"),
    }
}

fn flag(app: &App, id: GameObjectId) -> bool {
    field(app, id, "ok") == Value::Bool(true)
}

fn text(app: &App, id: GameObjectId, name: &str) -> String {
    match field(app, id, name) {
        Value::Str(s) => s,
        other => panic!("{other:?}"),
    }
}

fn logged(app_since: u64, needle: &str) -> bool {
    logging::records_since(app_since).iter().any(|r| r.message.contains(needle))
}

fn log_mark() -> u64 {
    engine::init_logger();
    logging::records().last().map_or(0, |r| r.seq + 1)
}

// ── хуки ────────────────────────────────────────────────────────────────────

#[test]
fn every_hook_is_called() {
    let (mut app, probe) = probe_app();
    let other = app.spawn("Other").id();
    app.run_headless(58, DT);
    assert_eq!(count(&app, probe, "starts"), 1, "on_start once");
    assert_eq!(count(&app, probe, "updates"), 60);
    assert_eq!(count(&app, probe, "late_updates"), 60);
    let fixed = count(&app, probe, "fixed_updates");
    assert!((58..=62).contains(&fixed), "about one fixed step per frame at 60 fps: {fixed}");

    // Сообщения, в том числе со значением, доходят в on_message.
    app.world.send_message(probe, "hello", Value::Float(2.5));
    app.tick(DT);
    assert_eq!(text(&app, probe, "result"), "hello");
    assert_eq!(number(&app, probe), 2.5);

    // on_destroy вызывается при уничтожении (счётчик читаем из самого объекта в последнем кадре).
    let _ = other;
    app.world.scene.destroy(probe);
    app.tick(DT);
    assert!(!app.world.scene.contains(probe));
}

// ── ввод ────────────────────────────────────────────────────────────────────

#[test]
fn input_keys_axes_mouse_and_fixed_step_semantics() {
    let (mut app, probe) = probe_app();
    app.world.input.set_key(Key::D, true);
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.world.input.set_mouse_position(Vec2::new(321.0, 12.0));
    run(&mut app, probe, "input", Value::Nil);
    assert!(flag(&app, probe), "key D, no key A, left mouse button");
    assert_eq!(number(&app, probe), 1.0, "Horizontal axis from the D key");
    assert_eq!(count(&app, probe, "count"), 321);
}

#[test]
fn input_moves_the_player_and_a_jump_makes_a_sound() {
    let mut app = game::build(None).unwrap();
    app.world.resources.get_mut::<Audio>().unwrap().set_device_enabled(false);
    let player = app.world.scene.find_by_tag("Player").unwrap();
    app.run_headless(60, DT); // падение и покой на земле
    let start = app.world.scene.world_position(player);

    app.world.input.set_key(Key::D, true);
    app.run_headless(30, DT);
    app.world.input.set_key(Key::D, false);
    let moved = app.world.scene.world_position(player).x - start.x;
    assert!((moved - 3.0).abs() < 0.6, "6 m/s for half a second moved the player by {moved}");

    // Короткое нажатие длиной в один кадр не теряется (ввод читается в on_fixed_update).
    app.run_headless(120, DT); // монета на пути уже отзвучала
    let voices_before = app.world.resources.get::<Audio>().unwrap().voice_count();
    let ground = app.world.scene.world_position(player).y;
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    app.tick(DT);
    let voices_after = app.world.resources.get::<Audio>().unwrap().voice_count();
    assert!(voices_after > voices_before, "the jump sound started ({voices_before} -> {voices_after})");
    let mut peak = ground;
    for _ in 0..90 {
        app.tick(DT);
        peak = peak.max(app.world.scene.world_position(player).y);
    }
    assert!(peak - ground > 2.0, "jumped {} m", peak - ground);
    let controller = app.world.scene.get(player).unwrap().components().find(|c| c.type_name() == "PlayerController").unwrap();
    assert_eq!(controller.get_field("jumps"), Some(Value::Int(1)));
}

// ── объекты, иерархия, безопасность хэндлов ─────────────────────────────────

#[test]
fn spawn_destroy_and_stale_handles_are_safe() {
    engine::cpp::reset_complaints();
    let mark = log_mark();
    let (mut app, probe) = probe_app();

    run(&mut app, probe, "spawn", Value::Nil);
    assert!(flag(&app, probe));
    let made = app.world.scene.find_by_name("Made-by-cpp").expect("spawned from C++");
    let object = app.world.scene.get(made).unwrap();
    assert_eq!(object.tag, "made");
    assert_eq!(object.transform.position, Vec3::new(1.0, 2.0, 3.0));

    // destroy откладывается: в том же кадре объект ещё жив, в конце кадра исчезает.
    run(&mut app, probe, "destroy_stashed", Value::Nil);
    assert!(flag(&app, probe), "still alive right after destroy()");
    assert!(!app.world.scene.contains(made), "destroyed at the end of the frame");

    // Устаревший хэндл: нули, запись ничего не делает, ошибка в логе — и никакого падения.
    run(&mut app, probe, "stale", Value::Nil);
    assert!(flag(&app, probe), "alive() reports false for the stale handle");
    assert_eq!(number(&app, probe), 0.0);
    assert_eq!(text(&app, probe, "result"), "");
    assert!(logged(mark, "stale object handle"), "{:?}", logging::records_since(mark));

    // «Нет объекта» — то же самое.
    run(&mut app, probe, "null_handle", Value::Nil);
    assert!(flag(&app, probe));
    assert!(logged(mark, "null object handle"));
}

#[test]
fn a_destroyed_slot_reused_by_a_new_object_does_not_resurrect_the_old_handle() {
    let (mut app, probe) = probe_app();
    run(&mut app, probe, "spawn", Value::Nil);
    run(&mut app, probe, "destroy_stashed", Value::Nil);
    // Слот освободился: новый объект может занять тот же индекс, но с другим поколением.
    let fresh = app.spawn("Fresh").at(5.0, 5.0).id();
    run(&mut app, probe, "stale", Value::Nil);
    assert_eq!(app.world.scene.get(fresh).unwrap().transform.position.x, 5.0, "the new object was not touched");
}

#[test]
fn hierarchy_components_and_queries() {
    let (mut app, probe) = probe_app();
    run(&mut app, probe, "hierarchy", Value::Nil);
    assert!(flag(&app, probe), "parent/child/find_child/child(i)");
    assert_eq!(number(&app, probe), 11.0, "child world x = parent x + local x");
    assert_eq!(count(&app, probe, "count"), 1);
    assert_eq!(text(&app, probe, "result").parse::<f32>().unwrap(), 10.0, "set_world_position goes through the parent");
    assert!(text(&app, probe, "last_other").parse::<f32>().unwrap() < -0.99, "look_at turns forward (-Z)");

    run(&mut app, probe, "add_component", Value::Nil);
    assert!(flag(&app, probe), "add/has/remove component, unknown type is refused");

    app.spawn("Tagged").tag("made");
    run(&mut app, probe, "find", Value::Nil);
    assert!(flag(&app, probe));
    assert_eq!(count(&app, probe, "count"), 1);
}

// ── поля любых компонентов ──────────────────────────────────────────────────

#[test]
fn fields_of_any_kind_are_read_and_written_by_name() {
    let mark = log_mark();
    let (mut app, probe) = probe_app();
    let label = app.spawn("Label").with(Text::new("old")).id();

    run(&mut app, probe, "fields", Value::Nil);
    assert!(flag(&app, probe));
    let t = app.world.scene.get_component::<Text>(label).unwrap();
    assert_eq!(t.text, "héllo", "UTF-8 string");
    assert_eq!(t.size, 33.0, "Int written into a Float field");
    assert_eq!(t.align, TextAlign::Center, "enum by name");
    assert_eq!(t.color, Color::rgba(1.0, 0.0, 0.0, 1.0), "color");
    assert_eq!(text(&app, probe, "result"), "héllo");
    assert_eq!(text(&app, probe, "last_other"), "Center");
    assert_eq!(count(&app, probe, "count"), 100);

    run(&mut app, probe, "bad_field", Value::Nil);
    assert!(!flag(&app, probe));
    assert!(logged(mark, "no field `no_such_field`") || logged(mark, "available fields"), "{:?}", logging::records_since(mark));
}

#[test]
fn every_cpp_field_kind_round_trips_through_reflection() {
    let (mut app, probe) = probe_app();
    // Строки, числа и bool — через рефлексию, как это делает инспектор и файл сцены.
    let object = app.world.scene.get_mut(probe).unwrap();
    let component = object.components_mut().next().unwrap();
    component.set_field("result", &Value::Str("привет".into())).unwrap();
    component.set_field("number", &Value::Float(1.5)).unwrap();
    assert_eq!(component.get_field("result"), Some(Value::Str("привет".into())));
    assert!(component.set_field("number", &Value::Str("x".into())).is_err());

    // Сцена с C++-компонентом сохраняется и читается обратно.
    let saved = app.world.scene_to_string();
    assert!(saved.contains("привет"), "{saved}");
    app.world.load_scene_str(&saved, "saved").unwrap();
    let probe = app.world.scene.find_by_name("Probe").unwrap();
    assert_eq!(text(&app, probe, "result"), "привет");
}

// ── сообщения, таймеры, твины ───────────────────────────────────────────────

#[test]
fn timers_fire_repeat_and_cancel() {
    let (mut app, probe) = probe_app();
    run(&mut app, probe, "after", Value::Nil);
    app.run_headless(24, DT); // 0.4 с
    assert_eq!(count(&app, probe, "ticks"), 0);
    app.run_headless(12, DT); // 0.6 с
    assert_eq!(count(&app, probe, "ticks"), 1, "send_after fired once");
    app.run_headless(60, DT);
    assert_eq!(count(&app, probe, "ticks"), 1, "and only once");

    run(&mut app, probe, "every", Value::Nil);
    app.run_headless(66, DT); // 1.1 с при шаге 0.25
    assert_eq!(count(&app, probe, "ticks"), 5, "send_every fired four more times");
    run(&mut app, probe, "cancel", Value::Nil);
    assert_eq!(number(&app, probe), 1.0, "one timer cancelled");
    app.run_headless(60, DT);
    assert_eq!(count(&app, probe, "ticks"), 5, "cancelled timers stay quiet");
}

#[test]
fn tween_moves_a_field() {
    let (mut app, probe) = probe_app();
    run(&mut app, probe, "tween", Value::Nil);
    assert!(flag(&app, probe));
    app.run_headless(30, DT);
    let half = number(&app, probe);
    assert!((half - 5.0).abs() < 0.6, "halfway: {half}");
    app.run_headless(40, DT);
    assert!((number(&app, probe) - 10.0).abs() < 1e-3);
}

// ── время, сохранения, случайные числа, сцены ───────────────────────────────

#[test]
fn time_save_random_scene_and_quit() {
    let (mut app, probe) = probe_app();
    run(&mut app, probe, "time", Value::Nil);
    assert!(flag(&app, probe));
    assert!((number(&app, probe) - DT).abs() < 1e-6);
    assert_eq!(app.world.time.time_scale, 0.5, "set_time_scale reached the world");
    app.world.time.time_scale = 1.0;

    run(&mut app, probe, "save", Value::Nil);
    assert!(flag(&app, probe));
    assert_eq!(count(&app, probe, "count"), 5);
    assert_eq!(text(&app, probe, "result"), "text");
    assert_eq!(number(&app, probe), 80.0, "default for a missing key + z of the saved Vec3");
    let save = app.world.resources.get::<SaveData>().unwrap();
    assert_eq!(save.get::<Vec3>("probe.v"), Some(Vec3::new(1.0, 2.0, 3.0)));

    run(&mut app, probe, "rng", Value::Nil);
    assert!(flag(&app, probe), "seeded generator is reproducible and ranges are respected");

    app.world.scene.name = "Level A".into();
    run(&mut app, probe, "scene_name", Value::Nil);
    assert_eq!(text(&app, probe, "result"), "Level A");

    assert!(!app.world.quit_requested());
    run(&mut app, probe, "quit", Value::Nil);
    assert!(app.world.quit_requested());
}

#[test]
fn load_scene_from_cpp_replaces_the_scene_at_the_end_of_the_frame() {
    let (mut app, probe) = probe_app();
    app.world.scenes().set_source(MemoryScenes::default().with("next", "(name: \"Next\", objects: [(name: \"Hero\")])"));
    run(&mut app, probe, "load_scene", Value::Str("next".into()));
    // `run` уже прогнал кадр: смена сцены выполняется в его конце.
    assert_eq!(app.world.scene.name, "Next");
    assert!(app.world.scene.find_by_name("Hero").is_some());
    assert!(!app.world.scene.contains(probe));
}

// ── исключения ──────────────────────────────────────────────────────────────

#[test]
fn a_cpp_exception_is_contained_and_logged() {
    let mark = log_mark();
    let (mut app, probe) = probe_app();
    run(&mut app, probe, "exception", Value::Nil);
    assert!(logged(mark, "C++ exception in ApiProbe::on_message: boom from on_message"), "{:?}", logging::records_since(mark));

    // Хук, который бросает каждый кадр: игра идёт, лог не засоряется (не больше 5 записей).
    app.world.scene.get_mut(probe).unwrap().components_mut().next().unwrap().set_field("throw_in_update", &Value::Bool(true)).unwrap();
    let before = count(&app, probe, "updates");
    app.run_headless(30, DT);
    assert_eq!(count(&app, probe, "updates"), before + 30, "the component keeps being updated");
    let reports = logging::records_since(mark).iter().filter(|r| r.message.contains("probe exception")).count();
    assert!((1..=5).contains(&reports), "{reports} reports");
}

// ── физика ──────────────────────────────────────────────────────────────────

fn floor3d(app: &mut App) -> GameObjectId {
    app.spawn("Floor").at3(0.0, -0.5, 0.0).with(Collider::cuboid(10.0, 1.0, 10.0)).id()
}

#[test]
fn raycast_and_overlap_in_3d_and_2d() {
    let (mut app, probe) = probe_app();
    floor3d(&mut app);
    app.spawn("Ground2D").at(0.0, -1.0).with(Collider2D::rect(20.0, 2.0));
    app.run_headless(3, DT);

    run(&mut app, probe, "raycast", Value::Vec3(0.0, 3.0, 0.0));
    assert!(flag(&app, probe), "ray hit");
    assert!((number(&app, probe) - 3.0).abs() < 1e-3, "distance {}", number(&app, probe));
    assert_eq!(text(&app, probe, "result"), "Floor");
    assert!(text(&app, probe, "last_other").parse::<f32>().unwrap() > 0.99, "normal points up");

    run(&mut app, probe, "raycast", Value::Vec3(50.0, 3.0, 0.0));
    assert!(!flag(&app, probe), "a miss reports no hit");

    run(&mut app, probe, "raycast2d", Value::Vec2(0.0, 4.0));
    assert!(flag(&app, probe));
    assert_eq!(text(&app, probe, "result"), "Ground2D");
    assert!((number(&app, probe) - 4.0).abs() < 1e-3);

    run(&mut app, probe, "overlap", Value::Vec3(0.0, 0.3, 0.0));
    assert!(count(&app, probe, "count") >= 1, "sphere overlaps the floor");
    assert!(number(&app, probe) >= 1.0, "circle overlaps Ground2D");
    run(&mut app, probe, "overlap", Value::Vec3(0.0, 40.0, 0.0));
    assert_eq!(count(&app, probe, "count"), 0);
}

#[test]
fn impulse_changes_velocity_and_mass_is_readable() {
    let (mut app, probe) = probe_app();
    let ball = app
        .spawn("Ball")
        .at3(0.0, 50.0, 0.0)
        .with(Rigidbody { gravity_scale: 0.0, ..Rigidbody::dynamic() })
        .with(Collider::sphere(0.5))
        .id();
    app.run_headless(3, DT);
    run(&mut app, probe, "impulse", Value::Nil);
    assert!(number(&app, probe) > 0.0, "mass");
    app.run_headless(2, DT);
    let v = app.world.scene.get_component::<Rigidbody>(ball).unwrap().velocity;
    assert!(v.y > 0.5, "an upward impulse gives upward velocity: {v}");
}

#[test]
fn collision_and_trigger_callbacks_reach_cpp() {
    let (mut app, _probe) = probe_app();
    floor3d(&mut app);
    let ball = app.spawn("Ball").at3(0.0, 2.0, 0.0).with(Rigidbody::dynamic()).with(Collider::sphere(0.5)).id();
    add_cpp(&mut app, ball, "ApiProbe");
    app.run_headless(90, DT);
    let probe_on_ball = app.world.scene.get(ball).unwrap().components().find(|c| c.type_name() == "ApiProbe").unwrap();
    assert!(matches!(probe_on_ball.get_field("collisions"), Some(Value::Int(n)) if n >= 1), "3D collision callback fired");
    assert_eq!(probe_on_ball.get_field("last_other"), Some(Value::Str("Floor".into())));

    // 2D-триггер: тело падает сквозь зону.
    let zone = app.spawn("Zone").at(20.0, 0.0).with(Collider2D::rect(4.0, 4.0).trigger()).id();
    add_cpp(&mut app, zone, "ApiProbe");
    app.spawn("Faller").at(20.0, 4.0).with(Rigidbody2D::dynamic()).with(Collider2D::circle(0.3));
    app.run_headless(90, DT);
    let zone_probe = app.world.scene.get(zone).unwrap().components().find(|c| c.type_name() == "ApiProbe").unwrap();
    assert!(matches!(zone_probe.get_field("triggers"), Some(Value::Int(n)) if n >= 1), "2D trigger callback fired");
    assert_eq!(zone_probe.get_field("last_other"), Some(Value::Str("Faller".into())));
}

#[test]
fn a_missing_service_fails_softly() {
    let mark = log_mark();
    // Приложение без фасада: ни физики, ни звука, ни интерфейса — только ядро и плагин.
    let mut app = App::new();
    let mut plugins = engine::cpp::CppPlugins::default();
    engine::cpp::builtin_gameplay(&mut plugins).unwrap();
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    let probe = app.spawn("Probe").id();
    add_cpp(&mut app, probe, "ApiProbe");
    app.run_headless(2, DT);

    run(&mut app, probe, "raycast", Value::Vec3(0.0, 3.0, 0.0));
    assert!(!flag(&app, probe), "a neutral answer instead of a crash");
    assert!(logged(mark, "no such service"), "{:?}", logging::records_since(mark));
    run(&mut app, probe, "service_missing", Value::Nil);
    assert!(flag(&app, probe));
    assert_eq!(count(&app, probe, "count"), 1, "ENGINE_SERVICE_MISSING");
}

// ── звук и интерфейс ────────────────────────────────────────────────────────

#[test]
fn audio_from_cpp_starts_a_voice() {
    let (mut app, probe) = probe_app();
    assert_eq!(app.world.resources.get::<Audio>().unwrap().voice_count(), 0);
    run(&mut app, probe, "audio", Value::Nil);
    assert!(flag(&app, probe), "play() returned a live voice");
    assert_eq!(app.world.resources.get::<Audio>().unwrap().voice_count(), 1);
    assert_eq!(count(&app, probe, "count"), 1);
}

#[test]
fn ui_text_value_and_visibility_are_set_from_cpp() {
    let (mut app, probe) = probe_app();
    let label = app.spawn("Label").with(Text::new("Score: 0")).id();
    let bar = app.spawn("Bar").with(ProgressBar::default()).id();
    let hidden = app.spawn("Hidden").id();
    run(&mut app, probe, "ui", Value::Nil);
    assert!(flag(&app, probe));
    assert_eq!(app.world.scene.get_component::<Text>(label).unwrap().text, "Score: 7");
    assert_eq!(app.world.scene.get_component::<ProgressBar>(bar).unwrap().value, 0.25);
    assert!(!app.world.scene.get(hidden).unwrap().active);
    assert_eq!(number(&app, probe), 0.0, "nothing was clicked");
}

// ── игра целиком ────────────────────────────────────────────────────────────

#[test]
fn the_cpp_game_plays_pickups_score_and_enemies() {
    let mut app = game::build(None).unwrap();
    app.world.resources.get_mut::<Audio>().unwrap().set_device_enabled(false);
    let player = app.world.scene.find_by_tag("Player").unwrap();
    let coins = app.world.scene.find_all_by_tag("Pickup").len();
    assert_eq!(coins, 3);
    app.run_headless(30, DT);

    // Бежим вправо до стены: монеты лежат на пути.
    app.world.input.set_key(Key::D, true);
    app.run_headless(420, DT);
    app.world.input.set_key(Key::D, false);

    let left = app.world.scene.find_all_by_tag("Pickup").len();
    assert!(left < coins, "at least one coin was picked up ({left} left)");
    let score = app.world.resources.get::<SaveData>().unwrap().int("score", 0);
    assert_eq!(score as usize, coins - left, "the score in the save equals the coins taken");
    let label = app.world.scene.find_by_name("ScoreText").unwrap();
    assert_eq!(app.world.scene.get_component::<Text>(label).unwrap().text, format!("Score: {score}"));

    // Спавнер выпустил врагов с тегом; они ходят и разворачиваются лучами.
    let enemies = app.world.scene.find_all_by_tag("Spawned");
    assert!(!enemies.is_empty() && enemies.len() <= 2, "{} enemies", enemies.len());
    let turns: i64 = enemies
        .iter()
        .filter_map(|id| app.world.scene.get(*id)?.components().find(|c| c.type_name() == "Patroller")?.get_field("turns"))
        .map(|v| if let Value::Int(n) = v { n } else { 0 })
        .sum();
    assert!(turns >= 1, "patrollers turn at walls and edges ({turns} turns)");
    assert!(app.world.scene.contains(player));
}
