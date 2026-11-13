//! Предметы и механизмы уровня: светлячки, контрольные точки, пружины, платформы,
//! ворота, выход, подсказки, снаряды и падающие камни.

use crate::common::{self, layer};
use crate::level::LevelManager;
use crate::phys::{Plat, Shared};
use crate::sfx;
use engine::prelude::*;

/// Состояние менеджера уровня, нужное предметам.
fn level_info(ctx: &Ctx) -> Option<(i32, bool, bool)> {
    let id = ctx.find_by_name("Level")?;
    let l = ctx.get_on::<LevelManager>(id)?;
    Some((l.fireflies, l.exit_ready(), l.is_playing()))
}

// ------------------------------------------------------------------ светлячок

component! {
    /// Светлячок: дрейфует, тянется к игроку вблизи, при касании засчитывается.
    pub struct Firefly {
        /// Золотой (секретный) — особый звук и счёт.
        pub golden: bool = false,
        pub home: Vec2 = Vec2::ZERO,
        pub phase: f32 = 0.0,
        pub t: f32 = 0.0,
    }
}

impl Component for Firefly {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.home = ctx.world_position().truncate();
        self.phase = self.home.x * 1.7 + self.home.y * 0.9;
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.t += dt;
        let player = common::player_view(ctx);
        if let Some(p) = player {
            let to = p.center - self.home;
            let d = to.length();
            let reach = if self.golden { 3.2 } else { 2.4 };
            if p.alive && d < reach && d > 0.001 {
                self.home += to / d * (3.0 + (reach - d) * 2.0) * dt;
            }
        }
        let pos = self.home + Vec2::new((self.t * 0.9 + self.phase).sin() * 0.3, (self.t * 1.3 + self.phase * 1.3).cos() * 0.24);
        ctx.transform().position = pos.extend(0.0);
        if let Some(p) = player {
            let radius = if self.golden { 0.95 } else { 0.8 };
            if p.alive && (p.center - pos).length() < radius && level_info(ctx).is_none_or(|l| l.2) {
                if self.golden {
                    sfx::play(ctx, "golden");
                    common::sparkle(ctx, pos, Color::rgb(3.4, 2.2, 0.5));
                    common::shake(ctx, 0.2);
                    common::to_level(ctx, "golden", Value::Nil);
                } else {
                    sfx::play(ctx, "firefly");
                    common::sparkle(ctx, pos, Color::rgb(2.6, 2.6, 1.2));
                    common::to_level(ctx, "firefly", Value::Nil);
                }
                ctx.destroy_self();
            }
        }
    }
}

// ------------------------------------------------------------------ контрольная точка

component! {
    /// Фонарный столб: касание запоминает место возрождения и лечит на сердце.
    pub struct Checkpoint {
        pub active: bool = false,
    }
}

impl Component for Checkpoint {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.active {
            return;
        }
        let Some(p) = common::player_view(ctx) else { return };
        let feet = ctx.world_position().truncate();
        if p.alive && common::touching(p.center, p.half, feet + Vec2::new(0.0, 0.75), Vec2::new(0.6, 0.9)) {
            self.active = true;
            if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                sprite.frame = 1;
            }
            sfx::play(ctx, "checkpoint");
            common::sparkle(ctx, feet + Vec2::new(0.0, 0.9), Color::rgb(3.0, 2.2, 0.7));
            common::to_level(ctx, "checkpoint", Value::Vec3(feet.x, feet.y, 0.0));
            ctx.send_message(p.id, "Heal", Value::Nil);
        }
    }
}

// ------------------------------------------------------------------ пружина

component! {
    /// Пружина: подбрасывает игрока.
    pub struct Spring {
        pub power: f32 = 20.0,
        pub anim: f32 = 0.0,
    }
}

impl Component for Spring {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.anim = (self.anim - dt).max(0.0);
        let frame = if self.anim > 0.0 { 1 } else { 0 };
        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            sprite.frame = frame;
        }
        let Some(p) = common::player_view(ctx) else { return };
        let feet = ctx.world_position().truncate();
        if p.alive && self.anim <= 0.0 && p.vel.y <= 1.0 && common::touching(p.center, p.half, feet + Vec2::new(0.0, 0.3), Vec2::new(0.5, 0.3)) {
            self.anim = 0.3;
            ctx.send_message(p.id, "Bounce", Value::Float(self.power));
            sfx::play(ctx, "spring");
            common::dust(ctx, feet, 0.7);
        }
    }
}

