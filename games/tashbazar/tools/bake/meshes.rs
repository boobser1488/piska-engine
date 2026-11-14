//! Свои модели с развёрткой под свой атлас: ящики (три размера, один атлас) и телега.
//! Модель собирается из брусков и колец (каждая грань получает свой прямоугольник атласа
//! с постоянной плотностью текселей), пишется в `assets/models/*.obj`; пиксельный атлас
//! рисуется здесь же (`textures/crate.png`, `textures/cart.png`).
//!
//! Плотность — 16 текселей на метр, как у всей карты (`textures.rs`), цвета — из её палитры.
//! Атлас 128 × 128; у областей поля в пиксель с продолжением края.

use std::error::Error;
use std::f32::consts::{PI, TAU};
use std::fmt::Write as _;
use std::path::Path;

use engine::prelude::*;
use image::{Rgb, RgbImage};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Ящики: имя префаба и размер.
pub const CRATES: [(&str, Vec3); 3] = [
    ("crate_small", Vec3::new(0.9, 0.9, 0.9)),
    ("crate_big", Vec3::new(1.25, 1.25, 1.25)),
    ("crate_long", Vec3::new(1.8, 0.9, 0.9)),
];

/// Тексели на метр.
pub const DENSITY: f32 = 16.0;
/// Сторона атласа.
pub const ATLAS: u32 = 128;
/// Поле вокруг области (px).
const PAD: f32 = 1.0;

/// Прямоугольник атласа в пикселях.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
    Region { x, y, w, h }
}

/// Детерминированный хэш → 0..1.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Меш: вершины с UV и нормалью, треугольники.
#[derive(Default)]
pub struct MeshOut {
    pos: Vec<Vec3>,
    uv: Vec<Vec2>,
    nrm: Vec<Vec3>,
    tris: Vec<[usize; 3]>,
}

/// Грань бруска: какую область атласа и как класть (кусок `u0..` по длине грани).
pub type FaceMap<'a> = &'a dyn Fn(Face) -> (Region, Vec2);

