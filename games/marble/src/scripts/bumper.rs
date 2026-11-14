use engine::prelude::*;

use crate::game::util;

component! {
    /// Бампер, как в пинболе: отталкивает шар от центра, подпрыгивает и вспыхивает.
    pub struct Bumper {
        /// Скорость шара от бампера после удара, м/с.
        pub kick: f32 = 9.0,
        /// Добавка вверх, м/с.
        pub lift_up: f32 = 1.5,
        /// Цвет вспышки (ярче 1 — даёт блум).
        pub glow: Color = Color::rgb(4.0, 1.4, 0.2),
        pub cooldown: f32 = 0.0,
        /// Масштаб в покое (запоминается при старте).
        pub rest_scale: Vec3 = Vec3::ONE,
    }
}

impl Component for Bumper {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.rest_scale = ctx.transform().scale;
    }

    fn on_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
    }

    fn on_collision_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if !util::is_ball(ctx.scene(), other) || self.cooldown > 0.0 {
            return;
        }
        self.cooldown = 0.15;
        let me = ctx.world_position();
        let mut away = ctx.scene().world_position(other) - me;
        away.y = 0.0;
        let away = away.normalize_or_zero();
        // Гасим скорость «в бампер» и выдаём новую вдоль нормали: шар не липнет и не гаснет.
        let into = util::velocity(ctx.scene(), other).dot(away);
        let delta = (self.kick - into).max(0.0);
        let lift = self.lift_up;
        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            let mass = physics.mass(other);
            physics.apply_impulse(other, (away * delta + Vec3::Y * lift) * mass);
        }
        util::play_at(ctx, "bumper", me, 0.9);

        // Пружинка: сжался и вернулся; вспышка гаснет.
        let id = ctx.id;
        ctx.start_tween(Tween::scale(id, self.rest_scale * 1.18).duration(0.1).ease(Ease::OutQuad).yoyo().loops(2));
        // Вспыхивает колпак — ребёнок «Cap».
        if let Some(cap) = ctx.scene().find_child(id, "Cap") {
            util::set_emissive(ctx.scene_mut(), cap, self.glow);
            ctx.start_tween(Tween::field(cap, "MeshRenderer.emissive", Color::rgb(0.15, 0.05, 0.0)).duration(0.3).ease(Ease::OutQuad));
        }
        util::shake(ctx, 0.12);
    }
}
