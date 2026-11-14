use engine::prelude::*;

use crate::game::util;

component! {
    /// Ускоритель: плита-триггер толкает шар вдоль «вперёд» (−Z узла) до скорости `speed`.
    /// Односторонний: шар, едущий уже быстрее, не тормозится.
    pub struct Booster {
        /// Целевая скорость вдоль плиты, м/с.
        pub speed: f32 = 13.0,
        /// Небольшой подскок, м/с — чтобы не цепляться за стыки.
        pub pop: f32 = 1.2,
        pub cooldown: f32 = 0.0,
    }
}

impl Component for Booster {
    fn on_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
    }

    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if self.cooldown > 0.0 || !util::is_ball(ctx.scene(), other) {
            return;
        }
        self.cooldown = 0.5;
        let along = ctx.scene().world_rotation(ctx.id) * Vec3::NEG_Z;
        let have = util::velocity(ctx.scene(), other).dot(along);
        let delta = (self.speed - have).max(0.0);
        let pop = self.pop;
        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            let mass = physics.mass(other);
            physics.apply_impulse(other, (along * delta + Vec3::Y * pop) * mass);
        }
        let at = ctx.world_position();
        util::play_at(ctx, "boost", at, 0.9);
        util::shake(ctx, 0.1);
        if let Some(arrows) = ctx.scene().find_child(ctx.id, "Arrows") {
            util::set_emissive(ctx.scene_mut(), arrows, Color::rgb(3.0, 2.4, 0.4));
            ctx.start_tween(Tween::field(arrows, "MeshRenderer.emissive", Color::rgb(0.6, 0.45, 0.05)).duration(0.6).ease(Ease::OutQuad));
        }
    }
}
