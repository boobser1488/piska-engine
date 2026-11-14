//! Правила и поток игры: меню → выбор цеха → отсчёт → заезд → результаты. Меню и каждый
//! цех — отдельные сцены (`assets/scenes/menu.scene`, `assets/scenes/courses/course_N.scene`);
//! менеджер игры (`GameManager` на объекте «Game» каждой сцены) принимает клики интерфейса,
//! сообщения механизмов («checkpoint», «gear», «finish»…) и вызывает функции отсюда.

use engine::prelude::*;

use super::data::{self, COUNT};
use super::sounds;
use super::state::{medal_for, Action, GameState, RunResult, Screen, COUNTDOWN};
use super::ui::{self, UiRefs};
use super::util;
use super::view::{BallStatus, ViewState, BALL_RADIUS};
use crate::scripts::ball::Ball;
use crate::scripts::checkpoint::Checkpoint;
use crate::scripts::gear_pickup::GearPickup;

// ----------------------------------------------------------------------
// Вход в сцену
// ----------------------------------------------------------------------

/// Менеджер игры стартовал в новой сцене: `course` — номер цеха этой сцены (из поля
/// компонента), `-1` — главное меню.
pub fn enter_scene(ctx: &mut Ctx, st: &mut GameState, course: i32, start_yaw_deg: f32) {
    sounds::load(ctx.world);
    if st.courses.is_empty() {
        st.courses = data::load_courses(ctx.world);
    }
    st.ui = UiRefs::resolve(&ctx.world.scene);
    st.loading = false;
    st.fade = 1.0;
    st.fade_target = 0.0;
    st.toast = 0.0;
    if st.boot.demo_progress && !st.boot_done {
        st.progress.unlocked = 6;
        st.progress.best = [48.2, 61.0, 77.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        st.progress.medal = [3, 3, 1, 0, 0, 0, 0, 0];
        st.progress.gears = [9, 8, 5, 0, 0, 0, 0, 0];
        st.progress.golden = [true, false, true, false, false, false, false, false];
        st.progress.done = [true, true, true, false, false, false, false, false];
    }
    apply_options(ctx, st);
    refresh_options_ui(ctx, st);

    if course < 0 {
        enter_menu(ctx, st);
    } else {
        enter_course(ctx, st, course as usize, start_yaw_deg);
    }
}

fn enter_menu(ctx: &mut Ctx, st: &mut GameState) {
    st.in_course = false;
    st.ball = None;
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.ball = None;
        view.snap = true;
    }
    // Скриншоты и ручная проверка: сразу открыть нужный экран или цех.
    if let (Some(screen), false) = (st.boot.screen.clone(), st.boot_done) {
        let course_boot = match screen.as_str() {
            "pause" | "results" | "lose" => Some(0),
            s if s.starts_with('c') => s[1..].parse::<usize>().ok().map(|n| n.clamp(1, COUNT) - 1),
            _ => None,
        };
        if let Some(index) = course_boot {
            st.progress.unlocked = COUNT;
            load_course(ctx, st, index);
            return;
        }
        match screen.as_str() {
            "select" => st.open_select = true,
            "options" => {
                set_screen(ctx, st, Screen::Options);
                st.boot_done = true;
                return;
            }
            _ => {}
        }
        st.boot_done = true;
    }
    if st.open_select {
        st.open_select = false;
        set_screen(ctx, st, Screen::Select);
        refresh_cards(ctx, st);
    } else {
        set_screen(ctx, st, Screen::Menu);
        refresh_menu(ctx, st);
    }
}

