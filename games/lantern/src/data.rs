//! Постоянные данные игры: список уровней, прогресс игрока и настройки (сохранение).

use engine::prelude::*;

/// Сколько уровней в игре.
pub const LEVEL_COUNT: usize = 6;

/// Описание уровня для меню (сами карты — в `assets/scenes/levelN.scene`).
pub struct LevelInfo {
    pub scene: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
}

pub static LEVELS: [LevelInfo; LEVEL_COUNT] = [
    LevelInfo { scene: "level1", name: "Тихая пещера", blurb: "Первые огоньки" },
    LevelInfo { scene: "level2", name: "Грибной грот", blurb: "Пружины и подъёмы" },
    LevelInfo { scene: "level3", name: "Кристальные залы", blurb: "Двойной прыжок в деле" },
    LevelInfo { scene: "level4", name: "Ржавые шахты", blurb: "Платформы и шипы" },
    LevelInfo { scene: "level5", name: "Ледяной провал", blurb: "Рывок сквозь тьму" },
    LevelInfo { scene: "level6", name: "Сердце тьмы", blurb: "Хозяин пещер" },
];

/// Какой уровень сейчас выбран (меню → сцена уровня).
#[derive(Clone, Copy, Debug, Default)]
pub struct Session {
    /// Номер уровня с нуля.
    pub level: usize,
}

/// Результат прохождения уровня.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunResult {
    pub time: f32,
    pub fireflies: i32,
    pub total: i32,
    pub golden: bool,
    pub deaths: i32,
}

impl RunResult {
    /// Звёзды: 1 — дошёл; 2 — не менее 60 % светлячков и не больше двух смертей;
    /// 3 — золотой светлячок и время не хуже контрольного.
    pub fn stars(&self, par: f32) -> u8 {
        let mut stars = 1;
        if self.total > 0 && self.fireflies * 5 >= self.total * 3 && self.deaths <= 2 {
            stars = 2;
            if self.golden && self.time <= par {
                stars = 3;
            }
        }
        stars
    }
}

/// Прогресс и настройки. Живёт в `world.resources`; на диск уходит через [`SaveData`].
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    /// Громкость музыки и эффектов, 0..=1.
    pub music: f32,
    pub sfx: f32,
    /// Тряска камеры.
    pub shake: bool,
    /// Сколько уровней открыто (1..=LEVEL_COUNT).
    pub unlocked: usize,
    pub completed: [bool; LEVEL_COUNT],
    pub stars: [u8; LEVEL_COUNT],
    /// Лучшее время, секунды (0 — ещё не проходили).
    pub best_time: [f32; LEVEL_COUNT],
    pub best_fireflies: [i32; LEVEL_COUNT],
    pub golden: [bool; LEVEL_COUNT],
}

impl Default for Progress {
    fn default() -> Self {
        Progress {
            music: 0.6,
            sfx: 0.8,
            shake: true,
            unlocked: 1,
            completed: [false; LEVEL_COUNT],
            stars: [0; LEVEL_COUNT],
            best_time: [0.0; LEVEL_COUNT],
            best_fireflies: [0; LEVEL_COUNT],
            golden: [false; LEVEL_COUNT],
        }
    }
}

impl Progress {
    /// Двойной прыжок открывается в конце второго уровня.
    pub fn has_double_jump(&self) -> bool {
        self.completed[1]
    }

    /// Рывок открывается в конце четвёртого уровня.
    pub fn has_dash(&self) -> bool {
        self.completed[3]
    }

    /// Суммарное число звёзд.
    pub fn total_stars(&self) -> u32 {
        self.stars.iter().map(|&s| s as u32).sum()
    }

    pub fn load(save: &SaveData) -> Progress {
        let mut p = Progress {
            music: save.float("opt.music", 0.6).clamp(0.0, 1.0),
            sfx: save.float("opt.sfx", 0.8).clamp(0.0, 1.0),
            shake: save.bool("opt.shake", true),
            ..Progress::default()
        };
        p.unlocked = (save.int("progress.unlocked", 1).clamp(1, LEVEL_COUNT as i64)) as usize;
        for i in 0..LEVEL_COUNT {
            p.completed[i] = save.bool(&format!("lvl{i}.done"), false);
            p.stars[i] = save.int(&format!("lvl{i}.stars"), 0).clamp(0, 3) as u8;
            p.best_time[i] = save.float(&format!("lvl{i}.time"), 0.0);
            p.best_fireflies[i] = save.int(&format!("lvl{i}.ff"), 0) as i32;
            p.golden[i] = save.bool(&format!("lvl{i}.gold"), false);
        }
        p
    }

