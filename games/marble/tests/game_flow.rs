//! Поток игры: меню, пауза, падение, финиш и медали, поражение по времени, сохранения.

mod common;

use common::*;
use engine::prelude::*;
use marble::{Action, Progress, Screen};

/// Нажать и отпустить клавишу (по кадру на каждое).
fn tap(app: &mut App, key: Key) {
    app.world.input.set_key(key, true);
    app.run_headless(1, DT);
    app.world.input.set_key(key, false);
    app.run_headless(1, DT);
}

/// Имя кнопки в фокусе.
fn focused_name(app: &App) -> String {
    let id = app.world.ui().focused();
    id.and_then(|id| app.world.scene.get(id)).map(|o| o.name.clone()).unwrap_or_default()
}

/// Клавишей «вниз» довести фокус до кнопки с таким именем.
fn focus_button(app: &mut App, name: &str) {
    for _ in 0..16 {
        if focused_name(app) == name {
            return;
        }
        tap(app, Key::Down);
    }
    panic!("the keyboard never reached the button `{name}`, stopped at `{}`", focused_name(app));
}

#[test]
fn menu_navigation_reaches_gameplay_with_the_keyboard() {
    let mut app = game();
    app.run_headless(30, DT);
    assert_eq!(state(&app).screen, Screen::Menu);

    // Меню → «Выбор цеха» → первая карточка → отсчёт → заезд.
    focus_button(&mut app, "menu_select");
    tap(&mut app, Key::Enter);
    app.run_headless(2, DT);
    assert_eq!(state(&app).screen, Screen::Select, "Enter on «Выбор цеха» opens the course list");
    focus_button(&mut app, "course_0");
    tap(&mut app, Key::Enter);
    app.run_headless(2, DT);
    assert_eq!(state(&app).screen, Screen::Countdown);
    app.run_headless(150, DT);
    assert_eq!(state(&app).screen, Screen::Playing);
    assert!(state(&app).ball.is_some());
}

#[test]
fn menu_navigation_works_from_the_gamepad() {
    let mut app = game();
    app.run_headless(30, DT);
    let pad = GamepadId(0);
    // Крестовина вниз — следующая кнопка; South — нажать.
    for _ in 0..16 {
        if focused_name(&app) == "menu_select" {
            break;
        }
        app.world.input.set_gamepad_button(pad, GamepadButton::DPadDown, true);
        app.run_headless(1, DT);
        app.world.input.set_gamepad_button(pad, GamepadButton::DPadDown, false);
        app.run_headless(1, DT);
    }
    assert_eq!(focused_name(&app), "menu_select");
    app.world.input.set_gamepad_button(pad, GamepadButton::South, true);
    app.run_headless(1, DT);
    app.world.input.set_gamepad_button(pad, GamepadButton::South, false);
    app.run_headless(2, DT);
    assert_eq!(state(&app).screen, Screen::Select);
    // Назад — кнопка East (Cancel).
    app.world.input.set_gamepad_button(pad, GamepadButton::East, true);
    app.run_headless(1, DT);
    app.world.input.set_gamepad_button(pad, GamepadButton::East, false);
    app.run_headless(2, DT);
    assert_eq!(state(&app).screen, Screen::Menu, "East goes back");
}

#[test]
fn locked_courses_cannot_be_started() {
    let mut app = game();
    app.run_headless(5, DT);
    act(&mut app, Action::SelectScreen);
    assert_eq!(state(&app).progress.unlocked, 1);
    act(&mut app, Action::Course(3));
    assert_eq!(state(&app).screen, Screen::Select, "course 4 is locked at the beginning");
    act(&mut app, Action::Course(0));
    run_until(&mut app, Screen::Countdown, 20);
}

#[test]
fn pause_freezes_the_game_and_resume_continues() {
    let mut app = game();
    start_course(&mut app, 0);
    app.run_headless(60, DT);
    let clock = state(&app).clock;
    assert!(clock > 0.9, "the stopwatch runs: {clock}");

    // Esc — пауза.
    app.world.input.set_key(Key::Escape, true);
    app.run_headless(1, DT);
    app.world.input.set_key(Key::Escape, false);
    app.run_headless(1, DT);
    assert_eq!(state(&app).screen, Screen::Paused);
    assert_eq!(app.world.time.time_scale, 0.0, "the world is frozen");

    // Шар в воздухе: пока пауза, он не падает.
    teleport_ball(&mut app, Vec3::new(0.0, 6.0, -3.0));
    app.run_headless(2, DT);
    let frozen_pos = ball_pos(&app);
    let frozen_clock = state(&app).clock;
    app.run_headless(120, DT);
    assert!((ball_pos(&app) - frozen_pos).length() < 1e-4, "physics is frozen on pause");
    assert_eq!(state(&app).clock, frozen_clock, "the stopwatch is frozen on pause");

    // Esc ещё раз — продолжить.
    tap(&mut app, Key::Escape);
    assert_eq!(state(&app).screen, Screen::Playing);
    app.run_headless(30, DT);
    assert!(state(&app).clock > frozen_clock + 0.4);
    assert!(ball_pos(&app).y < frozen_pos.y - 0.2, "the ball falls again");
}

