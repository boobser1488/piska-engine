//! Плеватель.

use crate::combat::{contact, resolve, set_flip};
use crate::common;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Плеватель: стоит на месте и плюётся в игрока по таймеру; раздавить можно.
    pub struct Spitter {
        pub timer: f32 = 1.5,
        pub period: f32 = 2.6,
        pub windup: f32 = 0.0,
        pub range: f32 = 13.0,
    }
}

const SPITTER_HALF: Vec2 = Vec2::new(0.35, 0.6);

impl Component for Spitter {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let feet = ctx.world_position().truncate();
        let center = feet + Vec2::new(0.0, SPITTER_HALF.y);
        let player = common::player_view(ctx);
        if let Some(p) = player {
            set_flip(ctx, p.center.x < center.x);
            let d = p.center - center;
            let in_range = d.x.abs() < self.range && d.y.abs() < 7.0 && p.alive;
            if self.windup > 0.0 {
                self.windup -= dt;
                if self.windup <= 0.0 {
                    let dir = (p.center - center).normalize_or_zero();
                    let mouth = center + Vec2::new(0.35 * dir.x.signum(), 0.35);
                    common::fire_projectile(ctx, mouth, dir * 5.5, 1.0);
                    sfx::play(ctx, "shoot");
                    if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                        a.restart("shoot");
                    }
                }
            } else if in_range {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.timer = self.period;
                    self.windup = 0.4;
                    sfx::play(ctx, "warn");
                    if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                        a.restart("windup");
                    }
                }
            }
            if self.windup <= 0.0 && self.timer < self.period - 0.35 {
                if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                    a.play("idle");
                }
            }
            if let Some(hit) = contact(&p, center, SPITTER_HALF, true) {
                resolve(ctx, &p, hit, center, Color::rgb(0.8, 2.4, 1.4));
            }
        }
    }
}
