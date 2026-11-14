//! Текстуры карты — пиксель-арт, нарисованный кодом (свои, без чужих картинок).
//!
//! Один стиль на всё: **16 текселей на метр** (как у моделей Piska Modeler: 16 единиц на
//! метр, тексель на единицу), одна палитра ([`PAL`], 36 цветов: рампы по 3–4 оттенка —
//! песок, песчаник, штукатурка, мостовая, терракота, дерево, железо, ткань, ставни),
//! узор крупный и спокойный — кладка, доски, плиты, плетение; без шума и градиентов,
//! контраст внутри плитки умеренный (вдали мипмапы сводят его к ровному цвету, рябить
//! нечему). Фильтр в `.meta` — `Pixel`: вблизи резкие пиксели, вдали мипмапы.
//!
//! Большие поверхности кладутся в мировых координатах (`world_tiling` материала = период
//! текстуры в метрах, `size / period` = 16), поэтому пиксели одного размера на всех стенах
//! и полах и сетка не рвётся на стыках блоков.

use std::error::Error;
use std::path::Path;

use image::{Rgb, RgbImage};

/// Текселей на метр — везде.
pub const TEXELS_PER_METRE: u32 = 16;

/// Детерминированный хэш → 0..1.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Палитра карты (sRGB, 0..255). Индексы — константы ниже.
pub const PAL: [[u8; 3]; 36] = [
    // песок 0..3
    [196, 166, 118], [214, 184, 134], [228, 200, 150], [238, 214, 166],
    // песчаник 4..7
    [158, 124, 86], [182, 146, 104], [204, 168, 122], [222, 188, 140],
    // штукатурка 8..10 (светлая — цвет даёт материал)
    [206, 200, 186], [226, 221, 208], [240, 236, 226],
    // мостовая 11..14
    [134, 126, 110], [156, 148, 130], [176, 168, 148], [192, 184, 164],
    // терракота 15..17
    [150, 76, 46], [174, 92, 58], [192, 108, 70],
    // дерево 18..21
    [78, 52, 34], [110, 76, 48], [142, 102, 66], [170, 128, 86],
    // железо 22..24, ржавчина 25
    [44, 42, 44], [70, 68, 70], [102, 100, 102], [122, 70, 40],
    // ткань: красная 26..27, светлая 28..29, синяя 30..31
    [158, 52, 40], [182, 64, 48], [222, 208, 180], [236, 224, 198], [46, 86, 138], [60, 104, 160],
    // ставни (светлые, цвет даёт материал) 32..33, затирка 34, тень 35
    [196, 200, 204], [228, 232, 236], [176, 160, 136], [40, 30, 24],
];

const SAND: usize = 0;
const STONE: usize = 4;
const PLASTER: usize = 8;
const PAVE: usize = 11;
const TERRA: usize = 15;
pub const WOOD: usize = 18;
pub const IRON: usize = 22;
pub const RUST: usize = 25;
const RED: usize = 26;
const CREAM: usize = 28;
const BLUE: usize = 30;
const SHUTTER: usize = 32;
const GROUT: usize = 34;

fn canvas(w: u32, h: u32, f: &dyn Fn(i32, i32) -> usize) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.put_pixel(x, y, Rgb(PAL[f(x as i32, y as i32)]));
        }
    }
    img
}

/// Перевязка рядами в пикселях (кладка, плиты): высота ряда, длины камней, замкнуто по
/// ширине `size`. Для точки — (ряд, камень, x от начала камня, длина камня, y от верха ряда).
struct Bond {
    size: i32,
    row: i32,
    cuts: Vec<Vec<i32>>,
}

impl Bond {
    fn new(size: i32, row: i32, len: (i32, i32), seed: u32) -> Self {
        let rows = size / row;
        let cuts = (0..rows)
            .map(|r| {
                let mut xs = Vec::new();
                let start = (hash(r, 7, seed) * len.1 as f32) as i32;
                let mut x = start;
                let mut k = 0;
                while x < start + size - len.0 / 2 {
                    xs.push(x.rem_euclid(size));
                    x += len.0 + (hash(r, k, seed + 3) * (len.1 - len.0 + 1) as f32) as i32;
                    k += 1;
                }
                xs.sort_unstable();
                xs
            })
            .collect();
        Self { size, row, cuts }
    }