/// Грани бруска: ±X — торцы (`End`), ±Y — верх/низ, ±Z — лицо/тыл.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl MeshOut {
    /// Четырёхугольник `a b c d` (против часовой, если смотреть с лица), UV в пикселях атласа.
    fn quad(&mut self, p: [Vec3; 4], n: Vec3, uv: [Vec2; 4]) {
        let base = self.pos.len();
        for i in 0..4 {
            self.pos.push(p[i]);
            self.uv.push(uv[i] / ATLAS as f32);
            self.nrm.push(n);
        }
        self.tris.push([base, base + 1, base + 2]);
        self.tris.push([base, base + 2, base + 3]);
    }

    /// Брусок: центр, размер, поворот. Каждой грани — область атласа и сдвиг внутри неё (px);
    /// грань кладётся с плотностью [`DENSITY`]: длинная сторона грани — по U области.
    pub fn bar(&mut self, center: Vec3, size: Vec3, rot: Quat, map: FaceMap) {
        let h = size * 0.5;
        let faces: [(Face, Vec3, Vec3, Vec3); 6] = [
            (Face::PosX, Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Face::NegX, Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Face::PosY, Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Face::NegY, Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Face::PosZ, Vec3::Z, Vec3::X, Vec3::Y),
            // Тыл: U тоже по +X (волокна и рукояти идут в ту же сторону, что и на лице).
            (Face::NegZ, Vec3::NEG_Z, Vec3::X, Vec3::NEG_Y),
        ];
        for (face, n, mut u, mut v) in faces {
            let (mut eu, mut ev) = ((u * h).length(), (v * h).length());
            // Длинная сторона — по U (волокна дерева идут вдоль бруска).
            if ev > eu {
                std::mem::swap(&mut u, &mut v);
                std::mem::swap(&mut eu, &mut ev);
                u = -u;
            }
            let c = n * h;
            let corners = [c - u * eu - v * ev, c + u * eu - v * ev, c + u * eu + v * ev, c - u * eu + v * ev];
            let p = corners.map(|q| center + rot * q);
            let (region, offset) = map(face);
            let (w, hh) = ((2.0 * eu * DENSITY).min(region.w), (2.0 * ev * DENSITY).min(region.h));
            let o = Vec2::new(region.x, region.y) + offset.min(Vec2::new(region.w - w, region.h - hh)).max(Vec2::ZERO);
            // Картинка: Y вниз; низ грани — внизу области.
            let uv = [o + Vec2::new(0.0, hh), o + Vec2::new(w, hh), o + Vec2::new(w, 0.0), o];
            self.quad(p, rot * n, uv);
        }
    }

    /// Кольцо из `segments` брусков (обод, шина) в плоскости XY вокруг `center` (ось — Z).
    #[allow(clippy::too_many_arguments)]
    pub fn ring(&mut self, center: Vec3, r_in: f32, r_out: f32, width: f32, segments: usize, phase: f32, rot: Quat, map: FaceMap) {
        let apothem = (r_in + r_out) * 0.5;
        let len = 2.0 * r_out * (PI / segments as f32).tan();
        for i in 0..segments {
            let a = phase + i as f32 * TAU / segments as f32;
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            let local = Quat::from_rotation_z(a + PI * 0.5);
            self.bar(center + rot * (dir * apothem), Vec3::new(len, r_out - r_in, width), rot * local, map);
        }
    }

    /// Цилиндр вдоль оси Z (ступица, ось): `segments` граней, торцы — веером.
    #[allow(clippy::too_many_arguments)]
    pub fn cylinder(&mut self, center: Vec3, radius: f32, length: f32, segments: usize, rot: Quat, side: Region, cap: Region) {
        let hz = length * 0.5;
        let circ = TAU * radius * DENSITY;
        for i in 0..segments {
            let (a0, a1) = (i as f32 * TAU / segments as f32, (i + 1) as f32 * TAU / segments as f32);
            let d0 = Vec3::new(a0.cos(), a0.sin(), 0.0);
            let d1 = Vec3::new(a1.cos(), a1.sin(), 0.0);
            let p = [d0 * radius - Vec3::Z * hz, d1 * radius - Vec3::Z * hz, d1 * radius + Vec3::Z * hz, d0 * radius + Vec3::Z * hz].map(|q| center + rot * q);
            let (u0, u1) = (side.x + (i as f32 / segments as f32) * circ.min(side.w), side.x + ((i + 1) as f32 / segments as f32) * circ.min(side.w));
            let vh = (length * DENSITY).min(side.h);
            let uv = [Vec2::new(u0, side.y + vh), Vec2::new(u1, side.y + vh), Vec2::new(u1, side.y), Vec2::new(u0, side.y)];
            // Нормаль грани (плоские грани — чёткие рёбра колпаков).
            let n = rot * ((d0 + d1) * 0.5).normalize();
            // Снаружи (ось вверх): угол растёт вправо — p уже против часовой.
            self.quad(p, n, uv);
        }
        for (z, n) in [(hz, Vec3::Z), (-hz, Vec3::NEG_Z)] {
            let mid = Vec2::new(cap.x + cap.w * 0.5, cap.y + cap.h * 0.5);
            let r_px = (radius * DENSITY).min(cap.w * 0.5);
            for i in 0..segments {
                let (a0, a1) = (i as f32 * TAU / segments as f32, (i + 1) as f32 * TAU / segments as f32);
                let (q0, q1) = (Vec3::new(a0.cos(), a0.sin(), 0.0), Vec3::new(a1.cos(), a1.sin(), 0.0));
                let base = self.pos.len();
                let pts = [Vec3::Z * z, q0 * radius + Vec3::Z * z, q1 * radius + Vec3::Z * z];
                let uvs = [mid, mid + Vec2::new(a0.cos(), -a0.sin()) * r_px, mid + Vec2::new(a1.cos(), -a1.sin()) * r_px];
                for k in 0..3 {
                    self.pos.push(center + rot * pts[k]);
                    self.uv.push(uvs[k] / ATLAS as f32);
                    self.nrm.push(rot * n);
                }
                if z > 0.0 {
                    self.tris.push([base, base + 1, base + 2]);
                } else {
                    self.tris.push([base, base + 2, base + 1]);
                }
            }
        }
    }

    /// Записать OBJ (UV: в OBJ (0, 0) — левый нижний угол).
    pub fn write_obj(&self, path: &Path, comment: &str) -> Result<()> {
        let mut s = format!("# {comment}\n");
        for p in &self.pos {
            let _ = writeln!(s, "v {:.5} {:.5} {:.5}", p.x, p.y, p.z);
        }
        for t in &self.uv {
            let _ = writeln!(s, "vt {:.6} {:.6}", t.x, 1.0 - t.y);
        }
        for n in &self.nrm {
            let _ = writeln!(s, "vn {:.4} {:.4} {:.4}", n.x, n.y, n.z);
        }
        for t in &self.tris {
            let _ = writeln!(s, "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}", t[0] + 1, t[1] + 1, t[2] + 1);
        }
        std::fs::create_dir_all(path.parent().ok_or("model dir")?)?;
        std::fs::write(path, s)?;
        Ok(())
    }

    pub fn triangles(&self) -> usize {
        self.tris.len()
    }
}