fn enter_course(ctx: &mut Ctx, st: &mut GameState, index: usize, start_yaw_deg: f32) {
    st.course = index.min(COUNT - 1);
    st.in_course = true;
    st.clock = 0.0;
    st.countdown = COUNTDOWN;
    st.last_count = -1;
    st.gears = 0;
    st.golden = false;
    st.falls = 0;
    st.checkpoint_idx = 0;
    st.pending_checkpoint = (0, 0.0);
    st.result = None;
    st.hint_time = 0.0;
    st.route = data::load_route(ctx.world, st.course);

    // Что в цехе: шар, шестерни, ворота, динамические тела — считаем по самой сцене.
    let scene = &ctx.world.scene;
    st.ball = scene.find_by_tag("Ball");
    st.gears_total = scene.iter_with::<GearPickup>().filter(|(_, g)| !g.golden).count() as u32;
    st.golden_exists = scene.iter_with::<GearPickup>().any(|(_, g)| g.golden);
    st.checkpoints_total = scene.iter_with::<Checkpoint>().count() as u32;
    st.bodies = scene
        .iter_with::<Rigidbody>()
        .filter(|(id, body)| body.body_type == BodyType::Dynamic && !util::is_ball(scene, *id))
        .count() as u32;
    st.checkpoint_pos = st.ball.map_or(Vec3::ZERO, |b| scene.world_position(b));
    st.checkpoint_yaw = start_yaw_deg.to_radians();

    // Камера смотрит вдоль цеха и следует за шаром.
    let (yaw, ball) = (st.checkpoint_yaw, st.ball);
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.yaw = yaw;
        view.pitch = 0.6;
        view.ball = ball;
        view.snap = true;
        view.trauma = 0.0;
    }
    if let Some(status) = ctx.resource_mut::<BallStatus>() {
        *status = BallStatus::default();
    }
    ui::set_text(&mut ctx.world.scene, st.ui.hud_count, "");
    ui::set_text(&mut ctx.world.scene, st.ui.hud_toast, "");
    set_screen(ctx, st, Screen::Countdown);

    // Скриншоты и ручная проверка: отсчёт пропускаем, шар ставим на долю маршрута, открываем экран.
    if let (Some(screen), false) = (st.boot.screen.clone(), st.boot_done) {
        st.boot_done = true;
        st.fade = 0.0;
        skip_countdown(ctx, st);
        match screen.as_str() {
            "pause" => apply(ctx, st, Action::Pause),
            "results" => {
                st.clock = 47.3;
                st.gears = 7;
                finish_course(ctx, st);
            }
            "lose" => {
                st.falls = 2;
                lose_course(ctx, st);
            }
            _ => {
                let at = st.boot.at;
                if at > 0.0 {
                    place_along_route(ctx, st, at);
                }
            }
        }
    }
}

