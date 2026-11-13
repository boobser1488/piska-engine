//! Симуляция «Роя»: чистая игровая логика без сцены и рендера.
//!
//! Враги, пули, обломки лежат в обычных `Vec` (пулы фиксированной ёмкости); у них нет ни
//! `GameObject`, ни твёрдых тел. Столкновения «круг против круга» идут через
//! равномерную сетку ([`Grid`]). Спрайты, частицы и звук делает слой отображения
//! ([`crate::view`]) по событиям [`Event`] и состоянию массивов — поэтому всю игру можно
//! гонять в тестах без окна и видеокарты, а бот играет в неё напрямую.

use engine::prelude::Vec2;

use crate::data::*;
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
    dash_hit: Vec<u32>,
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
    fn blank() -> Bullet {
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

    fn has_hit(&self, uid: u32) -> bool {
        self.hit[..self.nhit as usize].contains(&uid)
    }

    fn note_hit(&mut self, uid: u32) {
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

const PLAYER_START: Vec2 = Vec2::new(0.0, -2.8);

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
            offer: None,
            score: 0,
            combo: 0.0,
            combo_t: 0.0,
            run: RunStats::default(),
            plan: WavePlan { wave: 0, groups: Vec::new(), boss: None },
            intermission_t: 2.6,
            cleared_t: 0.0,
            events: Vec::new(),
            grid: Grid::new(ARENA_R + 3.0, 1.5),
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

    // -----------------------------------------------------------------------
    // Подсчёты
    // -----------------------------------------------------------------------

    pub fn mult(&self) -> f32 {
        (1.0 + self.combo * 0.1).min(8.0)
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

    // -----------------------------------------------------------------------
    // Волны
    // -----------------------------------------------------------------------

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

    fn queue_group(&mut self, g: SpawnGroup) {
        let base = self.rng.range(0.0, std::f32::consts::TAU);
        let edge = ARENA_R - 0.7;
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

    fn pick_core_target(&mut self, kind: Kind) -> bool {
        let p = match kind {
            Kind::Tank => 1.0,
            Kind::Chaser if self.wave >= 3 => 0.2,
            Kind::Splitter => 0.3,
            Kind::Bomber => 0.4,
            _ => 0.0,
        };
        p >= 1.0 || (p > 0.0 && self.rng.chance(p))
    }

    fn update_waves(&mut self, dt: f32) {
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
                        let pos = Vec2::new(0.0, ARENA_R - 2.6);
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

    fn begin_intermission(&mut self) {
        self.phase = Phase::Intermission;
        self.intermission_t = 2.6;
    }

    fn finish_wave(&mut self) {
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

    fn ebullets_clear(&mut self) {
        for i in 0..self.ebullets.len() {
            if self.ebullets[i].alive {
                self.ebullets[i].alive = false;
                self.free_eb.push(i);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Улучшения
    // -----------------------------------------------------------------------

    /// Три случайные карточки (без повторов, без исчерпанных).
    pub fn roll_offer(&mut self) -> Vec<Up> {
        let mut pool: Vec<Up> = Up::ALL.iter().copied().filter(|u| self.levels[u.index()] < u.info().max).collect();
        let mut out = Vec::new();
        while out.len() < 3 && !pool.is_empty() {
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

    // -----------------------------------------------------------------------
    // Выделение слотов
    // -----------------------------------------------------------------------

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
            info.hp * (1.0 + 0.04 * (w as f32 - 1.0).min(30.0))
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

    fn remove_enemy(&mut self, i: usize) {
        if self.enemies[i].alive {
            if self.enemies[i].kind == Kind::Mine {
                self.mines_alive = self.mines_alive.saturating_sub(1);
            }
            self.enemies[i].alive = false;
            self.free_e.push(i);
        }
    }

    fn alloc_pbullet(&mut self) -> Option<usize> {
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

    fn add_xp(&mut self, v: f32) {
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

    // -----------------------------------------------------------------------
    // Урон
    // -----------------------------------------------------------------------

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
            self.player.respawn_t = 3.0;
            self.player.dash_t = 0.0;
            self.combo = 0.0;
            self.run.deaths += 1;
            self.events.push(Event::PlayerDied);
            // Потерянный дрон стоит реактору прочности.
            self.hurt_core(25.0);
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
            self.combo_t = 3.0;
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
        if self.player.alive && self.player.pos.distance(pos) < radius + PLAYER_R {
            self.hurt_player(player_dmg);
        }
        if Vec2::ZERO.distance(pos) < radius + CORE_R {
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

    // -----------------------------------------------------------------------
    // Шаг симуляции
    // -----------------------------------------------------------------------

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

    fn update_telegraphs(&mut self, dt: f32) {
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

    fn update_misc(&mut self, dt: f32) {
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

    fn rebuild_grid(&mut self) {
        self.grid.clear();
        for (i, e) in self.enemies.iter().enumerate() {
            if e.alive {
                self.grid.insert(i as u32, e.pos, e.r);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Игрок
    // -----------------------------------------------------------------------

    fn update_player(&mut self, input: &PlayerInput, dt: f32) {
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
            p.dash_t = 0.17;
            p.dash_cd = stats.dash_cooldown;
            p.invuln = p.invuln.max(0.28);
            p.dash_hit.clear();
            self.events.push(Event::Dash { pos: p.pos, dir });
        }
        if p.dash_t > 0.0 {
            p.dash_t -= dt;
            p.vel = p.dash_dir * 24.0;
        } else {
            let want = input.mv.clamp_length_max(1.0) * stats.move_speed;
            p.vel += (want - p.vel) * (1.0 - (-14.0 * dt).exp());
        }
        p.pos += p.vel * dt;
        // Арена и реактор.
        let r = p.pos.length();
        if r > ARENA_R - 0.5 {
            p.pos *= (ARENA_R - 0.5) / r;
        }
        let min_core = CORE_R + PLAYER_R + 0.05;
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
            self.grid.query(pp, PLAYER_R + 0.25, |id| hits.push(id));
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

    fn shoot_volley(&mut self, pos: Vec2, aim: Vec2, stats: &Stats) {
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

    fn launch_missiles(&mut self, stats: &Stats) {
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

    // -----------------------------------------------------------------------
    // Враги
    // -----------------------------------------------------------------------

    fn target_pos(&self, e: &Enemy) -> Vec2 {
        if e.target_core || !self.player.alive {
            Vec2::ZERO
        } else {
            self.player.pos
        }
    }

    fn update_enemies(&mut self, dt: f32) {
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
                    let target_r = if e.target_core || !player_alive { CORE_R } else { PLAYER_R };
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
                        let trig_p = player_alive && e.pos.distance(player_pos) < e.r + PLAYER_R + 0.55;
                        let trig_c = e.pos.length() < e.r + CORE_R + 0.45;
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
                let limit = ARENA_R - 0.25;
                if r > limit {
                    e.pos *= limit / r;
                }
                let min_r = CORE_R + e.r * 0.9;
                if kind != Kind::Mine && r < min_r {
                    e.pos = if r > 1e-3 { e.pos / r * min_r } else { Vec2::new(0.0, min_r) };
                }
            }
            // Касание реактора.
            {
                let e = &self.enemies[i];
                if kind != Kind::Mine && !kind.is_boss() && e.touch_cd <= 0.0 && e.pos.length() < CORE_R + e.r + 0.12 && info.contact > 0.0 {
                    let dmg = if kind == Kind::Tank { info.contact } else { info.contact * 0.6 };
                    self.enemies[i].touch_cd = 0.9;
                    self.hurt_core(dmg);
                }
            }
            // Касание дрона.
            {
                let e = &self.enemies[i];
                if player_alive && info.contact > 0.0 && self.player.invuln <= 0.0 && e.pos.distance(self.player.pos) < e.r + PLAYER_R {
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

    // -----------------------------------------------------------------------
    // Пули игрока
    // -----------------------------------------------------------------------

    fn update_player_bullets(&mut self, dt: f32) {
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
            if dist + r > ARENA_R {
                if self.pbullets[b].bounces > 0 && !self.pbullets[b].missile {
                    let n = pos / dist.max(1e-3);
                    let bl = &mut self.pbullets[b];
                    bl.vel -= n * (2.0 * bl.vel.dot(n));
                    bl.pos = n * (ARENA_R - r - 0.02);
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

    fn explode_player(&mut self, pos: Vec2, radius: f32, dmg: f32, skip_uid: u32) {
        self.events.push(Event::Explosion { pos, radius, hostile: false });
        self.area_damage(pos, radius, dmg, skip_uid);
    }

    fn steer_missile(&mut self, b: usize, dt: f32) {
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

    // -----------------------------------------------------------------------
    // Пули врагов
    // -----------------------------------------------------------------------

    fn update_enemy_bullets(&mut self, dt: f32) {
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
            if pos.length() + r > ARENA_R + 0.3 {
                kill = true;
                self.events.push(Event::BulletWall { pos, hostile: true });
            } else if self.player.alive && self.player.invuln <= 0.0 && pos.distance(self.player.pos) < r + PLAYER_R {
                self.hurt_player(dmg);
                kill = true;
            } else if pos.length() < r + CORE_R {
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

    // -----------------------------------------------------------------------
    // Обломки
    // -----------------------------------------------------------------------

    fn update_pickups(&mut self, dt: f32) {
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
            if r > ARENA_R - 0.3 {
                pk.pos *= (ARENA_R - 0.3) / r;
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

    // -----------------------------------------------------------------------
    // Нагрузочный режим
    // -----------------------------------------------------------------------

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
            let r = self.rng.range(2.5, ARENA_R - 0.5);
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
