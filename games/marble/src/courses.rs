//! Восемь цехов. Каждый знакомит с одним механизмом и дальше смешивает его с прежними.
//! Цех — последовательность вызовов `Kit`; секции (`belt_run`, `elevator`, …) используются
//! в нескольких цехах, поэтому сложные цеха строятся из уже знакомых игроку кусков.

use engine::prelude::*;

use crate::kit::{Kit, Lane, Mat, Seg, Track, WpKind, THICK};

/// Описание цеха для меню и правил.
#[derive(Clone, Copy, Debug)]
pub struct CourseInfo {
    pub name: &'static str,
    /// Что нового в этом цехе.
    pub blurb: &'static str,
    /// Пороги медалей, секунды: уложился — получил.
    pub gold: f32,
    pub silver: f32,
    pub bronze: f32,
    /// Смена закончилась: время вышло — цех провален.
    pub limit: f32,
}

pub const COUNT: usize = 8;

pub const COURSES: [CourseInfo; COUNT] = [
    CourseInfo { name: "Первая смена", blurb: "Катиться, прыгать, ездить на платформе", gold: 60.0, silver: 80.0, bronze: 115.0, limit: 300.0 },
    CourseInfo { name: "Конвейерная", blurb: "Ленты: по ходу, против хода и поперёк", gold: 65.0, silver: 90.0, bronze: 130.0, limit: 300.0 },
    CourseInfo { name: "Метлы", blurb: "Вращающиеся и качающиеся перекладины", gold: 85.0, silver: 120.0, bronze: 170.0, limit: 360.0 },
    CourseInfo { name: "Вентиляция", blurb: "Вентиляторы: лифты, боковой ветер, прыжок на струе", gold: 60.0, silver: 80.0, bronze: 120.0, limit: 300.0 },
    CourseInfo { name: "Прессы", blurb: "Поршни с ритмом — проскочите между ударами", gold: 70.0, silver: 95.0, bronze: 135.0, limit: 330.0 },
    CourseInfo { name: "Хрупкий мост", blurb: "Плиты, что рассыпаются, качели и тяжёлые ящики", gold: 70.0, silver: 95.0, bronze: 135.0, limit: 330.0 },
    CourseInfo { name: "Ускорение", blurb: "Разгонные плиты, бамперы и длинные прыжки", gold: 65.0, silver: 90.0, bronze: 125.0, limit: 300.0 },
    CourseInfo { name: "Главный цех", blurb: "Всё сразу. Последняя смена", gold: 105.0, silver: 145.0, bronze: 205.0, limit: 460.0 },
];

/// Что получилось после постройки цеха.
#[derive(Clone, Debug)]
pub struct Built {
    /// Где стоит шар в начале.
    pub start: Vec3,
    /// Куда смотрит камера в начале, градусы.
    pub yaw_deg: f32,
    pub gears: u32,
    pub golden: bool,
    pub checkpoints: u32,
    pub bodies: u32,
    /// Ниже этой высоты — падение.
    pub kill_y: f32,
    /// Маршрут для бота в тестах.
    pub route: Vec<crate::kit::Waypoint>,
}

// Сигнальные цвета ламп (HDR: больше 1 — светятся).
const ORANGE: Color = Color::rgb(3.0, 1.2, 0.2);
const CYAN: Color = Color::rgb(0.3, 1.7, 3.0);
const OPEN: Lane = Lane::OPEN;
const RAILED: Lane = Lane::RAILED;

/// Построить цех `index` (0..8).
pub fn build(k: &mut Kit, index: usize) -> Built {
    let start = Vec3::new(0.0, 0.6, -3.0);
    let yaw_deg = 0.0;
    let low = match index {
        0 => course_1(k),
        1 => course_2(k),
        2 => course_3(k),
        3 => course_4(k),
        4 => course_5(k),
        5 => course_6(k),
        6 => course_7(k),
        _ => course_8(k),
    };
    let kill_y = k.min.y.min(low) - 14.0;
    decor(k, kill_y);
    Built {
        start,
        yaw_deg,
        gears: k.gears,
        golden: k.golden,
        checkpoints: k.checkpoints,
        bodies: k.bodies,
        kill_y,
        route: k.route.clone(),
    }
}

// ----------------------------------------------------------------------
// Общие куски
// ----------------------------------------------------------------------

/// Точка маршрута перед щелью с платформой: бот ждёт у края (3 м до него), пока платформа
/// не подъедет. Автоматическая точка конца участка заменяется ею.
fn wait_platform(k: &mut Kit, t: &Track, top: Vec3) {
    k.route.pop();
    k.wp(t.at(-3.0, 0.0, 0.0), WpKind::WaitMover(top));
}

/// Ряд шестерёнок вдоль участка.
fn gears_row(k: &mut Kit, seg: &Seg, from: f32, count: u32, step: f32, lat: f32) {
    for i in 0..count {
        k.gear(seg.pt(from + step * i as f32, lat, 0.95), false);
    }
}

/// Стартовая площадка с лампами; `t` остаётся на её краю.
fn start_pad(k: &mut Kit, t: &mut Track) -> Seg {
    let seg = k.straight(t, 12.0, 8.0, RAILED);
    for (i, side) in [-3.6f32, 3.6].iter().enumerate() {
        k.lamp_post(seg.pt(1.0, *side, 0.0), ORANGE, i as f32 * 0.5);
    }
    // Светящаяся линия старта на полу.
    let root = k.root;
    k.visual("Start Line", root, seg.pt(2.0, 0.0, 0.02), Quat::from_rotation_y(seg.yaw), Vec3::new(6.5, 0.03, 0.35), MeshShape::Cube, Mat::Glow(ORANGE));
    seg
}

