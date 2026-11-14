//! Контрольная точка: касание запоминает место возрождения.

use crate::common;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Фонарный столб: касание запоминает место возрождения и лечит на сердце.
    pub struct Checkpoint {
        pub active: bool = false,
    }
}

impl Component for Checkpoint {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.active {
            return;
        }
        let Some(p) = common::player_view(ctx) else { return };
        let feet = ctx.world_position().truncate();
        if p.alive && common::touching(p.center, p.half, feet + Vec2::new(0.0, 0.75), Vec2::new(0.6, 0.9)) {
            self.active = true;
            if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                sprite.frame = 1;
            }
            sfx::play(ctx, "checkpoint");
            common::sparkle(ctx, feet + Vec2::new(0.0, 0.9), Color::rgb(3.0, 2.2, 0.7));
            common::to_level(ctx, "checkpoint", Value::Vec3(feet.x, feet.y, 0.0));
            ctx.send_message(p.id, "Heal", Value::Nil);
        }
    }
}
