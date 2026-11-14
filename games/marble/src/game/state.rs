//! Состояние игры: экран, текущий заезд, прогресс и настройки (сохраняются). Лежит в
//! ресурсе `GameState` и переживает смену сцены: меню и цеха — разные сцены.

use engine::prelude::*;

use super::data::{CourseInfo, Waypoint, COUNT};
use super::ui::UiRefs;

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

/// Медаль за время (0 — без медали).
pub fn medal_for(info: &CourseInfo, time: f32) -> u8 {
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

/// Всё состояние игры.
pub struct GameState {
    pub screen: Screen,
    pub course: usize,
    /// Таблица цехов из `data/courses.ron`.
    pub courses: Vec<CourseInfo>,
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
    pub(crate) pending_checkpoint: (i32, f32),
    pub result: Option<RunResult>,
    pub progress: Progress,
    pub options: Options,
    pub ui: UiRefs,
    pub ball: Option<GameObjectId>,
    /// Маршрут цеха (`data/routes/course_N.ron`): для бота тестов и скриншотов.
    pub route: Vec<Waypoint>,
    /// Динамических тел в цехе (ящики, качели) — для проверки бюджета физики.
    pub bodies: u32,
    /// Заезд идёт «в цехе» (а не в меню).
    pub in_course: bool,
    pub toast: f32,
    pub fade: f32,
    pub(crate) fade_target: f32,
    pub hint_time: f32,
    pub queue: Vec<Action>,
    pub boot: Boot,
    /// Меню должно открыться на выборе цеха (после «Выбор цеха» из паузы и результатов).
    pub open_select: bool,
    /// Сцена загружается: действия игрока пока не принимаются.
    pub loading: bool,
    pub(crate) boot_done: bool,
    pub(crate) last_count: i32,
    pub(crate) save_dirty: bool,
}

impl GameState {
    pub fn new(save: &SaveData, boot: Boot) -> Self {
        Self {
            screen: Screen::Menu,
            course: 0,
            courses: Vec::new(),
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
            ui: UiRefs::default(),
            ball: None,
            route: Vec::new(),
            bodies: 0,
            in_course: false,
            toast: 0.0,
            fade: 1.0,
            fade_target: 0.0,
            hint_time: 0.0,
            queue: Vec::new(),
            boot,
            open_select: false,
            loading: false,
            boot_done: false,
            last_count: -1,
            save_dirty: false,
        }
    }

    /// Поставить действие в очередь — выполнится в ближайшем кадре (для тестов и скриптов).
    pub fn push(&mut self, action: Action) {
        self.queue.push(action);
    }

    /// Описание текущего цеха.
    pub fn info(&self) -> &CourseInfo {
        &self.courses[self.course.min(self.courses.len().saturating_sub(1))]
    }
}
