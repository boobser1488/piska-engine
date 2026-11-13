//! Сценарии в движке: ожидания с фиксированным шагом, сообщения, API объектов,
//! отражение, твины, интерфейс, горячая перезагрузка, RON, пак ассетов, ошибки.

use std::path::{Path, PathBuf};

use engine::prelude::*;
use engine::scenario::{self, ScriptValue};

/// Каталог ассетов теста со сценариями.
fn assets(test: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("scenario_tests").join(test);
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    dir
}

fn app_with(test: &str, files: &[(&str, &str)]) -> (App, PathBuf) {
    std::env::set_var("ENGINE_AUDIO", "off");
    let dir = assets(test, files);
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    (app, dir)
}

fn runner(app: &mut App, name: &str, path: &str) -> GameObjectId {
    app.spawn(name).with(ScenarioRunner::new(path)).id()
}

fn global(app: &App, id: GameObjectId, name: &str) -> ScriptValue {
    scenario::global(&app.world, id, name).unwrap_or(ScriptValue::Nil)
}

fn no_errors(app: &mut App) {
    let errors = scenario::take_errors(&mut app.world);
    assert!(errors.is_empty(), "{}", errors[0]);
}

#[test]
fn wait_timings_follow_fixed_dt() {
    let src = "let log = []\nfor (let i = 0; i < 3; i++) {\n  wait(0.5)\n  log.push(math.round(time() * 10) / 10)\n}\n";
    let (mut app, _) = app_with("timings", &[("scenarios/t.scenario", src)]);
    let id = runner(&mut app, "Timer", "scenarios/t.scenario");
    app.run_headless(16, 0.1);
    no_errors(&mut app);
    // Старт в кадре 1 (t = 0.1), каждое ожидание — 5 кадров.
    assert_eq!(global(&app, id, "log").to_text(), "[0.6, 1.1, 1.6]");
    assert!(!scenario::is_running(&app.world, id), "finished");
}

#[test]
fn messages_from_rust_reach_handlers_and_emit_reaches_rust() {
    engine_core::component! {
        pub struct Listener { pub got: String = String::new() }
    }
    impl Component for Listener {
        fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
            if message.name == "door_opened" {
                self.got = format!("{:?}", message.value);
            }
        }
    }
    let src = r#"
        let hits = 0
        on("hit", (dmg) => { hits += dmg })
        let key = wait_message("key")
        emit("door_opened", key .. "!")
    "#;
    let (mut app, _) = app_with("messages", &[("scenarios/door.scenario", src)]);
    app.world.registry.register::<Listener>();
    let parent = app.spawn("House").with(Listener::default()).id();
    let door = runner(&mut app, "Door", "scenarios/door.scenario");
    app.world.scene.set_parent(door, Some(parent)).unwrap();
    app.run_headless(2, 0.1);
    app.world.send_message(door, "hit", Value::Int(3));
    app.world.send_message(door, "hit", Value::Float(4.5));
    app.run_headless(3, 0.1);
    assert_eq!(global(&app, door, "hits"), ScriptValue::Num(7.5));
    app.world.send_message(door, "key", Value::Str("gold".into()));
    app.run_headless(4, 0.1);
    no_errors(&mut app);
    assert_eq!(app.world.scene.get_component::<Listener>(parent).unwrap().got, "Str(\"gold!\")");
}

