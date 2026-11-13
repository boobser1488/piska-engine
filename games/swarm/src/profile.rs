//! Сохранения «Роя»: настройки, прогресс, таблица лучших забегов, контрольная точка.
//!
//! Всё хранится в `SaveData` обычными ключами (на десктопе — файл, в браузере —
//! `localStorage`). Формат строк простой и терпимый к порче: нечитаемая запись
//! молча заменяется значением по умолчанию.

use engine::prelude::SaveData;

use crate::data::{ENDLESS_UNLOCK_WAVE, UP_COUNT};
use crate::sim::{Checkpoint, Mode};

/// Настройки игрока.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub music: f32,
    pub sfx: f32,
    /// Тряска камеры.
    pub shake: bool,
    /// Числа урона над врагами.
    pub numbers: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { music: 0.6, sfx: 0.8, shake: true, numbers: true }
    }
}

impl Options {
    pub fn load(save: &SaveData) -> Options {
        let d = Options::default();
        Options {
            music: save.float("opt.music", d.music).clamp(0.0, 1.0),
            sfx: save.float("opt.sfx", d.sfx).clamp(0.0, 1.0),
            shake: save.bool("opt.shake", d.shake),
            numbers: save.bool("opt.numbers", d.numbers),
        }
    }

    pub fn store(&self, save: &mut SaveData) {
        save.set_float("opt.music", self.music);
        save.set_float("opt.sfx", self.sfx);
        save.set_bool("opt.shake", self.shake);
        save.set_bool("opt.numbers", self.numbers);
    }
}

/// Долгосрочный прогресс.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// Самая дальняя волна, до которой доходил игрок (в любом режиме).
    pub max_wave: u32,
    pub best_campaign: u64,
    pub best_endless: u64,
    pub best_endless_wave: u32,
    pub wins: u32,
    pub runs: u32,
}

impl Progress {
    pub fn load(save: &SaveData) -> Progress {
        Progress {
            max_wave: save.int("prog.max_wave", 0).max(0) as u32,
            best_campaign: save.int("prog.best_campaign", 0).max(0) as u64,
            best_endless: save.int("prog.best_endless", 0).max(0) as u64,
            best_endless_wave: save.int("prog.best_endless_wave", 0).max(0) as u32,
            wins: save.int("prog.wins", 0).max(0) as u32,
            runs: save.int("prog.runs", 0).max(0) as u32,
        }
    }

    pub fn store(&self, save: &mut SaveData) {
        save.set_int("prog.max_wave", self.max_wave as i64);
        save.set_int("prog.best_campaign", self.best_campaign as i64);
        save.set_int("prog.best_endless", self.best_endless as i64);
        save.set_int("prog.best_endless_wave", self.best_endless_wave as i64);
        save.set_int("prog.wins", self.wins as i64);
        save.set_int("prog.runs", self.runs as i64);
    }

    /// Бесконечный режим открыт, если когда-либо начиналась 10-я волна.
    pub fn endless_unlocked(&self) -> bool {
        self.max_wave >= ENDLESS_UNLOCK_WAVE
    }

    /// Отметить начало волны. Возвращает `true`, если бесконечный режим открылся именно сейчас.
    pub fn note_wave(&mut self, wave: u32) -> bool {
        let was = self.endless_unlocked();
        self.max_wave = self.max_wave.max(wave);
        !was && self.endless_unlocked()
    }
}

/// Запись в таблице лучших забегов.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    pub score: u64,
    pub wave: u32,
    pub mode: Mode,
    pub secs: u32,
    pub won: bool,
}

pub const LEADERBOARD_SIZE: usize = 5;

pub fn load_leaderboard(save: &SaveData) -> Vec<RunRecord> {
    save.string("board", "")
        .split(';')
        .filter_map(|item| {
            let f: Vec<&str> = item.split(',').collect();
            if f.len() != 5 {
                return None;
            }
            Some(RunRecord {
                score: f[0].parse().ok()?,
                wave: f[1].parse().ok()?,
                mode: if f[2] == "e" { Mode::Endless } else { Mode::Campaign },
                secs: f[3].parse().ok()?,
                won: f[4] == "1",
            })
        })
        .take(LEADERBOARD_SIZE)
        .collect()
}

/// Добавить забег в таблицу (хранятся 5 лучших по очкам). Возвращает место (с 0) или `None`.
pub fn add_record(save: &mut SaveData, rec: RunRecord) -> Option<usize> {
    let mut board = load_leaderboard(save);
    let pos = board.iter().position(|r| rec.score > r.score).unwrap_or(board.len());
    if pos >= LEADERBOARD_SIZE {
        return None;
    }
    board.insert(pos, rec);
    board.truncate(LEADERBOARD_SIZE);
    let text: Vec<String> = board
        .iter()
        .map(|r| format!("{},{},{},{},{}", r.score, r.wave, if r.mode == Mode::Endless { "e" } else { "c" }, r.secs, u8::from(r.won)))
        .collect();
    save.set_string("board", &text.join(";"));
    Some(pos)
}

pub fn store_checkpoint(save: &mut SaveData, cp: &Checkpoint) {
    let levels: Vec<String> = cp.levels.iter().map(|l| l.to_string()).collect();
    save.set_string(
        "run",
        &format!(
            "{},{},{},{:.2},{},{:.1},{},{:.1}|{}",
            if cp.mode == Mode::Endless { "e" } else { "c" },
            cp.next_wave,
            cp.level,
            cp.xp,
            cp.score,
            cp.core_hp,
            cp.kills,
            cp.time,
            levels.join(",")
        ),
    );
}

pub fn load_checkpoint(save: &SaveData) -> Option<Checkpoint> {
    let text = save.string("run", "");
    let (head, lv) = text.split_once('|')?;
    let f: Vec<&str> = head.split(',').collect();
    if f.len() != 8 {
        return None;
    }
    let lvs: Vec<u8> = lv.split(',').filter_map(|x| x.parse().ok()).collect();
    if lvs.len() != UP_COUNT {
        return None;
    }
    let mut levels = [0u8; UP_COUNT];
    levels.copy_from_slice(&lvs);
    Some(Checkpoint {
        mode: if f[0] == "e" { Mode::Endless } else { Mode::Campaign },
        next_wave: f[1].parse().ok()?,
        level: f[2].parse().ok()?,
        xp: f[3].parse().ok()?,
        score: f[4].parse().ok()?,
        core_hp: f[5].parse().ok()?,
        kills: f[6].parse().ok()?,
        time: f[7].parse().ok()?,
        levels,
    })
}

pub fn clear_checkpoint(save: &mut SaveData) {
    save.delete_key("run");
}
