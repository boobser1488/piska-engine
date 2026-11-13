//! Таблицы игры: виды врагов, боссы и их сценарии атак, улучшения и итоговые
//! характеристики игрока, план волн.

use crate::atlas::{self, Rect};
use crate::rng::Rng;

// ---------------------------------------------------------------------------
// Арена
// ---------------------------------------------------------------------------

/// Радиус арены в мировых единицах.
pub const ARENA_R: f32 = 9.0;
/// Радиус реактора (ядра) в центре.
pub const CORE_R: f32 = 1.15;
/// Радиус дрона игрока.
pub const PLAYER_R: f32 = 0.42;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
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

/// Справочная запись о виде врага.
pub struct KindInfo {
    pub name: &'static str,
    pub hp: f32,
    pub speed: f32,
    pub radius: f32,
    /// Урон при касании дрона / ядра.
    pub contact: f32,
    pub score: u32,
    /// Сколько обломков падает (от, до).
    pub scrap: (u32, u32),
    /// «Стоимость» в бюджете волны.
    pub cost: f32,
    pub sprite: Rect,
    /// Размер спрайта в мировых единицах.
    pub size: f32,
    /// Спрайт поворачивается по курсу движения.
    pub faces_heading: bool,
    /// Цвет эффектов (линейный RGB, может быть ярче 1 для свечения).
    pub color: [f32; 3],
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

    pub fn is_boss(self) -> bool {
        matches!(self, Kind::BossHive | Kind::BossRam | Kind::BossConductor | Kind::BossOvermind)
    }

    /// Считается ли враг при подсчёте «осталось врагов» (мины — нет).
    pub fn counts_for_wave(self) -> bool {
        self != Kind::Mine
    }

