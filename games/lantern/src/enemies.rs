//! Враги: ползун, летучая мышь, плеватель, тяжеляк.

use crate::common::{self, PlayerView};
use crate::objects::spawn_projectile;
use crate::phys::{self, Body, Shared};
use crate::player::STOMP_V;
use crate::sfx;
use engine::prelude::*;

/// Итог касания игрока с врагом.
pub enum Hit {
    /// Игрок прыгнул сверху.
    Stomp,
    /// Игрок ранен; значение — в какую сторону его отбросить.
    Hurt(f32),
}

/// Как игрок коснулся врага (центр + половинки). `stompable` — можно ли раздавить сверху.
pub fn contact(p: &PlayerView, center: Vec2, half: Vec2, stompable: bool) -> Option<Hit> {
    if !p.alive || p.dashing || !common::touching(p.center, p.half, center, half) {
        return None;
    }
    let from_above = p.vel.y <= 1.0 && p.feet.y > center.y + half.y * 0.2;
    if stompable && from_above {
        Some(Hit::Stomp)
    } else if p.invulnerable {
        None
    } else {
        Some(Hit::Hurt(if p.center.x >= center.x { 1.0 } else { -1.0 }))
    }
}

/// Что делать с результатом касания: раздавить врага или ранить игрока. `true` — враг убит.
pub fn resolve(ctx: &mut Ctx, p: &PlayerView, hit: Hit, center: Vec2, color: Color) -> bool {
    match hit {
        Hit::Stomp => {
            ctx.send_message(p.id, "Bounce", Value::Float(STOMP_V));
            sfx::play(ctx, "stomp");
            common::burst(ctx, center, color, 0.6);
            common::shake(ctx, 0.2);
            common::to_level(ctx, "enemy_killed", Value::Nil);
            ctx.destroy_self();
            true
        }
        Hit::Hurt(dir) => {
            ctx.send_message(p.id, "Hurt", Value::Vec3(dir, 0.0, 0.0));
            false
        }
    }
}

fn shared(ctx: &Ctx) -> Option<Shared> {
    ctx.resource::<Shared>().cloned()
}

fn set_flip(ctx: &mut Ctx, flip: bool) {
    if let Some(s) = ctx.get_mut::<SpriteRenderer>() {
        s.flip_x = flip;
    }
}

/// Один шаг патрульного существа: идёт вперёд, у края и у стены разворачивается.
/// Параметры хода патрульного существа.
struct Gait {
    speed: f32,
    half: Vec2,
    /// Границы по X, за которые не уходить (если заданы).
    limit: Option<(f32, f32)>,
}

fn patrol(ctx: &mut Ctx, shared: &Shared, dir: &mut f32, vy: &mut f32, gait: Gait, dt: f32) -> Vec2 {
    let Gait { speed, half, limit } = gait;
    let feet = ctx.world_position().truncate();
    let mut body = Body { pos: feet + Vec2::new(0.0, half.y), half, vel: Vec2::new(*dir * speed, *vy) };
    let grid = shared.grid.borrow();
    // у края пропасти — разворот
    let ahead = body.pos.x + *dir * (half.x + 0.1);
    let grounded = grid.support_at(Vec2::new(body.pos.x, feet.y - 0.1));
    if grounded && !grid.support_at(Vec2::new(ahead, feet.y - 0.15)) {
        *dir = -*dir;
        body.vel.x = *dir * speed;
    }
    if let Some((lo, hi)) = limit {
        if (body.pos.x >= hi && *dir > 0.0) || (body.pos.x <= lo && *dir < 0.0) {
            *dir = -*dir;
            body.vel.x = *dir * speed;
        }
    }
    body.vel.y = (body.vel.y - 30.0 * dt).max(-20.0);
    let contact = phys::step(&grid, &[], &mut body, dt, false);
    if contact.wall != 0 {
        *dir = -*dir;
    }
    *vy = body.vel.y;
    let out = body.pos - Vec2::new(0.0, half.y);
    drop(grid);
    ctx.transform().position = out.extend(0.0);
    out
}

// ------------------------------------------------------------------ ползун

component! {
    /// Ползун: ходит по уступу туда-сюда; можно раздавить.
    pub struct Crawler {
        pub dir: f32 = -1.0,
        pub vy: f32 = 0.0,
        pub speed: f32 = 1.6,
        /// Откуда начали и как далеко уходить (0 — до края уступа).
        pub home_x: f32 = f32::NAN,
        pub range: f32 = 0.0,
    }
}

const CRAWLER_HALF: Vec2 = Vec2::new(0.4, 0.3);

