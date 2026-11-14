use engine::prelude::*;

use crate::game::util;

component! {
    /// Вентилятор: объёмная сила вдоль «вверх» узла. Тело внутри столба получает
    /// ускорение `lift` (больше g — поднимается, меньше — зависает) и мягко подтягивается
    /// к оси столба. Ротор (`rotor`, ссылка на ребёнка префаба) крутится.
    pub struct Fan {
        /// Ротор, который крутится.
        pub rotor: ObjectRef = ObjectRef::NONE,
        /// Ускорение у основания, м/с² (гравитация игры — 15).
        pub lift: f32 = 26.0,
        /// На сколько ослабевает к верхнему краю, 0..1.
        pub falloff: f32 = 0.45,
        /// Высота столба, м.
        pub height: f32 = 5.5,
        /// Радиус столба, м.
        pub radius: f32 = 1.7,
        /// Скорость ротора, град/с.
        pub spin_deg: f32 = 540.0,
    }
}

impl Component for Fan {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let base = ctx.world_position();
        let up = ctx.scene().world_rotation(ctx.id) * Vec3::Y;
        let mid = base + up * (self.height * 0.5);
        let reach = (self.height * self.height * 0.25 + self.radius * self.radius).sqrt() + 0.5;
        let Some(physics) = ctx.resource::<Physics3D>() else { return };
        let mut pushes = Vec::new();
        for body in physics.overlap_sphere(mid, reach) {
            if !util::has_body(ctx.scene(), body) {
                continue;
            }
            let rel = ctx.scene().world_position(body) - base;
            let h = rel.dot(up);
            if h < -0.3 || h > self.height {
                continue;
            }
            let lateral = rel - up * h;
            if lateral.length() > self.radius {
                continue;
            }
            let strength = self.lift * (1.0 - self.falloff * (h / self.height).clamp(0.0, 1.0));
            // Мягкая «пружина» к оси: шар не вылетает из столба сразу.
            let pull = lateral * -2.5;
            pushes.push((body, (up * strength + pull) * physics.mass(body)));
        }
        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            for (body, force) in pushes {
                physics.add_force(body, force);
            }
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(rotor) = ctx.resolve(self.rotor) else { return };
        if let Some(object) = ctx.scene_mut().get_mut(rotor) {
            object.transform.rotate_y((self.spin_deg * dt).to_radians());
        }
    }
}
