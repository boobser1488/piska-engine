//! Замер кадра цеха 8 без окна (стенд движка, `crates/engine/examples/bench/harness.rs`).
//! `cargo run -p marble --example bench_marble` → строки `BENCH {json}` для `tools/bench_report.py`.

#[path = "../../../crates/engine/examples/bench/harness.rs"]
mod harness;

use engine::prelude::*;
use harness::Case;
use marble::Boot;

fn course8() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let boot = Boot { screen: Some("c8".into()), at: 0.0, demo_progress: false };
    let mut app = marble::build_with(SaveData::in_memory(), boot).expect("the game builds");
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app.run_headless(5, 1.0 / 60.0);
    app
}

fn main() {
    harness::main_with(vec![Case::new("marble_course8", "Marble: course 8, ball at the start, game camera", course8)]);
}
