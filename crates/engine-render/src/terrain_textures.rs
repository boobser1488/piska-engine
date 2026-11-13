//! Встроенные текстуры слоёв рельефа — генерируются кодом (как в образце), файлов нет:
//! трава (отдельные травинки поверх почвы), земля с камешками, скала с трещинами и
//! слоями, песок с рябью, снег и серая «деталь» (mod2x, как detail-текстуры Source).
//! Альбедо (альфа — высота для смешивания по высоте) + карта нормалей, все тайлятся.
//! Свои текстуры слоёв пользователя приводятся к тому же виду ([`from_image`]).

use std::f32::consts::TAU;
use std::sync::OnceLock;

/// Сторона текстуры слоя.
pub(crate) const TEX_SIZE: u32 = 256;
pub(crate) const LAYER_GRASS: u32 = 0;
pub(crate) const LAYER_DIRT: u32 = 1;
pub(crate) const LAYER_ROCK: u32 = 2;
pub(crate) const LAYER_SAND: u32 = 3;
pub(crate) const LAYER_SNOW: u32 = 4;
/// Слоёв в массиве: пять материалов и деталь.
pub(crate) const LAYER_TOTAL: u32 = 6;

/// Слой: альбедо RGBA8 (альфа — высота) и нормаль RGBA8, по `TEX_SIZE²` пикселей.
#[derive(Clone)]
pub(crate) struct LayerPixels {
    pub albedo: Vec<u8>,
    pub normal: Vec<u8>,
}

fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

fn hashf(x: i32, y: i32, seed: u32) -> f32 {
    (hash(x, y, seed) & 0xff_ffff) as f32 / 16_777_215.0
}

fn value_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (x0, y0) = (xf as i32, yf as i32);
    let (fx, fy) = (x - xf, y - yf);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let w = |a: i32, b: i32| hashf(a.rem_euclid(period), b.rem_euclid(period), seed);
    let top = w(x0, y0) + (w(x0 + 1, y0) - w(x0, y0)) * sx;
    let bot = w(x0, y0 + 1) + (w(x0 + 1, y0 + 1) - w(x0, y0 + 1)) * sx;
    top + (bot - top) * sy
}

