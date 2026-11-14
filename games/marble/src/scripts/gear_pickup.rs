use engine::prelude::*;

use crate::game::util;

component! {
    /// Шестерёнка: вращается и покачивается; шар, коснувшийся её триггера, забирает её.
    /// Золотая (одна на цех) — отдельное сообщение, игра считает её отдельно.
    pub struct GearPickup {
        /// Золотая шестерня цеха.
        pub golden: bool = false,
        /// Вращение, град/с.
        pub spin_deg: f32 = 120.0,
        /// Покачивание, м.
        pub bob_height: f32 = 0.12,
        /// Исходная высота (запоминается при старте).
        pub origin: Vec3 = Vec3::ZERO,
        pub clock: f32 = 0.0,
        pub taken: bool = false,
    }
}

impl Component for GearPickup {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.origin = ctx.transform().position;
        self.clock = ctx.random().range(0.0, std::f32::consts::TAU);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.clock += dt;
        let spin = (self.spin_deg * dt).to_radians();
        let bob = (self.clock * 2.2).sin() * self.bob_height;
        let transform = ctx.transform();
        transform.rotate_y(spin);
        transform.position = self.origin + Vec3::Y * bob;
    }

    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if self.taken || !util::is_ball(ctx.scene(), other) {
            return;
        }
        self.taken = true;
        let at = ctx.world_position();
        util::tell_game(ctx, if self.golden { "golden_gear" } else { "gear" }, Value::Vec3(at.x, at.y, at.z));
        ctx.destroy_self();
    }
}
