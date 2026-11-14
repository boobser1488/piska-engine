//! Скрипты на Rust этой игры: каталог, в котором их создаёт редактор (New Rust Script).
//!
//! ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР — не правьте его руками: он перезаписывается, когда в
//! каталоге создают, переименовывают или удаляют скрипт, и перед каждой сборкой из
//! редактора. В нём `mod` для каждого файла каталога и `register`, который
//! регистрирует все компоненты, объявленные в них через `component!`.
//!
//! Игра вызывает один раз: `scripts::register(&mut app);`

pub mod ball;
pub mod blinker;
pub mod booster;
pub mod bumper;
pub mod camera_rig;
pub mod checkpoint;
pub mod cog;
pub mod collapsing_tile;
pub mod conveyor;
pub mod crusher;
pub mod fan;
pub mod finish_pad;
pub mod game_manager;
pub mod gear_pickup;
pub mod moving_platform;
pub mod seesaw_rig;
pub mod sweeper;
pub mod sweeper_arm;

/// Зарегистрировать компоненты всех скриптов каталога.
pub fn register(app: &mut engine::prelude::App) {
    app.register::<ball::Ball>();
    app.register::<blinker::Blinker>();
    app.register::<booster::Booster>();
    app.register::<bumper::Bumper>();
    app.register::<camera_rig::CameraRig>();
    app.register::<checkpoint::Checkpoint>();
    app.register::<cog::Cog>();
    app.register::<collapsing_tile::CollapsingTile>();
    app.register::<conveyor::Conveyor>();
    app.register::<crusher::Crusher>();
    app.register::<fan::Fan>();
    app.register::<finish_pad::FinishPad>();
    app.register::<game_manager::GameManager>();
    app.register::<gear_pickup::GearPickup>();
    app.register::<moving_platform::MovingPlatform>();
    app.register::<seesaw_rig::SeesawRig>();
    app.register::<sweeper::Sweeper>();
    app.register::<sweeper_arm::SweeperArm>();
}
