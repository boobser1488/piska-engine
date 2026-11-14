//! Тяжеляк: шипастая глыба.

use crate::combat::{contact, patrol, resolve, set_flip, shared, Gait};
use crate::common;
use engine::prelude::*;

component! {
    /// Тяжеляк: шипастая глыба. Раздавить нельзя — обойти или пробежать рывком.
    pub struct Brute {
        pub dir: f32 = -1.0,
        pub vy: f32 = 0.0,
        pub speed: f32 = 1.3,
        pub home_x: f32 = f32::NAN,
        pub range: f32 = 0.0,
    }
}

const BRUTE_HALF: Vec2 = Vec2::new(0.62, 0.6);

impl Component for Brute {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let dt = dt.min(1.0 / 30.0);
        let Some(shared) = shared(ctx) else { return };
        if self.home_x.is_nan() {
            self.home_x = ctx.world_position().x;
        }
        let limit = (self.range > 0.0).then_some((self.home_x - self.range, self.home_x + self.range));
        let feet = patrol(ctx, &shared, &mut self.dir, &mut self.vy, Gait { speed: self.speed, half: BRUTE_HALF, limit }, dt);
        set_flip(ctx, self.dir < 0.0);
        let center = feet + Vec2::new(0.0, BRUTE_HALF.y);
        if let Some(p) = common::player_view(ctx) {
            if let Some(hit) = contact(&p, center, BRUTE_HALF, false) {
                resolve(ctx, &p, hit, center, Color::WHITE);
            }
        }
    }
}
