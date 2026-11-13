//! Сценарий на языке Scenario поверх обычной 3D-сцены: отсчёт со звуком, кубы выезжают
//! по `move_to` (каждый — своей задачей), лампа пульсирует параллельной задачей,
//! обработчик сообщения меняет надпись интерфейса.
//!
//! Запуск: `cargo run --example scenario_demo`. Сценарий — `assets/scenarios/demo.scenario`:
//! правьте и сохраняйте его прямо во время игры — он перезапустится сам.

use engine::prelude::*;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Сцена демо (без окна — её же гоняет тест `tests/scenario_demo.rs`).
pub fn build() -> Result<App, EngineError> {
    let mut app = engine::app();
    engine::mount_assets(&mut app, format!("{ROOT}/assets"))?;
    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        audio.add_clip("blip", engine::audio::synth::sfx::coin());
        audio.add_clip("done", engine::audio::synth::sfx::powerup());
    }

    app.spawn("Camera")
        .at3(0.0, 4.2, 9.5)
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.0, -0.36, 0.0))
        .with(Camera::perspective(50.0));
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0))
        .with(Light::default());
    app.spawn("Lamp")
        .at3(0.0, 2.6, 0.0)
        .with(Light::point(Color::hex(0xff8a3c), 8.0, 9.0))
        .with(MeshRenderer::sphere().with_emissive(Color::rgb(9.0, 3.6, 0.9)).without_shadows())
        .scale(0.25, 0.25, 0.25);
    app.spawn("Ground")
        .scale(60.0, 1.0, 60.0)
        .with(MeshRenderer::plane().with_color(Color::hex(0x6f7a68)).with_material(0.0, 0.9).with_grid().without_shadows());
    for (i, (name, color)) in [("Red", 0xff7a59), ("Gold", 0xffc35a), ("Blue", 0x6cb4ff)].into_iter().enumerate() {
        app.spawn(name)
            .tag("Cube")
            .at3(-4.0 + i as f32 * 4.0, 0.6, -4.0)
            .scale(1.2, 1.2, 1.2)
            .with(MeshRenderer::cube().with_color(Color::hex(color)).with_material(0.0, 0.4));
    }

    let canvas = app.world.scene.ui_canvas().name("HUD").id();
    app.world
        .scene
        .ui_text(canvas, "…")
        .name("Caption")
        .anchor(Anchor::Top)
        .pos(0.0, 40.0)
        .size(900.0, 70.0)
        .font_size(44.0)
        .bold()
        .align(TextAlign::Center)
        .outline(2.0, Color::BLACK);

    app.spawn("Director").with(ScenarioRunner::new("scenarios/demo.scenario"));
    Ok(app)
}

#[allow(dead_code)]
fn main() -> Result<(), EngineError> {
    let app = build()?;
    engine::run(app, WindowConfig::titled("Scenario demo").with_size(1280, 720))
}