    fn at(&self, x: i32, y: i32) -> (i32, i32, i32, i32, i32) {
        let r = (y / self.row).min(self.cuts.len() as i32 - 1);
        let cuts = &self.cuts[r as usize];
        let i = cuts.partition_point(|c| *c <= x);
        let (left, stone) = if i == 0 { (cuts[cuts.len() - 1] - self.size, cuts.len() - 1) } else { (cuts[i - 1], i - 1) };
        let right = if i < cuts.len() { cuts[i] } else { cuts[0] + self.size };
        (r, stone as i32, x - left, right - left, y - r * self.row)
    }
}

/// Кладка из песчаника: ряды 4 px (25 см), камни 5–10 px, шов — тёмный оттенок рампы
/// снизу и справа (один пиксель), камень — один из трёх оттенков, редкая щербинка.
fn masonry(size: u32, seed: u32) -> RgbImage {
    let bond = Bond::new(size as i32, 4, (5, 10), seed);
    canvas(size, size, &|x, y| {
        let (r, s, dx, len, dy) = bond.at(x, y);
        if dy == 3 || dx == len - 1 {
            return STONE;
        }
        if hash(x, y, seed + 5) > 0.97 {
            return STONE + 1;
        }
        STONE + 1 + (hash(r, s, seed) * 3.0) as usize
    })
}

/// Штукатурка: ровная светлая, пятна 2×2 px полутоном, редкая отбитая заплата с кладкой.
fn plaster(size: u32, seed: u32) -> RgbImage {
    let brick = Bond::new(size as i32, 2, (3, 5), seed + 50);
    canvas(size, size, &|x, y| {
        let (cx, cy) = (x.div_euclid(6), y.div_euclid(6));
        // Отбито: в одной клетке 6×6 из сорока — заплата 4×3 кладки.
        if hash(cx, cy, seed + 9) > 0.975 {
            let (lx, ly) = (x - cx * 6, y - cy * 6);
            if (1..5).contains(&lx) && (1..4).contains(&ly) {
                let (_, _, dx, len, dy) = brick.at(x, y);
                return if dy == 1 || dx == len - 1 { STONE + 1 } else { STONE + 2 };
            }
        }
        if hash(x.div_euclid(2), y.div_euclid(2), seed) > 0.9 {
            PLASTER + 1
        } else if hash(x, y, seed + 3) > 0.995 {
            PLASTER
        } else {
            PLASTER + 2
        }
    })
}

/// Плиты: ряды `row` px, длины `len`, шов — пиксель тёмного оттенка, плита — один из
/// трёх оттенков, щербинки.
fn slabs(size: u32, row: i32, len: (i32, i32), seed: u32) -> RgbImage {
    let bond = Bond::new(size as i32, row, len, seed);
    canvas(size, size, &|x, y| {
        let (r, s, dx, len, dy) = bond.at(x, y);
        if dy == row - 1 || dx == len - 1 {
            return PAVE;
        }
        if hash(x, y, seed + 5) > 0.975 {
            return PAVE + 1;
        }
        PAVE + 1 + (hash(r, s, seed) * 3.0) as usize
    })
}

/// Булыжник: камни 4 × 3–4 px (25 см) со смещением рядов, шов — песок.
fn cobbles(size: u32, seed: u32) -> RgbImage {
    let bond = Bond::new(size as i32, 4, (3, 4), seed);
    canvas(size, size, &|x, y| {
        let (r, s, dx, len, dy) = bond.at(x, y);
        if dy == 3 || dx == len - 1 {
            return SAND;
        }
        PAVE + 1 + (hash(r, s, seed) * 3.0) as usize
    })
}

/// Терракотовая плитка 50 см (8 px) с затиркой в пиксель.
fn tiles(size: u32, seed: u32) -> RgbImage {
    canvas(size, size, &|x, y| {
        if x % 8 == 7 || y % 8 == 7 {
            return GROUT;
        }
        let shade = (hash(x / 8, y / 8, seed) * 3.0) as usize;
        if hash(x, y, seed + 4) > 0.97 {
            TERRA
        } else {
            TERRA + shade
        }
    })
}

/// Песок: ровный, редкие пиксели светлее и темнее, пологая рябь — пунктир через 6 px.
fn sand(size: u32, seed: u32) -> RgbImage {
    canvas(size, size, &|x, y| {
        let ripple = (y + (x / 9) % 2) % 6 == 0 && hash(x / 3, y, seed) > 0.4;
        let h = hash(x, y, seed + 1);
        if ripple {
            SAND + 1
        } else if h > 0.97 {
            SAND
        } else if h < 0.03 {
            SAND + 3
        } else {
            SAND + 2
        }
    })
}

