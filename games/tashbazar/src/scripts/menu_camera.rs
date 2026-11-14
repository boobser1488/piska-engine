use engine::prelude::*;

component! {
    /// Камера главного меню: медленно облетает карту по кругу и смотрит на центр.
    pub struct MenuCamera {
        /// Куда смотреть.
        pub center: Vec3 = Vec3::ZERO,
        pub radius: f32 = 55.0,
        pub height: f32 = 30.0,
        /// Градусов в секунду (реальное время: облёт идёт и на паузе).
        pub speed_deg: f32 = 3.0,
        /// Текущий угол, градусы.
        pub angle_deg: f32 = 200.0,
    }
}

impl MenuCamera {
    /// Поза камеры для угла `angle_deg`.
    pub fn pose(&self) -> (Vec3, Quat) {
        let a = self.angle_deg.to_radians();
        let position = self.center + Vec3::new(a.cos() * self.radius, self.height, a.sin() * self.radius);
        let look = (self.center - position).normalize_or_zero();
        let yaw = (-look.x).atan2(-look.z);
        let pitch = look.y.clamp(-1.0, 1.0).asin();
        (position, Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch))
    }
}

impl Component for MenuCamera {
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::UPDATE
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        let (position, rotation) = self.pose();
        let t = ctx.transform();
        t.position = position;
        t.rotation = rotation;
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        self.angle_deg = (self.angle_deg + self.speed_deg * ctx.time().unscaled_delta).rem_euclid(360.0);
        let (position, rotation) = self.pose();
        let t = ctx.transform();
        t.position = position;
        t.rotation = rotation;
    }
}
