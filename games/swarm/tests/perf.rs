//! Замер нагрузочной сцены (400 врагов, 600 пуль): время кадра симуляции и отрисовки,
//! draw call'ы. Нужна видеокарта, поэтому тест помечен `#[ignore]`:
//! `cargo test -p swarm --test perf -- --ignored --nocapture`.

use engine::prelude::*;
use swarm::scripts::game_manager::Game;
use swarm::sim::Mode;

#[test]
#[ignore = "нужна видеокарта; печатает числа"]
fn stress_scene_frame_time() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = swarm::build_with(SaveData::in_memory()).unwrap();
    app.run_headless(3, 1.0 / 60.0);
    {
        let id = app.world.scene.find_by_name("GameManager").unwrap();
        let _ = id;
    }
    // Забег в нагрузочном режиме: автопилот и 400 врагов.
    let mut game = app.world.resources.remove::<Game>().unwrap();
    game.debug.stress = true;
    game.debug.bot = true;
    app.world.resources.insert(game);
    app.world.input.set_key(Key::Tab, true);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Tab, false);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Enter, true);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Enter, false);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Tab, true);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Tab, false);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Enter, true);
    app.tick(1.0 / 60.0);
    app.world.input.set_key(Key::Enter, false);
    app.run_headless(120, 1.0 / 60.0);
    {
        let game = app.world.resources.get::<Game>().unwrap();
        assert_eq!(game.sim.mode, Mode::Campaign);
        println!("enemies {} player bullets {} enemy bullets {}", game.sim.alive_enemies(), game.sim.pbullets_alive(), game.sim.ebullets_alive());
    }
    let mut totals = (0.0f32, 0.0f32, 0.0f32);
    let n = 200;
    for _ in 0..n {
        app.tick(1.0 / 60.0);
        let p = app.profile();
        totals.0 += p.total_ms;
        totals.1 += p.scripts_ms;
        totals.2 += p.subsystems.iter().find(|(name, _)| *name == "audio").map_or(0.0, |(_, ms)| *ms);
    }
    println!("tick: {:.2} ms/frame (scripts {:.2}, audio subsystem {:.3})", totals.0 / n as f32, totals.1 / n as f32, totals.2 / n as f32);
    let mut renderer = engine::render::Renderer::headless().expect("a GPU adapter");
    let mut stats = None;
    let t = std::time::Instant::now();
    for _ in 0..30 {
        stats = Some(renderer.capture_view(&app.world, 1280, 720, None).expect("capture").1);
    }
    let s = stats.unwrap();
    println!("render+readback: {:.2} ms/frame; draw calls {}, sprites {}, extract {:.2} ms, encode {:.2} ms", t.elapsed().as_secs_f32() * 1000.0 / 30.0, s.draw_calls, s.sprites, s.extract_ms, s.encode_ms);
}
