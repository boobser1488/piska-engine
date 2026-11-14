//! Бот проходит все главные маршруты карты (от обоих спавнов к обеим точкам и к середине)
//! контроллером, как игрок: не застревает и укладывается в расчётное время ±15 %.
//! А полосу препятствий — с прыжками, прыжками с приседом и стрейфом — проходит до финиша.
//! `cargo test -p tashbazar --test routes` (без окна и звука).

mod common;

use common::*;
use tashbazar::game::CourseFinished;
use tashbazar::scripts::game_manager::{GameManager, Screen};
use tashbazar::scripts::movement_course::MovementCourse;

#[test]
fn every_main_route_is_walkable_within_its_timing() {
    let mut app = game();
    let table = routes(&app);
    let main: Vec<_> = table.routes.iter().filter(|r| r.id != "course").cloned().collect();
    assert_eq!(main.len(), 8, "four routes per side");
    let player = press(&mut app, "play");
    let mut lines = Vec::new();
    for route in &main {
        place_at_start(&mut app, player, route);
        let report = drive(&mut app, player, route);
        run_until_done(&mut app, &report, route.seconds * 2.0 + 5.0);
        let r = report.borrow().clone();
        let at = state(&app, player).origin;
        assert!(r.stuck.is_none(), "{}: stuck at {:?} heading to point {}", route.id, r.stuck, r.stuck.map_or(0, |s| s.2));
        let finished = r.finished_at.unwrap_or_else(|| panic!("{}: did not finish (reached {} of {}, at {at})", route.id, r.reached, route.points.len()));
        let seconds = (finished - r.started_at.unwrap()) as f32 * TICK;
        lines.push(format!("{:14} {:5.1} m  written {:5.1} s  bot {:5.1} s", route.id, route.length(), route.seconds, seconds));
        assert!(
            (seconds - route.seconds).abs() <= route.seconds * 0.15,
            "{}: {seconds:.1} s, written {:.1} s (±15 %)",
            route.id,
            route.seconds
        );
        assert!(r.max_speed_ups < 260.0, "{}: walking never exceeds run speed ({:.0})", route.id, r.max_speed_ups);
    }
    for line in lines {
        println!("{line}");
    }
}

#[test]
fn the_movement_course_is_completable_by_a_scripted_bot() {
    let mut app = game();
    let table = routes(&app);
    let route = table.get("course").expect("the course route").clone();
    let player = press(&mut app, "course");
    let course_id = app.world.scene.find_by_name("Movement Course").expect("course");
    assert!(app.world.scene.get_component::<MovementCourse>(course_id).unwrap().armed, "on the start pad");
    let report = drive(&mut app, player, &route);
    let mut finished = None;
    for _ in 0..(90.0 / TICK) as usize {
        app.tick(TICK);
        if let Some(done) = app.world.events.read::<CourseFinished>().next() {
            finished = Some(*done);
        }
        if finished.is_some() {
            break;
        }
    }
    let r = report.borrow().clone();
    let course = app.world.scene.get_component::<MovementCourse>(course_id).unwrap().clone();
    let at = state(&app, player).origin;
    let done = finished.unwrap_or_else(|| {
        panic!("the bot did not finish: reached point {} of {}, next checkpoint {}, falls {}, at {at}, stuck {:?}", r.reached, route.points.len(), course.next, course.falls, r.stuck)
    });
    assert_eq!(course.falls, 0, "a clean run: no falls");
    assert!(r.jumps >= 8, "the route needs jumps: {}", r.jumps);
    assert!(r.max_speed_ups > 255.0, "the strafe-jump gains speed: {:.0} u/s", r.max_speed_ups);
    println!("course by bot: {:.2} s, {} jumps, top speed {:.0} u/s", done.time, r.jumps, r.max_speed_ups);
    // Итоговый экран и лучшее время в сохранениях.
    app.run_headless(3, TICK);
    let game = app.world.scene.find_by_name("Game").unwrap();
    assert_eq!(app.world.scene.get_component::<GameManager>(game).unwrap().screen, Screen::Results);
    let best = app.world.resources.get::<engine::core::save::SaveData>().unwrap().float("course.best", 0.0);
    assert!((best - done.time).abs() < 1e-3, "best time saved: {best}");
}

#[test]
fn a_straight_jump_cannot_cross_the_aqueduct_breach_but_a_strafe_jump_can() {
    // Тот же участок полосы: прыжок без стрейфа с края пролома падает в двор.
    let mut app = game();
    let table = routes(&app);
    let course = table.get("course").unwrap();
    let player = press(&mut app, "course");
    let start = course.points.iter().position(|p| p.step == tashbazar::Step::StrafeJump).unwrap() - 1;
    let mut plain = course.clone();
    plain.points = course.points[start..start + 3].to_vec();
    plain.points[1].step = tashbazar::Step::Jump;
    place_at_start(&mut app, player, &plain);
    let report = drive(&mut app, player, &plain);
    run_until_done(&mut app, &report, 6.0);
    let fell = state(&app, player).origin.y < 2.0 || report.borrow().finished_at.is_none();
    assert!(fell, "a plain running jump must not clear the 4.35 m breach");

    let mut strafe = course.clone();
    strafe.points = course.points[start..start + 3].to_vec();
    let mut app = game();
    let player = press(&mut app, "course");
    place_at_start(&mut app, player, &strafe);
    let report = drive(&mut app, player, &strafe);
    run_until_done(&mut app, &report, 6.0);
    let s = state(&app, player);
    assert!(report.borrow().finished_at.is_some() && s.origin.y > 2.9, "the strafe-jump lands on the far side: {}", s.origin);
}