// ----------------------------------------------------------------------------------------
// Атлас: цвет из палитры карты по пикселям.

pub struct Atlas {
    pub color: Vec<[u8; 3]>,
}

fn pal(i: usize) -> [u8; 3] {
    super::textures::PAL[i]
}

const WOOD: usize = super::textures::WOOD;
const IRONS: usize = super::textures::IRON;
const RUST: usize = super::textures::RUST;

impl Atlas {
    fn new(fill: usize) -> Self {
        Self { color: vec![pal(fill); (ATLAS * ATLAS) as usize] }
    }

    /// Закрасить область (с полями): `f(x, y)` — пиксель внутри области → индекс палитры.
    fn paint(&mut self, r: Region, f: &dyn Fn(i32, i32) -> usize) {
        let (x0, y0) = ((r.x - PAD).max(0.0) as i32, (r.y - PAD).max(0.0) as i32);
        let (x1, y1) = (((r.x + r.w + PAD) as i32).min(ATLAS as i32), ((r.y + r.h + PAD) as i32).min(ATLAS as i32));
        for y in y0..y1 {
            for x in x0..x1 {
                // Поле: продолжение края области.
                let lx = (x - r.x as i32).clamp(0, r.w as i32 - 1);
                let ly = (y - r.y as i32).clamp(0, r.h as i32 - 1);
                self.color[(y as u32 * ATLAS + x as u32) as usize] = pal(f(lx, ly));
            }
        }
    }

    fn save(&self, dir: &Path, name: &str) -> Result<()> {
        let mut color = RgbImage::new(ATLAS, ATLAS);
        for (i, c) in self.color.iter().enumerate() {
            color.put_pixel(i as u32 % ATLAS, i as u32 / ATLAS, Rgb(*c));
        }
        std::fs::create_dir_all(dir)?;
        color.save(dir.join(format!("{name}.png")))?;
        Ok(())
    }
}

/// Доска в пикселях: тон `shade` (1..3 рампы дерева), по нижней строке — на тон темнее,
/// редкий сучок, гвоздь (пиксель железа) через `nails` px.
fn plank_px(x: i32, y: i32, rows: i32, shade: usize, seed: u32, nails: i32) -> usize {
    if nails > 0 && x % nails == nails / 2 && y == rows / 2 {
        return IRONS;
    }
    if y == rows - 1 || hash(x, seed as i32, 3) > 0.94 {
        return WOOD + shade - 1;
    }
    WOOD + shade
}

