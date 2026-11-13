//! Синтезатор: звуки и музыка без единого файла.
//!
//! * [`Tone`] — один тон: осциллятор + огибающая ADSR + свип частоты + фильтр.
//! * [`Sound`] — моно-буфер, который можно склеивать, накладывать, обрабатывать.
//! * [`Melody`] — строка нот → [`Sound`] (зацикливается без щелчка).
//! * [`sfx`] — готовые эффекты (прыжок, монета, удар, взрыв, клик…).
//! * [`music`] — готовые короткие музыкальные петли.
//!
//! ```
//! use engine_audio::synth::*;
//!
//! // Свой «прыжок»: квадрат, частота растёт от 220 до 660 Гц за 0.15 с.
//! let jump = Tone::new(Wave::Square, 220.0)
//!     .sweep_to(660.0)
//!     .duration(0.15)
//!     .adsr(0.005, 0.05, 0.6, 0.04)
//!     .volume(0.4)
//!     .render()
//!     .into_clip();
//! assert!(jump.duration() > 0.15);
//! ```

use std::f32::consts::TAU;

use crate::clip::AudioClip;

/// Частота дискретизации синтезатора по умолчанию.
pub const SAMPLE_RATE: u32 = 44_100;

/// Форма волны осциллятора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Square,
    Saw,
    Triangle,
    /// Белый шум. Частота тона задаёт «зернистость» (шум с выборкой и удержанием);
    /// 0 — обычный белый шум.
    Noise,
}

/// Как меняется частота при свипе.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sweep {
    /// Равномерно по ступеням музыкальной шкалы (так слышится «ровно»).
    Exponential,
    /// Равномерно по герцам.
    Linear,
}

/// Огибающая громкости ADSR. Все времена в секундах, `sustain` — уровень 0..1.
///
/// Нота «зажата» (`gate`) в течение длительности тона; после этого `release`
/// секунд громкость уходит в ноль.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Envelope {
    pub const fn adsr(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack,
            decay,
            sustain,
            release,
        }
    }

    /// Ударный звук: мгновенная атака и спад за `decay`.
    pub const fn percussive(decay: f32) -> Self {
        Self::adsr(0.001, decay, 0.0, 0.0)
    }

    /// Уровень в момент `t` от начала, пока клавиша зажата.
    fn held_level(&self, t: f32) -> f32 {
        if t < self.attack {
            return if self.attack > 0.0 { t / self.attack } else { 1.0 };
        }
        let t = t - self.attack;
        if t < self.decay && self.decay > 0.0 {
            1.0 + (self.sustain - 1.0) * (t / self.decay)
        } else {
            self.sustain
        }
    }

    /// Уровень огибающей в момент `t`, если клавиша отпущена в момент `gate`.
    pub fn level(&self, t: f32, gate: f32) -> f32 {
        if t < gate {
            self.held_level(t)
        } else if self.release > 0.0 && t < gate + self.release {
            self.held_level(gate) * (1.0 - (t - gate) / self.release)
        } else {
            0.0
        }
    }
}

impl Default for Envelope {
    /// Почти плоская огибающая с короткими «фасками», чтобы звук не щёлкал.
    fn default() -> Self {
        Self::adsr(0.004, 0.0, 1.0, 0.02)
    }
}

/// Описание одного тона. Строится цепочкой, превращается в звук методом [`render`](Self::render).
#[derive(Clone, Debug, PartialEq)]
pub struct Tone {
    pub wave: Wave,
    pub freq: f32,
    pub freq_end: f32,
    pub sweep: Sweep,
    /// Сколько секунд нота зажата (затем ещё `release`).
    pub duration: f32,
    pub volume: f32,
    pub envelope: Envelope,
    /// Скважность квадрата: 0.5 — меандр, 0.125 — «жужжащий» 8-битный.
    pub duty: f32,
    /// Вибрато: (частота модуляции Гц, глубина в полутонах).
    pub vibrato: Option<(f32, f32)>,
    /// ФНЧ: частота среза в начале и в конце тона (Гц).
    pub lowpass: Option<(f32, f32)>,
    pub seed: u32,
    pub sample_rate: u32,
}

impl Tone {
    pub fn new(wave: Wave, freq: f32) -> Self {
        Self {
            wave,
            freq,
            freq_end: freq,
            sweep: Sweep::Exponential,
            duration: 0.2,
            volume: 1.0,
            envelope: Envelope::default(),
            duty: 0.5,
            vibrato: None,
            lowpass: None,
            seed: 0x9E37_79B9,
            sample_rate: SAMPLE_RATE,
        }
    }

    /// Шум без высоты тона.
    pub fn noise() -> Self {
        Self::new(Wave::Noise, 0.0)
    }

