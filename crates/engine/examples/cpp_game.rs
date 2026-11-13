//! Маленькая игра, вся логика которой написана на C++: Rust только собирает сцену.
//!
//! Игрок (`PlayerController`) бегает и прыгает, собирает монеты (`Pickup`: триггер + звук + счёт
//! на экране), а `Spawner` выпускает врагов (`Patroller`), которые лучами ищут стены и обрывы.
//! Скрипты лежат в `plugins/cpp-gameplay/scripts/`.
//!
//! Запуск: `cargo run --example cpp_game` — A/D или стрелки, прыжок — пробел.
//! * По умолчанию играет встроенный (статический) плагин — CMake не нужен.
//! * Если рядом лежит собранная библиотека (`plugins/cpp-gameplay/bin/piska_gameplay.dll`, см.
//!   `docs/api/cpp.md`) или путь к ней передан первым аргументом, загружается она — и правки
//!   скриптов подхватываются на лету после `cmake --build`.
//! * `ENGINE_FRAMES=N` закрывает игру после N кадров, `ENGINE_SCREENSHOT=out.png` рисует кадр.

use std::path::PathBuf;

use engine::cpp::{builtin_gameplay, plugin_file_name, CppPlugins};
use engine::prelude::*;

/// Динамический плагин, если он собран (первый аргумент командной строки или `bin/` плагина).
#[allow(dead_code)]
fn dll_path() -> Option<PathBuf> {
    let argument = std::env::args().nth(1).map(PathBuf::from);
    let default = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/cpp-gameplay/bin"))
        .join(plugin_file_name("piska_gameplay"));
    argument.or(Some(default)).filter(|path| path.is_file())
}

/// Подключить игровую логику: библиотека с горячей перезагрузкой или встроенный модуль.
pub fn add_logic(app: &mut App, dll: Option<PathBuf>) -> Result<(), EngineError> {
    match dll {
        Some(path) => {
            log::info!("C++ gameplay: hot-reloadable plugin {}", path.display());
            engine::add_cpp_plugins(app, [path])
        }
        None => {
            log::info!("C++ gameplay: built-in static plugin");
            let mut plugins = CppPlugins::default();
            builtin_gameplay(&mut plugins)?;
            plugins.register_components(&mut app.world.registry);
            app.add_subsystem(plugins);
            Ok(())
        }
    }
}

fn sprite(color: Color, width: f32, height: f32) -> SpriteRenderer {
    let mut sprite = SpriteRenderer::colored(color);
    sprite.size = Vec2::new(width, height);
    sprite
}

fn add_cpp(app: &mut App, id: GameObjectId, kind: &str, fields: &[(&str, Value)]) {
    let mut component = app
        .world
        .registry
        .create(kind)
        .unwrap_or_else(|e| panic!("C++ component `{kind}` is not available: {e}"));
    for (name, value) in fields {
        component.set_field(name, value).unwrap_or_else(|e| panic!("{e}"));
    }
    app.world.scene.get_mut(id).expect("object exists").add_boxed(component);
}

/// Собрать сцену. Всё поведение — в C++-компонентах: `PlayerController`, `Pickup`, `Spawner`, `Patroller`.
pub fn build_scene(app: &mut App) {
    // Звуки: имена, которые используют скрипты.
    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        audio.add_clip("jump", synth::sfx::jump());
        audio.add_clip("coin", synth::sfx::coin());
    }

    let camera = Camera { ortho_size: 6.0, clear_color: Color::hex(0x5ab0d8), ..Default::default() };
    app.spawn("Camera").at(6.0, 1.0).with(camera).with(AudioListener::default());

    // Земля и стена, у которой враги разворачиваются.
    app.spawn("Ground").at(6.0, -3.0).with(sprite(Color::hex(0x2f7d4f), 30.0, 1.0)).with(Collider2D::rect(30.0, 1.0));
    app.spawn("Wall").at(11.0, -1.75).with(sprite(Color::hex(0x6b4f3a), 1.0, 2.0)).with(Collider2D::rect(1.0, 2.0));

    // Игрок.
    let player = app
        .spawn("Player")
        .tag("Player")
        .at(-4.0, -2.0)
        .with(sprite(Color::hex(0xff7a59), 0.8, 0.9))
        .with(Rigidbody2D::dynamic().locked_rotation())
        .with(Collider2D::rect(0.8, 0.9))
        .id();
    add_cpp(app, player, "PlayerController", &[]);

    // Монеты: триггеры с C++-компонентом Pickup.
    for x in [-1.0, 2.5, 7.0] {
        let coin = app
            .spawn("Coin")
            .tag("Pickup")
            .at(x, -1.6)
            .with(sprite(Color::hex(0xffd23f), 0.5, 0.5))
            .with(Collider2D::circle(0.3).trigger())
            .id();
        add_cpp(app, coin, "Pickup", &[]);
    }

    // Префаб врага: шаблон на сцене → префаб в памяти → шаблон убран.
    let template = app
        .spawn("Enemy")
        .at(0.0, 5.0)
        .with(sprite(Color::hex(0x7a3fd1), 0.8, 0.8))
        .with(Rigidbody2D::dynamic().locked_rotation())
        .with(Collider2D::rect(0.8, 0.8))
        .id();
    add_cpp(app, template, "Patroller", &[("speed", Value::Float(2.5))]);
    let prefab = Prefab::from_object(&app.world.scene, template).expect("template exists");
    app.world.scene.destroy(template);
    app.world.flush_destroyed();
    app.world.prefabs = Box::new(engine::core::serialize::MemoryPrefabs::default().with("enemy", prefab));

    // Спавнер врагов справа от стены.
    let spawner = app.spawn("Spawner").at(15.0, -2.0).id();
    add_cpp(
        app,
        spawner,
        "Spawner",
        &[
            ("prefab", Value::Str("enemy".into())),
            ("interval", Value::Float(2.0)),
            ("max_alive", Value::Int(2)),
            ("spread", Value::Float(1.5)),
        ],
    );

    // Интерфейс: счёт обновляет C++ (`Pickup`).
    let scene = &mut app.world.scene;
    let canvas = scene.ui_canvas().no_navigation().id();
    scene
        .ui_text(canvas, "Score: 0")
        .name("ScoreText")
        .anchor(Anchor::Top)
        .pos(0.0, 20.0)
        .size(400.0, 48.0)
        .font_size(32.0)
        .bold()
        .align(TextAlign::Center)
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.8));
}

/// Игра целиком: движок, C++-логика, сцена.
pub fn build(dll: Option<PathBuf>) -> Result<App, EngineError> {
    let mut app = engine::app();
    add_logic(&mut app, dll)?;
    app.world.resources.insert(SaveData::in_memory());
    build_scene(&mut app);
    Ok(app)
}

#[allow(dead_code)]
fn main() -> Result<(), EngineError> {
    let mut app = build(dll_path())?;
    // Рекорд и счёт переживают запуск, но новая игра начинается с нуля.
    app.world.resources.insert(SaveData::open("PiskaCppGame"));
    if let Some(save) = app.world.resources.get_mut::<SaveData>() {
        save.set_int("score", 0);
    }
    engine::run(app, WindowConfig::titled("C++ game"))
}
