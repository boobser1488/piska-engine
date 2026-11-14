//! Карта «Ташбазар»: пустынный городок на две точки задания. План (x — восток, z — юг,
//! север — к −z, 1 клетка схемы ≈ 4 м) — в README игры. Ключевые меры (docs/api/fps.md):
//! рост корпуса 1.37 м, глаза 1.22 м, ступень ≤ 0.34 м (у нас 0.25), прыжок 1.09 м
//! (ящик 0.9 м), прыжок с приседом 1.43 м (ящик 1.25 м), проходы 3–8 м, двери 2.2 м.
//!
//! Улицы и дворы описаны прямоугольниками «где можно ходить»; всё прочее внутри границ
//! карты застраивается домами автоматически (сетка по границам прямоугольников, клетки
//! сливаются в блоки), поэтому между улицами не бывает щелей в пустоту, а стены домов
//! точно совпадают с краями улиц. Особые постройки (крыши полосы препятствий, акведук,
//! арки, площадки) заданы явно.

use std::error::Error;

use engine::prelude::*;
use tashbazar::{Route, RoutePoint, Step};

use super::build::{rect, Builder, Rect};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Границы застройки.
const EXTENT: Rect = Rect { x0: -62.0, x1: 58.0, z0: -64.0, z1: 54.0 };
/// Канал (с береговыми камнями).
const CANAL: Rect = Rect { x0: -5.0, x1: 5.0, z0: -40.0, z1: 22.0 };
const CANAL_Y: f32 = -1.5;
/// Колодцы лестниц из канала на площади у моста.
const WEST_WELL: Rect = Rect { x0: -6.6, x1: -4.0, z0: -12.0, z1: -9.0 };
const EAST_WELL: Rect = Rect { x0: 4.0, x1: 6.6, z0: -2.5, z1: 0.5 };

/// Улица / двор: прямоугольник, название зоны (пусто — без своей зоны).
struct Walk {
    r: Rect,
    zone: &'static str,
}

fn walks() -> Vec<Walk> {
    let w = |x0, x1, z0, z1, zone| Walk { r: rect(x0, x1, z0, z1), zone };
    vec![
        w(-14.0, 14.0, -58.0, -40.0, "Караванные ворота"),
        w(-50.0, -14.0, -54.0, -46.0, "Канатная улица"),
        w(-50.0, -42.0, -46.0, 0.0, "Длинная стена"),
        w(-42.0, -36.0, -20.0, -17.0, "Красильный проход"),
        w(-36.0, -14.0, -26.5, -12.0, "Красильный двор"),
        w(-22.0, -4.0, -30.0, -26.5, "Обрыв над каналом"),
        w(-14.0, -4.0, -12.0, -4.0, "Западный спуск"),
        w(-42.0, -14.0, -10.0, -4.0, "Фонарный переулок"),
        w(-30.0, -25.0, -12.0, -10.0, ""),
        w(-31.0, -28.0, -4.0, 0.0, "Короткие ворота"),
        w(-54.0, -28.0, 0.0, 26.0, "Цистерна — площадка A"),
        w(-40.0, -34.0, 26.0, 34.0, "Оливковые ворота"),
        w(-40.0, -14.0, 34.0, 40.0, "Оливковая улица"),
        w(-14.0, 14.0, 22.0, 48.0, "Двор колодца"),
        w(14.0, 40.0, 34.0, 40.0, "Гранатовый сад"),
        w(32.0, 38.0, 28.0, 34.0, "Садовые ворота"),
        w(24.0, 52.0, 2.0, 28.0, "Караван-сарай — площадка Б"),
        w(4.0, 20.0, -8.0, -4.0, "Задний переулок"),
        w(16.0, 20.0, -4.0, 12.0, "Задний переулок"),
        w(20.0, 24.0, 8.0, 12.0, ""),
        w(4.0, 10.0, -8.0, 1.0, "Восточный спуск"),
        w(38.0, 44.0, -10.0, 2.0, "Тёмный проход"),
        w(38.0, 44.0, -48.0, -10.0, "Крытый базар"),
        w(14.0, 44.0, -54.0, -48.0, "Пряный ряд"),
    ]
}

/// Явные постройки (не заполняются автоматически): (план, высота, материал, имя).
fn landmarks() -> Vec<(Rect, f32, &'static str, &'static str)> {
    vec![
        (rect(-14.0, -4.0, -40.0, -30.0), 3.95, "plaster_ochre", "Gatehouse"),
        (rect(-14.0, -4.0, -26.5, -12.0), 3.45, "plaster_rose", "Bank House"),
        (rect(-36.0, -30.0, -12.0, -10.0), 2.5, "sandstone", "Yard Wall"),
        (rect(-25.0, -14.0, -12.0, -10.0), 2.5, "sandstone", "Yard Wall"),
        (rect(-42.0, -36.0, -4.0, 0.0), 4.4, "sandstone", "Arch Pier"),
        (rect(-36.0, -31.0, -4.0, 0.0), 2.4, "sandstone", "Low Wall"),
        (rect(-42.0, -36.0, -26.5, -20.0), 5.0, "plaster", "Dye Works"),
        (rect(-42.0, -36.0, -17.0, -10.0), 5.0, "plaster", "Dye Works"),
    ]
}

