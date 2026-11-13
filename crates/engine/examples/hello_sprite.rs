//! Hello World: окно, камера, спрайт и скрипт движения.
//! Запуск: `cargo run --example hello_sprite` — управление WASD / стрелками.

use engine::prelude::*;

component! {
    /// Двигает объект по осям ввода.
    pub struct Mover {
        pub speed: f32 = 4.0,
    }
}

impl Component for Mover {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let direction = Vec2::new(ctx.input().axis("Horizontal"), ctx.input().axis("Vertical"));
        ctx.transform().position += (direction * self.speed * dt).extend(0.0);
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    app.spawn("Camera").with(Camera::default());
    app.spawn("Player")
        .with(SpriteRenderer::colored(Color::hex(0xff7a59)))
        .with(Mover::default());
    engine::run(app, WindowConfig::titled("Hello, sprite"))
}
