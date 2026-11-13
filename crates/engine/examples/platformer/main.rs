//! 2D-платформер: уровень и префаб монеты загружаются из RON, физика — Rapier2D,
//! камера следит за игроком.
//!
//! Запуск: `cargo run --example platformer` — A/D или стрелки, прыжок — пробел.

use engine::prelude::*;

mod scripts;

/// Каталог ассетов в корне репозитория.
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS)?;
    app.world.load_scene(format!("{ASSETS}/scenes/level1.scene"))?;
    engine::run(app, WindowConfig::titled("Platformer"))
}
