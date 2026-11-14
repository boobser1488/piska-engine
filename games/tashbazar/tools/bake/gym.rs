//! Полигон: эталонные размеры для контроллера — ступени 8/16/18/19 ед, ящики под прыжок
//! (40/56/58 ед) и прыжок с приседом (64/74/76 ед), склоны 30/45/46/60°, лаз высотой
//! 56 ед, проёмы и коридоры, щели 3 и 4.4 м, подвижная платформа. Каждая станция — зона:
//! HUD пишет, что это за мера.

use std::error::Error;

use engine::fps::units;
use engine::prelude::*;
use tashbazar::scripts::moving_platform::MovingPlatform;

use super::build::{rect, Builder};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn zone(b: &mut Builder, title: &str, center: Vec3, size: Vec3) -> Result<()> {
    let id = b.spawn("markers/zone", title, center, Quat::IDENTITY, Vec3::ONE)?;
    if let Some(z) = b.world.scene.get_component_mut::<tashbazar::scripts::map_zone::MapZone>(id) {
        z.title = title.to_string();
        z.size = size;
    }
    Ok(())
}

pub fn build(b: &mut Builder) -> Result<()> {
    b.root_group("Gym");
    b.bx("tiles", "Floor", rect(-30.0, 30.0, -24.0, 24.0), -1.0, 0.0)?;
    for r in [rect(-31.0, 31.0, -25.0, -24.0), rect(-31.0, 31.0, 24.0, 25.0), rect(-31.0, -30.0, -24.0, 24.0), rect(30.0, 31.0, -24.0, 24.0)] {
        b.bx("plaster_ochre", "Wall", r, 0.0, 6.0)?;
        b.bx("trim", "Wall Band", r, 6.0, 6.3)?;
    }

    // Ступени (вдоль −Z), каждая 2 м шириной.
    b.root_group("Steps");
    for (i, u) in [8.0f32, 16.0, 18.0, 19.0].iter().enumerate() {
        let x = -26.0 + i as f32 * 3.0;
        b.bx("sandstone", "Step", rect(x - 1.0, x + 1.0, -14.0, -10.0), 0.0, units(*u))?;
        let note = if *u <= 18.0 { "заходим шагом" } else { "уже только прыжком" };
        zone(b, &format!("Ступень {u:.0} ед ({:.2} м) — {note}", units(*u)), Vec3::new(x, 1.0, -9.0), Vec3::new(2.0, 3.0, 2.0))?;
    }
    // Ящики: прыжок (57 ед) и прыжок с приседом (75 ед).
    b.root_group("Jump Boxes");
    for (i, u) in [40.0f32, 56.0, 58.0, 64.0, 74.0, 76.0].iter().enumerate() {
        let x = -26.0 + i as f32 * 3.0;
        let material = if *u <= 56.0 { "wood" } else if *u <= 74.0 { "wood_dark" } else { "metal" };
        b.bx(material, "Box", rect(x - 1.0, x + 1.0, 10.0, 12.0), 0.0, units(*u))?;
        let note = if *u <= 56.0 {
            "запрыгиваем"
        } else if *u <= 74.0 {
            "только с приседом в прыжке"
        } else {
            "не достать"
        };
        zone(b, &format!("Ящик {u:.0} ед ({:.2} м) — {note}", units(*u)), Vec3::new(x, 1.0, 8.5), Vec3::new(2.0, 3.0, 2.5))?;
    }

    // Склоны вдоль −Z: верх на высоте 2 м.
    b.root_group("Slopes");
    for (i, deg) in [30.0f32, 45.0, 46.0, 60.0].iter().enumerate() {
        let x = 2.0 + i as f32 * 4.0;
        let run = 2.0 / deg.to_radians().tan();
        b.ramp("sandstone", "Slope", true, x - 1.5, x + 1.5, -6.0, 0.0, -6.0 - run, 2.0, 1.0)?;
        b.bx("sandstone", "Slope Top", rect(x - 1.5, x + 1.5, -6.0 - run - 2.0, -6.0 - run), 0.0, 2.0)?;
        let note = if *deg <= 45.0 { "ходим" } else { "скользим (нормаль < 0.7)" };
        zone(b, &format!("Склон {deg:.0}° — {note}"), Vec3::new(x, 1.0, -4.5), Vec3::new(3.0, 3.0, 2.0))?;
    }
    // Длинная крутая рампа для сёрфа: грань к −X, 60°, 30 м.
    let a = 60f32.to_radians();
    let rot = Quat::from_rotation_z(a);
    b.spawn("blocks/sandstone", "Surf Ramp", Vec3::new(24.0, 2.6, 0.0), rot, Vec3::new(6.0, 0.6, 30.0))?;
    zone(b, "Рампа 60° — сёрф: стрейф в сторону рампы держит на ней", Vec3::new(20.0, 2.0, 0.0), Vec3::new(4.0, 5.0, 30.0))?;

    // Лаз: потолок на 56 ед (только в приседе, корпус 54 ед).
    b.root_group("Crawl");
    b.bx("plaster", "Crawl Side", rect(-8.0, -7.6, -2.0, 4.0), 0.0, 1.6)?;
    b.bx("plaster", "Crawl Side", rect(-5.4, -5.0, -2.0, 4.0), 0.0, 1.6)?;
    b.bx("plaster", "Crawl Roof", rect(-8.0, -5.0, -2.0, 4.0), units(56.0), 1.6)?;
    zone(b, "Лаз 56 ед (1.07 м) — только в приседе", Vec3::new(-6.5, 0.5, 1.0), Vec3::new(2.2, 1.0, 6.0))?;
    // Дверной проём 2.2 × 1.3 м и коридоры 2.5 и 4 м.
    b.bx("plaster_rose", "Door Wall", rect(-20.0, -17.65, 1.0, 1.5), 0.0, 3.4)?;
    b.bx("plaster_rose", "Door Wall", rect(-16.35, -14.0, 1.0, 1.5), 0.0, 3.4)?;
    b.bx("plaster_rose", "Door Lintel", rect(-17.65, -16.35, 1.0, 1.5), 2.2, 3.4)?;
    zone(b, "Дверь 2.2 × 1.3 м", Vec3::new(-17.0, 1.0, 1.25), Vec3::new(1.3, 2.2, 2.0))?;
    b.bx("plaster", "Corridor Wall", rect(-27.0, -26.5, -4.0, 4.0), 0.0, 3.0)?;
    b.bx("plaster", "Corridor Wall", rect(-24.0, -23.5, -4.0, 4.0), 0.0, 3.0)?;
    b.bx("plaster", "Corridor Wall", rect(-19.5, -19.0, -6.0, -2.0), 0.0, 3.0)?;
    zone(b, "Коридор 2.5 м", Vec3::new(-25.25, 1.0, 0.0), Vec3::new(2.5, 3.0, 8.0))?;

    // Щели между тумбами 1 м: 3 м — обычный прыжок с разбега, 4.4 м — нужен стрейф.
    b.root_group("Gaps");
    b.stairs("sandstone", "Gap Stairs", true, 4.0, 6.0, 17.0, 0.0, 15.0, 1.0, 4, 0.0)?;
    b.bx("sandstone_dark", "Ledge", rect(4.0, 6.0, 6.0, 15.0), 0.0, 1.0)?;
    b.bx("sandstone_dark", "Ledge", rect(4.0, 6.0, -1.0, 3.0), 0.0, 1.0)?;
    b.bx("sandstone_dark", "Ledge", rect(4.0, 6.0, -10.4 + 3.0 - 6.0, -1.0 - 4.4), 0.0, 1.0)?;
    zone(b, "Щель 3 м — прыжок с разбега", Vec3::new(5.0, 2.0, 4.5), Vec3::new(2.0, 2.0, 3.0))?;
    zone(b, "Щель 4.4 м — прыжок со стрейфом", Vec3::new(5.0, 2.0, -3.2), Vec3::new(2.0, 2.0, 4.4))?;

    // Подвижная платформа: везёт игрока.
    b.root_group("Lift");
    b.bx("sandstone", "Lift Stand", rect(12.0, 15.0, 14.0, 17.0), 0.0, 0.9)?;
    let platform = b.spawn("blocks/wood", "Moving Platform", Vec3::new(13.5, 0.75, 12.5), Quat::IDENTITY, Vec3::new(3.0, 0.3, 3.0))?;
    if let Some(object) = b.world.scene.get_mut(platform) {
        object.add(Rigidbody::kinematic());
        object.add(MovingPlatform { travel: Vec3::new(0.0, 1.5, -12.0), period: 5.0, ..MovingPlatform::default() });
    }
    zone(b, "Подвижная платформа — стоим, и она везёт", Vec3::new(13.5, 2.0, 5.0), Vec3::new(3.0, 4.0, 16.0))?;
    b.prop("crate_small", 20.0, 0.0, 18.0, 0.0)?;
    b.prop("crate_big", 22.0, 0.0, 18.0, 0.0)?;
    b.prop("barrel", 24.0, 0.0, 18.0, 0.0)?;
    zone(b, "Ящики карты: 0.9 м (прыжок) и 1.25 м (с приседом)", Vec3::new(22.0, 1.0, 18.0), Vec3::new(7.0, 3.0, 3.0))?;
    Ok(())
}