fn hash(a: f32, b: f32) -> u32 {
    let mut h = ((a * 10.0) as i32 as u32).wrapping_mul(0x9e37_79b9) ^ ((b * 10.0) as i32 as u32).wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// Результат: маршруты для ботов и отметки полосы.
pub struct Built {
    pub routes: Vec<Route>,
    pub objects: usize,
}

fn rp(x: f32, y: f32, z: f32, step: Step) -> RoutePoint {
    RoutePoint { pos: [x, y, z], step }
}

/// Построить карту в мире (ассеты подключены, префабы на диске).
pub fn build(b: &mut Builder) -> Result<Built> {
    let walks = walks();
    let marks = landmarks();

    // ---------------------------------------------------------------- земля
    b.root_group("Ground");
    let holes = vec![CANAL, WEST_WELL, EAST_WELL];
    b.floor("paving", "Street", EXTENT, &holes, 0.0, 2.0)?;
    // Береговые камни канала (верх — уровень улицы) с разрывами у колодцев лестниц.
    for (x0, x1, gaps) in [(-5.0, -4.0, (WEST_WELL.z0, WEST_WELL.z1)), (4.0, 5.0, (EAST_WELL.z0, EAST_WELL.z1))] {
        b.bx("sandstone", "Canal Bank", rect(x0, x1, CANAL.z0, gaps.0), -2.0, 0.0)?;
        b.bx("sandstone", "Canal Bank", rect(x0, x1, gaps.1, CANAL.z1), -2.0, 0.0)?;
    }
    b.bx("sand", "Canal Bed", rect(-4.0, 4.0, CANAL.z0, CANAL.z1), -2.5, CANAL_Y)?;
    // Пандусы в канал (14°) и лестницы из колодцев на площади у моста.
    b.ramp("sandstone", "Canal Ramp North", true, -4.0, 4.0, -34.0, CANAL_Y, -40.0, 0.0, 1.2)?;
    b.ramp("sandstone", "Canal Ramp South", true, -4.0, 4.0, 16.0, CANAL_Y, 22.0, 0.0, 1.2)?;
    b.stairs("sandstone_dark", "Steps", false, WEST_WELL.z0, WEST_WELL.z1, -4.0, CANAL_Y, -6.6, 0.0, 6, -2.0)?;
    b.stairs("sandstone_dark", "Steps", false, EAST_WELL.z0, EAST_WELL.z1, 4.0, CANAL_Y, 6.6, 0.0, 6, -2.0)?;
    // Облицовка стенок колодцев лестниц (иначе видны срезы мостовой).
    for well in [WEST_WELL, EAST_WELL] {
        // Торец у канала — на 1 см за нижней ступенью, верх — на 5 мм ниже мостовой:
        // ни одна грань облицовки не совпадает с гранями ступеней.
        let (x0, x1) = if well.x1 <= 0.0 { (well.x0, well.x1 - 0.01) } else { (well.x0 + 0.01, well.x1) };
        for (z0, z1) in [(well.z0, well.z0 + 0.03), (well.z1 - 0.03, well.z1)] {
            b.bx("sandstone", "Stair Wall", rect(x0, x1, z0, z1), CANAL_Y, -0.005)?;
        }
    }
    // Плитка площадки A и песок красильного двора — вставки на 3–4 см выше мостовой.
    b.bx("tiles", "Site Tiles", rect(-54.0, -28.0, 0.0, 26.0), 0.0, 0.04)?;
    b.bx("sand", "Yard Sand", rect(-36.0, -14.0, -26.5, -12.0), 0.0, 0.03)?;
    // Дворы — крупные плиты, базар — булыжник (вставки на 2–3 см выше мостовой).
    b.bx("flagstone", "Court Flagstones", rect(-14.0, 14.0, 22.0, 48.0), 0.0, 0.03)?;
    b.bx("flagstone", "Court Flagstones", rect(24.0, 52.0, 2.0, 28.0), 0.0, 0.03)?;
    b.bx("cobbles", "Market Cobbles", rect(14.0, 44.0, -54.0, -48.0), 0.0, 0.02)?;
    b.bx("cobbles", "Market Cobbles", rect(38.0, 44.0, -48.0, -10.0), 0.0, 0.02)?;

    // ---------------------------------------------------------------- застройка
    b.root_group("Buildings");
    let reserved: Vec<Rect> = marks.iter().map(|m| m.0).collect();
    let fill = filler_blocks(&walks, &reserved);
    let palette = ["plaster", "plaster_ochre", "plaster_rose", "sandstone", "plaster", "plaster_ochre"];
    let walkable = |x: f32, z: f32| {
        walks.iter().any(|w| x > w.r.x0 && x < w.r.x1 && z > w.r.z0 && z < w.r.z1)
            || (x > CANAL.x0 && x < CANAL.x1 && z > CANAL.z0 && z < CANAL.z1)
    };
    for r in &fill {
        let h = hash(r.x0, r.z0);
        // Окраина выше: из города не видно «края мира».
        let edge = r.x0 <= EXTENT.x0 + 0.1 || r.x1 >= EXTENT.x1 - 0.1 || r.z0 <= EXTENT.z0 + 0.1 || r.z1 >= EXTENT.z1 - 0.1;
        let height = if edge { 8.0 + (h % 4) as f32 * 0.5 } else { 4.5 + (h % 7) as f32 * 0.5 };
        let material = palette[(h >> 8) as usize % palette.len()];
        b.bx(material, "House", *r, 0.0, height)?;
        dress_house(b, r, height, material, &walkable, h)?;
    }
    for (r, height, material, name) in &marks {
        b.bx(material, name, *r, 0.0, *height)?;
        if *height > 3.0 && !matches!(*name, "Gatehouse" | "Bank House") {
            b.bx("trim", "Roof Band", *r, *height, *height + 0.3)?;
        }
    }
    // Своды над проходами: крыша красильного прохода и тёмного прохода, опора в нём.
    b.bx("plaster", "Tunnel Roof", rect(-42.0, -36.0, -20.0, -17.0), 2.8, 5.0)?;
    b.bx("sandstone", "Tunnel Roof", rect(38.0, 44.0, -10.0, 2.0), 3.2, 6.0)?;
    b.bx("sandstone", "Tunnel Pier", rect(38.0, 41.0, -6.0, -2.0), 0.0, 3.2)?;
    // Арка из Длинной стены на площадку A: перемычка над проходом.
    b.bx("sandstone_dark", "Arch Lintel", rect(-50.0, -42.0, -1.0, 0.0), 3.4, 4.4)?;
    b.bx("trim", "Arch Trim", rect(-50.0, -42.0, -1.05, 0.05), 3.25, 3.4)?;
    // Парапет на обрыве над каналом.
    b.bx("sandstone_dark", "Parapet", rect(-5.0, -4.0, -30.0, -26.5), 0.0, 1.0)?;
    // Парапеты у колодцев лестниц (со стороны площади), кроме входа.
    b.bx("sandstone_dark", "Parapet", rect(-6.6, -4.0, -12.0, -11.8), 0.0, 0.9)?;
    b.bx("sandstone_dark", "Parapet", rect(4.0, 6.6, 0.3, 0.5), 0.0, 0.9)?;

    // ---------------------------------------------------------------- мост
    b.root_group("Camel Bridge");
    b.bx("sandstone", "Deck", rect(-4.0, 4.0, -8.0, -4.0), 0.5, 0.75)?;
    b.stairs("sandstone", "Bridge Steps", false, -8.0, -4.0, -7.0, 0.0, -4.0, 0.75, 3, 0.0)?;
    b.stairs("sandstone", "Bridge Steps", false, -8.0, -4.0, 7.0, 0.0, 4.0, 0.75, 3, 0.0)?;
    b.bx("sandstone_dark", "Bridge Parapet", rect(-4.0, 4.0, -8.0, -7.7), 0.75, 1.45)?;
    b.bx("sandstone_dark", "Bridge Parapet", rect(-4.0, 4.0, -4.3, -4.0), 0.75, 1.45)?;
    b.prop("lantern_post", -7.6, 0.0, -7.4, 0.0)?;
    b.prop("lantern_post", 7.6, 0.0, -7.4, 180.0)?;

    // ---------------------------------------------------------------- акведук и башня
    b.root_group("Aqueduct");
    let deck = |b: &mut Builder, r: Rect| -> Result<()> {
        b.bx("sandstone", "Aqueduct Deck", r, 2.6, 3.0)?;
        Ok(())
    };
    deck(b, rect(-23.0, -14.0, -20.0, -17.0))?;
    deck(b, rect(-36.0, -27.35, -20.0, -17.0))?;
    deck(b, rect(-36.0, -33.0, -17.0, 2.0))?;
    for (x, z) in [(-16.5, -18.5), (-21.5, -18.5), (-30.5, -18.5), (-34.5, -14.0)] {
        b.bx("sandstone_dark", "Aqueduct Pier", rect(x - 0.5, x + 0.5, z - 0.5, z + 0.5), 0.0, 2.6)?;
    }
    b.bx("sandstone_dark", "Water Tower", rect(-37.0, -32.0, 2.0, 7.0), 0.0, 4.25)?;
    b.bx("trim", "Tower Band", rect(-37.06, -31.94, 1.94, 7.06), 3.6, 3.8)?;

    // ---------------------------------------------------------------- площадка A
    b.root_group("Site A — Cistern");
    b.bx("sandstone_dark", "Cistern Platform", rect(-48.0, -38.0, 10.0, 18.0), 0.0, 1.25)?;
    b.stairs("sandstone", "Platform Steps", true, -45.0, -41.0, 20.5, 0.0, 18.0, 1.25, 5, 0.0)?;
    let tank = b.spawn("shapes/tank", "Cistern Tank", Vec3::new(-43.0, 1.25 + 0.7, 14.0), Quat::IDENTITY, Vec3::new(3.6, 1.4, 3.6))?;
    let _ = tank;
    b.spawn("shapes/water_disc", "Cistern Water", Vec3::new(-43.0, 1.25 + 1.41, 14.0), Quat::IDENTITY, Vec3::new(3.2, 0.02, 3.2))?;
    b.prop("crate_big", -31.5, 0.04, 14.5, 0.0)?;
    b.crate_stack(-30.2, 0.04, 18.5, 2, 0.0)?;
    b.prop("crate_small", -31.1, 0.04, 18.5, 0.0)?;
    b.prop("barrel", -52.2, 0.04, 23.5, 0.0)?;
    b.prop("barrel", -51.3, 0.04, 24.4, 0.0)?;
    b.prop("cart", -50.5, 0.04, 6.5, 90.0)?;
    b.palm(-45.0, 0.04, 4.0)?;
    b.palm(-37.0, 0.04, 22.5)?;
    b.prop("crate_big", -44.0, 0.04, 23.5, 0.0)?;
    b.wall_mount("props/wall_lantern", -43.0, 2.4, 26.0, 180.0)?;

    // ---------------------------------------------------------------- площадка Б
    b.root_group("Site B — Caravanserai");
    // Северная галерея (крыша — балкон с парапетом) и восточная аркада.
    b.bx("sandstone", "Gallery Roof", rect(24.0, 52.0, 2.0, 6.0), 3.4, 3.8)?;
    for x in [27.0, 32.0, 36.6, 45.4, 50.0] {
        b.bx("sandstone_dark", "Gallery Pillar", rect(x - 0.4, x + 0.4, 5.2, 6.0), 0.0, 3.4)?;
    }
    // Лестница на галерею вдоль её южного края (поднимается на запад), парапет — кроме выхода.
    b.bx("sandstone_dark", "Gallery Parapet", rect(27.5, 52.0, 5.7, 6.0), 3.8, 4.4)?;
    b.stairs("sandstone", "Gallery Stairs", false, 6.0, 7.6, 33.0, 0.0, 26.25, 3.75, 15, 0.0)?;
    b.bx("sandstone", "Arcade Roof", rect(48.0, 52.0, 6.0, 28.0), 3.4, 3.8)?;
    for z in [11.0, 16.5, 22.0] {
        b.bx("sandstone_dark", "Arcade Pillar", rect(48.0, 48.8, z - 0.4, z + 0.4), 0.0, 3.4)?;
    }
    // Бассейн с водой посередине (низкая стенка — укрытие, 0.6 м).
    for r in [rect(35.0, 41.0, 13.0, 13.4), rect(35.0, 41.0, 18.6, 19.0), rect(35.0, 35.4, 13.4, 18.6), rect(40.6, 41.0, 13.4, 18.6)] {
        b.bx("sandstone_dark", "Basin Wall", r, 0.0, 0.6)?;
    }
    b.bx("water", "Basin Water", rect(35.4, 40.6, 13.4, 18.6), -0.2, 0.3)?;
    b.prop("crate_big", 45.5, 0.0, 9.5, 15.0)?;
    b.crate_stack(28.6, 0.0, 9.0, 2, 0.0)?;
    b.prop("crate_small", 29.5, 0.0, 9.0, 0.0)?;
    b.prop("cart", 43.5, 0.0, 25.0, 0.0)?;
    b.prop("barrel", 44.3, 0.0, 7.0, 0.0)?;
    b.prop("barrel", 45.2, 0.0, 7.4, 0.0)?;
    b.prop("sack", 44.6, 0.0, 8.2, 40.0)?;
    b.palm(28.5, 0.0, 23.5)?;
    b.palm(43.0, 0.0, 19.5)?;
    for x in [30.0, 47.0] {
        b.wall_mount("props/wall_lantern", x, 2.4, 2.0, 0.0)?;
    }

    // ---------------------------------------------------------------- базар и проходы
    b.root_group("Spice Market");
    for x in [17.0, 23.0, 29.0, 35.0, 41.0] {
        b.bx("wood_dark", "Beam", rect(x - 0.15, x + 0.15, -54.0, -48.0), 3.3, 3.55)?;
    }
    // Полотна лежат на балках (низ полотна = верх балок).
    for (i, x) in [20.0, 26.0, 32.0, 38.0].iter().enumerate() {
        let kind = if i % 2 == 0 { "cloth_red" } else { "cloth_blue" };
        b.spawn(&format!("blocks/{kind}"), "Canopy", Vec3::new(*x, 3.57, -51.0), Quat::IDENTITY, Vec3::new(6.0, 0.04, 5.6))?;
    }
    for x in [27.5, 31.25, 35.0] {
        b.prop("market_stall", x, 0.0, -54.0 + PLINTH, 0.0)?;
    }
    b.prop("sack", 23.9, 0.0, -53.4, 10.0)?;
    b.prop("jar", 21.35, 0.0, -53.55, 0.0)?;
    b.prop("jar", 21.95, 0.0, -53.35, 0.0)?;
    b.wall_mount("props/wall_lantern", 23.5, 2.5, -48.0, 180.0)?;
    b.wall_mount("props/wall_lantern", 38.0, 2.5, -48.0, 180.0)?;

    b.root_group("Covered Souk");
    let beams = [-45.0, -39.0, -33.0, -27.0, -21.0, -15.0, -10.15];
    for z in beams {
        b.bx("wood_dark", "Beam", rect(38.0, 44.0, z - 0.15, z + 0.15), 3.3, 3.55)?;
    }
    for (i, pair) in beams.windows(2).enumerate() {
        let kind = if i % 2 == 0 { "cloth_blue" } else { "cloth_red" };
        let (z0, z1) = (pair[0], pair[1]);
        b.spawn(&format!("blocks/{kind}"), "Canopy", Vec3::new(41.0, 3.57, (z0 + z1) * 0.5), Quat::IDENTITY, Vec3::new(5.6, 0.04, z1 - z0))?;
    }
    for z in [-42.0, -31.0, -20.0] {
        b.prop("market_stall", 38.0 + PLINTH, 0.0, z, 90.0)?;
    }
    b.prop("sack", 39.0, 0.0, -36.5, 0.0)?;
    b.prop("barrel", 43.3, 0.0, -26.0, 0.0)?;
    for z in [-44.0, -34.0, -24.0, -14.0] {
        b.wall_mount("props/wall_lantern", 44.0, 2.4, z, 270.0)?;
    }
    b.root_group("Dark Passage");
    for z in [-8.0, 0.5] {
        b.wall_mount("props/wall_lantern", 44.0, 2.3, z, 270.0)?;
    }

    // ---------------------------------------------------------------- красильный двор
    b.root_group("Dye Yard");
    // Чаны стоят на песке двора (верх песка — 0.03), краска — тонкий диск на срезе чана.
    for (x, z) in [(-31.0, -23.0), (-19.0, -15.5)] {
        b.spawn("shapes/tank", "Dye Vat", Vec3::new(x, 0.53, z), Quat::IDENTITY, Vec3::new(2.0, 1.0, 2.0))?;
        b.spawn("shapes/water_disc", "Dye", Vec3::new(x, 1.04, z), Quat::IDENTITY, Vec3::new(1.7, 0.02, 1.7))?;
    }
    b.prop("drying_line", -27.0, 0.03, -14.5, 0.0)?;
    b.prop("drying_line", -23.0, 0.03, -24.5, 0.0)?;
    b.prop("crate_small", -15.0, 0.03, -24.5, 0.0)?;
    b.prop("barrel", -35.2, 0.03, -12.9, 0.0)?;

    // ---------------------------------------------------------------- спавны и окрестности
    b.root_group("Caravan Gate — Attackers");
    // Ворота в северной стене (наглухо закрыты) — деталь фасада.
    for (i, x) in [-6.0f32, -3.0, 0.0, 3.0, 6.0].iter().enumerate() {
        let id = b.spawn("markers/spawn_attackers", "Spawn A", Vec3::new(*x, 0.0, -51.0), Quat::from_rotation_y(std::f32::consts::PI), Vec3::ONE)?;
        if let Some(s) = b.world.scene.get_component_mut::<tashbazar::scripts::spawn_point::SpawnPoint>(id) {
            s.index = i as i32;
        }
    }
    // Лестница из ящиков на крышу сторожки (полоса препятствий): 0.9 / 1.8 / 2.7 м, на
    // верхнем ярусе — 1.8 м разбега, дальше 1.6 м до стены и прыжок с приседом на 3.95 м.
    b.crate_stack(-9.0, 0.0, -44.75, 1, 0.0)?;
    b.crate_stack(-9.0, 0.0, -43.85, 2, 0.0)?;
    for level in 0..3 {
        b.prop("crate_long", -9.0, 0.9 * level as f32, -42.5, 90.0)?;
    }
    b.prop("cart", 10.5, 0.0, -44.0, 0.0)?;
    b.palm(-11.5, 0.0, -52.5)?;
    b.palm(10.0, 0.0, -54.0)?;
    b.prop("crate_big", 4.0, 0.0, -45.5, 10.0)?;
    b.prop("barrel", -3.5, 0.0, -44.5, 0.0)?;
    for x in [-8.0, 8.0] {
        b.prop("lantern_post", x, 0.0, -56.5, 0.0)?;
    }

    b.root_group("Well Court — Defenders");
    b.prop("well", 0.0, 0.0, 38.0, 0.0)?;
    for (i, x) in [-6.0f32, -3.0, 0.0, 3.0, 6.0].iter().enumerate() {
        let id = b.spawn("markers/spawn_defenders", "Spawn D", Vec3::new(*x, 0.0, 45.0), Quat::IDENTITY, Vec3::ONE)?;
        if let Some(s) = b.world.scene.get_component_mut::<tashbazar::scripts::spawn_point::SpawnPoint>(id) {
            s.index = i as i32;
        }
    }
    b.palm(-9.0, 0.0, 43.5)?;
    b.palm(9.0, 0.0, 27.5)?;
    b.prop("bench", -9.0, 0.0, 30.0, 90.0)?;
    b.prop("plant_pot", 9.0, 0.0, 31.0, 0.0)?;
    for x in [-7.0, 7.0] {
        b.prop("lantern_post", x, 0.0, 25.0, 0.0)?;
    }

    b.root_group("Streets");
    // Длинная стена: укрытия посередине длинной прямой.
    b.crate_stack(-48.6, 0.0, -24.0, 2, 0.0)?;
    b.prop("crate_small", -47.7, 0.0, -24.0, 0.0)?;
    b.prop("cart", -43.4, 0.0, -36.0, 90.0)?;
    b.prop("barrel", -49.2, 0.0, -10.5, 0.0)?;
    // Канатная улица.
    b.prop("sack", -30.0, 0.0, -53.3, 0.0)?;
    b.prop("crate_small", -24.0, 0.0, -53.4, 0.0)?;
    // Канал: лодка на сухом дне и ящики.
    b.prop("crate_long", 2.8, CANAL_Y, -24.0, 85.0)?;
    b.prop("barrel", 2.9, CANAL_Y, -22.4, 0.0)?;
    b.prop("barrel", 3.1, CANAL_Y, -21.5, 0.0)?;
    b.prop("crate_small", -2.8, CANAL_Y, 6.0, 0.0)?;
    b.prop("crate_long", 2.6, CANAL_Y, 12.0, 90.0)?;
    // Улицы защитников.
    for (x, z) in [(-20.0, 39.0), (-33.0, 35.0)] {
        b.prop("plant_pot", x, 0.0, z, 0.0)?;
    }
    b.prop("plant_pot", -26.0, 0.0, 39.2, 0.0)?;
    b.prop("plant_pot", 22.0, 0.0, 34.8, 0.0)?;
    b.prop("plant_pot", 30.0, 0.0, 39.2, 0.0)?;
    b.prop("bench", 26.0, 0.0, 39.5, 0.0)?;
    b.prop("plant_pot", 37.0, 0.0, 35.0, 0.0)?;
    // Задний переулок и площади у моста.
    b.prop("crate_big", 19.2, 0.0, 4.0, 0.0)?;
    b.prop("barrel", 12.0, 0.0, -7.3, 0.0)?;
    b.prop("lantern_post", 9.2, 0.0, 0.2, 180.0)?;
    b.prop("lantern_post", -13.2, 0.0, -11.2, 0.0)?;
    b.wall_mount("props/wall_lantern", -33.0, 2.0, -10.0, 0.0)?;
    b.wall_mount("props/wall_lantern", -39.0, 2.1, -20.0, 0.0)?;

    // ---------------------------------------------------------------- зоны
    // Окна — последними: перед ними уже стоят лотки и фонари, на стенах висят фонари и навесы.
    b.finish_windows()?;

    b.root_group("Zones");
    for walk in &walks {
        if walk.zone.is_empty() {
            continue;
        }
        let c = walk.r.center();
        let size = Vec3::new(walk.r.x1 - walk.r.x0, 7.0, walk.r.z1 - walk.r.z0);
        zone(b, walk.zone, Vec3::new(c.x, 1.5, c.y), size, walk.zone.contains("площадка"))?;
    }
    zone(b, "Сухой канал", Vec3::new(0.0, -0.5, -9.0), Vec3::new(8.0, 3.0, 62.0), false)?;
    zone(b, "Верблюжий мост", Vec3::new(0.0, 1.5, -6.0), Vec3::new(14.0, 3.0, 4.0), false)?;
    zone(b, "Арка", Vec3::new(-46.0, 1.5, -1.0), Vec3::new(8.0, 3.0, 4.0), false)?;
    zone(b, "Акведук", Vec3::new(-25.0, 4.5, -18.5), Vec3::new(22.0, 3.0, 3.0), false)?;
    zone(b, "Акведук", Vec3::new(-34.5, 4.5, -7.5), Vec3::new(3.0, 3.0, 19.0), false)?;
    zone(b, "Водонапорная башня", Vec3::new(-34.5, 5.5, 4.5), Vec3::new(5.0, 2.5, 5.0), false)?;
    zone(b, "Крыша сторожки", Vec3::new(-9.0, 5.0, -35.0), Vec3::new(10.0, 2.5, 10.0), false)?;
    zone(b, "Крыша дома у канала", Vec3::new(-9.0, 4.5, -19.25), Vec3::new(10.0, 2.5, 14.5), false)?;
    zone(b, "Галерея", Vec3::new(38.0, 5.0, 4.0), Vec3::new(28.0, 2.5, 4.0), false)?;
    zone(b, "Цистерна", Vec3::new(-43.0, 2.4, 14.0), Vec3::new(10.0, 2.5, 8.0), false)?;

    // ---------------------------------------------------------------- маршруты
    let routes = routes();
    Ok(Built { routes, objects: b.count })
}

fn zone(b: &mut Builder, title: &str, center: Vec3, size: Vec3, site: bool) -> Result<()> {
    let id = b.spawn("markers/zone", title, center, Quat::IDENTITY, Vec3::ONE)?;
    if let Some(z) = b.world.scene.get_component_mut::<tashbazar::scripts::map_zone::MapZone>(id) {
        z.title = title.to_string();
        z.size = size;
        z.site = site;
    }
    Ok(())
}

/// Сетка застройки: всё внутри границ, что не улица, не канал и не явная постройка.
fn filler_blocks(walks: &[Walk], reserved: &[Rect]) -> Vec<Rect> {
    let mut xs = vec![EXTENT.x0, EXTENT.x1];
    let mut zs = vec![EXTENT.z0, EXTENT.z1];
    let mut all: Vec<Rect> = walks.iter().map(|w| w.r).collect();
    // Береговые камни (x ±4…5) под домами — часть домов: вдоль канала нет «тротуара».
    all.push(rect(-4.0, 4.0, CANAL.z0, CANAL.z1));
    all.extend_from_slice(reserved);
    for r in &all {
        xs.extend([r.x0, r.x1]);
        zs.extend([r.z0, r.z1]);
    }
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    zs.sort_by(f32::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    let solid = |x0: f32, x1: f32, z0: f32, z1: f32| {
        let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
        !all.iter().any(|r| cx > r.x0 && cx < r.x1 && cz > r.z0 && cz < r.z1)
    };
    // Ряды: отрезки подряд идущих сплошных клеток.
    let mut rows: Vec<Vec<(f32, f32)>> = Vec::new();
    for zi in 0..zs.len() - 1 {
        let mut runs = Vec::new();
        let mut start: Option<f32> = None;
        for xi in 0..xs.len() - 1 {
            let s = solid(xs[xi], xs[xi + 1], zs[zi], zs[zi + 1]);
            if s && start.is_none() {
                start = Some(xs[xi]);
            }
            if !s {
                if let Some(x0) = start.take() {
                    runs.push((x0, xs[xi]));
                }
            }
        }
        if let Some(x0) = start {
            runs.push((x0, *xs.last().unwrap()));
        }
        rows.push(runs);
    }
    // Слить одинаковые отрезки соседних рядов в блоки; длинные блоки порезать на дома.
    let mut blocks: Vec<Rect> = Vec::new();
    let mut open: Vec<(f32, f32, f32)> = Vec::new(); // (x0, x1, z0)
    for zi in 0..rows.len() {
        let z0 = zs[zi];
        let mut next_open = Vec::new();
        for (x0, x1, start) in open.drain(..) {
            if rows[zi].iter().any(|r| (r.0 - x0).abs() < 1e-3 && (r.1 - x1).abs() < 1e-3) {
                next_open.push((x0, x1, start));
            } else {
                blocks.push(rect(x0, x1, start, z0));
            }
        }
        for r in &rows[zi] {
            if !next_open.iter().any(|o: &(f32, f32, f32)| (o.0 - r.0).abs() < 1e-3 && (o.1 - r.1).abs() < 1e-3) {
                next_open.push((r.0, r.1, z0));
            }
        }
        open = next_open;
    }
    let z_end = *zs.last().unwrap();
    for (x0, x1, start) in open {
        blocks.push(rect(x0, x1, start, z_end));
    }
    // Дома не длиннее ~14 м: так разные высоты и цвета дают живой силуэт улицы.
    let mut houses = Vec::new();
    for r in blocks {
        let nx = ((r.x1 - r.x0) / 14.0).ceil().max(1.0) as usize;
        let nz = ((r.z1 - r.z0) / 14.0).ceil().max(1.0) as usize;
        for i in 0..nx {
            for j in 0..nz {
                let x0 = r.x0 + (r.x1 - r.x0) * i as f32 / nx as f32;
                let x1 = r.x0 + (r.x1 - r.x0) * (i + 1) as f32 / nx as f32;
                let z0 = r.z0 + (r.z1 - r.z0) * j as f32 / nz as f32;
                let z1 = r.z0 + (r.z1 - r.z0) * (j + 1) as f32 / nz as f32;
                houses.push(rect(x0, x1, z0, z1));
            }
        }
    }
    houses
}

/// Выступ цоколя от стены, м.
pub const PLINTH: f32 = 0.06;

/// Крытые проходы: на их стенах нет окон и дверей (своды ниже перемычек).
const COVERED: [Rect; 2] = [Rect { x0: 38.0, x1: 44.0, z0: -10.0, z1: 2.0 }, Rect { x0: -42.0, x1: -36.0, z0: -20.0, z1: -17.0 }];

/// Отделка дома со стороны улиц: цоколь (0.45 м, с коллайдером), карниз под крышей,
/// парапет на крыше, окна с рамами и распахнутыми ставнями, двери с коробкой. Только на
/// участках стен, выходящих на проходимое место; двери разрывают цоколь.
fn dress_house(b: &mut Builder, r: &Rect, height: f32, material: &str, walkable: &dyn Fn(f32, f32) -> bool, seed: u32) -> Result<()> {
    // (начало стены, вдоль, наружу, длина, поворот «наружу» в градусах)
    let sides = [
        (Vec2::new(r.x0, r.z0), Vec2::X, Vec2::new(0.0, -1.0), r.x1 - r.x0, 180.0f32),
        (Vec2::new(r.x0, r.z1), Vec2::X, Vec2::new(0.0, 1.0), r.x1 - r.x0, 0.0),
        (Vec2::new(r.x0, r.z0), Vec2::Y, Vec2::new(-1.0, 0.0), r.z1 - r.z0, -90.0),
        (Vec2::new(r.x1, r.z0), Vec2::Y, Vec2::new(1.0, 0.0), r.z1 - r.z0, 90.0),
    ];
    let step = 0.25;
    for (k, (origin, along, out, length, yaw)) in sides.into_iter().enumerate() {
        // Участки стены, перед которыми можно ходить.
        let n = (length / step).round().max(1.0) as usize;
        let mut segments: Vec<(f32, f32)> = Vec::new();
        let mut start: Option<f32> = None;
        for i in 0..n {
            let t = (i as f32 + 0.5) * length / n as f32;
            let p = origin + along * t + out * 0.6;
            let open = walkable(p.x, p.y);
            if open && start.is_none() {
                start = Some(i as f32 * length / n as f32);
            }
            if !open {
                if let Some(s0) = start.take() {
                    segments.push((s0, i as f32 * length / n as f32));
                }
            }
        }
        if let Some(s0) = start {
            segments.push((s0, length));
        }
        for (t0, t1) in segments {
            if t1 - t0 < 0.5 {
                continue;
            }
            // Внешний угол — продлить цоколь и карниз на выступ, чтобы угол был закрыт.
            let corner = |t: f32, dir: f32| {
                let at = origin + along * t + along * dir * 0.3 + out * 0.3;
                (t <= 1e-3 || t >= length - 1e-3) && walkable(at.x, at.y)
            };
            // Над каналом (пол на 1.5 м ниже) — без дверей и цоколя.
            let mid = origin + along * ((t0 + t1) * 0.5) + out * 0.6;
            let over_canal = mid.x > CANAL.x0 && mid.x < CANAL.x1 && mid.y > CANAL.z0 && mid.y < CANAL.z1;
            let ext0 = if corner(t0, -1.0) { PLINTH } else { 0.0 };
            let ext1 = if corner(t1, 1.0) { PLINTH } else { 0.0 };
            let piece = |a: f32, c: f32, depth: f32| {
                let p0 = origin + along * a;
                let p1 = origin + along * c + out * depth;
                rect(p0.x, p1.x, p0.y, p1.y)
            };
            // Двери и окна по участку.
            let count = ((t1 - t0) / 3.2).floor() as usize;
            let mut doors: Vec<f32> = Vec::new();
            for i in 0..count {
                let t = t0 + (i as f32 + 0.5) * (t1 - t0) / count as f32;
                let h = seed.wrapping_add((k as u32) * 977).wrapping_add(i as u32 * 131).wrapping_mul(0x9e37_79b9) >> 7;
                let at = origin + along * t;
                let front = at + out * 0.6;
                let covered = COVERED.iter().any(|c| front.x > c.x0 && front.x < c.x1 && front.y > c.z0 && front.y < c.z1);
                if covered {
                    continue;
                }
                if h.is_multiple_of(5) && t - t0 > 1.2 && t1 - t > 1.2 && !over_canal {
                    b.wall_mount("facade/door", at.x, 0.0, at.y, yaw)?;
                    doors.push(t);
                    // Навес над каждой второй дверью — если улица перед ней шире 3 м.
                    let far = at + out * 3.0;
                    if (h >> 3).is_multiple_of(2) && walkable(far.x, far.y) {
                        b.wall_mount("props/awning", at.x, 0.0, at.y, yaw)?;
                    }
                } else if height > 3.8 && h % 5 != 4 {
                    // Окна ставятся в конце (`finish_windows`): вариант зависит от того, что
                    // окажется перед ними и рядом на стене.
                    b.plan_window(at.x, 2.0, at.y, yaw, false, h);
                }
                if height > 6.4 && !(h >> 4).is_multiple_of(3) {
                    b.plan_window(at.x, 4.6, at.y, yaw, true, h.rotate_left(11));
                }
            }
            // Цоколь с разрывами под двери.
            let mut cuts = vec![t0 - ext0];
            for d in &doors {
                cuts.push(d - 0.77);
                cuts.push(d + 0.77);
            }
            cuts.push(t1 + ext1);
            for pair in cuts.chunks(2) {
                if pair[1] - pair[0] > 0.05 && !over_canal {
                    b.bx("sandstone_dark", "Plinth", piece(pair[0], pair[1], PLINTH), 0.0, 0.45)?;
                }
            }
            // Карниз под крышей (декор) и парапет на крыше (продолжение стены).
            b.bx("trim", "Cornice", piece(t0 - ext0, t1 + ext1, 0.1), height - 0.32, height - 0.12)?;
            let inner = |a: f32, c: f32| {
                let (a, c) = (a + 0.01, c - 0.01);
                let p0 = origin + along * a;
                let p1 = origin + along * c - out * 0.3;
                rect(p0.x, p1.x, p0.y, p1.y)
            };
            b.bx(material, "Parapet", inner(t0, t1), height, height + 0.5)?;
        }
    }
    Ok(())
}

/// Маршруты для ботов: главные пути от спавнов к точкам и к середине, и полоса.
fn routes() -> Vec<Route> {
    use Step::*;
    let route = |id: &str, title: &str, points: Vec<RoutePoint>| {
        let mut r = Route { id: id.to_string(), title: title.to_string(), seconds: 0.0, points };
        // Расчётное время: длина пути бегом (250 ед/с) плюс 5 % на повороты и лестницы.
        r.seconds = (r.length() / engine::fps::units(250.0) * 1.05 * 10.0).round() / 10.0;
        r
    };
    let y = CANAL_Y;
    vec![
        route(
            "atk_a_long",
            "Атакующие → A через Канатную улицу и Длинную стену",
            vec![rp(-6.0, 0.0, -50.0, Run), rp(-20.0, 0.0, -50.0, Run), rp(-46.0, 0.0, -49.0, Run), rp(-46.0, 0.0, -4.0, Run), rp(-44.0, 0.0, 2.0, Run), rp(-35.0, 0.0, 11.0, Run)],
        ),
        route(
            "atk_a_mid",
            "Атакующие → A через канал, Западный спуск и Короткие ворота",
            vec![
                rp(0.0, 0.0, -50.0, Run),
                rp(0.0, 0.0, -41.0, Run),
                rp(0.0, y, -33.0, Run),
                rp(-1.5, y, -12.0, Run),
                rp(-3.6, y, -10.5, Run),
                rp(-7.2, 0.0, -10.5, Run),
                rp(-11.0, 0.0, -7.0, Run),
                rp(-27.0, 0.0, -7.0, Run),
                rp(-29.5, 0.0, -4.5, Run),
                rp(-29.5, 0.0, 8.0, Run),
                rp(-35.0, 0.0, 11.0, Run),
            ],
        ),
        route(
            "atk_b_market",
            "Атакующие → Б через Пряный ряд, Крытый базар и Тёмный проход",
            vec![rp(0.0, 0.0, -50.0, Run), rp(16.0, 0.0, -50.5, Run), rp(41.0, 0.0, -50.5, Run), rp(42.0, 0.0, -12.0, Run), rp(42.5, 0.0, -7.0, Run), rp(42.5, 0.0, 0.5, Run), rp(40.0, 0.0, 6.5, Run), rp(32.0, 0.0, 14.0, Run)],
        ),
        route(
            "atk_b_mid",
            "Атакующие → Б через канал, Восточный спуск и Задний переулок",
            vec![
                rp(0.0, 0.0, -50.0, Run),
                rp(0.0, 0.0, -41.0, Run),
                rp(0.0, y, -33.0, Run),
                rp(1.8, y, -3.0, Run),
                rp(3.6, y, -1.0, Run),
                rp(7.2, 0.0, -1.0, Run),
                rp(9.0, 0.0, -6.0, Run),
                rp(18.0, 0.0, -6.0, Run),
                rp(18.0, 0.0, 10.0, Run),
                rp(25.0, 0.0, 10.0, Run),
                rp(32.0, 0.0, 14.0, Run),
            ],
        ),
        route("atk_mid", "Атакующие → середина (под Верблюжьим мостом)", vec![rp(0.0, 0.0, -50.0, Run), rp(0.0, 0.0, -41.0, Run), rp(0.0, y, -33.0, Run), rp(0.0, y, -6.0, Run)]),
        route(
            "def_a",
            "Защитники → A через Оливковую улицу",
            vec![rp(0.0, 0.0, 44.0, Run), rp(-10.0, 0.0, 37.0, Run), rp(-36.5, 0.0, 37.0, Run), rp(-36.5, 0.0, 28.0, Run), rp(-35.0, 0.0, 20.0, Run), rp(-35.0, 0.0, 11.0, Run)],
        ),
        route(
            "def_b",
            "Защитники → Б через Гранатовый сад",
            vec![rp(0.0, 0.0, 44.0, Run), rp(10.0, 0.0, 37.0, Run), rp(35.0, 0.0, 37.0, Run), rp(35.0, 0.0, 28.0, Run), rp(33.0, 0.0, 22.0, Run), rp(32.0, 0.0, 14.0, Run)],
        ),
        route("def_mid", "Защитники → середина (под Верблюжьим мостом)", vec![rp(0.0, 0.0, 44.0, Run), rp(2.6, 0.0, 38.0, Run), rp(2.5, 0.0, 30.0, Run), rp(0.0, 0.0, 23.0, Run), rp(0.0, y, 15.0, Run), rp(0.0, y, -6.0, Run)]),
        // Полоса препятствий: точки, где прыгать, приседать в прыжке и стрейфить.
        route(
            "course",
            "Полоса препятствий: ящики, крыши, акведук, башня, цистерна",
            vec![
                rp(-9.0, 0.0, -48.5, Run),
                rp(-9.0, 0.0, -46.9, Jump),
                rp(-9.0, 0.9, -44.75, Jump),
                rp(-9.0, 1.8, -43.85, Jump),
                rp(-9.0, 2.7, -43.1, Run),
                rp(-9.0, 2.7, -42.0, CrouchJump),
                rp(-9.0, 3.95, -37.0, Run),
                rp(-9.0, 3.95, -30.35, Jump),
                rp(-9.0, 3.45, -22.0, Run),
                rp(-11.0, 3.45, -18.5, Run),
                rp(-15.0, 3.0, -18.5, Run),
                rp(-23.1, 3.0, -18.5, StrafeJump),
                rp(-30.0, 3.0, -18.5, Run),
                rp(-34.5, 3.0, -17.5, Run),
                rp(-34.5, 3.0, 0.0, CrouchJump),
                rp(-34.5, 4.25, 4.5, Run),
                rp(-34.5, 4.25, 8.5, Run),
                rp(-32.8, 0.0, 11.5, Run),
                rp(-36.0, 0.0, 12.0, CrouchJump),
                rp(-39.4, 1.25, 12.0, Run),
            ],
        ),
    ]
}

/// План карты символами (для README): 1 символ = 2.5 м, север сверху.
pub fn ascii_plan() -> String {
    let walks = walks();
    let cell = 2.5;
    let mut out = String::new();
    let mut z = EXTENT.z0 + cell * 0.5;
    while z < EXTENT.z1 {
        let mut x = EXTENT.x0 + cell * 0.5;
        while x < EXTENT.x1 {
            let inside = |r: Rect| x > r.x0 && x < r.x1 && z > r.z0 && z < r.z1;
            let c = if inside(rect(-6.0, -4.0, -12.0, -9.0)) || inside(rect(4.0, 6.6, -2.5, 0.5)) {
                's'
            } else if inside(rect(-4.0, 4.0, -8.0, -4.0)) {
                '='
            } else if inside(rect(-4.0, 4.0, CANAL.z0, CANAL.z1)) {
                '~'
            } else if inside(rect(-37.0, -32.0, 2.0, 7.0)) {
                'T'
            } else if inside(rect(-48.0, -38.0, 10.0, 18.0)) {
                'C'
            } else if inside(rect(35.0, 41.0, 13.0, 19.0)) {
                'o'
            } else if inside(rect(-23.0, -14.0, -20.0, -17.0)) || inside(rect(-36.0, -27.35, -20.0, -17.0)) || inside(rect(-36.0, -33.0, -17.0, 2.0)) {
                '#'
            } else if inside(rect(-8.0, 8.0, -53.0, -49.0)) {
                'a'
            } else if inside(rect(-8.0, 8.0, 43.0, 47.0)) {
                'd'
            } else if walks.iter().any(|w| inside(w.r)) {
                if inside(rect(-54.0, -28.0, 0.0, 26.0)) {
                    'A'
                } else if inside(rect(24.0, 52.0, 2.0, 28.0)) {
                    'B'
                } else {
                    '.'
                }
            } else {
                '▓'
            };
            out.push(c);
            x += cell;
        }
        out.push('\n');
        z += cell;
    }
    out
}