#[test]
fn pause_menu_restarts_and_leaves() {
    let mut app = game();
    start_course(&mut app, 0);
    app.run_headless(30, DT);
    act(&mut app, Action::Pause);
    act(&mut app, Action::Restart);
    run_until(&mut app, Screen::Countdown, 20);
    assert_eq!(state(&app).clock, 0.0);
    app.run_headless(150, DT);
    act(&mut app, Action::Pause);
    act(&mut app, Action::ToMenu);
    run_until(&mut app, Screen::Menu, 20);
    assert_eq!(app.world.time.time_scale, 1.0, "time runs again in the menu");
    assert!(state(&app).ball.is_none(), "the ball is gone with the course");
}

#[test]
fn falling_below_the_kill_plane_respawns_at_the_last_checkpoint() {
    let mut app = game();
    start_course(&mut app, 0);
    app.run_headless(20, DT);

    // Проезжаем первые ворота: шар в триггере → скрипт шлёт «checkpoint».
    let gate = app
        .world
        .scene
        .ids()
        .into_iter()
        .find(|id| app.world.scene.get(*id).is_some_and(|o| o.name == "Checkpoint"))
        .expect("the course has a checkpoint");
    let gate_pos = app.world.scene.world_position(gate);
    teleport_ball(&mut app, gate_pos + Vec3::new(0.0, 1.0, 0.0));
    app.run_headless(10, DT);
    assert_eq!(state(&app).checkpoint_idx, 1, "the gate was passed");
    let saved = state(&app).checkpoint_pos;
    assert!((saved - gate_pos).length() < 0.5, "the checkpoint remembers the gate position");

    // Улетаем вниз.
    teleport_ball(&mut app, Vec3::new(0.0, -200.0, 0.0));
    app.run_headless(8, DT);
    assert_eq!(state(&app).falls, 1, "the fall was counted");
    let pos = ball_pos(&app);
    assert!(((pos.x - saved.x).powi(2) + (pos.z - saved.z).powi(2)).sqrt() < 0.8, "the ball is back at the checkpoint: {pos:?} vs {saved:?}");
    assert!(pos.y > saved.y && pos.y < saved.y + 2.0);
    assert_eq!(state(&app).screen, Screen::Playing, "the run continues");

    // Второе падение — ещё одно возвращение, счётчик растёт.
    teleport_ball(&mut app, Vec3::new(30.0, -300.0, 30.0));
    app.run_headless(8, DT);
    assert_eq!(state(&app).falls, 2);
}

#[test]
fn a_checkpoint_never_moves_backwards() {
    let mut app = game();
    start_course(&mut app, 0);
    let gates: Vec<GameObjectId> = app.world.scene.ids().into_iter().filter(|id| app.world.scene.get(*id).is_some_and(|o| o.name == "Checkpoint")).collect();
    assert!(gates.len() >= 3);
    // Сначала дальняя, потом ближняя: ближняя не отодвигает точку возврата назад.
    let far = gates[gates.len() - 1];
    let far_pos = app.world.scene.world_position(far);
    teleport_ball(&mut app, far_pos + Vec3::Y);
    app.run_headless(10, DT);
    let index = state(&app).checkpoint_idx;
    assert_eq!(index as usize, gates.len());
    let near_pos = app.world.scene.world_position(gates[0]);
    teleport_ball(&mut app, near_pos + Vec3::Y);
    app.run_headless(10, DT);
    assert_eq!(state(&app).checkpoint_idx, index);
}

#[test]
fn gears_are_collected_counted_and_the_golden_one_is_separate() {
    let mut app = game();
    start_course(&mut app, 0);
    let before = state(&app).gears;
    let gear = find(&app, "Gear");
    let pos = app.world.scene.world_position(gear);
    teleport_ball(&mut app, pos);
    app.run_headless(8, DT);
    assert_eq!(state(&app).gears, before + 1, "touching a gear counts it");
    assert!(!app.world.scene.contains(gear), "the gear disappears");

    let golden = find(&app, "Golden Gear");
    let pos = app.world.scene.world_position(golden);
    teleport_ball(&mut app, pos);
    app.run_headless(8, DT);
    assert!(state(&app).golden, "the golden gear is flagged");
    assert_eq!(state(&app).gears, before + 1, "and is not mixed with the ordinary count");
}

