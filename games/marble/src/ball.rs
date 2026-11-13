//! Шар и камера: управление, прыжок, проверка земли, орбитальная камера с подтягиванием
//! к шару у стен, тряска, звук качения. Это ядро ощущения игры — оно на Rust, а механизмы
//! цехов — на C++ (`cpp/`).

use engine::prelude::*;

/// Гравитация игры, м/с². Сильнее земной: шар быстрее приземляется и меньше «плавает».
pub const GRAVITY: f32 = 15.0;
/// Радиус шара.
pub const BALL_RADIUS: f32 = 0.5;

/// Вид, общий для шара и камеры: куда смотрит камера (от этого зависит «вперёд» для
/// управления), её сглаженный фокус и тряска.
#[derive(Clone, Debug)]
pub struct ViewState {
    /// Рыскание камеры, радианы: 0 — смотрит вдоль −Z; плюс — поворот налево.
    pub yaw: f32,
    /// Угол места камеры над горизонтом, радианы.
    pub pitch: f32,
    /// Желаемое расстояние (колесо мыши меняет), метры.
    pub distance: f32,
    /// Чувствительность вращения, 0.3..2.
    pub sensitivity: f32,
    /// Тряска камеры 0..1 (квадрат даёт амплитуду).
    pub trauma: f32,
    /// Шар, за которым следим.
    pub ball: Option<GameObjectId>,
    /// Сглаженная точка, на которую смотрит камера.
    pub focus: Vec3,
    /// Сглаженное расстояние (после подтягивания у стен).
    pub shown_distance: f32,
    /// Следующий кадр камеры не сглаживает, а прыгает к цели (старт, респаун).
    pub snap: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.6,
            distance: 9.5,
            sensitivity: 1.0,
            trauma: 0.0,
            ball: None,
            focus: Vec3::ZERO,
            shown_distance: 9.5,
            snap: true,
        }
    }
}

/// Голос гула качения — живёт, пока идёт цех.
#[derive(Default)]
pub struct RollVoice {
    pub voice: Option<VoiceId>,
}

/// Что шар сообщает «наружу» за кадр (для HUD и тестов).
#[derive(Clone, Copy, Debug, Default)]
pub struct BallStatus {
    pub speed: f32,
    pub grounded: bool,
    pub jumps: u32,
}

/// Послать сообщение менеджеру игры («Game»).
pub fn tell_game(ctx: &mut Ctx, name: &str, value: Value) {
    if let Some(game) = ctx.world.scene.find_by_name("Game") {
        ctx.send_message(game, name, value);
    }
}

/// Разовые частицы (пыль, искры) в точке мира: объект удаляется сам.
pub fn burst(world: &mut World, pos: Vec3, effect: ParticleSystem) {
    world.scene.build("Burst").at3(pos.x, pos.y, pos.z).with(effect).id();
}

component! {
    /// Шар игрока: ускоряется силой относительно камеры, прыгает с земли, не быстрее
    /// `max_speed` (разгонные плиты и ленты могут разогнать его сильнее).
    pub struct Ball {
        /// Предел скорости от управления, м/с.
        pub max_speed: f32 = 8.5,
        /// Ускорение на земле, м/с².
        pub accel: f32 = 17.0,
        /// Ускорение в воздухе: управлять можно, но слабо.
        pub air_accel: f32 = 6.0,
        /// Вертикальная скорость прыжка, м/с.
        pub jump_speed: f32 = 6.9,
        /// Управление отключено (отсчёт, пауза, результаты).
        pub frozen: bool = false,
        /// Ниже этой высоты шар упал.
        pub kill_y: f32 = -60.0,
        /// Стоит ли шар на земле (после последнего шага физики).
        pub grounded: bool = false,
        pub coyote: f32 = 0.0,
        pub jump_buffer: f32 = 0.0,
        /// Пауза после прыжка: удержание кнопки не даёт «двойных» толчков, пока шар ещё у земли.
        pub jump_lock: f32 = 0.0,
        pub fell: bool = false,
        pub last_fall_speed: f32 = 0.0,
    }
}

impl Component for Ball {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let id = ctx.id;
        let pos = ctx.world_position();

        // Падение за край цеха.
        if pos.y < self.kill_y {
            if !self.fell {
                self.fell = true;
                tell_game(ctx, "fell", Value::Nil);
            }
            return;
        }
        self.fell = false;

        let velocity = ctx.get::<Rigidbody>().map_or(Vec3::ZERO, |body| body.velocity);

