//! Правила и поток игры: меню → выбор цеха → отсчёт → заезд → результаты. Менеджер
//! игры (`GameManager` на объекте «Game») принимает клики интерфейса, сообщения механизмов
//! на C++ (`"checkpoint"`, `"gear"`, `"finish"`…) и ведёт всё состояние в ресурсе `GameState`.

use engine::prelude::*;

use crate::ball::{burst, Ball, BallStatus, CameraRig, ViewState, BALL_RADIUS};
use crate::courses::{self, Built, COUNT, COURSES};
use crate::kit::Kit;
use crate::sounds;
use crate::ui::{self, UiIds};

/// Сколько секунд длится отсчёт перед стартом.
pub const COUNTDOWN: f32 = 2.4;

/// Экран игры.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Select,
    Options,
    Countdown,
    Playing,
    Paused,
    Results,
}

/// Действие игрока: клик по кнопке, клавиша или команда из теста.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// «Начать игру» / «Продолжить»: цех, на котором остановились.
    Play,
    SelectScreen,
    OptionsScreen,
    Quit,
    Course(usize),
    /// Из выбора цеха и настроек — в главное меню.
    Back,
    Resume,
    Restart,
    ToMenu,
    ToSelect,
    Next,
    Pause,
    /// Вернуться на контрольную точку.
    Reset,
}

/// Настройки, сохраняются.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub music: f32,
    pub sfx: f32,
    pub sensitivity: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { music: 0.7, sfx: 0.8, sensitivity: 1.0 }
    }
}

/// Лучшие результаты и открытые цехи, сохраняются.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    /// Сколько цехов открыто (минимум 1).
    pub unlocked: usize,
    /// Лучшее время цеха, секунды; 0 — ещё не проходили.
    pub best: [f32; COUNT],
    /// Лучшая медаль: 0 — нет, 1 бронза, 2 серебро, 3 золото.
    pub medal: [u8; COUNT],
    pub gears: [u32; COUNT],
    pub golden: [bool; COUNT],
    /// Пройден ли цех хотя бы раз.
    pub done: [bool; COUNT],
}

impl Default for Progress {
    fn default() -> Self {
        Self { unlocked: 1, best: [0.0; COUNT], medal: [0; COUNT], gears: [0; COUNT], golden: [false; COUNT], done: [false; COUNT] }
    }
}

impl Progress {
    pub fn load(save: &SaveData) -> Self {
        let mut p = Self { unlocked: (save.int("unlocked", 1).clamp(1, COUNT as i64)) as usize, ..Self::default() };
        for i in 0..COUNT {
            p.best[i] = save.float(&format!("c{i}.best"), 0.0);
            p.medal[i] = save.int(&format!("c{i}.medal"), 0).clamp(0, 3) as u8;
            p.gears[i] = save.int(&format!("c{i}.gears"), 0).max(0) as u32;
            p.golden[i] = save.bool(&format!("c{i}.golden"), false);
            p.done[i] = save.bool(&format!("c{i}.done"), false);
        }
        p
    }

    pub fn store(&self, save: &mut SaveData) {
        save.set_int("unlocked", self.unlocked as i64);
        for i in 0..COUNT {
            save.set_float(&format!("c{i}.best"), self.best[i]);
            save.set_int(&format!("c{i}.medal"), self.medal[i] as i64);
            save.set_int(&format!("c{i}.gears"), self.gears[i] as i64);
            save.set_bool(&format!("c{i}.golden"), self.golden[i]);
            save.set_bool(&format!("c{i}.done"), self.done[i]);
        }
    }

    /// Цех, на котором стоит «Продолжить»: первый ещё не пройденный из открытых.
    pub fn continue_index(&self) -> usize {
        (0..self.unlocked.min(COUNT)).find(|&i| !self.done[i]).unwrap_or(self.unlocked.min(COUNT) - 1)
    }

