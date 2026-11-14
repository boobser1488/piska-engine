//! Светлячок: дрейфует, тянется к игроку и засчитывается при касании.

use crate::common;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Светлячок: дрейфует, тянется к игроку вблизи, при касании засчитывается.
    pub struct Firefly {
        /// Золотой (секретный) — особый звук и счёт.
        pub golden: bool = false,
        pub home: Vec2 = Vec2::ZERO,
        pub phase: f32 = 0.0,
        pub t: f32 = 0.0,
    }
}

impl Component for Firefly {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.home = ctx.world_position().truncate();
        self.phase = self.home.x * 1.7 + self.home.y * 0.9;
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.t += dt;
        let player = common::player_view(ctx);
        if let Some(p) = player {
            let to = p.center - self.home;
            let d = to.length();
            let reach = if self.golden { 3.2 } else { 2.4 };
            if p.alive && d < reach && d > 0.001 {
                self.home += to / d * (3.0 + (reach - d) * 2.0) * dt;
            }
        }
        let pos = self.home + Vec2::new((self.t * 0.9 + self.phase).sin() * 0.3, (self.t * 1.3 + self.phase * 1.3).cos() * 0.24);
        ctx.transform().position = pos.extend(0.0);
        if let Some(p) = player {
            let radius = if self.golden { 0.95 } else { 0.8 };
            if p.alive && (p.center - pos).length() < radius && common::level_state(ctx).is_none_or(|l| l.2) {
                if self.golden {
                    sfx::play(ctx, "golden");
                    common::sparkle(ctx, pos, Color::rgb(3.4, 2.2, 0.5));
                    common::shake(ctx, 0.2);
                    common::to_level(ctx, "golden", Value::Nil);
                } else {
                    sfx::play(ctx, "firefly");
                    common::sparkle(ctx, pos, Color::rgb(2.6, 2.6, 1.2));
                    common::to_level(ctx, "firefly", Value::Nil);
                }
                ctx.destroy_self();
            }
        }
    }
}