/// Торец: тёмное кольцо по краю, светлая сердцевина.
fn end_px(x: i32, y: i32, w: i32) -> usize {
    if x == 0 || y == 0 || x == w - 1 || y == w - 1 {
        WOOD + 1
    } else if (x + y) % 3 == 0 {
        WOOD + 2
    } else {
        WOOD + 3
    }
}

/// Железо: тёмное, заклёпки — светлые пиксели через `rivet` px, редкая ржавчина.
fn iron_px(x: i32, y: i32, rivet: i32, seed: u32) -> usize {
    if rivet > 0 && x % rivet == rivet / 2 && y == 1 {
        return IRONS + 2;
    }
    if hash(x / 2, y, seed) > 0.88 {
        RUST
    } else {
        IRONS
    }
}

/// Трафаретный знак (свой, не чужая марка) на трёх досках — 6 px высотой: стрелка «верх»
/// 5 × 5 и полоса под ней; `w` — ширина области.
fn stencil_px(x: i32, y: i32, w: i32) -> bool {
    const ARROW: [u8; 5] = [0b00100, 0b01110, 0b10101, 0b00100, 0b00100];
    let ox = (w - 5) / 2;
    let (cx, cy) = (x - ox, y);
    if (0..5).contains(&cx) && (0..5).contains(&cy) {
        return ARROW[cy as usize] & (1 << (4 - cx)) != 0;
    }
    cy == 5 && (-1..6).contains(&cx)
}

// ----------------------------------------------------------------------------------------
// Ящик

/// Области атласа ящика.
mod crate_map {
    use super::{region, Region};
    /// Шесть досок (полосы по 4 px: доска 2 px + поля).
    pub const PLANKS: Region = region(1.0, 1.0, 126.0, 24.0);
    /// Доски крышки (светлее, выгорели).
    pub const LID: Region = region(1.0, 27.0, 126.0, 8.0);
    /// Обвязка: горизонтальная и угловая (полосы по 4 px).
    pub const BATTEN: Region = region(1.0, 37.0, 126.0, 8.0);
    /// Торцы.
    pub const END: Region = region(1.0, 47.0, 8.0, 8.0);
    /// Железные уголки.
    pub const IRON: Region = region(11.0, 47.0, 8.0, 8.0);
    /// Тень между досками (внутренний короб).
    pub const DARK: Region = region(21.0, 47.0, 8.0, 8.0);
    /// Три доски с трафаретом (одна сторона), 6 px высотой.
    pub const STENCIL: Region = region(31.0, 47.0, 20.0, 6.0);
}

