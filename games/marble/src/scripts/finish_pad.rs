use engine::prelude::*;

use crate::game::util;

component! {
    /// Площадка отгрузки: шар, въехавший в триггер, завершает цех. Маяк (ребёнок
    /// «Beacon») вращается и мигает.
    pub struct FinishPad {
        /// Скорость вращения маяка, град/с.
        pub beacon_deg: f32 = 180.0,
        pub done: bool = false,
        pub clock: f32 = 0.0,
    }
}

impl Component for FinishPad {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.clock += dt;
        let Some(beacon) = ctx.scene().find_child(ctx.id, "Beacon") else { return };
        let turn = (self.beacon_deg * dt).to_radians();
        let k = 1.6 + 1.4 * (0.5 + 0.5 * (self.clock * 5.0).sin());
        if let Some(object) = ctx.scene_mut().get_mut(beacon) {
            object.transform.rotate_y(turn);
        }
        util::set_emissive(ctx.scene_mut(), beacon, Color::rgb(0.2 * k, k, 0.5 * k));
    }

    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if self.done || !util::is_ball(ctx.scene(), other) {
            return;
        }
        self.done = true;
        let at = ctx.world_position();
        util::play_at(ctx, "chime", at, 1.0);
        util::tell_game(ctx, "finish", Value::Nil);
    }
}