/// Доски 3 px (19 см) + щель в пиксель, стыки досок в разных местах, редкое волокно на тон
/// темнее.
fn planks(size: u32, seed: u32) -> RgbImage {
    canvas(size, size, &|x, y| {
        let p = y / 4;
        if y % 4 == 3 {
            return WOOD;
        }
        let joint = (hash(p, 9, seed) * size as f32) as i32;
        let dj = (x - joint).rem_euclid(size as i32);
        if dj == 0 {
            return WOOD;
        }
        let shade = 1 + (hash(p, 3, seed) * 3.0) as usize;
        if hash(x / 3, y, seed + 7) > 0.9 {
            WOOD + shade - 1
        } else {
            WOOD + shade
        }
    })
}

/// Гофрированное железо: полосы по 2 px (12.5 см) двух оттенков, ржавые пятна.
fn corrugated(size: u32, seed: u32) -> RgbImage {
    canvas(size, size, &|x, y| {
        if hash(x.div_euclid(3), y.div_euclid(4), seed) > 0.85 {
            return RUST;
        }
        if (x / 2) % 2 == 0 {
            IRON + 2
        } else {
            IRON + 1
        }
    })
}

/// Полосатая ткань: полосы по `band` px, плетение — каждый четвёртый пиксель на тон темнее.
fn stripes(size: u32, band: i32, a: usize, b: usize) -> RgbImage {
    canvas(size, size, &|x, y| {
        let base = if (x / band) % 2 == 0 { a } else { b };
        if (x + y) % 4 == 0 {
            base
        } else {
            base + 1
        }
    })
}

/// Створка ставни (по UV на всю створку 0.4 × 1.2 м → 6 × 19 px): рамка в пиксель,
/// ламели через строку.
fn louvre() -> RgbImage {
    let (w, h) = (6, 19);
    canvas(w, h, &|x, y| {
        if x == 0 || x == w as i32 - 1 || y == 0 || y == h as i32 - 1 || y == 9 || y % 2 == 0 {
            SHUTTER
        } else {
            SHUTTER + 1
        }
    })
}

/// (имя, картинка, период в метрах; 0 — по UV).
pub fn list() -> Vec<(&'static str, RgbImage, f32)> {
    let m = TEXELS_PER_METRE;
    vec![
        ("sandstone", masonry(2 * m, 1), 2.0),
        ("plaster", plaster(4 * m, 2), 4.0),
        ("paving", slabs(4 * m, 8, (8, 12), 3), 4.0),
        ("flagstone", slabs(4 * m, 16, (12, 18), 10), 4.0),
        ("cobbles", cobbles(2 * m, 11), 2.0),
        ("tiles", tiles(2 * m, 4), 2.0),
        ("sand", sand(4 * m, 5), 4.0),
        ("planks", planks(2 * m, 6), 2.0),
        ("metal", corrugated(2 * m, 7), 2.0),
        ("cloth", stripes(2 * m, 4, RED, CREAM), 2.0),
        ("cloth_blue", stripes(2 * m, 4, BLUE, CREAM), 2.0),
        ("louvre", louvre(), 0.0),
    ]
}

/// Нарисовать все текстуры в `assets/textures/`.
pub fn write_all(assets: &Path) -> Result<Vec<&'static str>, Box<dyn Error>> {
    let dir = assets.join("textures");
    std::fs::create_dir_all(&dir)?;
    let mut names = Vec::new();
    for (name, image, _) in list() {
        image.save(dir.join(format!("{name}.png")))?;
        names.push(name);
    }
    Ok(names)
}

/// Фильтр `Pixel` в `.meta` всех картинок `textures/` (GUID не меняется): резкие пиксели
/// вблизи, мипмапы вдали. `.meta` к этому времени уже созданы базой ассетов.
pub fn pixel_filter(assets: &Path) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(assets.join("textures"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "meta") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let fixed = if text.contains("\"filter\"") {
            text.replace("Enum(\"Linear\")", "Enum(\"Pixel\")").replace("Enum(\"Nearest\")", "Enum(\"Pixel\")")
        } else if let Some(at) = text.rfind(')') {
            let body = text[..at].trim_end().trim_end_matches(',');
            format!("{body},\n    settings: {{\n        \"filter\": Enum(\"Pixel\"),\n    }},\n)\n")
        } else {
            text.clone()
        };
        if fixed != text {
            std::fs::write(&path, fixed)?;
        }
    }
    Ok(())
}