/// Контрольные ворота сразу за текущей точкой.
fn checkpoint_here(k: &mut Kit, t: &Track, width: f32) {
    k.checkpoint(t.at(1.2, 0.0, 0.0), t.yaw, width);
}

/// Лента вдоль пути.
fn belt_run(k: &mut Kit, t: &mut Track, len: f32, width: f32, speed: f32) {
    k.conveyor(t.at(len * 0.5, 0.0, 0.0), t.yaw, len, width, speed);
    t.pos += t.fwd() * len;
    k.wp(t.pos, WpKind::Go);
}

/// Лента поперёк дорожки толкает шар вправо/влево; `depth` — ширина полосы вдоль пути.
fn cross_belt(k: &mut Kit, t: &mut Track, lane_w: f32, depth: f32, push_right: bool, speed: f32) {
    let yaw = if push_right { t.yaw - std::f32::consts::FRAC_PI_2 } else { t.yaw + std::f32::consts::FRAC_PI_2 };
    k.conveyor(t.at(depth * 0.5, 0.0, 0.0), yaw, lane_w, depth, speed);
    t.pos += t.fwd() * depth;
    k.wp(t.pos, WpKind::Go);
}

/// Подъём на уступ высотой `rise` вентилятором-лифтом. Дорожка продолжается на уступе.
fn elevator(k: &mut Kit, t: &mut Track, w: f32, rise: f32) {
    let seg = k.straight(t, 10.0, w, OPEN);
    k.fan(seg.pt(7.8, 0.0, 0.0), Quat::IDENTITY, rise + 1.6, 1.8, 26.0);
    let wall = t.pos + t.fwd() * 0.5 + Vec3::Y * ((rise - THICK) * 0.5);
    k.block("Wall", wall, Vec3::new(w, rise + THICK, 1.0), Quat::from_rotation_y(t.yaw), Mat::PlateDark);
    // Стрелки на полу и лампы показывают, куда ехать.
    k.lamp(seg.pt(5.6, -1.8, 0.4), CYAN, 1.2, 0.0);
    k.lamp(seg.pt(5.6, 1.8, 0.4), CYAN, 1.2, 0.5);
    t.pos += t.fwd() * 1.0 + Vec3::Y * rise;
    k.wp(t.pos + t.fwd() * 1.0, WpKind::Go);
}

/// Зал с метлами: `sweepers` — (расстояние, смещение, длина, перекладин, скорость, качание, фаза).
fn sweeper_hall(k: &mut Kit, t: &mut Track, len: f32, w: f32, lane: Lane, sweepers: &[(f32, f32, f32, u32, f32, f32, f32)]) -> Seg {
    let seg = k.straight(t, len, w, lane);
    for &(d, lat, arm, bars, speed, swing, phase) in sweepers {
        k.sweeper(seg.pt(d, lat, 0.0), arm, bars, speed, swing, phase);
    }
    seg
}

/// Ряд прессов поперёк дорожки; `period` общий, фазы сдвинуты на `phase_step`.
fn press_gauntlet(k: &mut Kit, t: &mut Track, w: f32, count: u32, spacing: f32, period: f32, phase_step: f32) {
    let len = spacing * count as f32 + 3.0;
    let seg = k.straight(t, len, w, RAILED);
    for i in 0..count {
        let head = Vec3::new(w - 0.8, 1.0, 2.2);
        let up = seg.pt(2.5 + spacing * i as f32, 0.0, 3.3 + 0.05 + 0.5);
        let id = k.crusher(up, head, 3.3, period, period * 0.42, phase_step * i as f32);
        if let Some(object) = k.world.scene.get_mut(id) {
            object.transform.rotation = Quat::from_rotation_y(seg.yaw);
        }
    }
}

/// Мост из хрупких плит: `n` плит со смещением по бокам (шахматка).
fn tile_bridge(k: &mut Kit, t: &mut Track, n: u32, size: f32, zigzag: f32) {
    let gap = 0.55;
    for i in 0..n {
        let lat = if zigzag == 0.0 { 0.0 } else if i % 2 == 0 { -zigzag } else { zigzag };
        let center = t.at(size * 0.5, lat, 0.0);
        k.tile(center, t.yaw, size, size);
        // Маршрут бота идёт по центрам плит.
        k.wp(center + Vec3::Y * 0.3, WpKind::Go);
        t.pos += t.fwd() * (size + gap);
    }
    // Разрыв после последней плиты уже учтён; дорожка продолжается с зазором.
    t.pos -= t.fwd() * gap;
}

/// Ускоряющая плита и рампа-трамплин; после неё — щель `gap` с падением `drop`.
fn boost_jump(k: &mut Kit, t: &mut Track, w: f32, gap: f32, drop: f32, speed: f32) {
    let run = k.straight(t, 14.0, w, RAILED);
    k.booster(run.pt(6.0, 0.0, 0.0), run.yaw, w - 1.0, 4.0, speed);
    k.ramp(t, 6.0, 1.6, w, RAILED);
    k.gap(t, gap, -drop);
}

/// Стальная посадочная площадка.
fn landing(k: &mut Kit, t: &mut Track, len: f32, w: f32) -> Seg {
    k.straight(t, len, w, OPEN)
}

/// Финишная площадка в конце цеха.
fn finish_run(k: &mut Kit, t: &mut Track) {
    let seg = k.straight(t, 12.0, 8.0, RAILED);
    k.finish(seg.pt(8.0, 0.0, 0.0), t.yaw, 6.0);
    for side in [-3.6f32, 3.6] {
        k.lamp_post(seg.pt(2.0, side, 0.0), Color::rgb(0.3, 2.0, 0.8), 0.0);
    }
    k.wp(seg.pt(8.0, 0.0, 0.0), WpKind::Go);
}

