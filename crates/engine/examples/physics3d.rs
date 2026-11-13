//! 3D-физика: башня ящиков, мячи, маятник на верёвке, дверь на петле и персонаж,
//! который всё это толкает.
//!
//! Запуск: `cargo run --example physics3d`.
//! WASD или стрелки — идти, пробел — прыжок, E — взрывной импульс вокруг персонажа.

use engine::prelude::*;

component! {
    /// Управление персонажем: задаёт желаемую скорость контроллеру.
    pub struct PlayerWalk {
        pub speed: f32 = 4.5,
        pub jump_speed: f32 = 5.0,
        /// Радиус и сила «взрыва» по клавише E.
        pub blast_radius: f32 = 5.0,
        pub blast_speed: f32 = 9.0,
    }
}

impl Component for PlayerWalk {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let input = ctx.input();
        let direction = Vec3::new(input.axis("Horizontal"), 0.0, -input.axis("Vertical")).normalize_or_zero();
        let (jump, blast) = (input.button_down("Jump"), input.key_down(Key::E));

        let velocity = direction * self.speed;
        let jump_speed = self.jump_speed;
        if let Some(character) = ctx.get_mut::<CharacterController>() {
            character.velocity.x = velocity.x;
            character.velocity.z = velocity.z;
            if jump && character.grounded {
                character.velocity.y = jump_speed;
            }
        }
        if blast {
            let center = ctx.world_position() + Vec3::Y * 0.5;
            let (radius, speed) = (self.blast_radius, self.blast_speed);
            if let Some(physics) = ctx.resource_mut::<Physics3D>() {
                let hit = physics.radial_impulse(center, radius, speed);
                log::info!("blast: {} bodies pushed", hit.len());
            }
        }
    }
}

component! {
    /// Камера следует за объектом с тегом и смотрит на него.
    pub struct ChaseCamera {
        pub target_tag: String = String::from("Player"),
        pub offset: Vec3 = Vec3::new(0.0, 4.5, 9.0),
        pub smoothing: f32 = 5.0,
    }
}

impl Component for ChaseCamera {
    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(target) = ctx.find_by_tag(&self.target_tag) else {
            return;
        };
        let focus = ctx.scene().world_position(target) + Vec3::Y;
        let blend = 1.0 - (-self.smoothing * dt).exp();
        let goal = focus + self.offset;
        let transform = ctx.transform();
        transform.position = transform.position.lerp(goal, blend);
        transform.look_at(focus, Vec3::Y);
    }
}

component! {
    /// Вешает маятник на верёвку и дверь на петлю, когда сцена запущена.
    pub struct Rigging {
        pub rope_anchor: Vec3 = Vec3::new(-6.0, 6.0, -2.0),
    }
}

impl Component for Rigging {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let bob = ctx.scene().find_by_name("Bob");
        let door = ctx.scene().find_by_name("Door");
        let bob_position = bob.map(|id| ctx.scene().world_position(id));
        let door_position = door.map(|id| ctx.scene().world_position(id));
        let anchor = self.rope_anchor;
        let Some(physics) = ctx.resource_mut::<Physics3D>() else {
            return;
        };
        if let (Some(bob), Some(position)) = (bob, bob_position) {
            physics.rope_joint(bob, position, None, anchor, 0.0);
        }
        if let (Some(door), Some(position)) = (door, door_position) {
            // Петля — по левому краю двери, вокруг вертикальной оси.
            physics.hinge_joint(door, None, position - Vec3::X * 0.75, Vec3::Y);
        }
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    app.register::<PlayerWalk>().register::<ChaseCamera>().register::<Rigging>();

    app.spawn("Camera")
        .at3(0.0, 5.0, 14.0)
        .with(Camera::perspective(55.0))
        .with(ChaseCamera::default());
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0))
        .with(Light::default());
    app.spawn("Rigging").with(Rigging::default());

    // Земля: видимая плоскость и толстый коллайдер под ней.
    app.spawn("Ground")
        .scale(400.0, 1.0, 400.0)
        .with(MeshRenderer::plane().with_color(Color::hex(0x8a8f7c)).with_grid().without_shadows());
    app.spawn("Ground Collider")
        .at3(0.0, -0.5, 0.0)
        .with(Collider::cuboid(400.0, 1.0, 400.0));

    // Персонаж: позиция объекта — у ног, тело — дочерний меш.
    let player = app
        .spawn("Player")
        .tag("Player")
        .at3(0.0, 0.0, 6.0)
        .with(CharacterController::default())
        .with(PlayerWalk::default())
        .id();
    app.spawn("Body")
        .at3(0.0, 0.9, 0.0)
        .scale(0.7, 1.8, 0.7)
        .with(MeshRenderer::cylinder().with_color(Color::hex(0xff7a59)).with_material(0.0, 0.5))
        .child_of(player);

    // Башня ящиков 3 × 4.
    let colors = [0x4c8dff, 0x6fcf97, 0xf2c94c, 0xeb5757];
    for (row, color) in colors.into_iter().enumerate() {
        for column in 0..3 {
            app.spawn(format!("Crate {row}-{column}"))
                .at3(column as f32 * 1.02 - 1.02, 0.5 + row as f32 * 1.01, 0.0)
                .with(MeshRenderer::cube().with_color(Color::hex(color)).with_material(0.0, 0.6))
                .with(Rigidbody::dynamic())
                .with(Collider::cuboid(1.0, 1.0, 1.0).with_density(0.4));
        }
    }

    // Мячи: упругие, с непрерывным обнаружением столкновений.
    for index in 0..4 {
        app.spawn(format!("Ball {index}"))
            .at3(4.0 + index as f32 * 0.9, 3.0 + index as f32, 2.0)
            .scale(0.8, 0.8, 0.8)
            .with(MeshRenderer::sphere().with_color(Color::hex(0xffc35a)).with_material(1.0, 0.3))
            .with(Rigidbody {
                ccd: true,
                ..Rigidbody::default()
            })
            .with(Collider::sphere(0.5).with_material(0.4, 0.6));
    }

    // Маятник: шар на верёвке (сустав создаёт Rigging).
    app.spawn("Bob")
        .at3(-3.0, 4.0, -2.0)
        .with(MeshRenderer::sphere().with_color(Color::hex(0xb9c0c8)).with_material(1.0, 0.4))
        .with(Rigidbody::dynamic())
        .with(Collider::sphere(0.5).with_density(3.0));

    // Дверь на петле: её можно открыть, толкнув персонажем.
    app.spawn("Door")
        .at3(6.0, 1.1, -3.0)
        .scale(1.5, 2.2, 0.15)
        .with(MeshRenderer::cube().with_color(Color::hex(0x8d6e63)).with_material(0.0, 0.7))
        .with(Rigidbody {
            angular_damping: 0.8,
            ..Rigidbody::default()
        })
        .with(Collider::cuboid(1.0, 1.0, 1.0));

    engine::run(app, WindowConfig::titled("Physics 3D"))
}
