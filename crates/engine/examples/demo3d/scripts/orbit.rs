//! Orbit — скрипт на Rust: камера на орбите вокруг точки, стрелки влево/вправо
//! вращают её. Висит на объекте «Main Camera» сцены `scenes/demo3d.scene`.
//!
//! Код вкомпилирован в программу: после правки нажмите Build (Ctrl+B) — редактор
//! пересоберёт пример и перезапустится с новым кодом, сцена и выделение останутся.

use engine::prelude::*;

component! {
    /// Камера на орбите вокруг точки: стрелки влево/вправо.
    pub struct Orbit {
        pub target: Vec3 = Vec3::new(0.0, 0.9, 0.0),
        pub distance: f32 = 9.5,
        pub height: f32 = 3.2,
        pub angle_deg: f32 = 0.0,
        /// Скорость поворота от клавиш, градусов в секунду.
        pub speed_deg: f32 = 60.0,
    }
}

impl Component for Orbit {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.angle_deg += ctx.input().axis("Horizontal") * self.speed_deg * dt;
        let angle = self.angle_deg.to_radians();
        let eye = self.target + Vec3::new(angle.sin() * self.distance, self.height, angle.cos() * self.distance);
        let target = self.target;
        let transform = ctx.transform();
        transform.position = eye;
        transform.look_at(target, Vec3::Y);
    }
}
