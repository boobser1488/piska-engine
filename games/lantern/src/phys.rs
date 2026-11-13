//! Кинематическая физика платформера: сетка тайлов + односторонние платформы.
//!
//! Сам `Rigidbody2D` для платформера не берём: нужна жёсткая, детерминированная
//! «ощущаемость» (койот, буфер прыжка, подъём на движущейся платформе), поэтому тело —
//! просто прямоугольник, а мир — сетка тайлов, прочитанная из `Tilemap` уровня.
//! Все функции — чистые: их проверяют обычные тесты без окна.

use engine::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Тайл — твёрдая стена.
pub const SOLID: u8 = 1;
/// Тайл — «облачко»: держит сверху, снизу и сбоку пропускает.
pub const ONEWAY: u8 = 2;
/// Тайл — шипы.
pub const SPIKE: u8 = 4;

const EPS: f32 = 1e-3;

/// Что за тайл с таким номером (номера — из `tools/gen_assets.py`: 1–16 скала по маске
/// открытых сторон, 17 «облачко», 18–21 шипы, 22–23 заполнение, дальше — декор).
pub fn classify(tile: u32) -> u8 {
    match tile {
        1..=16 | 22 | 23 => SOLID,
        17 => ONEWAY,
        18..=21 => SPIKE,
        _ => 0,
    }
}

/// Опасная часть тайла шипов (в долях клетки: min, max).
pub fn spike_box(tile: u32) -> (Vec2, Vec2) {
    match tile {
        18 => (Vec2::new(0.12, 0.0), Vec2::new(0.88, 0.5)),  // вверх
        19 => (Vec2::new(0.12, 0.5), Vec2::new(0.88, 1.0)),  // вниз
        _ => (Vec2::new(0.1, 0.12), Vec2::new(0.9, 0.88)),   // со стены
    }
}

/// Сетка уровня: `y` растёт вверх, ячейка (0, 0) — левый нижний угол карты.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    cells: Vec<u8>,
    /// Номер тайла шипов для каждой ячейки (0 — нет).
    spikes: Vec<u8>,
}

impl Grid {
    /// Сетка из номеров тайлов `Tilemap` (строка 0 карты — верхняя).
    pub fn from_tilemap(map: &Tilemap) -> Grid {
        let grid = map.grid();
        let (w, h) = (grid.width as i32, grid.height as i32);
        let mut out = Grid {
            width: w,
            height: h,
            cells: vec![0; (w * h).max(0) as usize],
            spikes: vec![0; (w * h).max(0) as usize],
        };
        for row in 0..h {
            for x in 0..w {
                let tile = grid.get(x as usize, row as usize);
                let y = h - 1 - row;
                let i = (y * w + x) as usize;
                out.cells[i] = classify(tile);
                if classify(tile) == SPIKE {
                    out.spikes[i] = tile as u8;
                }
            }
        }
        out
    }

    /// Пустая сетка заданного размера (тесты).
    pub fn empty(width: i32, height: i32) -> Grid {
        Grid {
            width,
            height,
            cells: vec![0; (width * height) as usize],
            spikes: vec![0; (width * height) as usize],
        }
    }

    /// Флаги ячейки. За левым, правым краем и потолком — стена; под картой — пустота (яма).
    pub fn flags(&self, x: i32, y: i32) -> u8 {
        if x < 0 || x >= self.width || y >= self.height {
            return SOLID;
        }
        if y < 0 {
            return 0;
        }
        self.cells[(y * self.width + x) as usize]
    }

    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        self.flags(x, y) & SOLID != 0
    }

    pub fn set_flags(&mut self, x: i32, y: i32, flags: u8) {
        if x >= 0 && y >= 0 && x < self.width && y < self.height {
            self.cells[(y * self.width + x) as usize] = flags;
        }
    }

    pub fn spike_tile(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return 0;
        }
        self.spikes[(y * self.width + x) as usize] as u32
    }

    /// Твёрдо ли в точке мира.
    pub fn solid_at(&self, p: Vec2) -> bool {
        self.is_solid(p.x.floor() as i32, p.y.floor() as i32)
    }

    /// Есть ли опора (стена или «облачко») в точке мира.
    pub fn support_at(&self, p: Vec2) -> bool {
        self.flags(p.x.floor() as i32, p.y.floor() as i32) & (SOLID | ONEWAY) != 0
    }

    /// Есть ли шипы в прямоугольнике (центр, половинки). Возвращает номер тайла.
    pub fn spikes_in(&self, center: Vec2, half: Vec2) -> Option<u32> {
        let (x0, x1) = ((center.x - half.x).floor() as i32, (center.x + half.x).floor() as i32);
        let (y0, y1) = ((center.y - half.y).floor() as i32, (center.y + half.y).floor() as i32);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let tile = self.spike_tile(cx, cy);
                if tile == 0 {
                    continue;
                }
                let (lo, hi) = spike_box(tile);
                let (min, max) = (Vec2::new(cx as f32, cy as f32) + lo, Vec2::new(cx as f32, cy as f32) + hi);
                if center.x + half.x > min.x && center.x - half.x < max.x && center.y + half.y > min.y && center.y - half.y < max.y {
                    return Some(tile);
                }
            }
        }
        None
    }
}

