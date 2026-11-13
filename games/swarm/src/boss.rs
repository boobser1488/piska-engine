//! Поведение боссов: сценарий из шагов [`Step`], каждый шаг — предупреждение
//! (телеграф), исполнение и пауза. Состояние лежит в полях [`Enemy`]:
//! `st` — 0 начало шага, 1 предупреждение, 2 исполнение, 3 пауза/оглушение;
//! `st_t` — таймер состояния; `step` — номер шага; `tele` / `tele_kind` — что рисовать.

use engine::prelude::Vec2;

use crate::data::*;
use crate::sim::{Event, Sim};

/// Виды предупреждений (поле `tele_kind`): кольцо, линия рывка, спираль, цель телепорта.
pub const TELE_PULSE: u8 = 0;
pub const TELE_LINE: u8 = 1;
pub const TELE_SPIRAL: u8 = 2;
pub const TELE_BLINK: u8 = 3;

impl Sim {
    pub(crate) fn update_boss(&mut self, i: usize, dt: f32) {
        let kind = self.enemies[i].kind;
        let info = kind.info();
        let script = boss_script(kind);
        let hp_frac = self.enemies[i].hp / self.enemies[i].max_hp;
        let phase = if kind == Kind::BossOvermind {
            u8::from(hp_frac < 0.66) + u8::from(hp_frac < 0.33)
        } else {
            u8::from(hp_frac < 0.5)
        };
        self.enemies[i].phase = phase;
        let tempo = 1.0 + 0.3 * phase as f32;
        let player_pos = self.player.pos;
        let player_alive = self.player.alive;
        let wave_speed = speed_scale(self.wave);

        let mut want = Vec2::ZERO; // желаемая скорость
        let mut speed_mul = 1.0f32;
        let mut direct = false; // true — скорость задаёт исполнение шага (рывок)

        // ---- конечный автомат шагов ----
        let step = script[self.enemies[i].step as usize % script.len()];
        let st = self.enemies[i].st;
        match st {
            0 => self.begin_step(i, step, tempo),
            1 => {
                speed_mul = 0.3;
                self.enemies[i].st_t -= dt;
                self.enemies[i].tele = self.enemies[i].st_t.max(0.0);
                if matches!(step, Step::Charge) && self.enemies[i].st_t > 0.35 {
                    // Прицел рывка ещё доворачивается за игроком, в конце — фиксируется.
                    let to = if player_alive { player_pos } else { Vec2::ZERO } - self.enemies[i].pos;
                    let cur = self.enemies[i].dir;
                    let a = crate::sim::move_angle(cur.y.atan2(cur.x), to.y.atan2(to.x), 1.6 * dt);
                    self.enemies[i].dir = Vec2::new(a.cos(), a.sin());
                    self.enemies[i].heading = a;
                }
                if self.enemies[i].st_t <= 0.0 {
                    self.enemies[i].tele = 0.0;
                    self.enemies[i].st = 2;
                    self.enemies[i].st_t = 0.0;
                    self.enemies[i].timer = 0.0;
                }
            }
            2 => {
                speed_mul = 0.35;
                direct = self.exec_step(i, step, tempo, dt, &mut want);
            }
            _ => {
                // Пауза: обычное движение; после оглушения уязвимость снимается.
                if self.enemies[i].vulnerable {
                    speed_mul = 0.0;
                }
                self.enemies[i].st_t -= dt;
                if self.enemies[i].st_t <= 0.0 {
                    self.enemies[i].vulnerable = false;
                    self.enemies[i].step = ((self.enemies[i].step as usize + 1) % script.len()) as u8;
                    self.enemies[i].st = 0;
                }
                if !matches!(step, Step::Idle(_)) && !self.enemies[i].vulnerable {
                    speed_mul = 0.6;
                }
            }
        }

        // ---- обычное движение ----
        if !direct {
            let e = &self.enemies[i];
            let radial = e.pos.normalize_or_zero();
            let base = info.speed * wave_speed * speed_mul;
            want = match kind {
                Kind::BossHive | Kind::BossOvermind => {
                    let tangent = radial.perp();
                    (tangent * 0.8 + radial * (4.3 - e.pos.length()) * 0.5) * base
                }
                Kind::BossRam => {
                    let tgt = if player_alive { player_pos } else { Vec2::ZERO };
                    let to = tgt - e.pos;
                    if to.length() < 2.8 {
                        Vec2::ZERO
                    } else {
                        to.normalize_or_zero() * base
                    }
                }
                _ => {
                    let side = if player_alive { -player_pos.normalize_or_zero() } else { Vec2::Y };
                    let goal = side * 5.2;
                    (goal - e.pos).clamp_length_max(1.0) * base
                }
            };
        }
        let e = &mut self.enemies[i];
        let k = if direct { 1.0 } else { 1.0 - (-3.0 * dt).exp() };
        e.vel += (want - e.vel) * k;
        let v = e.vel;
        e.pos += v * dt;
        let max_c = ARENA_R - e.r * 0.55;
        let len = e.pos.length();
        if len > max_c {
            e.pos *= max_c / len;
        }
        if kind == Kind::BossRam {
            if e.vel.length_squared() > 0.5 && st != 1 {
                e.heading = e.vel.y.atan2(e.vel.x);
            }
        } else {
            e.heading += dt * 0.25;
        }
    }

