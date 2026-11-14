//! Нагрузочный режим для замера FPS (`SWARM_STRESS=1`).

use engine::prelude::*;

use crate::data::*;
use crate::defs::defs;
use crate::sim::*;

impl Sim {
    /// Заполнить арену до 400 врагов и 600 пуль (для замера FPS: `SWARM_STRESS=1`).
    /// Бессмертный дрон, враги живут и бродят, пули отскакивают от стены.
    pub fn fill_stress(&mut self) {
        self.player.invuln = 1.0;
        self.core.hp = self.core.max_hp;
        if self.phase == Phase::Intermission {
            self.intermission_t = 99.0;
        }
        let kinds = [Kind::Chaser, Kind::Swarmling, Kind::Shooter, Kind::Splitter, Kind::SplitterMini, Kind::Bomber];
        while self.alive_enemies() < 400 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let r = self.rng.range(2.5, arena_r() - 0.5);
            let k = kinds[self.rng.int(0, kinds.len() as i32 - 1) as usize];
            let Some(i) = self.spawn_enemy(k, Vec2::new(a.cos(), a.sin()) * r) else { break };
            // Враги стрессового режима очень живучи и не взрываются.
            self.enemies[i].hp = 1.0e9;
            self.enemies[i].max_hp = 1.0e9;
            self.enemies[i].target_core = false;
            if k == Kind::Bomber {
                self.enemies[i].kind = Kind::Chaser;
            }
        }
        while self.pbullets_alive() < 300 {
            let Some(slot) = self.alloc_pbullet() else { break };
            let d = self.rng.unit();
            let serial = self.next_serial;
            self.next_serial += 1;
            let mut b = Bullet::blank();
            b.alive = true;
            b.serial = serial;
            b.pos = d * self.rng.range(1.5, 8.0);
            b.vel = self.rng.unit() * 18.0;
            b.life = 4.0;
            b.bounces = 3;
            b.dmg = 0.0;
            self.pbullets[slot] = b;
        }
        while self.ebullets_alive() < 300 {
            let d = self.rng.unit();
            let pos = d * self.rng.range(1.5, 8.0);
            let v = self.rng.unit();
            self.fire_enemy_bullet(pos, v, 4.0, 0.0, 0.8);
            if self.ebullets.len() >= EBULLET_CAP && self.ebullets_alive() >= EBULLET_CAP - 5 {
                break;
            }
        }
    }
}