    /// Тон по имени ноты: `Tone::note(Wave::Triangle, "A4")`.
    /// Неизвестное имя даёт ноту ля первой октавы (440 Гц).
    pub fn note(wave: Wave, name: &str) -> Self {
        Self::new(wave, note_freq(name).unwrap_or(440.0))
    }

    /// Плавно менять частоту до `freq_end` к концу тона.
    pub fn sweep_to(mut self, freq_end: f32) -> Self {
        self.freq_end = freq_end;
        self
    }

    pub fn linear_sweep(mut self) -> Self {
        self.sweep = Sweep::Linear;
        self
    }

    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = seconds.max(0.0);
        self
    }

    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub fn adsr(mut self, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        self.envelope = Envelope::adsr(attack, decay, sustain, release);
        self
    }

    pub fn envelope(mut self, envelope: Envelope) -> Self {
        self.envelope = envelope;
        self
    }

    /// Ударная огибающая: громкость спадает за время `duration`, как «пум» или «клик».
    pub fn percussive(mut self) -> Self {
        self.envelope = Envelope::percussive(self.duration);
        self
    }

    pub fn duty(mut self, duty: f32) -> Self {
        self.duty = duty.clamp(0.02, 0.98);
        self
    }

    pub fn vibrato(mut self, rate_hz: f32, semitones: f32) -> Self {
        self.vibrato = Some((rate_hz, semitones));
        self
    }

    pub fn lowpass(mut self, cutoff_hz: f32) -> Self {
        self.lowpass = Some((cutoff_hz, cutoff_hz));
        self
    }

    /// ФНЧ со «скользящим» срезом: так звучат взрывы и удары.
    pub fn lowpass_sweep(mut self, from_hz: f32, to_hz: f32) -> Self {
        self.lowpass = Some((from_hz, to_hz));
        self
    }

    /// Зерно генератора шума: разные зёрна — разные, но воспроизводимые шумы.
    pub fn seed(mut self, seed: u32) -> Self {
        self.seed = seed.max(1);
        self
    }

    pub fn sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = rate.max(1);
        self
    }

    fn freq_at(&self, progress: f32) -> f32 {
        let p = progress.clamp(0.0, 1.0);
        match self.sweep {
            Sweep::Exponential if self.freq > 0.0 && self.freq_end > 0.0 => {
                self.freq * (self.freq_end / self.freq).powf(p)
            }
            _ => self.freq + (self.freq_end - self.freq) * p,
        }
    }

    /// Сгенерировать звук длиной `duration + release`.
    pub fn render(&self) -> Sound {
        let rate = self.sample_rate as f32;
        let total = ((self.duration + self.envelope.release) * rate).ceil() as usize;
        let mut samples = Vec::with_capacity(total);
        let mut phase = 0.0f32;
        let mut noise_state = self.seed.max(1);
        let mut held_noise = 0.0f32;
        let mut filtered = 0.0f32;
        for i in 0..total {
            let t = i as f32 / rate;
            let progress = if self.duration > 0.0 { t / self.duration } else { 1.0 };
            let mut freq = self.freq_at(progress);
            if let Some((vib_rate, depth)) = self.vibrato {
                freq *= (depth * (TAU * vib_rate * t).sin() / 12.0).exp2();
            }
            let dt = (freq / rate).clamp(0.0, 0.5);
            let mut value = match self.wave {
                Wave::Sine => (TAU * phase).sin(),
                Wave::Square => {
                    let mut v = if phase < self.duty { 1.0 } else { -1.0 };
                    v += poly_blep(phase, dt);
                    v -= poly_blep((phase + 1.0 - self.duty).fract(), dt);
                    v
                }
                Wave::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Noise => {
                    if freq <= 0.0 || freq >= rate * 0.5 || phase + dt >= 1.0 || i == 0 {
                        held_noise = next_noise(&mut noise_state);
                    }
                    held_noise
                }
            };
            phase += dt;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            if let Some((from, to)) = self.lowpass {
                let cutoff = from + (to - from) * progress.clamp(0.0, 1.0);
                let a = 1.0 - (-TAU * cutoff.max(1.0) / rate).exp();
                filtered += a * (value - filtered);
                value = filtered;
            }
            samples.push(value * self.volume * self.envelope.level(t, self.duration));
        }
        Sound {
            sample_rate: self.sample_rate,
            samples,
        }
    }
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

fn next_noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Частота MIDI-ноты (69 = ля первой октавы = 440 Гц).
pub fn midi_to_freq(midi: f32) -> f32 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

/// Номер MIDI-ноты по имени: `C4` = 60, `A4` = 69, `F#3`, `Bb5`.
pub fn note_midi(name: &str) -> Option<i32> {
    let mut chars = name.trim().chars();
    let base = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (accidental, octave) = match rest.chars().next()? {
        '#' => (1, &rest[1..]),
        'b' => (-1, &rest[1..]),
        _ => (0, rest.as_str()),
    };
    let octave: i32 = octave.parse().ok()?;
    Some(12 * (octave + 1) + base + accidental)
}

