//! Скрипты на Rust этой игры: каталог, в котором их создаёт редактор (New Rust Script).
//!
//! ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР — не правьте его руками: он перезаписывается, когда в
//! каталоге создают, переименовывают или удаляют скрипт, и перед каждой сборкой из
//! редактора. В нём `mod` для каждого файла каталога и `register`, который
//! регистрирует все компоненты, объявленные в них через `component!`.
//!
//! Игра вызывает один раз: `scripts::register(&mut app);`

pub mod checkpoint;
pub mod game_manager;
pub mod hud;
pub mod lantern_flicker;
pub mod map_zone;
pub mod menu_camera;
pub mod movement_course;
pub mod moving_platform;
pub mod player_sounds;
pub mod spawn_point;

/// Зарегистрировать компоненты всех скриптов каталога.
pub fn register(app: &mut engine::prelude::App) {
    app.register::<checkpoint::Checkpoint>();
    app.register::<game_manager::GameManager>();
    app.register::<hud::Hud>();
    app.register::<lantern_flicker::LanternFlicker>();
    app.register::<map_zone::MapZone>();
    app.register::<menu_camera::MenuCamera>();
    app.register::<movement_course::MovementCourse>();
    app.register::<moving_platform::MovingPlatform>();
    app.register::<player_sounds::PlayerSounds>();
    app.register::<spawn_point::SpawnPoint>();
}
