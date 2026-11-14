//! Проект как набор файлов: данные, префабы, сцена, UI-префабы, звук, настройки проекта.
//! Игра стартует из них одних; код ничего не строит.

use engine::prelude::*;
use swarm::data::*;
use swarm::defs::Defs;
use swarm::scripts::game_manager::Game;
use swarm::scripts::menus::Menus;
use swarm::ui::Screen;

fn assets() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn app() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = swarm::build_with(SaveData::in_memory()).expect("the game builds from its asset files");
    app.run_headless(3, 1.0 / 60.0);
    app
}

fn files(dir: &str, ext: &str) -> Vec<String> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, ext: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, ext, out);
            } else if path.extension().is_some_and(|e| e == ext) {
                out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let base = assets();
    let mut out = Vec::new();
    walk(&base, &base.join(dir), ext, &mut out);
    out.sort();
    out
}

#[test]
fn data_files_describe_every_enemy_upgrade_and_wave() {
    let app = app();
    let db = app.world.resources.get::<AssetDatabase>().unwrap();
    let defs = Defs::load(db).expect("data/*.ron are valid and readable through the asset database");
    assert_eq!(defs.enemies.len(), Kind::ALL.len());
    assert_eq!(defs.upgrades.len(), UP_COUNT);
    assert!(defs.waves.waves.len() >= 20, "the campaign table has 20 waves");
    for (i, wave) in defs.waves.waves.iter().enumerate() {
        assert!(!wave.groups.is_empty() || wave.boss.is_some(), "wave {} is not empty", i + 1);
        assert_eq!(wave.boss.is_some(), (i + 1) % 5 == 0, "bosses come on waves 5, 10, 15, 20");
    }
    for boss in Kind::BOSSES {
        assert!(defs.enemy(boss).script.len() >= 8, "{boss:?} has an attack script");
    }
    // Каждому виду — префаб, и он на месте.
    for def in &defs.enemies {
        assert!(db.guid_of(&def.prefab).is_some(), "prefab of {:?}: {}", def.id, def.prefab);
    }
    for up in &defs.upgrades {
        assert!(db.guid_of(&up.icon).is_some(), "icon of {:?}: {}", up.id, up.icon);
    }
    // Таблица волн совпадает с тем, что играет симуляция.
    swarm::defs::install(defs);
    assert_eq!(plan_wave(7).groups, swarm::defs::defs().waves.waves[6].groups);
    assert!(!plan_wave(25).groups.is_empty(), "endless waves come from the generator");
}

#[test]
fn upgrade_descriptions_are_grammatical_for_every_level() {
    for up in Up::ALL {
        for level in 1..=up.info().max {
            let text = up.describe(level);
            assert!(!text.is_empty() && !text.contains('{') && !text.contains('}'), "{up:?} level {level}: {text}");
        }
    }
    assert_eq!(Up::Multishot.describe(1), "Выстрел делится веером на 2 снаряда");
    assert_eq!(Up::Multishot.describe(3), "Выстрел делится веером на 4 снаряда");
    assert_eq!(Up::Multishot.describe(4), "Выстрел делится веером на 5 снарядов");
    assert_eq!(plural(1, "снаряд", "снаряда", "снарядов"), "снаряд");
    assert_eq!(plural(11, "снаряд", "снаряда", "снарядов"), "снарядов");
    assert_eq!(plural(22, "снаряд", "снаряда", "снарядов"), "снаряда");
}

