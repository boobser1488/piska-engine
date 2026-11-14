//! Данные игры из ассетов: список цехов с порогами медалей (`data/courses.ron`), маршруты
//! для бота и подсказок (`data/routes/course_N.ron`), список звуков (`data/sounds.ron`).
//! Всё читается через базу ассетов, поэтому работает и из каталога, и из пака.

use engine::prelude::*;
use serde::{Deserialize, Serialize};

/// Сколько цехов в игре.
pub const COUNT: usize = 8;

/// Описание цеха: название, что нового, пороги медалей и лимит смены, секунды.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CourseInfo {
    pub name: String,
    /// Что нового в этом цехе.
    pub blurb: String,
    /// Пороги медалей: уложился — получил.
    pub gold: f32,
    pub silver: f32,
    pub bronze: f32,
    /// Смена закончилась: время вышло — цех провален.
    pub limit: f32,
}

/// Файл `data/courses.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CourseTable {
    pub courses: Vec<CourseInfo>,
}

/// Чем занять шара на точке маршрута.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum WpKind {
    /// Просто доехать.
    Go,
    /// Доехать и прыгнуть (перед щелью).
    Jump,
    /// Стоять, пока механизм с тегом `Mover` не окажется рядом с этой точкой.
    WaitMover([f32; 3]),
}

/// Точка маршрута цеха (по ней ведёт шар бот тестов и ставится шар для скриншотов).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub pos: [f32; 3],
    pub kind: WpKind,
}

impl Waypoint {
    pub fn position(&self) -> Vec3 {
        Vec3::from_array(self.pos)
    }
}

/// Файл `data/routes/course_N.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub points: Vec<Waypoint>,
}

/// Файл `data/sounds.ron`: имя клипа → файл; музыка отдельно.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundList {
    pub effects: Vec<(String, String)>,
    pub music: Vec<(String, String)>,
}

/// Прочитать RON-ассет `path` (относительно корня ассетов).
pub fn load_ron<T: for<'de> Deserialize<'de>>(world: &World, path: &str) -> Option<T> {
    let database = world.resources.get::<AssetDatabase>()?;
    // Чтение и разбор — движком (`load_data`): ошибка называет файл, строку и столбец.
    match database.load_data(path) {
        Ok(value) => Some(value),
        Err(err) => {
            log::error!("data asset `{path}`: {err}");
            None
        }
    }
}

/// Таблица цехов; если файла нет — цехи с запасными названиями и порогами (в логе ошибка).
pub fn load_courses(world: &World) -> Vec<CourseInfo> {
    let mut courses = load_ron::<CourseTable>(world, "data/courses.ron").map(|t| t.courses).unwrap_or_default();
    while courses.len() < COUNT {
        let n = courses.len() + 1;
        courses.push(CourseInfo { name: format!("Course {n}"), blurb: String::new(), gold: 60.0, silver: 90.0, bronze: 130.0, limit: 300.0 });
    }
    courses.truncate(COUNT);
    courses
}

/// Маршрут цеха `index` (0-based).
pub fn load_route(world: &World, index: usize) -> Vec<Waypoint> {
    load_ron::<Route>(world, &format!("data/routes/course_{}.ron", index + 1)).map(|r| r.points).unwrap_or_default()
}
