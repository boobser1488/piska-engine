//! Ресурсы, которые делят шар и камера: вид (куда смотрит камера), состояние шара, голос
//! гула качения. Живут в `world.resources` и переживают смену сцены.

use engine::prelude::*;

/// Гравитация игры, м/с² (в `project.settings`: `physics3d.gravity`). Сильнее земной: шар
/// быстрее приземляется и меньше «плавает». Нужна коду только для проверок и подсказок.
pub const GRAVITY: f32 = 15.0;
/// Радиус шара.
pub const BALL_RADIUS: f32 = 0.5;

/// Вид, общий для шара и камеры: куда смотрит камера (от этого зависит «вперёд» для
/// управления), её сглаженный фокус и тряска.
#[derive(Clone, Debug)]
pub struct ViewState {
    /// Рыскание камеры, радианы: 0 — смотрит вдоль −Z; плюс — поворот налево.
    pub yaw: f32,
    /// Угол места камеры над горизонтом, радианы.
    pub pitch: f32,
    /// Желаемое расстояние (колесо мыши меняет), метры.
    pub distance: f32,
    /// Чувствительность вращения, 0.3..2.
    pub sensitivity: f32,
    /// Тряска камеры 0..1 (квадрат даёт амплитуду).
    pub trauma: f32,
    /// Шар, за которым следим.
    pub ball: Option<GameObjectId>,
    /// Сглаженная точка, на которую смотрит камера.
    pub focus: Vec3,
    /// Сглаженное расстояние (после подтягивания у стен).
    pub shown_distance: f32,
    /// Следующий кадр камеры не сглаживает, а прыгает к цели (старт, респаун).
    pub snap: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.6,
            distance: 9.5,
            sensitivity: 1.0,
            trauma: 0.0,
            ball: None,
            focus: Vec3::ZERO,
            shown_distance: 9.5,
            snap: true,
        }
    }
}

/// Голос гула качения — живёт, пока идёт цех.
#[derive(Default)]
pub struct RollVoice {
    pub voice: Option<VoiceId>,
}

/// Что шар сообщает «наружу» за кадр (для HUD и тестов).
#[derive(Clone, Copy, Debug, Default)]
pub struct BallStatus {
    pub speed: f32,
    pub grounded: bool,
    pub jumps: u32,
}
