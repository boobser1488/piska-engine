//! Орбитальная камера: следует за шаром, вращается мышью (правая кнопка), правым стиком
//! и клавишами Q / E, не проходит сквозь стены; в меню кружит вокруг витрины.

use engine::prelude::*;

use crate::game::view::ViewState;

component! {
    /// Орбитальная камера: следует за шаром, вращается мышью (правая кнопка), правым
    /// стиком и клавишами Q / E, не проходит сквозь стены. В меню — медленно кружит вокруг
    /// `orbit_center`.
    pub struct CameraRig {
        /// Диапазон расстояния, метры.
        pub min_distance: f32 = 4.5,
        pub max_distance: f32 = 15.0,
        /// Подъём точки взгляда над центром шара.
        pub height: f32 = 0.4,
        /// Режим витрины (меню): камера кружит вокруг `orbit_center`.
        pub orbit: bool = false,
        pub orbit_center: Vec3 = Vec3::ZERO,
        pub orbit_speed_deg: f32 = 9.0,
        pub orbit_distance: f32 = 17.0,
        /// Базовое поле зрения; растёт со скоростью.
        pub base_fov: f32 = 62.0,
    }
}

impl Component for CameraRig {
    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let mut view = ctx.world.resources.remove::<ViewState>().unwrap_or_default();
        let live = ctx.time().time_scale > 0.0;

        // Ввод вращения.
        if live && !self.orbit {
            let input = ctx.input();
            let mut turn = 0.0;
            let mut tilt = 0.0;
            if input.mouse_button(MouseButton::Right) {
                let delta = input.mouse_delta();
                turn -= delta.x * 0.0052;
                tilt += delta.y * 0.0040;
            }
            for pad in input.gamepads() {
                let stick = input.gamepad_right_stick(pad, 0.18);
                turn -= stick.x * 2.4 * dt;
                tilt -= stick.y * 1.4 * dt;
            }
            if input.key(Key::Q) {
                turn += 1.9 * dt;
            }
            if input.key(Key::E) {
                turn -= 1.9 * dt;
            }
            let wheel = input.scroll_delta().y;
            view.yaw += turn * view.sensitivity;
            view.pitch = (view.pitch + tilt * view.sensitivity).clamp(0.12, 1.15);
            view.distance = (view.distance - wheel * 0.7).clamp(self.min_distance, self.max_distance);
        }

        let mut speed = 0.0;
        let (focus_target, distance_target) = if self.orbit {
            view.yaw += self.orbit_speed_deg.to_radians() * dt;
            view.pitch = 0.42;
            (self.orbit_center, self.orbit_distance)
        } else {
            let ball_pos = view.ball.filter(|id| ctx.world.scene.contains(*id)).map(|id| ctx.world.scene.world_position(id));
            if let Some(body) = view.ball.and_then(|id| ctx.world.scene.get_component::<Rigidbody>(id)) {
                speed = body.velocity.length();
            }
            (ball_pos.map_or(view.focus, |p| p + Vec3::Y * self.height), view.distance + (speed * 0.09).min(1.4))
        };

        if view.snap {
            view.focus = focus_target;
            view.shown_distance = distance_target;
            view.snap = false;
        } else {
            view.focus = view.focus.lerp(focus_target, 1.0 - (-16.0 * dt).exp());
        }

        // Камера позади и выше; расстояние подтягивается к шару, если на пути стена.
        let dir = Vec3::new(view.yaw.sin() * view.pitch.cos(), view.pitch.sin(), view.yaw.cos() * view.pitch.cos());
        let mut distance = distance_target;
        if !self.orbit {
            let ignore = view.ball;
            let hit = ctx
                .resource::<Physics3D>()
                .and_then(|physics| physics.raycast(view.focus, dir, distance + 0.35, ignore))
                .map(|hit| (hit.distance - 0.35).max(1.2));
            if let Some(near) = hit {
                distance = distance.min(near);
            }
        }
        // Приближаемся мгновенно (не заходим в стену), отдаляемся плавно.
        view.shown_distance = if distance < view.shown_distance {
            distance
        } else {
            view.shown_distance + (distance - view.shown_distance) * (1.0 - (-5.0 * dt).exp())
        };

        let mut position = view.focus + dir * view.shown_distance;

        // Тряска от ударов.
        if view.trauma > 0.0 {
            let amount = view.trauma * view.trauma * 0.45;
            let random = ctx.random();
            position += Vec3::new(random.range(-1.0, 1.0), random.range(-1.0, 1.0), random.range(-1.0, 1.0)) * amount;
            view.trauma = (view.trauma - 1.7 * dt).max(0.0);
        }

        let transform = ctx.transform();
        transform.position = position;
        transform.look_at(view.focus, Vec3::Y);

        // Поле зрения растёт со скоростью — ощущение разгона.
        let fov_target = self.base_fov + (speed * 0.7).min(10.0);
        let base = self.base_fov;
        if let Some(camera) = ctx.get_mut::<Camera>() {
            camera.fov_deg = if (camera.fov_deg - fov_target).abs() > 40.0 { base } else { camera.fov_deg + (fov_target - camera.fov_deg) * (1.0 - (-4.0 * dt).exp()) };
        }

        ctx.world.resources.insert(view);
    }
}
