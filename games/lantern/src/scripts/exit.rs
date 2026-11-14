//! Выход уровня.

use crate::common;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Выход: загорается, когда собрано нужное число светлячков (и побеждён босс, если он есть).
    pub struct Exit {
        pub active: bool = false,
        pub used: bool = false,
        pub nag: f32 = 0.0,
    }
}

impl Component for Exit {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.nag = (self.nag - dt).max(0.0);
        let Some((_, ready, playing)) = common::level_state(ctx) else { return };
        if ready != self.active {
            self.active = ready;
            if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                a.play(if ready { "on" } else { "off" });
            }
            if let Some(glow) = ctx.children().first().copied() {
                if let Some(ps) = ctx.scene_mut().get_component_mut::<ParticleSystem>(glow) {
                    ps.playing = ready;
                }
            }
            if ready {
                sfx::play(ctx, "checkpoint");
                let at = ctx.world_position().truncate() + Vec2::new(0.0, 1.0);
                common::sparkle(ctx, at, Color::rgb(3.0, 2.2, 0.8));
            }
        }
        let Some(p) = common::player_view(ctx) else { return };
        let pos = ctx.world_position().truncate();
        if p.alive && playing && common::touching(p.center, p.half, pos + Vec2::new(0.0, 1.0), Vec2::new(0.6, 1.0)) {
            if self.active && !self.used {
                self.used = true;
                common::to_level(ctx, "exit_reached", Value::Nil);
            } else if !self.active && self.nag <= 0.0 {
                self.nag = 2.5;
                sfx::play(ctx, "error");
                common::to_level(ctx, "exit_locked", Value::Nil);
            }
        }
    }
}