#[test]
fn finishing_records_a_medal_unlocks_the_next_course_and_saves() {
    let backend = MemoryBackend::default();
    let mut app = game_with(SaveData::with_backend(backend.clone()));
    start_course(&mut app, 0);
    let t = state(&app).courses[0].gold;
    state_mut(&mut app).clock = t - 5.0;

    let finish = find(&app, "Finish");
    let pos = app.world.scene.world_position(finish);
    teleport_ball(&mut app, pos + Vec3::new(0.0, 0.8, 0.0));
    app.run_headless(10, DT);

    let st = state(&app);
    assert_eq!(st.screen, Screen::Results);
    let result = st.result.expect("a result");
    assert!(result.win);
    assert_eq!(result.medal, 3, "a time under the gold threshold earns gold");
    assert!(result.new_best);
    assert_eq!(st.progress.medal[0], 3);
    assert_eq!(st.progress.unlocked, 8, "the test opened everything beforehand, unlock never shrinks");
    assert!(st.progress.done[0]);

    // Сохранение пишется на диск (в память теста): рекорд, медаль, открытые цеха.
    app.run_headless(3, DT);
    let saved = backend.contents().expect("the progress was saved");
    assert!(saved.contains("c0.medal") && saved.contains("unlocked"), "{saved}");
}

#[test]
fn silver_and_bronze_follow_the_time_thresholds() {
    let app = game();
    for info in &state(&app).courses {
        assert_eq!(marble::medal_for(info, info.gold - 0.1), 3);
        assert_eq!(marble::medal_for(info, info.gold + 0.1), 2);
        assert_eq!(marble::medal_for(info, info.silver + 0.1), 1);
        assert_eq!(marble::medal_for(info, info.bronze + 0.1), 0);
    }
}

#[test]
fn a_slower_run_keeps_the_better_medal_and_time() {
    let mut app = game();
    start_course(&mut app, 0);
    let t = state(&app).courses[0].gold;
    state_mut(&mut app).clock = t - 1.0;
    let finish = find(&app, "Finish");
    let pos = app.world.scene.world_position(finish);
    teleport_ball(&mut app, pos + Vec3::Y);
    app.run_headless(10, DT);
    let best = state(&app).progress.best[0];
    // Ещё раз, медленнее.
    act(&mut app, Action::Restart);
    app.run_headless(150, DT);
    let t = state(&app).courses[0].bronze;
    state_mut(&mut app).clock = t + 3.0;
    let finish = find(&app, "Finish");
    let pos = app.world.scene.world_position(finish);
    teleport_ball(&mut app, pos + Vec3::Y);
    app.run_headless(10, DT);
    let result = state(&app).result.unwrap();
    assert_eq!(result.medal, 0);
    assert!(!result.new_best);
    assert_eq!(state(&app).progress.best[0], best, "the record time is kept");
    assert_eq!(state(&app).progress.medal[0], 3, "the gold medal is kept");
}

