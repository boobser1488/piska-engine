//! Дух-фонарь: бег с ускорением, прыжок переменной высоты, «койот», буфер прыжка,
//! двойной прыжок, рывок, урон с отбрасыванием и неуязвимостью.

use crate::common;
use crate::controls;
use crate::phys::{self, Body, Shared};
use crate::sfx;
use engine::prelude::*;

/// Половинки коллайдера (центр — над ступнями на `HALF.y`).
pub const HALF: Vec2 = Vec2::new(0.32, 0.46);
pub const MAX_HP: i32 = 3;

pub const RUN: f32 = 7.0;
const ACCEL_GROUND: f32 = 75.0;
const ACCEL_AIR: f32 = 46.0;
const FRICTION_GROUND: f32 = 90.0;
const FRICTION_AIR: f32 = 16.0;
pub const GRAVITY: f32 = 42.0;
const FALL_MAX: f32 = 24.0;
pub const JUMP_V: f32 = 15.2;
const DOUBLE_JUMP_V: f32 = 13.4;
const COYOTE: f32 = 0.1;
const BUFFER: f32 = 0.12;
pub const DASH_SPEED: f32 = 19.0;
pub const DASH_TIME: f32 = 0.17;
const DASH_COOLDOWN: f32 = 0.5;
const HURT_TIME: f32 = 0.32;
const INVULN: f32 = 1.4;
pub const STOMP_V: f32 = 12.5;
/// Максимальный шаг интегрирования, секунд.
const SUBSTEP: f32 = 1.0 / 120.0;

component! {
    /// Игрок.
    pub struct Player {
        pub vel: Vec2 = Vec2::ZERO,
        pub grounded: bool = false,
        pub coyote: f32 = 0.0,
        pub jump_buffer: f32 = 0.0,
        /// Прыжок идёт вверх (для переменной высоты).
        pub jumping: bool = false,
        /// Сколько двойных прыжков осталось в воздухе.
        pub air_jumps: i32 = 0,
        pub has_double_jump: bool = false,
        pub has_dash: bool = false,
        pub dash_time: f32 = 0.0,
        pub dash_cooldown: f32 = 0.0,
        pub dash_dir: f32 = 1.0,
        pub can_dash: bool = true,
        pub facing: f32 = 1.0,
        pub hp: i32 = 3,
        pub invuln: f32 = 0.0,
        pub hurt_time: f32 = 0.0,
        /// Платформа под ногами (биты `GameObjectId`) или −1.
        pub platform: i64 = -1,
        /// Последняя безопасная точка (ступни): сюда возвращаемся после шипов и ям.
        pub safe_pos: Vec2 = Vec2::ZERO,
        pub safe_timer: f32 = 0.0,
        pub drop_timer: f32 = 0.0,
        /// Пауза после возвращения на безопасную точку.
        pub respawn_time: f32 = 0.0,
        pub dead: bool = false,
        /// Управление выключено (заставка, победа, экраны).
        pub frozen: bool = false,
        pub squash: f32 = 0.0,
        pub dust_timer: f32 = 0.0,
        /// Сколько раз игрок падал в яму или на шипы (для статистики).
        pub falls: i32 = 0,
    }
}

/// Входные данные кадра.
#[derive(Clone, Copy, Default)]
struct Inp {
    axis: f32,
    jump_down: bool,
    jump_held: bool,
    dash_down: bool,
    down: bool,
}

fn read_input(ctx: &Ctx) -> Inp {
    let input = ctx.input();
    let touch = controls::read_touch(input);
    Inp {
        axis: (input.axis("Horizontal") + touch.axis).clamp(-1.0, 1.0),
        jump_down: input.button_down("Jump") || touch.jump_down,
        jump_held: input.button("Jump") || touch.jump_held,
        dash_down: input.button_down("Dash") || touch.dash_down,
        down: input.axis("Vertical") < -0.6 || touch.down,
    }
}

impl Player {
    fn feet(ctx: &Ctx) -> Vec2 {
        ctx.world_position().truncate()
    }

    fn set_feet(ctx: &mut Ctx, p: Vec2) {
        let z = ctx.transform().position.z;
        ctx.transform().position = p.extend(z);
    }

