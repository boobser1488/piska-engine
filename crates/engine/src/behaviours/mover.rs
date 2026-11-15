use engine_core::prelude::*;

component! {
    /// Едет с постоянной скоростью (единиц в секунду по X, Y, Z) — облака, пули, ленты.
    pub struct Mover {
        /// Скорость, единиц в секунду.
        pub velocity: Vec3 = Vec3::new(1.0, 0.0, 0.0),
    }
}

impl Component for Mover {
    fn update(&mut self, ctx: &mut Ctx) {
        ctx.move_by(self.velocity * ctx.dt());
    }
}

component! {
    /// Вращается: градусов в секунду вокруг X, Y и Z (для 2D — только Z).
    pub struct Rotator {
        /// Градусов в секунду вокруг каждой оси.
        pub degrees_per_second: Vec3 = Vec3::new(0.0, 0.0, 90.0),
    }
}

impl Component for Rotator {
    fn update(&mut self, ctx: &mut Ctx) {
        ctx.rotate_by(self.degrees_per_second * ctx.dt());
    }
}