/// Поставить шар на долю маршрута (скриншоты).
fn place_along_route(ctx: &mut Ctx, st: &mut GameState, fraction: f32) {
    let Some(ball) = st.ball else { return };
    if st.route.is_empty() {
        return;
    }
    let index = ((st.route.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    let pos = st.route[index].position() + Vec3::Y * (BALL_RADIUS + 0.15);
    if let Some(object) = ctx.world.scene.get_mut(ball) {
        object.transform.position = pos;
    }
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.snap = true;
    }
}

// ----------------------------------------------------------------------
// Кадр
// ----------------------------------------------------------------------

pub fn update(ctx: &mut Ctx, st: &mut GameState, dt: f32) {
    // Действия: клики интерфейса, клавиши, очередь из тестов.
    let mut actions = std::mem::take(&mut st.queue);
    collect_input_actions(ctx, st, &mut actions);
    for action in actions {
        apply(ctx, st, action);
    }
    if st.loading {
        return;
    }

    // Сдвиги ползунков настроек.
    if st.screen == Screen::Options {
        slider_events(ctx, st);
    }

    match st.screen {
        Screen::Countdown => {
            st.countdown -= dt;
            let shown = st.countdown.ceil() as i32;
            if shown != st.last_count && shown >= 1 && st.countdown > 0.0 && st.countdown <= 3.0 {
                st.last_count = shown;
                play_ui(ctx, "beep");
                ui::set_text(&mut ctx.world.scene, st.ui.hud_count, &shown.to_string());
            }
            if st.countdown <= 0.0 {
                go(ctx, st);
            }
        }
        Screen::Playing => {
            st.clock += dt;
            st.hint_time += dt;
            if st.clock >= st.info().limit {
                lose_course(ctx, st);
            }
        }
        _ => {}
    }

    // Затемнение переходов.
    let real = ctx.time().unscaled_delta;
    let step = 3.2 * dt.max(real);
    st.fade += (st.fade_target - st.fade).clamp(-step, step);
    if let Some(fade) = st.ui.fade {
        let alpha = st.fade.clamp(0.0, 1.0);
        let scene = &mut ctx.world.scene;
        let image = scene.get(fade).and_then(|o| o.children().first().copied());
        ui::set_image_color(scene, image, Color::rgba(0.0, 0.0, 0.0, alpha));
    }

    // Подсказка и тост живут по реальному времени, чтобы не зависать на паузе.
    st.toast = (st.toast - real).max(0.0);
    refresh_hud(ctx, st);
    if st.save_dirty {
        flush_save(ctx, st);
    }
}

/// Клики по кнопкам и клавиши, переведённые в действия.
fn collect_input_actions(ctx: &mut Ctx, st: &GameState, out: &mut Vec<Action>) {
    let ui = ctx.ui();
    let buttons = [
        ("menu_play", Action::Play),
        ("menu_select", Action::SelectScreen),
        ("menu_options", Action::OptionsScreen),
        ("menu_quit", Action::Quit),
        ("select_back", Action::Back),
        ("options_back", Action::Back),
        ("pause_resume", Action::Resume),
        ("pause_restart", Action::Restart),
        ("pause_select", Action::ToSelect),
        ("pause_menu", Action::ToMenu),
        ("res_next", Action::Next),
        ("res_retry", Action::Restart),
        ("res_menu", Action::ToMenu),
    ];
    // Кнопка скрытого экрана не срабатывает: фокус геймпада может стоять на ней, а «прыжок» — та же кнопка South.
    let live = |name: &str| {
        let screen = if name.starts_with("menu_") {
            Screen::Menu
        } else if name.starts_with("select_") || name.starts_with("course_") {
            Screen::Select
        } else if name.starts_with("options_") {
            Screen::Options
        } else if name.starts_with("pause_") {
            Screen::Paused
        } else {
            Screen::Results
        };
        st.screen == screen
    };
    out.extend(buttons.iter().filter(|(name, _)| live(name) && ui.clicked(name)).map(|(_, a)| *a));
    for i in 0..COUNT {
        if live("course_") && ui.clicked(&format!("course_{i}")) {
            out.push(Action::Course(i));
        }
    }

    // Клавиши и геймпад.
    let input = ctx.input();
    let pause = input.button_down("Pause");
    let cancel = input.button_down("Cancel");
    let reset = input.key_down(Key::R) || input.any_gamepad_button_down(GamepadButton::North);
    match st.screen {
        Screen::Playing | Screen::Countdown if pause => out.push(Action::Pause),
        Screen::Playing if reset => out.push(Action::Reset),
        Screen::Paused if pause || cancel => out.push(Action::Resume),
        Screen::Select | Screen::Options if cancel => out.push(Action::Back),
        _ => {}
    }
}

pub fn handle_message(ctx: &mut Ctx, st: &mut GameState, message: &Message) {
    let position = |v: &Value| match v {
        Value::Vec3(x, y, z) => Some(Vec3::new(*x, *y, *z)),
        _ => None,
    };
    match message.name.as_str() {
        "checkpoint_info" => {
            if let Value::Vec2(index, yaw) = &message.value {
                st.pending_checkpoint = (index.round() as i32, *yaw);
            }
        }
        "checkpoint" => {
            if !st.in_course || st.screen != Screen::Playing {
                return;
            }
            let (index, yaw) = st.pending_checkpoint;
            if let (Some(pos), true) = (position(&message.value), index > st.checkpoint_idx) {
                st.checkpoint_idx = index;
                st.checkpoint_pos = pos;
                st.checkpoint_yaw = yaw.to_radians();
                toast(ctx, st, &format!("Контрольная точка {} из {}", index, st.checkpoints_total), 1.8);
            }
        }
        "gear" | "golden_gear" => {
            if !st.in_course || st.screen != Screen::Playing {
                return;
            }
            let golden = message.name == "golden_gear";
            if golden {
                st.golden = true;
            } else {
                st.gears += 1;
            }
            let pos = position(&message.value).unwrap_or(Vec3::ZERO);
            util::effect(ctx.world, if golden { "sparkle_big" } else { "sparkle" }, pos);
            if golden {
                play(ctx, "golden", 1.0, 1.0);
                toast(ctx, st, "Золотая шестерня!", 2.4);
            } else {
                // Каждая следующая шестерня звенит чуть выше.
                let pitch = 0.9 + (st.gears as f32 * 0.04).min(0.6);
                play(ctx, "gear", 0.8, pitch);
            }
        }
        "finish" => {
            if st.screen == Screen::Playing {
                finish_course(ctx, st);
            }
        }
        "fell" => {
            if st.screen == Screen::Playing {
                respawn(ctx, st, "fell", "Упал! Назад на контрольную точку");
            }
        }
        "squashed" => {
            if st.screen == Screen::Playing {
                respawn(ctx, st, "thud", "Раздавило!");
            }
        }
        "shake" => {
            if let Value::Float(amount) = &message.value {
                if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
                    view.trauma = view.trauma.max(amount.clamp(0.0, 1.0));
                }
            }
        }
        "hide_count" if st.screen == Screen::Playing => {
            ui::set_text(&mut ctx.world.scene, st.ui.hud_count, "");
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------
// Действия и экраны
// ----------------------------------------------------------------------

/// Выполнить действие игрока.
pub fn apply(ctx: &mut Ctx, st: &mut GameState, action: Action) {
    if st.loading {
        return;
    }
    let screen = st.screen;
    match action {
        Action::Play if screen == Screen::Menu => {
            click_sound(ctx);
            let index = st.progress.continue_index();
            load_course(ctx, st, index);
        }
        Action::SelectScreen if screen == Screen::Menu => {
            click_sound(ctx);
            set_screen(ctx, st, Screen::Select);
            refresh_cards(ctx, st);
        }
        Action::SelectScreen | Action::ToSelect if matches!(screen, Screen::Paused | Screen::Results) => {
            click_sound(ctx);
            st.open_select = true;
            load_menu(ctx, st);
        }
        Action::OptionsScreen if screen == Screen::Menu => {
            click_sound(ctx);
            set_screen(ctx, st, Screen::Options);
            refresh_options_ui(ctx, st);
        }
        Action::Quit if screen == Screen::Menu => {
            click_sound(ctx);
            flush_save(ctx, st);
            ctx.quit();
        }
        Action::Course(i) if screen == Screen::Select => {
            if i < st.progress.unlocked {
                click_sound(ctx);
                load_course(ctx, st, i);
            } else {
                play_ui(ctx, "ui_error");
            }
        }
        Action::Back if screen == Screen::Select || screen == Screen::Options => {
            click_sound(ctx);
            flush_save(ctx, st);
            set_screen(ctx, st, Screen::Menu);
            refresh_menu(ctx, st);
        }
        Action::Pause if screen == Screen::Playing || screen == Screen::Countdown => {
            click_sound(ctx);
            set_screen(ctx, st, Screen::Paused);
        }
        Action::Resume if screen == Screen::Paused => {
            click_sound(ctx);
            let next = if st.countdown > 0.0 && st.clock == 0.0 { Screen::Countdown } else { Screen::Playing };
            set_screen(ctx, st, next);
        }
        Action::Restart if matches!(screen, Screen::Paused | Screen::Results) => {
            click_sound(ctx);
            let index = st.course;
            load_course(ctx, st, index);
        }
        Action::ToMenu if matches!(screen, Screen::Paused | Screen::Results) => {
            click_sound(ctx);
            load_menu(ctx, st);
        }
        Action::Next if screen == Screen::Results => {
            click_sound(ctx);
            let win = st.result.is_some_and(|r| r.win);
            if win && st.course + 1 < COUNT {
                let next = st.course + 1;
                load_course(ctx, st, next);
            } else {
                load_menu(ctx, st);
            }
        }
        Action::Reset if screen == Screen::Playing => {
            respawn(ctx, st, "respawn", "");
        }
        _ => {}
    }
}

/// Загрузить сцену цеха `index`; менеджер новой сцены начнёт отсчёт.
fn load_course(ctx: &mut Ctx, st: &mut GameState, index: usize) {
    flush_save(ctx, st);
    st.loading = true;
    ctx.load_scene(&format!("courses/course_{}", index.min(COUNT - 1) + 1));
}

/// Загрузить сцену главного меню.
fn load_menu(ctx: &mut Ctx, st: &mut GameState) {
    flush_save(ctx, st);
    st.loading = true;
    // Время идёт снова, звук паузы снят: новая сцена начинается «с нуля».
    ctx.set_time_scale(1.0);
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        if audio.is_paused() {
            audio.resume_all();
        }
    }
    ctx.load_scene("menu");
}

fn click_sound(ctx: &mut Ctx) {
    play_ui(ctx, "ui_click");
}

fn play_ui(ctx: &mut Ctx, clip: &str) {
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_ui(clip);
    }
}

fn play(ctx: &mut Ctx, clip: &str, volume: f32, pitch: f32) {
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_with(clip, PlayParams::new().volume(volume).pitch(pitch));
    }
}

