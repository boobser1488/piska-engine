use engine::prelude::*;

component! {
    /// Платформа ездит между начальной точкой и точкой «начало + travel». Кинематическое
    /// тело: шар на ней едет за счёт трения. Двигаем в `on_update` по времени — движение
    /// плавное при любой частоте кадров, а физика берёт последнюю позу на каждом шаге.
    pub struct MovingPlatform {
        /// Смещение конечной точки от начальной, м.
        pub travel: Vec3 = Vec3::new(0.0, 0.0, -8.0),
        /// Полный цикл туда и обратно, с.
        pub period: f32 = 6.0,
        /// Стоянка в каждой крайней точке, с.
        pub pause: f32 = 0.8,
        /// Сдвиг цикла, 0..1.
        pub phase: f32 = 0.0,
        /// Начальная точка (запоминается при старте).
        pub origin: Vec3 = Vec3::ZERO,
        /// Прошедшее время, с.
        pub clock: f32 = 0.0,
    }
}

impl MovingPlatform {
    /// Положение вдоль пути, 0..1: стоим, едем, стоим, возвращаемся.
    pub fn progress(&self) -> f32 {
        let cycle = self.period.max(0.1);
        let half = cycle * 0.5;
        let hold = self.pause.min(half * 0.9);
        let u = (self.clock + self.phase * cycle).rem_euclid(cycle);
        let back = u >= half;
        let local = if back { u - half } else { u };
        let run = smoothstep(((local - hold * 0.5) / (half - hold).max(0.01)).clamp(0.0, 1.0));
        if back {
            1.0 - run
        } else {
            run
        }
    }
}

impl Component for MovingPlatform {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.origin = ctx.transform().position;
        ctx.transform().position = self.origin + self.travel * self.progress();
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.clock += dt;
        ctx.transform().position = self.origin + self.travel * self.progress();
    }
}