#[test]
fn object_api() {
    let src = r#"
        const box = find("Box")
        box.position = vec3(1, 2, 3)
        box.position.x += 10
        box.scale = 2
        box.rotation = vec3(0, 90, 0)
        box.name = "Crate"
        const kids = box.children().map((c) => c.name)
        const copy = spawn("Crate", 5, 0)
        copy.tag = "Copy"
        const tagged = find_tag("Copy")
        const all = find_all_tag("Enemy").length
        find("Ghost").active = false
        destroy(find("Trash"))
        const missing = find("Nobody")
        const me = self.name
        const parent_name = find("Child").parent.name
    "#;
    let (mut app, _) = app_with("objects", &[("scenarios/o.scenario", src)]);
    let b = app.spawn("Box").id();
    let c = app.spawn("Child").id();
    app.world.scene.set_parent(c, Some(b)).unwrap();
    app.spawn("Ghost");
    app.spawn("Trash");
    app.spawn("E1").tag("Enemy");
    app.spawn("E2").tag("Enemy");
    let id = runner(&mut app, "Director", "scenarios/o.scenario");
    app.run_headless(2, 0.1);
    no_errors(&mut app);
    let o = app.world.scene.get(b).unwrap();
    assert_eq!(o.name, "Crate");
    assert_eq!(o.transform.position, Vec3::new(11.0, 2.0, 3.0));
    assert_eq!(o.transform.scale, Vec3::splat(2.0));
    assert!((o.transform.euler_deg().y - 90.0).abs() < 1e-3);
    assert_eq!(global(&app, id, "kids").to_text(), "[\"Child\"]");
    assert!(global(&app, id, "tagged").to_text().starts_with("<object"));
    assert_eq!(global(&app, id, "all"), ScriptValue::Num(2.0));
    assert_eq!(global(&app, id, "missing"), ScriptValue::Nil);
    assert_eq!(global(&app, id, "me"), ScriptValue::str("Director"));
    assert_eq!(global(&app, id, "parent_name"), ScriptValue::str("Crate"));
    let copy = app.world.scene.find_by_tag("Copy").unwrap();
    assert_eq!(app.world.scene.get(copy).unwrap().transform.position, Vec3::new(5.0, 0.0, 0.0));
    assert!(!app.world.scene.get(app.world.scene.find_by_name("Ghost").unwrap()).unwrap().active);
    assert!(app.world.scene.find_by_name("Trash").is_none());
}