    pub fn info(self) -> &'static KindInfo {
        match self {
            Kind::Chaser => &KindInfo {
                name: "Гонщик",
                hp: 30.0,
                speed: 2.7,
                radius: 0.42,
                contact: 10.0,
                score: 10,
                scrap: (1, 1),
                cost: 1.0,
                sprite: atlas::CHASER,
                size: 1.25,
                faces_heading: false,
                color: [2.2, 0.55, 0.45],
            },
            Kind::Swarmling => &KindInfo {
                name: "Рой-мальок",
                hp: 11.0,
                speed: 4.6,
                radius: 0.26,
                contact: 6.0,
                score: 5,
                scrap: (0, 1),
                cost: 0.38,
                sprite: atlas::SWARMLING,
                size: 0.85,
                faces_heading: true,
                color: [1.6, 2.2, 0.5],
            },
            Kind::Shooter => &KindInfo {
                name: "Стрелок",
                hp: 42.0,
                speed: 1.9,
                radius: 0.45,
                contact: 8.0,
                score: 25,
                scrap: (2, 2),
                cost: 2.2,
                sprite: atlas::SHOOTER,
                size: 1.5,
                faces_heading: true,
                color: [1.6, 0.9, 2.4],
            },
            Kind::Tank => &KindInfo {
                name: "Танк",
                hp: 240.0,
                speed: 1.1,
                radius: 0.9,
                contact: 14.0,
                score: 60,
                scrap: (5, 7),
                cost: 5.5,
                sprite: atlas::TANK,
                size: 2.4,
                faces_heading: true,
                color: [0.8, 2.2, 1.3],
            },
            Kind::Splitter => &KindInfo {
                name: "Делитель",
                hp: 75.0,
                speed: 2.0,
                radius: 0.62,
                contact: 10.0,
                score: 20,
                scrap: (1, 2),
                cost: 2.6,
                sprite: atlas::SPLITTER,
                size: 1.7,
                faces_heading: false,
                color: [2.2, 1.8, 0.5],
            },
            Kind::SplitterMini => &KindInfo {
                name: "Осколок",
                hp: 22.0,
                speed: 3.3,
                radius: 0.32,
                contact: 6.0,
                score: 6,
                scrap: (0, 1),
                cost: 0.0,
                sprite: atlas::SPLITTER_MINI,
                size: 1.0,
                faces_heading: false,
                color: [2.2, 1.8, 0.5],
            },
            Kind::Bomber => &KindInfo {
                name: "Подрывник",
                hp: 26.0,
                speed: 3.5,
                radius: 0.42,
                contact: 0.0,
                score: 30,
                scrap: (2, 2),
                cost: 2.2,
                sprite: atlas::BOMBER,
                size: 1.45,
                faces_heading: false,
                color: [2.4, 1.4, 0.5],
            },
            Kind::MineLayer => &KindInfo {
                name: "Минёр",
                hp: 70.0,
                speed: 2.0,
                radius: 0.5,
                contact: 8.0,
                score: 35,
                scrap: (3, 3),
                cost: 4.0,
                sprite: atlas::MINELAYER,
                size: 1.75,
                faces_heading: true,
                color: [0.6, 2.0, 1.8],
            },
            Kind::Mine => &KindInfo {
                name: "Мина",
                hp: 16.0,
                speed: 0.0,
                radius: 0.34,
                contact: 0.0,
                score: 0,
                scrap: (0, 0),
                cost: 0.0,
                sprite: atlas::MINE,
                size: 1.0,
                faces_heading: false,
                color: [2.4, 0.5, 0.7],
            },
            Kind::BossHive => &KindInfo {
                name: "Матка",
                hp: 1700.0,
                speed: 1.1,
                radius: 1.9,
                contact: 18.0,
                score: 1500,
                scrap: (28, 28),
                cost: 0.0,
                sprite: atlas::BOSS_HIVE,
                size: 5.6,
                faces_heading: false,
                color: [2.4, 0.7, 1.5],
            },
            Kind::BossRam => &KindInfo {
                name: "Таран",
                hp: 3600.0,
                speed: 1.9,
                radius: 1.8,
                contact: 30.0,
                score: 3000,
                scrap: (36, 36),
                cost: 0.0,
                sprite: atlas::BOSS_RAM,
                size: 5.2,
                faces_heading: true,
                color: [2.4, 1.6, 0.6],
            },
            Kind::BossConductor => &KindInfo {
                name: "Дирижёр",
                hp: 5800.0,
                speed: 1.4,
                radius: 1.7,
                contact: 22.0,
                score: 4500,
                scrap: (44, 44),
                cost: 0.0,
                sprite: atlas::BOSS_CONDUCTOR,
                size: 5.0,
                faces_heading: false,
                color: [0.9, 1.4, 2.6],
            },
            Kind::BossOvermind => &KindInfo {
                name: "Сверхразум",
                hp: 10500.0,
                speed: 1.5,
                radius: 2.1,
                contact: 30.0,
                score: 9000,
                scrap: (60, 60),
                cost: 0.0,
                sprite: atlas::BOSS_OVERMIND,
                size: 6.2,
                faces_heading: false,
                color: [2.6, 0.6, 0.6],
            },
        }
    }

    /// Босс волны `wave` (`None` — обычная волна). Каждая пятая волна, по кругу.
    pub fn boss_for_wave(wave: u32) -> Option<Kind> {
        if wave == 0 || !wave.is_multiple_of(5) {
            return None;
        }
        Some(match (wave / 5 - 1) % 4 {
            0 => Kind::BossHive,
            1 => Kind::BossRam,
            2 => Kind::BossConductor,
            _ => Kind::BossOvermind,
        })
    }
}

// ---------------------------------------------------------------------------
// Сценарии боссов
// ---------------------------------------------------------------------------