// ------------------------------------------------------------------ платформы

component! {
    /// Платформа, ходящая между двумя точками (точка — середина верхней грани).
    pub struct MovingPlatform {
        pub from: Vec2 = Vec2::ZERO,
        pub to: Vec2 = Vec2::ZERO,
        /// Скорость фазы, рад/с (полный цикл туда-обратно — 2π / speed).
        pub speed: f32 = 1.0,
        pub t: f32 = 0.0,
        pub width: f32 = 3.0,
    }
}

impl MovingPlatform {
    fn position(&self) -> Vec2 {
        self.from.lerp(self.to, 0.5 - 0.5 * self.t.cos())
    }
}

impl Component for MovingPlatform {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let p = self.position();
        ctx.transform().position = p.extend(0.0);
        self.publish(ctx, Vec2::ZERO);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let before = self.position();
        self.t += dt * self.speed;
        let now = self.position();
        ctx.transform().position = now.extend(0.0);
        self.publish(ctx, now - before);
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.retract(ctx.id);
        }
    }
}

impl MovingPlatform {
    fn publish(&self, ctx: &mut Ctx, delta: Vec2) {
        let p = self.position();
        let half = self.width * 0.5;
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.publish(Plat { id: ctx.id, min: Vec2::new(p.x - half, p.y - 0.5), max: Vec2::new(p.x + half, p.y), delta });
        }
    }
}

component! {
    /// Хрупкая платформа: через полсекунды после того, как на неё встали, рассыпается,
    /// через пару секунд собирается снова.
    pub struct CrumblePlatform {
        /// 0 — цела, 1 — трескается, 2 — рассыпалась.
        pub state: i32 = 0,
        pub timer: f32 = 0.0,
        pub home: Vec2 = Vec2::ZERO,
        pub width: f32 = 3.0,
    }
}

impl CrumblePlatform {
    fn publish(&self, ctx: &mut Ctx) {
        let half = self.width * 0.5;
        let p = self.home;
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.publish(Plat { id: ctx.id, min: Vec2::new(p.x - half, p.y - 0.5), max: Vec2::new(p.x + half, p.y), delta: Vec2::ZERO });
        }
    }
}

const CRUMBLE_DELAY: f32 = 0.55;
const CRUMBLE_RESPAWN: f32 = 2.8;

impl Component for CrumblePlatform {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.home = ctx.world_position().truncate();
        self.publish(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        match self.state {
            0 => {
                if let Some(p) = common::player_view(ctx) {
                    if p.platform == ctx.id.to_bits() as i64 {
                        self.state = 1;
                        self.timer = CRUMBLE_DELAY;
                        sfx::play(ctx, "crumble");
                    }
                }
            }
            1 => {
                self.timer -= dt;
                let k = 1.0 - (self.timer / CRUMBLE_DELAY).clamp(0.0, 1.0);
                let jitter = (self.timer * 70.0).sin() * 0.05 * k;
                ctx.transform().position = (self.home + Vec2::new(jitter, 0.0)).extend(0.0);
                if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                    sprite.frame = ((k * 3.0) as u32).min(2);
                }
                if self.timer <= 0.0 {
                    self.state = 2;
                    self.timer = CRUMBLE_RESPAWN;
                    if let Some(shared) = ctx.resource::<Shared>() {
                        shared.retract(ctx.id);
                    }
                    if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                        sprite.color = Color::TRANSPARENT;
                    }
                    for dx in [-1.0, 0.0, 1.0] {
                        common::dust(ctx, self.home + Vec2::new(dx, -0.3), 1.0);
                    }
                    common::shake(ctx, 0.15);
                }
            }
            _ => {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.state = 0;
                    ctx.transform().position = self.home.extend(0.0);
                    if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
                        sprite.color = Color::WHITE;
                        sprite.frame = 0;
                    }
                    self.publish(ctx);
                    common::sparkle(ctx, self.home, Color::rgb(1.6, 1.6, 2.4));
                }
            }
        }
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if let Some(shared) = ctx.resource::<Shared>() {
            shared.retract(ctx.id);
        }
    }
}

