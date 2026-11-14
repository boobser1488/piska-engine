//! Летучая мышь.

use crate::combat::{contact, resolve, set_flip};
use crate::common;
use engine::prelude::*;

component! {
    /// Летучая мышь: летает по синусоиде вокруг точки привязки.
    pub struct Bat {
        pub anchor: Vec2 = Vec2::ZERO,
        pub range: f32 = 3.0,
        pub amp: f32 = 1.5,
        pub speed: f32 = 1.0,
        pub t: f32 = 0.0,
    }
}

const BAT_HALF: Vec2 = Vec2::new(0.4, 0.3);

impl Component for Bat {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.anchor = ctx.world_position().truncate();
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.t += dt * self.speed;
        let before = ctx.world_position().x;
        let pos = self.anchor + Vec2::new((self.t * 0.8).sin() * self.range, (self.t * 1.9).sin() * self.amp);
        ctx.transform().position = pos.extend(0.0);
        if (pos.x - before).abs() > 1e-4 {
            set_flip(ctx, pos.x < before);
        }
        if let Some(p) = common::player_view(ctx) {
            if let Some(hit) = contact(&p, pos, BAT_HALF, true) {
                resolve(ctx, &p, hit, pos, Color::rgb(2.0, 0.6, 2.6));
            }
        }
    }
}