// ----------------------------------------------------------------------
// Витрина главного меню: кусочек завода, вокруг которого кружит камера
// ----------------------------------------------------------------------

/// Построить витрину меню; возвращает точку, вокруг которой кружит камера, и
/// расстояние, с которого она видна целиком.
pub fn build_showcase(k: &mut Kit) -> (Vec3, f32) {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    let b = k.straight(&mut t, 16.0, 8.0, OPEN);
    k.sweeper(b.pt(8.0, 0.0, 0.0), 7.4, 1, 44.0, 0.0, 0.0);
    k.bumper(b.pt(3.0, -2.6, 0.0), 0.8);
    k.bumper(b.pt(3.0, 2.6, 0.0), 0.8);
    gears_row(k, &b, 3.0, 2, 10.0, 3.4);
    k.corner(&mut t, 8.0, -90.0, RAILED);
    let c = k.straight(&mut t, 14.0, 6.0, RAILED);
    k.checkpoint(c.pt(3.0, 0.0, 0.0), c.yaw, 5.0);
    k.fan(c.pt(9.0, 0.0, 0.0), Quat::IDENTITY, 5.5, 1.5, 26.0);
    k.gear(c.pt(9.0, 0.0, 3.4), true);
    k.gap(&mut t, 4.0, 0.0);
    finish_run(k, &mut t);
    // Шар, который ждёт на старте (без тега «Ball»: механизмы его не замечают).
    let root = k.root;
    let ball = k.node("Showcase Ball", root, Vec3::new(0.0, 0.6, -3.0), Quat::IDENTITY, Vec3::splat(1.0));
    k.add(ball, MeshRenderer::sphere().with_color(Color::hex(0xdfe6f2)).with_material(1.0, 0.22));
    k.add(ball, Rigidbody::dynamic());
    k.add(ball, Collider::sphere(0.5).with_material(0.8, 0.1).with_density(4.0));
    let kill_y = k.min.y - 14.0;
    decor(k, kill_y);
    let center = (k.min + k.max) * 0.5;
    (Vec3::new(center.x, 0.0, center.z), (k.max - k.min).max_element() * 0.62 + 6.0)
}

// ----------------------------------------------------------------------
// Цех 1. Первая смена
// ----------------------------------------------------------------------

fn course_1(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 4, 1.8, 0.0);
    let r = k.ramp(&mut t, 14.0, -3.0, 6.0, RAILED);
    gears_row(k, &r, 3.0, 4, 2.5, 0.0);
    let b = k.straight(&mut t, 12.0, 6.0, RAILED);
    gears_row(k, &b, 2.0, 3, 3.0, 1.2);
    checkpoint_here(k, &t, 5.0);
    k.corner(&mut t, 6.0, 90.0, RAILED);
    let c = k.straight(&mut t, 18.0, 6.0, RAILED);
    gears_row(k, &c, 3.0, 4, 2.6, -1.0);
    // Уступ справа за бортиком — золотая шестерня: на неё надо запрыгнуть.
    let ledge_rot = Quat::from_rotation_y(c.yaw);
    k.block("Ledge", c.pt(10.0, 5.4, 1.1) - Vec3::Y * (THICK * 0.5), Vec3::new(3.2, THICK, 3.2), ledge_rot, Mat::PlateDark);
    k.block("Ledge Step", c.pt(7.0, 5.0, 0.4) - Vec3::Y * (THICK * 0.5), Vec3::new(2.4, THICK, 2.4), ledge_rot, Mat::PlateDark);
    k.gear(c.pt(10.0, 5.4, 2.1), true);
    k.gap(&mut t, 3.5, 0.0);
    let d = k.straight(&mut t, 14.0, 6.0, OPEN);
    checkpoint_here(k, &Track { pos: d.start, yaw: t.yaw }, 5.0);
    k.bumper(d.pt(7.0, -1.9, 0.0), 0.7);
    k.bumper(d.pt(7.0, 1.9, 0.0), 0.7);
    gears_row(k, &d, 4.0, 3, 3.5, 0.0);
    k.corner(&mut t, 6.0, -90.0, OPEN);
    let e = k.straight(&mut t, 10.0, 6.0, RAILED);
    gears_row(k, &e, 3.0, 3, 2.5, 0.0);
    // Платформа ездит через щель.
    let gap_len = 11.0;
    let platform_top = t.at(2.0, 0.0, 0.0);
    let fwd = t.fwd();
    k.platform(platform_top, t.yaw, Vec3::new(5.2, 0.5, 4.0), fwd * (gap_len - 4.0), 6.5, 1.2, 0.0);
    wait_platform(k, &t, platform_top);
    k.gap(&mut t, gap_len, 0.0);
    let f = k.straight(&mut t, 12.0, 6.0, RAILED);
    gears_row(k, &f, 3.0, 3, 3.0, 0.0);
    checkpoint_here(k, &Track { pos: f.start, yaw: t.yaw }, 5.0);
    let g = k.ramp(&mut t, 14.0, 3.0, 6.0, RAILED);
    gears_row(k, &g, 4.0, 3, 3.5, 0.0);
    finish_run(k, &mut t);
    -3.0
}

// ----------------------------------------------------------------------
// Цех 2. Конвейерная
// ----------------------------------------------------------------------