/// Ящик `size` (низ на y = 0): короб в тени, доски стен с просветами, обвязка по рёбрам,
/// раскос на каждой стене, железные уголки, крышка из других досок с двумя планками.
pub fn crate_mesh(size: Vec3, seed: u32) -> MeshOut {
    use crate_map::*;
    let mut m = MeshOut::default();
    let t = 0.018; // толщина доски
    let bt = 0.05; // ширина обвязки
    let bd = 0.022; // толщина обвязки
    // Стены — внутри, обвязка с уголками (5 мм) снаружи доходит ровно до `size`
    // (коллайдер = видимый ящик, соседние ящики не входят друг в друга).
    let (sx, sy, sz) = (size.x - 2.0 * bd - 0.01, size.y, size.z - 2.0 * bd - 0.01);
    // Короб внутри (виден в просветах).
    let dark = |_f: Face| (DARK, Vec2::ZERO);
    m.bar(Vec3::new(0.0, sy * 0.5, 0.0), Vec3::new(sx - 2.0 * t, sy - 2.0 * t, sz - 2.0 * t), Quat::IDENTITY, &dark);
    let plank_map = |strip: u32, shift: f32| {
        move |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (Region { y: PLANKS.y + strip as f32 * 4.0 + 1.0, h: 2.0, ..PLANKS }, Vec2::new(shift, 0.0)),
        }
    };
    // Стены: доски горизонтально. Стороны: (центр стены, наружу, вдоль, длина).
    let sides = [
        (Vec3::new(0.0, 0.0, sz * 0.5 - t * 0.5), Vec3::Z, sx),
        (Vec3::new(0.0, 0.0, -sz * 0.5 + t * 0.5), Vec3::NEG_Z, sx),
        (Vec3::new(sx * 0.5 - t * 0.5, 0.0, 0.0), Vec3::X, sz),
        (Vec3::new(-sx * 0.5 + t * 0.5, 0.0, 0.0), Vec3::NEG_X, sz),
    ];
    let rows = (sy / 0.125).round().max(2.0) as usize;
    let gap = 0.006;
    let ph = (sy - gap * (rows as f32 - 1.0)) / rows as f32;
    for (k, (c, out, len)) in sides.iter().enumerate() {
        let rot = Quat::from_rotation_y(out.x.atan2(out.z));
        // Доски кладутся до углов (на углах — обвязка).
        let length = len - 2.0 * t;
        for r in 0..rows {
            let y = r as f32 * (ph + gap) + ph * 0.5;
            let strip = (hash(k as i32, r as i32, seed) * 6.0) as u32 % 6;
            let shift = (hash(k as i32, r as i32, seed + 1) * 90.0).floor();
            // Одна сторона — с трафаретом (на трёх средних досках).
            let stenciled = k == 0 && rows >= 5 && length * DENSITY <= STENCIL.w + 0.5 && (rows / 2 - 1..=rows / 2 + 1).contains(&r);
            if stenciled {
                let band = (r + 1 - rows / 2) as f32; // 0..2, снизу вверх
                let centre = ((STENCIL.w - length * DENSITY) * 0.5).max(0.0).floor();
                let map = move |f: Face| match f {
                    Face::PosX | Face::NegX => (END, Vec2::ZERO),
                    Face::PosZ => (Region { y: STENCIL.y + (2.0 - band) * 2.0, h: 2.0, ..STENCIL }, Vec2::new(centre, 0.0)),
                    _ => (Region { y: PLANKS.y + 1.0, h: 2.0, ..PLANKS }, Vec2::ZERO),
                };
                m.bar(*c + Vec3::Y * y, Vec3::new(length, ph, t), rot, &map);
            } else {
                m.bar(*c + Vec3::Y * y, Vec3::new(length, ph, t), rot, &plank_map(strip, shift));
            }
        }
        // Обвязка: низ и верх стены, на досках.
        let batten = |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (Region { y: BATTEN.y + 1.0, h: 2.0, ..BATTEN }, Vec2::new((hash(k as i32, 5, seed) * 90.0).floor(), 0.0)),
        };
        let off = *out * (t * 0.5 + bd * 0.5);
        // До угловых брусков (их внутренняя грань — на `len / 2 − bt`).
        for y in [bt * 0.5, sy - bt * 0.5] {
            m.bar(*c + off + Vec3::Y * y, Vec3::new(len - 2.0 * bt, bt, bd), rot, &batten);
        }
        // Раскос по диагонали между обвязками, на 3 мм глубже лица обвязки.
        let (w, h) = (length - 2.0 * bt, sy - 2.0 * bt);
        let diag = (w * w + h * h).sqrt();
        let angle = h.atan2(w) * if k % 2 == 0 { 1.0 } else { -1.0 };
        let brace_rot = rot * Quat::from_rotation_z(angle);
        m.bar(*c + *out * (t * 0.5 + (bd - 0.003) * 0.5) + Vec3::Y * (sy * 0.5), Vec3::new(diag - bt * 0.5, bt * 0.9, bd - 0.003), brace_rot, &batten);
    }
    // Вертикальная обвязка на углах (поверх стен, квадратная).
    let iron_map = |_f: Face| (IRON, Vec2::ZERO);
    for (x, z) in [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let c = Vec3::new(x * (sx * 0.5 + bd * 0.5 - bt * 0.5), sy * 0.5, z * (sz * 0.5 + bd * 0.5 - bt * 0.5));
        // Вертикальный брусок: длинная сторона по локальной X, повёрнута вверх.
        let batten = |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (Region { y: BATTEN.y + 5.0, h: 2.0, ..BATTEN }, Vec2::new((hash(f as i32, 3, seed) * 90.0).floor(), 0.0)),
        };
        m.bar(c, Vec3::new(sy, bt + bd, bt + bd), Quat::from_rotation_z(PI * 0.5), &batten);
        // Уголки: на двух лицах вертикального бруска, у верха и низа.
        for y in [0.07, sy - 0.07] {
            for (n, along) in [(Vec3::new(x, 0.0, 0.0), Vec3::Z), (Vec3::new(0.0, 0.0, z), Vec3::X)] {
                let face = c + n * ((bt + bd) * 0.5 + 0.0025) + Vec3::Y * (y - sy * 0.5);
                let rot = Quat::from_rotation_y(n.x.atan2(n.z));
                let _ = along;
                m.bar(face, Vec3::new(bt + bd, 0.1, 0.005), rot, &iron_map);
            }
        }
    }
    // Крышка: доски вдоль X внутри стен, на 4 мм ниже их края; две планки поперёк.
    let (ix, iz) = (sx - 2.0 * t, sz - 2.0 * t);
    let lid_rows = (iz / 0.14).round().max(2.0) as usize;
    let lw = (iz - 0.005 * (lid_rows as f32 - 1.0)) / lid_rows as f32;
    let top = sy - 0.004;
    for r in 0..lid_rows {
        let z = -iz * 0.5 + r as f32 * (lw + 0.005) + lw * 0.5;
        let strip = r as u32 % 2;
        let map = move |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (Region { y: LID.y + strip as f32 * 4.0 + 1.0, h: 2.0, ..LID }, Vec2::new((hash(r as i32, 7, seed) * 90.0).floor(), 0.0)),
        };
        m.bar(Vec3::new(0.0, top - t * 0.5, z), Vec3::new(ix, t, lw), Quat::IDENTITY, &map);
    }
    m
}

