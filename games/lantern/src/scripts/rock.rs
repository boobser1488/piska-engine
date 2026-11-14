//! Падающий сталактит босса.

use crate::common;
use crate::phys::Shared;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Сталактит босса: дрожит, потом падает.
    pub struct Rock {
        pub warn: f32 = 0.9,
        pub vy: f32 = 0.0,
        pub falling: bool = false,
        pub home_x: f32 = f32::NAN,
    }
}

impl Component for Rock {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let pos = ctx.world_position().truncate();
        if self.home_x.is_nan() {
            self.home_x = pos.x;
        }
        if !self.falling {
            self.warn -= dt;
            let jitter = (self.warn * 90.0).sin() * 0.07;
            ctx.transform().position = Vec3::new(self.home_x + jitter, pos.y, 0.0);
            if self.warn <= 0.0 {
                self.falling = true;
            }
            return;
        }
        self.vy = (self.vy - 34.0 * dt).max(-22.0);
        let new = Vec2::new(self.home_x, pos.y + self.vy * dt);
        ctx.transform().position = new.extend(0.0);
        let tip = new - Vec2::new(0.0, 2.0);
        let ground = ctx.resource::<Shared>().is_some_and(|s| s.grid.borrow().solid_at(tip));
        if let Some(p) = common::player_view(ctx) {
            if p.alive && !p.invulnerable && common::touching(p.center, p.half, new - Vec2::new(0.0, 1.0), Vec2::new(0.35, 1.0)) {
                ctx.send_message(p.id, "Hurt", Value::Vec3(if p.center.x > new.x { 1.0 } else { -1.0 }, 0.0, 0.0));
            }
        }
        if ground {
            common::dust(ctx, tip + Vec2::new(0.0, 0.2), 1.3);
            common::shake(ctx, 0.25);
            sfx::play(ctx, "land");
            ctx.destroy_self();
        }
    }
}