// ------------------------------------------------------------------ ворота

component! {
    /// Запертые ворота: открываются, когда собрано достаточно светлячков.
    pub struct Gate {
        pub need: i32 = 5,
        pub open: bool = false,
        pub nag: f32 = 0.0,
        /// Высота ворот в клетках.
        pub height: i32 = 3,
    }
}

impl Gate {
    fn set_solid(&self, ctx: &Ctx, solid: bool) {
        let p = ctx.world_position();
        let (x, y) = (p.x.floor() as i32, p.y.round() as i32);
        if let Some(shared) = ctx.resource::<Shared>() {
            let mut grid = shared.grid.borrow_mut();
            for dy in 0..self.height {
                grid.set_flags(x, y + dy, if solid { crate::phys::SOLID } else { 0 });
            }
        }
    }
}

impl Component for Gate {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.set_solid(ctx, true);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if self.open {
            return;
        }
        self.nag = (self.nag - dt).max(0.0);
        let Some(p) = common::player_view(ctx) else { return };
        let pos = ctx.world_position().truncate();
        let near = (p.center.x - pos.x).abs() < 2.0 && p.center.y > pos.y - 0.5 && p.center.y < pos.y + self.height as f32 + 0.5;
        if !near || !p.alive {
            return;
        }
        let fireflies = level_info(ctx).map_or(0, |l| l.0);
        if fireflies >= self.need {
            self.open = true;
            self.set_solid(ctx, false);
            sfx::play(ctx, "gate");
            common::shake(ctx, 0.3);
            for k in 0..3 {
                common::dust(ctx, pos + Vec2::new(0.0, 0.5 + k as f32), 1.0);
            }
            let id = ctx.id;
            ctx.start_tween(Tween::position(id, Vec3::new(pos.x, pos.y + 3.2, 0.0)).duration(0.8).ease(Ease::InOutQuad));
            ctx.start_tween(Tween::field(id, "SpriteRenderer.color.a", 0.0f32).duration(0.8).delay(0.3));
            ctx.destroy_self_after(1.2);
        } else if self.nag <= 0.0 {
            self.nag = 2.5;
            sfx::play(ctx, "error");
            common::to_level(ctx, "need_more", Value::Int(self.need as i64));
        }
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if !self.open {
            self.set_solid(ctx, false);
        }
    }
}

// ------------------------------------------------------------------ выход

component! {
    /// Выход: загорается, когда собрано нужное число светлячков (и побеждён босс, если он есть).
    pub struct Exit {
        pub active: bool = false,
        pub used: bool = false,
        pub nag: f32 = 0.0,
    }
}

impl Component for Exit {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.nag = (self.nag - dt).max(0.0);
        let Some((_, ready, playing)) = level_info(ctx) else { return };
        if ready != self.active {
            self.active = ready;
            if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
                a.play(if ready { "on" } else { "off" });
            }
            if let Some(glow) = ctx.children().first().copied() {
                if let Some(ps) = ctx.scene_mut().get_component_mut::<ParticleSystem>(glow) {
                    ps.playing = ready;
                }
            }
            if ready {
                sfx::play(ctx, "checkpoint");
                let at = ctx.world_position().truncate() + Vec2::new(0.0, 1.0);
                common::sparkle(ctx, at, Color::rgb(3.0, 2.2, 0.8));
            }
        }
        let Some(p) = common::player_view(ctx) else { return };
        let pos = ctx.world_position().truncate();
        if p.alive && playing && common::touching(p.center, p.half, pos + Vec2::new(0.0, 1.0), Vec2::new(0.6, 1.0)) {
            if self.active && !self.used {
                self.used = true;
                common::to_level(ctx, "exit_reached", Value::Nil);
            } else if !self.active && self.nag <= 0.0 {
                self.nag = 2.5;
                sfx::play(ctx, "error");
                common::to_level(ctx, "exit_locked", Value::Nil);
            }
        }
    }
}

// ------------------------------------------------------------------ подсказка

component! {
    /// Невидимая зона: при входе игрока показывает подсказку один раз.
    pub struct Hint {
        pub text: String = String::new(),
        pub size: Vec2 = Vec2::new(2.0, 2.0),
        pub shown: bool = false,
    }
}