/// Частота ноты по имени: `note_freq("A4") == Some(440.0)`.
pub fn note_freq(name: &str) -> Option<f32> {
    note_midi(name).map(|m| midi_to_freq(m as f32))
}

// ----------------------------------------------------------------------
// Sound
// ----------------------------------------------------------------------

/// Моно-звук в памяти. Все преобразования возвращают новый звук, поэтому их удобно
/// цеплять: `tone.render().gain(0.5).fade_out(0.05).then(other)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl Sound {
    pub fn new(sample_rate: u32, samples: Vec<f32>) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
            samples,
        }
    }

    pub fn silence(seconds: f32) -> Self {
        Self::new(SAMPLE_RATE, vec![0.0; (seconds.max(0.0) * SAMPLE_RATE as f32) as usize])
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }

    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    /// Умножить громкость.
    pub fn gain(mut self, gain: f32) -> Self {
        for s in &mut self.samples {
            *s *= gain;
        }
        self
    }

    /// Привести пик к `peak` (например 0.8).
    pub fn normalize(self, peak: f32) -> Self {
        let current = self.peak();
        if current > 1e-9 {
            self.gain(peak / current)
        } else {
            self
        }
    }

    /// Склеить: сначала этот звук, затем `other`.
    pub fn then(mut self, other: Sound) -> Self {
        let other = other.resampled(self.sample_rate);
        self.samples.extend_from_slice(&other.samples);
        self
    }

    /// Добавить `seconds` тишины в конец.
    pub fn pad(mut self, seconds: f32) -> Self {
        let extra = (seconds.max(0.0) * self.sample_rate as f32) as usize;
        self.samples.resize(self.samples.len() + extra, 0.0);
        self
    }

    /// Наложить `other` с начала; длина — по более длинному.
    pub fn mix(self, other: Sound) -> Self {
        self.mix_at(other, 0.0)
    }

    /// Наложить `other`, начав через `offset` секунд.
    pub fn mix_at(mut self, other: Sound, offset: f32) -> Self {
        let other = other.resampled(self.sample_rate);
        let start = (offset.max(0.0) * self.sample_rate as f32) as usize;
        let end = start + other.samples.len();
        if self.samples.len() < end {
            self.samples.resize(end, 0.0);
        }
        for (dst, src) in self.samples[start..end].iter_mut().zip(&other.samples) {
            *dst += src;
        }
        self
    }

    /// Склеить много звуков подряд.
    pub fn concat<I: IntoIterator<Item = Sound>>(parts: I) -> Sound {
        let mut iter = parts.into_iter();
        let first = iter.next().unwrap_or_else(|| Sound::silence(0.0));
        iter.fold(first, Sound::then)
    }

    /// Смешать много звуков, начиная каждый с нуля.
    pub fn mix_all<I: IntoIterator<Item = Sound>>(parts: I) -> Sound {
        let mut iter = parts.into_iter();
        let first = iter.next().unwrap_or_else(|| Sound::silence(0.0));
        iter.fold(first, Sound::mix)
    }

    /// Повторить `times` раз подряд.
    pub fn repeat(self, times: usize) -> Self {
        let mut samples = Vec::with_capacity(self.samples.len() * times);
        for _ in 0..times {
            samples.extend_from_slice(&self.samples);
        }
        Self {
            sample_rate: self.sample_rate,
            samples,
        }
    }

    pub fn reverse(mut self) -> Self {
        self.samples.reverse();
        self
    }

    pub fn fade_in(mut self, seconds: f32) -> Self {
        let n = ((seconds * self.sample_rate as f32) as usize).min(self.samples.len());
        for (i, s) in self.samples[..n].iter_mut().enumerate() {
            *s *= i as f32 / n as f32;
        }
        self
    }

    pub fn fade_out(mut self, seconds: f32) -> Self {
        let len = self.samples.len();
        let n = ((seconds * self.sample_rate as f32) as usize).min(len);
        for (i, s) in self.samples[len - n..].iter_mut().enumerate() {
            *s *= 1.0 - i as f32 / n as f32;
        }
        self
    }

    /// Простой ФНЧ (однополюсный): «глушит» верха.
    pub fn lowpass(mut self, cutoff_hz: f32) -> Self {
        let a = 1.0 - (-TAU * cutoff_hz.max(1.0) / self.sample_rate as f32).exp();
        let mut y = 0.0;
        for s in &mut self.samples {
            y += a * (*s - y);
            *s = y;
        }
        self
    }

    /// Эхо: `feedback` — доля, возвращающаяся в повтор, `mix` — громкость повторов.
    /// Хвост эха добавляется в конец (длина растёт).
    pub fn echo(self, delay: f32, feedback: f32, mix: f32) -> Self {
        let d = ((delay * self.sample_rate as f32) as usize).max(1);
        let feedback = feedback.clamp(0.0, 0.95);
        let mut tail = 0usize;
        let mut level = 1.0;
        while level > 0.01 && tail < 16 {
            tail += 1;
            level *= feedback;
        }
        let dry = self.samples.len();
        let mut wet = self.samples;
        wet.resize(dry + d * tail, 0.0);
        for i in d..wet.len() {
            wet[i] += feedback * wet[i - d];
        }
        // На выходе: сухой сигнал + `mix` долей того, что добавило эхо.
        let mix = mix.clamp(0.0, 1.0);
        let out = wet
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                let echo_part = if i >= d { feedback * wet[i - d] } else { 0.0 };
                w - echo_part * (1.0 - mix)
            })
            .collect();
        Self {
            sample_rate: self.sample_rate,
            samples: out,
        }
    }

    /// Расставить звук `hit` по сетке шагов: `x` — удар, любой другой символ — пауза.
    /// `Sound::pattern(&kick, "x...x...x..x....", 0.125)`. Длина результата — ровно
    /// число шагов, хвосты ударов заворачиваются в начало, поэтому петля бесшовна.
    pub fn pattern(hit: &Sound, pattern: &str, step_seconds: f32) -> Sound {
        let steps: Vec<char> = pattern.chars().filter(|c| !c.is_whitespace()).collect();
        let rate = hit.sample_rate;
        let total = (steps.len() as f32 * step_seconds * rate as f32).round() as usize;
        let mut out = vec![0.0f32; total.max(1)];
        for (i, c) in steps.iter().enumerate() {
            if *c == 'x' || *c == 'X' {
                let start = (i as f32 * step_seconds * rate as f32).round() as usize;
                for (j, s) in hit.samples.iter().enumerate() {
                    let at = (start + j) % out.len();
                    out[at] += s;
                }
            }
        }
        Sound::new(rate, out)
    }

    fn resampled(self, rate: u32) -> Sound {
        if self.sample_rate == rate || self.samples.is_empty() {
            return Sound {
                sample_rate: rate,
                samples: self.samples,
            };
        }
        let ratio = self.sample_rate as f64 / rate as f64;
        let n = (self.samples.len() as f64 / ratio) as usize;
        let samples = (0..n)
            .map(|i| {
                let pos = i as f64 * ratio;
                let a = pos as usize;
                let frac = (pos - a as f64) as f32;
                let s0 = self.samples[a];
                let s1 = *self.samples.get(a + 1).unwrap_or(&s0);
                s0 + (s1 - s0) * frac
            })
            .collect();
        Sound {
            sample_rate: rate,
            samples,
        }
    }

    /// Превратить в клип (моно).
    pub fn into_clip(self) -> AudioClip {
        AudioClip::from_samples(self.sample_rate, 1, self.samples)
    }
}

