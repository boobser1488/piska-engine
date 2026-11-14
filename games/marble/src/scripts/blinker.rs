use engine::prelude::*;

use crate::game::util;

component! {
    /// Лампа: яркость свечения (`MeshRenderer.emissive`) плавно пульсирует.
    pub struct Blinker {
        /// Цвет при полной яркости (ярче 1 — блум).
        pub color: Color = Color::rgb(3.0, 1.2, 0.2),
        /// Вспышек в секунду.
        pub rate: f32 = 0.8,
        /// Минимальная доля яркости.
        pub low: f32 = 0.15,
        /// Сдвиг, 0..1.
        pub phase: f32 = 0.0,
        pub clock: f32 = 0.0,
    }
}

impl Component for Blinker {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.clock += dt;
        let wave = 0.5 + 0.5 * ((self.clock * self.rate + self.phase) * std::f32::consts::TAU).sin();
        let k = self.low + (1.0 - self.low) * wave;
        let id = ctx.id;
        util::set_emissive(ctx.scene_mut(), id, Color::rgb(self.color.r * k, self.color.g * k, self.color.b * k));
    }
}
