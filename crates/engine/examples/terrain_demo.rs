//! Рельеф как в проекте-образце: 2 × 2 км процедурной земли (море, побережье, холмы,
//! горы со снегом), чанки с LOD и подгрузкой вокруг камеры, вода, тени — и
//! несколько шаров, катящихся по склону (коллайдер рельефа создаётся сам).
//!
//! Запуск: `cargo run --example terrain_demo`.
//! Полёт: ПКМ + мышь — осмотреться, WASD — лететь, Q / E — вниз / вверх, Shift — быстрее.
//! F — пешком (персонаж ходит по рельефу, пробел — прыжок) / снова полёт.
//!
//! Снимок без окна: `ENGINE_SCREENSHOT=shot.png ENGINE_FRAMES=120 cargo run --example terrain_demo`;
//! `TERRAIN_DEMO_VIEW="x,y,z,tx,ty,tz"` задаёт, откуда и куда смотрит камера,
//! `TERRAIN_DEMO_DESERT=1` — пустыня (песок и тёплые скалы, как карта образца),
//! `TERRAIN_DEMO_ABOVE_GROUND=1` — высота глаза в `TERRAIN_DEMO_VIEW` считается от земли.

use engine::prelude::*;

/// Угол рельефа: рельеф 2048 м с центром в начале координат.
const CORNER: f32 = -1024.0;

component! {
    /// Камера: полёт или ходьба персонажем `Walker`.
    pub struct DemoCamera {
        pub speed: f32 = 40.0,
        pub yaw: f32 = 0.0,
        pub pitch: f32 = -0.2,
        pub walking: bool = false,
    }
}

impl Component for DemoCamera {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let input = ctx.input();
        if input.mouse_button(MouseButton::Right) {
            let delta = input.mouse_delta();
            self.yaw -= delta.x * 0.003;
            self.pitch = (self.pitch - delta.y * 0.003).clamp(-1.5, 1.5);
        }
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let mut wish = forward * input.axis("Vertical") + right * input.axis("Horizontal");
        let toggle = input.key_down(Key::F);
        let fast = input.key(Key::LShift);
        let (down, up, jump) = (input.key(Key::Q), input.key(Key::E), input.button_down("Jump"));
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);

        if toggle {
            self.walking = !self.walking;
            // Персонаж встаёт на землю под камерой.
            let here = ctx.world_position();
            if let (true, Some(walker)) = (self.walking, ctx.find_by_name("Walker")) {
                let ground = terrain_height(ctx.world, here.x, here.z).unwrap_or(here.y);
                if let Some(object) = ctx.scene_mut().get_mut(walker) {
                    object.transform.position = Vec3::new(here.x, ground + 0.5, here.z);
                }
            }
        }

        if self.walking {
            let Some(walker) = ctx.find_by_name("Walker") else { return };
            if let Some(character) = ctx.get_on_mut::<CharacterController>(walker) {
                wish = wish.normalize_or_zero() * if fast { 9.0 } else { 4.5 };
                character.velocity.x = wish.x;
                character.velocity.z = wish.z;
                if jump && character.grounded {
                    character.velocity.y = 5.0;
                }
            }
            let feet = ctx.scene().world_position(walker);
            let transform = ctx.transform();
            transform.position = feet + Vec3::Y * 1.7;
            transform.rotation = rotation;
            return;
        }

        if up {
            wish += Vec3::Y;
        }
        if down {
            wish -= Vec3::Y;
        }
        let speed = self.speed * if fast { 5.0 } else { 1.0 };
        let transform = ctx.transform();
        transform.position += wish.normalize_or_zero() * speed * dt;
        transform.rotation = rotation;
    }
}

