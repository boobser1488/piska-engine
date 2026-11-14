//! Поведение обычных врагов по видам: гонщик, рой-мальок, стрелок, танк, делитель,
//! подрывник, минёр, мина. Числа — из `data/enemies.ron`.

use engine::prelude::*;

use crate::data::*;
use crate::defs::defs;
use crate::sim::*;

impl Sim {
    pub(crate) fn target_pos(&self, e: &Enemy) -> Vec2 {
        if e.target_core || !self.player.alive {
            Vec2::ZERO
        } else {
            self.player.pos
        }
    }

    pub(crate) fn update_enemies(&mut self, dt: f32) {
        let stats_slow = (self.stats.slow_strength, self.stats.slow_radius);
        let speed_mul = speed_scale(self.wave);
        let player_pos = self.player.pos;
        let player_alive = self.player.alive;
        let shields = if self.stats.shield_count > 0 { self.shield_positions() } else { Vec::new() };
        let mut hits = std::mem::take(&mut self.scratch);
        for i in 0..self.enemies.len() {
            if !self.enemies[i].alive {
                continue;
            }
            // Копия нужных полей: дальше метод может менять другие сущности.
            let kind = self.enemies[i].kind;
            let info = kind.info();
            {
                let e = &mut self.enemies[i];
                e.age += dt;
                e.flash = (e.flash - dt).max(0.0);
                e.touch_cd = (e.touch_cd - dt).max(0.0);
                e.shield_cd = (e.shield_cd - dt).max(0.0);
                e.slow = if stats_slow.0 > 0.0 && player_alive && e.pos.distance(player_pos) < stats_slow.1 + e.r && !kind.is_boss() { 1.0 - stats_slow.0 } else { 1.0 };
                e.knock *= (-6.0 * dt).exp();
            }
            let tgt = self.target_pos(&self.enemies[i]);
            let mut desired = Vec2::ZERO;
            let mut speed = info.speed * speed_mul * self.enemies[i].slow;
            match kind {
                Kind::Chaser | Kind::Splitter | Kind::SplitterMini | Kind::Tank => {
                    desired = (tgt - self.enemies[i].pos).normalize_or_zero() * speed;
                }
                Kind::Swarmling => {
                    let e = &self.enemies[i];
                    let to = (tgt - e.pos).normalize_or_zero();
                    let wob = (e.age * 6.0 + e.uid as f32).sin() * 0.55;
                    let (s, c) = wob.sin_cos();
                    desired = Vec2::new(to.x * c - to.y * s, to.x * s + to.y * c) * speed;
                }
                Kind::Bomber => {
                    let e = &self.enemies[i];
                    let to = tgt - e.pos;
                    let dist = to.length();
                    let target_r = if e.target_core || !player_alive { core_r() } else { player_r() };
                    if dist < target_r + e.r + 0.9 {
                        let pos = e.pos;
                        self.remove_enemy(i);
                        self.events.push(Event::EnemyDied { pos, kind });
                        self.blast_hostile(pos, 2.3, 24.0, 24.0, false);
                        continue;
                    }
                    speed *= 1.0 + (e.age * 0.12).min(0.5);
                    desired = to / dist.max(1e-3) * speed;
                }
                Kind::Shooter => {
                    let e = &self.enemies[i];
                    let to = player_pos - e.pos;
                    let dist = to.length();
                    let dir = to / dist.max(1e-3);
                    desired = if !player_alive {
                        (Vec2::ZERO - e.pos).normalize_or_zero() * speed * 0.5
                    } else if dist > 8.0 {
                        dir * speed
                    } else if dist < 5.2 {
                        -dir * speed
                    } else {
                        dir.perp() * speed * 0.5 * if e.uid.is_multiple_of(2) { 1.0 } else { -1.0 }
                    };
                    let e = &mut self.enemies[i];
                    e.timer -= dt;
                    e.st = u8::from(e.timer < 0.35);
                    if e.timer <= 0.0 && player_alive {
                        e.timer = self.rng.range(2.0, 2.8) / (1.0 + 0.02 * self.wave as f32);
                        let pos = e.pos;
                        let n = if self.wave >= 12 { 3 } else { 1 };
                        let base = dir.y.atan2(dir.x);
                        for k in 0..n {
                            let a = base + (k as f32 - (n - 1) as f32 * 0.5) * 0.28;
                            self.fire_enemy_bullet(pos + dir * 0.5, Vec2::new(a.cos(), a.sin()), 6.2, 8.0, 0.8);
                        }
                        self.events.push(Event::EnemyShot { pos });
                    }
                }
                Kind::MineLayer => {
                    let e = &self.enemies[i];
                    let radial = e.pos.normalize_or_zero();
                    let tangent = radial.perp() * e.dir.x;
                    let want_r = 6.6;
                    let mut d = tangent + radial * ((want_r - e.pos.length()) * 0.5);
                    if player_alive && e.pos.distance(player_pos) < 3.0 {
                        d += (e.pos - player_pos).normalize_or_zero() * 1.5;
                    }
                    desired = d.normalize_or_zero() * speed;
                    let e = &mut self.enemies[i];
                    e.timer -= dt;
                    if e.timer <= 0.0 {
                        e.timer = 3.2;
                        let pos = e.pos;
                        if self.mines_alive < 12 {
                            if let Some(j) = self.spawn_enemy_ex(Kind::Mine, pos, false) {
                                self.enemies[j].pos = pos;
                                self.events.push(Event::MinePlaced { pos });
                            }
                        }
                    }
                }
                Kind::Mine => {
                    let e = &mut self.enemies[i];
                    e.timer -= dt;
                    let armed = e.timer <= 0.0;
                    if e.timer <= 0.0 && e.timer + dt > 0.0 {
                        let pos = e.pos;
                        self.events.push(Event::MineArmed { pos });
                    }
                    let e = &self.enemies[i];
                    if e.age > 28.0 {
                        self.remove_enemy(i);
                        continue;
                    }
                    if armed {
                        let trig_p = player_alive && e.pos.distance(player_pos) < e.r + player_r() + 0.55;
                        let trig_c = e.pos.length() < e.r + core_r() + 0.45;
                        if trig_p || trig_c {
                            let pos = e.pos;
                            self.remove_enemy(i);
                            self.events.push(Event::EnemyDied { pos, kind });
                            self.blast_hostile(pos, 2.0, 22.0, 22.0, false);
                            continue;
                        }
                    }
                }
                Kind::BossHive | Kind::BossRam | Kind::BossConductor | Kind::BossOvermind => {
                    self.update_boss(i, dt);
                    if !self.enemies[i].alive {
                        continue;
                    }
                    // Босс двигается сам (см. boss.rs), здесь только общие проверки.
                }
            }
            // Интеграция движения (боссы двигаются в update_boss, но скорость берут отсюда же).
            if !kind.is_boss() {
                let e = &mut self.enemies[i];
                e.vel += (desired - e.vel) * (1.0 - (-8.0 * dt).exp());
                let step = (e.vel + e.knock) * dt;
                e.pos += step;
                if info.faces_heading {
                    if desired.length_squared() > 0.01 {
                        e.heading = desired.y.atan2(desired.x);
                    }
                } else {
                    e.heading += dt * 1.4 * if e.uid.is_multiple_of(2) { 1.0 } else { -1.0 };
                }
            }
            // Границы арены и реактор.
            {
                let e = &mut self.enemies[i];
                let r = e.pos.length();
                let limit = arena_r() - 0.25;
                if r > limit {
                    e.pos *= limit / r;
                }
                let min_r = core_r() + e.r * 0.9;
                if kind != Kind::Mine && r < min_r {
                    e.pos = if r > 1e-3 { e.pos / r * min_r } else { Vec2::new(0.0, min_r) };
                }
            }
            // Касание реактора.
            {
                let e = &self.enemies[i];
                if kind != Kind::Mine && !kind.is_boss() && e.touch_cd <= 0.0 && e.pos.length() < core_r() + e.r + 0.12 && info.contact > 0.0 {
                    let dmg = if kind == Kind::Tank { info.contact } else { info.contact * 0.6 };
                    self.enemies[i].touch_cd = 0.9;
                    self.hurt_core(dmg);
                }
            }
            // Касание дрона.
            {
                let e = &self.enemies[i];
                if player_alive && info.contact > 0.0 && self.player.invuln <= 0.0 && e.pos.distance(self.player.pos) < e.r + player_r() {
                    let away = (self.player.pos - e.pos).normalize_or_zero();
                    let dmg = info.contact;
                    if !kind.is_boss() {
                        self.enemies[i].knock -= away * 6.0;
                    }
                    self.hurt_player(dmg);
                    self.player.vel += away * 6.0;
                }
            }
            // Щиты.
            if !shields.is_empty() && self.enemies[i].shield_cd <= 0.0 {
                let e = &self.enemies[i];
                let mut hit_at = None;
                for s in &shields {
                    if s.distance(e.pos) < e.r + 0.32 {
                        hit_at = Some(*s);
                        break;
                    }
                }
                if let Some(s) = hit_at {
                    let away = (self.enemies[i].pos - s).normalize_or_zero();
                    self.enemies[i].shield_cd = 0.4;
                    if !kind.is_boss() {
                        self.enemies[i].knock += away * 7.0;
                    }
                    self.events.push(Event::ShieldHit { pos: s });
                    self.damage_enemy(i, self.stats.shield_damage, false);
                }
            }
        }
        // Мягкое расталкивание врагов (по сетке этого кадра).
        for i in 0..self.enemies.len() {
            if !self.enemies[i].alive || self.enemies[i].kind == Kind::Mine {
                continue;
            }
            let (pos, r, kind) = (self.enemies[i].pos, self.enemies[i].r, self.enemies[i].kind);
            if kind.is_boss() {
                continue;
            }
            hits.clear();
            self.grid.query(pos, r, |id| hits.push(id));
            let mut push = Vec2::ZERO;
            for &id in &hits {
                let j = id as usize;
                if j == i || !self.enemies[j].alive || self.enemies[j].kind == Kind::Mine {
                    continue;
                }
                let d = pos - self.enemies[j].pos;
                let dist = d.length();
                let min = r + self.enemies[j].r;
                if dist < min {
                    let wi = r * r;
                    let wj = self.enemies[j].r * self.enemies[j].r;
                    let dir = if dist > 1e-3 { d / dist } else { Vec2::from_angle(i as f32) };
                    push += dir * (min - dist) * (wj / (wi + wj));
                }
            }
            self.enemies[i].pos += push * (1.0 - (-10.0 * dt).exp()).clamp(0.2, 0.9);
        }
        self.scratch = hits;
    }
}

component! {
    /// Враг: внешний вид вида. Висит на префабах `prefabs/enemies/*` (характеристики —
    /// в `data/enemies.ron`, поле `kind` — имя записи оттуда).
    pub struct EnemyKind {
        pub kind: String = String::from("Chaser"),
        /// Покачивание размера при движении (доля).
        pub wobble: f32 = 0.04,
        /// Сколько секунд спрайт «вырастает» после появления.
        pub spawn_pop: f32 = 0.3,
    }
}

impl Component for EnemyKind {}