fn course_2(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    // Лента по ходу: разгоняет.
    let begin = t.pos;
    belt_run(k, &mut t, 18.0, 5.0, 4.5);
    for i in 0..4 {
        k.gear(begin + Vec3::new(0.0, 0.95, -4.0 - 3.5 * i as f32), false);
    }
    let b = k.straight(&mut t, 6.0, 6.0, RAILED);
    // Золотая на полке сбоку от ленты против хода.
    let shelf_rot = Quat::from_rotation_y(b.yaw);
    k.block("Shelf", b.pt(8.0, 4.6, 0.0) - Vec3::Y * (THICK * 0.5), Vec3::new(2.6, THICK, 7.0), shelf_rot, Mat::PlateDark);
    k.gear(b.pt(8.0, 4.6, 0.95), true);
    // Лента против хода: тормозит и требует разгона.
    let before = t.pos;
    belt_run(k, &mut t, 16.0, 5.0, -3.5);
    for i in 0..3 {
        k.gear(before + Vec3::new(1.1, 0.95, -4.0 - 4.0 * i as f32), false);
    }
    let c = k.straight(&mut t, 8.0, 6.0, RAILED);
    checkpoint_here(k, &Track { pos: c.start, yaw: t.yaw }, 5.0);
    // Поперечные ленты сносят к краю, где нет бортика.
    k.straight(&mut t, 5.0, 6.0, OPEN);
    cross_belt(k, &mut t, 6.0, 4.0, true, 3.2);
    let d = k.straight(&mut t, 4.0, 6.0, OPEN);
    gears_row(k, &d, 1.0, 2, 1.6, 0.0);
    cross_belt(k, &mut t, 6.0, 4.0, false, 3.2);
    k.straight(&mut t, 4.0, 6.0, OPEN);
    cross_belt(k, &mut t, 6.0, 4.0, true, 3.8);
    let e = k.straight(&mut t, 6.0, 6.0, OPEN);
    checkpoint_here(k, &Track { pos: e.start, yaw: t.yaw }, 5.0);
    k.corner(&mut t, 6.0, 90.0, RAILED);
    // Быстрая лента с щелью и платформой.
    let before = t.pos;
    belt_run(k, &mut t, 14.0, 5.0, 6.0);
    for i in 0..3 {
        k.gear(before + t.fwd() * (3.0 + 3.5 * i as f32) + Vec3::Y * 0.95, false);
    }
    k.gap(&mut t, 3.5, 0.0);
    let f = k.straight(&mut t, 8.0, 6.0, RAILED);
    gears_row(k, &f, 2.0, 2, 3.0, 0.0);
    k.corner(&mut t, 6.0, -90.0, RAILED);
    // Платформа везёт через щель, дальше лента против хода.
    let gap_len = 12.0;
    let fwd = t.fwd();
    k.straight(&mut t, 4.0, 6.0, RAILED);
    let top = t.at(2.0, 0.0, 0.0);
    k.platform(top, t.yaw, Vec3::new(5.2, 0.5, 4.0), fwd * (gap_len - 4.0), 7.0, 1.2, 0.0);
    wait_platform(k, &t, top);
    k.gap(&mut t, gap_len, 0.0);
    checkpoint_here(k, &t, 5.0);
    belt_run(k, &mut t, 12.0, 5.0, -3.0);
    let g = k.straight(&mut t, 6.0, 6.0, RAILED);
    gears_row(k, &g, 1.5, 2, 3.0, 0.0);
    belt_run(k, &mut t, 14.0, 5.0, 5.0);
    finish_run(k, &mut t);
    0.0
}

// ----------------------------------------------------------------------
// Цех 3. Метлы
// ----------------------------------------------------------------------

fn course_3(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    k.ramp(&mut t, 8.0, -2.0, 8.0, RAILED);
    // Зал 1: одна метла посередине.
    let h1 = sweeper_hall(k, &mut t, 24.0, 10.0, OPEN, &[(12.0, 0.0, 8.4, 1, 55.0, 0.0, 0.0)]);
    gears_row(k, &h1, 3.0, 2, 2.5, 3.6);
    gears_row(k, &h1, 16.0, 2, 2.5, -3.6);
    checkpoint_here(k, &t, 8.0);
    k.straight(&mut t, 6.0, 6.0, RAILED);
    // Зал 2: две метлы со сдвигом по фазе, вращаются в разные стороны.
    let h2 = sweeper_hall(
        k,
        &mut t,
        32.0,
        10.0,
        OPEN,
        &[(9.0, -1.2, 7.6, 1, 62.0, 0.0, 0.0), (22.0, 1.2, 7.6, 1, -62.0, 0.0, 90.0)],
    );
    gears_row(k, &h2, 3.0, 3, 1.5, 4.0);
    k.gear(h2.pt(16.0, -4.4, 0.95), false);
    checkpoint_here(k, &t, 8.0);
    k.straight(&mut t, 6.0, 6.0, RAILED);
    // Мост: узкий; качающиеся перекладины закреплены по краям и перекрывают его по очереди.
    let bridge = sweeper_hall(k, &mut t, 28.0, 3.6, OPEN, &[(9.0, 1.8, 5.4, 1, 60.0, 85.0, 0.0), (19.0, -1.8, 5.4, 1, 60.0, 85.0, 0.0)]);
    gears_row(k, &bridge, 4.0, 3, 6.0, 0.0);
    checkpoint_here(k, &t, 5.0);
    // Золотая: площадка сбоку от зала 3, на которую заводит короткая рампа.
    let h3 = sweeper_hall(k, &mut t, 22.0, 12.0, OPEN, &[(11.0, 0.0, 6.5, 2, 48.0, 0.0, 0.0)]);
    k.block("Nook", h3.pt(18.0, 7.6, 0.0) - Vec3::Y * (THICK * 0.5), Vec3::new(3.2, THICK, 3.2), Quat::from_rotation_y(h3.yaw), Mat::PlateDark);
    k.gear(h3.pt(18.0, 7.6, 0.95), true);
    gears_row(k, &h3, 2.0, 2, 2.0, -4.0);
    k.corner(&mut t, 8.0, 90.0, RAILED);
    // Три метлы подряд на узкой дорожке.
    let h4 = sweeper_hall(
        k,
        &mut t,
        40.0,
        6.0,
        RAILED,
        &[(8.0, 0.0, 5.0, 1, 70.0, 0.0, 0.0), (20.0, 0.0, 5.0, 1, -70.0, 0.0, 45.0), (32.0, 0.0, 5.0, 2, 55.0, 0.0, 0.0)],
    );
    gears_row(k, &h4, 14.0, 3, 1.6, 0.0);
    checkpoint_here(k, &Track { pos: h4.start + h4.fwd() * 2.0, yaw: t.yaw }, 5.0);
    k.ramp(&mut t, 10.0, 2.0, 6.0, RAILED);
    finish_run(k, &mut t);
    -2.0
}