impl From<Sound> for AudioClip {
    fn from(sound: Sound) -> Self {
        sound.into_clip()
    }
}

/// `audio.add_clip("name", sound)` принимает и готовый `Sound`.
impl From<Sound> for std::sync::Arc<AudioClip> {
    fn from(sound: Sound) -> Self {
        std::sync::Arc::new(sound.into_clip())
    }
}

impl From<Tone> for Sound {
    fn from(tone: Tone) -> Self {
        tone.render()
    }
}

// ----------------------------------------------------------------------
// Melody
// ----------------------------------------------------------------------

/// Мелодия: строка нот, один токен на шаг.
///
/// * `C4`, `F#3`, `Bb5` — нота;
/// * `_` (или `.`) — пауза;
/// * `-` — продлить предыдущую ноту ещё на шаг.
///
/// ```
/// use engine_audio::synth::*;
/// let lead = Melody::new(Wave::Square, 120.0)
///     .volume(0.3)
///     .notes("C5 E5 G5 - E5 _ C5 -")
///     .render_loop();
/// // Ровно 8 шагов по 1/2 доли при 120 уд/мин: 2 секунды.
/// assert!((lead.duration() - 2.0).abs() < 0.01);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Melody {
    pub wave: Wave,
    pub bpm: f32,
    pub steps_per_beat: u32,
    pub volume: f32,
    pub envelope: Envelope,
    pub duty: f32,
    /// Какую долю шага нота звучит до отпускания (остальное — тишина перед следующей).
    pub gate: f32,
    steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Note(f32),
    Rest,
    Hold,
}

impl Melody {
    /// Новая мелодия; шаг — восьмая нота (2 шага на долю). `bpm` — ударов в минуту.
    pub fn new(wave: Wave, bpm: f32) -> Self {
        Self {
            wave,
            bpm: bpm.max(1.0),
            steps_per_beat: 2,
            volume: 0.5,
            envelope: Envelope::adsr(0.005, 0.05, 0.7, 0.05),
            duty: 0.5,
            gate: 0.9,
            steps: Vec::new(),
        }
    }