    /// Начинал ли игрок игру вообще.
    pub fn started(&self) -> bool {
        self.done.iter().any(|d| *d) || self.unlocked > 1
    }
}

impl Options {
    pub fn load(save: &SaveData) -> Self {
        let d = Self::default();
        Self {
            music: save.float("opt.music", d.music).clamp(0.0, 1.0),
            sfx: save.float("opt.sfx", d.sfx).clamp(0.0, 1.0),
            sensitivity: save.float("opt.sens", d.sensitivity).clamp(0.3, 2.0),
        }
    }

    pub fn store(&self, save: &mut SaveData) {
        save.set_float("opt.music", self.music);
        save.set_float("opt.sfx", self.sfx);
        save.set_float("opt.sens", self.sensitivity);
    }
}

/// Итог заезда — для экрана результатов.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunResult {
    pub win: bool,
    pub time: f32,
    pub medal: u8,
    pub new_best: bool,
    pub gears: u32,
    pub gears_total: u32,
    pub golden: bool,
    pub golden_exists: bool,
    pub falls: u32,
}

/// Что показать при запуске (для скриншотов; читается из окружения в `lib.rs`).
#[derive(Clone, Debug, Default)]
pub struct Boot {
    /// `menu`, `select`, `options`, `c1`…`c8`, `pause`, `results`, `lose`.
    pub screen: Option<String>,
    /// Доля маршрута (0..1), куда поставить шар для скриншота цеха.
    pub at: f32,
    /// Заполнить прогресс примерными данными (для скриншота выбора цеха).
    pub demo_progress: bool,
}

/// Всё состояние игры.
pub struct GameState {
    pub screen: Screen,
    pub course: usize,
    /// Время заезда, секунды (идёт только в `Playing`).
    pub clock: f32,
    pub countdown: f32,
    pub gears: u32,
    pub gears_total: u32,
    pub golden: bool,
    pub golden_exists: bool,
    pub falls: u32,
    pub checkpoints_total: u32,
    pub checkpoint_idx: i32,
    pub checkpoint_pos: Vec3,
    pub checkpoint_yaw: f32,
    pending_checkpoint: (i32, f32),
    pub result: Option<RunResult>,
    pub progress: Progress,
    pub options: Options,
    pub ui: Option<UiIds>,
    pub course_root: Option<GameObjectId>,
    pub ball: Option<GameObjectId>,
    pub built: Option<Built>,
    /// Заезд идёт «в игре» (а не в витрине меню).
    pub in_course: bool,
    pub toast: f32,
    pub fade: f32,
    fade_target: f32,
    pub hint_time: f32,
    pub queue: Vec<Action>,
    pub boot: Boot,
    last_count: i32,
    save_dirty: bool,
}

impl GameState {
    pub fn new(save: &SaveData, boot: Boot) -> Self {
        Self {
            screen: Screen::Menu,
            course: 0,
            clock: 0.0,
            countdown: 0.0,
            gears: 0,
            gears_total: 0,
            golden: false,
            golden_exists: false,
            falls: 0,
            checkpoints_total: 0,
            checkpoint_idx: 0,
            checkpoint_pos: Vec3::ZERO,
            checkpoint_yaw: 0.0,
            pending_checkpoint: (0, 0.0),
            result: None,
            progress: Progress::load(save),
            options: Options::load(save),
            ui: None,
            course_root: None,
            ball: None,
            built: None,
            in_course: false,
            toast: 0.0,
            fade: 1.0,
            fade_target: 0.0,
            hint_time: 0.0,
            queue: Vec::new(),
            boot,
            last_count: -1,
            save_dirty: false,
        }
    }

    /// Поставить действие в очередь — выполнится в ближайшем кадре (для тестов и скриптов).
    pub fn push(&mut self, action: Action) {
        self.queue.push(action);
    }
}