/// Тайлящийся fBm в [0, 1].
fn fbm(u: f32, v: f32, period: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 0.5, 0.0);
    for o in 0..octaves {
        let p = period << o;
        sum += value_noise(u * p as f32, v * p as f32, p, seed.wrapping_add(o * 7919)) * amp;
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

/// Тайлящийся шум Ворлея: расстояния до ближайшей и второй точки (в клетках).
fn worley(u: f32, v: f32, cells: i32, seed: u32) -> (f32, f32) {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2) = (9.0f32, 9.0f32);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let (wx, wy) = (cx.rem_euclid(cells), cy.rem_euclid(cells));
            let px = cx as f32 + hashf(wx, wy, seed);
            let py = cy as f32 + hashf(wx, wy, seed ^ 0x9e37_79b9);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2)
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale3(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Альбедо (sRGB 0..1) и высота (0..1) материала в точке (u, v).
fn material(layer: u32, u: f32, v: f32) -> ([f32; 3], f32) {
    match layer {
        LAYER_GRASS => {
            // Почва под травинками; сами травинки — в `stamp_grass`.
            let n = fbm(u, v, 16, 4, 11);
            (mix3([0.10, 0.11, 0.05], [0.17, 0.17, 0.08], n), 0.1 * n)
        }
        LAYER_DIRT => {
            let n = fbm(u, v, 8, 5, 21);
            let fine = fbm(u, v, 64, 2, 23);
            let (f1, _) = worley(u, v, 32, 22);
            let pebble = (1.0 - f1 / 0.38).max(0.0);
            let mut c = mix3([0.30, 0.23, 0.13], [0.44, 0.35, 0.21], n);
            let stone = mix3([0.40, 0.38, 0.32], [0.55, 0.52, 0.45], fine);
            c = mix3(c, stone, smooth(0.0, 0.12, pebble));
            (scale3(c, 0.85 + 0.3 * fine), (0.35 * n + 0.65 * pebble.sqrt()).min(1.0))
        }
        LAYER_ROCK => {
            let n = fbm(u, v, 4, 6, 31);
            let (f1, f2) = worley(u, v, 6, 32);
            let crack = smooth(0.0, 0.07, f2 - f1);
            let strata = ((v * 18.0 + n * 3.0) * TAU).sin() * 0.5 + 0.5;
            let base = mix3([0.30, 0.29, 0.27], [0.52, 0.50, 0.46], n);
            let c = scale3(base, (0.85 + 0.15 * strata) * (0.4 + 0.6 * crack));
            (c, (0.7 * n + 0.3 * strata) * (0.3 + 0.7 * crack) + 0.2 * f1.min(1.0))
        }
        LAYER_SAND => {
            let n = fbm(u, v, 16, 4, 41);
            let warp = fbm(u, v, 4, 3, 42);
            let ripple = ((v * 16.0 + warp * 3.0) * TAU).sin() * 0.5 + 0.5;
            let grain = fbm(u, v, 256, 1, 43);
            let c = mix3([0.70, 0.62, 0.45], [0.84, 0.77, 0.60], n);
            (scale3(c, 0.9 + 0.08 * ripple + 0.1 * (grain - 0.5)), 0.6 * ripple + 0.4 * n)
        }
        LAYER_SNOW => {
            let n = fbm(u, v, 8, 5, 51);
            (mix3([0.80, 0.84, 0.90], [0.97, 0.98, 1.0], n), n)
        }
        _ => {
            // Деталь: серый шум около 0.5, в шейдере умножается на 2 (mod2x).
            let n = fbm(u, v, 32, 4, 61);
            let g = 0.5 + (n - 0.5) * 0.7;
            ([g, g, g], n)
        }
    }
}

fn normal_strength(layer: u32) -> f32 {
    match layer {
        LAYER_GRASS => 2.0,
        LAYER_DIRT => 3.5,
        LAYER_ROCK => 5.0,
        LAYER_SAND => 1.5,
        LAYER_SNOW => 1.2,
        _ => 1.5,
    }
}

/// Зерно на пиксель: чёткая мелкая фактура вместо мягких пятен.
fn grain_amount(layer: u32) -> f32 {
    match layer {
        LAYER_GRASS => 0.10,
        LAYER_DIRT => 0.16,
        LAYER_ROCK => 0.14,
        LAYER_SAND => 0.12,
        LAYER_SNOW => 0.04,
        _ => 0.0,
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Отдельные травинки (короткие штрихи) поверх почвы. Травинка перекрывает
/// предыдущие только там, где она «выше» — как газон сверху.
fn stamp_grass(color: &mut [[f32; 3]], height: &mut [f32], n: usize) {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let dark = [0.13, 0.22, 0.05];
    let mid = [0.24, 0.36, 0.09];
    let light = [0.40, 0.50, 0.17];
    let dry = [0.47, 0.45, 0.22];
    // Плотность травинок на пиксель — как в образце (70 000 на 512²).
    let blades = 70_000 * n * n / (512 * 512);
    let scale = n as f32 / 512.0;
    for _ in 0..blades {
        let (x0, y0) = (rng.next() * n as f32, rng.next() * n as f32);
        let angle = rng.next() * TAU;
        let len = (4.0 + rng.next() * 8.0) * scale.max(0.5);
        let base_h = 0.35 + 0.65 * rng.next();
        let tone = rng.next();
        let patch = fbm(x0 / n as f32, y0 / n as f32, 4, 3, 13);
        let mut c = if tone < 0.5 { mix3(dark, mid, tone * 2.0) } else { mix3(mid, light, tone * 2.0 - 1.0) };
        c = mix3(c, dry, smooth(0.55, 0.8, patch) * 0.5);
        let (dx, dy) = (angle.cos(), angle.sin());
        let steps = len.max(1.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let px = (x0 + dx * s as f32).rem_euclid(n as f32) as usize % n;
            let py = (y0 + dy * s as f32).rem_euclid(n as f32) as usize % n;
            let h = base_h * (0.55 + 0.45 * t);
            let i = py * n + px;
            if h > height[i] {
                height[i] = h;
                color[i] = scale3(c, 0.75 + 0.35 * t);
            }
        }
    }
}

/// Нормаль из поля высот (тайлится).
fn normals_from_height(height: &[f32], n: usize, strength: f32) -> Vec<u8> {
    let mut normal = vec![0u8; n * n * 4];
    let hs = |x: usize, y: usize| height[(y % n) * n + (x % n)];
    for y in 0..n {
        for x in 0..n {
            let dx = (hs(x + 1, y) - hs(x + n - 1, y)) * strength;
            let dy = (hs(x, y + 1) - hs(x, y + n - 1)) * strength;
            let len = (dx * dx + dy * dy + 1.0).sqrt();
            let v = [-dx / len, -dy / len, 1.0 / len];
            let i = (y * n + x) * 4;
            for k in 0..3 {
                normal[i + k] = ((v[k] * 0.5 + 0.5) * 255.0 + 0.5) as u8;
            }
            normal[i + 3] = 255;
        }
    }
    normal
}

/// Один встроенный слой.
fn generate_layer(layer: u32) -> LayerPixels {
    let n = TEX_SIZE as usize;
    let mut colors = vec![[0f32; 3]; n * n];
    let mut height = vec![0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let (c, h) = material(layer, x as f32 / n as f32, y as f32 / n as f32);
            colors[y * n + x] = c;
            height[y * n + x] = h;
        }
    }
    if layer == LAYER_GRASS {
        stamp_grass(&mut colors, &mut height, n);
    }
    let grain = grain_amount(layer);
    let mut albedo = vec![0u8; n * n * 4];
    for i in 0..n * n {
        let g = 1.0 + (hashf(i as i32, layer as i32, 977) - 0.5) * 2.0 * grain;
        for k in 0..3 {
            albedo[i * 4 + k] = ((colors[i][k] * g).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        albedo[i * 4 + 3] = (height[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    let normal = normals_from_height(&height, n, normal_strength(layer));
    LayerPixels { albedo, normal }
}

/// Все встроенные слои (считаются один раз на процесс; на десктопе — параллельно).
pub(crate) fn builtin_layers() -> &'static [LayerPixels] {
    static LAYERS: OnceLock<Vec<LayerPixels>> = OnceLock::new();
    LAYERS.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..LAYER_TOTAL).map(|l| scope.spawn(move || generate_layer(l))).collect();
                handles.into_iter().map(|h| h.join().expect("terrain texture thread")).collect()
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            (0..LAYER_TOTAL).map(generate_layer).collect()
        }
    })
}

/// Своя текстура слоя: приводится к `TEX_SIZE²`, высота — по яркости, нормаль — из неё.
pub(crate) fn from_image(image: &image::RgbaImage, layer: u32) -> LayerPixels {
    let n = TEX_SIZE as usize;
    let resized = image::imageops::resize(image, TEX_SIZE, TEX_SIZE, image::imageops::FilterType::Triangle);
    let mut albedo = resized.into_raw();
    let mut height = vec![0f32; n * n];
    for i in 0..n * n {
        let [r, g, b] = [albedo[i * 4], albedo[i * 4 + 1], albedo[i * 4 + 2]].map(|c| c as f32 / 255.0);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        height[i] = luma;
        albedo[i * 4 + 3] = (luma * 255.0 + 0.5) as u8;
    }
    let normal = normals_from_height(&height, n, normal_strength(layer));
    LayerPixels { albedo, normal }
}

/// Уменьшить RGBA8 вдвое (2×2 → 1); нормали перенормируются.
pub(crate) fn downsample(src: &[u8], size: usize, is_normal: bool) -> Vec<u8> {
    let half = (size / 2).max(1);
    let mut out = vec![0u8; half * half * 4];
    for y in 0..half {
        for x in 0..half {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(size - 1);
                let sy = (y * 2 + dy).min(size - 1);
                let i = (sy * size + sx) * 4;
                for k in 0..4 {
                    acc[k] += src[i + k] as u32;
                }
            }
            let o = (y * half + x) * 4;
            if is_normal {
                let v: [f32; 3] = std::array::from_fn(|k| acc[k] as f32 / 4.0 / 255.0 * 2.0 - 1.0);
                let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-5);
                for k in 0..3 {
                    out[o + k] = ((v[k] / len * 0.5 + 0.5) * 255.0 + 0.5) as u8;
                }
                out[o + 3] = 255;
            } else {
                for k in 0..4 {
                    out[o + k] = ((acc[k] + 2) / 4) as u8;
                }
            }
        }
    }
    out
}
