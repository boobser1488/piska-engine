//! Симуляция «Роя»: чистая игровая логика без сцены и рендера.
//!
//! Враги, пули, обломки лежат в обычных `Vec` (пулы фиксированной ёмкости); у них нет ни
//! `GameObject`, ни твёрдых тел. Столкновения «круг против круга» идут через
//! равномерную сетку ([`Grid`]). Спрайты, частицы и звук делает слой отображения
//! ([`crate::view`]) по событиям [`Event`] и состоянию массивов — поэтому всю игру можно
//! гонять в тестах без окна и видеокарты, а бот играет в неё напрямую.

use engine::prelude::Vec2;

use crate::data::*;
use crate::defs::defs;
use crate::grid::Grid;
use crate::rng::Rng;

// ---------------------------------------------------------------------------
// События (для звука, частиц, тряски камеры, чисел урона)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Shot { pos: Vec2, dir: Vec2 },
    EnemyShot { pos: Vec2 },
    EnemyHit { pos: Vec2, dmg: f32, crit: bool, kind: Kind },
    EnemyDied { pos: Vec2, kind: Kind },
    Explosion { pos: Vec2, radius: f32, hostile: bool },
    PlayerHurt { dmg: f32 },
    CoreHurt { dmg: f32 },
    PlayerDied,
    PlayerRespawn,
    ScrapPicked { value: f32 },
    RepairPicked,
    LevelUp,
    UpgradeChosen(Up),
    WaveStart { wave: u32, boss: Option<Kind> },
    WaveCleared { wave: u32 },
    BossAppear { kind: Kind },
    BossTelegraph { pos: Vec2, kind: u8 },
    BossDied { pos: Vec2, kind: Kind },
    BossImpact { pos: Vec2 },
    Dash { pos: Vec2, dir: Vec2 },
    ShieldHit { pos: Vec2 },
    MissileLaunch { pos: Vec2 },
    BulletWall { pos: Vec2, hostile: bool },
    Ricochet { pos: Vec2 },
    MinePlaced { pos: Vec2 },
    MineArmed { pos: Vec2 },
    Victory,
    Defeat,
}

// ---------------------------------------------------------------------------
// Сущности
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Campaign,
    Endless,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Передышка перед волной.
    Intermission,
    /// Волна идёт.
    Wave,
    /// Все враги убиты, ждём выбора улучшения.
    Cleared,
    Won,
    Lost,
}

/// Управление за кадр (клавиатура / мышь / геймпад / касания сводятся к одному виду).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerInput {
    /// Куда ехать, длина ≤ 1.
    pub mv: Vec2,
    /// Единичный вектор прицела (нулевой — оставить прежний).
    pub aim: Vec2,
    pub fire: bool,
    /// Нажатие рывка в этом кадре.
    pub dash: bool,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub pos: Vec2,
    pub vel: Vec2,
    pub aim: Vec2,
    pub hp: f32,
    pub alive: bool,
    pub respawn_t: f32,
    pub fire_cd: f32,
    pub dash_t: f32,
    pub dash_cd: f32,
    pub dash_dir: Vec2,
    pub invuln: f32,
    pub flash: f32,
    pub missile_t: f32,
    pub shield_phase: f32,
    pub muzzle: f32,
    pub(crate) dash_hit: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Core {
    pub hp: f32,
    pub max_hp: f32,
    pub flash: f32,
}

#[derive(Clone, Debug)]
pub struct Enemy {
    pub alive: bool,
    pub uid: u32,
    pub kind: Kind,
    pub pos: Vec2,
    pub vel: Vec2,
    pub knock: Vec2,
    pub hp: f32,
    pub max_hp: f32,
    pub r: f32,
    pub target_core: bool,
    pub age: f32,
    pub flash: f32,
    pub touch_cd: f32,
    pub shield_cd: f32,
    pub timer: f32,
    /// Состояние (у обычных врагов: 1 — «заряжается»; у боссов — см. `boss.rs`).
    pub st: u8,
    pub st_t: f32,
    pub step: u8,
    pub dir: Vec2,
    pub aux: f32,
    pub aux_pos: Vec2,
    /// Поворот спрайта (радианы).
    pub heading: f32,
    /// Босс оглушён и получает на 50% больше урона.
    pub vulnerable: bool,
    /// Оставшееся время предупреждения атаки босса и его вид (для отрисовки).
    pub tele: f32,
    pub tele_kind: u8,
    pub slow: f32,
    pub phase: u8,
}