#[test]
fn reflection_get_and_set() {
    engine_core::component! {
        pub struct Health { pub hp: i32 = 10, pub armor: f32 = 0.5, pub label: String = String::new() }
    }
    impl Component for Health {}
    let src = r##"
        const h = find("Hero")
        const before = h.get("Health.hp")
        h.set("Health.hp", before + 5.4)
        h.set("Health.label", 42)
        h.set("Light.color", color("#ff0000"))
        const armor = h.get("Health.armor")
        const has = [h.has("Health"), h.has("Rigidbody")]
        let err = nil
        try { h.get("Health.mana") } catch (e) { err = e }
    "##;
    let (mut app, _) = app_with("reflection", &[("scenarios/r.scenario", src)]);
    app.world.registry.register::<Health>();
    let hero = app.spawn("Hero").with(Health::default()).with(Light::default()).id();
    let id = runner(&mut app, "R", "scenarios/r.scenario");
    app.run_headless(2, 0.1);
    no_errors(&mut app);
    let h = app.world.scene.get_component::<Health>(hero).unwrap();
    assert_eq!(h.hp, 15);
    assert_eq!(h.label, "42");
    assert_eq!(app.world.scene.get_component::<Light>(hero).unwrap().color, Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
    assert_eq!(global(&app, id, "armor"), ScriptValue::Num(0.5));
    assert_eq!(global(&app, id, "has").to_text(), "[true, false]");
    assert!(global(&app, id, "err").to_text().contains("has no field `mana`"), "{:?}", global(&app, id, "err"));
}

#[test]
fn move_to_suspends_until_the_tween_ends_and_tween_animates_fields() {
    let src = r#"
        const box = find("Box")
        box.tween("Light.intensity", 10, 0.5)
        box.move_to(4, 0, 1.0, "linear")
        const arrived = time()
    "#;
    let (mut app, _) = app_with("tweens", &[("scenarios/m.scenario", src)]);
    let b = app.spawn("Box").with(Light { intensity: 0.0, ..Light::default() }).id();
    let id = runner(&mut app, "Mover", "scenarios/m.scenario");
    app.run_headless(6, 0.1);
    let x = app.world.scene.get(b).unwrap().transform.position.x;
    assert!(x > 1.0 && x < 3.5, "half way: {x}");
    assert_eq!(global(&app, id, "arrived"), ScriptValue::Nil, "still waiting");
    app.run_headless(10, 0.1);
    no_errors(&mut app);
    assert!((app.world.scene.get(b).unwrap().transform.position.x - 4.0).abs() < 1e-4);
    assert!((app.world.scene.get_component::<Light>(b).unwrap().intensity - 10.0).abs() < 1e-4);
    let arrived = global(&app, id, "arrived").as_num().unwrap();
    assert!((1.0..=1.4).contains(&arrived), "resumed right after the tween: {arrived}");
}

#[test]
fn ui_functions() {
    let src = r#"
        ui.text("Hint", "Лови!")
        const t = ui.text("Hint")
        ui.hide("Panel")
        ui.value("Volume", 0.25)
        const v = ui.value("Volume")
    "#;
    let (mut app, _) = app_with("ui", &[("scenarios/u.scenario", src)]);
    let canvas = app.world.scene.ui_canvas().name("HUD").id();
    let hint = app.world.scene.ui_text(canvas, "…").name("Hint").id();
    let panel = app.world.scene.ui_panel(canvas).name("Panel").id();
    let volume = app.world.scene.ui_slider(canvas, "volume", 0.0, 1.0, 0.5).name("Volume").id();
    let id = runner(&mut app, "UI", "scenarios/u.scenario");
    app.run_headless(2, 0.1);
    no_errors(&mut app);
    assert_eq!(app.world.scene.get_component::<Text>(hint).unwrap().text, "Лови!");
    assert_eq!(global(&app, id, "t"), ScriptValue::str("Лови!"));
    assert!(!app.world.scene.get(panel).unwrap().active);
    assert_eq!(app.world.scene.get_component::<Slider>(volume).unwrap().value, 0.25);
    assert_eq!(global(&app, id, "v"), ScriptValue::Num(0.25));
}

#[test]
fn hot_reload_restarts_running_runners() {
    let (mut app, dir) = app_with(
        "hot_reload",
        &[("scenarios/h.scenario", "let version = 1\nwhile (true) { wait_frame() }\n")],
    );
    let id = runner(&mut app, "Hot", "scenarios/h.scenario");
    app.run_headless(3, 0.1);
    assert_eq!(global(&app, id, "version"), ScriptValue::Num(1.0));
    std::fs::write(dir.join("scenarios/h.scenario"), "let version = 2\nwhile (true) { wait_frame() }\n").unwrap();
    app.run_headless(5, 0.1);
    assert_eq!(global(&app, id, "version"), ScriptValue::Num(2.0), "restarted with the new code");
    // Синтаксическая ошибка: старая версия продолжает работать, ошибка в логе.
    std::fs::write(dir.join("scenarios/h.scenario"), "let version = \nwhile (true) { wait_frame() }\n").unwrap();
    app.run_headless(5, 0.1);
    let errors = scenario::take_errors(&mut app.world);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].location(), "scenarios/h.scenario:2:1");
    assert_eq!(global(&app, id, "version"), ScriptValue::Num(2.0));
    std::fs::write(dir.join("scenarios/h.scenario"), "let version = 3\nwhile (true) { wait_frame() }\n").unwrap();
    app.run_headless(5, 0.1);
    assert_eq!(global(&app, id, "version"), ScriptValue::Num(3.0));
    no_errors(&mut app);
}

#[test]
fn a_saved_syntax_error_is_reported_while_the_game_is_stopped() {
    let (mut app, dir) = app_with("idle_check", &[("scenarios/ok.scenario", "print('hi')\n")]);
    for _ in 0..40 {
        app.idle();
    }
    no_errors(&mut app);
    std::fs::write(dir.join("scenarios/ok.scenario"), "if x then\n").unwrap();
    for _ in 0..40 {
        app.idle();
    }
    let errors = scenario::take_errors(&mut app.world);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("expected `end`"), "{}", errors[0]);
    // Повторно та же ошибка не показывается.
    for _ in 0..40 {
        app.idle();
    }
    no_errors(&mut app);
}

