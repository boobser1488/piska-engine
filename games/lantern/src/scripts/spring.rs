//! Пружина: подбрасывает игрока.

use crate::common;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Пружина: подбрасывает игрока.
    pub struct Spring {
        pub power: f32 = 20.0,
        pub anim: f32 = 0.0,
    }
}

impl Component for Spring {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.anim = (self.anim - dt).max(0.0);
        let frame = if self.anim > 0.0 { 1 } else { 0 };
        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            sprite.frame = frame;
        }
        let Some(p) = common::player_view(ctx) else { return };
        let feet = ctx.world_position().truncate();
        if p.alive && self.anim <= 0.0 && p.vel.y <= 1.0 && common::touching(p.center, p.half, feet + Vec2::new(0.0, 0.3), Vec2::new(0.5, 0.3)) {
            self.anim = 0.3;
            ctx.send_message(p.id, "Bounce", Value::Float(self.power));
            sfx::play(ctx, "spring");
            common::dust(ctx, feet, 0.7);
        }
    }
}
