//! Земля сцен витрины, тоже низкополигональная: остров (поле высот с плоскими гранями,
//! цвет грани — по высоте и уклону), лесная поляна и подиум-диорама мастерской.
//!
//! Высоту в любой точке даёт то же разбиение на треугольники, что и меш
//! (`Field::height`), поэтому расставленные по ней объекты стоят точно на гранях.

use engine::prelude::*;

use super::geo::*;
use super::kit::{Model, Theme};
use super::palette::C;

/// Сглаженный шум значений (детерминированный, без зависимостей).
pub fn value_noise(x: f32, z: f32, seed: u64) -> f32 {
    let hash = |ix: i32, iz: i32| {
        let mut rng = Rng::new(seed ^ ((ix as u64) << 32) ^ (iz as u32 as u64).wrapping_mul(0x9e37_79b9));
        rng.f()
    };
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let a = hash(ix, iz) + (hash(ix + 1, iz) - hash(ix, iz)) * sx;
    let b = hash(ix, iz + 1) + (hash(ix + 1, iz + 1) - hash(ix, iz + 1)) * sx;
    a + (b - a) * sz
}

/// Фрактальный шум, примерно −1..1.
pub fn fbm(x: f32, z: f32, octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += (value_noise(x * freq, z * freq, seed + o as u64) * 2.0 - 1.0) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

/// Поле высот на квадратной сетке с центром в нуле: `n × n` клеток по `step` метров.
pub struct Field {
    pub n: usize,
    pub step: f32,
    pub heights: Vec<f32>,
}

impl Field {
    fn build(n: usize, step: f32, f: impl Fn(f32, f32) -> f32) -> Self {
        let half = n as f32 * step * 0.5;
        let mut heights = Vec::with_capacity((n + 1) * (n + 1));
        for j in 0..=n {
            for i in 0..=n {
                heights.push(f(i as f32 * step - half, j as f32 * step - half));
            }
        }
        Self { n, step, heights }
    }

    fn point(&self, i: usize, j: usize) -> Vec3 {
        let half = self.n as f32 * self.step * 0.5;
        Vec3::new(i as f32 * self.step - half, self.heights[j * (self.n + 1) + i], j as f32 * self.step - half)
    }

    /// Два треугольника клетки (диагональ чередуется — грани не выстраиваются в полосы).
    fn cell(&self, i: usize, j: usize) -> [[Vec3; 3]; 2] {
        let (a, b, c, d) = (self.point(i, j), self.point(i + 1, j), self.point(i + 1, j + 1), self.point(i, j + 1));
        if (i + j) % 2 == 0 {
            [[a, d, c], [a, c, b]]
        } else {
            [[a, d, b], [b, d, c]]
        }
    }

    /// Высота поверхности меша в точке (x, z); за краем — высота края.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let half = self.n as f32 * self.step * 0.5;
        let fx = ((x + half) / self.step).clamp(0.0, self.n as f32 - 1e-3);
        let fz = ((z + half) / self.step).clamp(0.0, self.n as f32 - 1e-3);
        let (i, j) = (fx as usize, fz as usize);
        let p = Vec3::new(x.clamp(-half, half), 0.0, z.clamp(-half, half));
        for t in self.cell(i, j) {
            if let Some(h) = barycentric_height(&t, p) {
                return h;
            }
        }
        self.point(i, j).y
    }

    /// Нормаль грани под точкой.
    pub fn normal(&self, x: f32, z: f32) -> Vec3 {
        let e = 0.05;
        let dx = self.height(x + e, z) - self.height(x - e, z);
        let dz = self.height(x, z + e) - self.height(x, z - e);
        Vec3::new(-dx, 2.0 * e, -dz).normalize()
    }

    fn triangles(&self) -> Vec<[Vec3; 3]> {
        let mut out = Vec::with_capacity(self.n * self.n * 2);
        for j in 0..self.n {
            for i in 0..self.n {
                out.extend(self.cell(i, j));
            }
        }
        out
    }
}