#[test]
fn a_scene_with_a_runner_round_trips_through_ron() {
    let (mut app, _) = app_with("ron", &[("scenarios/r.scenario", "let x = 1\n")]);
    let guid = app.world.resources.get::<AssetDatabase>().unwrap().guid_of("scenarios/r.scenario").unwrap();
    let r = ScenarioRunner { script: AssetHandle::from_guid(guid), auto_start: false, ..Default::default() };
    app.spawn("Director").with(r);
    let text = app.world.scene_to_string();
    assert!(text.contains("ScenarioRunner"), "{text}");
    assert!(text.contains(&guid.to_string()));
    let mut other = engine::app();
    other.world.load_scene_str(&text, "test").unwrap();
    let id = other.world.scene.find_by_name("Director").unwrap();
    let back = other.world.scene.get_component::<ScenarioRunner>(id).unwrap();
    assert_eq!(back.script.guid(), guid);
    assert!(!back.auto_start);
    assert!(back.hot_reload);
}

#[test]
fn auto_start_off_waits_for_start_from_rust() {
    let (mut app, _) = app_with("manual", &[("scenarios/m.scenario", "let started = true\n")]);
    let mut r = ScenarioRunner::new("scenarios/m.scenario");
    r.auto_start = false;
    let id = app.spawn("M").with(r).id();
    app.run_headless(3, 0.1);
    assert_eq!(global(&app, id, "started"), ScriptValue::Nil);
    scenario::start(&mut app.world, id);
    app.run_headless(1, 0.1);
    assert_eq!(global(&app, id, "started"), ScriptValue::Bool(true));
}

#[test]
fn runs_from_an_asset_pack() {
    let dir = assets("pack", &[("scenarios/p.scenario", "let n = 0\nfor i = 1, 4 do n += i end\n")]);
    // Каталог один раз открывается, чтобы появились .meta (как при экспорте).
    AssetDatabase::open(&dir).unwrap();
    let mut builder = engine::assets::AssetPack::builder();
    builder.add_dir(&dir).unwrap();
    let pack = engine::assets::AssetPack::from_bytes(builder.to_bytes()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let mut app = engine::app();
    app.world.resources.insert(AssetDatabase::from_pack(pack).unwrap());
    let id = runner(&mut app, "P", "scenarios/p.scenario");
    app.run_headless(2, 0.1);
    no_errors(&mut app);
    assert_eq!(global(&app, id, "n"), ScriptValue::Num(10.0));
}

#[test]
fn a_script_error_does_not_stop_the_game() {
    let (mut app, _) = app_with(
        "errors",
        &[
            ("scenarios/bad.scenario", "wait(0.2)\nlet p = find('Nobody')\np.position = vec3()\n"),
            ("scenarios/good.scenario", "let ticks = 0\nwhile (true) { ticks += 1\nwait_frame() }\n"),
            ("scenarios/loop.scenario", "let n = 0\nwhile (true) { n += 1 }\n"),
        ],
    );
    scenario::limits(&mut app.world).instructions_per_resume = 20_000;
    scenario::limits(&mut app.world).max_busy_frames = 3;
    runner(&mut app, "Bad", "scenarios/bad.scenario");
    let good = runner(&mut app, "Good", "scenarios/good.scenario");
    runner(&mut app, "Loop", "scenarios/loop.scenario");
    app.run_headless(10, 0.1);
    let errors = scenario::take_errors(&mut app.world);
    let places: Vec<String> = errors.iter().map(|e| e.location()).collect();
    assert_eq!(places.len(), 2, "{errors:?}");
    assert!(places.contains(&"scenarios/bad.scenario:3:3".to_string()), "{places:?}");
    assert!(errors.iter().any(|e| e.message.contains("frames without waiting")));
    assert!(errors.iter().any(|e| e.message.contains("set `position` on nil")), "{errors:?}");
    assert_eq!(global(&app, good, "ticks"), ScriptValue::Num(10.0));
}

#[test]
fn rust_functions_registered_for_scripts() {
    let (mut app, _) = app_with("register", &[("scenarios/c.scenario", "give_coins(5)\nconst total = give_coins(2)\nlet err\ntry { give_coins('x') } catch (e) { err = e }\n")]);
    scenario::register(&mut app, "give_coins", |ctx, args| {
        let n = args.int(0)?;
        let name = ctx.world.scene.get(ctx.object).map(|o| o.name.clone()).unwrap_or_default();
        let coins = ctx.world.resources.get_or_insert_with(|| 0i64);
        *coins += n;
        assert_eq!(name, "Shop");
        Ok(ScriptValue::Num(*coins as f64))
    });
    let id = runner(&mut app, "Shop", "scenarios/c.scenario");
    app.run_headless(2, 0.1);
    no_errors(&mut app);
    assert_eq!(global(&app, id, "total"), ScriptValue::Num(7.0));
    assert_eq!(global(&app, id, "err"), ScriptValue::str("bad argument #1 to `give_coins`: expected a number, got the string \"x\""));
    assert_eq!(*app.world.resources.get::<i64>().unwrap(), 7);
}

#[test]
fn unknown_names_are_reported_with_a_suggestion_before_running() {
    let (mut app, _) = app_with("unknown", &[("scenarios/u.scenario", "wiat(1)\n")]);
    runner(&mut app, "U", "scenarios/u.scenario");
    app.run_headless(2, 0.1);
    let errors = scenario::take_errors(&mut app.world);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location(), "scenarios/u.scenario:1:1");
    assert_eq!(errors[0].hint.as_deref(), Some("did you mean `wait`?"));
}

