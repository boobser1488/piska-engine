//! Хрупкая платформа.

use crate::common;
use crate::phys::{Plat, Shared};
use crate::sfx;
use engine::prelude::*;

component! {
    /// Хрупкая платформа: через полсекунды после того, как на неё встали, рассыпается,
    /// через пару секунд собирается снова.
    pub struct CrumblePlatform {
        /// 0 — цела, 1 — трескается, 2 — рассыпалась.
        pub state: i32 = 0,
        pub timer: f32 = 0.0,
        pub home: Vec2 = Vec2::ZERO,
        pub width: f32 = 3.0,
    }
}

impl CrumblePlatform {
    fn publish(&self, ctx: &mut Ctx) {
        let half = self.width * 0.5;
        let p = self.home;
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.publish(Plat { id: ctx.id, min: Vec2::new(p.x - half, p.y - 0.5), max: Vec2::new(p.x + half, p.y), delta: Vec2::ZERO });
        }
    }
}

const CRUMBLE_DELAY: f32 = 0.55;
const CRUMBLE_RESPAWN: f32 = 2.8;

impl Component for CrumblePlatform {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.home = ctx.world_position().truncate();
        self.publish(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        match self.state {
            0 => {
                if let Some(p) = common::player_view(ctx) {
                    if p.platform == ctx.id.to_bits() as i64 {
                        self.state = 1;
                        self.timer = CRUMBLE_DELAY;
                        sfx::play(ctx, "crumble");
                    }
                }
            }
            1 => {
                self.timer -= dt;
                let k = 1.0 - (self.timer / CRUMBLE_DELAY).clamp(0.0, 1.0);
                let jitter = (self.timer * 70.0).sin() * 0.05 * k;
                ctx.transform().position = (self.home + Vec2::new(jitter, 0.0)).extend(0.0);
                if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                    sprite.frame = ((k * 3.0) as u32).min(2);
                }
                if self.timer <= 0.0 {
                    self.state = 2;
                    self.timer = CRUMBLE_RESPAWN;
                    if let Some(shared) = ctx.resource::<Shared>() {
                        shared.retract(ctx.id);
                    }
                    if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                        sprite.color = Color::TRANSPARENT;
                    }
                    for dx in [-1.0, 0.0, 1.0] {
                        common::dust(ctx, self.home + Vec2::new(dx, -0.3), 1.0);
                    }
                    common::shake(ctx, 0.15);
                }
            }
            _ => {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.state = 0;
                    ctx.transform().position = self.home.extend(0.0);
                    if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                        sprite.color = Color::WHITE;
                        sprite.frame = 0;
                    }
                    self.publish(ctx);
                    common::sparkle(ctx, self.home, Color::rgb(1.6, 1.6, 2.4));
                }
            }
        }
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.retract(ctx.id);
        }
    }
}