fn barycentric_height(t: &[Vec3; 3], p: Vec3) -> Option<f32> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let det = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
    if det.abs() < 1e-9 {
        return None;
    }
    let l1 = ((b.z - c.z) * (p.x - c.x) + (c.x - b.x) * (p.z - c.z)) / det;
    let l2 = ((c.z - a.z) * (p.x - c.x) + (a.x - c.x) * (p.z - c.z)) / det;
    let l3 = 1.0 - l1 - l2;
    (l1 >= -1e-4 && l2 >= -1e-4 && l3 >= -1e-4).then_some(l1 * a.y + l2 * b.y + l3 * c.y)
}

/// Лицом вверх (сетка задаётся без учёта обхода).
fn up(t: [Vec3; 3]) -> [Vec3; 3] {
    if normal(&t).y < 0.0 { [t[0], t[2], t[1]] } else { t }
}

// ---------------- остров ----------------

/// Деревня на острове стоит на площадке с центром здесь (x, z) и радиусом.
pub const VILLAGE: (f32, f32, f32) = (-4.0, 6.0, 15.0);
/// Холм с мельницей.
pub const HILL: (f32, f32) = (14.0, -12.0);

/// Высота острова до разбиения на грани.
fn island_shape(x: f32, z: f32) -> f32 {
    // Береговая линия — искажённый круг радиусом ~42 м.
    let angle = z.atan2(x);
    let r = (x * x + z * z).sqrt();
    let coast = 40.0 + 7.0 * fbm(angle.cos() * 1.3 + 3.0, angle.sin() * 1.3 + 7.0, 3, 5);
    let t = (r / coast).clamp(0.0, 2.0);
    // Профиль: плато ~3 м, спуск к берегу, под водой шельф до −5 м.
    let mut h = if t < 1.0 { 3.2 * (1.0 - t.powf(3.0)) + 0.25 } else { 0.25 - (t - 1.0) * 12.0 };
    h = h.max(-5.0);
    // Холм с мельницей и обрыв на востоке.
    let dh = Vec2::new(x - HILL.0, z - HILL.1).length();
    h += 6.5 * (1.0 - (dh / 16.0).clamp(0.0, 1.0)).powf(1.6);
    let east = sstep(14.0, 30.0, x) * (1.0 - sstep(-6.0, 22.0, z.abs())) * (1.0 - sstep(0.98, 1.1, t));
    h += east * 5.0;
    // Неровности; на площадке деревни — почти ровно.
    let dv = Vec2::new(x - VILLAGE.0, z - VILLAGE.1).length();
    let flat = sstep(VILLAGE.2 * 0.7, VILLAGE.2 * 1.3, dv);
    h += fbm(x * 0.08, z * 0.08, 3, 9) * 1.2 * flat * (t < 1.0) as i32 as f32;
    let village_level = 3.3;
    h = village_level + (h - village_level) * flat.max(if t > 0.95 { 1.0 } else { 0.0 });
    h
}

pub fn island_field() -> Field {
    Field::build(44, 2.5, island_shape)
}

/// Остров: ~3.9 тыс. треугольников. Песок у воды, мокрый песок и дно ниже, трава с
/// пятнами, скалы на крутом.
pub fn island() -> Model {
    let field = island_field();
    let mut m = Model::new_ground("island", 4000);
    for t in field.triangles() {
        let t = up(t);
        let h = (t[0].y + t[1].y + t[2].y) / 3.0;
        let n = normal(&t).normalize();
        let c = (t[0] + t[1] + t[2]) / 3.0;
        let patch = fbm(c.x * 0.07 + 11.0, c.z * 0.07, 2, 21);
        let color = if h < -1.6 {
            C::Seabed
        } else if h < -0.1 {
            C::SandWet
        } else if h < 0.75 {
            C::Shore
        } else if n.y < 0.72 {
            if patch > 0.0 { C::Cliff } else { C::CliffDark }
        } else if n.y < 0.86 {
            C::MeadowDark
        } else if patch > 0.25 {
            C::GrassLight
        } else if patch < -0.3 {
            C::MeadowDark
        } else {
            C::Meadow
        };
        m.push_tri(t, color);
    }
    m
}

// ---------------- поляна ----------------

