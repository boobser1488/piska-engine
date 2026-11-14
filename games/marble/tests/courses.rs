//! Каждый цех — сцена из ассетов: непустая и содержит всё, что нужно для игры.

mod common;

use common::*;
use marble::COUNT;

#[test]
fn every_course_scene_loads_and_is_playable() {
    let mut app = game();
    for index in 0..COUNT {
        start_course(&mut app, index);
        let st = state(&app);
        let info = st.courses[index].clone();
        let name = info.name.clone();

        assert!(app.world.scene.len() > 150, "{name}: the scene is full of objects, got {}", app.world.scene.len());
        assert!(st.gears_total >= 8, "{name}: at least 8 gears, got {}", st.gears_total);
        assert!(st.golden_exists, "{name}: one golden gear");
        assert!(st.checkpoints_total >= 2, "{name}: checkpoints");
        assert!(st.route.len() >= 8, "{name}: a route for the bot");
        assert!(app.world.scene.find_by_name("Finish").is_some(), "{name}: shipping pad");
        assert_eq!(app.world.scene.find_all_by_tag("Ball").len(), 1, "{name}: exactly one ball");

        // Шестерёнки — объекты сцены с компонентом GearPickup.
        let gears = count_named(&app, "Gear") as u32;
        assert_eq!(gears, st.gears_total, "{name}: gear objects match the count");

        // Шар стоит на старте, а не падает.
        app.run_headless(40, DT);
        let pos = ball_pos(&app);
        assert!(pos.y > -1.0 && pos.y < 3.0, "{name}: the ball rests on the start pad, y = {}", pos.y);

        // Пороги медалей идут по возрастанию, лимит смены больше бронзы.
        assert!(info.gold < info.silver && info.silver < info.bronze && info.bronze < info.limit, "{name}: medal times are ordered");

        act(&mut app, marble::Action::Pause);
        act(&mut app, marble::Action::ToMenu);
        run_until(&mut app, marble::Screen::Menu, 20);
    }
}

#[test]
fn courses_get_longer_and_harder() {
    let mut app = game();
    let mut lengths = Vec::new();
    for index in 0..COUNT {
        start_course(&mut app, index);
        let route = &state(&app).route;
        let length: f32 = route.windows(2).map(|p| (p[1].position() - p[0].position()).length()).sum();
        lengths.push(length);
        to_menu(&mut app);
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
        let bodies = state(&app).bodies;
        assert!(bodies < 60, "course {index}: {bodies} dynamic bodies");
        to_menu(&mut app);
    }
}

#[test]
fn the_course_list_comes_from_the_data_file() {
    let app = game();
    let st = state(&app);
    assert_eq!(st.courses.len(), COUNT);
    assert!(st.courses.iter().all(|c| !c.name.is_empty() && !c.blurb.is_empty()));
}
