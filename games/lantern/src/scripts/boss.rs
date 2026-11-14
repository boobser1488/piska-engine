//! Хозяин Тьмы — босс шестого уровня. Три фазы по числу оставшихся «жизней»:
//! 1) парит и плюётся; 2) веер плевков и таран; 3) ещё и падающие сталактиты.

use crate::common;
use crate::combat::{self, Hit};
use crate::phys::Shared;
use crate::scripts::player::STOMP_V;
use crate::sfx;
use engine::prelude::*;

pub const BOSS_HP: i32 = 9;
const HALF: Vec2 = Vec2::new(1.25, 1.0);

/// Состояния боя.
pub mod st {
    pub const DORMANT: i32 = 0;
    pub const INTRO: i32 = 1;
    pub const HOVER: i32 = 2;
    pub const TELEGRAPH: i32 = 3;
    pub const CHARGE: i32 = 4;
    pub const DAZED: i32 = 5;
    pub const DYING: i32 = 6;
}

component! {
    /// Босс: арена задаётся полями (границы по X, пол, потолок, дверь, которая запирается).
    pub struct Boss {
        pub hp: i32 = 9,
        pub state: i32 = 0,
        pub timer: f32 = 0.0,
        pub t: f32 = 0.0,
        pub dir: f32 = 1.0,
        pub shoot_timer: f32 = 2.0,
        pub charge_timer: f32 = 6.0,
        pub rock_timer: f32 = 3.0,
        pub invuln: f32 = 0.0,
        pub charge_dir: f32 = 1.0,
        /// Левая и правая границы арены по X.
        pub arena_x: Vec2 = Vec2::ZERO,
        pub floor_y: f32 = 0.0,
        pub ceiling_y: f32 = 0.0,
        /// Дверь, которая закрывается на время боя: клетка X и диапазон Y (включительно).
        pub door: Vec3 = Vec3::ZERO,
        pub door_y1: f32 = 0.0,
        pub home: Vec2 = Vec2::ZERO,
        /// Тайлмап уровня, в котором дверь закладывается плитками (ссылка, задаётся в сцене).
        pub tiles: ComponentRef<Tilemap> = ComponentRef::NONE,
    }
}

impl Boss {
    fn phase(&self) -> i32 {
        if self.hp > 6 {
            1
        } else if self.hp > 3 {
            2
        } else {
            3
        }
    }

    fn animate(&self, ctx: &mut Ctx, clip: &str) {
        if let Some(a) = ctx.get_mut::<SpriteAnimator>() {
            a.play(clip);
        }
    }

    fn seal_door(&self, ctx: &mut Ctx) {
        let x = self.door.x as i32;
        let (y0, y1) = (self.door.y as i32, self.door_y1 as i32);
        if let Some(shared) = ctx.resource::<Shared>() {
            let mut grid = shared.grid.borrow_mut();
            for y in y0..=y1 {
                grid.set_flags(x, y, crate::phys::SOLID);
            }
            let height = grid.height;
            drop(grid);
            if let Some(map) = self.tiles.get(ctx.scene()) {
                if let Some(tm) = ctx.scene_mut().get_component_mut::<Tilemap>(map) {
                    for y in y0..=y1 {
                        tm.set_tile(x as usize, (height - 1 - y) as usize, 22);
                    }
                }
            }
        }
        common::shake(ctx, 0.5);
        sfx::play(ctx, "gate");
    }

    fn shoot(&self, ctx: &mut Ctx, from: Vec2, target: Vec2) {
        let base = (target - from).normalize_or_zero();
        let count = match self.phase() {
            1 => 1,
            2 => 3,
            _ => 5,
        };
        for i in 0..count {
            let spread = (i as f32 - (count as f32 - 1.0) / 2.0) * 0.30;
            let (s, c) = spread.sin_cos();
            let dir = Vec2::new(base.x * c - base.y * s, base.x * s + base.y * c);
            common::fire_projectile(ctx, from + dir * 1.0, dir * 5.8, 1.3);
        }
        sfx::play(ctx, "shoot");
    }

    fn drop_rocks(&self, ctx: &mut Ctx) {
        for _ in 0..2 {
            let x = ctx.random().range(self.arena_x.x + 1.5, self.arena_x.y - 1.5);
            if let Err(err) = ctx.instantiate_at("prefabs/rock.prefab", Vec3::new(x, self.ceiling_y, 0.0)) {
                log::error!("rock: {err}");
            }
        }
        sfx::play(ctx, "warn");
    }
}

impl Component for Boss {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.home = ctx.world_position().truncate();
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.t += dt;
        self.invuln = (self.invuln - dt).max(0.0);
        let mut pos = ctx.world_position().truncate();
        let player = common::player_view(ctx);
        let hover_y = self.floor_y + 3.8 + (self.t * 2.0).sin() * 0.3;
        let phase = self.phase();
        let speed = [0.0, 2.6, 3.5, 4.4][phase as usize];

