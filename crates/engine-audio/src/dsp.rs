//! Цифровая обработка сигнала: ограниченные по полосе осцилляторы, фильтры,
//! измерение громкости (LUFS), лимитер мастер-шины, кубическая интерполяция.
//!
//! Всё здесь — чистые функции и маленькие структуры без выделений памяти в горячем
//! цикле: ими пользуются и синтезатор (оффлайн), и микшер (поток звука).

use std::f32::consts::PI;
use std::sync::OnceLock;

// ----------------------------------------------------------------------
// Ограниченные по полосе осцилляторы (таблицы волн по октавам)
// ----------------------------------------------------------------------

/// Длина одной таблицы (степень двойки). 4096 точек на период — с запасом в 8 раз
/// для самой «густой» таблицы (512 гармоник), так что линейной интерполяции хватает.
const TABLE_LEN: usize = 4096;
const TABLE_MASK: usize = TABLE_LEN - 1;
/// Уровней: на уровне `j` — гармоники 1..=2^j. Верхний — 512 гармоник.
const LEVELS: usize = 10;
/// Доля частоты дискретизации, выше которой гармоники не синтезируются (0.45·fs —
/// 19.8 кГц при 44.1 кГц): ни одна гармоника не отражается ниже Найквиста.
const BAND_EDGE: f32 = 0.45;

struct WaveTables {
    saw: Vec<f32>,
    triangle: Vec<f32>,
}

fn tables() -> &'static WaveTables {
    static TABLES: OnceLock<WaveTables> = OnceLock::new();
    TABLES.get_or_init(build_tables)
}

/// Таблицы считаются один раз: ряд Фурье с теми же фазами, что у «наивных» волн
/// (`2p-1` у пилы, `1-4|p-0.5|` у треугольника), чтобы звук совпадал по форме.
fn build_tables() -> WaveTables {
    let sin: Vec<f64> = (0..TABLE_LEN)
        .map(|n| (std::f64::consts::TAU * n as f64 / TABLE_LEN as f64).sin())
        .collect();
    let mut saw = vec![0.0f32; TABLE_LEN * LEVELS];
    let mut triangle = vec![0.0f32; TABLE_LEN * LEVELS];
    let mut acc_saw = vec![0.0f64; TABLE_LEN];
    let mut acc_tri = vec![0.0f64; TABLE_LEN];
    let mut done = 0usize;
    for level in 0..LEVELS {
        let harmonics = 1usize << level;
        for k in done + 1..=harmonics {
            let saw_amp = -2.0 / (std::f64::consts::PI * k as f64);
            let tri_amp = if k % 2 == 1 {
                -8.0 / (std::f64::consts::PI * std::f64::consts::PI * (k * k) as f64)
            } else {
                0.0
            };
            for n in 0..TABLE_LEN {
                let s = sin[(k * n) & TABLE_MASK];
                acc_saw[n] += saw_amp * s;
                if tri_amp != 0.0 {
                    // cos(x) = sin(x + π/2): сдвиг на четверть таблицы.
                    acc_tri[n] += tri_amp * sin[(k * n + TABLE_LEN / 4) & TABLE_MASK];
                }
            }
        }
        done = harmonics;
        let base = level * TABLE_LEN;
        for n in 0..TABLE_LEN {
            saw[base + n] = acc_saw[n] as f32;
            triangle[base + n] = acc_tri[n] as f32;
        }
    }
    WaveTables { saw, triangle }
}

/// Какие два уровня смешать для частоты `freq` и с каким весом второго.
/// Оба уровня содержат только гармоники ниже `BAND_EDGE·rate`, поэтому наложения нет,
/// а смешивание по логарифму частоты убирает ступеньки тембра на свипах.
/// `None` — даже основной тон выше границы полосы (звучит тишина).
#[inline]
fn level_pair(freq: f32, rate: f32) -> Option<(usize, usize, f32)> {
    let freq = freq.abs();
    if freq <= 0.0 {
        return Some((LEVELS - 1, LEVELS - 1, 0.0));
    }
    let allowed = BAND_EDGE * rate / freq;
    if allowed < 1.0 {
        return None;
    }
    let l = allowed.log2();
    let j = l.floor() as usize;
    if j >= LEVELS {
        return Some((LEVELS - 1, LEVELS - 1, 0.0));
    }
    let lo = j.saturating_sub(1);
    Some((lo, j, if j == 0 { 0.0 } else { l - j as f32 }))
}