    pub fn steps_per_beat(mut self, steps: u32) -> Self {
        self.steps_per_beat = steps.max(1);
        self
    }

    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub fn adsr(mut self, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        self.envelope = Envelope::adsr(attack, decay, sustain, release);
        self
    }

    pub fn duty(mut self, duty: f32) -> Self {
        self.duty = duty;
        self
    }

    pub fn gate(mut self, fraction: f32) -> Self {
        self.gate = fraction.clamp(0.05, 1.0);
        self
    }

    /// Добавить шаги из строки (можно вызывать несколько раз — шаги дописываются).
    /// Нераспознанный токен считается паузой.
    pub fn notes(mut self, text: &str) -> Self {
        for token in text.split_whitespace() {
            self.steps.push(match token {
                "_" | "." => Step::Rest,
                "-" => Step::Hold,
                other => note_freq(other).map(Step::Note).unwrap_or(Step::Rest),
            });
        }
        self
    }

    /// Число шагов.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Длительность шага, секунд.
    pub fn step_seconds(&self) -> f32 {
        60.0 / self.bpm / self.steps_per_beat as f32
    }

    fn render_inner(&self, wrap: bool) -> Sound {
        let step_s = self.step_seconds();
        let rate = SAMPLE_RATE;
        let total = (self.steps.len() as f32 * step_s * rate as f32).round() as usize;
        let mut out = vec![0.0f32; total.max(1)];
        let mut i = 0;
        while i < self.steps.len() {
            let Step::Note(freq) = self.steps[i] else {
                i += 1;
                continue;
            };
            let mut hold = 1;
            while i + hold < self.steps.len() && self.steps[i + hold] == Step::Hold {
                hold += 1;
            }
            let start = (i as f32 * step_s * rate as f32).round() as usize;
            let gate = (hold as f32 - 1.0 + self.gate) * step_s;
            let tone = Tone::new(self.wave, freq)
                .duration(gate)
                .envelope(self.envelope)
                .duty(self.duty)
                .volume(self.volume)
                .render();
            for (j, s) in tone.samples.iter().enumerate() {
                let at = start + j;
                if at >= out.len() {
                    if wrap {
                        let at = at % out.len();
                        out[at] += s;
                    } else {
                        out.resize(at + 1, 0.0);
                        out[at] += s;
                    }
                } else {
                    out[at] += s;
                }
            }
            i += hold;
        }
        Sound::new(rate, out)
    }

    /// Звук ровно по числу шагов; хвосты нот заворачиваются в начало —
    /// получается бесшовная петля.
    pub fn render_loop(&self) -> Sound {
        self.render_inner(true)
    }

    /// Звук с хвостом последней ноты (для одиночных фраз, фанфар, джинглов).
    pub fn render(&self) -> Sound {
        self.render_inner(false)
    }
}

// ----------------------------------------------------------------------
// Готовые эффекты и музыка
// ----------------------------------------------------------------------

/// Готовые звуковые эффекты. Все возвращают клипы, которые остаётся передать в
/// `Audio::add_clip`. Звуки детерминированы: один вызов — одни и те же сэмплы.
pub mod sfx {
    use super::*;

    fn clip(sound: Sound) -> AudioClip {
        sound.normalize(0.8).into_clip()
    }

    /// Прыжок: быстрый подъём квадратной волны.
    pub fn jump() -> AudioClip {
        clip(
            Tone::new(Wave::Square, 260.0)
                .sweep_to(620.0)
                .duration(0.16)
                .duty(0.35)
                .adsr(0.004, 0.06, 0.5, 0.05)
                .render(),
        )
    }

    /// Приземление: глухой «шлёп».
    pub fn land() -> AudioClip {
        clip(
            Tone::new(Wave::Sine, 140.0)
                .sweep_to(50.0)
                .duration(0.08)
                .percussive()
                .render()
                .mix(Tone::noise().duration(0.05).lowpass(900.0).percussive().volume(0.3).render()),
        )
    }

    /// Монетка: две ноты вверх.
    pub fn coin() -> AudioClip {
        clip(
            Tone::new(Wave::Square, 987.8)
                .duration(0.07)
                .duty(0.5)
                .adsr(0.002, 0.0, 1.0, 0.01)
                .volume(0.6)
                .render()
                .then(
                    Tone::new(Wave::Square, 1318.5)
                        .duration(0.2)
                        .adsr(0.002, 0.15, 0.1, 0.08)
                        .volume(0.6)
                        .render(),
                ),
        )
    }