/// Медаль за время (0 — без медали).
pub fn medal_for(course: usize, time: f32) -> u8 {
    let info = &COURSES[course.min(COUNT - 1)];
    if time <= info.gold {
        3
    } else if time <= info.silver {
        2
    } else if time <= info.bronze {
        1
    } else {
        0
    }
}

component! {
    /// Менеджер игры: интерфейс, правила, переходы между экранами.
    pub struct GameManager {
        pub booted: bool = false,
    }
}

/// Выполнить `f` с состоянием игры, временно вынутым из ресурсов (чтобы свободно
/// пользоваться `ctx`).
fn with_state(ctx: &mut Ctx, f: impl FnOnce(&mut Ctx, &mut GameState)) {
    let Some(mut state) = ctx.world.resources.remove::<GameState>() else {
        log::error!("GameState resource is missing: build the game with marble::build()");
        return;
    };
    f(ctx, &mut state);
    ctx.world.resources.insert(state);
}

impl Component for GameManager {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.booted = true;
        with_state(ctx, boot);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        with_state(ctx, |ctx, st| update(ctx, st, dt));
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        with_state(ctx, |ctx, st| handle_message(ctx, st, message));
    }
}

// ----------------------------------------------------------------------
// Запуск
// ----------------------------------------------------------------------

