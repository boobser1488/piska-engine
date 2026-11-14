//! Платформа, ходящая между двумя точками.

use crate::phys::{Plat, Shared};
use engine::prelude::*;

component! {
    /// Платформа, ходящая между двумя точками (точка — середина верхней грани).
    pub struct MovingPlatform {
        pub from: Vec2 = Vec2::ZERO,
        pub to: Vec2 = Vec2::ZERO,
        /// Скорость фазы, рад/с (полный цикл туда-обратно — 2π / speed).
        pub speed: f32 = 1.0,
        pub t: f32 = 0.0,
        pub width: f32 = 3.0,
    }
}

impl MovingPlatform {
    fn position(&self) -> Vec2 {
        self.from.lerp(self.to, 0.5 - 0.5 * self.t.cos())
    }
}

impl Component for MovingPlatform {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let p = self.position();
        ctx.transform().position = p.extend(0.0);
        self.publish(ctx, Vec2::ZERO);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let before = self.position();
        self.t += dt * self.speed;
        let now = self.position();
        ctx.transform().position = now.extend(0.0);
        self.publish(ctx, now - before);
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.retract(ctx.id);
        }
    }
}

impl MovingPlatform {
    fn publish(&self, ctx: &mut Ctx, delta: Vec2) {
        let p = self.position();
        let half = self.width * 0.5;
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.publish(Plat { id: ctx.id, min: Vec2::new(p.x - half, p.y - 0.5), max: Vec2::new(p.x + half, p.y), delta });
        }
    }
}
