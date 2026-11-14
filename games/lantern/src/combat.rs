//! Общее для врагов и босса: касание с игроком (раздавить / ранить) и патрульный ход.

use crate::common::{self, PlayerView};
use crate::phys::{self, Body, Shared};
use crate::scripts::player::STOMP_V;
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

pub fn shared(ctx: &Ctx) -> Option<Shared> {
    ctx.resource::<Shared>().cloned()
}

pub fn set_flip(ctx: &mut Ctx, flip: bool) {
    if let Some(s) = ctx.get_mut::<SpriteRenderer>() {
        s.flip_x = flip;
    }
}

/// Один шаг патрульного существа: идёт вперёд, у края и у стены разворачивается.
/// Параметры хода патрульного существа.
pub struct Gait {
    pub speed: f32,
    pub half: Vec2,
    /// Границы по X, за которые не уходить (если заданы).
    pub limit: Option<(f32, f32)>,
}

pub fn patrol(ctx: &mut Ctx, shared: &Shared, dir: &mut f32, vy: &mut f32, gait: Gait, dt: f32) -> Vec2 {
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

