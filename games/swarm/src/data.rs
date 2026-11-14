//! Перечисления игры (виды врагов, шаги боссов, улучшения) и расчёты по таблицам из
//! `assets/data/*.ron` (см. [`crate::defs`]): характеристики игрока, генератор волн.
//!
//! Числа здесь не живут: враги, боссы, улучшения и баланс лежат в файлах данных.

use serde::{Deserialize, Serialize};

use crate::defs::{defs, EnemyDef};
use crate::rng::Rng;

// ---------------------------------------------------------------------------
// Арена
// ---------------------------------------------------------------------------

/// Радиус арены в мировых единицах.
pub fn arena_r() -> f32 {
    defs().balance.arena_radius
}
/// Радиус реактора (ядра) в центре.
pub fn core_r() -> f32 {
    defs().balance.core_radius
}
/// Радиус дрона игрока.
pub fn player_r() -> f32 {
    defs().balance.player_radius
}
/// Сколько волн в кампании.
pub const CAMPAIGN_WAVES: u32 = 20;
/// После какой волны открывается бесконечный режим.
pub const ENDLESS_UNLOCK_WAVE: u32 = 10;
/// Размеры пулов: больше живых врагов / пуль на арене не бывает.
pub const ENEMY_CAP: usize = 460;
pub const PBULLET_CAP: usize = 360;
pub const EBULLET_CAP: usize = 420;
pub const PICKUP_CAP: usize = 320;

// ---------------------------------------------------------------------------
// Враги
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum Kind {
    Chaser,
    Swarmling,
    Shooter,
    Tank,
    Splitter,
    SplitterMini,
    Bomber,
    MineLayer,
    Mine,
    BossHive,
    BossRam,
    BossConductor,
    BossOvermind,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Chaser,
        Kind::Swarmling,
        Kind::Shooter,
        Kind::Tank,
        Kind::Splitter,
        Kind::SplitterMini,
        Kind::Bomber,
        Kind::MineLayer,
        Kind::Mine,
        Kind::BossHive,
        Kind::BossRam,
        Kind::BossConductor,
        Kind::BossOvermind,
    ];

    /// Боссы по порядку появления.
    pub const BOSSES: [Kind; 4] = [Kind::BossHive, Kind::BossRam, Kind::BossConductor, Kind::BossOvermind];

    pub fn is_boss(self) -> bool {
        matches!(self, Kind::BossHive | Kind::BossRam | Kind::BossConductor | Kind::BossOvermind)
    }

    /// Считается ли враг при подсчёте «осталось врагов» (мины — нет).
    pub fn counts_for_wave(self) -> bool {
        self != Kind::Mine
    }

    /// Запись о виде из `data/enemies.ron`.
    pub fn info(self) -> &'static EnemyDef {
        defs().enemy(self)
    }

    /// Босс волны `wave` (`None` — обычная волна). Каждая пятая волна, по кругу.
    pub fn boss_for_wave(wave: u32) -> Option<Kind> {
        let every = defs().balance.generator.boss_every.max(1);
        if wave == 0 || !wave.is_multiple_of(every) {
            return None;
        }
        Some(Kind::BOSSES[((wave / every - 1) % 4) as usize])
    }
}

// ---------------------------------------------------------------------------
// Сценарии боссов
// ---------------------------------------------------------------------------

/// Шаг сценария босса. Каждая атака — предупреждение (телеграф), исполнение, пауза.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Step {
    /// Пауза: босс просто двигается.
    Idle(f32),
    /// `count` колец по `n` пуль, скорость пуль `speed`.
    Ring { n: u8, speed: f32, count: u8 },
    /// `volleys` залпов по `per` пуль в сторону игрока.
    Aimed { volleys: u8, per: u8 },
    /// Призвать `count` врагов вида `kind`.
    Summon { kind: Kind, count: u8 },
    /// Рывок на игрока: линия-предупреждение, разгон, удар о стену (оглушение + кольцо).
    Charge,
    /// Вращающаяся спираль из `arms` рукавов `secs` секунд.
    Spiral { secs: f32, arms: u8 },
    /// Телепорт в случайную точку после предупреждения.
    Blink,
    /// Выложить `count` мин.
    Mines { count: u8 },
}

pub fn boss_script(kind: Kind) -> &'static [Step] {
    &kind.info().script
}

// ---------------------------------------------------------------------------
// Улучшения
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum Up {
    Rate,
    Damage,
    Multishot,
    Pierce,
    Ricochet,
    Explosive,
    Shield,
    Missile,
    DashDmg,
    Magnet,
    Hull,
    Repair,
    Slow,
    Crit,
    Thrusters,
    DashCd,
    Armor,
    Siphon,
}