        // Земля под шаром: луч вниз чуть длиннее радиуса.
        let was_grounded = self.grounded;
        self.grounded = ctx
            .resource::<Physics3D>()
            .and_then(|physics| physics.raycast(pos, Vec3::NEG_Y, BALL_RADIUS + 0.2, Some(id)))
            .is_some();
        if self.grounded {
            self.coyote = 0.11;
        } else {
            self.coyote = (self.coyote - dt).max(0.0);
            self.last_fall_speed = -velocity.y;
        }
        if self.grounded && !was_grounded && self.last_fall_speed > 4.5 {
            let volume = (self.last_fall_speed / 14.0).clamp(0.25, 0.9);
            if let Some(audio) = ctx.resource_mut::<Audio>() {
                audio.play_with("land", PlayParams::new().volume(volume).pitch(0.9));
            }
            burst(ctx.world, pos - Vec3::Y * 0.4, ParticleSystem::dust().in_3d());
        }

        if let Some(status) = ctx.resource_mut::<BallStatus>() {
            status.speed = velocity.length();
            status.grounded = self.grounded;
        }

        if self.frozen {
            self.jump_buffer = 0.0;
            return;
        }

        // Ввод: оси относительно камеры.
        let input = ctx.input();
        let mut stick = Vec2::new(input.axis("Horizontal"), input.axis("Vertical"));
        if stick.length() > 1.0 {
            stick = stick.normalize();
        }
        let jump_pressed = input.button_down("Jump");
        let yaw = ctx.resource::<ViewState>().map_or(0.0, |view| view.yaw);
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let wish = right * stick.x + forward * stick.y;

        let mass = ctx.resource::<Physics3D>().map_or(1.0, |physics| physics.mass(id)).max(0.01);
        let horizontal = Vec3::new(velocity.x, 0.0, velocity.z);
        let mut force = Vec3::ZERO;
        if wish.length() > 0.05 {
            let dir = wish.normalize();
            let accel = if self.grounded { self.accel } else { self.air_accel };
            // Разгоняем, пока скорость вдоль желаемого направления меньше предела.
            if horizontal.dot(dir) < self.max_speed {
                force += wish * accel * mass;
            }
            // Помощь в развороте: боковая скорость гасится, чтобы шар не «плыл» по льду.
            if self.grounded {
                let side = horizontal - dir * horizontal.dot(dir);
                force -= side * 3.0 * mass;
            }
        } else if self.grounded {
            // Нет ввода: лёгкое торможение, чтобы шар не катился вечно.
            force -= horizontal * 1.6 * mass;
        }

        // Прыжок: буфер нажатия и «койот» прощают чуть ранние и чуть поздние нажатия.
        if jump_pressed {
            self.jump_buffer = 0.12;
        } else {
            self.jump_buffer = (self.jump_buffer - dt).max(0.0);
        }
        let mut impulse = Vec3::ZERO;
        let mut jumped = false;
        self.jump_lock = (self.jump_lock - dt).max(0.0);
        if self.jump_buffer > 0.0 && self.coyote > 0.0 && self.jump_lock <= 0.0 && velocity.y < 2.5 {
            impulse.y = mass * (self.jump_speed - velocity.y.max(0.0));
            self.jump_buffer = 0.0;
            self.coyote = 0.0;
            self.jump_lock = 0.3;
            jumped = true;
        }

        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            if force != Vec3::ZERO {
                physics.add_force(id, force);
            }
            if impulse != Vec3::ZERO {
                physics.apply_impulse(id, impulse);
            }
        }
        if jumped {
            if let Some(status) = ctx.resource_mut::<BallStatus>() {
                status.jumps += 1;
            }
            if let Some(audio) = ctx.resource_mut::<Audio>() {
                audio.play("jump");
            }
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        // Гул качения: громче и выше с ростом скорости, в воздухе стихает.
        let speed = ctx.get::<Rigidbody>().map_or(0.0, |body| body.velocity.length());
        let paused = ctx.time().time_scale <= 0.0;
        let target = if self.grounded && !paused && !self.frozen { (speed / 9.0).clamp(0.0, 1.0) } else { 0.0 };
        let pitch = 0.7 + (speed / 14.0).clamp(0.0, 1.0) * 0.9;
        let existing = ctx.resource::<RollVoice>().and_then(|roll| roll.voice);
        let mut voice = existing.filter(|v| ctx.resource::<Audio>().is_some_and(|audio| audio.is_playing(*v)));
        if voice.is_none() {
            voice = ctx.resource_mut::<Audio>().map(|audio| audio.play_with("roll", PlayParams::new().looping(true).volume(0.0)));
            if let Some(roll) = ctx.world.resources.get_mut::<RollVoice>() {
                roll.voice = voice;
            }
        }
        if let (Some(voice), Some(audio)) = (voice, ctx.resource_mut::<Audio>()) {
            audio.set_volume(voice, target * 0.55);
            audio.set_pitch(voice, pitch);
        }
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        let voice = ctx.resource_mut::<RollVoice>().and_then(|roll| roll.voice.take());
        if let (Some(voice), Some(audio)) = (voice, ctx.resource_mut::<Audio>()) {
            audio.stop(voice);
        }
    }
}

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