#[test]
fn running_out_of_time_loses_the_shift() {
    let mut app = game();
    start_course(&mut app, 0);
    let t = state(&app).courses[0].limit;
    state_mut(&mut app).clock = t - 0.5;
    app.run_headless(60, DT);
    let st = state(&app);
    assert_eq!(st.screen, Screen::Results);
    let result = st.result.unwrap();
    assert!(!result.win, "time ran out");
    assert_eq!(result.medal, 0);
    assert!(!st.progress.done[0], "a lost shift does not count as completed");
    // Заново — снова отсчёт; дальше идти нельзя.
    act(&mut app, Action::Next);
    run_until(&mut app, Screen::Menu, 20);
    assert_eq!(state(&app).screen, Screen::Menu, "after a loss the main button leaves... to the menu");
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn progress_and_options_round_trip_through_save_data() {
    let backend = MemoryBackend::default();
    let mut save = SaveData::with_backend(backend.clone());
    let mut progress = Progress::default();
    progress.unlocked = 5;
    progress.best[2] = 71.25;
    progress.medal[2] = 2;
    progress.gears[2] = 7;
    progress.golden[2] = true;
    progress.done[2] = true;
    progress.store(&mut save);
    marble::Options { music: 0.25, sfx: 0.5, sensitivity: 1.5 }.store(&mut save);
    save.save().unwrap();

    let reloaded = SaveData::with_backend(backend);
    assert_eq!(Progress::load(&reloaded), progress);
    let options = marble::Options::load(&reloaded);
    assert_eq!((options.music, options.sfx, options.sensitivity), (0.25, 0.5, 1.5));

    // Игра, запущенная поверх этого сохранения, видит прогресс и настройки.
    let app = game_with(reloaded);
    assert_eq!(state(&app).progress.unlocked, 5);
    assert_eq!(state(&app).options.music, 0.25);
    let audio = app.world.resources.get::<Audio>().unwrap();
    assert!((audio.bus_volume(Bus::Music) - 0.25).abs() < 0.05, "the music bus follows the saved volume");
}

#[test]
fn options_sliders_change_volumes_and_are_remembered() {
    let backend = MemoryBackend::default();
    let mut app = game_with(SaveData::with_backend(backend.clone()));
    act(&mut app, Action::OptionsScreen);
    assert_eq!(state(&app).screen, Screen::Options);
    // Двигаем ползунок музыки стрелкой: фокус на нём, ← уменьшает.
    focus_button_like(&mut app, "opt_music");
    for _ in 0..6 {
        tap(&mut app, Key::Left);
    }
    let music = state(&app).options.music;
    assert!(music < 0.5, "the slider lowered the music volume: {music}");
    let audio = app.world.resources.get::<Audio>().unwrap();
    assert!((audio.bus_volume(Bus::Music) - music).abs() < 0.05, "and the Music bus followed");
    act(&mut app, Action::Back);
    app.run_headless(3, DT);
    let saved = backend.contents().expect("options saved on leaving the screen");
    assert!(saved.contains("opt.music"), "{saved}");
}

/// Довести фокус клавишей Tab до виджета с таким именем.
fn focus_button_like(app: &mut App, name: &str) {
    for _ in 0..16 {
        if focused_name(app) == name {
            return;
        }
        tap(app, Key::Tab);
    }
    panic!("Tab never reached `{name}`, stopped at `{}`", focused_name(app));
}

#[test]
fn the_camera_follows_the_ball_and_stays_out_of_walls() {
    let mut app = game();
    start_course(&mut app, 0);
    app.run_headless(60, DT);
    let camera = app.world.scene.find_by_tag("MainCamera").unwrap();
    let cam = app.world.scene.world_position(camera);
    let ball = ball_pos(&app);
    let distance = (cam - ball).length();
    assert!((3.0..14.0).contains(&distance), "the camera hangs behind the ball: {distance}");
    assert!(cam.z > ball.z, "behind: the ball rolls toward -Z");

    // Тяжёлая стена за шаром: луч подтягивает камеру ближе.
    let ball = ball_pos(&app);
    spawn_block(&mut app, ball + Vec3::new(0.0, 3.0, 3.5), Vec3::new(12.0, 8.0, 1.0));
    app.run_headless(40, DT);
    let wall_distance = (app.world.scene.world_position(camera) - ball_pos(&app)).length();
    assert!(wall_distance < distance - 2.0, "pulled in near the wall: {wall_distance} < {distance}");
}

#[test]
fn gravity_is_the_games_own() {
    let app = game();
    let physics = app.world.resources.get::<Physics3D>().unwrap();
    assert_eq!(physics.gravity.y, -marble::GRAVITY);
}

#[test]
fn the_ball_jumps_only_from_the_ground() {
    let mut app = game();
    start_course(&mut app, 0);
    app.run_headless(30, DT);
    let rest = ball_pos(&app).y;
    let pad = GamepadId(0);
    app.world.input.set_gamepad_button(pad, GamepadButton::South, true);
    app.run_headless(1, DT);
    app.world.input.set_gamepad_button(pad, GamepadButton::South, false);
    let mut top = rest;
    for _ in 0..30 {
        app.run_headless(1, DT);
        top = top.max(ball_pos(&app).y);
    }
    assert!(top - rest > 1.2, "a jump lifts the ball by more than a metre: {}", top - rest);
    assert!(top - rest < 2.0, "but not too high: {}", top - rest);

    // Прыжок в воздухе не работает.
    teleport_ball(&mut app, Vec3::new(0.0, 8.0, -3.0));
    app.run_headless(3, DT);
    app.world.input.set_gamepad_button(pad, GamepadButton::South, true);
    app.run_headless(1, DT);
    app.world.input.set_gamepad_button(pad, GamepadButton::South, false);
    let vy = app.world.scene.get_component::<Rigidbody>(ball_id(&app)).unwrap().velocity.y;
    assert!(vy < 0.5, "no double jump in the air, vertical speed {vy}");
}