pub const UP_COUNT: usize = 18;

pub struct UpInfo {
    /// Путь иконки в ассетах.
    pub icon: &'static str,
    pub name: &'static str,
    pub max: u8,
    pub weight: f32,
}

impl Up {
    pub const ALL: [Up; UP_COUNT] = [
        Up::Rate,
        Up::Damage,
        Up::Multishot,
        Up::Pierce,
        Up::Ricochet,
        Up::Explosive,
        Up::Shield,
        Up::Missile,
        Up::DashDmg,
        Up::Magnet,
        Up::Hull,
        Up::Repair,
        Up::Slow,
        Up::Crit,
        Up::Thrusters,
        Up::DashCd,
        Up::Armor,
        Up::Siphon,
    ];

    pub fn index(self) -> usize {
        Up::ALL.iter().position(|u| *u == self).unwrap_or(0)
    }

    pub fn info(self) -> UpInfo {
        let d = defs().upgrade(self);
        UpInfo { icon: &d.icon, name: &d.name, max: d.max, weight: d.weight }
    }

    /// Описание эффекта следующего уровня (`level` — уровень ПОСЛЕ взятия): шаблон из
    /// `data/upgrades.ron`, числа в нём считаются по `params`.
    pub fn describe(self, level: u8) -> String {
        let d = defs().upgrade(self);
        let l = level as f32;
        let p = |i: usize| d.params.get(i).copied().unwrap_or(0.0);
        let b = &defs().balance;
        let vals: Vec<String> = match self {
            Up::Rate => vec![format!("{:.0}", 100.0 * p(0)), format!("{:.0}", 100.0 * p(0) * l)],
            Up::Damage => vec![format!("{:.0}", 100.0 * p(0)), format!("{:.0}", 100.0 * p(0) * l)],
            Up::Multishot => {
                let n = level as u32 + 1;
                vec![n.to_string(), plural(n, "снаряд", "снаряда", "снарядов").to_string()]
            }
            Up::Pierce | Up::Ricochet | Up::Shield => {
                vec![(level as u32 + u32::from(self == Up::Shield)).to_string()]
            }
            Up::Explosive => vec![format!("{:.1}", p(0) + p(1) * l)],
            Up::Missile => vec![format!("{:.1}", (p(0) - p(1) * l).max(p(2))), (1 + level as u32 / 2).to_string()],
            Up::DashDmg => vec![format!("{:.0}", p(0) * l)],
            Up::Magnet => vec![format!("{:.1}", b.magnet_radius + p(0) * l)],
            Up::Hull => vec![format!("{:.0}", b.player_hp + p(0) * l)],
            Up::Repair => vec![format!("{:.2}", p(0) * l)],
            Up::Slow => vec![format!("{:.0}", 100.0 * (p(0) + p(1) * l).min(p(2)))],
            Up::Crit => vec![format!("{:.0}", 100.0 * (p(0) * l).min(p(1)))],
            Up::Thrusters => vec![format!("{:.0}", 100.0 * p(0) * l)],
            Up::DashCd => vec![format!("{:.1}", b.dash_cooldown * p(0).powi(level as i32))],
            Up::Armor => vec![format!("{:.2}", p(1).powi(level as i32))],
            Up::Siphon => vec![format!("{:.1}", 100.0 * (b.repair_drop + p(0) * l))],
        };
        let mut text = d.desc.clone();
        for (i, v) in vals.iter().enumerate() {
            text = text.replace(&format!("{{{i}}}"), v);
        }
        text
    }
}

/// Склонение существительного после числительного: 1 снаряд, 2–4 снаряда, 5+ снарядов.
pub fn plural(n: u32, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
    let (t, h) = (n % 10, n % 100);
    if t == 1 && h != 11 {
        one
    } else if (2..=4).contains(&t) && !(12..=14).contains(&h) {
        few
    } else {
        many
    }
}

/// Итоговые характеристики игрока, считаются из уровней улучшений.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub fire_interval: f32,
    pub damage: f32,
    pub shots: u32,
    pub pierce: u32,
    pub bounces: u32,
    pub explode_radius: f32,
    pub shield_count: u32,
    pub shield_damage: f32,
    pub missile_interval: f32,
    pub missile_count: u32,
    pub dash_damage: f32,
    pub magnet_radius: f32,
    pub max_hp: f32,
    pub core_regen: f32,
    pub slow_strength: f32,
    pub slow_radius: f32,
    pub crit_chance: f32,
    pub move_speed: f32,
    pub dash_cooldown: f32,
    pub core_max: f32,
    pub core_taken: f32,
    pub repair_drop: f32,
}