    /// Подбор бонуса: восходящее арпеджио.
    pub fn powerup() -> AudioClip {
        let notes = ["C5", "E5", "G5", "C6", "E6"];
        clip(Sound::concat(notes.iter().map(|n| {
            Tone::note(Wave::Square, n)
                .duration(0.055)
                .duty(0.25)
                .adsr(0.002, 0.0, 1.0, 0.015)
                .render()
        })))
    }

    /// Удар по врагу: щелчок шума и низкий тон.
    pub fn hit() -> AudioClip {
        clip(
            Tone::noise()
                .duration(0.12)
                .lowpass_sweep(5000.0, 400.0)
                .percussive()
                .render()
                .mix(Tone::new(Wave::Sine, 180.0).sweep_to(70.0).duration(0.1).percussive().volume(0.8).render()),
        )
    }

    /// Игрок получил урон: падающий «пила» с шумом.
    pub fn hurt() -> AudioClip {
        clip(
            Tone::new(Wave::Saw, 340.0)
                .sweep_to(90.0)
                .duration(0.28)
                .adsr(0.002, 0.1, 0.4, 0.08)
                .volume(0.7)
                .render()
                .mix(Tone::noise().duration(0.1).lowpass(2500.0).percussive().volume(0.35).render()),
        )
    }

    /// Выстрел / лазер.
    pub fn laser() -> AudioClip {
        clip(
            Tone::new(Wave::Saw, 2000.0)
                .sweep_to(180.0)
                .duration(0.2)
                .adsr(0.002, 0.08, 0.3, 0.06)
                .lowpass(6000.0)
                .render(),
        )
    }

    /// Взрыв: гулкий шум с падающим срезом и басовый «бум».
    pub fn explosion() -> AudioClip {
        clip(
            Tone::noise()
                .duration(0.9)
                .adsr(0.002, 0.8, 0.0, 0.1)
                .lowpass_sweep(2800.0, 90.0)
                .render()
                .mix(
                    Tone::new(Wave::Sine, 90.0)
                        .sweep_to(28.0)
                        .duration(0.7)
                        .adsr(0.002, 0.6, 0.0, 0.1)
                        .volume(0.9)
                        .render(),
                ),
        )
    }

    /// Короткий «бип».
    pub fn blip() -> AudioClip {
        clip(Tone::new(Wave::Square, 880.0).duration(0.05).duty(0.5).volume(0.6).render())
    }

    /// Клик по кнопке интерфейса.
    pub fn ui_click() -> AudioClip {
        clip(
            Tone::new(Wave::Triangle, 1500.0)
                .sweep_to(900.0)
                .duration(0.03)
                .percussive()
                .render(),
        )
    }

    /// Наведение на кнопку: тихий короткий тик.
    pub fn ui_hover() -> AudioClip {
        clip(Tone::new(Wave::Sine, 1900.0).duration(0.018).percussive().volume(0.5).render())
    }

    /// Подтверждение: две ноты вверх, мягкие.
    pub fn ui_confirm() -> AudioClip {
        clip(
            Tone::new(Wave::Triangle, 660.0)
                .duration(0.07)
                .adsr(0.003, 0.0, 1.0, 0.02)
                .render()
                .then(Tone::new(Wave::Triangle, 990.0).duration(0.12).adsr(0.003, 0.08, 0.2, 0.06).render()),
        )
    }

    /// Отказ / ошибка: низкий двойной «бзз».
    pub fn ui_error() -> AudioClip {
        let buzz = Tone::new(Wave::Square, 150.0).duration(0.09).duty(0.4).adsr(0.003, 0.0, 1.0, 0.01).volume(0.5);
        clip(buzz.render().then(Sound::silence(0.04)).then(buzz.render()))
    }

    /// Шаг по земле.
    pub fn footstep() -> AudioClip {
        clip(Tone::noise().duration(0.05).lowpass_sweep(1800.0, 300.0).percussive().render())
    }

    /// Победная фанфара.
    pub fn win() -> AudioClip {
        let melody = Melody::new(Wave::Square, 150.0)
            .steps_per_beat(2)
            .volume(0.4)
            .duty(0.25)
            .adsr(0.004, 0.05, 0.7, 0.12)
            .notes("C5 E5 G5 C6 - - G5 C6 - - - -");
        clip(melody.render())
    }

    /// Поражение: падающая интонация.
    pub fn lose() -> AudioClip {
        let melody = Melody::new(Wave::Triangle, 100.0)
            .volume(0.5)
            .adsr(0.004, 0.1, 0.6, 0.2)
            .notes("G4 - E4 - C4 - - - - -");
        clip(melody.render())
    }
}

/// Короткие музыкальные петли (бесшовные), чтобы в игре сразу была музыка.
pub mod music {
    use super::*;

