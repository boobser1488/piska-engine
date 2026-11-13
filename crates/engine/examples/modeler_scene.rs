//! Модели Piska Modeler в движке: `assets/scenes/models.scene` — ящик, фонарь, ель и
//! ремонтный робот из `assets/models/*.pmodel`, каждый — обычный `MeshRenderer`.
//!
//! Запуск: `cargo run --example modeler_scene` (с `-- --editor` — в редакторе; двойной щелчок
//! по `.pmodel` в окне Project открывает модель в Piska Modeler).

use engine::prelude::*;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    engine::mount_assets(&mut app, ASSETS)?;
    if std::env::args().any(|a| a == "--editor") {
        return engine::edit(
            app,
            EditorConfig::titled("Piska Modeler Samples").open_scene("scenes/models.scene"),
        );
    }
    app.world.load_scene(format!("{ASSETS}/scenes/models.scene"))?;
    engine::run(app, WindowConfig::titled("Piska Modeler Samples"))
}