impl Stats {
    pub fn from_levels(levels: &[u8; UP_COUNT]) -> Stats {
        let df = defs();
        let b = &df.balance;
        let lv = |u: Up| levels[u.index()];
        let l = |u: Up| levels[u.index()] as f32;
        let p = |u: Up, i: usize| df.upgrade(u).params.get(i).copied().unwrap_or(0.0);
        let slow_l = l(Up::Slow);
        Stats {
            fire_interval: b.fire_interval / (1.0 + p(Up::Rate, 0) * l(Up::Rate)),
            damage: b.damage * (1.0 + p(Up::Damage, 0) * l(Up::Damage)),
            shots: 1 + lv(Up::Multishot) as u32,
            pierce: lv(Up::Pierce) as u32,
            bounces: lv(Up::Ricochet) as u32,
            explode_radius: if lv(Up::Explosive) > 0 { p(Up::Explosive, 0) + p(Up::Explosive, 1) * l(Up::Explosive) } else { 0.0 },
            shield_count: if lv(Up::Shield) > 0 { 1 + lv(Up::Shield) as u32 } else { 0 },
            shield_damage: p(Up::Shield, 0) + p(Up::Shield, 1) * l(Up::Shield),
            missile_interval: if lv(Up::Missile) > 0 {
                (p(Up::Missile, 0) - p(Up::Missile, 1) * l(Up::Missile)).max(p(Up::Missile, 2))
            } else {
                f32::INFINITY
            },
            missile_count: if lv(Up::Missile) > 0 { 1 + lv(Up::Missile) as u32 / 2 } else { 0 },
            dash_damage: p(Up::DashDmg, 0) * l(Up::DashDmg),
            magnet_radius: b.magnet_radius + p(Up::Magnet, 0) * l(Up::Magnet),
            max_hp: b.player_hp + p(Up::Hull, 0) * l(Up::Hull),
            core_regen: p(Up::Repair, 0) * l(Up::Repair),
            slow_strength: if slow_l > 0.0 { (p(Up::Slow, 0) + p(Up::Slow, 1) * slow_l).min(p(Up::Slow, 2)) } else { 0.0 },
            slow_radius: if slow_l > 0.0 { p(Up::Slow, 3) + p(Up::Slow, 4) * slow_l } else { 0.0 },
            crit_chance: (p(Up::Crit, 0) * l(Up::Crit)).min(p(Up::Crit, 1)),
            move_speed: b.move_speed * (1.0 + p(Up::Thrusters, 0) * l(Up::Thrusters)),
            dash_cooldown: b.dash_cooldown * p(Up::DashCd, 0).powi(lv(Up::DashCd) as i32),
            core_max: b.core_hp + p(Up::Armor, 0) * l(Up::Armor),
            core_taken: p(Up::Armor, 1).powi(lv(Up::Armor) as i32),
            repair_drop: b.repair_drop + p(Up::Siphon, 0) * l(Up::Siphon),
        }
    }

    /// Главная характеристика улучшения (для тестов: она должна меняться) и
    /// «лучше ли, когда больше».
    pub fn primary(&self, up: Up) -> (f32, bool) {
        match up {
            Up::Rate => (self.fire_interval, false),
            Up::Damage => (self.damage, true),
            Up::Multishot => (self.shots as f32, true),
            Up::Pierce => (self.pierce as f32, true),
            Up::Ricochet => (self.bounces as f32, true),
            Up::Explosive => (self.explode_radius, true),
            Up::Shield => (self.shield_count as f32, true),
            Up::Missile => (self.missile_count as f32 * 100.0 - self.missile_interval.min(100.0), true),
            Up::DashDmg => (self.dash_damage, true),
            Up::Magnet => (self.magnet_radius, true),
            Up::Hull => (self.max_hp, true),
            Up::Repair => (self.core_regen, true),
            Up::Slow => (self.slow_strength, true),
            Up::Crit => (self.crit_chance, true),
            Up::Thrusters => (self.move_speed, true),
            Up::DashCd => (self.dash_cooldown, false),
            Up::Armor => (self.core_max, true),
            Up::Siphon => (self.repair_drop, true),
        }
    }
}

/// Сколько обломков нужно для уровня `level` (с 1).
pub fn xp_needed(level: u32) -> f32 {
    let b = &defs().balance;
    b.xp_base + b.xp_per_level * level as f32 + b.xp_per_level_sq * (level * level) as f32
}