// ----------------------------------------------------------------------
// Цех 4. Вентиляция
// ----------------------------------------------------------------------

fn course_4(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    // Лифт на уступ.
    elevator(k, &mut t, 8.0, 4.5);
    let b = k.straight(&mut t, 10.0, 6.0, RAILED);
    gears_row(k, &b, 2.0, 3, 2.5, 0.0);
    checkpoint_here(k, &t, 5.0);
    // Боковой ветер на узком мосту.
    let wind = |k: &mut Kit, seg: &Seg, d: f32, from_right: bool, lift: f32| {
        let side = if from_right { 4.4 } else { -4.4 };
        let tilt = if from_right { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
        let rot = Quat::from_rotation_y(seg.yaw) * Quat::from_rotation_z(tilt);
        k.fan(seg.pt(d, side, 1.0), rot, 7.4, 1.5, lift);
    };
    let bridge = k.straight(&mut t, 30.0, 3.4, OPEN);
    wind(k, &bridge, 7.0, false, 13.0);
    wind(k, &bridge, 15.0, true, 13.0);
    wind(k, &bridge, 23.0, false, 13.0);
    gears_row(k, &bridge, 4.0, 4, 6.0, 0.0);
    k.straight(&mut t, 6.0, 6.0, RAILED);
    checkpoint_here(k, &t, 5.0);
    // Лифт номер два, выше.
    elevator(k, &mut t, 8.0, 5.0);
    let c = k.straight(&mut t, 8.0, 6.0, OPEN);
    // Золотая на выступе сбоку: на него можно взлететь только на отдельной струе.
    let side_rot = Quat::from_rotation_y(c.yaw);
    k.block("Fan Pad", c.pt(4.0, 4.7, 0.0) - Vec3::Y * (THICK * 0.5), Vec3::new(3.4, THICK, 5.0), side_rot, Mat::Plate);
    k.fan(c.pt(4.0, 4.8, 0.0), Quat::IDENTITY, 6.0, 1.5, 26.0);
    k.block("Perch", c.pt(4.0, 8.6, 3.8) - Vec3::Y * (THICK * 0.5), Vec3::new(3.2, THICK, 3.2), side_rot, Mat::PlateDark);
    k.gear(c.pt(4.0, 8.6, 4.75), true);
    // Прыжок со струёй: щель, посадка ниже.
    let launch = k.straight(&mut t, 8.0, 6.0, OPEN);
    k.fan(launch.pt(6.0, 0.0, 0.0), Quat::IDENTITY, 5.5, 1.8, 26.0);
    k.gap(&mut t, 8.0, -4.0);
    let d = landing(k, &mut t, 12.0, 8.0);
    gears_row(k, &d, 3.0, 3, 2.5, 0.0);
    checkpoint_here(k, &t, 6.0);
    k.corner(&mut t, 8.0, 90.0, RAILED);
    // Ветер вдоль дорожки + лифт.
    let e = k.straight(&mut t, 16.0, 5.0, RAILED);
    gears_row(k, &e, 3.0, 4, 3.0, 0.0);
    elevator(k, &mut t, 6.0, 4.0);
    k.ramp(&mut t, 14.0, -5.0, 6.0, RAILED);
    finish_run(k, &mut t);
    -9.0
}

// ----------------------------------------------------------------------
// Цех 5. Прессы
// ----------------------------------------------------------------------

fn course_5(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    k.ramp(&mut t, 10.0, -2.0, 6.0, RAILED);
    // Первый ряд: четыре пресса «волной».
    press_gauntlet(k, &mut t, 5.0, 4, 6.0, 3.6, 0.25);
    gears_row(k, &Seg { start: t.pos - t.fwd() * 26.0, yaw: t.yaw, len: 26.0, width: 5.0, slope: 0.0 }, 5.0, 3, 6.0, 0.0);
    checkpoint_here(k, &t, 4.5);
    k.straight(&mut t, 6.0, 6.0, RAILED);
    // Пресс над лентой: лента тащит под удар.
    let belt_start = t.pos;
    belt_run(k, &mut t, 22.0, 5.0, 3.5);
    for i in 0..2 {
        let up = belt_start + t.fwd() * (7.0 + 8.0 * i as f32) + Vec3::Y * (3.3 + 0.05 + 0.5);
        let id = k.crusher(up, Vec3::new(4.2, 1.0, 2.2), 3.3, 3.2, 1.3, 0.25 * i as f32);
        if let Some(o) = k.world.scene.get_mut(id) {
            o.transform.rotation = Quat::from_rotation_y(t.yaw);
        }
    }
    let b = k.straight(&mut t, 8.0, 6.0, RAILED);
    gears_row(k, &b, 2.0, 3, 2.5, 0.0);
    checkpoint_here(k, &t, 4.5);
    k.corner(&mut t, 6.0, 90.0, RAILED);
    // Широкий зал: левый и правый прессы бьют по очереди, надо выбирать сторону.
    let hall = k.straight(&mut t, 30.0, 10.0, RAILED);
    for i in 0..3 {
        for (j, lat) in [-2.5f32, 2.5].iter().enumerate() {
            let up = hall.pt(5.0 + 9.0 * i as f32, *lat, 3.3 + 0.55);
            let id = k.crusher(up, Vec3::new(4.2, 1.0, 3.0), 3.3, 3.6, 1.5, 0.5 * j as f32 + 0.17 * i as f32);
            if let Some(o) = k.world.scene.get_mut(id) {
                o.transform.rotation = Quat::from_rotation_y(hall.yaw);
            }
        }
    }
    gears_row(k, &hall, 5.0, 3, 9.0, 0.0);
    k.gear(hall.pt(14.0, 4.6, 0.95), true);
    checkpoint_here(k, &t, 8.0);
    k.corner(&mut t, 6.0, -90.0, RAILED);
    // Финал: прессы + метла.
    let fin = k.straight(&mut t, 30.0, 5.0, RAILED);
    k.sweeper(fin.pt(8.0, 0.0, 0.0), 4.4, 1, 70.0, 0.0, 0.0);
    for i in 0..2 {
        let up = fin.pt(18.0 + 6.0 * i as f32, 0.0, 3.3 + 0.55);
        let id = k.crusher(up, Vec3::new(4.2, 1.0, 2.2), 3.3, 3.0, 1.2, 0.5 * i as f32);
        if let Some(o) = k.world.scene.get_mut(id) {
            o.transform.rotation = Quat::from_rotation_y(fin.yaw);
        }
    }
    gears_row(k, &fin, 12.0, 2, 3.0, 0.0);
    finish_run(k, &mut t);
    -2.0
}

// ----------------------------------------------------------------------
// Цех 6. Хрупкий мост
// ----------------------------------------------------------------------

fn course_6(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    // Мост из плит, прямой.
    tile_bridge(k, &mut t, 6, 2.6, 0.0);
    k.gap(&mut t, 0.6, 0.0);
    let b = k.straight(&mut t, 8.0, 6.0, RAILED);
    gears_row(k, &b, 2.0, 3, 2.5, 0.0);
    checkpoint_here(k, &t, 5.0);
    // Качели: вход и выход на высоте нижнего конца доски.
    let len = 8.0;
    let drop = (len * 0.5) * 0.25;
    let pivot_top = t.pos + t.fwd() * (len * 0.5) + Vec3::Y * (drop + 0.0);
    k.seesaw(pivot_top, t.yaw, len, 3.4);
    t.pos += t.fwd() * len;
    k.wp(t.pos, WpKind::Go);
    let c = k.straight(&mut t, 8.0, 6.0, RAILED);
    gears_row(k, &c, 2.0, 2, 3.0, 0.0);
    checkpoint_here(k, &t, 5.0);
    k.corner(&mut t, 6.0, 90.0, RAILED);
    // Переулок ящиков: слева обрыв, справа бортик; ящики надо столкнуть с края.
    let alley = k.straight(&mut t, 24.0, 4.6, OPEN);
    let rot = Quat::from_rotation_y(alley.yaw);
    k.block("Rail", alley.pt(12.0, 2.15, 0.28), Vec3::new(0.3, 0.56, 24.0), rot, Mat::Hazard);
    for (i, d) in [8.0f32, 13.0, 18.0].iter().enumerate() {
        k.crate_(alley.pt(*d, -0.6 + 0.5 * (i as f32 - 1.0), 0.75), 1.3);
        k.crate_(alley.pt(*d + 0.4, 0.7, 0.75), 1.3);
    }
    gears_row(k, &alley, 4.0, 2, 2.0, 0.0);
    checkpoint_here(k, &t, 4.0);
    k.corner(&mut t, 6.0, -90.0, RAILED);
    // Зигзаг из плит над пропастью.
    tile_bridge(k, &mut t, 8, 2.6, 0.6);
    k.gear(t.at(-6.0, 3.6, 1.0), true);
    k.gap(&mut t, 0.6, 0.0);
    let d = k.straight(&mut t, 8.0, 6.0, RAILED);
    gears_row(k, &d, 2.0, 3, 2.5, 0.0);
    checkpoint_here(k, &t, 5.0);
    // Платформа и качели рядом.
    let gap_len = 10.0;
    let top = t.at(2.0, 0.0, 0.0);
    let fwd = t.fwd();
    k.platform(top, t.yaw, Vec3::new(5.2, 0.5, 4.0), fwd * (gap_len - 4.0), 6.0, 1.0, 0.0);
    wait_platform(k, &t, top);
    k.gap(&mut t, gap_len, 0.0);
    k.straight(&mut t, 6.0, 6.0, RAILED);
    k.ramp(&mut t, 12.0, 2.0, 6.0, RAILED);
    finish_run(k, &mut t);
    -2.0
}

// ----------------------------------------------------------------------
// Цех 7. Ускорение
// ----------------------------------------------------------------------

fn course_7(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    // Первый разгон: прыжок на 12 метров.
    boost_jump(k, &mut t, 6.0, 7.0, 3.0, 14.0);
    let b = landing(k, &mut t, 14.0, 10.0);
    gears_row(k, &b, 3.0, 3, 3.0, 0.0);
    checkpoint_here(k, &t, 8.0);
    // Слалом бамперов по скоростной дорожке.
    let run = k.straight(&mut t, 36.0, 8.0, RAILED);
    for (i, d) in [6.0f32, 12.0, 18.0, 24.0, 30.0].iter().enumerate() {
        let lat = if i % 2 == 0 { -2.2 } else { 2.2 };
        k.bumper(run.pt(*d, lat, 0.0), 0.8);
    }
    k.booster(run.pt(2.0, 0.0, 0.0), run.yaw, 5.0, 4.0, 11.0);
    gears_row(k, &run, 6.0, 4, 6.0, 0.0);
    k.gear(run.pt(15.0, 3.2, 0.95), true);
    checkpoint_here(k, &t, 8.0);
    // Второй трамплин: дальше и ниже.
    k.straight(&mut t, 4.0, 8.0, RAILED);
    boost_jump(k, &mut t, 8.0, 10.0, 4.0, 16.0);
    k.corner(&mut t, 10.0, 90.0, OPEN);
    // Пинбол-арена с бамперами.
    let arena = k.straight(&mut t, 28.0, 14.0, RAILED);
    let spots = [(6.0f32, -3.5f32), (6.0, 3.5), (11.0, 0.0), (15.0, -4.5), (15.0, 4.5), (19.0, -1.5), (19.0, 1.5), (23.0, -4.0), (23.0, 4.0)];
    for (d, lat) in spots {
        k.bumper(arena.pt(d, lat, 0.0), 0.9);
    }
    gears_row(k, &arena, 4.0, 3, 9.0, 0.0);
    checkpoint_here(k, &t, 10.0);
    // Длинный финальный прыжок.
    k.corner(&mut t, 10.0, -90.0, OPEN);
    boost_jump(k, &mut t, 8.0, 12.0, 5.0, 18.0);
    landing(k, &mut t, 12.0, 10.0);
    finish_run(k, &mut t);
    -9.0
}

// ----------------------------------------------------------------------
// Цех 8. Главный цех
// ----------------------------------------------------------------------

fn course_8(k: &mut Kit) -> f32 {
    let mut t = Track::new(Vec3::ZERO);
    let a = start_pad(k, &mut t);
    gears_row(k, &a, 5.0, 3, 2.0, 0.0);
    // 1. Конвейеры и поперечная лента.
    belt_run(k, &mut t, 14.0, 5.0, 4.5);
    let b = k.straight(&mut t, 4.0, 6.0, OPEN);
    gears_row(k, &b, 1.0, 2, 2.0, 0.0);
    cross_belt(k, &mut t, 6.0, 4.0, true, 3.5);
    k.straight(&mut t, 4.0, 6.0, OPEN);
    cross_belt(k, &mut t, 6.0, 4.0, false, 3.5);
    k.straight(&mut t, 5.0, 6.0, RAILED);
    checkpoint_here(k, &t, 5.0);
    // 2. Зал с метлами.
    k.ramp(&mut t, 8.0, -2.0, 8.0, RAILED);
    let h = sweeper_hall(k, &mut t, 28.0, 10.0, OPEN, &[(9.0, -1.0, 7.8, 1, 60.0, 0.0, 0.0), (21.0, 1.0, 7.8, 1, -60.0, 0.0, 90.0)]);
    gears_row(k, &h, 3.0, 3, 8.0, 4.2);
    checkpoint_here(k, &t, 8.0);
    k.straight(&mut t, 4.0, 6.0, RAILED);
    // 3. Лифт и боковой ветер.
    elevator(k, &mut t, 8.0, 4.5);
    let c = k.straight(&mut t, 4.0, 6.0, RAILED);
    k.gear(c.pt(2.0, 3.6, 4.0), true);
    let bridge = k.straight(&mut t, 24.0, 3.4, OPEN);
    for (d, right) in [(6.0f32, false), (13.0, true), (20.0, false)] {
        let side = if right { 4.4 } else { -4.4 };
        let tilt = if right { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
        k.fan(bridge.pt(d, side, 1.0), Quat::from_rotation_y(bridge.yaw) * Quat::from_rotation_z(tilt), 7.4, 1.5, 12.0);
    }
    gears_row(k, &bridge, 4.0, 3, 6.0, 0.0);
    checkpoint_here(k, &t, 5.0);
    k.corner(&mut t, 6.0, 90.0, RAILED);
    // 4. Прессы.
    press_gauntlet(k, &mut t, 5.0, 3, 6.0, 3.4, 0.3);
    checkpoint_here(k, &t, 4.5);
    k.straight(&mut t, 4.0, 6.0, RAILED);
    // 5. Хрупкий мост с зигзагом.
    tile_bridge(k, &mut t, 6, 2.6, 1.0);
    k.gap(&mut t, 0.6, 0.0);
    let d = k.straight(&mut t, 8.0, 6.0, RAILED);
    gears_row(k, &d, 2.0, 3, 2.5, 0.0);
    checkpoint_here(k, &t, 5.0);
    // 6. Платформа.
    let gap_len = 11.0;
    let top = t.at(2.0, 0.0, 0.0);
    let fwd = t.fwd();
    k.platform(top, t.yaw, Vec3::new(5.2, 0.5, 4.0), fwd * (gap_len - 4.0), 6.0, 1.0, 0.0);
    wait_platform(k, &t, top);
    k.gap(&mut t, gap_len, 0.0);
    k.straight(&mut t, 6.0, 6.0, RAILED);
    k.corner(&mut t, 6.0, -90.0, RAILED);
    // 7. Бамперы и разгон на финальный прыжок.
    let run = k.straight(&mut t, 24.0, 8.0, RAILED);
    for (i, dd) in [5.0f32, 10.0, 15.0].iter().enumerate() {
        k.bumper(run.pt(*dd, if i % 2 == 0 { -2.2 } else { 2.2 }, 0.0), 0.8);
    }
    gears_row(k, &run, 4.0, 3, 6.0, 0.0);
    checkpoint_here(k, &t, 6.0);
    boost_jump(k, &mut t, 8.0, 10.0, 4.0, 16.0);
    landing(k, &mut t, 10.0, 10.0);
    finish_run(k, &mut t);
    -7.0
}

// ----------------------------------------------------------------------
// Оформление: облака внизу, колонны, шестерни, трубы с дымом
// ----------------------------------------------------------------------

/// Декорации вокруг цеха; детерминированы (зерно — размеры цеха).
fn decor(k: &mut Kit, kill_y: f32) {
    let root = k.root;
    let center = (k.min + k.max) * 0.5;
    let extent = (k.max - k.min).max(Vec3::splat(40.0));
    let mut rng = Random::new((extent.x * 31.0 + extent.z * 17.0) as u64 + 7);

    // Море облаков: насыщенно-синий слой далеко внизу + белые пухлые облака поверх него —
    // слой читается как «внизу», темнее и синее трассы.
    let sea = k.visual(
        "Cloud Sea",
        root,
        Vec3::new(center.x, kill_y - 11.0, center.z),
        Quat::IDENTITY,
        Vec3::new(1600.0, 1.0, 1600.0),
        MeshShape::Plane,
        Mat::Plate,
    );
    if let Some(m) = k.world.scene.get_mut(sea).and_then(|o| o.get_mut::<MeshRenderer>()) {
        m.cast_shadows = false;
        m.texture = AssetHandle::none();
        m.tiling = 0.0;
        m.metallic = 0.0;
        m.color = Color::hex(0x2c62b8);
        m.roughness = 1.0;
        m.emissive = Color::rgb(0.01, 0.04, 0.12);
    }
    for _ in 0..34 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let radius = rng.range(0.3, 1.0) * (extent.x.max(extent.z) * 0.9 + 70.0);
        let p = Vec3::new(center.x + angle.cos() * radius, kill_y - 10.0 + rng.range(0.0, 3.0), center.z + angle.sin() * radius);
        let size = rng.range(14.0, 38.0);
        let puff = k.visual("Cloud", root, p, Quat::IDENTITY, Vec3::new(size, rng.range(3.0, 6.0), size * rng.range(0.6, 1.2)), MeshShape::Sphere, Mat::Plate);
        if let Some(m) = k.world.scene.get_mut(puff).and_then(|o| o.get_mut::<MeshRenderer>()) {
            m.cast_shadows = false;
            m.texture = AssetHandle::none();
            m.tiling = 0.0;
            m.metallic = 0.0;
            m.roughness = 1.0;
            m.color = Color::hex(0xf4f8ff);
            m.emissive = Color::rgb(0.10, 0.12, 0.16);
        }
    }

    // Колонны, уходящие вниз в облака.
    let columns = 26;
    for i in 0..columns {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let radius = rng.range(0.55, 1.0) * extent.x.max(extent.z) * 0.9 + 18.0;
        let p = Vec3::new(center.x + angle.cos() * radius * rng.range(0.4, 1.0), 0.0, center.z + angle.sin() * radius * rng.range(0.4, 1.0));
        let width = rng.range(3.0, 9.0);
        let top = k.min.y - rng.range(4.0, 14.0);
        let height = top - (kill_y - 8.0);
        k.visual("Pillar", root, Vec3::new(p.x, top - height * 0.5, p.z), Quat::IDENTITY, Vec3::new(width, height, width), MeshShape::Cylinder, Mat::Brushed);
        if i % 4 == 0 {
            // Труба с дымом.
            let smoke = k.node("Smoke", root, Vec3::new(p.x, top + 0.3, p.z), Quat::IDENTITY, Vec3::ONE);
            k.add(smoke, ParticleSystem { emission_rate: 6.0, max_particles: 60, seed: i + 3, ..ParticleSystem::smoke().scaled(width * 0.45) });
        }
    }

    // Гигантские шестерни: диски с зубьями, медленно вращаются (Cog из C++).
    for i in 0..8 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let radius = extent.x.max(extent.z) * 0.75 + rng.range(30.0, 60.0);
        let p = Vec3::new(center.x + angle.cos() * radius, k.min.y + rng.range(-10.0, 18.0), center.z + angle.sin() * radius);
        let size = rng.range(14.0, 30.0);
        let cog = k.node("Cog", root, p, Quat::from_rotation_y(angle) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), Vec3::ONE);
        k.visual("Disc", cog, Vec3::ZERO, Quat::IDENTITY, Vec3::new(size, 1.6, size), MeshShape::Cylinder, Mat::PlateDark);
        k.visual("Rim", cog, Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY, Vec3::new(size * 0.55, 2.2, size * 0.55), MeshShape::Cylinder, Mat::Brushed);
        for t in 0..14 {
            let a = t as f32 * std::f32::consts::TAU / 14.0;
            k.visual("Tooth", cog, Vec3::new(a.cos() * size * 0.5, 0.0, a.sin() * size * 0.5), Quat::from_rotation_y(-a), Vec3::new(size * 0.14, 1.6, size * 0.1), MeshShape::Cube, Mat::PlateDark);
        }
        let speed = rng.range(4.0, 9.0) * if i % 2 == 0 { 1.0 } else { -1.0 };
        k.cpp(cog, "Cog", &[("speed_deg", Value::Float(speed)), ("axis", Value::Int(1))]);
    }
}