/// Шаг сценария босса. Каждая атака — предупреждение (телеграф), исполнение, пауза.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    match kind {
        Kind::BossHive => &[
            Step::Idle(1.4),
            Step::Ring { n: 16, speed: 5.0, count: 2 },
            Step::Idle(1.2),
            Step::Summon { kind: Kind::Swarmling, count: 8 },
            Step::Idle(1.0),
            Step::Aimed { volleys: 3, per: 3 },
            Step::Idle(1.2),
            Step::Summon { kind: Kind::Chaser, count: 4 },
            Step::Ring { n: 20, speed: 5.5, count: 2 },
        ],
        Kind::BossRam => &[
            Step::Idle(1.2),
            Step::Charge,
            Step::Idle(0.8),
            Step::Charge,
            Step::Idle(0.6),
            Step::Mines { count: 4 },
            Step::Aimed { volleys: 2, per: 3 },
            Step::Idle(1.0),
            Step::Charge,
        ],
        Kind::BossConductor => &[
            Step::Idle(1.0),
            Step::Spiral { secs: 4.0, arms: 3 },
            Step::Idle(0.8),
            Step::Blink,
            Step::Summon { kind: Kind::Shooter, count: 3 },
            Step::Idle(0.8),
            Step::Spiral { secs: 4.0, arms: 2 },
            Step::Blink,
            Step::Ring { n: 20, speed: 5.5, count: 2 },
        ],
        _ => &[
            Step::Idle(1.0),
            Step::Ring { n: 24, speed: 5.5, count: 3 },
            Step::Aimed { volleys: 4, per: 5 },
            Step::Summon { kind: Kind::Bomber, count: 4 },
            Step::Charge,
            Step::Spiral { secs: 5.0, arms: 4 },
            Step::Blink,
            Step::Summon { kind: Kind::Chaser, count: 6 },
            Step::Ring { n: 28, speed: 6.0, count: 3 },
            Step::Idle(0.6),
        ],
    }
}

