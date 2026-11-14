//! Снаряды: пули игрока (пробитие, рикошет, взрыв, криты), ракеты с наведением, пули врагов.

use engine::prelude::*;

use crate::data::*;
use crate::defs::defs;
use crate::sim::*;

impl Sim {
    pub(crate) fn update_player_bullets(&mut self, dt: f32) {
        let mut hits = std::mem::take(&mut self.scratch);
        for b in 0..self.pbullets.len() {
            if !self.pbullets[b].alive {
                continue;
            }
            // Самонаведение ракет.
            if self.pbullets[b].missile {
                self.steer_missile(b, dt);
            }
            {
                let bl = &mut self.pbullets[b];
                bl.pos += bl.vel * dt;
                bl.life -= dt;
                if bl.life <= 0.0 {
                    bl.alive = false;
                    self.free_pb.push(b);
                    continue;
                }
            }
            // Стена арены.
            let (pos, r) = (self.pbullets[b].pos, self.pbullets[b].r);
            let dist = pos.length();
            if dist + r > arena_r() {
                if self.pbullets[b].bounces > 0 && !self.pbullets[b].missile {
                    let n = pos / dist.max(1e-3);
                    let bl = &mut self.pbullets[b];
                    bl.vel -= n * (2.0 * bl.vel.dot(n));
                    bl.pos = n * (arena_r() - r - 0.02);
                    bl.bounces -= 1;
                    bl.life = bl.life.max(0.45);
                    self.events.push(Event::Ricochet { pos });
                } else {
                    let explode = self.pbullets[b].explode;
                    if self.pbullets[b].missile && explode > 0.0 {
                        self.explode_player(pos, explode, self.pbullets[b].dmg * 0.6, 0);
                    }
                    self.pbullets[b].alive = false;
                    self.free_pb.push(b);
                    self.events.push(Event::BulletWall { pos, hostile: false });
                    continue;
                }
            }
            // Враги.
            let pos = self.pbullets[b].pos;
            hits.clear();
            self.grid.query(pos, r, |id| hits.push(id));
            let mut dead = false;
            for &id in &hits {
                let i = id as usize;
                if !self.enemies[i].alive || self.pbullets[b].has_hit(self.enemies[i].uid) {
                    continue;
                }
                // Пуля могла уйти: проверяем по актуальным позициям.
                if self.enemies[i].pos.distance(pos) > r + self.enemies[i].r {
                    continue;
                }
                let (dmg, crit, explode, uid) = {
                    let bl = &self.pbullets[b];
                    (bl.dmg, bl.crit, bl.explode, self.enemies[i].uid)
                };
                self.pbullets[b].note_hit(uid);
                self.damage_enemy(i, dmg, crit);
                if explode > 0.0 {
                    self.explode_player(pos, explode, dmg * if self.pbullets[b].missile { 0.6 } else { 0.55 }, uid);
                }
                if self.pbullets[b].missile || self.pbullets[b].pierce == 0 {
                    dead = true;
                    break;
                }
                self.pbullets[b].pierce -= 1;
            }
            if dead {
                self.pbullets[b].alive = false;
                self.free_pb.push(b);
            }
        }
        self.scratch = hits;
    }

    pub(crate) fn explode_player(&mut self, pos: Vec2, radius: f32, dmg: f32, skip_uid: u32) {
        self.events.push(Event::Explosion { pos, radius, hostile: false });
        self.area_damage(pos, radius, dmg, skip_uid);
    }

    pub(crate) fn steer_missile(&mut self, b: usize, dt: f32) {
        let pos = self.pbullets[b].pos;
        let mut target = None;
        if self.pbullets[b].target != 0 {
            target = self.enemies.iter().position(|e| e.alive && e.uid == self.pbullets[b].target);
        }
        if target.is_none() {
            let mut best = f32::MAX;
            for (i, e) in self.enemies.iter().enumerate() {
                if e.alive && e.kind != Kind::Mine {
                    let d = e.pos.distance_squared(pos);
                    if d < best {
                        best = d;
                        target = Some(i);
                    }
                }
            }
            self.pbullets[b].target = target.map_or(0, |i| self.enemies[i].uid);
        }
        let bl = &mut self.pbullets[b];
        let speed = (bl.vel.length() + 18.0 * dt).min(13.0);
        if let Some(i) = target {
            let want = (self.enemies[i].pos - pos).normalize_or_zero();
            let cur = bl.vel.normalize_or_zero();
            let ang = cur.y.atan2(cur.x);
            let target_ang = want.y.atan2(want.x);
            let new_ang = move_angle(ang, target_ang, 6.5 * dt);
            bl.vel = Vec2::new(new_ang.cos(), new_ang.sin()) * speed;
        } else {
            bl.vel = bl.vel.normalize_or_zero() * speed;
        }
    }

    pub(crate) fn update_enemy_bullets(&mut self, dt: f32) {
        let shields = if self.stats.shield_count > 0 { self.shield_positions() } else { Vec::new() };
        for b in 0..self.ebullets.len() {
            if !self.ebullets[b].alive {
                continue;
            }
            {
                let bl = &mut self.ebullets[b];
                bl.pos += bl.vel * dt;
                bl.life -= dt;
            }
            let (pos, r, dmg) = (self.ebullets[b].pos, self.ebullets[b].r, self.ebullets[b].dmg);
            let mut kill = self.ebullets[b].life <= 0.0;
            if pos.length() + r > arena_r() + 0.3 {
                kill = true;
                self.events.push(Event::BulletWall { pos, hostile: true });
            } else if self.player.alive && self.player.invuln <= 0.0 && pos.distance(self.player.pos) < r + player_r() {
                self.hurt_player(dmg);
                kill = true;
            } else if pos.length() < r + core_r() {
                self.hurt_core(dmg * 0.8);
                self.events.push(Event::BulletWall { pos, hostile: true });
                kill = true;
            } else if shields.iter().any(|s| s.distance(pos) < r + 0.32) {
                self.events.push(Event::ShieldHit { pos });
                kill = true;
            }
            if kill {
                self.ebullets[b].alive = false;
                self.free_eb.push(b);
            }
        }
    }
}

component! {
    /// Снаряды: цвета следов. Урон, скорострельность, пробитие и остальное — из
    /// улучшений (`data/upgrades.ron`) и `data/balance.ron`.
    pub struct Weapons {
        pub bullet_trail: Color = Color::rgb(0.25, 1.5, 2.2),
        pub crit_trail: Color = Color::rgb(2.8, 2.0, 0.5),
        pub missile_trail: Color = Color::rgb(2.6, 1.2, 0.4),
        pub crit_scale: f32 = 1.3,
    }
}

impl Component for Weapons {}