    /// Бодрая приключенческая петля: Am – F – C – G, 4 такта, около 7 секунд.
    pub fn adventure() -> AudioClip {
        let bpm = 132.0;
        let step = 60.0 / bpm / 2.0;
        let bass = Melody::new(Wave::Triangle, bpm)
            .volume(0.55)
            .adsr(0.004, 0.06, 0.6, 0.05)
            .notes("A2 _ E3 _ A2 _ E3 _")
            .notes("F2 _ C3 _ F2 _ C3 _")
            .notes("C3 _ G3 _ C3 _ G3 _")
            .notes("G2 _ D3 _ G2 _ D3 _")
            .render_loop();
        let lead = Melody::new(Wave::Square, bpm)
            .volume(0.17)
            .duty(0.25)
            .adsr(0.005, 0.06, 0.6, 0.06)
            .notes("E5 - C5 E5 A5 - E5 -")
            .notes("C5 - A4 C5 F5 - C5 -")
            .notes("E5 - G5 E5 C5 - E5 -")
            .notes("D5 - B4 D5 G5 - D5 _")
            .render_loop();
        let kick = Tone::new(Wave::Sine, 150.0).sweep_to(45.0).duration(0.12).percussive().volume(0.8).render();
        let hat = Tone::noise().duration(0.03).lowpass_sweep(9000.0, 6000.0).percussive().volume(0.12).render();
        let kicks = Sound::pattern(&kick, "x...x...x...x...x...x...x...x...", step);
        let hats = Sound::pattern(&hat, ".x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x", step);
        Sound::mix_all([bass, lead, kicks, hats]).normalize(0.7).into_clip()
    }

    /// Спокойная ночная петля: мягкие ноты, без ударных.
    pub fn calm() -> AudioClip {
        let bpm = 84.0;
        let pad = Melody::new(Wave::Sine, bpm)
            .volume(0.4)
            .adsr(0.15, 0.2, 0.6, 0.4)
            .gate(1.0)
            .notes("A3 - - - - - - - F3 - - - - - - - C3 - - - - - - - G3 - - - - - - -")
            .render_loop();
        let bell = Melody::new(Wave::Triangle, bpm)
            .volume(0.22)
            .adsr(0.004, 0.5, 0.0, 0.3)
            .notes("E5 _ _ C5 _ A4 _ _ C5 _ _ A4 _ F4 _ _ G4 _ _ E4 _ C4 _ _ B4 _ _ D5 _ G4 _ _")
            .render_loop();
        Sound::mix_all([pad, bell]).echo(0.36, 0.35, 0.5).normalize(0.6).into_clip()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_crossings(samples: &[f32]) -> usize {
        samples.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn sine_tone_has_requested_frequency_and_amplitude() {
        let sound = Tone::new(Wave::Sine, 440.0).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).render();
        assert_eq!(sound.len(), SAMPLE_RATE as usize);
        let crossings = zero_crossings(&sound.samples);
        assert!((crossings as i32 - 440).abs() <= 1, "{crossings}");
        assert!((sound.peak() - 1.0).abs() < 0.01);
    }

    #[test]
    fn square_saw_triangle_keep_their_pitch() {
        for wave in [Wave::Square, Wave::Saw, Wave::Triangle] {
            let sound = Tone::new(wave, 220.0).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).render();
            let crossings = zero_crossings(&sound.samples);
            assert!((crossings as i32 - 220).abs() <= 2, "{wave:?}: {crossings}");
            assert!(sound.peak() <= 1.2, "{wave:?} peak {}", sound.peak());
        }
    }

    #[test]
    fn sweep_goes_from_start_to_end_frequency() {
        let sound = Tone::new(Wave::Sine, 200.0)
            .sweep_to(800.0)
            .duration(2.0)
            .adsr(0.0, 0.0, 1.0, 0.0)
            .render();
        let n = SAMPLE_RATE as usize / 4;
        let first = zero_crossings(&sound.samples[..n]) as f32 * 4.0;
        let last = zero_crossings(&sound.samples[sound.len() - n..]) as f32 * 4.0;
        assert!(first < 300.0, "{first}");
        assert!(last > 600.0, "{last}");
    }