// ---------------------------------------------------------------------------
// Улучшения
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
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
    /// Имя файла иконки (`ui/icons/<key>.png`) и ключ сохранения.
    pub key: &'static str,
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
        let (key, name, max, weight) = match self {
            Up::Rate => ("rate", "Скорострельность", 6, 1.2),
            Up::Damage => ("damage", "Мощность залпа", 8, 1.2),
            Up::Multishot => ("multishot", "Мультизалп", 4, 1.0),
            Up::Pierce => ("pierce", "Бронебойные", 4, 0.9),
            Up::Ricochet => ("ricochet", "Рикошет", 3, 0.8),
            Up::Explosive => ("explosive", "Разрывные заряды", 4, 0.9),
            Up::Shield => ("shield", "Орбитальные щиты", 5, 0.9),
            Up::Missile => ("missile", "Самонаводящиеся ракеты", 5, 0.9),
            Up::DashDmg => ("dashdmg", "Таранный рывок", 4, 0.8),
            Up::Magnet => ("magnet", "Магнит обломков", 4, 0.9),
            Up::Hull => ("hull", "Усиленный корпус", 5, 1.0),
            Up::Repair => ("repair", "Ремонт реактора", 6, 1.0),
            Up::Slow => ("slow", "Замедляющее поле", 4, 0.8),
            Up::Crit => ("crit", "Критический удар", 5, 0.9),
            Up::Thrusters => ("thrusters", "Форсаж двигателей", 4, 0.8),
            Up::DashCd => ("dashcd", "Ускоритель рывка", 3, 0.7),
            Up::Armor => ("armor", "Броня реактора", 4, 0.9),
            Up::Siphon => ("siphon", "Нано-ремонтники", 4, 0.7),
        };
        UpInfo { key, name, max, weight }
    }

    /// Описание эффекта следующего уровня (`level` — уровень ПОСЛЕ взятия).
    pub fn describe(self, level: u8) -> String {
        let l = level as f32;
        match self {
            Up::Rate => format!("Темп стрельбы +22% (всего +{:.0}%)", 22.0 * l),
            Up::Damage => format!("Урон снарядов +30% (всего +{:.0}%)", 30.0 * l),
            Up::Multishot => format!("Выстрел делится на {} снарядов веером", level as u32 + 1),
            Up::Pierce => format!("Снаряд проходит насквозь, целей: {}", level),
            Up::Ricochet => format!("Отскоков снаряда от стены: {}", level),
            Up::Explosive => format!("Взрыв при попадании, радиус {:.1}", 0.9 + 0.3 * l),
            Up::Shield => format!("Щитов-шаров вокруг дрона: {}", level as u32 + 1),
            Up::Missile => format!(
                "Ракеты каждые {:.1} с: по {} за залп",
                (4.2 - 0.5 * l).max(1.6),
                1 + level as u32 / 2
            ),
            Up::DashDmg => format!("Рывок наносит {:.0} урона всем на пути", 45.0 * l),
            Up::Magnet => format!("Радиус притяжения обломков {:.1}", 2.6 + 1.1 * l),
            Up::Hull => format!("Прочность дрона +25 (всего {})", 100 + 25 * level as u32),
            Up::Repair => format!("Реактор +30 прочности сразу и +{:.2}/с ремонта", 0.35 * l),
            Up::Slow => format!("Враги рядом замедлены на {:.0}%", (20.0 + 10.0 * l).min(70.0)),
            Up::Crit => format!("Шанс крита {:.0}% (урон ×2)", (7.0 * l).min(50.0)),
            Up::Thrusters => format!("Скорость дрона +10% (всего +{:.0}%)", 10.0 * l),
            Up::DashCd => format!("Перезарядка рывка {:.1} с", 2.4 * 0.85f32.powi(level as i32)),
            Up::Armor => format!("Прочность реактора +40, урон по нему −8% (×{:.2})", 0.92f32.powi(level as i32)),
            Up::Siphon => format!("Шанс {:.1}% выбить ремкомплект из врага", 100.0 * (0.015 + 0.03 * l)),
        }
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
        let l = |u: Up| levels[u.index()] as f32;
        let li = |u: Up| levels[u.index()] as u32;
        let slow_l = l(Up::Slow);
        Stats {
            fire_interval: 0.26 / (1.0 + 0.22 * l(Up::Rate)),
            damage: 10.0 * (1.0 + 0.3 * l(Up::Damage)),
            shots: 1 + li(Up::Multishot),
            pierce: li(Up::Pierce),
            bounces: li(Up::Ricochet),
            explode_radius: if li(Up::Explosive) > 0 { 0.9 + 0.3 * l(Up::Explosive) } else { 0.0 },
            shield_count: if li(Up::Shield) > 0 { 1 + li(Up::Shield) } else { 0 },
            shield_damage: 14.0 + 4.0 * l(Up::Shield),
            missile_interval: if li(Up::Missile) > 0 { (4.2 - 0.5 * l(Up::Missile)).max(1.6) } else { f32::INFINITY },
            missile_count: if li(Up::Missile) > 0 { 1 + li(Up::Missile) / 2 } else { 0 },
            dash_damage: 45.0 * l(Up::DashDmg),
            magnet_radius: 2.6 + 1.1 * l(Up::Magnet),
            max_hp: 100.0 + 25.0 * l(Up::Hull),
            core_regen: 0.35 * l(Up::Repair),
            slow_strength: if slow_l > 0.0 { (0.2 + 0.1 * slow_l).min(0.7) } else { 0.0 },
            slow_radius: if slow_l > 0.0 { 3.0 + 0.4 * slow_l } else { 0.0 },
            crit_chance: (0.07 * l(Up::Crit)).min(0.5),
            move_speed: 6.4 * (1.0 + 0.1 * l(Up::Thrusters)),
            dash_cooldown: 2.4 * 0.85f32.powi(levels[Up::DashCd.index()] as i32),
            core_max: 200.0 + 40.0 * l(Up::Armor),
            core_taken: 0.92f32.powi(levels[Up::Armor.index()] as i32),
            repair_drop: 0.015 + 0.03 * l(Up::Siphon),
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
    12.0 + 5.0 * level as f32 + 0.6 * (level * level) as f32
}

// ---------------------------------------------------------------------------
// Волны
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    /// Каждый враг в своей случайной точке края.
    Scatter,
    /// Пачка из одной точки края.
    Pack,
    /// Равномерно по всему краю.
    Ring,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Сколько врагов (без боссов и их призывов) создаёт план.
    pub fn total(&self) -> u32 {
        self.groups.iter().map(|g| g.count).sum::<u32>() + u32::from(self.boss.is_some())
    }
}

