//! Скрипты на Rust этой игры: каталог, в котором их создаёт редактор (New Rust Script).
//!
//! ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР — не правьте его руками: он перезаписывается, когда в
//! каталоге создают, переименовывают или удаляют скрипт, и перед каждой сборкой из
//! редактора. В нём `mod` для каждого файла каталога и `register`, который
//! регистрирует все компоненты, объявленные в них через `component!`.
//!
//! Игра вызывает один раз: `scripts::register(&mut app);`

pub mod bosses;
pub mod drone;
pub mod enemy_kinds;
pub mod game_manager;
pub mod hud;
pub mod menus;
pub mod reactor;
pub mod stress;
pub mod touch_controls;
pub mod upgrades;
pub mod waves;
pub mod weapons;

/// Зарегистрировать компоненты всех скриптов каталога.
pub fn register(app: &mut engine::prelude::App) {
    app.register::<bosses::Boss>();
    app.register::<drone::Drone>();
    app.register::<enemy_kinds::EnemyKind>();
    app.register::<game_manager::GameManager>();
    app.register::<hud::Hud>();
    app.register::<menus::Menus>();
    app.register::<reactor::ReactorCore>();
    app.register::<touch_controls::TouchControls>();
    app.register::<upgrades::Upgrades>();
    app.register::<waves::Waves>();
    app.register::<weapons::Weapons>();
}
