//! Путь новичка от начала до конца: проект из мастера хаба → каждая заготовка окна
//! «New Rust Script» (через API редактора, как это делает кнопка) → `cargo test` проекта:
//! код собирается, а тест без окна проверяет видимый результат каждой заготовки
//! (объект поехал, монета подобрана, текст на кнопке сменился…).
//!
//! Долго (собирает движок для отдельного проекта), поэтому `#[ignore]`:
//!
//! ```text
//! cargo test -p piska-hub --test beginner_path -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use engine_editor::script_templates::{create_script, ScriptTemplate};
use piska_hub::paths;
use piska_hub::templates::{self, NewProject, Template};

/// Имена компонентов, под которыми тест создаёт заготовки.
const SCRIPTS: [(&str, ScriptTemplate); 8] = [
    ("Hero", ScriptTemplate::Empty),
    ("Patrol", ScriptTemplate::Mover),
    ("Walker", ScriptTemplate::PlayerTopDown),
    ("Runner", ScriptTemplate::Platformer),
    ("Hiker", ScriptTemplate::Player3D),
    ("Coin", ScriptTemplate::Collectible),
    ("CoinSpawner", ScriptTemplate::Spawner),
    ("ClickCounter", ScriptTemplate::UiButton),
];

/// Тест, который кладётся в созданный проект: по сцене на заготовку.
const PROJECT_TEST: &str = r#"//! Заготовки New Rust Script делают видимое (тест пути новичка).

use engine::prelude::*;
use LIB::scripts::*;

const DT: f32 = 1.0 / 60.0;

component! {
    pub struct Inbox { pub got: Vec<String> = Vec::new() }
}
impl Component for Inbox {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        self.got.push(message.name.clone());
    }
}

fn app() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    LIB::register(&mut app);
    app.world.resources.insert(SaveData::in_memory());
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app
}

fn errors(app: &App) -> Vec<String> {
    app.world.resources.get::<ScriptErrors>().map(|e| e.messages().to_vec()).unwrap_or_default()
}

fn pos(app: &App, id: GameObjectId) -> Vec3 {
    app.world.scene.world_position(id)
}

