//! Реактор в центре арены: внешний вид (пульсация, свечение при попадании, краснота при
//! низкой прочности). Прочность и радиус — в `data/balance.ron`.

use engine::prelude::*;

component! {
    /// Реактор: настройки анимации. Висит на объекте `ReactorCore` (префаб
    /// `prefabs/reactor_core.prefab`).
    pub struct ReactorCore {
        /// Скорость пульсации, рад/с.
        pub pulse_speed: f32 = 2.2,
        /// Размах пульсации (доля размера).
        pub pulse_amount: f32 = 0.025,
        /// Добавка к размеру в момент попадания.
        pub hit_pulse: f32 = 0.06,
        /// Скорость вращения внешнего кольца, рад/с.
        pub ring_spin: f32 = 0.45,
        /// Цвет вспышки при попадании.
        pub hit_color: Color = Color::rgb(2.6, 1.6, 1.4),
        /// Внешнее кольцо, которое вращается (ребёнок префаба).
        pub ring: ObjectRef = ObjectRef::NONE,
    }
}

impl Component for ReactorCore {}
