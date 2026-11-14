//! Сквозной прогон: бот ведёт шар по маршруту цеха вводом (левый стик и прыжок) — как игрок.

mod common;

use common::*;
use marble::Screen;

#[test]
fn a_scripted_run_finishes_course_one() {
    let mut app = game();
    start_course(&mut app, 0);
    let frames = play_with_bot(&mut app, 60 * 200);
    let st = state(&app);
    assert_eq!(st.screen, Screen::Results, "the bot reached the shipping pad within {frames} frames (clock {:.1}, falls {})", st.clock, st.falls);
    let result = st.result.expect("a result");
    assert!(result.win, "finished, not timed out");
    assert!(result.time < st.courses[0].limit);
    assert!(result.gears >= 3, "the route passes gears: {}", result.gears);
    assert!(st.progress.done[0]);
    println!("course 1 by bot: {:.1} s, {} gears, {} falls, medal {}", result.time, result.gears, result.falls, result.medal);
}

#[test]
fn the_same_run_is_deterministic() {
    let run = || {
        let mut app = game();
        start_course(&mut app, 0);
        play_with_bot(&mut app, 60 * 200);
        let st = state(&app);
        (st.clock, st.gears, st.falls)
    };
    assert_eq!(run(), run(), "a fixed time step and a seeded random give the same run twice");
}

/// Обзор: бот пробует каждый цех (для настройки медалей). `cargo test -p marble --test course_run -- --ignored --nocapture`
#[test]
#[ignore]
fn bot_survey_of_all_courses() {
    for index in 0..marble::COUNT {
        let mut app = game();
        start_course(&mut app, index);
        let frames = play_with_bot(&mut app, 60 * 330);
        let st = state(&app);
        println!(
            "course {}: screen {:?}, clock {:.1} s, gears {}/{}, falls {}, route index done in {} frames",
            index + 1, st.screen, st.clock, st.gears, st.gears_total, st.falls, frames
        );
    }
}

/// Трасса бота по цеху `MARBLE_TRACE=N`: где шар и к какой точке маршрута он едет.
#[test]
#[ignore]
fn bot_trace() {
    let index: usize = std::env::var("MARBLE_TRACE").ok().and_then(|v| v.parse().ok()).unwrap_or(2) - 1;
    let seconds: u32 = std::env::var("MARBLE_TRACE_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let mut app = game();
    start_course(&mut app, index);
    let mut bot = Bot::new();
    let mut last_falls = 0;
    for frame in 0..(60 * seconds) {
        bot.drive(&mut app);
        app.run_headless(1, DT);
        let st = state(&app);
        if frame % 10 == 0 && frame > 60 && frame < 60 * 40 || st.falls != last_falls {
            let route = &st.route;
            let i = bot.route_index().min(route.len() - 1);
            let p = ball_pos(&app);
            println!(
                "t={:5.1} pos=({:6.1},{:5.1},{:6.1}) wp#{} {:?} at ({:.1},{:.1},{:.1}) falls={}",
                st.clock, p.x, p.y, p.z, i, route[i].kind, route[i].pos[0], route[i].pos[1], route[i].pos[2], st.falls
            );
            last_falls = st.falls;
        }
        if state(&app).screen == Screen::Results {
            break;
        }
    }
}

/// Каждый из восьми цехов проходим: бот доезжает до площадки отгрузки (с возвратами на
/// контрольные точки) быстрее бронзового времени.
#[test]
fn a_scripted_run_finishes_every_course() {
    for index in 0..marble::COUNT {
        let mut app = game();
        start_course(&mut app, index);
        play_with_bot(&mut app, 60 * 200);
        let st = state(&app);
        let bronze = st.courses[index].bronze;
        let result = st.result.expect("a result");
        assert!(result.win, "course {} finished by the bot (clock {:.1}, falls {})", index + 1, st.clock, st.falls);
        assert!(result.time < bronze, "the scripted run is faster than the bronze time of course {}: {:.1}", index + 1, result.time);
        assert!(result.gears >= 3, "course {}: the route collects gears", index + 1);
    }
}