/// Поляна: середина (костёр) в нуле.
fn clearing_shape(x: f32, z: f32) -> f32 {
    let r = (x * x + z * z).sqrt();
    let bowl = sstep(9.0, 30.0, r) * 2.2;
    bowl + fbm(x * 0.12, z * 0.12, 3, 31) * 0.6 * sstep(3.0, 10.0, r)
}

pub fn clearing_field() -> Field {
    Field::build(36, 2.0, clearing_shape)
}

pub fn clearing() -> Model {
    let field = clearing_field();
    let mut m = Model::new_ground("clearing", 2700);
    for t in field.triangles() {
        let t = up(t);
        let c = (t[0] + t[1] + t[2]) / 3.0;
        let r = Vec2::new(c.x, c.z).length();
        let patch = fbm(c.x * 0.15 + 4.0, c.z * 0.15, 2, 33);
        // Тропинка уходит от костра к −Z, слегка виляя.
        let path = (c.x - 1.5 * (c.z * 0.15).sin()).abs() < 1.3 && c.z < -2.0;
        let color = if r < 3.2 {
            C::Dirt
        } else if path {
            C::Path
        } else if r > 22.0 {
            if patch > 0.0 { C::ForestDark } else { C::Forest }
        } else if patch > 0.3 {
            C::Moss
        } else if patch < -0.25 {
            C::Forest
        } else {
            C::GrassDark
        };
        m.push_tri(t, color);
    }
    m
}

// ---------------- диорама ----------------

/// Сторона пола диорамы (м); пол — квадрат с центром в нуле на y = 0.
pub const DIORAMA: f32 = 8.0;

/// Подиум-диорама мастерской: пол из досок, две стены (задняя −Z и левая −X) с окном
/// и полками-кронштейнами, толстое основание с фаской.
pub fn diorama() -> Model {
    let h = DIORAMA * 0.5;
    let mut m = Model::new_ground("diorama", 700);
    // Основание.
    m.add_geo(at(cuboid(DIORAMA + 0.6, 1.4, DIORAMA + 0.6), tr(0.0, -1.6, 0.0)), C::Plinth);
    m.add_geo(at(cuboid(DIORAMA + 0.4, 0.2, DIORAMA + 0.4), tr(0.0, -0.2, 0.0)), C::PlinthEdge);
    // Доски пола.
    let boards = 16;
    for i in 0..boards {
        let x = -h + (i as f32 + 0.5) * DIORAMA / boards as f32;
        let c = if i % 3 == 1 { C::FloorDark } else { C::Floor };
        m.add_geo(at(cuboid(DIORAMA / boards as f32 - 0.03, 0.06, DIORAMA), tr(x, -0.06, 0.0)), c);
    }
    // Задняя стена с окном (проём 2.4 × 1.6 по центру x = 1.0, низ на 1.3 м).
    let (wall_h, t) = (4.2, 0.3);
    let back = |m: &mut Model, x0: f32, x1: f32, y0: f32, y1: f32| {
        m.add_geo(block(Vec3::new(x0, y0, -h - t), Vec3::new(x1, y1, -h)), C::WallBlue);
    };
    back(&mut m, -h - t, -0.2, 0.0, wall_h);
    back(&mut m, 2.2, h, 0.0, wall_h);
    back(&mut m, -0.2, 2.2, 0.0, 1.3);
    back(&mut m, -0.2, 2.2, 2.9, wall_h);
    // Рама и подоконник.
    m.add_geo(block(Vec3::new(-0.3, 1.18, -h - 0.05), Vec3::new(2.3, 1.3, -h + 0.18)), C::WoodLight);
    m.add_geo(block(Vec3::new(0.94, 1.3, -h - 0.2), Vec3::new(1.06, 2.9, -h - 0.1)), C::WoodDark);
    m.add_geo(block(Vec3::new(-0.2, 2.06, -h - 0.2), Vec3::new(2.2, 2.14, -h - 0.1)), C::WoodDark);
    // Дневной свет за окном: светящееся «небо» в проёме (тени не отбрасывает).
    m.add_glow(block(Vec3::new(-0.2, 1.3, -h - t), Vec3::new(2.2, 2.9, -h - t + 0.04)), C::CloudShade);
    // Левая стена.
    m.add_geo(block(Vec3::new(-h - t, 0.0, -h - t), Vec3::new(-h, wall_h, h)), C::WallBlueDark);
    // Плинтусы и балка поверху.
    m.add_geo(block(Vec3::new(-h, 0.0, -h), Vec3::new(h, 0.18, -h + 0.06)), C::WoodDark);
    m.add_geo(block(Vec3::new(-h, 0.0, -h), Vec3::new(-h + 0.06, 0.18, h)), C::WoodDark);
    m.add_geo(block(Vec3::new(-h - t, wall_h, -h - t), Vec3::new(h, wall_h + 0.2, -h + 0.1)), C::WoodDark);
    m.add_geo(block(Vec3::new(-h - t, wall_h, -h - t), Vec3::new(-h + 0.1, wall_h + 0.2, h)), C::WoodDark);
    // Доска с инструментами на левой стене.
    m.add_geo(block(Vec3::new(-h, 1.5, -1.6), Vec3::new(-h + 0.06, 2.7, 0.6)), C::Wood);
    for (k, z) in [-1.3, -0.8, -0.3, 0.2].into_iter().enumerate() {
        let len = 0.4 + (k % 2) as f32 * 0.25;
        m.add_geo(block(Vec3::new(-h + 0.06, 2.4 - len, z - 0.04), Vec3::new(-h + 0.12, 2.4, z + 0.04)), C::Steel);
    }
    m
}