    #[test]
    fn adsr_levels_follow_the_envelope() {
        let e = Envelope::adsr(0.1, 0.1, 0.5, 0.2);
        assert!((e.level(0.0, 1.0) - 0.0).abs() < 1e-6);
        assert!((e.level(0.05, 1.0) - 0.5).abs() < 1e-6);
        assert!((e.level(0.1, 1.0) - 1.0).abs() < 1e-6);
        assert!((e.level(0.15, 1.0) - 0.75).abs() < 1e-6);
        assert!((e.level(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!((e.level(1.1, 1.0) - 0.25).abs() < 1e-6);
        assert_eq!(e.level(1.3, 1.0), 0.0);
    }

    #[test]
    fn tone_length_includes_release() {
        let sound = Tone::new(Wave::Sine, 100.0).duration(0.5).adsr(0.0, 0.0, 1.0, 0.25).render();
        assert!((sound.duration() - 0.75).abs() < 0.001);
    }

    #[test]
    fn concat_mix_and_pad_compose_lengths() {
        let a = Sound::new(1000, vec![0.5; 100]);
        let b = Sound::new(1000, vec![0.25; 50]);
        assert_eq!(a.clone().then(b.clone()).len(), 150);
        let mixed = a.clone().mix(b.clone());
        assert_eq!(mixed.len(), 100);
        assert!((mixed.samples[0] - 0.75).abs() < 1e-6);
        assert!((mixed.samples[99] - 0.5).abs() < 1e-6);
        assert_eq!(a.clone().mix_at(b, 0.09).len(), 140);
        assert_eq!(a.pad(0.05).len(), 150);
    }

    #[test]
    fn mixing_different_sample_rates_resamples() {
        let a = Sound::new(1000, vec![0.0; 1000]);
        let b = Sound::new(500, vec![1.0; 500]);
        let m = a.mix(b);
        assert_eq!(m.len(), 1000);
        assert!((m.samples[10] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn note_names_map_to_frequencies() {
        assert_eq!(note_midi("C4"), Some(60));
        assert_eq!(note_midi("A4"), Some(69));
        assert_eq!(note_midi("F#3"), Some(54));
        assert_eq!(note_midi("Bb5"), Some(82));
        assert!((note_freq("A4").unwrap() - 440.0).abs() < 1e-3);
        assert!((note_freq("A5").unwrap() - 880.0).abs() < 1e-2);
        assert_eq!(note_midi("H4"), None);
        assert_eq!(note_midi("C"), None);
    }

    #[test]
    fn melody_loop_has_exact_length_and_wraps_the_tail() {
        let melody = Melody::new(Wave::Sine, 120.0).notes("A4 - _ A4");
        let looped = melody.render_loop();
        let expected = (4.0 * melody.step_seconds() * SAMPLE_RATE as f32).round() as usize;
        assert_eq!(looped.len(), expected);
        // Последняя нота звенит дальше конца: хвост оказался в начале петли.
        let plain = melody.render();
        assert!(plain.len() > looped.len());
        assert!(looped.samples[..2000].iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn pattern_places_hits_on_the_grid() {
        let hit = Sound::new(1000, vec![1.0; 10]);
        let p = Sound::pattern(&hit, "x..x", 0.1);
        assert_eq!(p.len(), 400);
        assert_eq!(p.samples[0], 1.0);
        assert_eq!(p.samples[50], 0.0);
        assert_eq!(p.samples[300], 1.0);
    }

    #[test]
    fn presets_are_audible_and_not_clipping() {
        for (name, clip) in [
            ("jump", sfx::jump()),
            ("land", sfx::land()),
            ("coin", sfx::coin()),
            ("powerup", sfx::powerup()),
            ("hit", sfx::hit()),
            ("hurt", sfx::hurt()),
            ("laser", sfx::laser()),
            ("explosion", sfx::explosion()),
            ("blip", sfx::blip()),
            ("ui_click", sfx::ui_click()),
            ("ui_hover", sfx::ui_hover()),
            ("ui_confirm", sfx::ui_confirm()),
            ("ui_error", sfx::ui_error()),
            ("footstep", sfx::footstep()),
            ("win", sfx::win()),
            ("lose", sfx::lose()),
            ("adventure", music::adventure()),
            ("calm", music::calm()),
        ] {
            assert!(clip.peak() > 0.3, "{name} is too quiet: {}", clip.peak());
            assert!(clip.peak() <= 1.0, "{name} clips: {}", clip.peak());
            assert!(clip.duration() > 0.01, "{name}");
            assert!(clip.samples().iter().all(|s| s.is_finite()), "{name}");
        }
    }

    #[test]
    fn music_loops_are_seamless_lengths() {
        let clip = music::adventure();
        let step = 60.0 / 132.0 / 2.0;
        assert!((clip.duration() - 32.0 * step).abs() < 0.01, "{}", clip.duration());
    }

    #[test]
    fn noise_is_deterministic_per_seed() {
        let a = Tone::noise().duration(0.05).seed(7).render();
        let b = Tone::noise().duration(0.05).seed(7).render();
        let c = Tone::noise().duration(0.05).seed(8).render();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fades_and_reverse() {
        let s = Sound::new(100, vec![1.0; 100]).fade_out(0.5);
        assert!((s.samples[0] - 1.0).abs() < 1e-6);
        assert!(s.samples[99] < 0.05);
        let r = Sound::new(100, vec![1.0, 2.0, 3.0]).reverse();
        assert_eq!(r.samples, vec![3.0, 2.0, 1.0]);
    }
}