#[test]
fn the_project_is_made_of_asset_files() {
    let prefabs = files("prefabs", "prefab");
    let ui = files("ui", "prefab");
    let audio = files("audio", "wav");
    let data = files("data", "ron");
    assert!(prefabs.len() >= 30, "{} prefabs: {prefabs:?}", prefabs.len());
    assert_eq!(ui.len(), 7, "{ui:?}");
    assert!(audio.len() >= 33, "{} sounds", audio.len());
    assert_eq!(data.len(), 4, "{data:?}");
    assert!(assets().join("project.settings").is_file());
    for needed in ["prefabs/drone.prefab", "prefabs/reactor_core.prefab", "prefabs/bosses/hive.prefab", "prefabs/enemies/tank.prefab", "prefabs/projectiles/missile.prefab"] {
        assert!(prefabs.iter().any(|p| p == needed), "{needed}");
    }
    // Каждый префаб создаётся, у визуальных есть спрайт.
    let mut app = app();
    for path in prefabs.iter().chain(ui.iter()) {
        let id = app.world.instantiate(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let object = app.world.scene.get(id).unwrap();
        let visual = object.has::<SpriteRenderer>() || object.has::<ParticleSystem>() || object.has::<Canvas>();
        assert!(visual, "{path} has something to draw");
        app.world.scene.destroy(id);
    }
}

#[test]
fn the_scene_holds_the_arena_the_drone_the_reactor_the_manager_and_the_screens() {
    let app = app();
    let scene = &app.world.scene;
    assert!(scene.len() > 300, "the scene (with pools) is populated: {}", scene.len());
    for name in ["Main Camera", "Environment", "Space", "ArenaFloor", "Drone", "ReactorCore", "GameManager", "HUD", "MenuScreen", "ModeScreen", "OptionsScreen", "PauseScreen", "UpgradeScreen", "ResultsScreen"] {
        assert!(scene.find_by_name(name).is_some(), "`{name}` is an object of arena.scene");
    }
    // Дрон, реактор и экраны — экземпляры префабов.
    for (name, prefab) in [("Drone", "drone"), ("ReactorCore", "reactor_core"), ("HUD", "hud"), ("MenuScreen", "main_menu")] {
        let id = scene.find_by_name(name).unwrap();
        let reference = scene.get(id).unwrap().prefab().unwrap_or("");
        assert!(reference.contains(prefab), "{name} is an instance of {prefab}: `{reference}`");
    }
    // Менеджер: по скрипту на поведение.
    let manager = scene.find_by_name("GameManager").unwrap();
    let kinds: Vec<&str> = scene.get(manager).unwrap().components().map(|c| c.type_name()).collect();
    for k in ["GameManager", "Menus", "TouchControls", "Waves", "Upgrades", "Hud"] {
        assert!(kinds.contains(&k), "{k} is on the manager: {kinds:?}");
    }
    let drone = scene.find_by_name("Drone").unwrap();
    assert!(scene.get(drone).unwrap().has::<swarm::scripts::drone::Drone>());
    assert_eq!(scene.get(drone).unwrap().tag, "Player");
}

#[test]
fn the_input_map_comes_from_project_settings() {
    let app = app();
    let input = &app.world.input;
    for axis in ["Horizontal", "Vertical", "AimX", "AimY"] {
        assert!(input.has_axis(axis), "axis {axis}");
    }
    for button in ["Dash", "Fire", "Submit", "Cancel"] {
        assert!(input.has_button(button), "button {button}");
    }
    assert!(!input.has_button("Jump"), "the platformer actions are not in this game");
    let settings = app.world.project_settings();
    assert_eq!(settings.script_order.get("GameManager"), Some(&-10));
}

#[test]
fn the_quit_button_can_be_hidden_as_in_the_web_build() {
    let mut app = app();
    let quit = app.world.resources.get::<Game>().unwrap().ui.quit_btn;
    assert!(app.world.scene.get(quit).unwrap().active, "Quit is shown on desktop");
    assert_eq!(app.world.resources.get::<Game>().unwrap().screen, Screen::Menu);
    let manager = app.world.scene.find_by_name("GameManager").unwrap();
    app.world.scene.get_component_mut::<Menus>(manager).unwrap().show_quit = false;
    app.run_headless(2, 1.0 / 60.0);
    assert!(!app.world.scene.get(quit).unwrap().active, "show_quit = false hides Quit (the web build forces it)");
}
