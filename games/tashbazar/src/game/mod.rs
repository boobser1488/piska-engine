//! Ядро игры без компонентов: настройки игрока, данные из ассетов (маршруты для ботов,
//! список звуков), события. Компоненты (скрипты) — в `src/scripts/`, по одному в файле.

pub mod data;
pub mod options;
pub mod sounds;

use engine::prelude::*;

/// Полоса препятствий пройдена (шлёт `MovementCourse`, читает `GameManager`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CourseFinished {
    /// Время круга, с.
    pub time: f32,
    /// Лучшее время до этого круга (0 — не было).
    pub previous_best: f32,
}

/// Поворот объекта → yaw контроллера (градусы; 0 — взгляд вдоль −Z).
pub fn yaw_of(rotation: Quat) -> f32 {
    let ahead = rotation * Vec3::NEG_Z;
    (-ahead.x).atan2(-ahead.z).to_degrees()
}

/// Время «м:сс.сс».
pub fn format_time(seconds: f32) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor();
    format!("{}:{:05.2}", minutes as u32, seconds - minutes * 60.0)
}