fn toast(ctx: &mut Ctx, st: &mut GameState, message: &str, seconds: f32) {
    st.toast = seconds;
    ui::set_text(&mut ctx.world.scene, st.ui.hud_toast, message);
}

/// Показать нужные холсты и настроить время, звук и шар под экран.
fn set_screen(ctx: &mut Ctx, st: &mut GameState, screen: Screen) {
    st.screen = screen;
    let refs = st.ui.clone();
    let scene = &mut ctx.world.scene;
    ui::set_visible(scene, refs.menu, screen == Screen::Menu);
    ui::set_visible(scene, refs.select, screen == Screen::Select);
    ui::set_visible(scene, refs.options, screen == Screen::Options);
    ui::set_visible(scene, refs.hud, matches!(screen, Screen::Countdown | Screen::Playing | Screen::Paused));
    ui::set_visible(scene, refs.pause, screen == Screen::Paused);
    ui::set_visible(scene, refs.results, screen == Screen::Results);
    ctx.set_time_scale(if screen == Screen::Paused { 0.0 } else { 1.0 });
    let frozen = !matches!(screen, Screen::Playing);
    if let Some(ball) = st.ball {
        if let Some(b) = ctx.world.scene.get_component_mut::<Ball>(ball) {
            b.frozen = frozen;
        }
    }
    // Музыка по экрану: меню — одна, цеха — другая.
    let music = match screen {
        Screen::Menu | Screen::Select | Screen::Options => sounds::MUSIC_MENU,
        _ if st.course < 4 => sounds::MUSIC_FACTORY_A,
        _ => sounds::MUSIC_FACTORY_B,
    };
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_music(music, 0.9);
        if screen == Screen::Paused {
            audio.pause_all();
        } else if audio.is_paused() {
            audio.resume_all();
        }
    }
}