impl Enemy {
    fn blank() -> Enemy {
        Enemy {
            alive: false,
            uid: 0,
            kind: Kind::Chaser,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            knock: Vec2::ZERO,
            hp: 0.0,
            max_hp: 1.0,
            r: 0.4,
            target_core: false,
            age: 0.0,
            flash: 0.0,
            touch_cd: 0.0,
            shield_cd: 0.0,
            timer: 0.0,
            st: 0,
            st_t: 0.0,
            step: 0,
            dir: Vec2::X,
            aux: 0.0,
            aux_pos: Vec2::ZERO,
            heading: 0.0,
            vulnerable: false,
            tele: 0.0,
            tele_kind: 0,
            slow: 1.0,
            phase: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bullet {
    pub alive: bool,
    /// Номер выстрела: слой отображения по нему понимает, что слот занят новой пулей.
    pub serial: u32,
    pub pos: Vec2,
    pub vel: Vec2,
    pub r: f32,
    pub dmg: f32,
    pub life: f32,
    pub pierce: u32,
    pub bounces: u32,
    pub explode: f32,
    pub crit: bool,
    pub missile: bool,
    pub target: u32,
    hit: [u32; 4],
    nhit: u8,
}

impl Bullet {
    pub(crate) fn blank() -> Bullet {
        Bullet {
            alive: false,
            serial: 0,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            r: 0.14,
            dmg: 0.0,
            life: 0.0,
            pierce: 0,
            bounces: 0,
            explode: 0.0,
            crit: false,
            missile: false,
            target: 0,
            hit: [0; 4],
            nhit: 0,
        }
    }

    pub(crate) fn has_hit(&self, uid: u32) -> bool {
        self.hit[..self.nhit as usize].contains(&uid)
    }

    pub(crate) fn note_hit(&mut self, uid: u32) {
        if (self.nhit as usize) < self.hit.len() {
            self.hit[self.nhit as usize] = uid;
            self.nhit += 1;
        } else {
            self.hit.rotate_left(1);
            self.hit[3] = uid;
        }
    }
}

#[derive(Clone, Debug)]
pub struct EBullet {
    pub alive: bool,
    pub serial: u32,
    pub pos: Vec2,
    pub vel: Vec2,
    pub r: f32,
    pub dmg: f32,
    pub life: f32,
    /// Размер спрайта (мировые единицы).
    pub size: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickupKind {
    Scrap,
    Repair,
}

#[derive(Clone, Debug)]
pub struct Pickup {
    pub alive: bool,
    pub serial: u32,
    pub kind: PickupKind,
    pub pos: Vec2,
    pub vel: Vec2,
    pub value: f32,
    pub age: f32,
}

#[derive(Clone, Debug)]
pub struct Telegraph {
    pub pos: Vec2,
    pub t: f32,
    pub total: f32,
    pub kind: Kind,
    pub target_core: bool,
}

/// Итоги забега для экрана результатов.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStats {
    pub kills: u32,
    pub damage: f32,
    pub shots: u32,
    pub scrap: u32,
    pub best_mult: f32,
    pub deaths: u32,
    pub upgrades: u32,
}

/// Состояние для «Продолжить»: сохраняется после каждой выбранной награды волны.
#[derive(Clone, Debug, PartialEq)]
pub struct Checkpoint {
    pub mode: Mode,
    /// Волна, которая начнётся при продолжении.
    pub next_wave: u32,
    pub levels: [u8; UP_COUNT],
    pub level: u32,
    pub xp: f32,
    pub score: u64,
    pub core_hp: f32,
    pub kills: u32,
    pub time: f32,
}

pub struct Sim {
    pub mode: Mode,
    pub phase: Phase,
    pub wave: u32,
    pub time: f32,
    pub wave_time: f32,
    pub rng: Rng,
    pub player: Player,
    pub core: Core,
    pub enemies: Vec<Enemy>,
    pub pbullets: Vec<Bullet>,
    pub ebullets: Vec<EBullet>,
    pub pickups: Vec<Pickup>,
    pub telegraphs: Vec<Telegraph>,
    pub levels: [u8; UP_COUNT],
    pub stats: Stats,
    pub xp: f32,
    pub level: u32,
    pub xp_need: f32,
    pub pending_upgrades: u32,
    /// Сколько карточек предлагается за раз.
    pub offer_count: usize,
    pub offer: Option<Vec<Up>>,
    pub score: u64,
    pub combo: f32,
    pub combo_t: f32,
    pub run: RunStats,
    pub plan: WavePlan,
    pub intermission_t: f32,
    pub cleared_t: f32,
    pub events: Vec<Event>,
    pub grid: Grid,
    /// Слоты боссов не нужны: босс — обычный враг, его находят по виду.
    pub(crate) free_e: Vec<usize>,
    pub(crate) free_pb: Vec<usize>,
    pub(crate) free_eb: Vec<usize>,
    pub(crate) free_pk: Vec<usize>,
    pub(crate) next_uid: u32,
    pub(crate) next_serial: u32,
    pub(crate) cursor: usize,
    pub(crate) boss_spawned: bool,
    pub(crate) vacuum: bool,
    pub(crate) scratch: Vec<u32>,
    pub(crate) mines_alive: usize,
}

pub(crate) const PLAYER_START: Vec2 = Vec2::new(0.0, -2.8);

impl Sim {
    pub fn new(seed: u64, mode: Mode) -> Sim {
        let levels = [0u8; UP_COUNT];
        let stats = Stats::from_levels(&levels);
        Sim {
            mode,
            phase: Phase::Intermission,
            wave: 0,
            time: 0.0,
            wave_time: 0.0,
            rng: Rng::new(seed),
            player: Player {
                pos: PLAYER_START,
                vel: Vec2::ZERO,
                aim: Vec2::Y,
                hp: stats.max_hp,
                alive: true,
                respawn_t: 0.0,
                fire_cd: 0.0,
                dash_t: 0.0,
                dash_cd: 0.0,
                dash_dir: Vec2::Y,
                invuln: 1.0,
                flash: 0.0,
                missile_t: 2.0,
                shield_phase: 0.0,
                muzzle: 0.0,
                dash_hit: Vec::new(),
            },
            core: Core { hp: stats.core_max, max_hp: stats.core_max, flash: 0.0 },
            enemies: Vec::with_capacity(ENEMY_CAP),
            pbullets: Vec::with_capacity(PBULLET_CAP),
            ebullets: Vec::with_capacity(EBULLET_CAP),
            pickups: Vec::with_capacity(PICKUP_CAP),
            telegraphs: Vec::new(),
            levels,
            stats,
            xp: 0.0,
            level: 1,
            xp_need: xp_needed(1),
            pending_upgrades: 0,
            offer_count: 3,
            offer: None,
            score: 0,
            combo: 0.0,
            combo_t: 0.0,
            run: RunStats::default(),
            plan: WavePlan { wave: 0, groups: Vec::new(), boss: None },
            intermission_t: 2.6,
            cleared_t: 0.0,
            events: Vec::new(),
            grid: Grid::new(arena_r() + 3.0, 1.5),
            free_e: Vec::new(),
            free_pb: Vec::new(),
            free_eb: Vec::new(),
            free_pk: Vec::new(),
            next_uid: 1,
            next_serial: 1,
            cursor: 0,
            boss_spawned: false,
            vacuum: false,
            scratch: Vec::new(),
            mines_alive: 0,
        }
    }

    /// Забег, продолженный с контрольной точки.
    pub fn from_checkpoint(seed: u64, cp: &Checkpoint) -> Sim {
        let mut sim = Sim::new(seed, cp.mode);
        sim.levels = cp.levels;
        sim.stats = Stats::from_levels(&sim.levels);
        sim.level = cp.level.max(1);
        sim.xp = cp.xp;
        sim.xp_need = xp_needed(sim.level);
        sim.score = cp.score;
        sim.core.max_hp = sim.stats.core_max;
        sim.core.hp = cp.core_hp.clamp(1.0, sim.core.max_hp);
        sim.player.hp = sim.stats.max_hp;
        sim.run.kills = cp.kills;
        sim.time = cp.time;
        sim.wave = cp.next_wave.saturating_sub(1);
        sim
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            mode: self.mode,
            next_wave: self.wave + 1,
            levels: self.levels,
            level: self.level,
            xp: self.xp,
            score: self.score,
            core_hp: self.core.hp,
            kills: self.run.kills,
            time: self.time,
        }
    }

    pub fn mult(&self) -> f32 {
        (1.0 + self.combo * defs().balance.combo_step).min(defs().balance.combo_cap)
    }

    pub fn alive_enemies(&self) -> usize {
        self.enemies.iter().filter(|e| e.alive && e.kind.counts_for_wave()).count()
    }

    /// Сколько врагов ещё предстоит победить в волне (живые + ожидающие появления).
    pub fn remaining(&self) -> u32 {
        let mut pending = self.telegraphs.iter().filter(|t| t.kind.counts_for_wave()).count() as u32;
        if self.phase == Phase::Wave {
            pending += self.plan.groups[self.cursor.min(self.plan.groups.len())..].iter().map(|g| g.count).sum::<u32>();
            if self.plan.boss.is_some() && !self.boss_spawned {
                pending += 1;
            }
        }
        self.alive_enemies() as u32 + pending
    }

    pub fn boss(&self) -> Option<&Enemy> {
        self.enemies.iter().find(|e| e.alive && e.kind.is_boss())
    }

    pub fn count_alive(&self, kind: Kind) -> usize {
        self.enemies.iter().filter(|e| e.alive && e.kind == kind).count()
    }

    pub fn pbullets_alive(&self) -> usize {
        self.pbullets.iter().filter(|b| b.alive).count()
    }

    pub fn ebullets_alive(&self) -> usize {
        self.ebullets.iter().filter(|b| b.alive).count()
    }

    /// Нужно ли показать выбор улучшения (есть награда, и пауза после волны прошла).
    pub fn upgrade_ready(&self) -> bool {
        self.pending_upgrades > 0 && !matches!(self.phase, Phase::Won | Phase::Lost) && (self.phase != Phase::Cleared || self.cleared_t >= 1.6)
    }

    pub fn is_over(&self) -> bool {
        matches!(self.phase, Phase::Won | Phase::Lost)
    }

    pub fn spawn_enemy(&mut self, kind: Kind, pos: Vec2) -> Option<usize> {
        self.spawn_enemy_ex(kind, pos, false)
    }

    pub(crate) fn spawn_enemy_ex(&mut self, kind: Kind, pos: Vec2, target_core: bool) -> Option<usize> {
        let slot = if let Some(i) = self.free_e.pop() {
            i
        } else if self.enemies.len() < ENEMY_CAP {
            self.enemies.push(Enemy::blank());
            self.enemies.len() - 1
        } else {
            return None;
        };
        let info = kind.info();
        let w = self.wave.max(1);
        let hp = if kind.is_boss() {
            info.hp * (1.0 + defs().balance.generator.boss_hp_per_wave * (w as f32 - 1.0).min(30.0))
        } else {
            info.hp * hp_scale(w)
        };
        let uid = self.next_uid;
        self.next_uid += 1;
        let mut e = Enemy::blank();
        e.alive = true;
        e.uid = uid;
        e.kind = kind;
        e.pos = pos;
        e.hp = hp;
        e.max_hp = hp;
        e.r = info.radius;
        e.target_core = target_core || kind == Kind::Tank;
        e.timer = match kind {
            Kind::Shooter => 1.0 + self.rng.range(0.0, 1.6),
            Kind::MineLayer => 1.5,
            Kind::Mine => 1.0,
            _ => 0.0,
        };
        e.dir = if kind == Kind::MineLayer && self.rng.chance(0.5) { -Vec2::X } else { Vec2::X };
        e.heading = self.rng.range(0.0, std::f32::consts::TAU);
        if kind.is_boss() {
            e.timer = 0.0;
            e.st_t = 1.2;
        }
        if kind == Kind::Mine {
            self.mines_alive += 1;
        }
        self.enemies[slot] = e;
        Some(slot)
    }

    pub(crate) fn remove_enemy(&mut self, i: usize) {
        if self.enemies[i].alive {
            if self.enemies[i].kind == Kind::Mine {
                self.mines_alive = self.mines_alive.saturating_sub(1);
            }
            self.enemies[i].alive = false;
            self.free_e.push(i);
        }
    }

    pub(crate) fn alloc_pbullet(&mut self) -> Option<usize> {
        let slot = if let Some(i) = self.free_pb.pop() {
            i
        } else if self.pbullets.len() < PBULLET_CAP {
            self.pbullets.push(Bullet::blank());
            self.pbullets.len() - 1
        } else {
            return None;
        };
        Some(slot)
    }

    pub(crate) fn fire_enemy_bullet(&mut self, pos: Vec2, dir: Vec2, speed: f32, dmg: f32, size: f32) {
        let slot = if let Some(i) = self.free_eb.pop() {
            i
        } else if self.ebullets.len() < EBULLET_CAP {
            self.ebullets.push(EBullet {
                alive: false,
                serial: 0,
                pos: Vec2::ZERO,
                vel: Vec2::ZERO,
                r: 0.2,
                dmg: 0.0,
                life: 0.0,
                size: 0.8,
            });
            self.ebullets.len() - 1
        } else {
            return;
        };
        let serial = self.next_serial;
        self.next_serial += 1;
        let scale = 1.0 + 0.02 * (self.wave.min(40) as f32);
        self.ebullets[slot] = EBullet {
            alive: true,
            serial,
            pos,
            vel: dir.normalize_or_zero() * speed,
            r: size * 0.26,
            dmg: dmg * scale,
            life: 7.0,
            size,
        };
    }

    pub fn spawn_pickup(&mut self, pos: Vec2, kind: PickupKind, value: f32) {
        let slot = if let Some(i) = self.free_pk.pop() {
            i
        } else if self.pickups.len() < PICKUP_CAP {
            self.pickups.push(Pickup { alive: false, serial: 0, kind, pos, vel: Vec2::ZERO, value: 0.0, age: 0.0 });
            self.pickups.len() - 1
        } else {
            // Пул полон: обломок сразу засчитывается, ничего не теряется.
            if kind == PickupKind::Scrap {
                self.add_xp(value);
            }
            return;
        };
        let serial = self.next_serial;
        self.next_serial += 1;
        let v = self.rng.unit() * self.rng.range(1.5, 4.5);
        self.pickups[slot] = Pickup { alive: true, serial, kind, pos, vel: v, value, age: 0.0 };
    }

    pub(crate) fn add_xp(&mut self, v: f32) {
        self.xp += v;
        self.run.scrap += v as u32;
        while self.xp >= self.xp_need {
            self.xp -= self.xp_need;
            self.level += 1;
            self.xp_need = xp_needed(self.level);
            self.pending_upgrades += 1;
            self.events.push(Event::LevelUp);
        }
    }

    pub fn hurt_player(&mut self, dmg: f32) {
        if !self.player.alive || self.player.invuln > 0.0 || self.is_over() {
            return;
        }
        self.player.hp -= dmg;
        self.player.invuln = 0.55;
        self.player.flash = 0.18;
        self.events.push(Event::PlayerHurt { dmg });
        if self.player.hp <= 0.0 {
            self.player.hp = 0.0;
            self.player.alive = false;
            self.player.respawn_t = defs().balance.respawn_time;
            self.player.dash_t = 0.0;
            self.combo = 0.0;
            self.run.deaths += 1;
            self.events.push(Event::PlayerDied);
            // Потерянный дрон стоит реактору прочности.
            self.hurt_core(defs().balance.drone_loss_core_damage);
        }
    }

    pub fn hurt_core(&mut self, dmg: f32) {
        if self.is_over() {
            return;
        }
        let dmg = dmg * self.stats.core_taken;
        self.core.hp -= dmg;
        self.core.flash = 0.2;
        self.events.push(Event::CoreHurt { dmg });
        if self.core.hp <= 0.0 {
            self.core.hp = 0.0;
            self.phase = Phase::Lost;
            self.events.push(Event::Defeat);
        }
    }

    pub fn damage_enemy(&mut self, i: usize, dmg: f32, crit: bool) {
        if !self.enemies[i].alive || self.is_over() {
            return;
        }
        let dmg = if self.enemies[i].vulnerable { dmg * 1.5 } else { dmg };
        let e = &mut self.enemies[i];
        e.hp -= dmg;
        e.flash = 0.09;
        let (pos, kind, dead) = (e.pos, e.kind, e.hp <= 0.0);
        self.run.damage += dmg;
        self.events.push(Event::EnemyHit { pos, dmg, crit, kind });
        if dead {
            self.kill_enemy(i, false);
        }
    }

    /// Убить врага: очки, обломки, особые смерти. `chain` — массовая гибель при смерти
    /// босса (без взрывов и деления).
    pub(crate) fn kill_enemy(&mut self, i: usize, chain: bool) {
        if !self.enemies[i].alive {
            return;
        }
        let (kind, pos, uid) = (self.enemies[i].kind, self.enemies[i].pos, self.enemies[i].uid);
        let info = kind.info();
        self.remove_enemy(i);
        self.events.push(Event::EnemyDied { pos, kind });
        if kind != Kind::Mine {
            self.run.kills += 1;
            let pts = (info.score as f32 * self.mult()) as u64;
            self.score += pts;
            self.combo += 1.0;
            self.combo_t = defs().balance.combo_time;
            self.run.best_mult = self.run.best_mult.max(self.mult());
            let n = self.rng.int(info.scrap.0 as i32, info.scrap.1 as i32);
            let big = kind.is_boss();
            for _ in 0..n {
                let p = pos + self.rng.unit() * self.rng.range(0.0, info.radius * 0.8);
                self.spawn_pickup(p, PickupKind::Scrap, if big { 2.0 } else { 1.0 });
            }
            if !kind.is_boss() && self.rng.chance(self.stats.repair_drop) {
                self.spawn_pickup(pos, PickupKind::Repair, 1.0);
            }
        }
        let _ = uid;
        if chain {
            return;
        }
        match kind {
            Kind::Splitter => {
                for s in [-1.0f32, 1.0] {
                    if let Some(j) = self.spawn_enemy_ex(Kind::SplitterMini, pos, false) {
                        let d = Vec2::new(s, 0.4 * s).normalize();
                        self.enemies[j].knock = d * 7.0;
                        self.enemies[j].pos = pos + d * 0.3;
                    }
                }
            }
            Kind::Bomber => {
                self.blast_hostile(pos, 1.9, 15.0, 15.0, true);
            }
            Kind::Mine => {
                self.blast_hostile(pos, 2.0, 22.0, 22.0, false);
            }
            k if k.is_boss() => {
                self.events.push(Event::BossDied { pos, kind: k });
                self.ebullets_clear();
                self.score += 0;
                for j in 0..self.enemies.len() {
                    if self.enemies[j].alive && self.enemies[j].kind != Kind::Mine {
                        self.kill_enemy(j, true);
                    } else if self.enemies[j].alive {
                        self.remove_enemy(j);
                    }
                }
            }
            _ => {}
        }
    }

    /// Вражеский взрыв: бьёт дрона и реактор в радиусе; `hurt_enemies` — и других врагов.
    pub(crate) fn blast_hostile(&mut self, pos: Vec2, radius: f32, player_dmg: f32, core_dmg: f32, hurt_enemies: bool) {
        self.events.push(Event::Explosion { pos, radius, hostile: true });
        if self.player.alive && self.player.pos.distance(pos) < radius + player_r() {
            self.hurt_player(player_dmg);
        }
        if Vec2::ZERO.distance(pos) < radius + core_r() {
            self.hurt_core(core_dmg);
        }
        if hurt_enemies {
            self.area_damage(pos, radius, 30.0 * self.stats.damage / 10.0, u32::MAX);
        }
    }

    /// Урон врагам в радиусе (кроме `skip_uid`).
    pub(crate) fn area_damage(&mut self, pos: Vec2, radius: f32, dmg: f32, skip_uid: u32) {
        let mut hits = std::mem::take(&mut self.scratch);
        hits.clear();
        self.grid.query(pos, radius, |id| hits.push(id));
        for &id in &hits {
            let i = id as usize;
            if self.enemies[i].alive && self.enemies[i].uid != skip_uid && self.enemies[i].pos.distance(pos) <= radius + self.enemies[i].r {
                self.damage_enemy(i, dmg, false);
            }
        }
        self.scratch = hits;
    }

    pub fn step(&mut self, input: &PlayerInput, dt: f32) {
        if self.events.len() > 4096 {
            self.events.clear();
        }
        if self.is_over() || dt <= 0.0 {
            return;
        }
        self.time += dt;
        self.update_waves(dt);
        self.update_telegraphs(dt);
        self.update_player(input, dt);
        self.rebuild_grid();
        self.update_enemies(dt);
        self.update_player_bullets(dt);
        self.update_enemy_bullets(dt);
        self.update_pickups(dt);
        self.update_misc(dt);
    }

    pub(crate) fn update_misc(&mut self, dt: f32) {
        self.combo_t -= dt;
        if self.combo_t <= 0.0 && self.combo > 0.0 {
            self.combo = (self.combo - 7.0 * dt).max(0.0);
        }
        if self.core.hp < self.core.max_hp && self.stats.core_regen > 0.0 && !self.is_over() {
            self.core.hp = (self.core.hp + self.stats.core_regen * dt).min(self.core.max_hp);
        }
        self.core.flash = (self.core.flash - dt).max(0.0);
        self.player.flash = (self.player.flash - dt).max(0.0);
        self.player.muzzle = (self.player.muzzle - dt).max(0.0);
    }

    pub(crate) fn rebuild_grid(&mut self) {
        self.grid.clear();
        for (i, e) in self.enemies.iter().enumerate() {
            if e.alive {
                self.grid.insert(i as u32, e.pos, e.r);
            }
        }
    }

    pub(crate) fn update_pickups(&mut self, dt: f32) {
        let magnet = self.stats.magnet_radius;
        let pp = self.player.pos;
        let alive = self.player.alive;
        for i in 0..self.pickups.len() {
            if !self.pickups[i].alive {
                continue;
            }
            let (pos, kind) = (self.pickups[i].pos, self.pickups[i].kind);
            let pk = &mut self.pickups[i];
            pk.age += dt;
            let d = pp - pos;
            let dist = d.length();
            let pulled = alive && (self.vacuum || (pk.age > 0.25 && dist < magnet));
            if pulled {
                let speed = if self.vacuum { 20.0 } else { 7.0 + (magnet - dist).max(0.0) * 4.0 };
                pk.vel += (d / dist.max(1e-3) * speed - pk.vel) * (1.0 - (-10.0 * dt).exp());
            } else {
                pk.vel *= (-2.6 * dt).exp();
            }
            pk.pos += pk.vel * dt;
            let r = pk.pos.length();
            if r > arena_r() - 0.3 {
                pk.pos *= (arena_r() - 0.3) / r;
            }
            if alive && pk.pos.distance(pp) < 0.65 && pk.age > 0.12 {
                let value = pk.value;
                pk.alive = false;
                self.free_pk.push(i);
                match kind {
                    PickupKind::Scrap => {
                        self.events.push(Event::ScrapPicked { value });
                        self.add_xp(value);
                    }
                    PickupKind::Repair => {
                        if self.player.hp < self.stats.max_hp - 0.5 {
                            self.player.hp = (self.player.hp + 25.0).min(self.stats.max_hp);
                        } else {
                            self.core.hp = (self.core.hp + 12.0).min(self.core.max_hp);
                        }
                        self.events.push(Event::RepairPicked);
                    }
                }
            }
        }
    }

}

/// Повернуть угол `a` к `b` не больше чем на `max` радиан по кратчайшей дуге.
pub fn move_angle(a: f32, b: f32, max: f32) -> f32 {
    let mut d = (b - a) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d.clamp(-max, max)
}
