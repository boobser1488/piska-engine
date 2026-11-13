//! Каждый цех строится, непустой и содержит всё, что нужно для игры.

mod common;

use common::*;
use marble::courses::{COUNT, COURSES};

#[test]
fn every_course_loads_and_is_playable() {
    let mut app = game();
    for (index, info) in COURSES.iter().enumerate().take(COUNT) {
        start_course(&mut app, index);
        let st = state(&app);
        let built = st.built.as_ref().expect("built course");
        let name = info.name;

        assert!(app.world.scene.len() > 150, "{name}: the course is full of geometry, got {} objects", app.world.scene.len());
        assert!(built.gears >= 8, "{name}: at least 8 gears, got {}", built.gears);
        assert!(built.golden, "{name}: one golden gear");
        assert!(built.checkpoints >= 2, "{name}: checkpoints");
        assert!(built.route.len() >= 8, "{name}: a route for the bot");
        assert!(built.kill_y < -10.0, "{name}: kill plane below the course");
        assert!(app.world.scene.find_by_name("Finish").is_some(), "{name}: shipping pad");
        assert_eq!(app.world.scene.find_all_by_tag("Ball").len(), 1, "{name}: exactly one ball");

        // Шестерёнки и ворота — настоящие C++-компоненты с триггерами.
        let gears = count_named(&app, "Gear");
        assert_eq!(gears as u32, built.gears, "{name}: gear objects match the count");

        // Шар стоит на старте, а не падает.
        app.run_headless(40, DT);
        let pos = ball_pos(&app);
        assert!(pos.y > -1.0 && pos.y < 3.0, "{name}: the ball rests on the start pad, y = {}", pos.y);

        // Пороги медалей идут по возрастанию, лимит смены больше бронзы.
        assert!(info.gold < info.silver && info.silver < info.bronze && info.bronze < info.limit, "{name}: medal times are ordered");

        // Возврат в меню убирает цех, и следующий строится заново.
        act(&mut app, marble::Action::Pause);
        act(&mut app, marble::Action::ToMenu);
        assert_eq!(state(&app).screen, marble::Screen::Menu);
        app.run_headless(2, DT);
    }
}

#[test]
fn courses_get_longer_and_harder() {
    let mut app = game();
    let mut lengths = Vec::new();
    for index in 0..COUNT {
        start_course(&mut app, index);
        let route = &state(&app).built.as_ref().unwrap().route;
        let mut length = 0.0;
        for pair in route.windows(2) {
            length += (pair[1].pos - pair[0].pos).length();
        }
        lengths.push(length);
        act(&mut app, marble::Action::Pause);
        act(&mut app, marble::Action::ToMenu);
    }
    // Каждый цех — по меньшей мере сотня метров пути; финал самый длинный из всех.
    assert!(lengths.iter().all(|l| *l > 120.0), "{lengths:?}");
    let last = *lengths.last().unwrap();
    assert!(lengths.iter().all(|l| *l <= last + 1.0), "the finale is the longest course: {lengths:?}");
}

#[test]
fn rigid_body_budget_stays_small() {
    // Динамические тела в цехе — ящики и качели; их число небольшое (производительность).
    let mut app = game();
    for index in 0..COUNT {
        start_course(&mut app, index);
        let bodies = state(&app).built.as_ref().unwrap().bodies;
        assert!(bodies < 60, "course {index}: {bodies} dynamic bodies");
        act(&mut app, marble::Action::Pause);
        act(&mut app, marble::Action::ToMenu);
    }
}