/// Закончить отсчёт: шар свободен.
fn go(ctx: &mut Ctx, st: &mut GameState) {
    st.countdown = 0.0;
    play(ctx, "go", 1.0, 1.0);
    ui::set_text(&mut ctx.world.scene, st.ui.hud_count, "Поехали!");
    set_screen(ctx, st, Screen::Playing);
    ctx.after(0.9, "hide_count", Value::Nil);
}

/// Пропустить отсчёт (скриншоты, тесты).
pub fn skip_countdown(ctx: &mut Ctx, st: &mut GameState) {
    if st.screen == Screen::Countdown {
        st.countdown = 0.0;
        set_screen(ctx, st, Screen::Playing);
        ui::set_text(&mut ctx.world.scene, st.ui.hud_count, "");
    }
}

/// Вернуть шар на последнюю контрольную точку.
fn respawn(ctx: &mut Ctx, st: &mut GameState, sound: &str, message: &str) {
    let Some(ball) = st.ball else { return };
    let pos = st.checkpoint_pos + Vec3::Y * (BALL_RADIUS + 0.35);
    if let Some(object) = ctx.world.scene.get_mut(ball) {
        object.transform.position = pos;
        object.transform.rotation = Quat::IDENTITY;
        if let Some(body) = object.get_mut::<Rigidbody>() {
            body.velocity = Vec3::ZERO;
            body.angular_velocity = Vec3::ZERO;
        }
        if let Some(b) = object.get_mut::<Ball>() {
            b.fell = false;
        }
    }
    if sound != "respawn" {
        st.falls += 1;
    }
    let yaw = st.checkpoint_yaw;
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.yaw = yaw;
        view.snap = true;
    }
    play(ctx, sound, 0.9, 1.0);
    util::effect(ctx.world, "sparkle", pos);
    if !message.is_empty() {
        toast(ctx, st, message, 2.0);
    }
    st.fade = 0.55;
    st.fade_target = 0.0;
}

