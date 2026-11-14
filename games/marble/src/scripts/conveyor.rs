use engine::prelude::*;

use crate::game::util;

component! {
    /// Конвейер: тянет всё, что лежит на ленте, вдоль «вперёд» узла (−Z). Узел не
    /// масштабируется; лента (коллайдер `Belt`) и бегущие планки (`Slat …`) — его дети.
    /// Скорость поддерживается силой с пределом ускорения 9 м/с² (меньше, чем у шара):
    /// против ленты идти можно, но трудно.
    pub struct Conveyor {
        /// м/с вдоль ленты; минус — задом наперёд.
        pub speed: f32 = 4.0,
        /// Как быстро тело подтягивается к скорости ленты.
        pub grip: f32 = 12.0,
        /// Габариты ленты: ширина, толщина, длина.
        pub size: Vec3 = Vec3::new(4.0, 0.4, 10.0),
        /// Расстояние между бегущими планками.
        pub slat_spacing: f32 = 1.2,
        /// Пройденный лентой путь (для анимации планок).
        pub scroll: f32 = 0.0,
    }
}

impl Component for Conveyor {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let center = ctx.world_position();
        let reach = 0.5 * (self.size.x * self.size.x + self.size.z * self.size.z).sqrt() + 1.5;
        let along = ctx.scene().world_rotation(ctx.id) * Vec3::NEG_Z;
        let Some(physics) = ctx.resource::<Physics3D>() else { return };
        let mut pushes = Vec::new();
        for body in physics.overlap_sphere(center, reach) {
            if !util::has_body(ctx.scene(), body) {
                continue;
            }
            let at = ctx.scene().world_position(body);
            let local = util::to_local_axes(ctx.scene(), ctx.id, at);
            if local.x.abs() > self.size.x * 0.5 + 0.15 || local.z.abs() > self.size.z * 0.5 + 0.15 {
                continue;
            }
            let above = local.y - self.size.y * 0.5;
            if !(-0.2..=1.4).contains(&above) {
                continue;
            }
            let mass = physics.mass(body);
            let wanted = self.speed - util::velocity(ctx.scene(), body).dot(along);
            let accel = (wanted * self.grip).clamp(-9.0, 9.0);
            pushes.push((body, along * accel * mass));
        }
        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            for (body, force) in pushes {
                physics.add_force(body, force);
            }
        }
    }

    /// Планки бегут по ленте и переходят с конца в начало: видно, куда и как быстро она везёт.
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.scroll += self.speed * dt;
        let half = self.size.z * 0.5;
        let length = self.size.z.max(0.1);
        let children: Vec<GameObjectId> = ctx.object().children().to_vec();
        let mut index = 0.0;
        for child in children {
            let is_slat = ctx.scene().get(child).is_some_and(|o| o.name.starts_with("Slat"));
            if !is_slat {
                continue;
            }
            let u = ((self.scroll + index * self.slat_spacing) / length).rem_euclid(1.0);
            if let Some(object) = ctx.scene_mut().get_mut(child) {
                object.transform.position.z = half - u * length;
            }
            index += 1.0;
        }
    }
}
