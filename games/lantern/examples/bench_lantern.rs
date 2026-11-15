//! Замер кадра самого тяжёлого уровня (4-й: больше всего объектов в сцене) без окна —
//! стенд движка, `crates/engine/examples/bench/harness.rs`.
//! `cargo run -p lantern --example bench_lantern` → строки `BENCH {json}` для `tools/bench_report.py`.

#[path = "../../../crates/engine/examples/bench/harness.rs"]
mod harness;

use engine::prelude::*;
use harness::Case;

fn level4() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = lantern::build_with(SaveData::in_memory()).expect("the game builds");
    app.run_headless(2, 1.0 / 60.0);
    lantern::goto_level(&mut app, 3);
    app.run_headless(5, 1.0 / 60.0);
    app
}

fn main() {
    harness::main_with(vec![Case::new("lantern_level4", "Lantern: level 4 (largest scene), player idle", level4)]);
}
