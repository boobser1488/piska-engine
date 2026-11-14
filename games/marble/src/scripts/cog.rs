use engine::prelude::*;

component! {
    /// Шестерня-декорация: вращение вокруг одной из локальных осей (0 — X, 1 — Y, 2 — Z).
    pub struct Cog {
        /// Градусов в секунду.
        pub speed_deg: f32 = 12.0,
        /// Ось вращения.
        pub axis: i32 = 1,
    }
}

impl Component for Cog {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let step = (self.speed_deg * dt).to_radians();
        let transform = ctx.transform();
        let turn = match self.axis {
            0 => Quat::from_rotation_x(step),
            2 => Quat::from_rotation_z(step),
            _ => Quat::from_rotation_y(step),
        };
        transform.rotation *= turn;
    }
}
