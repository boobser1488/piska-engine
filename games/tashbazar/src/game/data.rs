//! Данные игры из ассетов: маршруты карты (`data/routes.ron` — по ним ходят боты тестов,
//! в них же записано расчётное время пути) и список звуков (`data/sounds.ron`). Всё
//! читается через базу ассетов, поэтому работает и из каталога, и из пака.

use engine::prelude::*;
use serde::{Deserialize, Serialize};

/// Что делать в точке маршрута (для бота и для описания полосы препятствий).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    /// Бежать к точке.
    Run,
    /// В точке прыгнуть (обычный прыжок с разбега).
    Jump,
    /// В точке прыгнуть и в воздухе присесть: прыжок с приседом достаёт на 18 ед выше.
    CrouchJump,
    /// В точке прыгнуть и стрейфить в воздухе к следующей точке (набор скорости).
    StrafeJump,
    /// От этой точки до следующей — распрыжка со стрейфами.
    Hop,
}

/// Точка маршрута.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutePoint {
    pub pos: [f32; 3],
    pub step: Step,
}

impl RoutePoint {
    pub fn position(&self) -> Vec3 {
        Vec3::from_array(self.pos)
    }
}

/// Маршрут: откуда, куда, расчётное время и точки.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// Короткое имя: `atk_a_long`, `course` …
    pub id: String,
    /// Описание для README и отчётов.
    pub title: String,
    /// Расчётное время пути бегом (250 ед/с), секунды.
    pub seconds: f32,
    pub points: Vec<RoutePoint>,
}

impl Route {
    /// Длина ломаной по горизонтали, м.
    pub fn length(&self) -> f32 {
        self.points
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0].position(), w[1].position());
                Vec2::new(b.x - a.x, b.z - a.z).length()
            })
            .sum()
    }
}

/// Файл `data/routes.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteTable {
    pub routes: Vec<Route>,
}

impl RouteTable {
    pub fn get(&self, id: &str) -> Option<&Route> {
        self.routes.iter().find(|r| r.id == id)
    }
}

/// Файл `data/sounds.ron`: имя клипа → путь к `.wav`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundList {
    pub effects: Vec<(String, String)>,
}

/// Прочитать таблицу из подключённых ассетов (`None` и запись в лог, если её нет).
pub fn load_ron<T: serde::de::DeserializeOwned>(world: &World, path: &str) -> Option<T> {
    let database = world.resources.get::<AssetDatabase>()?;
    match database.load_data::<T>(path) {
        Ok(value) => Some(value),
        Err(error) => {
            log::error!("{path}: {error}");
            None
        }
    }
}
