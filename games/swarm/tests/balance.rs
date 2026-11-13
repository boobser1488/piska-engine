//! Подбор баланса: бот проходит кампанию, печатая время и потери по волнам.
//! `cargo test -p swarm --test balance -- --ignored --nocapture`

use swarm::bot::bot_step;
use swarm::sim::*;

#[test]
#[ignore = "долгий замер баланса"]
fn bot_plays_the_campaign() {
    for seed in [1u64, 2, 3] {
        let mut sim = Sim::new(seed, Mode::Campaign);
        let mut last = 0;
        let mut t0 = 0.0;
        let mut frames = 0u64;
        while !sim.is_over() && frames < 60 * 60 * 40 {
            bot_step(&mut sim, 1.0 / 60.0);
            frames += 1;
            if sim.wave != last {
                println!("seed {seed} wave {:>2} started at {:>6.1}s (prev wave {:>5.1}s) core {:>5.0} hp {:>4.0} score {} level {}", sim.wave, sim.time, sim.time - t0, sim.core.hp, sim.player.hp, sim.score, sim.level);
                last = sim.wave;
                t0 = sim.time;
            }
        }
        println!("seed {seed}: {:?} at wave {} after {:.0}s, kills {}", sim.phase, sim.wave, sim.time, sim.run.kills);
    }
}
