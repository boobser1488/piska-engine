use engine_core::prelude::*;
use engine_physics::{CharacterController, PhysicsCtx};

component! {
    /// Герой от третьего лица на `CharacterController`: ходит по осям `Horizontal` /
    /// `Vertical` относительно камеры (если она есть) и поворачивается лицом по ходу,
    /// прыгает по `Jump`. Гравитацию, ступеньки и стены берёт на себя контроллер.
    /// От первого лица — готовый `FpsController`.
    pub struct PlayerMove3D {
        /// Скорость ходьбы, м/с.
        pub speed: f32 = 5.0,
        /// Начальная скорость прыжка, м/с.
        pub jump_speed: f32 = 5.0,
        /// Как быстро разворачивается лицом по ходу (0 — не разворачиваться).
        pub turn_speed: f32 = 12.0,
    }
}

impl Component for PlayerMove3D {
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        let input = ctx.axis2("Horizontal", "Vertical");
        // «Вперёд» — куда смотрит главная камера, по земле.
        let camera = ctx.first_with::<engine_render::Camera>();
        let (forward, right) = if ctx.exists(camera) {
            let f = ctx.on(camera).forward();
            let f = Vec3::new(f.x, 0.0, f.z).try_normalize().unwrap_or(Vec3::NEG_Z);
            (f, Vec3::new(-f.z, 0.0, f.x))
        } else {
            (Vec3::NEG_Z, Vec3::X)
        };
        let walk = (forward * input.y + right * input.x) * self.speed;

        let grounded = ctx.grounded();
        let mut velocity = ctx.velocity();
        velocity.x = walk.x;
        velocity.z = walk.z;
        if grounded && ctx.pressed("Jump") {
            velocity.y = self.jump_speed;
        }
        if let Some(character) = ctx.get_mut::<CharacterController>() {
            character.velocity = velocity;
        } else {
            ctx.set_velocity(velocity); // обучающая ошибка: нет контроллера
        }

        if self.turn_speed > 0.0 && walk.length_squared() > 1e-4 {
            let yaw = f32::atan2(-walk.x, -walk.z);
            let target = Quat::from_rotation_y(yaw);
            let t = 1.0 - (-self.turn_speed * ctx.dt()).exp();
            let me = ctx.id;
            if let Some(object) = ctx.scene_mut().get_mut(me) {
                object.transform.rotation = object.transform.rotation.slerp(target, t);
            }
        }
    }
}
