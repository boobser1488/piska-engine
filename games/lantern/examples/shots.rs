//! Снимки экрана без окна (для разработки и проверки глазами).
//!
//! ```text
//! LANTERN_SCREEN=menu|levels|options|help|pause|win|lose|level1..level6 \
//!   [LANTERN_PLAYER=x,y] [LANTERN_CAM=x,y,ortho] [LANTERN_STOMP=1] \
//!   ENGINE_SCREENSHOT=out.png ENGINE_FRAMES=30 cargo run -p lantern --example shots
//! ```
//! `LANTERN_PLAYER` переносит игрока (ступни), `LANTERN_CAM` ставит неподвижную камеру
//! (центр и половину высоты кадра) — так снимается обзор всего уровня.

use engine::prelude::*;
use lantern::{LevelManager, Progress, Rules, RunResult};

const DT: f32 = 1.0 / 60.0;

fn pair(name: &str) -> Option<Vec<f32>> {
    let text = std::env::var(name).ok()?;
    Some(text.split(',').filter_map(|v| v.trim().parse().ok()).collect())
}

fn press(app: &mut App, key: Key) {
    app.world.input.set_key(key, true);
    app.tick(DT);
    app.world.input.set_key(key, false);
    app.tick(DT);
}

fn focus(app: &mut App, name: &str) {
    for _ in 0..30 {
        let focused = app.world.ui().focused().and_then(|id| app.world.scene.get(id)).map(|o| o.name.clone());
        if focused.as_deref() == Some(name) {
            return;
        }
        press(app, Key::Tab);
    }
}

fn main() -> Result<(), EngineError> {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = lantern::build_with(SaveData::in_memory())?;
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    let screen = std::env::var("LANTERN_SCREEN").unwrap_or_else(|_| "menu".to_string());

    // Немного прогресса, чтобы меню и результаты выглядели «живыми».
    if let Some(p) = app.world.resources.get_mut::<Progress>() {
        p.record(0, &RunResult { time: 112.0, fireflies: 41, total: 45, golden: true, deaths: 0 }, 150.0, &Rules::default());
        p.record(1, &RunResult { time: 190.0, fireflies: 22, total: 37, golden: false, deaths: 1 }, 190.0, &Rules::default());
        p.record(2, &RunResult { time: 280.0, fireflies: 34, total: 40, golden: false, deaths: 2 }, 240.0, &Rules::default());
        p.record(3, &RunResult { time: 300.0, fireflies: 30, total: 40, golden: true, deaths: 1 }, 280.0, &Rules::default());
        if screen.starts_with("level") {
            // последние уровни — со всеми способностями
        }
    }
    app.world.scenes().load("menu");
    app.run_headless(3, DT);

    match screen.as_str() {
        "levels" => {
            focus(&mut app, "play");
            press(&mut app, Key::Enter);
            app.run_headless(30, DT);
        }
        "options" => {
            focus(&mut app, "options");
            press(&mut app, Key::Enter);
            app.run_headless(30, DT);
        }
        "help" => {
            focus(&mut app, "help");
            press(&mut app, Key::Enter);
            app.run_headless(30, DT);
        }
        s if s.starts_with("level") => {
            let n: usize = s[5..].parse().unwrap_or(1);
            lantern::goto_level(&mut app, n.saturating_sub(1));
            app.run_headless(4, DT);
            if let Some(p) = pair("LANTERN_PLAYER") {
                if let Some(id) = app.world.scene.find_by_tag("Player") {
                    app.world.scene.get_mut(id).unwrap().transform.position = Vec3::new(p[0], p[1], 0.0);
                }
            }
            if std::env::var("LANTERN_STOMP").is_ok() {
                // кадры, в которых видны эффекты: пара светлячков собрана
                if let Some(id) = app.world.scene.find_by_name("Level") {
                    if let Some(m) = app.world.scene.get_component_mut::<LevelManager>(id) {
                        m.fireflies = m.need / 2;
                    }
                }
            }
            app.run_headless(20, DT);
            if screen == "pause" {
                press(&mut app, Key::Escape);
            }
            if let Some(c) = pair("LANTERN_CAM") {
                let cam = app.world.scene.find_by_tag("MainCamera").unwrap();
                if let Some(ctrl) = app.world.scene.get_component_mut::<CameraController2D>(cam) {
                    ctrl.target_tag = String::new();
                    ctrl.use_bounds = false;
                }
                if let Some(cm) = app.world.scene.get_component_mut::<Camera>(cam) {
                    cm.ortho_size = c.get(2).copied().unwrap_or(7.0);
                }
                app.world.scene.get_mut(cam).unwrap().transform.position = Vec3::new(c[0], c[1], 0.0);
                app.run_headless(2, DT);
            }
        }
        "pause" | "win" | "lose" => {
            lantern::goto_level(&mut app, 0);
            app.run_headless(4, DT);
            if let Some(p) = pair("LANTERN_PLAYER") {
                if let Some(id) = app.world.scene.find_by_tag("Player") {
                    app.world.scene.get_mut(id).unwrap().transform.position = Vec3::new(p[0], p[1], 0.0);
                }
            }
            app.run_headless(30, DT);
            match screen.as_str() {
                "pause" => press(&mut app, Key::Escape),
                "win" => {
                    let id = app.world.scene.find_by_name("Level").unwrap();
                    if let Some(m) = app.world.scene.get_component_mut::<LevelManager>(id) {
                        m.fireflies = m.need + 12;
                        m.golden = true;
                        m.time = 120.0;
                    }
                    let exit = app.world.scene.find_with::<lantern::scripts::exit::Exit>()[0];
                    let at = app.world.scene.world_position(exit);
                    let pl = app.world.scene.find_by_tag("Player").unwrap();
                    app.world.scene.get_mut(pl).unwrap().transform.position = at;
                    app.run_headless(150, DT);
                }
                _ => {
                    let pl = app.world.scene.find_by_tag("Player").unwrap();
                    app.world.send_message(pl, "Hurt", Value::Vec3(1.0, 0.0, 0.0));
                    app.run_headless(2, DT);
                    if let Some(p) = app.world.scene.get_component_mut::<lantern::Player>(pl) {
                        p.hp = 1;
                        p.invuln = 0.0;
                    }
                    app.world.send_message(pl, "Hurt", Value::Vec3(1.0, 0.0, 0.0));
                    app.run_headless(120, DT);
                }
            }
        }
        _ => {}
    }
    engine::run(app, WindowConfig::titled("Хранитель фонаря").with_size(1280, 720))
}