/// Цех пройден: медаль, рекорд, открыть следующий, показать результаты.
pub fn finish_course(ctx: &mut Ctx, st: &mut GameState) {
    if st.screen != Screen::Playing {
        return;
    }
    let i = st.course;
    let time = st.clock;
    let medal = medal_for(st.info(), time);
    let (gears, golden) = (st.gears, st.golden);
    let p = &mut st.progress;
    let new_best = p.best[i] <= 0.0 || time < p.best[i];
    if new_best {
        p.best[i] = time;
    }
    p.medal[i] = p.medal[i].max(medal);
    p.gears[i] = p.gears[i].max(gears);
    p.golden[i] |= golden;
    p.done[i] = true;
    if i + 1 < COUNT {
        p.unlocked = p.unlocked.max(i + 2).min(COUNT);
    }
    st.result = Some(RunResult {
        win: true,
        time,
        medal,
        new_best,
        gears: st.gears,
        gears_total: st.gears_total,
        golden: st.golden,
        golden_exists: st.golden_exists,
        falls: st.falls,
    });
    st.save_dirty = true;
    if let Some(ball) = st.ball {
        if let Some(rb) = ctx.world.scene.get_component_mut::<Rigidbody>(ball) {
            rb.velocity *= 0.3;
        }
    }
    play(ctx, "win", 1.0, 1.0);
    let pos = st.ball.map(|b| ctx.world.scene.world_position(b)).unwrap_or(Vec3::ZERO);
    util::effect(ctx.world, "explosion", pos + Vec3::Y);
    util::effect(ctx.world, "sparkle_big", pos + Vec3::Y * 0.5);
    set_screen(ctx, st, Screen::Results);
    fill_results(ctx, st);
}

/// Смена закончилась: время вышло.
pub fn lose_course(ctx: &mut Ctx, st: &mut GameState) {
    if st.screen != Screen::Playing {
        return;
    }
    st.result = Some(RunResult {
        win: false,
        time: st.clock,
        medal: 0,
        new_best: false,
        gears: st.gears,
        gears_total: st.gears_total,
        golden: st.golden,
        golden_exists: st.golden_exists,
        falls: st.falls,
    });
    play(ctx, "lose", 1.0, 1.0);
    set_screen(ctx, st, Screen::Results);
    fill_results(ctx, st);
}

// ----------------------------------------------------------------------
// Тексты интерфейса
// ----------------------------------------------------------------------

fn refresh_menu(ctx: &mut Ctx, st: &GameState) {
    let label = if st.progress.started() {
        format!("Продолжить: цех {}", st.progress.continue_index() + 1)
    } else {
        "Начать игру".to_string()
    };
    ui::set_text(&mut ctx.world.scene, st.ui.menu_play_label, &label);
}