        match self.state {
            st::DORMANT => {
                pos.y = hover_y;
                pos.x = self.home.x + (self.t * 0.6).sin() * 1.5;
                if let Some(p) = player {
                    if p.alive && p.center.x > self.arena_x.x + 2.5 {
                        self.state = st::INTRO;
                        self.timer = 2.2;
                        self.seal_door(ctx);
                        common::to_level(ctx, "boss_start", Value::Vec3(self.hp as f32, p.feet.x, p.feet.y));
                        sfx::music(ctx, "music_boss", 0.8);
                    }
                }
            }
            st::INTRO => {
                self.timer -= dt;
                pos.y = hover_y;
                self.animate(ctx, "telegraph");
                if (self.timer * 20.0).sin() > 0.0 {
                    common::shake(ctx, 0.02);
                }
                if self.timer <= 0.0 {
                    self.state = st::HOVER;
                    self.animate(ctx, "fly");
                    sfx::play(ctx, "boss_hit");
                    common::shake(ctx, 0.5);
                }
            }
            st::HOVER => {
                if self.invuln > 0.0 {
                    self.animate(ctx, "hurt");
                } else {
                    self.animate(ctx, "fly");
                }
                pos.y = exp_decay(pos.y, hover_y, 4.0, dt);
                pos.x += self.dir * speed * dt;
                let (lo, hi) = (self.arena_x.x + 2.6, self.arena_x.y - 2.6);
                if pos.x > hi {
                    pos.x = hi;
                    self.dir = -1.0;
                } else if pos.x < lo {
                    pos.x = lo;
                    self.dir = 1.0;
                }
                self.shoot_timer -= dt;
                if self.shoot_timer <= 0.0 {
                    self.shoot_timer = [0.0, 2.4, 2.2, 2.0][phase as usize];
                    if let Some(p) = player.filter(|p| p.alive) {
                        self.shoot(ctx, pos, p.center);
                    }
                }
                if phase >= 3 {
                    self.rock_timer -= dt;
                    if self.rock_timer <= 0.0 {
                        self.rock_timer = 3.4;
                        self.drop_rocks(ctx);
                    }
                }
                if phase >= 2 {
                    self.charge_timer -= dt;
                    if self.charge_timer <= 0.0 && self.invuln <= 0.0 {
                        self.charge_timer = if phase == 2 { 7.0 } else { 5.5 };
                        self.state = st::TELEGRAPH;
                        self.timer = 0.9;
                        self.charge_dir = match player {
                            Some(p) if p.center.x < pos.x => -1.0,
                            _ => 1.0,
                        };
                        sfx::play(ctx, "warn");
                    }
                }
            }
            st::TELEGRAPH => {
                self.timer -= dt;
                self.animate(ctx, "telegraph");
                pos.x += (self.t * 60.0).sin() * 0.03;
                pos.y = exp_decay(pos.y, self.floor_y + 2.6, 5.0, dt);
                if self.timer <= 0.0 {
                    self.state = st::CHARGE;
                    sfx::play(ctx, "dash");
                    common::shake(ctx, 0.3);
                }
            }
            st::CHARGE => {
                pos.y = exp_decay(pos.y, self.floor_y + 1.0, 12.0, dt);
                pos.x += self.charge_dir * 13.0 * dt;
                let (lo, hi) = (self.arena_x.x + 1.4, self.arena_x.y - 1.4);
                if pos.x >= hi || pos.x <= lo {
                    pos.x = pos.x.clamp(lo, hi);
                    self.state = st::DAZED;
                    self.timer = 2.4;
                    common::shake(ctx, 0.6);
                    common::dust(ctx, Vec2::new(pos.x, self.floor_y + 0.2), 2.0);
                    sfx::play(ctx, "land");
                }
            }
            st::DAZED => {
                self.timer -= dt;
                self.animate(ctx, "hurt");
                pos.y = exp_decay(pos.y, self.floor_y + 1.0, 8.0, dt);
                if self.timer <= 0.0 {
                    self.state = st::HOVER;
                    self.shoot_timer = 1.2;
                    self.dir = if pos.x > (self.arena_x.x + self.arena_x.y) * 0.5 { -1.0 } else { 1.0 };
                }
            }
            _ => {
                // гибель: дрожит и взрывается
                self.timer -= dt;
                self.animate(ctx, "die");
                pos.y -= dt * 0.6;
                if (self.t * 7.0).fract() < dt * 7.0 {
                    let dx = ctx.random().range(-1.2, 1.2);
                    let dy = ctx.random().range(-1.0, 1.0);
                    common::burst(ctx, pos + Vec2::new(dx, dy), Color::rgb(3.0, 1.4, 0.5), 0.9);
                    common::shake(ctx, 0.3);
                }
                if self.timer <= 0.0 {
                    common::burst(ctx, pos, Color::rgb(3.4, 2.4, 1.0), 2.4);
                    common::to_level(ctx, "boss_defeated", Value::Nil);
                    ctx.destroy_self();
                    return;
                }
            }
        }
        ctx.transform().position = pos.extend(0.0);

        // ---- касание с игроком ----
        if self.state == st::DORMANT || self.state == st::INTRO || self.state == st::DYING {
            return;
        }
        if let Some(p) = player {
            let stompable = self.state != st::CHARGE && self.invuln <= 0.0;
            if let Some(hit) = combat::contact(&p, pos, HALF, stompable) {
                match hit {
                    Hit::Stomp => {
                        self.hp -= 1;
                        self.invuln = 1.5;
                        ctx.send_message(p.id, "Bounce", Value::Float(STOMP_V + 1.5));
                        sfx::play(ctx, "boss_hit");
                        common::shake(ctx, 0.6);
                        common::burst(ctx, pos + Vec2::new(0.0, 0.8), Color::rgb(3.0, 1.6, 0.6), 1.0);
                        common::to_level(ctx, "boss_hp", Value::Int(self.hp as i64));
                        if self.hp <= 0 {
                            self.state = st::DYING;
                            self.timer = 2.2;
                            sfx::play(ctx, "boss_die");
                        } else if self.state == st::DAZED {
                            self.state = st::HOVER;
                            self.shoot_timer = 1.5;
                        }
                    }
                    Hit::Hurt(dir) => {
                        ctx.send_message(p.id, "Hurt", Value::Vec3(dir, 0.0, 0.0));
                    }
                }
            }
        }
    }
}