/// Пиксельный атлас ящика.
pub fn crate_atlas(dir: &Path) -> Result<()> {
    use crate_map::*;
    let mut a = Atlas::new(WOOD);
    for strip in 0..6u32 {
        let shade = 1 + (strip as usize % 3);
        a.paint(Region { y: PLANKS.y + strip as f32 * 4.0, h: 4.0, ..PLANKS }, &move |x, y| plank_px(x, (y - 1).clamp(0, 1), 2, shade, 10 + strip, 0));
    }
    for strip in 0..2u32 {
        a.paint(Region { y: LID.y + strip as f32 * 4.0, h: 4.0, ..LID }, &move |x, y| plank_px(x, (y - 1).clamp(0, 1), 2, 3, 20 + strip, 0));
    }
    // Обвязка темнее досок, гвозди через 5 px.
    a.paint(Region { h: 4.0, ..BATTEN }, &|x, y| if y == 2 && x % 5 == 2 { IRONS } else { WOOD + 1 });
    a.paint(Region { y: BATTEN.y + 4.0, h: 4.0, ..BATTEN }, &|x, y| if y == 2 && x % 5 == 2 { IRONS } else { WOOD + 1 });
    a.paint(END, &|x, y| end_px(x, y, 8));
    a.paint(IRON, &|x, y| iron_px(x, y, 0, 30));
    a.paint(DARK, &|_, _| super::textures::PAL.len() - 1);
    // Трафарет: три доски, знак тёмной краской поверх.
    a.paint(STENCIL, &|x, y| if stencil_px(x, y, STENCIL.w as i32) { IRONS } else { plank_px(x, y % 2, 2, 2, 40, 0) });
    a.save(dir, "crate")
}

// ----------------------------------------------------------------------------------------
// Телега

