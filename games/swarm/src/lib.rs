//! «Рой» — аркадный шутер на выживание в круглой арене на Piska Engine.
//!
//! Одинокий дрон-ремонтник защищает реактор от волн взбесившихся машин: 20 волн,
//! 4 босса, 18 улучшений, бесконечный режим.
//!
//! Игра — проект, который можно читать и править в редакторе:
//! * `assets/scenes/arena.scene` — арена, реактор, дрон, камера, менеджер игры и экраны
//!   интерфейса (экземпляры префабов из `assets/ui`);
//! * `assets/prefabs/` — дрон, враги, боссы, снаряды, обломки, эффекты;
//! * `assets/data/*.ron` — враги, улучшения, баланс, таблица волн;
//! * `assets/audio/*.wav` — звуки и музыка; `assets/project.settings` — управление;
//! * `src/scripts/` — скрипты (по файлу на поведение), `src/sim.rs` — пул врагов и пуль с
//!   сеткой столкновений, `src/view.rs` — спрайты пулов из префабов.
//!
//! Здесь только сборка игры: подключить ассеты, зарегистрировать скрипты, загрузить сцену.

pub mod atlas;
pub mod bot;
pub mod data;
pub mod debug;
pub mod defs;
pub mod grid;
pub mod profile;
pub mod rng;
pub mod sim;
pub mod sound;
pub mod ui;
pub mod view;

/// Скрипты на Rust (по файлу на поведение); `mod.rs` пишет редактор.
pub mod scripts;

use engine::prelude::*;

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Piska Swarm";

/// Зарегистрировать компоненты игры (сцена и префабы ссылаются на них по имени).
pub fn register(app: &mut App) {
    scripts::register(app);
}

/// Собрать игру с заданным хранилищем сохранений (тесты передают `SaveData::in_memory()`).
/// Управление берётся из `assets/project.settings`, звуки — из `assets/audio`.
pub fn build_with(save: SaveData) -> Result<App, EngineError> {
    build_from(save, engine::assets_dir!())
}

/// То же, но с каталогом ассетов `assets` (тесты проверяют копию проекта после правок в редакторе).
pub fn build_from(save: SaveData, assets: impl AsRef<std::path::Path>) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    app.world.resources.insert(save);
    engine::mount_assets(&mut app, assets)?;
    let db = app.world.resources.get::<AssetDatabase>().cloned().expect("mount_assets inserted the database");
    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        sound::register(audio, &db);
    }
    engine::load_scene(&mut app, "scenes/arena.scene")?;
    Ok(app)
}

/// Собрать игру: сохранения — там, где они живут на этой платформе; зерно случайности — своё
/// на каждый запуск.
pub fn build() -> Result<App, EngineError> {
    let mut app = build_with(engine::save_data(GAME_NAME))?;
    app.world.resources.insert(Random::from_entropy());
    Ok(app)
}

engine::game_main!(build, WindowConfig::titled("Рой").with_size(1280, 720));