/// С какой волны вид появляется в обычных волнах.
fn unlock_wave(kind: Kind) -> u32 {
    match kind {
        Kind::Chaser => 1,
        Kind::Swarmling => 2,
        Kind::Shooter => 3,
        Kind::Tank => 4,
        Kind::Splitter => 6,
        Kind::Bomber => 7,
        Kind::MineLayer => 9,
        _ => 99,
    }
}

const REGULAR: [Kind; 7] =
    [Kind::Chaser, Kind::Swarmling, Kind::Shooter, Kind::Tank, Kind::Splitter, Kind::Bomber, Kind::MineLayer];

/// Множитель здоровья врагов в волне.
pub fn hp_scale(wave: u32) -> f32 {
    let w = wave.max(1) as f32;
    1.0 + 0.07 * (w - 1.0) + if wave > CAMPAIGN_WAVES { 0.12 * (w - CAMPAIGN_WAVES as f32) } else { 0.0 }
}

/// Множитель скорости врагов в волне.
pub fn speed_scale(wave: u32) -> f32 {
    (1.0 + 0.012 * (wave.max(1) as f32 - 1.0)).min(1.4)
}

/// Длительность спавна волны (секунды): враги появляются в течение этого времени.
pub fn wave_duration(wave: u32) -> f32 {
    (16.0 + 1.1 * wave as f32).min(44.0)
}

/// План волны: детерминирован (зависит только от номера волны).
pub fn plan_wave(wave: u32) -> WavePlan {
    let w = wave.max(1);
    let mut rng = Rng::new(0xA11CE ^ (w as u64).wrapping_mul(7919));
    let boss = Kind::boss_for_wave(w);
    let mut budget = 6.0 + 3.2 * w as f32 + 0.28 * (w * w) as f32;
    if boss.is_some() {
        budget *= 0.4;
    }
    let duration = wave_duration(w);
    let mut groups = Vec::new();
    let available: Vec<Kind> = REGULAR.iter().copied().filter(|k| unlock_wave(*k) <= w).collect();
    let weight = |k: Kind| -> f32 {
        match k {
            Kind::Chaser => 5.0,
            Kind::Swarmling => 3.0,
            Kind::Shooter => 2.2,
            Kind::Tank => 1.0,
            Kind::Splitter => 1.6,
            Kind::Bomber => 1.6,
            Kind::MineLayer => 0.6,
            _ => 0.0,
        }
    };
    // Первая волна — только гонщики, чтобы игрок освоился.
    let mut guard = 0;
    while budget > 0.9 && guard < 200 {
        guard += 1;
        let weights: Vec<f32> = available.iter().map(|k| if k.info().cost <= budget { weight(*k) } else { 0.0 }).collect();
        if weights.iter().all(|x| *x == 0.0) {
            break;
        }
        let kind = available[rng.weighted(&weights)];
        let count = match kind {
            Kind::Chaser => rng.int(3, 6),
            Kind::Swarmling => rng.int(6, 12),
            Kind::Shooter => rng.int(1, 3),
            Kind::Tank => rng.int(1, 2),
            Kind::Splitter => rng.int(1, 3),
            Kind::Bomber => rng.int(2, 4),
            _ => 1,
        } as u32;
        let count = count.min(((budget / kind.info().cost).floor() as u32).max(1));
        budget -= kind.info().cost * count as f32;
        let pattern = match kind {
            Kind::Swarmling => Pattern::Pack,
            Kind::Chaser if rng.chance(0.3) => Pattern::Ring,
            Kind::Bomber => Pattern::Scatter,
            _ => Pattern::Scatter,
        };
        groups.push(SpawnGroup { at: 0.0, kind, count, pattern });
    }
    // Разложить группы по времени: равномерно по длительности с небольшим дрожанием.
    let n = groups.len().max(1) as f32;
    let start = if boss.is_some() { 3.5 } else { 0.5 };
    for (i, g) in groups.iter_mut().enumerate() {
        let base = start + (duration - start) * (i as f32 / n);
        g.at = base + rng.range(0.0, (duration / n * 0.6).min(3.0));
    }
    groups.sort_by(|a, b| a.at.total_cmp(&b.at));
    WavePlan { wave: w, groups, boss }
}
