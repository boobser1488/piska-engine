//! Скрипты на Rust этой игры: каталог, в котором их создаёт редактор (New Rust Script).
//!
//! ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР — не правьте его руками: он перезаписывается, когда в
//! каталоге создают, переименовывают или удаляют скрипт, и перед каждой сборкой из
//! редактора. В нём `mod` для каждого файла каталога и `register`, который
//! регистрирует все компоненты, объявленные в них через `component!`.
//!
//! Игра вызывает один раз: `scripts::register(&mut app);`

pub mod bat;
pub mod boss;
pub mod brute;
pub mod checkpoint;
pub mod crawler;
pub mod crumble_platform;
pub mod exit;
pub mod firefly;
pub mod gate;
pub mod level_manager;
pub mod menu_controller;
pub mod moving_platform;
pub mod parallax;
pub mod player;
pub mod projectile;
pub mod rock;
pub mod spitter;
pub mod spring;

/// Зарегистрировать компоненты всех скриптов каталога.
pub fn register(app: &mut engine::prelude::App) {
    app.register::<bat::Bat>();
    app.register::<boss::Boss>();
    app.register::<brute::Brute>();
    app.register::<checkpoint::Checkpoint>();
    app.register::<crawler::Crawler>();
    app.register::<crumble_platform::CrumblePlatform>();
    app.register::<exit::Exit>();
    app.register::<firefly::Firefly>();
    app.register::<gate::Gate>();
    app.register::<level_manager::LevelManager>();
    app.register::<menu_controller::MenuController>();
    app.register::<moving_platform::MovingPlatform>();
    app.register::<parallax::Parallax>();
    app.register::<player::Player>();
    app.register::<projectile::Projectile>();
    app.register::<rock::Rock>();
    app.register::<spitter::Spitter>();
    app.register::<spring::Spring>();
}