#[inline]
fn table_read(table: &[f32], level: usize, phase: f32) -> f32 {
    let x = phase * TABLE_LEN as f32;
    let i = x as usize;
    let frac = x - i as f32;
    let base = level * TABLE_LEN;
    let a = table[base + (i & TABLE_MASK)];
    let b = table[base + ((i + 1) & TABLE_MASK)];
    a + (b - a) * frac
}

#[inline]
fn blended(table: &[f32], phase: f32, freq: f32, rate: f32) -> f32 {
    let Some((lo, hi, w)) = level_pair(freq, rate) else {
        return 0.0;
    };
    let phase = phase - phase.floor();
    let a = table_read(table, lo, phase);
    if w <= 0.0 || lo == hi {
        return a;
    }
    a + (table_read(table, hi, phase) - a) * w
}

/// Пила без наложения спектров: форма `2p-1` (фаза `p` в 0..1).
#[inline]
pub fn bl_saw(phase: f32, freq: f32, rate: f32) -> f32 {
    blended(&tables().saw, phase, freq, rate)
}

/// Треугольник без наложения спектров: форма `1-4|p-0.5|`.
#[inline]
pub fn bl_triangle(phase: f32, freq: f32, rate: f32) -> f32 {
    blended(&tables().triangle, phase, freq, rate)
}

/// Прямоугольник со скважностью `duty` без наложения спектров и без постоянной
/// составляющей: разность двух пил. Размах 2, как у ±1-меандра; при `duty` 0.5 —
/// ровно меандр ±1.
#[inline]
pub fn bl_pulse(phase: f32, duty: f32, freq: f32, rate: f32) -> f32 {
    let t = &tables().saw;
    blended(t, phase - duty, freq, rate) - blended(t, phase, freq, rate)
}

// ----------------------------------------------------------------------
// Фильтр: двухполюсный SVF (топология TPT — устойчив при быстрой модуляции среза)
// ----------------------------------------------------------------------

/// Коэффициенты фильтра для среза `cutoff` (Гц) и добротности `q`.
#[derive(Clone, Copy, Debug)]
pub struct SvfCoefs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoefs {
    /// Срез ограничивается 0.45·fs: выше фильтр TPT теряет смысл (тангенс растёт).
    pub fn new(cutoff: f32, q: f32, rate: f32) -> Self {
        let rate = rate.max(1.0);
        Self::normalized(cutoff.max(5.0) / rate, q)
    }

    /// По нормированной частоте среза (доля частоты дискретизации, 0..0.45).
    pub fn normalized(cutoff: f32, q: f32) -> Self {
        let fc = if cutoff.is_finite() { cutoff.clamp(1e-5, 0.45) } else { 0.45 };
        let g = (PI * fc).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { a1, a2, a3, k }
    }
}

/// Состояние одного фильтра.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// Состояние «как будто на входе давно стоит `x`»: включение без переходного щелчка.
    pub fn settled(x: f32) -> Self {
        Self { ic1: 0.0, ic2: x }
    }

    /// Шаг фильтра: (низкие, полоса, высокие).
    #[inline]
    pub fn tick(&mut self, c: &SvfCoefs, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - c.k * v1 - v2)
    }

    #[inline]
    pub fn lowpass(&mut self, c: &SvfCoefs, x: f32) -> f32 {
        self.tick(c, x).0
    }

    #[inline]
    pub fn highpass(&mut self, c: &SvfCoefs, x: f32) -> f32 {
        self.tick(c, x).2
    }
}

/// Добротность Баттерворта для двухполюсного фильтра.
pub const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

// ----------------------------------------------------------------------
// Громкость: K-взвешивание BS.1770 и интегральная громкость со стробированием
// ----------------------------------------------------------------------

struct Biquad {
    b: [f64; 3],
    a: [f64; 3],
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 3]) -> Self {
        let n = a[0];
        Self {
            b: [b[0] / n, b[1] / n, b[2] / n],
            a: [1.0, a[1] / n, a[2] / n],
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z1;
        self.z1 = self.b[1] * x - self.a[1] * y + self.z2;
        self.z2 = self.b[2] * x - self.a[2] * y;
        y
    }
}

