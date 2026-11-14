//! Волны: расписание появлений по плану из `data/waves.ron`, метки на краю арены,
//! конец волны и передышка. Компонент `Waves` на менеджере хранит настройки показа.

use engine::prelude::*;

use crate::data::*;
use crate::defs::defs;
use crate::sim::*;

impl Sim {
    /// Начать волну `n` сразу (план, счётчики, событие).
    pub fn start_wave(&mut self, n: u32) {
        self.wave = n;
        self.plan = plan_wave(n);
        self.wave_time = 0.0;
        self.cursor = 0;
        self.boss_spawned = false;
        self.vacuum = false;
        self.phase = Phase::Wave;
        self.events.push(Event::WaveStart { wave: n, boss: self.plan.boss });
    }

    pub(crate) fn queue_group(&mut self, g: SpawnGroup) {
        let base = self.rng.range(0.0, std::f32::consts::TAU);
        let edge = arena_r() - 0.7;
        for k in 0..g.count {
            let (ang, rad, t) = match g.pattern {
                Pattern::Scatter => (self.rng.range(0.0, std::f32::consts::TAU), edge, 1.0 + 0.05 * k as f32),
                Pattern::Pack => (base + self.rng.range(-0.16, 0.16), edge - self.rng.range(0.0, 0.8), 1.0 + 0.03 * k as f32),
                Pattern::Ring => (base + k as f32 * std::f32::consts::TAU / g.count as f32, edge, 1.0),
            };
            let pos = Vec2::new(ang.cos(), ang.sin()) * rad;
            let target_core = self.pick_core_target(g.kind);
            self.telegraphs.push(Telegraph { pos, t, total: t, kind: g.kind, target_core });
        }
    }

    pub(crate) fn pick_core_target(&mut self, kind: Kind) -> bool {
        let def = kind.info();
        let p = if self.wave >= def.core_chance_from_wave { def.core_chance } else { 0.0 };
        p >= 1.0 || (p > 0.0 && self.rng.chance(p))
    }

    pub(crate) fn update_waves(&mut self, dt: f32) {
        match self.phase {
            Phase::Intermission => {
                self.intermission_t -= dt;
                if self.intermission_t <= 0.0 {
                    self.start_wave(self.wave + 1);
                }
            }
            Phase::Wave => {
                self.wave_time += dt;
                while self.cursor < self.plan.groups.len() && self.plan.groups[self.cursor].at <= self.wave_time {
                    let g = self.plan.groups[self.cursor].clone();
                    self.cursor += 1;
                    self.queue_group(g);
                }
                if let (Some(kind), false) = (self.plan.boss, self.boss_spawned) {
                    if self.wave_time >= 0.4 {
                        self.boss_spawned = true;
                        let pos = Vec2::new(0.0, arena_r() - 2.6);
                        self.telegraphs.push(Telegraph { pos, t: 2.4, total: 2.4, kind, target_core: false });
                        self.events.push(Event::BossAppear { kind });
                    }
                }
                let spawn_done = self.cursor >= self.plan.groups.len() && (self.plan.boss.is_none() || self.boss_spawned);
                if spawn_done && self.telegraphs.is_empty() && self.alive_enemies() == 0 {
                    self.finish_wave();
                }
            }
            Phase::Cleared => {
                self.cleared_t += dt;
                if self.pending_upgrades == 0 {
                    self.begin_intermission();
                }
            }
            Phase::Won | Phase::Lost => {}
        }
    }

    pub(crate) fn begin_intermission(&mut self) {
        self.phase = Phase::Intermission;
        self.intermission_t = 2.6;
    }

    pub(crate) fn finish_wave(&mut self) {
        // Мины после волны гаснут без взрыва.
        for i in 0..self.enemies.len() {
            if self.enemies[i].alive && self.enemies[i].kind == Kind::Mine {
                self.remove_enemy(i);
            }
        }
        self.ebullets_clear();
        self.vacuum = true;
        self.events.push(Event::WaveCleared { wave: self.wave });
        let bonus = (100.0 * self.wave as f32 * (0.5 + 0.5 * self.core.hp / self.core.max_hp)) as u64;
        self.score += bonus;
        self.core.hp = (self.core.hp + 0.06 * self.core.max_hp).min(self.core.max_hp);
        if self.mode == Mode::Campaign && self.wave >= CAMPAIGN_WAVES {
            self.phase = Phase::Won;
            self.events.push(Event::Victory);
            return;
        }
        self.phase = Phase::Cleared;
        self.cleared_t = 0.0;
        self.pending_upgrades += 1;
    }

    pub(crate) fn ebullets_clear(&mut self) {
        for i in 0..self.ebullets.len() {
            if self.ebullets[i].alive {
                self.ebullets[i].alive = false;
                self.free_eb.push(i);
            }
        }
    }

    pub(crate) fn update_telegraphs(&mut self, dt: f32) {
        let mut i = 0;
        while i < self.telegraphs.len() {
            self.telegraphs[i].t -= dt;
            if self.telegraphs[i].t <= 0.0 {
                let t = self.telegraphs.swap_remove(i);
                if self.spawn_enemy_ex(t.kind, t.pos, t.target_core).is_none() {
                    // Нет места: попробуем чуть позже.
                    self.telegraphs.push(Telegraph { t: 0.25, total: 0.25, ..t });
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
}

component! {
    /// Показ волн: тексты крупных надписей. {wave} и {boss} подставляются при показе.
    pub struct Waves {
        pub wave_title: String = String::from("ВОЛНА {wave}"),
        pub boss_line: String = String::from("Босс: {boss}"),
        pub clear_title: String = String::from("ВОЛНА ПРОЙДЕНА"),
        pub clear_sub: String = String::from("Волна {wave} позади"),
        pub warning_title: String = String::from("ВНИМАНИЕ!"),
        pub unlock_text: String = String::from("Открыт бесконечный режим!"),
        pub boss_down_title: String = String::from("БОСС УНИЧТОЖЕН"),
    }
}

impl Component for Waves {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<crate::scripts::game_manager::Game>() else { return };
        let c = &mut game.cfg;
        c.wave_title = self.wave_title.clone();
        c.boss_line = self.boss_line.clone();
        c.clear_title = self.clear_title.clone();
        c.clear_sub = self.clear_sub.clone();
        c.warning_title = self.warning_title.clone();
        c.unlock_text = self.unlock_text.clone();
        c.boss_down_title = self.boss_down_title.clone();
        ctx.world.resources.insert(game);
    }
}
