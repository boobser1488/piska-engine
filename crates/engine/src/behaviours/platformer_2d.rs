use engine_core::prelude::*;
use engine_physics::PhysicsCtx;

component! {
    /// Герой платформера: бег по оси `Horizontal`, прыжок по действию `Jump`. Прощает
    /// поздний прыжок («время койота» после схода с края) и ранее нажатие (буфер прыжка).
    /// Нужны `Rigidbody2D` (лучше с `lock_rotation`) и `Collider2D`.
    pub struct Platformer2D {
        /// Скорость бега, единиц в секунду.
        pub speed: f32 = 6.0,
        /// Начальная скорость прыжка вверх.
        pub jump_speed: f32 = 11.0,
        /// Сколько секунд после схода с края ещё можно прыгнуть.
        pub coyote_time: f32 = 0.1,
        /// Сколько секунд помнится нажатие прыжка до приземления.
        pub jump_buffer: f32 = 0.12,
        /// Секунд с тех пор, как стояли на земле.
        pub air_time: f32 = 0.0,
        /// Секунд с нажатия прыжка (большое число — не нажимали).
        pub since_jump_press: f32 = 99.0,
        /// Отражать спрайт по направлению бега.
        pub flip_sprite: bool = true,
    }
}

impl Component for Platformer2D {
    // Шаг физики: скорость тела меняется ровно перед тем, как физика её применит.
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        let dt = ctx.dt();
        let run = ctx.axis("Horizontal");
        if ctx.pressed("Jump") {
            self.since_jump_press = 0.0;
        } else {
            self.since_jump_press += dt;
        }
        if ctx.grounded() {
            self.air_time = 0.0;
        } else {
            self.air_time += dt;
        }

        let mut velocity = ctx.velocity2();
        velocity.x = run * self.speed;
        let can_jump = self.air_time <= self.coyote_time;
        if can_jump && self.since_jump_press <= self.jump_buffer && velocity.y <= 0.1 {
            velocity.y = self.jump_speed;
            self.since_jump_press = 99.0;
            self.air_time = self.coyote_time + 1.0; // не прыгнуть второй раз в воздухе
        }
        ctx.set_velocity(velocity);

        // Спрайт необязателен: длинная форма `get_mut` молча пропускает объект без него.
        if let (true, Some(sprite)) = (self.flip_sprite && run != 0.0, ctx.get_mut::<engine_render::SpriteRenderer>()) {
            sprite.flip_x = run < 0.0;
        }
    }
}