/// Движущаяся «облачная» платформа, опубликованная в общем списке.
#[derive(Clone, Copy, Debug)]
pub struct Plat {
    pub id: GameObjectId,
    pub min: Vec2,
    pub max: Vec2,
    /// Как далеко платформа сдвинулась в этом кадре (седок едет вместе с ней).
    pub delta: Vec2,
}

/// Общее состояние уровня, на которое смотрят все компоненты. Живёт в `world.resources`;
/// клонировать дёшево (внутри `Rc`).
#[derive(Clone, Default)]
pub struct Shared {
    pub grid: Rc<RefCell<Grid>>,
    pub plats: Rc<RefCell<Vec<Plat>>>,
}

impl Shared {
    pub fn new(grid: Grid) -> Shared {
        Shared {
            grid: Rc::new(RefCell::new(grid)),
            plats: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn publish(&self, plat: Plat) {
        let mut list = self.plats.borrow_mut();
        match list.iter_mut().find(|p| p.id == plat.id) {
            Some(slot) => *slot = plat,
            None => list.push(plat),
        }
    }

    pub fn retract(&self, id: GameObjectId) {
        self.plats.borrow_mut().retain(|p| p.id != id);
    }

    pub fn plat(&self, id: GameObjectId) -> Option<Plat> {
        self.plats.borrow().iter().find(|p| p.id == id).copied()
    }
}

/// Тело: центр прямоугольника, половинки сторон, скорость.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub pos: Vec2,
    pub half: Vec2,
    pub vel: Vec2,
}

/// Чего коснулись за шаг.
#[derive(Clone, Copy, Debug, Default)]
pub struct Contact {
    /// −1 стена слева, +1 стена справа, 0 — нет.
    pub wall: i8,
    pub floor: bool,
    pub ceiling: bool,
    /// Платформа, на которую встали (если не тайл).
    pub platform: Option<GameObjectId>,
    /// Вертикальная скорость в момент удара об пол (до обнуления), для пыли и звука.
    pub impact: f32,
}

/// Один шаг движения: сначала по X, потом по Y. `drop` — игрок жмёт «вниз» и проваливается
/// сквозь «облачка». Скорость за шаг должна быть меньше клетки (при 24 м/с и шаге 1/120 с — 0.2).
pub fn step(grid: &Grid, plats: &[Plat], body: &mut Body, dt: f32, drop: bool) -> Contact {
    let mut contact = Contact::default();
    let Vec2 { x: hx, y: hy } = body.half;

    // ---- X ----
    body.pos.x += body.vel.x * dt;
    let (y0, y1) = (((body.pos.y - hy + EPS).floor()) as i32, ((body.pos.y + hy - EPS).floor()) as i32);
    let (x0, x1) = ((body.pos.x - hx).floor() as i32, (body.pos.x + hx).floor() as i32);
    for cy in y0..=y1 {
        for cx in x0..=x1 {
            if !grid.is_solid(cx, cy) {
                continue;
            }
            if body.vel.x > 0.0 || (body.vel.x == 0.0 && body.pos.x < cx as f32 + 0.5) {
                body.pos.x = cx as f32 - hx - EPS;
                contact.wall = 1;
            } else {
                body.pos.x = cx as f32 + 1.0 + hx + EPS;
                contact.wall = -1;
            }
            body.vel.x = 0.0;
        }
    }

    // ---- Y ----
    let prev_bottom = body.pos.y - hy;
    body.pos.y += body.vel.y * dt;
    let (x0, x1) = (((body.pos.x - hx + EPS).floor()) as i32, ((body.pos.x + hx - EPS).floor()) as i32);
    let (y0, y1) = ((body.pos.y - hy).floor() as i32, (body.pos.y + hy).floor() as i32);
    for cy in y0..=y1 {
        for cx in x0..=x1 {
            let flags = grid.flags(cx, cy);
            if flags & SOLID != 0 {
                if body.vel.y <= 0.0 {
                    contact.impact = contact.impact.max(-body.vel.y);
                    body.pos.y = cy as f32 + 1.0 + hy;
                    contact.floor = true;
                } else {
                    body.pos.y = cy as f32 - hy - EPS;
                    contact.ceiling = true;
                }
                body.vel.y = 0.0;
            } else if flags & ONEWAY != 0 && body.vel.y <= 0.0 && !drop && prev_bottom >= cy as f32 + 1.0 - 0.03 {
                // верхняя часть «облачка» — тонкая, но опорой считаем всю грань клетки
                contact.impact = contact.impact.max(-body.vel.y);
                body.pos.y = cy as f32 + 1.0 + hy;
                body.vel.y = 0.0;
                contact.floor = true;
            }
        }
    }
    if body.vel.y <= 0.0 && !drop {
        let bottom = body.pos.y - hy;
        let mut best: Option<&Plat> = None;
        for p in plats {
            let overlaps_x = body.pos.x + hx > p.min.x + 0.04 && body.pos.x - hx < p.max.x - 0.04;
            if overlaps_x && prev_bottom >= p.max.y - 0.18 && bottom <= p.max.y + EPS && best.is_none_or(|b| p.max.y > b.max.y) {
                best = Some(p);
            }
        }
        if let Some(p) = best {
            contact.impact = contact.impact.max(-body.vel.y);
            body.pos.y = p.max.y + hy;
            body.vel.y = 0.0;
            contact.floor = true;
            contact.platform = Some(p.id);
        }
    }
    contact
}

/// Пересекаются ли два прямоугольника (центр + половинки).
pub fn overlap(c1: Vec2, h1: Vec2, c2: Vec2, h2: Vec2) -> bool {
    (c1.x - c2.x).abs() < h1.x + h2.x && (c1.y - c2.y).abs() < h1.y + h2.y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_grid() -> Grid {
        let mut g = Grid::empty(20, 10);
        for x in 0..20 {
            g.set_flags(x, 0, SOLID);
        }
        g
    }

    #[test]
    fn falling_body_lands_on_the_floor() {
        let g = floor_grid();
        let mut b = Body { pos: Vec2::new(5.0, 4.0), half: Vec2::new(0.3, 0.5), vel: Vec2::ZERO };
        let mut landed = false;
        for _ in 0..240 {
            b.vel.y -= 40.0 / 120.0;
            let c = step(&g, &[], &mut b, 1.0 / 120.0, false);
            landed |= c.floor;
        }
        assert!(landed);
        assert!((b.pos.y - 1.5).abs() < 0.01, "{}", b.pos.y);
    }

    #[test]
    fn walls_stop_horizontal_motion() {
        let mut g = floor_grid();
        for y in 1..5 {
            g.set_flags(8, y, SOLID);
        }
        let mut b = Body { pos: Vec2::new(5.0, 1.5), half: Vec2::new(0.3, 0.5), vel: Vec2::new(7.0, 0.0) };
        let mut wall = 0;
        for _ in 0..120 {
            b.vel.y -= 0.3;
            let c = step(&g, &[], &mut b, 1.0 / 120.0, false);
            if c.wall != 0 {
                wall = c.wall;
            }
            b.vel.x = 7.0;
        }
        assert_eq!(wall, 1);
        assert!(b.pos.x < 8.0 - 0.3 + 0.01);
    }

    #[test]
    fn one_way_tiles_hold_from_above_and_pass_from_below() {
        let mut g = floor_grid();
        g.set_flags(5, 3, ONEWAY);
        let mut up = Body { pos: Vec2::new(5.5, 1.5), half: Vec2::new(0.3, 0.5), vel: Vec2::new(0.0, 14.0) };
        for _ in 0..20 {
            step(&g, &[], &mut up, 1.0 / 120.0, false);
        }
        assert!(up.pos.y > 2.5, "passes up through the cloud: {}", up.pos.y);
        let mut down = Body { pos: Vec2::new(5.5, 6.0), half: Vec2::new(0.3, 0.5), vel: Vec2::ZERO };
        let mut floor = false;
        for _ in 0..200 {
            down.vel.y -= 0.3;
            floor |= step(&g, &[], &mut down, 1.0 / 120.0, false).floor;
        }
        assert!(floor && (down.pos.y - 4.5).abs() < 0.01, "stands on the cloud: {}", down.pos.y);
    }

    #[test]
    fn out_of_map_below_is_a_pit() {
        let g = Grid::empty(10, 10);
        assert_eq!(g.flags(3, -1), 0);
        assert_eq!(g.flags(-1, 3), SOLID);
        assert_eq!(g.flags(3, 10), SOLID);
    }
}
