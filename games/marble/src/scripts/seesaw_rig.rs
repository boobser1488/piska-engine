use engine::prelude::*;

component! {
    /// Доска качелей: вешает себя на шарнир по своей оси «вправо» (поперёк хода), когда
    /// физика уже знает о теле. Ось проходит через центр доски.
    pub struct SeesawRig {
        /// Затухание качания (угловое демпфирование шарнира задаёт `Rigidbody`).
        pub joined: bool = false,
    }
}

impl Component for SeesawRig {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let id = ctx.id;
        let anchor = ctx.world_position();
        let axis = ctx.scene().world_rotation(id) * Vec3::X;
        // Доска наклонена вокруг своей X-оси, поэтому её ось «вправо» совпадает с осью шарнира.
        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            physics.hinge_joint(id, None, anchor, axis);
            self.joined = true;
        }
    }
}
