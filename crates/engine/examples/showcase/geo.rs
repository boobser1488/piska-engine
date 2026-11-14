//! Геометрия набора витрины: примитивы с плоскими гранями (каждый треугольник — своя
//! нормаль), преобразования и детерминированный генератор случайных чисел.
//!
//! Все примитивы стоят основанием на y = 0 (кроме `blob` — он с центром в нуле) и
//! возвращают треугольники с обходом против часовой стрелки снаружи (как ждёт рендерер с
//! отсечением задних граней).

use engine::prelude::*;

/// Треугольники одного примитива.
pub type Geo = Vec<[Vec3; 3]>;

/// Детерминированный генератор (splitmix64): одинаковое зерно — одинаковые модели и сцены
/// на любой машине.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Равномерно в [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Равномерно в [a, b).
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    /// Целое в [0, n).
    pub fn index(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

// ---------------- преобразования ----------------

pub fn tr(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}
pub fn rx(deg: f32) -> Mat4 {
    Mat4::from_rotation_x(deg.to_radians())
}
pub fn ry(deg: f32) -> Mat4 {
    Mat4::from_rotation_y(deg.to_radians())
}
pub fn rz(deg: f32) -> Mat4 {
    Mat4::from_rotation_z(deg.to_radians())
}
pub fn sc(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_scale(Vec3::new(x, y, z))
}

/// Применить матрицу. Зеркальная матрица (отрицательный определитель) меняет обход,
/// поэтому вершины переставляются — грани остаются лицом наружу.
pub fn at(geo: Geo, m: Mat4) -> Geo {
    let flip = m.determinant() < 0.0;
    geo.into_iter()
        .map(|[a, b, c]| {
            let (a, b, c) = (m.transform_point3(a), m.transform_point3(b), m.transform_point3(c));
            if flip { [a, c, b] } else { [a, b, c] }
        })
        .collect()
}

pub fn normal(t: &[Vec3; 3]) -> Vec3 {
    (t[1] - t[0]).cross(t[2] - t[0])
}

/// Развернуть треугольник лицом в сторону `outward`.
fn orient(t: [Vec3; 3], outward: Vec3) -> [Vec3; 3] {
    if normal(&t).dot(outward) < 0.0 { [t[0], t[2], t[1]] } else { t }
}

/// Для выпуклых примитивов: каждая грань — лицом от центра.
fn orient_convex(geo: Geo, center: Vec3) -> Geo {
    geo.into_iter()
        .map(|t| {
            let c = (t[0] + t[1] + t[2]) / 3.0;
            orient(t, c - center)
        })
        .collect()
}

fn degenerate(t: &[Vec3; 3]) -> bool {
    normal(t).length_squared() < 1e-12
}

// ---------------- примитивы ----------------

/// Коробка между углами `min` и `max`.
pub fn block(min: Vec3, max: Vec3) -> Geo {
    let p = |x: bool, y: bool, z: bool| {
        Vec3::new(if x { max.x } else { min.x }, if y { max.y } else { min.y }, if z { max.z } else { min.z })
    };
    let quads = [
        [p(false, false, false), p(true, false, false), p(true, true, false), p(false, true, false)],
        [p(false, false, true), p(true, false, true), p(true, true, true), p(false, true, true)],
        [p(false, false, false), p(false, true, false), p(false, true, true), p(false, false, true)],
        [p(true, false, false), p(true, true, false), p(true, true, true), p(true, false, true)],
        [p(false, false, false), p(true, false, false), p(true, false, true), p(false, false, true)],
        [p(false, true, false), p(true, true, false), p(true, true, true), p(false, true, true)],
    ];
    let geo = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
    orient_convex(geo, (min + max) * 0.5)
}

/// Коробка `sx × sy × sz`, основание в y = 0, по центру x, z.
pub fn cuboid(sx: f32, sy: f32, sz: f32) -> Geo {
    block(Vec3::new(-sx * 0.5, 0.0, -sz * 0.5), Vec3::new(sx * 0.5, sy, sz * 0.5))
}

/// Тело вращения вокруг оси Y: профиль `(радиус, высота)` снизу вверх, `n` сторон.
/// Точки с нулевым радиусом — полюса (донышко и макушка закрыты).
pub fn lathe(profile: &[(f32, f32)], n: usize, phase_deg: f32) -> Geo {
    let ring = |(r, y): (f32, f32), i: usize| {
        let a = phase_deg.to_radians() + std::f32::consts::TAU * i as f32 / n as f32;
        Vec3::new(r * a.cos(), y, r * a.sin())
    };
    let mut geo = Geo::new();
    for k in 0..profile.len().saturating_sub(1) {
        for i in 0..n {
            let (a0, a1) = (ring(profile[k], i), ring(profile[k], (i + 1) % n));
            let (b0, b1) = (ring(profile[k + 1], i), ring(profile[k + 1], (i + 1) % n));
            for t in [[a0, b0, a1], [a1, b0, b1]] {
                if !degenerate(&t) {
                    geo.push(t);
                }
            }
        }
    }
    geo
}

/// Цилиндр (призма) радиуса `r`, высоты `h`.
pub fn cyl(n: usize, r: f32, h: f32) -> Geo {
    lathe(&[(0.0, 0.0), (r, 0.0), (r, h), (0.0, h)], n, 0.0)
}

/// Усечённый конус.
pub fn frustum(n: usize, r0: f32, r1: f32, h: f32) -> Geo {
    lathe(&[(0.0, 0.0), (r0, 0.0), (r1, h), (0.0, h)], n, 0.0)
}

/// Конус.
pub fn cone(n: usize, r: f32, h: f32) -> Geo {
    lathe(&[(0.0, 0.0), (r, 0.0), (0.0, h)], n, 0.0)
}

/// Неровный «камень» / «крона»: икосаэдр (`subdiv` раз поделённый), вершины сдвинуты
/// по радиусу на ±`jitter`, растянут до полуосей `r`. Центр — в нуле.
pub fn blob(r: Vec3, subdiv: u32, jitter: f32, seed: u64) -> Geo {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut verts: Vec<Vec3> = [
        (-1.0, t, 0.0), (1.0, t, 0.0), (-1.0, -t, 0.0), (1.0, -t, 0.0),
        (0.0, -1.0, t), (0.0, 1.0, t), (0.0, -1.0, -t), (0.0, 1.0, -t),
        (t, 0.0, -1.0), (t, 0.0, 1.0), (-t, 0.0, -1.0), (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut faces: Vec<[usize; 3]> = vec![
        [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
        [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
        [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
    ];
    for _ in 0..subdiv {
        let mut cache = std::collections::HashMap::new();
        let mut mid = |a: usize, b: usize, verts: &mut Vec<Vec3>| {
            let key = (a.min(b), a.max(b));
            *cache.entry(key).or_insert_with(|| {
                verts.push(((verts[a] + verts[b]) * 0.5).normalize());
                verts.len() - 1
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let (ab, bc, ca) = (mid(a, b, &mut verts), mid(b, c, &mut verts), mid(c, a, &mut verts));
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        faces = next;
    }
    let mut rng = Rng::new(seed);
    let verts: Vec<Vec3> = verts.iter().map(|v| *v * (1.0 + rng.range(-jitter, jitter)) * r).collect();
    let geo = faces.iter().map(|&[a, b, c]| [verts[a], verts[b], verts[c]]).collect();
    orient_convex(geo, Vec3::ZERO)
}

/// Двускатная крыша: основание `w` (вдоль X) × `d` (вдоль Z) на y = 0, конёк вдоль X на
/// высоте `h`.
pub fn gable(w: f32, d: f32, h: f32) -> Geo {
    let (x, z) = (w * 0.5, d * 0.5);
    let a = Vec3::new(-x, 0.0, -z);
    let b = Vec3::new(x, 0.0, -z);
    let c = Vec3::new(x, 0.0, z);
    let e = Vec3::new(-x, 0.0, z);
    let r0 = Vec3::new(-x, h, 0.0);
    let r1 = Vec3::new(x, h, 0.0);
    let geo = vec![
        [a, b, r1], [a, r1, r0], // скат к −Z
        [e, c, r1], [e, r1, r0], // скат к +Z
        [a, e, r0], [b, c, r1],  // фронтоны
        [a, b, c], [a, c, e],    // низ
    ];
    orient_convex(geo, Vec3::new(0.0, h * 0.3, 0.0))
}

/// Оболочка по сечениям: `rings[k]` — замкнутый контур (одинаковое число точек), грани
/// между соседними контурами, торцы закрыты веером.
pub fn loft(rings: &[Vec<Vec3>]) -> Geo {
    let center = |ring: &Vec<Vec3>| ring.iter().copied().sum::<Vec3>() / ring.len() as f32;
    let mut geo = Geo::new();
    for k in 0..rings.len() - 1 {
        let (a, b) = (&rings[k], &rings[k + 1]);
        let mid = (center(a) + center(b)) * 0.5;
        let m = a.len();
        for i in 0..m {
            let j = (i + 1) % m;
            for t in [[a[i], b[i], a[j]], [a[j], b[i], b[j]]] {
                if !degenerate(&t) {
                    let c = (t[0] + t[1] + t[2]) / 3.0;
                    geo.push(orient(t, c - mid));
                }
            }
        }
    }
    let (first, last) = (&rings[0], &rings[rings.len() - 1]);
    let along = center(last) - center(first);
    for (ring, dir) in [(first, -along), (last, along)] {
        let c = center(ring);
        for i in 0..ring.len() {
            let t = [c, ring[i], ring[(i + 1) % ring.len()]];
            if !degenerate(&t) {
                geo.push(orient(t, dir));
            }
        }
    }
    geo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_outward(geo: &Geo, center: Vec3) -> bool {
        geo.iter().all(|t| normal(t).dot((t[0] + t[1] + t[2]) / 3.0 - center) > 0.0)
    }

    #[test]
    fn primitives_face_outward() {
        assert!(closed_outward(&cuboid(1.0, 1.0, 1.0), Vec3::new(0.0, 0.5, 0.0)));
        assert!(closed_outward(&cyl(6, 1.0, 2.0), Vec3::new(0.0, 1.0, 0.0)));
        assert!(closed_outward(&cone(5, 1.0, 2.0), Vec3::new(0.0, 0.5, 0.0)));
        assert!(closed_outward(&blob(Vec3::ONE, 1, 0.1, 3), Vec3::ZERO));
        assert!(closed_outward(&at(cuboid(1.0, 1.0, 1.0), sc(-1.0, 1.0, 1.0)), Vec3::new(0.0, 0.5, 0.0)));
    }
}

/// Плавная ступенька от `e0` до `e1` (как `smoothstep` в шейдерах).
pub fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Ещё более плавная ступенька (нулевые первая и вторая производные на краях).
pub fn ssstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