#[test]
fn key_and_ease_names() {
    assert_eq!(scenario::key_by_name("space"), Some(Key::Space));
    assert_eq!(scenario::key_by_name("A"), Some(Key::A));
    assert_eq!(scenario::key_by_name("1"), Some(Key::Digit1));
    assert_eq!(scenario::key_by_name("arrow_left"), Some(Key::Left));
    assert_eq!(scenario::key_by_name("nope"), None);
    assert_eq!(scenario::ease_by_name("out_quad"), Some(Ease::OutQuad));
    assert_eq!(scenario::ease_by_name("InOutSine"), Some(Ease::InOutSine));
}

const ROOT_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

#[test]
fn every_scenario_in_the_repository_assets_compiles_and_links() {
    let mut app = engine::app();
    engine::mount_assets(&mut app, ROOT_ASSETS).unwrap();
    let paths: Vec<String> = app.world.resources.get::<AssetDatabase>().unwrap().assets()
        .filter(|(_, p)| p.ends_with(".scenario")).map(|(_, p)| p.to_string()).collect();
    assert!(paths.len() >= 2, "{paths:?}");
    for p in paths {
        scenario::check(&mut app.world, &p).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn the_demo3d_director_pulses_the_lamp_and_patrols() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    engine::mount_assets(&mut app, ROOT_ASSETS).unwrap();
    let lamp = app.spawn("Lamp").with(Light::default()).with(MeshRenderer::sphere()).id();
    let ball = app.spawn("Steel Sphere").at3(1.6, 0.45, 2.8).id();
    app.spawn("Director").with(ScenarioRunner::new("scenarios/demo3d.scenario"));
    let start_color = app.world.scene.get_component::<Light>(lamp).unwrap().color;
    // Первая точка — та, где шар стоит; вторую он проходит за 0.9 с после паузы 0.2 с.
    app.run_headless(125, 1.0 / 60.0);
    no_errors(&mut app);
    let p = app.world.scene.get(ball).unwrap().transform.position;
    assert!((p - Vec3::new(-0.4, 0.45, 3.6)).length() < 0.5, "patrolling: {p}");
    assert_ne!(app.world.scene.get_component::<Light>(lamp).unwrap().color, start_color);
}
