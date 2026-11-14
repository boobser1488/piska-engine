//! Камера игрока идёт ровно в настоящей игре (сцена и префабы из ассетов), а не только на
//! испытательном уровне движка: за каждый кадр она проходит путь, пропорциональный
//! длительности кадра, при любой частоте монитора.

mod common;

use engine::fps::{FpsRuntime, UserCmd};
use engine::prelude::*;

fn camera_of(app: &App, player: GameObjectId) -> GameObjectId {
    let scene = &app.world.scene;
    let mut stack = vec![player];
    while let Some(id) = stack.pop() {
        if id != player && scene.get_component::<Camera>(id).is_some() {
            return id;
        }
        if let Some(object) = scene.get(id) {
            stack.extend(object.children().iter().copied());
        }
    }
    panic!("the player has no camera child");
}

/// Самое свободное направление от точки старта: куда тело уходит дальше всего за 3 секунды.
fn free_yaw() -> f32 {
    let mut best = (0.0f32, 0.0f32);
    for step in 0..12 {
        let yaw = step as f32 * 30.0;
        let mut app = common::game();
        let player = common::press(&mut app, "play");
        let start = common::state(&app, player).origin;
        app.world.resources.get_or_insert_with(FpsRuntime::default).set_command(player, Some(UserCmd { forward: 1.0, yaw_deg: yaw, ..UserCmd::default() }));
        app.run_headless(64 * 3, common::TICK);
        let gone = (common::state(&app, player).origin - start).length();
        if gone > best.0 {
            best = (gone, yaw);
        }
    }
    assert!(best.0 > 9.0, "no direction with a free run from the spawn: best {:.1} m", best.0);
    best.1
}

#[test]
fn the_player_camera_moves_evenly_at_any_refresh_rate() {
    let yaw = free_yaw();
    let mut report = Vec::new();
    let mut worst = 0.0f32;
    for (name, pattern) in [
        ("60 Hz", vec![1.0 / 60.0]),
        ("75 Hz", vec![1.0 / 75.0]),
        ("144 Hz", vec![1.0 / 144.0]),
        ("60 Hz with jitter", vec![0.0150, 0.0183, 0.0162, 0.0171, 0.0158, 0.0176]),
    ] {
        let mut app = common::game();
        let player = common::press(&mut app, "play");
        let camera = camera_of(&app, player);
        app.world.resources.get_or_insert_with(FpsRuntime::default).set_command(player, Some(UserCmd { forward: 1.0, yaw_deg: yaw, ..UserCmd::default() }));
        let mut frame = 0usize;
        let mut dt = || {
            frame += 1;
            pattern[frame % pattern.len()]
        };
        let mut t = 0.0;
        while t < 0.8 {
            let d = dt();
            app.tick(d);
            t += d;
        }
        let speed = common::state(&app, player).velocity.length();
        assert!(speed > 4.0, "{name}: the player is running: {speed:.2} m/s");
        let mut previous = app.world.scene.world_position(camera);
        let (mut lo, mut hi) = (f32::MAX, 0.0f32);
        let mut t = 0.0;
        while t < 1.0 {
            let d = dt();
            app.tick(d);
            t += d;
            let now = app.world.scene.world_position(camera);
            let apparent = (now - previous).length() / d / speed;
            previous = now;
            lo = lo.min(apparent);
            hi = hi.max(apparent);
        }
        report.push(format!("{name}: per-frame speed {:.0}%..{:.0}% of the real one", lo * 100.0, hi * 100.0));
        worst = worst.max((1.0 - lo).abs()).max((hi - 1.0).abs());
    }
    assert!(worst < 0.05, "the camera does not move evenly:\n{}", report.join("\n"));
}