fn refresh_cards(ctx: &mut Ctx, st: &GameState) {
    let scene = &mut ctx.world.scene;
    for (i, card) in st.ui.cards.iter().enumerate() {
        let open = i < st.progress.unlocked;
        let p = &st.progress;
        ui::set_text(scene, card.title, &format!("{}. {}", i + 1, st.courses[i].name));
        if let Some(button) = scene.get_component_mut::<Button>(card.button) {
            button.interactable = open;
        }
        let (time, gears) = if !open {
            ("Закрыт".to_string(), String::new())
        } else if p.done[i] {
            (ui::fmt_time(p.best[i]), format!("{}{}", p.gears[i], if p.golden[i] { " + золотая" } else { "" }))
        } else {
            ("не пройден".to_string(), String::new())
        };
        ui::set_text(scene, card.time, &time);
        ui::set_text(scene, card.gears, &gears);
        let medal = if open && p.done[i] { p.medal[i] } else { 0 };
        ui::set_image_color(scene, card.medal, Color::hex(ui::medal_color(medal)));
        ui::set_text_color(scene, card.title, if open { ui::INK } else { ui::MUTED });
    }
}

fn refresh_options_ui(ctx: &mut Ctx, st: &GameState) {
    let o = st.options;
    let scene = &mut ctx.world.scene;
    for (slider, value) in [(st.ui.opt_music, o.music), (st.ui.opt_sfx, o.sfx), (st.ui.opt_sens, o.sensitivity)] {
        if let Some(s) = slider.and_then(|id| scene.get_component_mut::<Slider>(id)) {
            s.value = value;
        }
    }
    ui::set_text(scene, st.ui.opt_music_text, &format!("Музыка: {}%", (o.music * 100.0).round() as i32));
    ui::set_text(scene, st.ui.opt_sfx_text, &format!("Звуки: {}%", (o.sfx * 100.0).round() as i32));
    ui::set_text(scene, st.ui.opt_sens_text, &format!("Чувствительность камеры: {}%", (o.sensitivity * 100.0).round() as i32));
}

/// Применить настройки к шинам звука и камере.
pub fn apply_options(ctx: &mut Ctx, st: &GameState) {
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.set_bus_volume(Bus::Music, st.options.music);
        audio.set_bus_volume(Bus::Sfx, st.options.sfx);
        audio.set_bus_volume(Bus::Ui, st.options.sfx);
    }
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.sensitivity = st.options.sensitivity;
    }
}

fn slider_events(ctx: &mut Ctx, st: &mut GameState) {
    let mut changed = false;
    let ui = ctx.ui();
    if let Some(v) = ui.value_changed("opt_music") {
        st.options.music = v.clamp(0.0, 1.0);
        changed = true;
    }
    if let Some(v) = ui.value_changed("opt_sfx") {
        st.options.sfx = v.clamp(0.0, 1.0);
        changed = true;
    }
    if let Some(v) = ui.value_changed("opt_sens") {
        st.options.sensitivity = v.clamp(0.3, 2.0);
        changed = true;
    }
    if changed {
        apply_options(ctx, st);
        refresh_options_ui(ctx, st);
        let options = st.options;
        options.store(ctx.save_data());
        st.save_dirty = true;
    }
}

fn fill_results(ctx: &mut Ctx, st: &GameState) {
    let Some(r) = st.result else { return };
    let info = st.info().clone();
    let best = st.progress.best[st.course];
    let last = st.course + 1 >= COUNT;
    let refs = &st.ui;
    let next_label = refs.res_next_label(&ctx.world.scene);
    let scene = &mut ctx.world.scene;
    if r.win {
        ui::set_text(scene, refs.res_title, if last { "Завод запущен!" } else { "Цех пройден!" });
        ui::set_text_color(scene, refs.res_title, ui::GREEN);
    } else {
        ui::set_text(scene, refs.res_title, "Смена окончена");
        ui::set_text_color(scene, refs.res_title, ui::RED);
    }
    ui::set_text(scene, refs.res_course, &format!("Цех {} · {}", st.course + 1, info.name));
    ui::set_image_color(scene, refs.res_medal, Color::hex(ui::medal_color(r.medal)));
    ui::set_text(scene, refs.res_medal_text, if r.win { ui::medal_name(r.medal) } else { "Время вышло" });
    ui::set_text_color(scene, refs.res_medal_text, if r.win { ui::medal_color(r.medal) } else { ui::RED });
    ui::set_text(scene, refs.res_time, &format!("Время: {}", ui::fmt_time(r.time)));
    let best_line = if !r.win {
        format!("Лимит смены: {}", ui::fmt_time(info.limit))
    } else if r.new_best {
        "Новый рекорд!".to_string()
    } else {
        format!("Рекорд: {}", ui::fmt_time(best))
    };
    ui::set_text(scene, refs.res_best, &best_line);
    ui::set_text_color(scene, refs.res_best, if r.win && r.new_best { ui::AMBER } else { ui::MUTED });
    ui::set_text(scene, refs.res_gears, &format!("Шестерни: {} из {}", r.gears, r.gears_total));
    let golden = if !r.golden_exists { "" } else if r.golden { "Золотая шестерня: найдена" } else { "Золотая шестерня: не найдена" };
    ui::set_text(scene, refs.res_golden, golden);
    ui::set_text_color(scene, refs.res_golden, if r.golden { ui::GOLD } else { ui::MUTED });
    ui::set_text(scene, refs.res_falls, &format!("Падений: {}", r.falls));
    let label = if r.win && !last { "Дальше" } else if r.win { "В меню" } else { "Заново" };
    ui::set_text(scene, next_label, label);
}

