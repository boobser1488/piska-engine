//! Определения игры из файлов `assets/data/*.ron`: враги и боссы, каталог улучшений,
//! общий баланс, таблица волн.
//!
//! Игра читает их через базу ассетов ([`Defs::load`]), поэтому всё работает и из пака
//! (браузер, Android), и правится в редакторе. Чистые тесты симуляции без приложения
//! берут копию тех же файлов, вшитую при сборке ([`Defs::embedded`]).

use std::sync::RwLock;

use engine::prelude::AssetDatabase;
use serde::{Deserialize, Serialize};

use crate::data::{Kind, Pattern, SpawnGroup, Step, Up, UP_COUNT};

/// Враг или босс: характеристики и правила появления. Картинка — в префабе `prefab`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnemyDef {
    pub id: Kind,
    pub name: String,
    pub prefab: String,
    pub hp: f32,
    pub speed: f32,
    pub radius: f32,
    /// Урон при касании дрона / реактора.
    pub contact: f32,
    pub score: u32,
    /// Сколько обломков падает (от, до).
    pub scrap: (u32, u32),
    /// «Стоимость» в бюджете волны.
    pub cost: f32,
    pub faces_heading: bool,
    /// Цвет эффектов (линейный RGB, может быть ярче 1 для свечения).
    pub color: (f32, f32, f32),
    /// С какой волны вид попадает в сгенерированные волны (99 — никогда).
    pub unlock_wave: u32,
    pub weight: f32,
    /// Размер пачки (от, до).
    pub group: (i32, i32),
    /// Шанс, что враг идёт к реактору, а не к дрону.
    pub core_chance: f32,
    pub core_chance_from_wave: u32,
    /// Сценарий атак (только боссы).
    #[serde(default)]
    pub script: Vec<Step>,
}

impl EnemyDef {
    pub fn rgb(&self) -> [f32; 3] {
        [self.color.0, self.color.1, self.color.2]
    }
}

/// Улучшение: название, иконка, предел уровней, вес в выпадении, числа, описание.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpDef {
    pub id: Up,
    pub name: String,
    pub icon: String,
    pub max: u8,
    pub weight: f32,
    #[serde(default)]
    pub params: Vec<f32>,
    pub desc: String,
}

/// Параметры генератора волн (после 20-й волны и для запекания таблицы).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generator {
    pub budget_base: f32,
    pub budget_per_wave: f32,
    pub budget_per_wave_sq: f32,
    pub boss_budget_mult: f32,
    pub duration_base: f32,
    pub duration_per_wave: f32,
    pub duration_max: f32,
    pub hp_per_wave: f32,
    pub hp_endless_extra: f32,
    pub speed_per_wave: f32,
    pub speed_cap: f32,
    pub boss_hp_per_wave: f32,
    pub boss_every: u32,
}

/// Общие числа: арена, дрон, реактор, опыт, комбо.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Balance {
    pub arena_radius: f32,
    pub core_radius: f32,
    pub player_radius: f32,
    pub player_hp: f32,
    pub core_hp: f32,
    pub fire_interval: f32,
    pub damage: f32,
    pub move_speed: f32,
    pub dash_cooldown: f32,
    pub dash_speed: f32,
    pub dash_time: f32,
    pub magnet_radius: f32,
    pub repair_drop: f32,
    pub drone_loss_core_damage: f32,
    pub respawn_time: f32,
    pub xp_base: f32,
    pub xp_per_level: f32,
    pub xp_per_level_sq: f32,
    pub combo_time: f32,
    pub combo_step: f32,
    pub combo_cap: f32,
    pub generator: Generator,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WaveDef {
    pub groups: Vec<SpawnGroup>,
    pub boss: Option<Kind>,
}

/// Таблица волн: `waves[0]` — первая волна. Дальше работает генератор.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WaveTable {
    pub waves: Vec<WaveDef>,
}

pub struct Defs {
    pub balance: Balance,
    pub enemies: Vec<EnemyDef>,
    pub upgrades: Vec<UpDef>,
    pub waves: WaveTable,
}