mod cart_map {
    use super::{region, Region};
    /// Доски кузова и бортов: 4 полосы по 5 px (доска 3 px + поля).
    pub const PLANKS: Region = region(1.0, 1.0, 126.0, 20.0);
    /// Оглобли: 2.2 м × 0.08, к концу — затёртые руками рукояти.
    pub const SHAFT: Region = region(1.0, 23.0, 126.0, 4.0);
    /// Обод и спицы — тёмное дерево.
    pub const FELLOE: Region = region(1.0, 29.0, 126.0, 4.0);
    /// Шина и оковки.
    pub const IRON: Region = region(1.0, 35.0, 126.0, 4.0);
    pub const END: Region = region(1.0, 47.0, 8.0, 8.0);
    pub const HUB: Region = region(11.0, 47.0, 16.0, 8.0);
    pub const HUB_CAP: Region = region(29.0, 47.0, 8.0, 8.0);
    pub const BEAM: Region = region(1.0, 41.0, 126.0, 4.0);
}

/// Телега (корень — на земле под осью, оглобли к −X): кузов из досок на двух брусьях,
/// борта из двух досок со стойками и оковками на углах, ось, колёса со ступицей, десятью
/// спицами, ободом из двенадцати косяков и железной шиной (нижний сегмент лежит на земле),
/// оглобли с затёртыми рукоятями.
pub fn cart_mesh() -> MeshOut {
    use cart_map::*;
    let mut m = MeshOut::default();
    let strip = |i: u32, shift: f32| {
        move |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (Region { y: PLANKS.y + i as f32 * 5.0 + 1.0, h: 3.0, ..PLANKS }, Vec2::new(shift.floor(), 0.0)),
        }
    };
    let end_or = |r: Region| {
        move |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (r, Vec2::ZERO),
        }
    };
    // Брусья под кузовом (0.63..0.68), доски пола (0.68..0.73).
    for z in [-0.45, 0.45] {
        m.bar(Vec3::new(0.0, 0.655, z), Vec3::new(1.8, 0.05, 0.08), Quat::IDENTITY, &end_or(BEAM));
    }
    let n = 8;
    let w = (1.2 - 0.006 * (n as f32 - 1.0)) / n as f32;
    for i in 0..n {
        let z = -0.6 + i as f32 * (w + 0.006) + w * 0.5;
        m.bar(Vec3::new(0.0, 0.705, z), Vec3::new(1.8, 0.05, w), Quat::IDENTITY, &strip(i as u32 % 4, hash(i, 1, 5) * 90.0));
    }
    // Борта: две доски на сторону, стойки, оковки на углах.
    for (k, (c, along, len)) in [
        (Vec3::new(0.0, 0.0, 0.575), Vec3::X, 1.8f32),
        (Vec3::new(0.0, 0.0, -0.575), Vec3::X, 1.8),
        (Vec3::new(0.875, 0.0, 0.0), Vec3::Z, 1.1f32),
        (Vec3::new(-0.875, 0.0, 0.0), Vec3::Z, 1.1),
    ]
    .into_iter()
    .enumerate()
    {
        let rot = if along == Vec3::X { Quat::IDENTITY } else { Quat::from_rotation_y(PI * 0.5) };
        for (j, y) in [0.8, 0.95].into_iter().enumerate() {
            m.bar(c + Vec3::Y * y, Vec3::new(len, 0.14, 0.05), rot, &strip(((k + j) % 4) as u32, hash(k as i32, j as i32, 6) * 90.0));
        }
        // Оковки на концах борта: железная полоса, загнутая на 3 мм над доской.
        for e in [-1.0f32, 1.0] {
            let at = c + rot * Vec3::new(e * (len * 0.5 - 0.06), 0.875, 0.0);
            let out = rot * Vec3::Z * if c.z < 0.0 || c.x < 0.0 { -1.0 } else { 1.0 };
            m.bar(at + out * 0.0275, Vec3::new(0.05, 0.29, 0.005), rot, &|_f| (IRON, Vec2::new(20.0, 0.0)));
        }
    }
    // Ось и колёса.
    let along_z = Quat::IDENTITY;
    m.cylinder(Vec3::new(0.0, 0.45, 0.0), 0.05, 1.5, 10, along_z, IRON, HUB_CAP);
    for z in [-0.69f32, 0.69] {
        let c = Vec3::new(0.0, 0.45, z);
        m.cylinder(c, 0.085, 0.16, 12, along_z, HUB, HUB_CAP);
        // Железный колпак ступицы снаружи.
        m.cylinder(c + Vec3::Z * z.signum() * 0.09, 0.06, 0.02, 12, along_z, IRON, HUB_CAP);
        // Спицы: от ступицы до обода.
        for i in 0..10 {
            let a = i as f32 * TAU / 10.0 + 0.1;
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            let (r0, r1) = (0.08, 0.38);
            m.bar(c + dir * ((r0 + r1) * 0.5), Vec3::new(r1 - r0, 0.04, 0.035), Quat::from_rotation_z(a), &end_or(FELLOE));
        }
        // Обод (косяки) и шина: нижний сегмент — горизонтально у земли.
        let phase = -PI * 0.5;
        m.ring(c, 0.37, 0.43, 0.075, 12, phase, Quat::IDENTITY, &end_or(FELLOE));
        m.ring(c, 0.43, 0.45, 0.08, 12, phase, Quat::IDENTITY, &|_f| (IRON, Vec2::ZERO));
    }
    // Подушки оси.
    for z in [-0.5, 0.5] {
        m.bar(Vec3::new(0.0, 0.565, z), Vec3::new(0.14, 0.13, 0.1), Quat::IDENTITY, &end_or(BEAM));
    }
    // Оглобли: от низа передка до земли.
    for z in [-0.45, 0.45] {
        let t = 0.07;
        let a = Vec3::new(-0.88, 0.63 + t * 0.5, z);
        let b = Vec3::new(-2.25, t * 0.5, z);
        let d = b - a;
        let len = Vec2::new(d.x, d.y).length();
        let rot = Quat::from_rotation_z(d.y.atan2(d.x));
        // Конец у земли — затёртый: в атласе рукоять справа.
        let map = move |f: Face| match f {
            Face::PosX | Face::NegX => (END, Vec2::ZERO),
            _ => (Region { y: SHAFT.y + 1.0, h: 2.0, ..SHAFT }, Vec2::new((SHAFT.w - len * DENSITY).floor(), 0.0)),
        };
        m.bar((a + b) * 0.5, Vec3::new(len, t, t), rot, &map);
    }
    m
}

