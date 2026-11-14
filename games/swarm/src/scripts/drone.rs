//! Дрон игрока: движение, рывок, стрельба залпом, ракеты, орбитальные щиты.

use engine::prelude::*;

use crate::data::*;
use crate::defs::defs;
use crate::sim::*;

impl Sim {
    pub(crate) fn update_player(&mut self, input: &PlayerInput, dt: f32) {
        let stats = self.stats.clone();
        let over = self.is_over();
        let p = &mut self.player;
        p.invuln = (p.invuln - dt).max(0.0);
        p.fire_cd = (p.fire_cd - dt).max(0.0);
        p.dash_cd = (p.dash_cd - dt).max(0.0);
        p.shield_phase += dt * 3.2;
        if input.aim.length_squared() > 0.01 {
            p.aim = input.aim.normalize();
        }
        if !p.alive {
            p.respawn_t -= dt;
            if p.respawn_t <= 0.0 && !over {
                p.alive = true;
                p.pos = PLAYER_START;
                p.vel = Vec2::ZERO;
                p.hp = stats.max_hp;
                p.invuln = 2.0;
                self.events.push(Event::PlayerRespawn);
            }
            return;
        }
        // Движение и рывок.
        if input.dash && p.dash_cd <= 0.0 {
            let dir = if input.mv.length_squared() > 0.04 { input.mv.normalize() } else { p.aim };
            p.dash_dir = dir;
            p.dash_t = defs().balance.dash_time;
            p.dash_cd = stats.dash_cooldown;
            p.invuln = p.invuln.max(0.28);
            p.dash_hit.clear();
            self.events.push(Event::Dash { pos: p.pos, dir });
        }
        if p.dash_t > 0.0 {
            p.dash_t -= dt;
            p.vel = p.dash_dir * defs().balance.dash_speed;
        } else {
            let want = input.mv.clamp_length_max(1.0) * stats.move_speed;
            p.vel += (want - p.vel) * (1.0 - (-14.0 * dt).exp());
        }
        p.pos += p.vel * dt;
        // Арена и реактор.
        let r = p.pos.length();
        if r > arena_r() - 0.5 {
            p.pos *= (arena_r() - 0.5) / r;
        }
        let min_core = core_r() + player_r() + 0.05;
        if r < min_core {
            p.pos = if r > 1e-3 { p.pos / r * min_core } else { Vec2::new(0.0, -min_core) };
        }
        // Стрельба.
        let (pos, aim) = (p.pos, p.aim);
        if input.fire && p.fire_cd <= 0.0 {
            p.fire_cd = stats.fire_interval;
            p.muzzle = 0.06;
            self.shoot_volley(pos, aim, &stats);
        }
        // Ракеты.
        if stats.missile_count > 0 {
            self.player.missile_t -= dt;
            if self.player.missile_t <= 0.0 {
                self.player.missile_t = stats.missile_interval;
                self.launch_missiles(&stats);
            }
        }
        // Урон рывком.
        if self.player.dash_t > 0.0 && stats.dash_damage > 0.0 {
            let mut hits = std::mem::take(&mut self.scratch);
            hits.clear();
            let pp = self.player.pos;
            self.grid.query(pp, player_r() + 0.25, |id| hits.push(id));
            for &id in &hits {
                let i = id as usize;
                let uid = self.enemies[i].uid;
                if self.enemies[i].alive && !self.player.dash_hit.contains(&uid) {
                    self.player.dash_hit.push(uid);
                    let away = (self.enemies[i].pos - pp).normalize_or_zero();
                    if !self.enemies[i].kind.is_boss() {
                        self.enemies[i].knock += away * 9.0;
                    }
                    self.damage_enemy(i, stats.dash_damage, false);
                }
            }
            self.scratch = hits;
        }
    }

    pub(crate) fn shoot_volley(&mut self, pos: Vec2, aim: Vec2, stats: &Stats) {
        let n = stats.shots as i32;
        let base = aim.y.atan2(aim.x);
        let step = 0.12;
        self.run.shots += 1;
        self.events.push(Event::Shot { pos: pos + aim * 0.6, dir: aim });
        for k in 0..n {
            let Some(slot) = self.alloc_pbullet() else { return };
            let ang = base + (k as f32 - (n - 1) as f32 * 0.5) * step + self.rng.range(-0.012, 0.012);
            let dir = Vec2::new(ang.cos(), ang.sin());
            let crit = self.rng.chance(stats.crit_chance);
            let serial = self.next_serial;
            self.next_serial += 1;
            let mut b = Bullet::blank();
            b.alive = true;
            b.serial = serial;
            b.pos = pos + dir * 0.6;
            b.vel = dir * 22.0;
            b.r = 0.15;
            b.dmg = stats.damage * if crit { 2.0 } else { 1.0 };
            b.life = 0.85;
            b.pierce = stats.pierce;
            b.bounces = stats.bounces;
            b.explode = stats.explode_radius;
            b.crit = crit;
            self.pbullets[slot] = b;
        }
    }

    pub(crate) fn launch_missiles(&mut self, stats: &Stats) {
        let pos = self.player.pos;
        for k in 0..stats.missile_count {
            let Some(slot) = self.alloc_pbullet() else { return };
            let ang = self.player.aim.y.atan2(self.player.aim.x) + std::f32::consts::FRAC_PI_2 * if k % 2 == 0 { 1.0 } else { -1.0 } + self.rng.range(-0.4, 0.4);
            let dir = Vec2::new(ang.cos(), ang.sin());
            let serial = self.next_serial;
            self.next_serial += 1;
            let mut b = Bullet::blank();
            b.alive = true;
            b.serial = serial;
            b.pos = pos + dir * 0.5;
            b.vel = dir * 6.0;
            b.r = 0.28;
            b.dmg = stats.damage * 3.5;
            b.life = 4.0;
            b.explode = 1.5;
            b.missile = true;
            self.pbullets[slot] = b;
            self.events.push(Event::MissileLaunch { pos });
        }
    }

    /// Позиции орбитальных щитов.
    pub fn shield_positions(&self) -> Vec<Vec2> {
        let n = self.stats.shield_count;
        (0..n)
            .map(|k| {
                let a = self.player.shield_phase + k as f32 * std::f32::consts::TAU / n as f32;
                self.player.pos + Vec2::new(a.cos(), a.sin()) * 1.75
            })
            .collect()
    }
}

component! {
    /// Дрон игрока: внешний вид (цвета и растяжение при рывке). Само движение и стрельба
    /// считаются в симуляции (`Sim::update_player`), числа — в `data/balance.ron`.
    pub struct Drone {
        /// Растяжение спрайта во время рывка.
        pub dash_stretch: Vec2 = Vec2::new(1.45, 0.75),
        pub dash_color: Color = Color::rgb(1.6, 2.4, 3.0),
        pub hurt_color: Color = Color::rgb(3.0, 1.0, 1.0),
        /// Частота мигания неуязвимости, Гц.
        pub blink_hz: f32 = 34.0,
        pub muzzle_color: Color = Color::rgb(0.6, 2.2, 3.0),
        /// Вспышка у ствола (ребёнок префаба; ссылка — объект можно переименовать).
        pub muzzle: ObjectRef = ObjectRef::NONE,
        /// Поле замедления вокруг дрона (ребёнок префаба).
        pub slow_field: ObjectRef = ObjectRef::NONE,
    }
}

impl Component for Drone {}
