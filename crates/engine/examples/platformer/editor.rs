//! Редактор, открытый на уровне платформера.
//!
//! Запуск: `cargo run --example editor`.
//!
//! Редактор — библиотека: это та же игра с теми же скриптами, только вместо
//! `engine::run` вызывается `engine::edit`. Поэтому `PlayerController`, `Coin` и
//! `CameraFollow` видны в инспекторе и в списке Add Component без всякой настройки.

use engine::prelude::*;

mod scripts;

/// Каталог ассетов в корне репозитория.
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS)?;
    engine::edit(
        app,
        EditorConfig::titled("Piska Editor").open_scene("scenes/level1.scene"),
    )
}