    /// Отбросить и ранить (враг, снаряд, шипы).
    fn hit(&mut self, ctx: &mut Ctx, dir: f32, from_spikes: bool) {
        if self.invuln > 0.0 || self.dash_time > 0.0 || self.dead || self.frozen {
            return;
        }
        self.hp -= 1;
        self.invuln = INVULN;
        self.hurt_time = HURT_TIME;
        self.dash_time = 0.0;
        self.vel = Vec2::new(dir * 6.5, 9.0);
        self.jumping = false;
        common::shake(ctx, 0.55);
        sfx::play(ctx, "hurt");
        let at = Self::feet(ctx) + Vec2::new(0.0, 0.5);
        common::burst(ctx, at, Color::rgb(2.6, 0.5, 0.5), 0.5);
        common::to_level(ctx, "player_hurt", Value::Int(self.hp as i64));
        if self.hp <= 0 {
            self.die(ctx);
        } else if from_spikes {
            self.return_to_safe(ctx);
        }
    }

    fn die(&mut self, ctx: &mut Ctx) {
        self.dead = true;
        self.vel = Vec2::ZERO;
        let at = Self::feet(ctx) + Vec2::new(0.0, 0.5);
        common::burst(ctx, at, Color::rgb(3.0, 2.0, 0.6), 1.2);
        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            sprite.color = Color::TRANSPARENT;
        }
        common::to_level(ctx, "player_dead", Value::Nil);
    }

    /// Вернуться на последнюю безопасную точку (после ямы или шипов).
    fn return_to_safe(&mut self, ctx: &mut Ctx) {
        self.falls += 1;
        self.vel = Vec2::ZERO;
        self.respawn_time = 0.45;
        self.invuln = self.invuln.max(INVULN);
        self.platform = -1;
        self.dash_time = 0.0;
        Self::set_feet(ctx, self.safe_pos);
    }

    fn fall_into_pit(&mut self, ctx: &mut Ctx) {
        if self.dead {
            return;
        }
        self.hp -= 1;
        common::shake(ctx, 0.45);
        sfx::play(ctx, "hurt");
        common::to_level(ctx, "player_hurt", Value::Int(self.hp as i64));
        if self.hp <= 0 {
            self.die(ctx);
        } else {
            self.return_to_safe(ctx);
        }
    }

    fn step(&mut self, ctx: &mut Ctx, shared: &Shared, inp: &Inp, dt: f32, first: bool) {
        // ---- таймеры ----
        self.invuln = (self.invuln - dt).max(0.0);
        self.hurt_time = (self.hurt_time - dt).max(0.0);
        self.dash_cooldown = (self.dash_cooldown - dt).max(0.0);
        self.drop_timer = (self.drop_timer - dt).max(0.0);
        self.jump_buffer = (self.jump_buffer - dt).max(0.0);
        let was_dashing = self.dash_time > 0.0;
        self.dash_time = (self.dash_time - dt).max(0.0);
        if was_dashing && self.dash_time <= 0.0 {
            self.vel.x = self.dash_dir * RUN * 0.7;
        }
        if first && inp.jump_down {
            self.jump_buffer = BUFFER;
        }

        let feet = Self::feet(ctx);
        let mut body = Body { pos: feet + Vec2::new(0.0, HALF.y), half: HALF, vel: self.vel };

        // ---- рывок ----
        if first && inp.dash_down && self.has_dash && self.can_dash && self.dash_cooldown <= 0.0 && self.hurt_time <= 0.0 {
            self.dash_time = DASH_TIME;
            self.dash_dir = if inp.axis.abs() > 0.2 { inp.axis.signum() } else { self.facing };
            self.facing = self.dash_dir;
            self.can_dash = false;
            self.dash_cooldown = DASH_COOLDOWN;
            self.vel.y = 0.0;
            self.jumping = false;
            sfx::play(ctx, "dash");
            common::dust(ctx, feet + Vec2::new(-self.dash_dir * 0.4, 0.4), 1.2);
            common::shake(ctx, 0.12);
        }

        // ---- горизонталь ----
        if self.dash_time > 0.0 {
            self.vel.x = self.dash_dir * DASH_SPEED;
            self.vel.y = 0.0;
        } else if self.hurt_time > 0.0 {
            let fr = if self.grounded { FRICTION_GROUND } else { FRICTION_AIR };
            self.vel.x = move_towards(self.vel.x, 0.0, fr * 0.25 * dt);
        } else if inp.axis.abs() > 0.05 {
            let target = inp.axis * RUN;
            let mut accel = if self.grounded { ACCEL_GROUND } else { ACCEL_AIR };
            if self.vel.x * inp.axis < 0.0 {
                accel *= 1.6;
            }
            self.vel.x = move_towards(self.vel.x, target, accel * dt);
            self.facing = inp.axis.signum();
        } else {
            let fr = if self.grounded { FRICTION_GROUND } else { FRICTION_AIR };
            self.vel.x = move_towards(self.vel.x, 0.0, fr * dt);
        }

        // ---- прыжок ----
        let control = self.hurt_time <= 0.0 && self.dash_time <= 0.0;
        if control && self.jump_buffer > 0.0 {
            let on_cloud = self.grounded && {
                let grid = shared.grid.borrow();
                grid.flags(body.pos.x.floor() as i32, (body.pos.y - HALF.y - 0.1).floor() as i32) & phys::ONEWAY != 0
            };
            if inp.down && on_cloud {
                self.drop_timer = 0.25;
                self.jump_buffer = 0.0;
                self.grounded = false;
                self.coyote = 0.0;
            } else if self.grounded || self.coyote > 0.0 {
                self.vel.y = JUMP_V;
                self.jumping = true;
                self.grounded = false;
                self.coyote = 0.0;
                self.jump_buffer = 0.0;
                self.squash = -0.18;
                self.platform = -1;
                sfx::play(ctx, "jump");
                common::dust(ctx, feet, 0.6);
            } else if self.has_double_jump && self.air_jumps > 0 {
                self.vel.y = DOUBLE_JUMP_V;
                self.air_jumps -= 1;
                self.jumping = true;
                self.jump_buffer = 0.0;
                self.squash = -0.22;
                sfx::play(ctx, "djump");
                common::sparkle(ctx, feet + Vec2::new(0.0, 0.2), Color::rgb(1.2, 2.6, 3.0));
            }
        }

        // ---- гравитация ----
        if self.dash_time <= 0.0 {
            let mut g = GRAVITY;
            if self.vel.y > 0.0 && !inp.jump_held && self.jumping {
                g *= 2.4;
            } else if self.vel.y < 0.0 {
                g *= 1.12;
            }
            self.vel.y = (self.vel.y - g * dt).max(-FALL_MAX);
        }

        // ---- движение ----
        if first && self.platform >= 0 {
            if let Some(p) = shared.plat(GameObjectId::from_bits(self.platform as u64)) {
                body.pos += p.delta;
            }
        }
        body.vel = self.vel;
        let contact = {
            let grid = shared.grid.borrow();
            let plats = shared.plats.borrow();
            phys::step(&grid, &plats, &mut body, dt, self.drop_timer > 0.0)
        };
        self.vel = body.vel;
        let was_grounded = self.grounded;
        self.grounded = contact.floor;
        self.platform = match contact.platform {
            Some(id) if contact.floor => id.to_bits() as i64,
            _ => -1,
        };
        if contact.floor {
            self.coyote = COYOTE;
            self.air_jumps = if self.has_double_jump { 1 } else { 0 };
            self.can_dash = true;
            self.jumping = false;
            if !was_grounded && contact.impact > 7.0 {
                sfx::play(ctx, "land");
                common::dust(ctx, body.pos - Vec2::new(0.0, HALF.y), 0.9);
                self.squash = (contact.impact / 40.0).min(0.3);
                if contact.impact > 18.0 {
                    common::shake(ctx, 0.12);
                }
            }
        } else {
            self.coyote = (self.coyote - dt).max(0.0);
        }
        if contact.ceiling && self.vel.y > 0.0 {
            self.vel.y = 0.0;
        }
        let feet = body.pos - Vec2::new(0.0, HALF.y);
        Self::set_feet(ctx, feet);

        // ---- безопасная точка ----
        if self.grounded && self.platform < 0 && self.hurt_time <= 0.0 {
            self.safe_timer += dt;
            if self.safe_timer > 0.3 {
                let grid = shared.grid.borrow();
                let ok = grid.support_at(feet + Vec2::new(-0.3, -0.2))
                    && grid.support_at(feet + Vec2::new(0.3, -0.2))
                    && grid.spikes_in(feet + Vec2::new(0.0, 0.5), Vec2::new(1.6, 0.9)).is_none();
                if ok {
                    self.safe_pos = feet;
                }
            }
        } else {
            self.safe_timer = 0.0;
        }

        // ---- шипы и яма ----
        let spiked = shared.grid.borrow().spikes_in(body.pos, HALF * 0.85).is_some();
        if spiked && self.invuln <= 0.0 && self.dash_time <= 0.0 {
            self.hit(ctx, -self.facing, true);
        }
        if feet.y < -3.0 {
            self.fall_into_pit(ctx);
        }
    }

    fn animate(&mut self, ctx: &mut Ctx, dt: f32) {
        let clip = if self.hurt_time > 0.0 {
            "hurt"
        } else if self.dash_time > 0.0 {
            "dash"
        } else if !self.grounded {
            if self.vel.y > 0.0 { "jump" } else { "fall" }
        } else if self.vel.x.abs() > 0.6 {
            "run"
        } else {
            "idle"
        };
        let (facing, invuln, running) = (self.facing, self.invuln, clip == "run");
        if !self.dead {
            if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                sprite.flip_x = facing < 0.0;
                let blink = invuln > 0.0 && ((invuln * 14.0) as i32) % 2 == 0;
                sprite.color = if blink { Color::rgba(1.0, 1.0, 1.0, 0.35) } else { Color::WHITE };
            }
        }
        if let Some(animator) = ctx.get_mut::<SpriteAnimator>() {
            animator.play(clip);
        }
        self.squash = exp_decay(self.squash, 0.0, 14.0, dt);
        let s = self.squash;
        ctx.transform().scale = Vec3::new(1.0 + s, 1.0 - s, 1.0);
        if running && self.grounded {
            self.dust_timer -= dt;
            if self.dust_timer <= 0.0 {
                self.dust_timer = 0.24;
                let at = Self::feet(ctx) + Vec2::new(-self.facing * 0.2, 0.05);
                common::dust(ctx, at, 0.45);
            }
        }
    }
}

