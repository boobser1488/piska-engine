//! Контроллер от первого лица с классической («квейковской») физикой движения.
//! Нужна возможность `physics3d`. Руководство — docs/api/fps.md.
//!
//! * [`FpsController`] — компонент игрока: ноги — позиция объекта, дочерняя `Camera` — глаза.
//! * [`movement`] — само движение без компонентов: [`player_move`], [`MoveSettings`],
//!   [`MoveState`], [`UserCmd`]; его можно гонять в тестах и для ботов.
//! * [`FpsRuntime`] — ресурс со скрытым состоянием контроллеров; через него боты и тесты
//!   ведут игрока командами (`set_command`, `set_driver`).
//!
//! Захват курсора для взгляда мышью — `Input::set_cursor_captured(true)`.

mod controller;
pub mod movement;

pub use controller::{horizontal_fov_deg, vertical_fov_deg, FpsController, FpsRuntime};
pub use movement::{player_move, HullWorld, MoveSettings, MoveState, PhysicsHullWorld, UserCmd, SKIN, UNIT};

/// Перевести классические единицы в метры (`units(250.0)` = 4.7625).
pub fn units(value: f32) -> f32 {
    value * UNIT
}

/// Перевести метры в классические единицы.
pub fn to_units(metres: f32) -> f32 {
    metres / UNIT
}

/// Зарегистрировать компонент (это делает `engine::app()`).
pub fn register(registry: &mut engine_core::reflect::TypeRegistry) {
    registry.register::<FpsController>();
    registry.categorize(&["FpsController"], "Physics 3D");
}
