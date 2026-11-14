//! «Хранитель фонаря» — 2D-платформер на Piska Engine.
//!
//! Маленький дух-фонарь спускается по шести тёмным пещерам. В каждой — парящие светлячки:
//! собрав достаточно, вы зажигаете выход. Есть шипы, пропасти, хрупкие и движущиеся
//! платформы, пружины, контрольные точки, четыре вида врагов и босс. Двойной прыжок
//! открывается в конце 2-го уровня, рывок — в конце 4-го.
//!
//! Устройство: игра — это проект. Всё содержимое лежит в `assets/`: сцены уровней и меню,
//! префабы персонажей и предметов, интерфейс (`ui/`), звук (`audio/`), таблицы (`data/`),
//! сценарии (`scenarios/`), карта ввода (`project.settings`). Поведение — компоненты в
//! `src/scripts/` (по одному на файл); общие помощники — `phys`, `combat`, `common`.
//! `build()` только подключает ассеты, регистрирует скрипты и грузит первую сцену.
//! Как собрана игра и как её править — `README.md`.
#![forbid(unsafe_code)]

pub mod audio_bank;
pub mod combat;
pub mod common;
pub mod controls;
pub mod data;
pub mod phys;
pub mod scripts;
pub mod sfx;
pub mod ui;

use engine::prelude::*;

pub use data::{GameData, LevelInfo, Progress, Rules, RunResult, Session, LEVEL_COUNT};
pub use scripts::level_manager::{LevelManager, LevelState};
pub use scripts::player::Player;

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Lantern Keeper";

/// Зарегистрировать компоненты игры (сцены и префабы ссылаются на них по имени).
pub fn register(app: &mut App) {
    scripts::register(app);
}

/// Собрать игру с заданным хранилищем сохранений (тесты передают `SaveData::in_memory()`).
pub fn build_with(save: SaveData) -> Result<App, EngineError> {
    build_in(save, engine::assets_dir!())
}

/// То же, но с каталогом ассетов `assets` (копия проекта после правок в редакторе — в тестах).
pub fn build_in(save: SaveData, assets: impl AsRef<std::path::Path>) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);

    let progress = Progress::load(&save);
    app.world.resources.insert(save);
    engine::mount_assets(&mut app, assets)?; // сцены, префабы, project.settings

    let database = app.world.resources.get::<AssetDatabase>().expect("mount_assets inserted the database").clone();
    app.world.resources.insert(GameData::load(&database)?);
    audio_bank::install(&mut app)?;
    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
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
    let scene = app
        .world
        .resources
        .get::<GameData>()
        .map(|d| d.level(n).scene.clone())
        .unwrap_or_else(|| format!("level{}", n + 1));
    app.world.scenes().load(&scene);
}

engine::game_main!(build, WindowConfig::titled("Хранитель фонаря").with_size(1280, 720));