    pub fn store(&self, save: &mut SaveData) {
        save.set_float("opt.music", self.music);
        save.set_float("opt.sfx", self.sfx);
        save.set_bool("opt.shake", self.shake);
        save.set_int("progress.unlocked", self.unlocked as i64);
        for i in 0..LEVEL_COUNT {
            save.set_bool(&format!("lvl{i}.done"), self.completed[i]);
            save.set_int(&format!("lvl{i}.stars"), self.stars[i] as i64);
            save.set_float(&format!("lvl{i}.time"), self.best_time[i]);
            save.set_int(&format!("lvl{i}.ff"), self.best_fireflies[i] as i64);
            save.set_bool(&format!("lvl{i}.gold"), self.golden[i]);
        }
    }

    /// Записать результат уровня `level` (с нуля): лучшие значения сохраняются, следующий
    /// уровень открывается.
    pub fn record(&mut self, level: usize, result: &RunResult, par: f32) {
        if level >= LEVEL_COUNT {
            return;
        }
        self.completed[level] = true;
        self.stars[level] = self.stars[level].max(result.stars(par));
        if self.best_time[level] <= 0.0 || result.time < self.best_time[level] {
            self.best_time[level] = result.time;
        }
        self.best_fireflies[level] = self.best_fireflies[level].max(result.fireflies);
        self.golden[level] |= result.golden;
        self.unlocked = self.unlocked.max((level + 2).min(LEVEL_COUNT));
    }

    /// С какого уровня продолжать: первый непройденный из открытых.
    pub fn next_level(&self) -> usize {
        (0..self.unlocked).find(|&i| !self.completed[i]).unwrap_or(self.unlocked - 1)
    }

    pub fn has_progress(&self) -> bool {
        self.completed.iter().any(|&c| c)
    }
}

/// Время как «м:сс».
pub fn format_time(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_follow_the_rules() {
        let r = |ff, gold, time, deaths| RunResult { time, fireflies: ff, total: 10, golden: gold, deaths };
        assert_eq!(r(2, false, 500.0, 9).stars(100.0), 1);
        assert_eq!(r(8, false, 500.0, 2).stars(100.0), 2);
        assert_eq!(r(8, true, 90.0, 2).stars(100.0), 3);
        assert_eq!(r(8, true, 190.0, 2).stars(100.0), 2);
        assert_eq!(r(10, true, 10.0, 3).stars(100.0), 1);
    }

    #[test]
    fn progress_roundtrips_through_save_data() {
        let mut p = Progress { music: 0.25, shake: false, ..Progress::default() };
        p.record(0, &RunResult { time: 61.0, fireflies: 9, total: 10, golden: true, deaths: 0 }, 90.0);
        p.record(1, &RunResult { time: 80.0, fireflies: 5, total: 10, golden: false, deaths: 1 }, 90.0);
        let mut save = SaveData::in_memory();
        p.store(&mut save);
        let back = Progress::load(&save);
        assert_eq!(back, p);
        assert_eq!(back.unlocked, 3);
        assert!(back.has_double_jump() && !back.has_dash());
        assert_eq!(back.stars[0], 3);
    }

    #[test]
    fn best_results_are_kept() {
        let mut p = Progress::default();
        p.record(0, &RunResult { time: 50.0, fireflies: 9, total: 10, golden: false, deaths: 0 }, 90.0);
        p.record(0, &RunResult { time: 70.0, fireflies: 4, total: 10, golden: false, deaths: 0 }, 90.0);
        assert_eq!(p.best_time[0], 50.0);
        assert_eq!(p.best_fireflies[0], 9);
        assert_eq!(p.stars[0], 2);
        assert_eq!(p.next_level(), 1);
    }
}
