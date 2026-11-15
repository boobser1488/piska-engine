//! Замер кадра карты без окна (стенд движка, `crates/engine/examples/bench/harness.rs`):
//! проход вдоль длинной стены и вид с крыши на весь город.
//! `cargo run -p tashbazar --example bench_tashbazar` → строки `BENCH {json}` для
//! `tools/bench_report.py`.

#[path = "../../../crates/engine/examples/bench/harness.rs"]
mod harness;

use engine::prelude::*;
use harness::Case;

const DT: f32 = 1.0 / 60.0;

fn game() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = tashbazar::build_in(SaveData::in_memory(), concat!(env!("CARGO_MANIFEST_DIR"), "/assets"), tashbazar::MAP_SCENE)
        .expect("the game builds");
    app.run_headless(3, DT);
    let game = app.world.scene.find_by_name("Game").expect("Game");
    app.world.send_message(game, "play", Value::Nil);
    app.run_headless(5, DT);
    app
}

/// Поставить игрока (в полёте) в точку с поворотом и наклоном.
fn place(app: &mut App, feet: Vec3, yaw: f32, pitch: f32) {
    let Some(player) = app.world.scene.find_by_name("Player") else {
        return;
    };
    if let Some(object) = app.world.scene.get_mut(player) {
        object.transform.position = feet;
    }
    if let Some(c) = app.world.scene.get_component_mut::<FpsController>(player) {
        c.yaw_deg = yaw;
        c.pitch_deg = pitch;
        c.velocity = Vec3::ZERO;
        c.noclip = true;
    }
}

fn main() {
    // Загрузка карты по частям: `-- --load-split`.
    if std::env::args().any(|a| a == "--load-split") {
        harness::load_split(
            "tashbazar",
            || {
                let mut app = engine::app();
                tashbazar::register(&mut app);
                app.world.resources.insert(SaveData::in_memory());
                engine::mount_assets(&mut app, concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("assets");
                app
            },
            tashbazar::MAP_SCENE,
        );
        return;
    }
    harness::main_with(vec![
        Case::new("tashbazar_street", "Tashbazar: walk along the long wall street", game).with_step(|app, frame| {
            // Вдоль стены по оси z, взгляд вдоль улицы.
            let z = -45.0 + (frame % 360) as f32 * 0.25;
            place(app, Vec3::new(-46.0, 0.0, z), 180.0, 0.0);
        }),
        Case::new("tashbazar_rooftop", "Tashbazar: rooftop view over the whole town, slow pan", game).with_step(|app, frame| {
            place(app, Vec3::new(0.0, 14.0, -62.0), 160.0 + (frame as f32 * 0.1).sin() * 25.0, -14.0);
        }),
    ]);
}