// ---------------------------------------------------------------------------
// Волны
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Pattern {
    /// Каждый враг в своей случайной точке края.
    Scatter,
    /// Пачка из одной точки края.
    Pack,
    /// Равномерно по всему краю.
    Ring,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnGroup {
    /// Секунды от начала волны.
    pub at: f32,
    pub kind: Kind,
    pub count: u32,
    pub pattern: Pattern,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WavePlan {
    pub wave: u32,
    pub groups: Vec<SpawnGroup>,
    pub boss: Option<Kind>,
}

impl WavePlan {
    /// Сколько врагов (без призывов боссов) создаёт план.
    pub fn total(&self) -> u32 {
        self.groups.iter().map(|g| g.count).sum::<u32>() + u32::from(self.boss.is_some())
    }
}

/// Множитель здоровья врагов в волне.
pub fn hp_scale(wave: u32) -> f32 {
    let g = &defs().balance.generator;
    let w = wave.max(1) as f32;
    1.0 + g.hp_per_wave * (w - 1.0) + if wave > CAMPAIGN_WAVES { g.hp_endless_extra * (w - CAMPAIGN_WAVES as f32) } else { 0.0 }
}

/// Множитель скорости врагов в волне.
pub fn speed_scale(wave: u32) -> f32 {
    let g = &defs().balance.generator;
    (1.0 + g.speed_per_wave * (wave.max(1) as f32 - 1.0)).min(g.speed_cap)
}

/// Длительность спавна волны (секунды): враги появляются в течение этого времени.
pub fn wave_duration(wave: u32) -> f32 {
    let g = &defs().balance.generator;
    (g.duration_base + g.duration_per_wave * wave as f32).min(g.duration_max)
}

/// План волны: из таблицы `data/waves.ron`, а за её пределами (бесконечный режим) —
/// из генератора. Детерминирован (зависит только от номера волны).
pub fn plan_wave(wave: u32) -> WavePlan {
    let w = wave.max(1);
    if let Some(def) = defs().waves.waves.get(w as usize - 1) {
        if !def.groups.is_empty() || def.boss.is_some() {
            return WavePlan { wave: w, groups: def.groups.clone(), boss: def.boss };
        }
    }
    generate_wave(w)
}

/// Генератор волн по бюджету: из него запекается таблица, он же ведёт бесконечный режим.
pub fn generate_wave(wave: u32) -> WavePlan {
    let df = defs();
    let g = &df.balance.generator;
    let w = wave.max(1);
    let mut rng = Rng::new(0xA11CE ^ (w as u64).wrapping_mul(7919));
    let boss = Kind::boss_for_wave(w);
    let mut budget = g.budget_base + g.budget_per_wave * w as f32 + g.budget_per_wave_sq * (w * w) as f32;
    if boss.is_some() {
        budget *= g.boss_budget_mult;
    }
    let duration = wave_duration(w);
    let mut groups = Vec::new();
    let available: Vec<&EnemyDef> = df.enemies.iter().filter(|e| e.unlock_wave <= w && e.weight > 0.0).collect();
    let mut guard = 0;
    while budget > 0.9 && guard < 200 {
        guard += 1;
        let weights: Vec<f32> = available.iter().map(|e| if e.cost <= budget { e.weight } else { 0.0 }).collect();
        if weights.iter().all(|x| *x == 0.0) {
            break;
        }
        let def = available[rng.weighted(&weights)];
        let count = rng.int(def.group.0, def.group.1).max(1) as u32;
        let count = count.min(((budget / def.cost).floor() as u32).max(1));
        budget -= def.cost * count as f32;
        let pattern = match def.id {
            Kind::Swarmling => Pattern::Pack,
            Kind::Chaser if rng.chance(0.3) => Pattern::Ring,
            _ => Pattern::Scatter,
        };
        groups.push(SpawnGroup { at: 0.0, kind: def.id, count, pattern });
    }
    // Разложить группы по времени: равномерно по длительности с небольшим дрожанием.
    let n = groups.len().max(1) as f32;
    let start = if boss.is_some() { 3.5 } else { 0.5 };
    for (i, group) in groups.iter_mut().enumerate() {
        let base = start + (duration - start) * (i as f32 / n);
        group.at = base + rng.range(0.0, (duration / n * 0.6).min(3.0));
    }
    groups.sort_by(|a, b| a.at.total_cmp(&b.at));
    WavePlan { wave: w, groups, boss }
}