fn boot(ctx: &mut Ctx, st: &mut GameState) {
    // Ресурс физики появляется вместе с подсистемой, поэтому гравитацию ставим уже в игре.
    if let Some(physics) = ctx.resource_mut::<Physics3D>() {
        physics.gravity = Vec3::new(0.0, -crate::ball::GRAVITY, 0.0);
    }
    let ids = ui::build(ctx.scene_mut());
    st.ui = Some(ids);
    if st.boot.demo_progress {
        st.progress.unlocked = 6;
        st.progress.best = [48.2, 61.0, 77.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        st.progress.medal = [3, 3, 1, 0, 0, 0, 0, 0];
        st.progress.gears = [9, 8, 5, 0, 0, 0, 0, 0];
        st.progress.golden = [true, false, true, false, false, false, false, false];
        st.progress.done = [true, true, true, false, false, false, false, false];
    }
    apply_options(ctx, st);
    refresh_options_ui(ctx, st);
    show_menu(ctx, st);
    st.fade = 1.0;
    st.fade_target = 0.0;

    // Скриншоты и ручная проверка: сразу открыть нужный экран.
    if let Some(screen) = st.boot.screen.clone() {
        match screen.as_str() {
            "select" => apply(ctx, st, Action::SelectScreen),
            "options" => apply(ctx, st, Action::OptionsScreen),
            "pause" | "results" | "lose" => {
                start_course(ctx, st, 0);
                skip_countdown(ctx, st);
                match screen.as_str() {
                    "pause" => apply(ctx, st, Action::Pause),
                    "results" => {
                        st.clock = 47.3;
                        st.gears = 7;
                        finish_course(ctx, st);
                    }
                    _ => {
                        st.falls = 2;
                        lose_course(ctx, st);
                    }
                }
            }
            s if s.starts_with('c') && s[1..].parse::<usize>().is_ok() => {
                let n = s[1..].parse::<usize>().unwrap_or(1).clamp(1, COUNT);
                st.progress.unlocked = COUNT;
                start_course(ctx, st, n - 1);
                skip_countdown(ctx, st);
                let at = st.boot.at;
                if at > 0.0 {
                    place_along_route(ctx, st, at);
                }
            }
            _ => {}
        }
        st.fade = 0.0;
    }
}

/// Поставить шар на долю маршрута (скриншоты).
fn place_along_route(ctx: &mut Ctx, st: &mut GameState, fraction: f32) {
    let (Some(built), Some(ball)) = (st.built.as_ref(), st.ball) else { return };
    if built.route.is_empty() {
        return;
    }
    let index = ((built.route.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    let pos = built.route[index].pos + Vec3::Y * (BALL_RADIUS + 0.15);
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

fn update(ctx: &mut Ctx, st: &mut GameState, dt: f32) {
    // Действия: клики интерфейса, клавиши, очередь из тестов.
    let mut actions = std::mem::take(&mut st.queue);
    collect_input_actions(ctx, st, &mut actions);
    for action in actions {
        apply(ctx, st, action);
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
                if let Some(ids) = &st.ui {
                    ui::set_text(ctx.scene_mut(), ids.hud_count, &shown.to_string());
                }
            }
            if st.countdown <= 0.0 {
                go(ctx, st);
            }
        }
        Screen::Playing => {
            st.clock += dt;
            st.hint_time += dt;
            if st.clock >= COURSES[st.course].limit {
                lose_course(ctx, st);
            }
        }
        _ => {}
    }

    // Затемнение переходов.
    let speed = 3.2;
    st.fade += (st.fade_target - st.fade).clamp(-speed * dt.max(ctx.time().unscaled_delta), speed * dt.max(ctx.time().unscaled_delta));
    if let Some(ids) = &st.ui {
        let alpha = st.fade.clamp(0.0, 1.0);
        let scene = ctx.scene_mut();
        if let Some(&image) = scene.get(ids.fade).and_then(|o| o.children().first()) {
            ui::set_image_color(scene, image, Color::rgba(0.0, 0.0, 0.0, alpha));
        }
    }

    // Подсказка и тост живут по реальному времени, чтобы не зависать на паузе.
    let real = ctx.time().unscaled_delta;
    st.toast = (st.toast - real).max(0.0);
    refresh_hud(ctx, st);
    if st.save_dirty {
        flush_save(ctx, st);
    }
}

/// Клики по кнопкам и клавиши, переведённые в действия.
fn collect_input_actions(ctx: &mut Ctx, st: &GameState, out: &mut Vec<Action>) {
    let ui = ctx.ui();
    let click = |name: &str| ui.clicked(name);
    let mut buttons: Vec<(&str, Action)> = vec![
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
    let cards: Vec<(String, usize)> = (0..COUNT).map(|i| (format!("course_{i}"), i)).collect();
    let mut found: Vec<Action> = buttons.drain(..).filter(|(name, _)| click(name)).map(|(_, a)| a).collect();
    for (name, i) in &cards {
        if click(name) {
            found.push(Action::Course(*i));
        }
    }
    out.extend(found);

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

fn handle_message(ctx: &mut Ctx, st: &mut GameState, message: &Message) {
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
            burst(ctx.world, pos, ParticleSystem::pickup_sparkle().in_3d().scaled(if golden { 2.0 } else { 1.0 }));
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
            if let Some(ids) = &st.ui {
                ui::set_text(ctx.scene_mut(), ids.hud_count, "");
            }
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------
// Действия и экраны
// ----------------------------------------------------------------------

/// Выполнить действие игрока.
pub fn apply(ctx: &mut Ctx, st: &mut GameState, action: Action) {
    let screen = st.screen;
    // Допустимость действий зависит от экрана: кнопки скрытых холстов кликнуть нельзя,
    // но очередь из тестов и клавиши приходят отовсюду.
    match action {
        Action::Play if screen == Screen::Menu => {
            click_sound(ctx);
            let index = st.progress.continue_index();
            start_course(ctx, st, index);
        }
        Action::SelectScreen if screen == Screen::Menu || screen == Screen::Paused || screen == Screen::Results => {
            click_sound(ctx);
            if screen != Screen::Menu {
                leave_course(ctx, st);
            }
            show_select(ctx, st);
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
                start_course(ctx, st, i);
            } else {
                play_ui(ctx, "ui_error");
            }
        }
        Action::Back if screen == Screen::Select || screen == Screen::Options => {
            click_sound(ctx);
            flush_save(ctx, st);
            show_menu(ctx, st);
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
        Action::Restart if screen == Screen::Paused || screen == Screen::Results || screen == Screen::Playing => {
            click_sound(ctx);
            let index = st.course;
            start_course(ctx, st, index);
        }
        Action::ToMenu if screen == Screen::Paused || screen == Screen::Results => {
            click_sound(ctx);
            leave_course(ctx, st);
            show_menu(ctx, st);
        }
        Action::ToSelect if screen == Screen::Paused || screen == Screen::Results => {
            click_sound(ctx);
            leave_course(ctx, st);
            show_select(ctx, st);
        }
        Action::Next if screen == Screen::Results => {
            click_sound(ctx);
            let win = st.result.is_some_and(|r| r.win);
            if win && st.course + 1 < COUNT {
                let next = st.course + 1;
                start_course(ctx, st, next);
            } else {
                leave_course(ctx, st);
                show_menu(ctx, st);
            }
        }
        Action::Reset if screen == Screen::Playing => {
            respawn(ctx, st, "respawn", "");
        }
        _ => {}
    }
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
    if let Some(ids) = &st.ui {
        ui::set_text(ctx.scene_mut(), ids.hud_toast, message);
    }
}

/// Показать нужный холст и настроить время, звук и камеру под экран.
fn set_screen(ctx: &mut Ctx, st: &mut GameState, screen: Screen) {
    st.screen = screen;
    let Some(ids) = st.ui.clone() else { return };
    let scene = ctx.scene_mut();
    scene.ui_set_visible(ids.menu, screen == Screen::Menu);
    scene.ui_set_visible(ids.select, screen == Screen::Select);
    scene.ui_set_visible(ids.options, screen == Screen::Options);
    scene.ui_set_visible(ids.hud, matches!(screen, Screen::Countdown | Screen::Playing | Screen::Paused));
    scene.ui_set_visible(ids.pause, screen == Screen::Paused);
    scene.ui_set_visible(ids.results, screen == Screen::Results);
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

fn show_menu(ctx: &mut Ctx, st: &mut GameState) {
    ensure_showcase(ctx, st);
    st.in_course = false;
    set_screen(ctx, st, Screen::Menu);
    refresh_menu(ctx, st);
    if let Some(rig) = camera_rig(ctx) {
        rig.orbit = true;
    }
}

fn show_select(ctx: &mut Ctx, st: &mut GameState) {
    ensure_showcase(ctx, st);
    st.in_course = false;
    set_screen(ctx, st, Screen::Select);
    refresh_cards(ctx, st);
    if let Some(rig) = camera_rig(ctx) {
        rig.orbit = true;
    }
}

/// Камера и её компонент.
fn camera_rig<'a>(ctx: &'a mut Ctx) -> Option<&'a mut CameraRig> {
    let id = ctx.world.scene.find_by_tag("MainCamera")?;
    ctx.world.scene.get_component_mut::<CameraRig>(id)
}

/// Убрать цех и шар (после выхода в меню).
fn leave_course(ctx: &mut Ctx, st: &mut GameState) {
    if let Some(ball) = st.ball.take() {
        ctx.world.scene.destroy(ball);
    }
    if let Some(root) = st.course_root.take() {
        ctx.world.scene.destroy(root);
    }
    st.built = None;
    st.in_course = false;
    st.result = None;
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.ball = None;
    }
}

/// Витрина меню: небольшой кусок завода, вокруг которого кружит камера.
fn ensure_showcase(ctx: &mut Ctx, st: &mut GameState) {
    if st.course_root.is_some() && !st.in_course {
        return;
    }
    leave_course(ctx, st);
    let root = ctx.world.scene.spawn("CourseRoot");
    let (center, distance) = {
        let mut kit = Kit::new(ctx.world, root);
        courses::build_showcase(&mut kit)
    };
    st.course_root = Some(root);
    if let Some(rig) = camera_rig(ctx) {
        rig.orbit = true;
        rig.orbit_center = center;
        rig.orbit_distance = distance;
    }
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.snap = true;
        view.ball = None;
    }
}

// ----------------------------------------------------------------------
// Цех
// ----------------------------------------------------------------------

/// Построить цех `index` и начать отсчёт.
pub fn start_course(ctx: &mut Ctx, st: &mut GameState, index: usize) {
    let index = index.min(COUNT - 1);
    leave_course(ctx, st);

    let root = ctx.world.scene.spawn("CourseRoot");
    let built = {
        let mut kit = Kit::new(ctx.world, root);
        courses::build(&mut kit, index)
    };
    let ball = spawn_ball(ctx, &built);

    st.course = index;
    st.course_root = Some(root);
    st.ball = Some(ball);
    st.in_course = true;
    st.clock = 0.0;
    st.countdown = COUNTDOWN;
    st.last_count = -1;
    st.gears = 0;
    st.gears_total = built.gears;
    st.golden = false;
    st.golden_exists = built.golden;
    st.falls = 0;
    st.checkpoints_total = built.checkpoints;
    st.checkpoint_idx = 0;
    st.checkpoint_pos = built.start;
    st.checkpoint_yaw = built.yaw_deg.to_radians();
    st.pending_checkpoint = (0, 0.0);
    st.result = None;
    st.hint_time = 0.0;
    st.built = Some(built);

    // Камера смотрит вдоль цеха и следует за шаром.
    let yaw = st.checkpoint_yaw;
    if let Some(view) = ctx.world.resources.get_mut::<ViewState>() {
        view.yaw = yaw;
        view.pitch = 0.6;
        view.ball = Some(ball);
        view.snap = true;
        view.trauma = 0.0;
    }
    if let Some(rig) = camera_rig(ctx) {
        rig.orbit = false;
    }
    if let Some(status) = ctx.resource_mut::<BallStatus>() {
        *status = BallStatus::default();
    }
    if let Some(ids) = &st.ui {
        let scene = ctx.scene_mut();
        ui::set_text(scene, ids.hud_count, "");
        ui::set_text(scene, ids.hud_toast, "");
    }
    st.fade = 1.0;
    st.fade_target = 0.0;
    set_screen(ctx, st, Screen::Countdown);
}

/// Шар: стальная сфера с физикой и компонентом управления.
fn spawn_ball(ctx: &mut Ctx, built: &Built) -> GameObjectId {
    let start = built.start;
    let kill_y = built.kill_y;
    let ball = ctx
        .world
        .scene
        .build("Ball")
        .tag("Ball")
        .at3(start.x, start.y, start.z)
        .with(MeshRenderer::sphere().with_color(Color::hex(0xc9d3e4)).with_material(0.9, 0.25))
        .with(Rigidbody { ccd: true, angular_damping: 0.05, linear_damping: 0.02, ..Rigidbody::dynamic() })
        .with(Collider::sphere(BALL_RADIUS).with_material(0.8, 0.08).with_density(4.0))
        .with(Ball { kill_y, frozen: true, ..Ball::default() })
        .id();
    // Две яркие полосы-кольца: по ним видно, как шар катится.
    for (name, rotation) in [("Band A", Quat::IDENTITY), ("Band B", Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))] {
        ctx.world
            .scene
            .build(name)
            .rotation(rotation)
            .scale(1.035, 0.24, 1.035)
            .with(MeshRenderer::cylinder().with_color(Color::hex(0xff5a10)).with_material(0.1, 0.45).with_emissive(Color::rgb(0.5, 0.1, 0.0)))
            .child_of(ball);
    }
    ball
}

/// Закончить отсчёт: шар свободен.
fn go(ctx: &mut Ctx, st: &mut GameState) {
    st.countdown = 0.0;
    play(ctx, "go", 1.0, 1.0);
    if let Some(ids) = &st.ui {
        let scene = ctx.scene_mut();
        ui::set_text(scene, ids.hud_count, "Поехали!");
    }
    set_screen(ctx, st, Screen::Playing);
    ctx.after(0.9, "hide_count", Value::Nil);
}

/// Пропустить отсчёт (скриншоты, тесты).
pub fn skip_countdown(ctx: &mut Ctx, st: &mut GameState) {
    if st.screen == Screen::Countdown {
        st.countdown = 0.0;
        set_screen(ctx, st, Screen::Playing);
        if let Some(ids) = &st.ui {
            ui::set_text(ctx.scene_mut(), ids.hud_count, "");
        }
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
    burst(ctx.world, pos, ParticleSystem::pickup_sparkle().in_3d());
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
    let medal = medal_for(i, time);
    let p = &mut st.progress;
    let new_best = p.best[i] <= 0.0 || time < p.best[i];
    if new_best {
        p.best[i] = time;
    }
    p.medal[i] = p.medal[i].max(medal);
    p.gears[i] = p.gears[i].max(st.gears);
    p.golden[i] |= st.golden;
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
    burst(ctx.world, pos + Vec3::Y, ParticleSystem::explosion().in_3d().scaled(1.4));
    burst(ctx.world, pos + Vec3::Y * 0.5, ParticleSystem::pickup_sparkle().in_3d().scaled(2.5));
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
    let Some(ids) = &st.ui else { return };
    let label = if st.progress.started() {
        format!("Продолжить: цех {}", st.progress.continue_index() + 1)
    } else {
        "Начать игру".to_string()
    };
    ui::set_text(ctx.scene_mut(), ids.menu_play_label, &label);
}

fn refresh_cards(ctx: &mut Ctx, st: &GameState) {
    let Some(ids) = &st.ui else { return };
    let scene = ctx.scene_mut();
    for (i, card) in ids.cards.iter().enumerate() {
        let open = i < st.progress.unlocked;
        let p = &st.progress;
        ui::set_text(scene, card.title, &format!("{}. {}", i + 1, COURSES[i].name));
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
    let Some(ids) = &st.ui else { return };
    let o = st.options;
    let scene = ctx.scene_mut();
    for (slider, value) in [(ids.opt_music, o.music), (ids.opt_sfx, o.sfx), (ids.opt_sens, o.sensitivity)] {
        if let Some(s) = scene.get_component_mut::<Slider>(slider) {
            s.value = value;
        }
    }
    ui::set_text(scene, ids.opt_music_text, &format!("Музыка: {}%", (o.music * 100.0).round() as i32));
    ui::set_text(scene, ids.opt_sfx_text, &format!("Звуки: {}%", (o.sfx * 100.0).round() as i32));
    ui::set_text(scene, ids.opt_sens_text, &format!("Чувствительность камеры: {}%", (o.sensitivity * 100.0).round() as i32));
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
        // Пример громкости: щелчок на новом уровне.
    }
    if let Some(v) = ui.value_changed("opt_sens") {
        st.options.sensitivity = v.clamp(0.3, 2.0);
        changed = true;
    }
    if changed {
        apply_options(ctx, st);
        refresh_options_ui(ctx, st);
        let mut o = st.options;
        o.sensitivity = o.sensitivity.clamp(0.3, 2.0);
        let save = ctx.save_data();
        o.store(save);
        st.save_dirty = true;
    }
}

fn fill_results(ctx: &mut Ctx, st: &GameState) {
    let (Some(ids), Some(r)) = (&st.ui, st.result) else { return };
    let info = &COURSES[st.course];
    let best = st.progress.best[st.course];
    let last = st.course + 1 >= COUNT;
    let scene = ctx.scene_mut();
    if r.win {
        ui::set_text(scene, ids.res_title, if last { "Завод запущен!" } else { "Цех пройден!" });
        ui::set_text_color(scene, ids.res_title, ui::GREEN);
    } else {
        ui::set_text(scene, ids.res_title, "Смена окончена");
        ui::set_text_color(scene, ids.res_title, ui::RED);
    }
    ui::set_text(scene, ids.res_course, &format!("Цех {} · {}", st.course + 1, info.name));
    ui::set_image_color(scene, ids.res_medal, Color::hex(ui::medal_color(r.medal)));
    ui::set_text(scene, ids.res_medal_text, if r.win { ui::medal_name(r.medal) } else { "Время вышло" });
    ui::set_text_color(scene, ids.res_medal_text, if r.win { ui::medal_color(r.medal) } else { ui::RED });
    ui::set_text(scene, ids.res_time, &format!("Время: {}", ui::fmt_time(r.time)));
    let best_line = if !r.win {
        format!("Лимит смены: {}", ui::fmt_time(info.limit))
    } else if r.new_best {
        "Новый рекорд!".to_string()
    } else {
        format!("Рекорд: {}", ui::fmt_time(best))
    };
    ui::set_text(scene, ids.res_best, &best_line);
    ui::set_text_color(scene, ids.res_best, if r.win && r.new_best { ui::AMBER } else { ui::MUTED });
    ui::set_text(scene, ids.res_gears, &format!("Шестерни: {} из {}", r.gears, r.gears_total));
    let golden = if !r.golden_exists { "" } else if r.golden { "Золотая шестерня: найдена" } else { "Золотая шестерня: не найдена" };
    ui::set_text(scene, ids.res_golden, golden);
    ui::set_text_color(scene, ids.res_golden, if r.golden { ui::GOLD } else { ui::MUTED });
    ui::set_text(scene, ids.res_falls, &format!("Падений: {}", r.falls));
    let next_label = if r.win && !last { "Дальше" } else if r.win { "В меню" } else { "Заново" };
    let target = scene.get(ids.res_next).and_then(|o| o.children().first().copied());
    if let Some(label) = target {
        ui::set_text(scene, label, next_label);
    }
}

fn refresh_hud(ctx: &mut Ctx, st: &GameState) {
    let Some(ids) = &st.ui else { return };
    if !matches!(st.screen, Screen::Countdown | Screen::Playing | Screen::Paused) {
        return;
    }
    let info = &COURSES[st.course];
    let speed = ctx.resource::<BallStatus>().map_or(0.0, |s| s.speed);
    let toast_visible = st.toast > 0.0;
    let hint_visible = st.course == 0 && st.hint_time < 14.0 && st.screen != Screen::Paused;
    let scene = ctx.scene_mut();
    ui::set_text(scene, ids.hud_course, &format!("Цех {} · {}", st.course + 1, info.name));
    ui::set_text(scene, ids.hud_gears, &format!("Шестерни {}/{}", st.gears, st.gears_total));
    let golden = if !st.golden_exists { "" } else if st.golden { "Золотая шестерня: найдена" } else { "Золотая шестерня: ищется" };
    ui::set_text(scene, ids.hud_golden, golden);
    ui::set_text_color(scene, ids.hud_golden, if st.golden { ui::GOLD } else { ui::MUTED });
    ui::set_text(scene, ids.hud_time, &ui::fmt_time(st.clock));
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
            ui::set_text(scene, ids.hud_target, &format!("{name} до {}", ui::fmt_time(limit)));
            ui::set_text_color(scene, ids.hud_target, color);
        }
        None => {
            ui::set_text(scene, ids.hud_target, &format!("Лимит смены {}", ui::fmt_time(info.limit)));
            ui::set_text_color(scene, ids.hud_target, ui::MUTED);
        }
    }
    ui::set_text(scene, ids.hud_checkpoint, &format!("Контрольная точка {}/{}", st.checkpoint_idx, st.checkpoints_total));
    if let Some(bar) = scene.get_component_mut::<ProgressBar>(ids.hud_speed) {
        bar.value = (speed / 16.0).clamp(0.0, 1.0);
    }
    if !toast_visible {
        ui::set_text(scene, ids.hud_toast, "");
    }
    scene.ui_set_visible(ids.hud_hint, hint_visible);
}

fn flush_save(ctx: &mut Ctx, st: &mut GameState) {
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