const ENEMIES: &str = include_str!("../assets/data/enemies.ron");
const UPGRADES: &str = include_str!("../assets/data/upgrades.ron");
const BALANCE: &str = include_str!("../assets/data/balance.ron");
const WAVES: &str = include_str!("../assets/data/waves.ron");

fn parse<T: for<'de> Deserialize<'de>>(text: &str, what: &str) -> Result<T, String> {
    ron::from_str(text).map_err(|e| format!("cannot read `{what}`: {e}. Fix the RON file in assets/data"))
}

impl Defs {
    pub fn parse(enemies: &str, upgrades: &str, balance: &str, waves: &str) -> Result<Defs, String> {
        let enemies: Vec<EnemyDef> = parse(enemies, "data/enemies.ron")?;
        let upgrades: Vec<UpDef> = parse(upgrades, "data/upgrades.ron")?;
        let balance: Balance = parse(balance, "data/balance.ron")?;
        let waves: WaveTable = parse(waves, "data/waves.ron")?;
        Defs::checked(enemies, upgrades, balance, waves)
    }

    /// Проверить таблицы и собрать определения.
    fn checked(enemies: Vec<EnemyDef>, mut upgrades: Vec<UpDef>, balance: Balance, waves: WaveTable) -> Result<Defs, String> {
        for kind in Kind::ALL {
            if !enemies.iter().any(|e| e.id == kind) {
                return Err(format!("data/enemies.ron has no entry for {kind:?}"));
            }
        }
        // Порядок записей — порядок перечисления `Up`.
        upgrades.sort_by_key(|u| u.id.index());
        if upgrades.len() != UP_COUNT || upgrades.iter().enumerate().any(|(i, u)| u.id.index() != i) {
            return Err(format!("data/upgrades.ron must list each of the {UP_COUNT} upgrades exactly once"));
        }
        Ok(Defs { balance, enemies, upgrades, waves })
    }

    /// Копия файлов данных, вшитая при сборке (для тестов без приложения).
    pub fn embedded() -> Defs {
        Defs::parse(ENEMIES, UPGRADES, BALANCE, WAVES).expect("the bundled data files are valid")
    }

    /// Прочитать файлы из базы ассетов (каталог или пак).
    pub fn load(db: &AssetDatabase) -> Result<Defs, String> {
        // Чтение и разбор — движком (`AssetDatabase::load_data`).
        fn read<T: for<'de> Deserialize<'de>>(db: &AssetDatabase, path: &str) -> Result<T, String> {
            db.load_data(path).map_err(|e| format!("{e}. Fix the RON file in assets/data"))
        }
        Defs::checked(read(db, "data/enemies.ron")?, read(db, "data/upgrades.ron")?, read(db, "data/balance.ron")?, read(db, "data/waves.ron")?)
    }

    pub fn enemy(&self, kind: Kind) -> &EnemyDef {
        self.enemies.iter().find(|e| e.id == kind).expect("validated in Defs::parse")
    }

    pub fn upgrade(&self, up: Up) -> &UpDef {
        &self.upgrades[up.index()]
    }
}

static CURRENT: RwLock<Option<&'static Defs>> = RwLock::new(None);

/// Определения игры, действующие сейчас.
pub fn defs() -> &'static Defs {
    if let Some(d) = *CURRENT.read().expect("defs lock") {
        return d;
    }
    let d: &'static Defs = Box::leak(Box::new(Defs::embedded()));
    let mut slot = CURRENT.write().expect("defs lock");
    *slot.get_or_insert(d)
}

/// Сделать `new` действующими определениями (игра зовёт это при старте после чтения базы).
pub fn install(new: Defs) {
    let d: &'static Defs = Box::leak(Box::new(new));
    *CURRENT.write().expect("defs lock") = Some(d);
}

/// Шаблон для таблицы волн из генератора (запекатель пишет им `waves.ron`).
pub fn bake_wave_table(count: u32) -> WaveTable {
    let waves = (1..=count)
        .map(|w| {
            let plan = crate::data::generate_wave(w);
            WaveDef { groups: plan.groups, boss: plan.boss }
        })
        .collect();
    WaveTable { waves }
}

impl Default for Pattern {
    fn default() -> Self {
        Pattern::Scatter
    }
}
