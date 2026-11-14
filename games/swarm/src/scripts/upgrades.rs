//! Улучшения: три случайные карточки (веса из `data/upgrades.ron`), выбор, применение.

use engine::prelude::*;

use crate::data::*;
use crate::defs::defs;
use crate::sim::*;

impl Sim {
    /// Три случайные карточки (без повторов, без исчерпанных).
    pub fn roll_offer(&mut self) -> Vec<Up> {
        let mut pool: Vec<Up> = Up::ALL.iter().copied().filter(|u| self.levels[u.index()] < u.info().max).collect();
        let mut out = Vec::new();
        while out.len() < self.offer_count && !pool.is_empty() {
            let weights: Vec<f32> = pool.iter().map(|u| u.info().weight).collect();
            let i = self.rng.weighted(&weights);
            out.push(pool.swap_remove(i));
        }
        self.offer = Some(out.clone());
        out
    }

    /// Применить улучшение. `false` — нечего выбирать или улучшение исчерпано.
    pub fn choose_upgrade(&mut self, up: Up) -> bool {
        if self.pending_upgrades == 0 || self.levels[up.index()] >= up.info().max {
            return false;
        }
        self.apply_upgrade(up);
        self.pending_upgrades -= 1;
        self.offer = None;
        self.run.upgrades += 1;
        self.events.push(Event::UpgradeChosen(up));
        true
    }

    /// Награда, когда все улучшения исчерпаны: чинит всё понемногу.
    pub fn skip_upgrade(&mut self) {
        if self.pending_upgrades > 0 {
            self.pending_upgrades -= 1;
            self.core.hp = self.core.max_hp;
            self.player.hp = self.stats.max_hp;
            self.offer = None;
        }
    }

    /// Поднять уровень улучшения (без проверки наград): так его применяют и тесты.
    pub fn apply_upgrade(&mut self, up: Up) {
        let old = self.stats.clone();
        self.levels[up.index()] += 1;
        self.stats = Stats::from_levels(&self.levels);
        match up {
            Up::Hull => {
                self.player.hp += self.stats.max_hp - old.max_hp;
            }
            Up::Repair => {
                self.core.hp = (self.core.hp + 30.0).min(self.core.max_hp);
            }
            Up::Armor => {
                self.core.max_hp = self.stats.core_max;
                self.core.hp += self.stats.core_max - old.core_max;
            }
            Up::Missile if old.missile_count == 0 => {
                self.player.missile_t = 1.0;
            }
            _ => {}
        }
        self.player.hp = self.player.hp.min(self.stats.max_hp);
    }
}

component! {
    /// Выбор улучшений.
    pub struct Upgrades {
        /// Сколько карточек предлагается (1-3).
        pub offer_count: i32 = 3,
    }
}

impl Component for Upgrades {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<crate::scripts::game_manager::Game>() else { return };
        let n = self.offer_count.clamp(1, 3) as usize;
        game.cfg.offer_count = n;
        game.sim.offer_count = n;
        ctx.world.resources.insert(game);
    }
}
