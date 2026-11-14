//! Декорации: слои параллакса, бесконечно «едущие» вместе с камерой.

use engine::prelude::*;

component! {
    /// Один из трёх спрайтов слоя фона: вместе они образуют бесшовную ленту шириной в три
    /// периода. `factor` — доля скорости камеры (0 — слой «приклеен» к экрану, 1 — стоит на
    /// месте, как земля).
    pub struct Parallax {
        pub factor: f32 = 0.2,
        /// Ширина одного спрайта (и период текстуры) в мировых единицах.
        pub period: f32 = 32.0,
        /// Номер спрайта в ленте: 0, 1, 2.
        pub slot: i32 = 0,
        /// Насколько слой следует за камерой по вертикали (0 — совсем не отстаёт).
        pub y_follow: f32 = 0.2,
        /// Середина карты по Y, вокруг которой камера считается «в покое».
        pub mid_y: f32 = 15.0,
        /// Самопрокрутка, единиц/с (меню).
        pub drift: f32 = 0.0,
        pub t: f32 = 0.0,
    }
}

impl Component for Parallax {
    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.t += dt;
        let cam = ctx
            .find_by_tag("MainCamera")
            .map(|c| ctx.scene().world_position(c).truncate())
            .unwrap_or(Vec2::ZERO);
        // Слой сдвигается на экране на `factor` от движения камеры: в мире он стоит в точке
        // cam.x·(1 − factor); лишнее отбрасываем целыми периодами (текстура бесшовная).
        let shift = (cam.x * self.factor + self.t * self.drift).rem_euclid(self.period);
        let x = cam.x - shift + (self.slot - 1) as f32 * self.period;
        let y = cam.y - self.y_follow * (cam.y - self.mid_y);
        ctx.transform().position = Vec3::new(x, y, 0.0);
    }
}
