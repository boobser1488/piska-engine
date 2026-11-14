use engine::prelude::*;

component! {
    /// Вращающаяся «метла»: узел крутится вокруг вертикальной оси, плечи — его дети с
    /// кинематическими коллайдерами (`SweeperArm`). `swing_deg > 0` — не вертится по
    /// кругу, а качается.
    pub struct Sweeper {
        /// Градусов в секунду (при качании — скорость фазы).
        pub speed_deg: f32 = 50.0,
        /// 0 — полный оборот; иначе размах ± в градусах.
        pub swing_deg: f32 = 0.0,
        /// Начальный угол.
        pub phase_deg: f32 = 0.0,
        /// Начальный поворот узла, градусы Эйлера (запоминается при старте).
        pub base: Vec3 = Vec3::ZERO,
        pub angle: f32 = 0.0,
    }
}

impl Sweeper {
    fn apply(&self, ctx: &mut Ctx) {
        let a = if self.swing_deg > 0.0 { self.swing_deg * self.angle.to_radians().sin() } else { self.angle };
        ctx.transform().set_euler_deg(Vec3::new(self.base.x, self.base.y + a, self.base.z));
    }
}

impl Component for Sweeper {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.base = ctx.transform().euler_deg();
        self.angle = self.phase_deg;
        self.apply(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.angle += self.speed_deg * dt;
        if self.angle.abs() > 36000.0 {
            self.angle %= 360.0;
        }
        self.apply(ctx);
    }
}
