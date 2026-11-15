//! Замер нагрузочного забега без окна (стенд движка, `crates/engine/examples/bench/harness.rs`):
//! автопилот, 400 врагов и сотни пуль. `cargo run -p swarm --example bench_swarm` → строки
//! `BENCH {json}` для `tools/bench_report.py`.

#[path = "../../../crates/engine/examples/bench/harness.rs"]
mod harness;

use engine::prelude::*;
use harness::Case;
use swarm::scripts::game_manager::Game;

const DT: f32 = 1.0 / 60.0;

fn press(app: &mut App, key: Key) {
    app.world.input.set_key(key, true);
    app.tick(DT);
    app.world.input.set_key(key, false);
    app.tick(DT);
}

/// Как `tests/perf.rs`: нагрузочный режим, автопилот, кампания, 2 с разгона.
fn stress() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = swarm::build_with(SaveData::in_memory()).expect("the game builds");
    app.run_headless(3, DT);
    let mut game = app.world.resources.remove::<Game>().expect("Game");
    game.debug.stress = true;
    game.debug.bot = true;
    app.world.resources.insert(game);
    press(&mut app, Key::Tab);
    press(&mut app, Key::Enter);
    press(&mut app, Key::Tab);
    press(&mut app, Key::Enter);
    app.run_headless(120, DT);
    app
}

fn main() {
    harness::main_with(vec![Case::new("swarm_stress", "Swarm: stress mode, bot, 400 enemies + bullets", stress)]);
}