/// Откуда и куда смотреть: из `TERRAIN_DEMO_VIEW` или вид на побережье и горы.
fn view() -> (Vec3, Vec3) {
    let parsed: Option<Vec<f32>> = std::env::var("TERRAIN_DEMO_VIEW")
        .ok()
        .map(|text| text.split(',').filter_map(|v| v.trim().parse().ok()).collect());
    match parsed {
        Some(v) if v.len() == 6 => (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5])),
        _ => (Vec3::new(-260.0, 95.0, 420.0), Vec3::new(120.0, 60.0, -60.0)),
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    app.register::<DemoCamera>();

    let desert = std::env::var("TERRAIN_DEMO_DESERT").is_ok_and(|v| v != "0");
    let terrain = Terrain {
        seed: 99,
        size: 2048.0,
        view_distance: 1600.0,
        // Площадка в центре (как площадки под карты в образце).
        flat_zones: vec![Vec3::new(1024.0, 1024.0, 60.0)],
        desert: if desert { 1.0 } else { 0.0 },
        ..Terrain::default()
    };
    let sampler = terrain.sampler(None);
    let ground = |x: f32, z: f32| sampler.height_local(x - CORNER, z - CORNER);
    app.spawn("Terrain").at3(CORNER, 0.0, CORNER).with(terrain.clone());
    app.spawn("Sea").with(Water::default());
    app.spawn("Environment").with(Environment {
        fog_density: 0.0005,
        shadow_distance: 180.0,
        ..Environment::default()
    });
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, -0.5, -0.7, 0.0))
        .with(Light::default());

    let (mut eye, target) = view();
    if std::env::var("TERRAIN_DEMO_ABOVE_GROUND").is_ok_and(|v| v != "0") {
        eye.y += ground(eye.x, eye.z);
    }
    let look = (target - eye).normalize();
    let yaw = (-look.x).atan2(-look.z);
    let pitch = look.y.clamp(-1.0, 1.0).asin();
    app.spawn("Camera")
        .at3(eye.x, eye.y, eye.z)
        .rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0))
        .with(Camera { far: 4000.0, ..Camera::perspective(60.0) })
        .with(DemoCamera { yaw, pitch, ..DemoCamera::default() });
    app.spawn("Walker")
        .at3(eye.x, ground(eye.x, eye.z) + 0.5, eye.z)
        .with(CharacterController::default());

    // Шары — на самом крутом склоне суши рядом с точкой, куда смотрит камера.
    let mut best = (target.x, target.z, 1.0f32);
    for k in 0..400 {
        let (x, z) = (target.x + (k % 20) as f32 * 8.0 - 80.0, target.z + (k / 20) as f32 * 8.0 - 80.0);
        let n = sampler.normal_local(x - CORNER, z - CORNER);
        if ground(x, z) > 3.0 && n.y < best.2 && n.y > 0.75 {
            best = (x, z, n.y);
        }
    }
    // Ящики на площадке в центре — для масштаба.
    let plaza = ground(0.0, 0.0);
    for (i, (x, z)) in [(-6.0, -4.0), (-4.5, -4.2), (5.0, 3.0), (6.2, 1.5), (0.0, 9.0)].into_iter().enumerate() {
        app.spawn(format!("Crate {i}"))
            .at3(x, plaza + 0.6, z)
            .scale(1.2, 1.2, 1.2)
            .rotation(Quat::from_rotation_y(i as f32 * 0.7))
            .with(MeshRenderer::cube().with_color(Color::hex(0x9c6b3c)).with_material(0.0, 0.8))
            .with(Collider::cuboid(1.0, 1.0, 1.0));
    }

    let colors = [0xffc35a, 0xeb5757, 0x4c8dff, 0x6fcf97, 0xb9c0c8, 0xff7a59];
    for (i, color) in colors.into_iter().enumerate() {
        let (x, z) = (best.0 + (i % 3) as f32 * 2.5, best.1 + (i / 3) as f32 * 2.5);
        app.spawn(format!("Ball {i}"))
            .at3(x, ground(x, z) + 1.5, z)
            .scale(1.2, 1.2, 1.2)
            .with(MeshRenderer::sphere().with_color(Color::hex(color)).with_material(0.0, 0.4))
            .with(Rigidbody { ccd: true, ..Rigidbody::default() })
            .with(Collider::sphere(0.5).with_material(0.6, 0.2));
    }

    engine::run(app, WindowConfig::titled("Terrain").with_size(1280, 720))
}
