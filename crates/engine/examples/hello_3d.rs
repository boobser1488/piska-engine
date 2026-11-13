//! 3D «из коробки»: меши с PBR-материалами, солнце с тенями, точечный свет, небо.
//!
//! Запуск: `cargo run --example hello_3d`. Стрелки влево/вправо вращают камеру.

use engine::prelude::*;

component! {
    /// Вращает объект вокруг вертикальной оси.
    pub struct Spinner {
        pub speed_deg: f32 = 45.0,
    }
}

impl Component for Spinner {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        ctx.transform().rotate_y(self.speed_deg.to_radians() * dt);
    }
}

component! {
    /// Камера на орбите вокруг точки: стрелки влево/вправо.
    pub struct Orbit {
        pub target: Vec3 = Vec3::new(0.0, 0.8, 0.0),
        pub distance: f32 = 9.0,
        pub height: f32 = 3.6,
        pub angle_deg: f32 = 28.0,
        pub speed_deg: f32 = 60.0,
    }
}

impl Component for Orbit {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.angle_deg += ctx.input().axis("Horizontal") * self.speed_deg * dt;
        let angle = self.angle_deg.to_radians();
        let eye = self.target + Vec3::new(angle.sin() * self.distance, self.height, angle.cos() * self.distance);
        let (target, transform) = (self.target, ctx.transform());
        transform.position = eye;
        transform.look_at(target, Vec3::Y);
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    app.register::<Spinner>().register::<Orbit>();

    // Камера: перспективная, сразу с небом.
    app.spawn("Camera").with(Camera::perspective(50.0)).with(Orbit::default());

    // Солнце: объект повёрнут так, что «вперёд» смотрит вниз и вбок.
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.85, 0.0))
        .with(Light::default());
    app.spawn("Lamp")
        .at3(-2.4, 1.3, 2.2)
        .with(Light::point(Color::hex(0xff8a3c), 14.0, 7.0))
        .with(MeshRenderer::sphere().with_emissive(Color::rgb(9.0, 3.6, 0.9)).without_shadows())
        .scale(0.18, 0.18, 0.18);

    app.spawn("Ground")
        .scale(400.0, 1.0, 400.0)
        .with(
            MeshRenderer::plane()
                .with_color(Color::hex(0x8a8f7c))
                .with_material(0.0, 0.9)
                .with_grid()
                .without_shadows(),
        );

    // Материалы: пластик, золото, матовая керамика, сталь.
    app.spawn("Plastic Cube")
        .at3(-2.6, 0.75, -0.3)
        .scale(1.5, 1.5, 1.5)
        .with(MeshRenderer::cube().with_color(Color::hex(0xff7a59)).with_material(0.0, 0.35))
        .with(Spinner::default());
    app.spawn("Gold Sphere")
        .at3(0.0, 1.0, 0.0)
        .scale(2.0, 2.0, 2.0)
        .with(MeshRenderer::sphere().with_color(Color::hex(0xffc35a)).with_material(1.0, 0.28));
    app.spawn("Ceramic Cylinder")
        .at3(2.6, 0.9, -0.4)
        .scale(1.3, 1.8, 1.3)
        .with(MeshRenderer::cylinder().with_color(Color::hex(0xe9e4da)).with_material(0.0, 0.7));
    app.spawn("Steel Sphere")
        .at3(1.3, 0.45, 2.3)
        .scale(0.9, 0.9, 0.9)
        .with(MeshRenderer::sphere().with_color(Color::hex(0xb9c0c8)).with_material(1.0, 0.45));
    let stack = app.spawn("Stack").at3(-0.9, 0.3, 2.6).id();
    for (index, color) in [0x4c8dff, 0x6fcf97, 0xf2c94c].into_iter().enumerate() {
        app.spawn(format!("Block {index}"))
            .at3(0.0, index as f32 * 0.6, 0.0)
            .rotation(Quat::from_rotation_y(index as f32 * 0.5))
            .scale(0.9 - index as f32 * 0.2, 0.6, 0.9 - index as f32 * 0.2)
            .with(MeshRenderer::cube().with_color(Color::hex(color)).with_material(0.0, 0.5))
            .child_of(stack);
    }

    engine::run(app, WindowConfig::titled("Hello 3D"))
}