impl Component for Crawler {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let dt = dt.min(1.0 / 30.0);
        let Some(shared) = shared(ctx) else { return };
        if self.home_x.is_nan() {
            self.home_x = ctx.world_position().x;
        }
        let limit = (self.range > 0.0).then_some((self.home_x - self.range, self.home_x + self.range));
        let feet = patrol(ctx, &shared, &mut self.dir, &mut self.vy, Gait { speed: self.speed, half: CRAWLER_HALF, limit }, dt);
        set_flip(ctx, self.dir < 0.0);
        let center = feet + Vec2::new(0.0, CRAWLER_HALF.y);
        if let Some(p) = common::player_view(ctx) {
            if let Some(hit) = contact(&p, center, CRAWLER_HALF, true) {
                resolve(ctx, &p, hit, center, Color::rgb(2.4, 0.6, 1.8));
            }
        }
    }
}

// ------------------------------------------------------------------ тяжеляк

component! {
    /// Тяжеляк: шипастая глыба. Раздавить нельзя — обойти или пробежать рывком.
    pub struct Brute {
        pub dir: f32 = -1.0,
        pub vy: f32 = 0.0,
        pub speed: f32 = 1.3,
        pub home_x: f32 = f32::NAN,
        pub range: f32 = 0.0,
    }
}

const BRUTE_HALF: Vec2 = Vec2::new(0.62, 0.6);

impl Component for Brute {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let dt = dt.min(1.0 / 30.0);
        let Some(shared) = shared(ctx) else { return };
        if self.home_x.is_nan() {
            self.home_x = ctx.world_position().x;
        }
        let limit = (self.range > 0.0).then_some((self.home_x - self.range, self.home_x + self.range));
        let feet = patrol(ctx, &shared, &mut self.dir, &mut self.vy, Gait { speed: self.speed, half: BRUTE_HALF, limit }, dt);
        set_flip(ctx, self.dir < 0.0);
        let center = feet + Vec2::new(0.0, BRUTE_HALF.y);
        if let Some(p) = common::player_view(ctx) {
            if let Some(hit) = contact(&p, center, BRUTE_HALF, false) {
                resolve(ctx, &p, hit, center, Color::WHITE);
            }
        }
    }
}

// ------------------------------------------------------------------ летучая мышь

component! {
    /// Летучая мышь: летает по синусоиде вокруг точки привязки.
    pub struct Bat {
        pub anchor: Vec2 = Vec2::ZERO,
        pub range: f32 = 3.0,
        pub amp: f32 = 1.5,
        pub speed: f32 = 1.0,
        pub t: f32 = 0.0,
    }
}

const BAT_HALF: Vec2 = Vec2::new(0.4, 0.3);

impl Component for Bat {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.anchor = ctx.world_position().truncate();
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.t += dt * self.speed;
        let before = ctx.world_position().x;
        let pos = self.anchor + Vec2::new((self.t * 0.8).sin() * self.range, (self.t * 1.9).sin() * self.amp);
        ctx.transform().position = pos.extend(0.0);
        if (pos.x - before).abs() > 1e-4 {
            set_flip(ctx, pos.x < before);
        }
        if let Some(p) = common::player_view(ctx) {
            if let Some(hit) = contact(&p, pos, BAT_HALF, true) {
                resolve(ctx, &p, hit, pos, Color::rgb(2.0, 0.6, 2.6));
            }
        }
    }
}

// ------------------------------------------------------------------ плеватель

component! {
    /// Плеватель: стоит на месте и плюётся в игрока по таймеру; раздавить можно.
    pub struct Spitter {
        pub timer: f32 = 1.5,
        pub period: f32 = 2.6,
        pub windup: f32 = 0.0,
        pub range: f32 = 13.0,
    }
}

const SPITTER_HALF: Vec2 = Vec2::new(0.35, 0.6);

impl Component for Spitter {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let feet = ctx.world_position().truncate();
        let center = feet + Vec2::new(0.0, SPITTER_HALF.y);
        let player = common::player_view(ctx);
        if let Some(p) = player {
            set_flip(ctx, p.center.x < center.x);
            let d = p.center - center;
            let in_range = d.x.abs() < self.range && d.y.abs() < 7.0 && p.alive;
            if self.windup > 0.0 {
                self.windup -= dt;
                if self.windup <= 0.0 {
                    let dir = (p.center - center).normalize_or_zero();
                    let mouth = center + Vec2::new(0.35 * dir.x.signum(), 0.35);
                    spawn_projectile(ctx, mouth, dir * 5.5, 1.0);
                    sfx::play(ctx, "shoot");
                    if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                        a.restart("shoot");
                    }
                }
            } else if in_range {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.timer = self.period;
                    self.windup = 0.4;
                    sfx::play(ctx, "warn");
                    if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                        a.restart("windup");
                    }
                }
            }
            if self.windup <= 0.0 && self.timer < self.period - 0.35 {
                if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                    a.play("idle");
                }
            }
            if let Some(hit) = contact(&p, center, SPITTER_HALF, true) {
                resolve(ctx, &p, hit, center, Color::rgb(0.8, 2.4, 1.4));
            }
        }
    }
}
