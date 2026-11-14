use engine::prelude::*;

component! {
    /// Подвижная платформа: ездит туда-обратно между начальной позицией и `start + travel`
    /// с плавными разворотами и может вращаться вокруг вертикали. Нужны `Rigidbody`
    /// (Kinematic) и `Collider` — игрок на ней едет вместе с ней.
    pub struct MovingPlatform {
        /// Смещение второй точки, м.
        pub travel: Vec3 = Vec3::new(0.0, 0.0, 6.0),
        /// Секунд на путь в одну сторону.
        pub period: f32 = 4.0,
        /// Вращение, градусов в секунду.
        pub spin_deg: f32 = 0.0,
        /// Начальная позиция (запоминается при старте).
        pub start: Vec3 = Vec3::ZERO,
        pub time: f32 = 0.0,
    }
}

impl Component for MovingPlatform {
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::FIXED_UPDATE
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        self.start = ctx.transform().position;
    }

    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.time += dt;
        let period = self.period.max(0.1);
        let phase = (self.time / period).rem_euclid(2.0);
        let s = if phase < 1.0 { phase } else { 2.0 - phase };
        let eased = s * s * (3.0 - 2.0 * s);
        let (start, travel, spin) = (self.start, self.travel, self.spin_deg);
        let t = ctx.transform();
        t.position = start + travel * eased;
        if spin != 0.0 {
            t.rotation = Quat::from_rotation_y((spin * dt).to_radians()) * t.rotation;
        }
    }
}
