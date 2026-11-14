use engine::prelude::*;

component! {
    /// Фонарь: свет чуть дрожит, как огонь за стеклом. Стоит рядом с `Light`.
    pub struct LanternFlicker {
        /// Яркость без дрожи.
        pub intensity: f32 = 2.0,
        /// Доля дрожи (0 — ровный свет).
        pub amount: f32 = 0.12,
        pub speed: f32 = 7.0,
        /// Сдвиг фазы, чтобы фонари дрожали вразнобой.
        pub phase: f32 = 0.0,
    }
}

impl Component for LanternFlicker {
    fn hooks(&self) -> Hooks {
        Hooks::UPDATE
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let t = ctx.time().elapsed as f32 * self.speed + self.phase;
        let wobble = (t.sin() * 0.6 + (t * 2.31).sin() * 0.3 + (t * 5.7).sin() * 0.1) * self.amount;
        let intensity = self.intensity * (1.0 + wobble);
        if let Some(light) = ctx.get_mut::<Light>() {
            light.intensity = intensity;
        }
    }
}
