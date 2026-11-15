use engine_core::prelude::*;

component! {
    /// Игрок с видом сверху: ходит по осям `Horizontal` / `Vertical` (WASD, стрелки,
    /// крестовина, стик). С `Rigidbody2D` (гравитация 0) задаёт скорость тела и упирается
    /// в стены, без него — просто двигает объект.
    pub struct PlayerMove2D {
        /// Скорость, единиц в секунду.
        pub speed: f32 = 5.0,
    }
}

impl Component for PlayerMove2D {
    fn update(&mut self, ctx: &mut Ctx) {
        let dir = ctx.axis2("Horizontal", "Vertical");
        #[cfg(feature = "physics2d")]
        if ctx.has::<engine_physics::Rigidbody2D>() {
            use engine_physics::PhysicsCtx;
            ctx.set_velocity(dir * self.speed);
            return;
        }
        ctx.move_by(dir * self.speed * ctx.dt());
    }
}
