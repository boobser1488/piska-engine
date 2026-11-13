//! Шум и генератор высот рельефа — перенос генератора из проекта-образца (`openworld`):
//! симплекс-шум с сидом (вход в f64, чтобы вдали от начала координат не терялась
//! точность), fBm, гребенчатый мультифрактал, искажение области, материки и море,
//! холмы, горные хребты под маской и выровненные площадки.
//!
//! Всё здесь — чистые функции от (сида, x, z): одинаковый результат в любом потоке,
//! у рендера, физики и скриптов.

const F2: f64 = 0.366_025_403_784_438_6; // (sqrt(3) - 1) / 2
const G2: f64 = 0.211_324_865_405_187_1; // (3 - sqrt(3)) / 6
const GRAD: [(f64, f64); 8] = [
    (1.0, 1.0),
    (-1.0, 1.0),
    (1.0, -1.0),
    (-1.0, -1.0),
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
];

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Двумерный симплекс-шум с перестановкой, перемешанной по сиду.
pub(crate) struct Simplex2 {
    perm: [u8; 512],
}

impl Simplex2 {
    pub fn new(seed: u64) -> Self {
        let mut p: [u8; 256] = std::array::from_fn(|i| i as u8);
        let mut s = seed;
        for i in (1..256).rev() {
            let j = (splitmix(&mut s) % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        let perm = std::array::from_fn(|i| p[i & 255]);
        Self { perm }
    }

    #[inline]
    fn grad(&self, i: usize, j: usize) -> (f64, f64) {
        GRAD[(self.perm[i + self.perm[j] as usize] & 7) as usize]
    }

    /// Примерно [-1, 1].
    pub fn get(&self, x: f64, y: f64) -> f64 {
        let s = (x + y) * F2;
        let i = (x + s).floor();
        let j = (y + s).floor();
        let t = (i + j) * G2;
        let x0 = x - (i - t);
        let y0 = y - (j - t);
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = x0 - i1 as f64 + G2;
        let y1 = y0 - j1 as f64 + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;
        let ii = (i as i64 & 255) as usize;
        let jj = (j as i64 & 255) as usize;

        let corner = |g: (f64, f64), x: f64, y: f64| {
            let t = 0.5 - x * x - y * y;
            if t <= 0.0 {
                0.0
            } else {
                let t2 = t * t;
                t2 * t2 * (g.0 * x + g.1 * y)
            }
        };
        let n0 = corner(self.grad(ii, jj), x0, y0);
        let n1 = corner(self.grad(ii + i1, jj + j1), x1, y1);
        let n2 = corner(self.grad(ii + 1, jj + 1), x2, y2);
        70.0 * (n0 + n1 + n2)
    }

    /// Фрактальное броуновское движение, нормировано примерно в [-1, 1].
    pub fn fbm(&self, x: f64, y: f64, octaves: u32) -> f64 {
        let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
        for o in 0..octaves.max(1) {
            let off = o as f64 * 17.31;
            sum += self.get(x * freq + off, y * freq - off) * amp;
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        sum / norm
    }

    /// Гребенчатый мультифрактал, примерно [0, 1]: острые гребни — горные хребты.
    pub fn ridged(&self, x: f64, y: f64, octaves: u32) -> f64 {
        let (mut sum, mut amp, mut freq, mut norm, mut weight) = (0.0, 0.5, 1.0, 0.0, 1.0);
        for o in 0..octaves.max(1) {
            let off = o as f64 * 31.7;
            let mut v = 1.0 - self.get(x * freq + off, y * freq - off * 0.3).abs();
            v = v * v * weight;
            weight = (v * 2.0).clamp(0.0, 1.0);
            sum += v * amp;
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        sum / norm
    }
}

pub(crate) fn smoothstep64(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Параметры генератора (поля компонента `Terrain`, влияющие на форму).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NoiseParams {
    pub seed: u64,
    /// Во сколько раз крупнее формы рельефа, чем в образце (1 — как в образце).
    pub scale: f64,
    /// Множитель высот.
    pub height: f32,
    /// Множитель высоты горных хребтов.
    pub mountains: f32,
    /// Октавы холмов (у хребтов — на одну больше).
    pub octaves: u32,
    /// Сдвиг «материк / море»: больше — больше суши (в образце 0.18).
    pub land_bias: f64,
    /// Выровненные площадки: (x, z, радиус) в метрах от угла рельефа.
    pub flat_zones: Vec<(f64, f64, f64)>,
    /// Ширина перехода от площадки к природному рельефу, м.
    pub flat_falloff: f64,
}

/// Детерминированное поле высот: море, равнины, холмы, хребты. Координаты — метры.
pub(crate) struct Generator {
    continent: Simplex2,
    warp_a: Simplex2,
    warp_b: Simplex2,
    hills: Simplex2,
    mountain: Simplex2,
    mountain_mask: Simplex2,
    detail: Simplex2,
    params: NoiseParams,
    /// Площадки с уже вычисленной высотой.
    flat: Vec<(f64, f64, f64, f32)>,
}

impl Generator {
    pub fn new(params: NoiseParams) -> Self {
        let seed = params.seed;
        let n = |k: u64| Simplex2::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(k));
        let mut generator = Self {
            continent: n(1),
            warp_a: n(2),
            warp_b: n(3),
            hills: n(4),
            mountain: n(5),
            mountain_mask: n(6),
            detail: n(7),
            params,
            flat: Vec::new(),
        };
        // Высота площадки — природная высота в её центре, но не ниже моря (+2 м), как в образце.
        let zones = generator.params.flat_zones.clone();
        for (x, z, radius) in zones {
            let h = generator.natural_height(x, z).max(2.0 * generator.params.height);
            generator.flat.push((x, z, radius.max(0.0), h));
        }
        generator
    }

    /// Высота в метрах в точке (x, z).
    pub fn height(&self, x: f64, z: f64) -> f32 {
        let mut h = self.natural_height(x, z);
        let falloff = self.params.flat_falloff.max(1e-3);
        for &(fx, fz, radius, fh) in &self.flat {
            let d = (x - fx).hypot(z - fz);
            if d < radius + falloff {
                let w = 1.0 - smoothstep64(radius, radius + falloff, d);
                h += (fh - h) * w as f32;
            }
        }
        h
    }

    fn natural_height(&self, x: f64, z: f64) -> f32 {
        let p = &self.params;
        // Координаты шума: крупнее масштаб — медленнее меняется рельеф.
        let k = 1.0 / p.scale.max(1e-3);
        let (sx, sz) = (x * k, z * k);
        // Искажение области ломает «сеточный» вид сырого шума.
        let wx = sx + self.warp_a.fbm(sx * 0.0007, sz * 0.0007, 3) * 150.0;
        let wz = sz + self.warp_b.fbm(sx * 0.0007, sz * 0.0007, 3) * 150.0;

        // Материки и море.
        let c = self.continent.fbm(wx * 0.000_22, wz * 0.000_22, 5) + p.land_bias;
        let land = smoothstep64(-0.12, 0.12, c);
        let mut h = -38.0 + land * 44.0;

        // Холмы, у побережья ниже.
        h += self.hills.fbm(wx * 0.0025, wz * 0.0025, p.octaves) * (6.0 + 22.0 * land);

        // Хребты — только вглубь суши и только там, где разрешает маска.
        let mask = smoothstep64(0.05, 0.45, c)
            * smoothstep64(-0.15, 0.35, self.mountain_mask.fbm(wx * 0.0004, wz * 0.0004, 3));
        if mask > 0.0 && p.mountains > 0.0 {
            let r = self.mountain.ridged(wx * 0.0009, wz * 0.0009, p.octaves + 1);
            h += r.powf(1.8) * 380.0 * mask * p.mountains as f64;
        }

        // Мелкие неровности (в метрах мира, не масштабируются).
        h += self.detail.fbm(x * 0.04, z * 0.04, 2) * 0.8;
        h as f32 * p.height
    }

    /// 0..1 — разнообразие для шейдера: граница травы, пятна, линия снега.
    pub fn variation(&self, x: f64, z: f64) -> f32 {
        let broad = self.detail.fbm(x * 0.006 + 100.0, z * 0.006 - 50.0, 3);
        let fine = self.hills.get(x * 0.05 - 300.0, z * 0.05 + 200.0);
        ((broad * 1.1 + fine * 0.25) * 0.5 + 0.5).clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(seed: u64) -> NoiseParams {
        NoiseParams {
            seed,
            scale: 1.0,
            height: 1.0,
            mountains: 1.0,
            octaves: 5,
            land_bias: 0.18,
            flat_zones: vec![(500.0, 500.0, 30.0)],
            flat_falloff: 40.0,
        }
    }

    #[test]
    fn simplex_is_bounded_and_seeded() {
        let a = Simplex2::new(1);
        let b = Simplex2::new(2);
        let mut differs = false;
        for i in 0..2000 {
            let (x, y) = (i as f64 * 0.37 - 300.0, i as f64 * 0.11 + 7.0);
            let v = a.get(x, y);
            assert!((-1.01..=1.01).contains(&v), "{v}");
            differs |= (v - b.get(x, y)).abs() > 1e-6;
        }
        assert!(differs);
    }

    #[test]
    fn flat_zone_is_level() {
        let generator = Generator::new(params(7));
        let centre = generator.height(500.0, 500.0);
        for (dx, dz) in [(0.0, 0.0), (10.0, 0.0), (-20.0, 5.0), (0.0, 29.0)] {
            assert!((generator.height(500.0 + dx, 500.0 + dz) - centre).abs() < 1e-4);
        }
        assert!(centre >= 2.0 - 1e-4, "flat zones stay above the sea");
    }
}
