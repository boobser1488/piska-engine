//! Шар игрока: управление, прыжок, проверка земли, звук качения. Скрипт висит на шаре
//! (`prefabs/ball.prefab`); поля настраиваются в Инспекторе.

use engine::prelude::*;

use crate::game::util;
use crate::game::view::{BallStatus, RollVoice, ViewState, BALL_RADIUS};

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
                util::tell_game(ctx, "fell", Value::Nil);
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
            util::effect(ctx.world, "dust", pos - Vec3::Y * 0.4);
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

