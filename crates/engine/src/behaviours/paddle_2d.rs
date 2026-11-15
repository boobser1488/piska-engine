use engine_core::prelude::*;

use crate::screen::ScreenCtx;

component! {
    /// Ракетка, корзина, кораблик у края экрана: ездит влево-вправо по оси `Horizontal`
    /// (A/D, стрелки, крестовина, стик) или, пока зажата мышь (не над интерфейсом) или палец
    /// на экране, — к указателю. Не выезжает за край видимой области главной камеры.
    /// С `Rigidbody2D` задаёт скорость тела по X (по Y она остаётся телу, например
    /// гравитации), без него — двигает объект.
    pub struct Paddle2D {
        /// Скорость, единиц в секунду.
        pub speed: f32 = 8.0,
        /// Ехать к пальцу или зажатой мыши.
        pub follow_pointer: bool = true,
        /// Отступ от края экрана, единиц мира (обычно половина ширины спрайта).
        pub view_margin: f32 = 0.5,
        /// Отражать `SpriteRenderer` по X, когда едет влево.
        pub flip_sprite: bool = false,
    }
}

impl Component for Paddle2D {
    fn update(&mut self, ctx: &mut Ctx) {
        let mut run = ctx.axis("Horizontal");
        let x = ctx.pos().x;
        if self.follow_pointer && ctx.pointer_held() {
            // Ближе к указателю — медленнее: объект не дрожит под пальцем.
            run = ((ctx.pointer_world().x - x) * 2.0).clamp(-1.0, 1.0);
        }
        let area = ctx.view_rect().expand(-self.view_margin);
        if (x <= area.x && run < 0.0) || (x >= area.max().x && run > 0.0) {
            run = 0.0;
        }
        if self.flip_sprite && run != 0.0 {
            if let Some(sprite) = ctx.get_mut::<engine_render::SpriteRenderer>() {
                sprite.flip_x = run < 0.0;
            }
        }
        #[cfg(feature = "physics2d")]
        if ctx.has::<engine_physics::Rigidbody2D>() {
            use engine_physics::PhysicsCtx;
            let fall = ctx.velocity2().y;
            ctx.set_velocity(Vec2::new(run * self.speed, fall));
            return;
        }
        ctx.move_by(Vec2::new(run * self.speed * ctx.dt(), 0.0));
    }
}
