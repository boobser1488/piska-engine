use engine::prelude::*;

use crate::game::util;

component! {
    /// Контрольная точка: триггер-ворота. Шар, проехавший сквозь них, запоминает место:
    /// после падения игра вернёт его сюда. Лампа (ребёнок «Lamp») меняет цвет с красного
    /// на зелёный.
    pub struct Checkpoint {
        /// Порядковый номер на цехе.
        pub index: i32 = 1,
        /// Куда смотрит камера после возврата сюда, градусы.
        pub yaw_deg: f32 = 0.0,
        pub reached: bool = false,
    }
}

impl Checkpoint {
    fn paint(ctx: &mut Ctx, on: bool) {
        let Some(lamp) = ctx.scene().find_child(ctx.id, "Lamp") else { return };
        let color = if on { Color::rgb(0.3, 3.0, 0.6) } else { Color::rgb(3.0, 0.4, 0.2) };
        util::set_emissive(ctx.scene_mut(), lamp, color);
    }
}

impl Component for Checkpoint {
    fn on_start(&mut self, ctx: &mut Ctx) {
        Self::paint(ctx, false);
    }

    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if self.reached || !util::is_ball(ctx.scene(), other) {
            return;
        }
        self.reached = true;
        Self::paint(ctx, true);
        let at = ctx.world_position();
        util::tell_game(ctx, "checkpoint_info", Value::Vec2(self.index as f32, self.yaw_deg));
        util::tell_game(ctx, "checkpoint", Value::Vec3(at.x, at.y, at.z));
        util::play_at(ctx, "chime", at, 0.9);
    }
}