    fn begin_step(&mut self, i: usize, step: Step, tempo: f32) {
        let pos = self.enemies[i].pos;
        let player_pos = if self.player.alive { self.player.pos } else { Vec2::ZERO };
        let e = &mut self.enemies[i];
        let tele = |e: &mut crate::sim::Enemy, secs: f32, kind: u8| {
            e.st = 1;
            e.st_t = secs / tempo;
            e.tele = e.st_t;
            e.tele_kind = kind;
        };
        match step {
            Step::Idle(t) => {
                e.st = 3;
                e.st_t = t / tempo;
                e.tele = 0.0;
            }
            Step::Ring { count, .. } => {
                tele(e, 0.9, TELE_PULSE);
                e.aux = count as f32;
                self.events.push(Event::BossTelegraph { pos, kind: TELE_PULSE });
            }
            Step::Aimed { volleys, .. } => {
                tele(e, 0.6, TELE_PULSE);
                e.aux = volleys as f32;
                self.events.push(Event::BossTelegraph { pos, kind: TELE_PULSE });
            }
            Step::Summon { .. } => {
                tele(e, 1.0, TELE_PULSE);
                self.events.push(Event::BossTelegraph { pos, kind: TELE_PULSE });
            }
            Step::Charge => {
                tele(e, 1.0, TELE_LINE);
                e.dir = (player_pos - pos).normalize_or_zero();
                if e.dir == Vec2::ZERO {
                    e.dir = Vec2::X;
                }
                e.heading = e.dir.y.atan2(e.dir.x);
                self.events.push(Event::BossTelegraph { pos, kind: TELE_LINE });
            }
            Step::Spiral { arms: _, secs: _ } => {
                tele(e, 0.8, TELE_SPIRAL);
                e.dir = if e.step.is_multiple_of(2) { Vec2::X } else { -Vec2::X };
                self.events.push(Event::BossTelegraph { pos, kind: TELE_SPIRAL });
            }
            Step::Blink => {
                tele(e, 0.9, TELE_BLINK);
                let a = self.rng.range(0.0, std::f32::consts::TAU);
                let r = self.rng.range(3.4, 6.4);
                self.enemies[i].aux_pos = Vec2::new(a.cos(), a.sin()) * r;
                self.events.push(Event::BossTelegraph { pos: self.enemies[i].aux_pos, kind: TELE_BLINK });
            }
            Step::Mines { .. } => {
                tele(e, 0.5, TELE_PULSE);
                self.events.push(Event::BossTelegraph { pos, kind: TELE_PULSE });
            }
        }
    }