#[test]
fn empty_template_walks_with_the_arrows() {
    let mut app = app();
    let hero = app.spawn("Hero").with(hero::Hero::default()).id();
    app.world.input.set_key(Key::D, true);
    app.run_headless(61, DT);
    assert!((pos(&app, hero).x - 3.0).abs() < 0.1, "{:?}", pos(&app, hero));
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn mover_template_goes_and_comes_back() {
    let mut app = app();
    let id = app.spawn("Patrol").with(patrol::Patrol::default()).id();
    app.run_headless(46, DT); // половина пути (1.5 с в одну сторону)
    let x = pos(&app, id).x;
    assert!(x > 1.0 && x < 2.0, "{x}");
    app.run_headless(135, DT); // 3 с: снова у старта
    assert!(pos(&app, id).x < 0.2, "{:?}", pos(&app, id));
}

#[test]
fn top_down_template_walks_up() {
    let mut app = app();
    let id = app.spawn("Walker").with(walker::Walker::default()).with(SpriteRenderer::default()).id();
    app.world.input.set_key(Key::W, true);
    app.run_headless(61, DT);
    assert!((pos(&app, id).y - 5.0).abs() < 0.1, "{:?}", pos(&app, id));
}

#[test]
fn platformer_template_runs_and_jumps() {
    let mut app = app();
    app.spawn("Ground").at(0.0, -0.5).with(Collider2D::rect(40.0, 1.0));
    let id = app
        .spawn("Runner")
        .at(0.0, 0.6)
        .with(runner::Runner::default())
        .with(Rigidbody2D::default().locked_rotation())
        .with(Collider2D::rect(0.8, 1.0))
        .id();
    app.run_headless(30, DT);
    let rest = pos(&app, id).y;
    app.world.input.set_key(Key::Space, true);
    app.world.input.set_key(Key::D, true);
    app.run_headless(12, DT);
    assert!(pos(&app, id).y > rest + 0.8, "jumped: {:?}", pos(&app, id));
    assert!(pos(&app, id).x > 0.5, "ran: {:?}", pos(&app, id));
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn player_3d_template_walks_forward() {
    let mut app = app();
    app.spawn("Floor").at3(0.0, -0.5, 0.0).with(Collider::cuboid(20.0, 1.0, 20.0));
    let id = app.spawn("Hiker").at3(0.0, 0.05, 0.0).with(CharacterController::default()).with(hiker::Hiker::default()).id();
    app.run_headless(20, DT);
    app.world.input.set_key(Key::W, true);
    app.run_headless(60, DT);
    assert!(pos(&app, id).z < -3.5, "{:?}", pos(&app, id));
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn collectible_template_is_picked_up_by_the_player() {
    let mut app = app();
    let game = app.spawn("Game").with(Inbox::default()).id();
    let player = app.spawn("Player").tag("Player").id();
    let coin = app.spawn("Coin").with(coin::Coin::default()).id();
    app.tick(DT);
    app.world.dispatch_hook(coin, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, player));
    app.world.flush_messages();
    app.tick(DT);
    assert!(app.world.scene.get(coin).is_none(), "the coin is gone");
    assert_eq!(app.world.scene.get_component::<Inbox>(game).unwrap().got, ["Collected"]);
}

#[test]
fn spawner_template_spawns_its_prefab() {
    let mut app = app();
    let template = app.spawn("Coin").tag("Coin").id();
    let prefab = Prefab::from_object(&app.world.scene, template).unwrap();
    app.world.scene.destroy(template);
    app.world.prefabs = Box::new(engine::core::serialize::MemoryPrefabs::default().with("prefabs/coin.prefab", prefab));
    app.spawn("Spawner").with(coin_spawner::CoinSpawner::default());
    app.run_headless(130, DT); // ~2.2 с: одна монета (каждые 2 с)
    assert_eq!(app.world.scene.find_all_by_tag("Coin").len(), 1);
}

#[test]
fn ui_button_template_counts_clicks_in_the_label() {
    let mut app = app();
    {
        let scene = &mut app.world.scene;
        let canvas = scene.ui_canvas().id();
        scene.ui_text(canvas, "").name("Label");
        scene
            .ui_button(canvas, "press", "Press")
            .anchor(Anchor::Center)
            .size(200.0, 60.0)
            .with(click_counter::ClickCounter::default());
    }
    app.tick(DT);
    let label = app.world.scene.find_by_name("Label").unwrap();
    assert_eq!(app.world.scene.get_component::<Text>(label).unwrap().text, "Click the button!");
    app.world.input.set_mouse_position(Vec2::new(640.0, 360.0));
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.tick(DT);
    app.world.input.set_mouse_button(MouseButton::Left, false);
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<Text>(label).unwrap().text, "Clicked 1 times");
}
"#;

fn cargo(project: &Path, args: &[&str]) -> (bool, String) {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command.args(args).arg("--offline").current_dir(project).env("ENGINE_AUDIO", "off");
    let output = command.output().expect("cargo runs");
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-beginner-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
#[ignore = "builds the engine for a standalone project; run with --ignored"]
fn every_new_script_starting_point_works_in_a_new_project() {
    let root = paths::find_engine_root().unwrap();
    let dir = scratch();
    let plan = templates::plan(
        &NewProject { name: "gen_beginner".into(), location: dir.clone(), template: Template::Empty2D, cpp: false },
        &root,
    )
    .unwrap();
    templates::create(&plan, &root).unwrap();
    let project = plan.dir.clone();

    // Сборка в общем кэше движка: те же версии зависимостей и те же настройки профиля.
    if let Ok(lock) = std::fs::read(root.join("Cargo.lock")) {
        std::fs::write(project.join("Cargo.lock"), lock).unwrap();
    }
    if let Ok(config) = std::fs::read(root.join(".cargo/config.toml")) {
        std::fs::create_dir_all(project.join(".cargo")).unwrap();
        std::fs::write(project.join(".cargo/config.toml"), config).unwrap();
    }

    let scripts = project.join("src/scripts");
    for (name, template) in SCRIPTS {
        create_script(&scripts, name, template).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let module = std::fs::read_to_string(scripts.join("mod.rs")).unwrap();
    for (name, _) in SCRIPTS {
        assert!(module.contains(&format!("::{name}>();")), "mod.rs registers {name}:\n{module}");
    }
    let lib = plan.dir.file_name().unwrap().to_string_lossy().replace('-', "_");
    std::fs::write(project.join("tests/beginner.rs"), PROJECT_TEST.replace("LIB", &lib)).unwrap();

    let (ok, output) = cargo(&project, &["test", "--test", "beginner"]);
    assert!(ok, "the new project with every starting point failed:\n{output}");
    assert!(output.contains("test result: ok. 8 passed"), "{output}");
    std::fs::remove_dir_all(&dir).unwrap();
}