fn refresh_hud(ctx: &mut Ctx, st: &GameState) {
    if !matches!(st.screen, Screen::Countdown | Screen::Playing | Screen::Paused) || st.ui.hud.is_none() {
        return;
    }
    let info = st.info();
    let speed = ctx.resource::<BallStatus>().map_or(0.0, |s| s.speed);
    let toast_visible = st.toast > 0.0;
    let hint_visible = st.course == 0 && st.hint_time < 14.0 && st.screen != Screen::Paused;
    let refs = &st.ui;
    let scene = &mut ctx.world.scene;
    ui::set_text(scene, refs.hud_course, &format!("Цех {} · {}", st.course + 1, info.name));
    ui::set_text(scene, refs.hud_gears, &format!("Шестерни {}/{}", st.gears, st.gears_total));
    let golden = if !st.golden_exists { "" } else if st.golden { "Золотая шестерня: найдена" } else { "Золотая шестерня: ищется" };
    ui::set_text(scene, refs.hud_golden, golden);
    ui::set_text_color(scene, refs.hud_golden, if st.golden { ui::GOLD } else { ui::MUTED });
    ui::set_text(scene, refs.hud_time, &ui::fmt_time(st.clock));
    // Ближайшая недостигнутая медаль.
    let target = if st.clock <= info.gold {
        Some(("Золото", info.gold, ui::GOLD))
    } else if st.clock <= info.silver {
        Some(("Серебро", info.silver, ui::SILVER))
    } else if st.clock <= info.bronze {
        Some(("Бронза", info.bronze, ui::BRONZE))
    } else {
        None
    };
    match target {
        Some((name, limit, color)) => {
            ui::set_text(scene, refs.hud_target, &format!("{name} до {}", ui::fmt_time(limit)));
            ui::set_text_color(scene, refs.hud_target, color);
        }
        None => {
            ui::set_text(scene, refs.hud_target, &format!("Лимит смены {}", ui::fmt_time(info.limit)));
            ui::set_text_color(scene, refs.hud_target, ui::MUTED);
        }
    }
    ui::set_text(scene, refs.hud_checkpoint, &format!("Контрольная точка {}/{}", st.checkpoint_idx, st.checkpoints_total));
    if let Some(bar) = refs.hud_speed.and_then(|id| scene.get_component_mut::<ProgressBar>(id)) {
        bar.value = (speed / 16.0).clamp(0.0, 1.0);
    }
    if !toast_visible {
        ui::set_text(scene, refs.hud_toast, "");
    }
    ui::set_visible(scene, refs.hud_hint, hint_visible);
}

pub fn flush_save(ctx: &mut Ctx, st: &mut GameState) {
    st.save_dirty = false;
    let progress = st.progress.clone();
    let options = st.options;
    let save = ctx.save_data();
    progress.store(save);
    options.store(save);
    if let Err(err) = save.save() {
        log::warn!("progress is not saved: {err}");
    }
}
