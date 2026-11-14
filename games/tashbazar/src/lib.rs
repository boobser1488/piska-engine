//! «Ташбазар» — контроллер от первого лица с классической физикой движения
//! (`engine::fps::FpsController`) и карта в жанре командных шутеров: пустынный городок
//! на две точки задания, со спавнами двух сторон, серединой и двумя флангами.
//!
//! Игра — образец проекта: **содержимое лежит в ассетах** (сцены `assets/scenes/`, префабы
//! `assets/prefabs/`, интерфейс `assets/ui/`, материалы и текстуры, модели, звуки, данные
//! `assets/data/`, настройки `assets/project.settings`), а **код — это скрипты**
//! (`src/scripts/`, по одному компоненту в файле). `build()` подключает ассеты, регистрирует
//! скрипты и загружает сцену карты; меню, HUD и игрок — объекты этой сцены. Ассеты пишет
//! инструмент `examples/bake_assets.rs` (`tools/bake/`); в игре он не используется.

/// Ядро без компонентов: настройки, данные из ассетов, события.
pub mod game;
/// Скрипты на Rust (редактор: Assets → New Rust Script).
pub mod scripts;

use engine::prelude::*;

pub use game::data::{Route, RoutePoint, RouteTable, Step};
pub use game::options::Options;

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Tashbazar";

/// Сцена карты (с главным меню).
pub const MAP_SCENE: &str = "scenes/tashbazar.scene";
/// Сцена-полигон с эталонными размерами.
pub const GYM_SCENE: &str = "scenes/gym.scene";

/// Зарегистрировать компоненты игры.
pub fn register(app: &mut App) {
    scripts::register(app);
}

/// Собрать игру с заданными сохранениями, каталогом ассетов и первой сценой.
pub fn build_in(save: SaveData, assets: impl AsRef<std::path::Path>, scene: &str) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    app.world.resources.insert(save);
    // Настройки проекта (ввод, шаг физики, окно) читаются отсюда же.
    engine::mount_assets(&mut app, assets)?;
    engine::load_scene(&mut app, scene)?;
    Ok(app)
}

/// Собрать игру с заданными сохранениями: ассеты из каталога игры, сцена карты.
pub fn build_with(save: SaveData) -> Result<App, EngineError> {
    build_in(save, engine::assets_dir!(), MAP_SCENE)
}

/// Собрать игру: сохранения — там, где они живут на этой платформе. `TASHBAZAR_SCENE=gym`
/// (не в браузере) открывает сразу полигон.
pub fn build() -> Result<App, EngineError> {
    #[cfg(not(target_arch = "wasm32"))]
    let scene = match std::env::var("TASHBAZAR_SCENE").as_deref() {
        Ok("gym") => GYM_SCENE,
        _ => MAP_SCENE,
    };
    #[cfg(target_arch = "wasm32")]
    let scene = MAP_SCENE;
    build_in(engine::save_data(GAME_NAME), engine::assets_dir!(), scene)
}

engine::game_main!(build, WindowConfig::titled("Ташбазар").with_size(1280, 720));
