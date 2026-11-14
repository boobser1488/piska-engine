use engine::prelude::*;

use crate::game::util;

component! {
    /// Плечо метлы: звонкий удар, когда оно задевает шар. Висит на детях узла `Sweeper`.
    pub struct SweeperArm {
        pub volume: f32 = 0.8,
        pub cooldown: f32 = 0.0,
    }
}

impl Component for SweeperArm {
    fn on_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
    }

    fn on_collision_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if !util::is_ball(ctx.scene(), other) || self.cooldown > 0.0 {
            return;
        }
        self.cooldown = 0.25;
        let at = ctx.scene().world_position(other);
        util::play_at(ctx, "clang", at, self.volume);
        util::shake(ctx, 0.1);
    }
}