impl Component for Hint {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.shown {
            return;
        }
        let Some(p) = common::player_view(ctx) else { return };
        let pos = ctx.world_position().truncate();
        if p.alive && common::touching(p.center, p.half, pos, self.size * 0.5) {
            self.shown = true;
            common::to_level(ctx, "hint", Value::Str(self.text.clone()));
        }
    }
}

// ------------------------------------------------------------------ снаряд

component! {
    /// Медленный плевок: летит по прямой, разбивается о стену, ранит игрока.
    pub struct Projectile {
        pub vel: Vec2 = Vec2::ZERO,
        pub life: f32 = 7.0,
        pub radius: f32 = 0.22,
    }
}

impl Component for Projectile {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.life -= dt;
        let pos = ctx.world_position().truncate() + self.vel * dt;
        ctx.transform().position = pos.extend(0.0);
        ctx.transform().rotate_z(dt * 6.0);
        let hit_wall = ctx.resource::<Shared>().is_some_and(|s| s.grid.borrow().solid_at(pos));
        if hit_wall || self.life <= 0.0 {
            common::burst(ctx, pos, Color::rgb(2.4, 0.5, 1.2), 0.3);
            ctx.destroy_self();
            return;
        }
        if let Some(p) = common::player_view(ctx) {
            if p.alive && !p.dashing && (p.center - pos).length() < self.radius + 0.3 {
                if !p.invulnerable {
                    let dir = if self.vel.x >= 0.0 { 1.0 } else { -1.0 };
                    ctx.send_message(p.id, "Hurt", Value::Vec3(dir, 0.0, 0.0));
                }
                common::burst(ctx, pos, Color::rgb(2.4, 0.5, 1.2), 0.3);
                ctx.destroy_self();
            }
        }
    }
}

/// Выпустить плевок.
pub fn spawn_projectile(ctx: &mut Ctx, at: Vec2, vel: Vec2, scale: f32) -> GameObjectId {
    let gfx = ctx.resource::<common::Gfx>().cloned().unwrap_or_default();
    let mut sprite = SpriteRenderer::textured(gfx.tex("projectile")).with_size(0.7 * scale, 0.7 * scale);
    sprite.sorting_layer = layer::ENEMIES;
    sprite.order = 3;
    ctx.world
        .scene
        .build("Projectile")
        .at(at.x, at.y)
        .with(sprite)
        .with(Projectile { vel, radius: 0.22 * scale, ..Projectile::default() })
        .with(TrailRenderer::glowing(Color::rgb(2.2, 0.5, 1.2), 0.25 * scale, 0.35))
        .id()
}

// ------------------------------------------------------------------ падающий камень

component! {
    /// Сталактит босса: дрожит, потом падает.
    pub struct Rock {
        pub warn: f32 = 0.9,
        pub vy: f32 = 0.0,
        pub falling: bool = false,
        pub home_x: f32 = f32::NAN,
    }
}

impl Component for Rock {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let pos = ctx.world_position().truncate();
        if self.home_x.is_nan() {
            self.home_x = pos.x;
        }
        if !self.falling {
            self.warn -= dt;
            let jitter = (self.warn * 90.0).sin() * 0.07;
            ctx.transform().position = Vec3::new(self.home_x + jitter, pos.y, 0.0);
            if self.warn <= 0.0 {
                self.falling = true;
            }
            return;
        }
        self.vy = (self.vy - 34.0 * dt).max(-22.0);
        let new = Vec2::new(self.home_x, pos.y + self.vy * dt);
        ctx.transform().position = new.extend(0.0);
        let tip = new - Vec2::new(0.0, 2.0);
        let ground = ctx.resource::<Shared>().is_some_and(|s| s.grid.borrow().solid_at(tip));
        if let Some(p) = common::player_view(ctx) {
            if p.alive && !p.invulnerable && common::touching(p.center, p.half, new - Vec2::new(0.0, 1.0), Vec2::new(0.35, 1.0)) {
                ctx.send_message(p.id, "Hurt", Value::Vec3(if p.center.x > new.x { 1.0 } else { -1.0 }, 0.0, 0.0));
            }
        }
        if ground {
            common::dust(ctx, tip + Vec2::new(0.0, 0.2), 1.3);
            common::shake(ctx, 0.25);
            sfx::play(ctx, "land");
            ctx.destroy_self();
        }
    }
}