impl Model {
    fn new_ground(name: &'static str, budget: usize) -> Self {
        Self { name, theme: Theme::Ground, budget, tris: Vec::new() }
    }

    fn push_tri(&mut self, t: [Vec3; 3], c: C) {
        self.tris.push(super::kit::Tri { p: t, c, m: super::kit::Mat::Kit });
    }

    fn add_glow(&mut self, geo: Geo, c: C) {
        self.tris.extend(geo.into_iter().map(|p| super::kit::Tri { p, c, m: super::kit::Mat::Glow }));
    }

    fn add_geo(&mut self, geo: Geo, c: C) {
        for t in geo {
            self.push_tri(t, c);
        }
    }
}

// ---------------- ночное небо ----------------

/// Откуда светит луна на поляне (направление от сцены к луне).
pub const MOON: Vec3 = Vec3::new(-0.35, 0.55, -0.76);

/// Звёзды и луна поляны: светящиеся треугольнички на куполе радиусом 380 м и
/// многогранный диск луны в стороне `MOON`.
pub fn night_sky() -> Model {
    let mut m = Model::new_ground("night_sky", 700);
    let mut rng = Rng::new(404);
    let radius = 380.0;
    for _ in 0..180 {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let el = rng.range(0.12f32, 1.4);
        let dir = Vec3::new(a.cos() * el.cos(), el.sin(), a.sin() * el.cos());
        let center = dir * radius;
        let size = rng.range(0.6, 1.5);
        let (u, v) = (dir.cross(Vec3::Y).normalize_or_zero(), dir.cross(dir.cross(Vec3::Y)).normalize_or_zero());
        let tri = [center + u * size, center - u * size * 0.5 + v * size * 0.87, center - u * size * 0.5 - v * size * 0.87];
        let tri = if normal(&tri).dot(dir) > 0.0 { [tri[0], tri[2], tri[1]] } else { tri };
        let c = if rng.f() < 0.2 { C::Eye } else { C::Snow };
        m.add_glow(vec![tri], c);
    }
    let moon = MOON.normalize() * (radius - 20.0);
    let disc = at(cyl(10, 14.0, 1.0), Mat4::from_translation(moon) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, -MOON.normalize())));
    m.add_glow(disc, C::Snow);
    m
}

/// Все модели земли и неба.
pub fn all() -> Vec<Model> {
    vec![island(), clearing(), diorama(), night_sky()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_height_matches_mesh_vertices() {
        let field = island_field();
        let p = field.point(10, 17);
        assert!((field.height(p.x, p.z) - p.y).abs() < 1e-3);
        // Деревня — ровная площадка над водой.
        let h = field.height(VILLAGE.0, VILLAGE.1);
        assert!(h > 2.0 && h < 4.5, "village height {h}");
        assert!(field.height(0.0, 60.0) < -1.0, "the sea must surround the island");
    }
}