/// Два звена K-фильтра (полка +4 дБ выше ~1.7 кГц и ФВЧ 38 Гц) для частоты `rate`.
/// Те же формулы, что у `tools/audio_report.py` (и у pyloudnorm): синус 997 Гц 0 дБFS = −3.01 LUFS.
fn k_filter(rate: f64) -> (Biquad, Biquad) {
    let (g, q, fc) = (3.999_843_853_973_347, 0.707_175_236_955_419_6, 1_681.974_450_955_533);
    let k = (std::f64::consts::PI * fc / rate).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let shelf = Biquad::new(
        [vh + vb * k / q + k * k, 2.0 * (k * k - vh), vh - vb * k / q + k * k],
        [1.0 + k / q + k * k, 2.0 * (k * k - 1.0), 1.0 - k / q + k * k],
    );
    let (fc, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (std::f64::consts::PI * fc / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = Biquad::new([a0, -2.0 * a0, a0], [a0, 2.0 * (k * k - 1.0), 1.0 - k / q + k * k]);
    (shelf, hp)
}

/// Интегральная громкость в LUFS (BS.1770: K-взвешивание, блоки 400 мс с шагом 100 мс,
/// стробирование −70 LUFS и −10 LU). `samples` — чередующиеся каналы. Звук короче
/// 400 мс меряется одним блоком на всю длину. Тишина — `f32::NEG_INFINITY`.
pub fn integrated_loudness(samples: &[f32], channels: usize, rate: u32) -> f32 {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    if frames == 0 {
        return f32::NEG_INFINITY;
    }
    // Мощность K-взвешенного сигнала по кадрам (сумма по каналам).
    let mut power = vec![0.0f64; frames];
    for c in 0..channels {
        let (mut shelf, mut hp) = k_filter(rate as f64);
        for (i, p) in power.iter_mut().enumerate() {
            let y = hp.tick(shelf.tick(samples[i * channels + c] as f64));
            *p += y * y;
        }
    }
    let block = ((0.4 * rate as f64) as usize).clamp(1, frames);
    let hop = (block / 4).max(1);
    // Префиксные суммы — средняя мощность любого блока за O(1).
    let mut prefix = Vec::with_capacity(frames + 1);
    prefix.push(0.0f64);
    let mut acc = 0.0;
    for p in &power {
        acc += p;
        prefix.push(acc);
    }
    let blocks: Vec<f64> = (0..=frames - block)
        .step_by(hop)
        .map(|s| (prefix[s + block] - prefix[s]) / block as f64)
        .collect();
    let to_lufs = |z: f64| -0.691 + 10.0 * z.max(1e-20).log10();
    let abs: Vec<f64> = blocks.iter().copied().filter(|&z| to_lufs(z) > -70.0).collect();
    if abs.is_empty() {
        return f32::NEG_INFINITY;
    }
    let relative = to_lufs(abs.iter().sum::<f64>() / abs.len() as f64) - 10.0;
    let gated: Vec<f64> = abs.into_iter().filter(|&z| to_lufs(z) > relative).collect();
    to_lufs(gated.iter().sum::<f64>() / gated.len() as f64) as f32
}

// ----------------------------------------------------------------------
// Интерполяция и мелочи
// ----------------------------------------------------------------------

/// Кубическая интерполяция Катмулла — Рома между `y1` и `y2` (`t` в 0..1).
#[inline]
pub fn cubic(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// Равномощная кривая для затуханий: 0 → 0, 1 → 1, середина ≈ 0.69 (≈ −3 дБ), монотонна.
/// Многочлен вместо синуса — дёшево в потоке звука.
#[inline]
pub fn equal_power(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * (1.5 - 0.5 * x * x)
}

/// Мягкое ограничение: линейно до `knee`, выше плавно (tanh) подходит к 1.0.
#[inline]
pub fn soft_clip(x: f32, knee: f32) -> f32 {
    let a = x.abs();
    if a <= knee {
        return x;
    }
    let room = 1.0 - knee;
    let y = knee + room * ((a - knee) / room).tanh();
    y.copysign(x)
}

// ----------------------------------------------------------------------
// Лимитер мастер-шины с заглядыванием вперёд
// ----------------------------------------------------------------------

/// Длина заглядывания (кадров). Она же — задержка выхода и самое короткое время, за
/// которое лимитер меняет усиление на всю глубину (1.3 мс при 48 кГц).
pub const LIMITER_LOOKAHEAD: usize = 64;

/// Пиковый лимитер с заглядыванием вперёд: выход никогда не превышает `ceiling`,
/// а усиление меняется плавно (не быстрее 1/`LIMITER_LOOKAHEAD` на кадр) — без
/// «жёсткого» клиппирования и щелчков. Пока сигнал ниже порога, лимитер
/// прозрачен бит в бит (только задержка).
pub struct Limiter {
    ceiling: f32,
    release: f32,
    /// Задержка звука: кольцо на `LIMITER_LOOKAHEAD - 1` кадров.
    delay: [[f32; 2]; LIMITER_LOOKAHEAD - 1],
    delay_pos: usize,
    /// Скользящий минимум требуемого усиления за окно (монотонная очередь на кольце).
    min_val: [f32; LIMITER_LOOKAHEAD],
    min_idx: [u64; LIMITER_LOOKAHEAD],
    min_head: usize,
    min_len: usize,
    /// Удержание с плавным отпусканием.
    held: f32,
    /// Скользящее среднее удержания (сглаживает атаку).
    boxcar: [f32; LIMITER_LOOKAHEAD],
    box_pos: usize,
    box_sum: f64,
    frame: u64,
    gain: f32,
}

impl Limiter {
    /// `ceiling` — потолок (линейный, например 0.891 = −1 дБFS); `release_s` — время
    /// возврата усиления, секунд.
    pub fn new(ceiling: f32, release_s: f32, rate: u32) -> Self {
        let mut l = Self {
            ceiling: ceiling.clamp(0.01, 1.0),
            release: 0.0,
            delay: [[0.0; 2]; LIMITER_LOOKAHEAD - 1],
            delay_pos: 0,
            min_val: [1.0; LIMITER_LOOKAHEAD],
            min_idx: [0; LIMITER_LOOKAHEAD],
            min_head: 0,
            min_len: 0,
            held: 1.0,
            boxcar: [1.0; LIMITER_LOOKAHEAD],
            box_pos: 0,
            box_sum: LIMITER_LOOKAHEAD as f64,
            frame: 0,
            gain: 1.0,
        };
        l.set_release(release_s, rate);
        l
    }

    pub fn set_release(&mut self, release_s: f32, rate: u32) {
        self.release = (-1.0 / (release_s.max(0.001) * rate.max(1) as f32)).exp();
    }

    pub fn set_ceiling(&mut self, ceiling: f32) {
        self.ceiling = ceiling.clamp(0.01, 1.0);
    }

    pub fn ceiling(&self) -> f32 {
        self.ceiling
    }

    /// Текущее усиление (1.0 — лимитер не работает).
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Обработать один стерео-кадр (на выходе — кадр `LIMITER_LOOKAHEAD - 1` назад).
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        let need = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };

        // Скользящий минимум за последние LIMITER_LOOKAHEAD кадров.
        let n = self.frame;
        while self.min_len > 0 {
            let back = (self.min_head + self.min_len - 1) % LIMITER_LOOKAHEAD;
            if self.min_val[back] >= need {
                self.min_len -= 1;
            } else {
                break;
            }
        }
        if self.min_len > 0 && self.min_idx[self.min_head] + LIMITER_LOOKAHEAD as u64 <= n {
            self.min_head = (self.min_head + 1) % LIMITER_LOOKAHEAD;
            self.min_len -= 1;
        }
        let slot = (self.min_head + self.min_len) % LIMITER_LOOKAHEAD;
        self.min_val[slot] = need;
        self.min_idx[slot] = n;
        self.min_len += 1;
        let window_min = self.min_val[self.min_head];

        // Мгновенно вниз, плавно вверх.
        if window_min < self.held {
            self.held = window_min;
        } else {
            self.held = window_min + (self.held - window_min) * self.release;
        }

        // Сглаживание прямоугольным окном той же длины: к моменту выхода пика
        // среднее не больше требуемого усиления.
        self.box_sum += self.held as f64 - self.boxcar[self.box_pos] as f64;
        self.boxcar[self.box_pos] = self.held;
        self.box_pos = (self.box_pos + 1) % LIMITER_LOOKAHEAD;
        let gain = ((self.box_sum / LIMITER_LOOKAHEAD as f64) as f32).min(1.0);
        self.gain = gain;

        let [dl, dr] = self.delay[self.delay_pos];
        self.delay[self.delay_pos] = [l, r];
        self.delay_pos = (self.delay_pos + 1) % (LIMITER_LOOKAHEAD - 1);
        self.frame += 1;
        if gain >= 1.0 {
            (dl, dr)
        } else {
            (dl * gain, dr * gain)
        }
    }

}

/// Прижать пики буфера (чередующиеся каналы) к `ceiling` лимитером с заглядыванием,
/// считая буфер петлёй: сначала прогон по кругу для «разогрева», затем результат с
/// компенсацией задержки — стык петли остаётся бесшовным. Для разовых звуков с тихим
/// концом это то же самое, что обычная обработка.
pub fn limit_loop(samples: &mut [f32], channels: usize, ceiling: f32, rate: u32) {
    let ch = channels.max(1);
    let frames = samples.len() / ch;
    if frames == 0 {
        return;
    }
    let mut lim = Limiter::new(ceiling, 0.05, rate);
    let frame = |s: &[f32], i: usize| {
        let b = (i % frames) * ch;
        let l = s[b];
        (l, if ch > 1 { s[b + 1] } else { l })
    };
    for i in 0..frames {
        let (l, r) = frame(samples, i);
        lim.process(l, r);
    }
    let src = samples.to_vec();
    let delay = LIMITER_LOOKAHEAD - 1;
    for i in 0..frames {
        let (l, r) = frame(&src, i);
        lim.process(l, r);
        let g = lim.gain();
        let j = (i + frames * (delay / frames + 1) - delay) % frames;
        for c in 0..ch {
            samples[j * ch + c] = src[j * ch + c] * g;
        }
    }
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > ceiling {
        let k = ceiling / peak;
        for s in samples.iter_mut() {
            *s *= k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_match_the_naive_shapes_away_from_edges() {
        // 100 Гц при 44.1 кГц: почти все гармоники на месте; середина периода — гладкая.
        for &p in &[0.2f32, 0.4, 0.6, 0.8] {
            let saw = bl_saw(p, 100.0, 44_100.0);
            assert!((saw - (2.0 * p - 1.0)).abs() < 0.02, "saw {p}: {saw}");
            let tri = bl_triangle(p, 100.0, 44_100.0);
            assert!((tri - (1.0 - 4.0 * (p - 0.5).abs())).abs() < 0.02, "tri {p}: {tri}");
            let sq = bl_pulse(p + 0.01, 0.5, 100.0, 44_100.0);
            let want = if p + 0.01 < 0.5 { 1.0 } else { -1.0 };
            assert!((sq - want).abs() < 0.1, "square {p}: {sq}");
        }
    }

    #[test]
    fn above_the_band_edge_is_silent() {
        assert_eq!(bl_saw(0.3, 21_000.0, 44_100.0), 0.0);
    }

    #[test]
    fn loudness_of_a_full_scale_1k_sine_is_about_minus_3() {
        let rate = 48_000;
        let s: Vec<f32> = (0..rate * 2).map(|i| (std::f32::consts::TAU * 997.0 * i as f32 / rate as f32).sin()).collect();
        let l = integrated_loudness(&s, 1, rate);
        assert!((l - -3.0).abs() < 0.2, "{l}");
    }

    #[test]
    fn limiter_never_exceeds_the_ceiling_and_is_transparent_below_it() {
        let mut lim = Limiter::new(0.5, 0.05, 48_000);
        let mut max_out = 0.0f32;
        for i in 0..20_000 {
            let x = if (i / 3000) % 2 == 0 { 0.3 } else { 1.7 } * ((i as f32) * 0.05).sin();
            let (l, _) = lim.process(x, -x);
            max_out = max_out.max(l.abs());
        }
        assert!(max_out <= 0.5 + 1e-6, "{max_out}");
        let mut quiet = Limiter::new(0.9, 0.05, 48_000);
        let input: Vec<f32> = (0..1000).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let out: Vec<f32> = input.iter().map(|&x| quiet.process(x, x).0).collect();
        assert_eq!(&out[LIMITER_LOOKAHEAD - 1..], &input[..1000 - (LIMITER_LOOKAHEAD - 1)]);
    }
}