impl Component for Player {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.safe_pos = Self::feet(ctx);
        self.hp = MAX_HP;
        self.air_jumps = if self.has_double_jump { 1 } else { 0 };
    }

    // Игрок двигается в late_update: платформы и враги уже отработали в on_update.
    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        if self.respawn_time > 0.0 {
            self.respawn_time = (self.respawn_time - dt).max(0.0);
        }
        self.animate(ctx, dt);
        if self.frozen || self.dead || self.respawn_time > 0.0 {
            return;
        }
        let Some(shared) = ctx.resource::<Shared>().cloned() else {
            return;
        };
        let inp = read_input(ctx);
        let dt = dt.min(0.05);
        let n = ((dt / SUBSTEP) - 1e-3).ceil().max(1.0) as usize;
        let h = dt / n as f32;
        for i in 0..n {
            self.step(ctx, &shared, &inp, h, i == 0);
            if self.dead || self.respawn_time > 0.0 {
                break;
            }
        }
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        match (message.name.as_str(), &message.value) {
            ("Hurt", Value::Vec3(dir, kind, _)) => self.hit(ctx, *dir, *kind > 0.5),
            ("Bounce", Value::Float(v)) => {
                self.vel.y = *v;
                self.jumping = false;
                self.grounded = false;
                self.air_jumps = if self.has_double_jump { 1 } else { 0 };
                self.can_dash = true;
                self.platform = -1;
            }
            ("Revive", Value::Vec3(x, y, _)) => {
                self.dead = false;
                self.hp = MAX_HP;
                self.vel = Vec2::ZERO;
                self.invuln = 2.0;
                self.hurt_time = 0.0;
                self.platform = -1;
                self.safe_pos = Vec2::new(*x, *y);
                Self::set_feet(ctx, Vec2::new(*x, *y));
                if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                    sprite.color = Color::WHITE;
                }
                common::sparkle(ctx, Vec2::new(*x, *y + 0.5), Color::rgb(3.0, 2.4, 0.8));
            }
            ("Freeze", Value::Bool(on)) => {
                self.frozen = *on;
                if *on {
                    self.vel.x = 0.0;
                }
            }
            ("Heal", _) => self.hp = (self.hp + 1).min(MAX_HP),
            _ => {}
        }
    }
}