pub fn cart_atlas(dir: &Path) -> Result<()> {
    use cart_map::*;
    let mut a = Atlas::new(WOOD);
    for i in 0..4u32 {
        let shade = 1 + (i as usize % 3);
        a.paint(Region { y: PLANKS.y + i as f32 * 5.0, h: 5.0, ..PLANKS }, &move |x, y| plank_px(x, (y - 1).clamp(0, 2), 3, shade, 50 + i, 0));
    }
    // Оглобля: последние 7 px (≈ 45 см) — тёмная затёртая рукоять.
    let grip = SHAFT.w as i32 - 7;
    a.paint(SHAFT, &move |x, y| if x >= grip { WOOD } else { plank_px(x, (y - 1).clamp(0, 1), 2, 3, 60, 0) });
    a.paint(FELLOE, &|x, y| plank_px(x, y.min(3), 4, 1, 70, 0));
    a.paint(BEAM, &|x, y| plank_px(x, y.min(3), 4, 2, 80, 6));
    a.paint(IRON, &|x, y| iron_px(x, y, 6, 90));
    a.paint(END, &|x, y| end_px(x, y, 8));
    // Ступица: дерево с двумя железными обручами.
    a.paint(HUB, &|x, y| if y <= 1 || y >= 6 { IRONS } else { plank_px(x, y, 8, 2, 100, 0) });
    a.paint(HUB_CAP, &|x, y| iron_px(x, y, 0, 110));
    a.save(dir, "cart")
}
