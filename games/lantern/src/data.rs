//! Постоянные данные игры: список уровней, прогресс игрока и настройки (сохранение).

use engine::prelude::*;

/// Сколько уровней в игре.
pub const LEVEL_COUNT: usize = 6;

/// Строка таблицы уровней `assets/data/levels.ron`.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct LevelInfo {
    /// Имя сцены (`level1` → `scenes/level1.scene`).
    pub scene: String,
    pub name: String,
    pub subtitle: String,
    /// Короткое описание на карточке выбора уровня.
    pub blurb: String,
    /// Сколько светлячков нужно для выхода.
    pub need: i32,
    /// Контрольное время для третьей звезды, секунд.
    pub par_time: f32,
    /// Имя клипа музыки (`assets/audio/<имя>.wav`).
    pub music: String,
}

/// Правила звёзд — `assets/data/rules.ron`.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Rules {
    /// Для второй звезды: доля собранных светлячков, %.
    pub star2_fireflies_percent: i32,
    /// Для второй звезды: не больше стольких поражений.
    pub star2_max_deaths: i32,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { star2_fireflies_percent: 60, star2_max_deaths: 2 }
    }
}

/// Таблицы содержимого игры, прочитанные из `assets/data/`.
#[derive(Clone, Debug)]
pub struct GameData {
    pub levels: Vec<LevelInfo>,
    pub rules: Rules,
}

impl GameData {
    /// Прочитать `data/levels.ron` и `data/rules.ron` через базу ассетов (работает и из пака).
    pub fn load(db: &AssetDatabase) -> Result<GameData, engine::assets::AssetError> {
        // Файлы данных читает движок (`AssetDatabase::load_data`): RON или JSON по расширению.
        let data = GameData { levels: db.load_data("data/levels.ron")?, rules: db.load_data("data/rules.ron")? };
        if data.levels.len() != LEVEL_COUNT {
            return Err(engine::assets::AssetError::Import {
                path: "data/levels.ron".to_string(),
                reason: format!("expected {LEVEL_COUNT} levels, found {}", data.levels.len()),
            });
        }
        Ok(data)
    }

    pub fn level(&self, index: usize) -> &LevelInfo {
        &self.levels[index.min(self.levels.len() - 1)]
    }
}

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
    /// Звёзды: 1 — дошёл; 2 — достаточно светлячков и не больше N поражений (`rules.ron`);
    /// 3 — золотой светлячок и время не хуже контрольного.
    pub fn stars(&self, par: f32, rules: &Rules) -> u8 {
        let mut stars = 1;
        if self.total > 0 && self.fireflies * 100 >= self.total * rules.star2_fireflies_percent && self.deaths <= rules.star2_max_deaths {
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
    pub fn record(&mut self, level: usize, result: &RunResult, par: f32, rules: &Rules) {
        if level >= LEVEL_COUNT {
            return;
        }
        self.completed[level] = true;
        self.stars[level] = self.stars[level].max(result.stars(par, rules));
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
        let rules = Rules::default();
        assert_eq!(r(2, false, 500.0, 9).stars(100.0, &rules), 1);
        assert_eq!(r(8, false, 500.0, 2).stars(100.0, &rules), 2);
        assert_eq!(r(8, true, 90.0, 2).stars(100.0, &rules), 3);
        assert_eq!(r(8, true, 190.0, 2).stars(100.0, &rules), 2);
        assert_eq!(r(10, true, 10.0, 3).stars(100.0, &rules), 1);
    }

    #[test]
    fn progress_roundtrips_through_save_data() {
        let mut p = Progress { music: 0.25, shake: false, ..Progress::default() };
        p.record(0, &RunResult { time: 61.0, fireflies: 9, total: 10, golden: true, deaths: 0 }, 90.0, &Rules::default());
        p.record(1, &RunResult { time: 80.0, fireflies: 5, total: 10, golden: false, deaths: 1 }, 90.0, &Rules::default());
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
        p.record(0, &RunResult { time: 50.0, fireflies: 9, total: 10, golden: false, deaths: 0 }, 90.0, &Rules::default());
        p.record(0, &RunResult { time: 70.0, fireflies: 4, total: 10, golden: false, deaths: 0 }, 90.0, &Rules::default());
        assert_eq!(p.best_time[0], 50.0);
        assert_eq!(p.best_fireflies[0], 9);
        assert_eq!(p.stars[0], 2);
        assert_eq!(p.next_level(), 1);
    }
}
