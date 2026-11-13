//! «Хранитель фонаря» — 2D-платформер на Piska Engine.
//!
//! Маленький дух-фонарь спускается по шести тёмным пещерам. В каждой — парящие светлячки:
//! собрав достаточно, вы зажигаете выход. Есть шипы, пропасти, хрупкие и движущиеся
//! платформы, пружины, контрольные точки, четыре вида врагов и босс. Двойной прыжок
//! открывается в конце 2-го уровня, рывок — в конце 4-го.
//!
//! Устройство: карты уровней — `assets/scenes/levelN.scene` (тайлмапы и список объектов,
//! пишет `tools/gen_levels.py`), поведение — компоненты ниже, физика — `phys` (кинематика по
//! сетке тайлов). `build()` собирает приложение; `engine::game_main!` делает из него
//! программу для десктопа, браузера и Android (`docs/api/export.md`).
#![forbid(unsafe_code)]

pub mod boss;
pub mod common;
pub mod controls;
pub mod data;
pub mod enemies;
pub mod level;
pub mod menu;
pub mod objects;
pub mod phys;
pub mod player;
pub mod scenery;
pub mod sfx;
pub mod spawn;
pub mod ui;

/// Скрипты на Rust, созданные в редакторе (New Rust Script).
pub mod scripts;

use engine::prelude::*;

pub use data::{Progress, RunResult, Session, LEVELS, LEVEL_COUNT};
pub use level::{LevelManager, LevelState};
pub use player::Player;

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Lantern Keeper";

/// Зарегистрировать компоненты игры (сцены ссылаются на них по имени).
pub fn register(app: &mut App) {
    scripts::register(app);
    app.register::<Player>()
        .register::<LevelManager>()
        .register::<menu::MenuController>()
        .register::<scenery::Parallax>()
        .register::<objects::Firefly>()
        .register::<objects::Checkpoint>()
        .register::<objects::Spring>()
        .register::<objects::MovingPlatform>()
        .register::<objects::CrumblePlatform>()
        .register::<objects::Gate>()
        .register::<objects::Exit>()
        .register::<objects::Hint>()
        .register::<objects::Projectile>()
        .register::<objects::Rock>()
        .register::<enemies::Crawler>()
        .register::<enemies::Brute>()
        .register::<enemies::Bat>()
        .register::<enemies::Spitter>()
        .register::<boss::Boss>();
}

/// Собрать игру с заданным хранилищем сохранений (тесты передают `SaveData::in_memory()`).
pub fn build_with(save: SaveData) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    controls::setup(&mut app.world.input);

    let progress = Progress::load(&save);
    app.world.resources.insert(save);
    engine::mount_assets(&mut app, engine::assets_dir!())?;

    let database = app.world.resources.get::<AssetDatabase>().expect("mount_assets inserted the database").clone();
    app.world.resources.insert(common::Gfx::load(&database)?);

    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        sfx::install(audio);
        sfx::apply_volumes(audio, progress.music, progress.sfx);
    }
    app.world.resources.insert(progress);
    app.world.resources.insert(Session::default());

    engine::load_scene(&mut app, "scenes/menu.scene")?;
    Ok(app)
}

/// Собрать игру: сохранения — там, где они живут на этой платформе.
pub fn build() -> Result<App, EngineError> {
    build_with(engine::save_data(GAME_NAME))
}

/// Перейти на уровень `n` (с нуля) прямо сейчас: для тестов и снимков экрана.
pub fn goto_level(app: &mut App, n: usize) {
    app.world.scenes().load(LEVELS[n.min(LEVEL_COUNT - 1)].scene);
}

engine::game_main!(build, WindowConfig::titled("Хранитель фонаря").with_size(1280, 720));
