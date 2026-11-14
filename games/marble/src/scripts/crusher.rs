use engine::prelude::*;

use crate::game::util;

component! {
    /// Пресс: поршень ждёт наверху, быстро бьёт вниз, держит и медленно поднимается. Шар
    /// под опускающимся поршнем раздавлен — игра вернёт его на контрольную точку
    /// («squashed»). Узел (без масштаба) — верхняя точка хода; голова (`head`, кинематический
    /// коллайдер) и штанга (`shaft`, только вид) — поля-ссылки на детей префаба: их можно
    /// переименовать в редакторе.
    pub struct Crusher {
        /// Голова-поршень (кинематический коллайдер).
        pub head: ObjectRef = ObjectRef::NONE,
        /// Штанга над головой (только вид).
        pub shaft: ObjectRef = ObjectRef::NONE,
        /// Ход поршня вниз, м.
        pub stroke: f32 = 3.0,
        /// Полный цикл, с.
        pub period: f32 = 3.6,
        /// Стоит наверху, с.
        pub wait_up: f32 = 1.6,
        /// Удар вниз, с.
        pub slam_time: f32 = 0.14,
        /// Держит внизу, с.
        pub hold_down: f32 = 0.5,
        /// Сдвиг цикла, 0..1.
        pub phase: f32 = 0.0,
        pub clock: f32 = 0.0,
        pub squashed_this_cycle: bool = false,
        pub thudded: bool = false,
        pub last_depth: f32 = 0.0,
    }
}

impl Crusher {
    /// Глубина хода 0 (вверху) .. 1 (внизу) и признак «бьёт» (голова идёт вниз или внизу).
    fn depth(&self) -> (f32, bool) {
        let cycle = self.period.max(self.wait_up + self.slam_time + self.hold_down + 0.2);
        let mut u = (self.clock + self.phase * cycle).rem_euclid(cycle);
        if u < self.wait_up {
            return (0.0, false);
        }
        u -= self.wait_up;
        if u < self.slam_time {
            return (smoothstep(u / self.slam_time), true);
        }
        u -= self.slam_time;
        if u < self.hold_down {
            return (1.0, true);
        }
        u -= self.hold_down;
        let rise = (cycle - self.wait_up - self.slam_time - self.hold_down).max(0.1);
        (1.0 - smoothstep(u / rise), false)
    }

    fn apply(&mut self, ctx: &mut Ctx, s: f32) {
        self.last_depth = s;
        let y = -self.stroke * s;
        let head = ctx.resolve(self.head);
        let shaft = ctx.resolve(self.shaft);
        let head_half = head.and_then(|h| ctx.scene().get(h)).map_or(0.5, |o| o.transform.scale.y * 0.5);
        let shaft_half = shaft.and_then(|h| ctx.scene().get(h)).map_or(1.0, |o| o.transform.scale.y * 0.5);
        if let Some(object) = head.and_then(|h| ctx.scene_mut().get_mut(h)) {
            object.transform.position = Vec3::new(0.0, y, 0.0);
        }
        if let Some(object) = shaft.and_then(|h| ctx.scene_mut().get_mut(h)) {
            object.transform.position = Vec3::new(0.0, y + head_half + shaft_half, 0.0);
        }
    }
}

impl Component for Crusher {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.apply(ctx, 0.0);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.clock += dt;
        let (s, _) = self.depth();
        self.apply(ctx, s);
        if s < 0.05 {
            self.squashed_this_cycle = false;
            self.thudded = false;
        }
        if s > 0.97 && !self.thudded {
            self.thudded = true;
            let at = ctx.world_position() - Vec3::Y * self.stroke;
            util::play_at(ctx, "thud", at, 1.0);
            util::shake(ctx, 0.28);
        }
    }

    /// Шар под головой, пока та идёт вниз или стоит внизу, раздавлен.
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.squashed_this_cycle || self.last_depth < 0.35 {
            return;
        }
        let (_, danger) = self.depth();
        if !danger {
            return;
        }
        let Some(head) = ctx.resolve(self.head) else { return };
        let center = ctx.scene().world_position(head);
        let half = ctx.scene().get(head).map_or(Vec3::splat(0.5), |o| o.transform.scale * 0.5);
        let Some(ball) = util::find_ball_near(ctx, center - Vec3::Y * (half.y + 0.4), half.x + half.z + 1.0) else { return };
        // Положение шара в осях головы (она повёрнута вместе с узлом).
        let ball_at = ctx.scene().world_position(ball);
        let d = util::to_local_axes(ctx.scene(), head, ball_at);
        if d.x.abs() < half.x + 0.3 && d.z.abs() < half.z + 0.3 && d.y + 0.5 > -half.y - 0.1 && d.y < half.y {
            self.squashed_this_cycle = true;
            util::play_at(ctx, "thud", center, 1.0);
            util::tell_game(ctx, "squashed", Value::Nil);
        }
    }
}