    /// Исполнение шага; возвращает `true`, если босс сам задаёт скорость (рывок).
    fn exec_step(&mut self, i: usize, step: Step, tempo: f32, dt: f32, want: &mut Vec2) -> bool {
        let pos = self.enemies[i].pos;
        let r = self.enemies[i].r;
        let player_pos = if self.player.alive { self.player.pos } else { Vec2::ZERO };
        let wave_f = self.wave.min(30) as f32;
        let dmg_scale = 1.0 + 0.0 * wave_f;
        match step {
            Step::Idle(_) => {
                self.enemies[i].st = 3;
            }
            Step::Ring { n, speed, count } => {
                self.enemies[i].timer -= dt;
                if self.enemies[i].timer <= 0.0 {
                    if self.enemies[i].aux <= 0.0 {
                        self.finish_step(i, 0.5, tempo);
                        return false;
                    }
                    let ring_idx = count as f32 - self.enemies[i].aux;
                    self.enemies[i].aux -= 1.0;
                    self.enemies[i].timer = 0.5 / tempo;
                    let off = ring_idx * std::f32::consts::PI / n as f32 + self.enemies[i].heading;
                    for k in 0..n {
                        let a = off + k as f32 * std::f32::consts::TAU / n as f32;
                        let d = Vec2::new(a.cos(), a.sin());
                        self.fire_enemy_bullet(pos + d * r * 0.7, d, speed * (1.0 + 0.08 * (tempo - 1.0) / 0.3), 10.0 * dmg_scale, 0.9);
                    }
                    self.events.push(Event::EnemyShot { pos });
                }
            }
            Step::Aimed { per, .. } => {
                self.enemies[i].timer -= dt;
                if self.enemies[i].timer <= 0.0 {
                    if self.enemies[i].aux <= 0.0 {
                        self.finish_step(i, 0.6, tempo);
                        return false;
                    }
                    self.enemies[i].aux -= 1.0;
                    self.enemies[i].timer = 0.34 / tempo;
                    let dir = (player_pos - pos).normalize_or_zero();
                    let base = dir.y.atan2(dir.x);
                    for k in 0..per {
                        let a = base + (k as f32 - (per - 1) as f32 * 0.5) * 0.2;
                        self.fire_enemy_bullet(pos + dir * r * 0.7, Vec2::new(a.cos(), a.sin()), 7.2, 11.0 * dmg_scale, 0.85);
                    }
                    self.events.push(Event::EnemyShot { pos });
                }
            }
            Step::Summon { kind, count } => {
                let n = count as u32 + if self.enemies[i].phase >= 1 { count as u32 / 4 + 1 } else { 0 };
                for k in 0..n {
                    let a = k as f32 * std::f32::consts::TAU / n as f32 + self.enemies[i].heading;
                    let d = Vec2::new(a.cos(), a.sin());
                    let spawn = pos + d * (r + 0.7);
                    let spawn = if spawn.length() > ARENA_R - 0.6 { spawn.normalize() * (ARENA_R - 0.6) } else { spawn };
                    if let Some(j) = self.spawn_enemy_ex(kind, spawn, false) {
                        self.enemies[j].knock = d * 6.0;
                    }
                }
                self.events.push(Event::Explosion { pos, radius: r + 0.8, hostile: true });
                self.finish_step(i, 0.8, tempo);
            }
            Step::Charge => {
                let dir = self.enemies[i].dir;
                let speed = (15.0 + 2.0 * (tempo - 1.0) / 0.3).min(19.0);
                self.enemies[i].aux += dt;
                *want = dir * speed;
                let len = pos.length();
                let hit_wall = len + r * 0.55 >= ARENA_R - 0.05;
                let hit_core = len <= CORE_R + r * 0.9 + 0.15 && self.enemies[i].aux > 0.1;
                if hit_wall || hit_core || self.enemies[i].aux > 1.7 {
                    self.events.push(Event::BossImpact { pos });
                    if hit_core {
                        self.hurt_core(35.0);
                    }
                    for k in 0..12 {
                        let a = k as f32 * std::f32::consts::TAU / 12.0;
                        let d = Vec2::new(a.cos(), a.sin());
                        self.fire_enemy_bullet(pos + d * r * 0.8, d, 5.5, 10.0, 0.85);
                    }
                    self.enemies[i].vel = Vec2::ZERO;
                    self.enemies[i].vulnerable = true;
                    self.enemies[i].aux = 0.0;
                    self.enemies[i].st = 3;
                    self.enemies[i].st_t = 1.9;
                    return false;
                }
                return true;
            }
            Step::Spiral { secs, arms } => {
                self.enemies[i].aux += dt;
                self.enemies[i].timer -= dt;
                let sign = self.enemies[i].dir.x;
                // Фаза спирали: aux — прошедшее время.
                if self.enemies[i].timer <= 0.0 {
                    self.enemies[i].timer = 0.085 / tempo;
                    let base = self.enemies[i].aux * 2.4 * sign;
                    for k in 0..arms {
                        let a = base + k as f32 * std::f32::consts::TAU / arms as f32;
                        let d = Vec2::new(a.cos(), a.sin());
                        self.fire_enemy_bullet(pos + d * r * 0.5, d, 5.0, 9.0 * dmg_scale, 0.75);
                    }
                }
                self.enemies[i].tele = 0.0;
                if self.enemies[i].aux >= secs {
                    self.finish_step(i, 0.7, tempo);
                }
            }
            Step::Blink => {
                let target = self.enemies[i].aux_pos;
                self.events.push(Event::Explosion { pos, radius: r, hostile: true });
                self.enemies[i].pos = target;
                self.enemies[i].vel = Vec2::ZERO;
                for k in 0..10 {
                    let a = k as f32 * std::f32::consts::TAU / 10.0;
                    let d = Vec2::new(a.cos(), a.sin());
                    self.fire_enemy_bullet(target + d * r * 0.8, d, 5.0, 9.0, 0.8);
                }
                self.events.push(Event::BossImpact { pos: target });
                self.finish_step(i, 0.5, tempo);
            }
            Step::Mines { count } => {
                for k in 0..count {
                    let a = k as f32 * std::f32::consts::TAU / count as f32 + self.enemies[i].heading;
                    let d = Vec2::new(a.cos(), a.sin());
                    let mut spawn = pos + d * (r + 1.0);
                    if spawn.length() > ARENA_R - 1.0 {
                        spawn = spawn.normalize() * (ARENA_R - 1.0);
                    }
                    if self.mines_alive < 12 {
                        self.spawn_enemy_ex(Kind::Mine, spawn, false);
                        self.events.push(Event::MinePlaced { pos: spawn });
                    }
                }
                self.finish_step(i, 0.6, tempo);
            }
        }
        false
    }

    fn finish_step(&mut self, i: usize, recover: f32, tempo: f32) {
        let e = &mut self.enemies[i];
        e.st = 3;
        e.st_t = recover / tempo;
        e.tele = 0.0;
        e.aux = 0.0;
    }
}
