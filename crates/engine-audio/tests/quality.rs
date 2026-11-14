//! Качество звука без ушей: наложение спектров, щелчки, перегрузка, громкость,
//! пересчёт частоты, плавность громкости, детерминизм. Всё — по отрисованным сэмплам.

use std::f64::consts::PI;
use std::sync::Arc;

use engine_audio::synth::*;
use engine_audio::{AudioClip, Mixer, VoiceParams};

const RATE: u32 = 44_100;

// ---------------------------------------------------------------------------
// Маленький БПФ (radix-2) и спектральные метрики
// ---------------------------------------------------------------------------

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two());
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Спектр мощности с окном Блэкмана — Харриса (n — степень двойки).
fn power_spectrum(x: &[f32]) -> Vec<f64> {
    let n = x.len();
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let t = 2.0 * PI * i as f64 / n as f64;
            let w = 0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            v as f64 * w
        })
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (0..n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}

/// Доля энергии не у гармоник `f0`, дБ (оценка наложения спектров).
fn inharmonic_db(x: &[f32], rate: f32, f0: f32) -> f64 {
    let spec = power_spectrum(x);
    let df = rate as f64 / x.len() as f64;
    let mut harmonic = 0.0;
    let mut total = 0.0;
    for (k, p) in spec.iter().enumerate() {
        let f = k as f64 * df;
        total += p;
        let h = (f / f0 as f64).round();
        if f < 20.0 || (h >= 1.0 && (f - h * f0 as f64).abs() <= 6.0 * df) {
            harmonic += p;
        }
    }
    10.0 * ((total - harmonic) / total).max(1e-20).log10()
}

/// Энергия в полосе [lo, hi) Гц.
fn band(spec: &[f64], n: usize, rate: f32, lo: f32, hi: f32) -> f64 {
    let df = rate as f64 / n as f64;
    spec.iter()
        .enumerate()
        .filter(|(k, _)| {
            let f = *k as f64 * df;
            f >= lo as f64 && f < hi as f64
        })
        .map(|(_, p)| p)
        .sum()
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0, |m, w| m.max((w[1] - w[0]).abs()))
}

fn left(buf: &[f32]) -> Vec<f32> {
    buf.chunks(2).map(|f| f[0]).collect()
}

fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    mixer.render(&mut out);
    out
}

fn sine_clip(freq: f32, amp: f32, seconds: f32, rate: u32) -> Arc<AudioClip> {
    let n = (rate as f32 * seconds) as usize;
    Arc::new(AudioClip::mono(
        rate,
        (0..n).map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin()).collect(),
    ))
}

fn raw(rate: u32) -> Mixer {
    let mut m = Mixer::new(rate);
    m.set_headroom_db(0.0);
    m.set_limiter_enabled(false);
    m
}

// ---------------------------------------------------------------------------
// Осцилляторы
// ---------------------------------------------------------------------------

/// Квадрат, пила, импульс и треугольник на 3.5 кГц: всё, что не гармоники, — наложение.
/// Было (наивный треугольник, 2-точечный PolyBLEP): −26…−36 дБ. Порог: −60 дБ.
#[test]
fn oscillators_do_not_alias_at_high_pitch() {
    // 3445.3 Гц = 320 бинов по 10.77 Гц при n = 4096: гармоники точно на бинах.
    let n = 1 << 14;
    let f0 = 320.0 * RATE as f32 / 4096.0;
    for (name, tone) in [
        ("square", Tone::new(Wave::Square, f0)),
        ("pulse 1/8", Tone::new(Wave::Square, f0).duty(0.125)),
        ("saw", Tone::new(Wave::Saw, f0)),
        ("triangle", Tone::new(Wave::Triangle, f0)),
        ("sine", Tone::new(Wave::Sine, f0)),
    ] {
        let s = tone.duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).lowpass(20_000.0).render();
        let seg = &s.samples[4096..4096 + n];
        let db = inharmonic_db(seg, RATE as f32, f0);
        assert!(db < -60.0, "{name}: inharmonic energy {db:.1} dB");
    }
}

/// Свип пилы до 4 кГц тоже без «иголок»: энергия выше 20 кГц ничтожна, а ниже
/// основного тона в конце свипа — нет отражений.
#[test]
fn sweeps_stay_band_limited() {
    let s = Tone::new(Wave::Saw, 500.0)
        .sweep_to(4_000.0)
        .duration(1.0)
        .adsr(0.0, 0.0, 1.0, 0.0)
        .lowpass(20_000.0)
        .render();
    let n = 8192;
    let seg = &s.samples[s.len() - n - 200..s.len() - 200];
    let spec = power_spectrum(seg);
    let total: f64 = spec.iter().sum();
    let below = band(&spec, n, RATE as f32, 50.0, 2_000.0);
    assert!(10.0 * (below / total).log10() < -50.0, "aliases below the fundamental: {:.1} dB", 10.0 * (below / total).log10());
}

#[test]
fn pulse_has_no_dc_offset() {
    let s = Tone::new(Wave::Square, 440.0).duty(0.125).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).render();
    let mean = s.samples.iter().map(|&v| v as f64).sum::<f64>() / s.len() as f64;
    assert!(mean.abs() < 0.002, "DC {mean}");
}

#[test]
fn noise_rolls_off_by_default() {
    let s = Tone::noise().duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).render();
    let n = 1 << 15;
    let spec = power_spectrum(&s.samples[..n]);
    let mid = band(&spec, n, RATE as f32, 1_000.0, 4_000.0) / 3_000.0;
    let air = band(&spec, n, RATE as f32, 16_000.0, 20_000.0) / 4_000.0;
    let rolloff = 10.0 * (mid / air).log10();
    assert!(rolloff > 20.0, "16–20 kHz is only {rolloff:.1} dB below 1–4 kHz");
    // Хэт в стиле игр (`lowpass(9000)`) — тоже: двухполюсный фильтр, а не однополюсный.
    let hat = Tone::noise().duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).lowpass(9_000.0).render();
    let spec = power_spectrum(&hat.samples[..n]);
    let lo = band(&spec, n, RATE as f32, 1_000.0, 4_000.0) / 3_000.0;
    let hi = band(&spec, n, RATE as f32, 16_000.0, 20_000.0) / 4_000.0;
    assert!(10.0 * (lo / hi).log10() > 9.0, "{:.1}", 10.0 * (lo / hi).log10());
}

// ---------------------------------------------------------------------------
// Щелчки
// ---------------------------------------------------------------------------

/// Худший случай: синус без атаки и без затухания, ноты встык и с паузами.
/// Самый крутой шаг синуса 440 Гц — 0.063; ступенька на краю ноты была бы до 1.0.
#[test]
fn note_starts_and_stops_do_not_click() {
    let melody = Melody::new(Wave::Sine, 150.0)
        .volume(1.0)
        .adsr(0.0, 0.0, 1.0, 0.0)
        .gate(0.73)
        .notes("A4 A4 _ A4 - E5 _ _ A4@0.5 A4");
    let s = melody.render();
    let limit = (2.0 * std::f32::consts::PI * 660.0 / RATE as f32) * 1.05;
    assert!(max_step(&s.samples) <= limit, "max step {} > {limit}", max_step(&s.samples));
    // То же для одиночного тона с `adsr(0, 0, 1, 0)`.
    let t = Tone::new(Wave::Sine, 440.0).phase_check();
    assert!(max_step(&t.samples) <= 0.07, "{}", max_step(&t.samples));
}

trait PhaseCheck {
    fn phase_check(self) -> Sound;
}
impl PhaseCheck for Tone {
    /// Обрыв в середине периода (0.1013 с — не кратно периоду).
    fn phase_check(self) -> Sound {
        self.duration(0.1013).adsr(0.0, 0.0, 1.0, 0.0).render()
    }
}

#[test]
fn loops_join_without_a_jump() {
    let melody = Melody::new(Wave::Sine, 120.0).volume(0.8).adsr(0.0, 0.0, 1.0, 0.3).notes("A4 - - E5 _ C5 - A4");
    let looped = melody.render_loop();
    let s = &looped.samples;
    let join = (s[0] - s[s.len() - 1]).abs();
    assert!(join <= max_step(s) + 1e-6, "join {join} vs max step inside {}", max_step(s));
    for (name, clip) in [("adventure", music::adventure()), ("calm", music::calm())] {
        let s = clip.samples();
        let typical = {
            let mut d: Vec<f32> = s.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
            d.sort_by(|a, b| a.partial_cmp(b).unwrap());
            d[d.len() * 99 / 100]
        };
        let jump = (s[0] - s[s.len() - 1]).abs();
        assert!(jump <= typical * 1.5 + 1e-4, "{name}: loop jump {jump} vs p99 step {typical}");
    }
}

/// Вытеснение голоса (лимит голосов и лимит копий клипа) — затухание, а не обрыв.
#[test]
fn voice_steals_fade_instead_of_clicking() {
    let mut mixer = raw(RATE);
    let mut steal = Mixer::with_voices(RATE, 4);
    steal.set_headroom_db(0.0);
    steal.set_limiter_enabled(false);
    for m in [&mut mixer, &mut steal] {
        m.set_max_instances(3);
    }
    let clips: Vec<Arc<AudioClip>> = (0..6).map(|i| sine_clip(200.0 + 37.0 * i as f32, 0.2, 2.0, RATE)).collect();
    let mut worst = 0.0f32;
    for m in [&mut mixer, &mut steal] {
        // Голоса разной высоты; потом лавина одинаковых — вытесняются старые.
        for c in &clips[..4] {
            m.play(Arc::clone(c), VoiceParams::default());
        }
        let mut out = render(m, 1000);
        for _ in 0..12 {
            m.play(Arc::clone(&clips[5]), VoiceParams { start_offset: 0.3, ..VoiceParams::default() });
            out.extend(render(m, 333));
        }
        worst = worst.max(max_step(&left(&out)));
    }
    // Самый крутой шаг суммы шести синусов 0.2 при ≤ 400 Гц — не больше 0.066;
    // обрыв голоса на гребне дал бы ступеньку 0.2.
    assert!(worst < 0.09, "step {worst}");
}

// ---------------------------------------------------------------------------
// Мастер-шина
// ---------------------------------------------------------------------------

/// 64 голоса на полной громкости: выход ≤ потолка, усиление лимитера меняется
/// не быстрее 1/64 за сэмпл (без «ступенек» — щелчков).
#[test]
fn limiter_holds_64_full_scale_voices_smoothly() {
    let mut mixer = Mixer::new(48_000);
    mixer.set_max_instances(64);
    let noise = Tone::noise().duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).lowpass(20_000.0).render().normalize(1.0);
    let noise = Arc::new(noise.into_clip());
    for i in 0..64 {
        let clip = if i % 2 == 0 { Arc::clone(&noise) } else { sine_clip(60.0 + i as f32 * 13.0, 1.0, 1.0, RATE) };
        mixer.play(clip, VoiceParams { looping: true, pitch: 1.0 + i as f32 * 0.01, ..VoiceParams::default() });
    }
    let mut peak = 0.0f32;
    let mut gains = Vec::new();
    let mut frame = [0.0f32; 2];
    for _ in 0..24_000 {
        mixer.render(&mut frame);
        peak = peak.max(frame[0].abs()).max(frame[1].abs());
        gains.push(mixer.limiter_gain());
    }
    assert!(peak <= 0.8915, "peak {peak}");
    let min_gain = gains.iter().fold(1.0f32, |m, &g| m.min(g));
    assert!(min_gain < 0.5, "the limiter really worked: {min_gain}");
    let max_dg = gains.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(max_dg <= 1.0 / 64.0 + 1e-4, "gain step {max_dg}");
}

#[test]
fn headroom_is_minus_6_db_by_default_and_quiet_signals_pass_untouched() {
    let mut mixer = Mixer::new(RATE);
    assert!((mixer.headroom_db() - -6.0).abs() < 1e-3);
    mixer.play(sine_clip(500.0, 0.8, 1.0, RATE), VoiceParams::default());
    let out = left(&render(&mut mixer, 20_000));
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((peak - 0.8 * 0.501).abs() < 0.005, "{peak}");
    assert_eq!(mixer.limiter_gain(), 1.0);
}

// ---------------------------------------------------------------------------
// Громкость: политика
// ---------------------------------------------------------------------------

#[test]
fn presets_follow_the_loudness_policy() {
    let fx = [
        ("jump", sfx::jump()),
        ("land", sfx::land()),
        ("coin", sfx::coin()),
        ("powerup", sfx::powerup()),
        ("hit", sfx::hit()),
        ("hurt", sfx::hurt()),
        ("laser", sfx::laser()),
        ("explosion", sfx::explosion()),
        ("blip", sfx::blip()),
        ("footstep", sfx::footstep()),
        ("win", sfx::win()),
        ("lose", sfx::lose()),
    ];
    for (name, clip) in fx {
        assert!((clip.peak() - Level::SFX_PEAK).abs() < 0.01, "{name}: peak {}", clip.peak());
    }
    for (name, clip) in [
        ("ui_click", sfx::ui_click()),
        ("ui_hover", sfx::ui_hover()),
        ("ui_confirm", sfx::ui_confirm()),
        ("ui_error", sfx::ui_error()),
    ] {
        assert!(clip.peak() <= Level::UI_PEAK + 0.01 && clip.peak() > 0.2, "{name}: peak {}", clip.peak());
    }
    for (name, clip) in [("adventure", music::adventure()), ("calm", music::calm())] {
        let l = clip.loudness();
        assert!((Level::MUSIC_LUFS - l).abs() <= 1.0, "{name}: {l} LUFS");
        assert!(clip.peak() <= Level::PEAK_CEILING + 1e-4, "{name}: peak {}", clip.peak());
    }
}

#[test]
fn normalized_to_hits_the_target_loudness() {
    let s = Tone::new(Wave::Triangle, 220.0).duration(2.0).volume(0.1).render();
    let clip = s.clone().render_at(-20.0);
    assert!((clip.loudness() - -20.0).abs() < 0.2, "{}", clip.loudness());
    // Слишком «пиковый» звук не поднимается выше −1 дБFS.
    let spiky = Sound::new(RATE, {
        let mut v = vec![0.001f32; RATE as usize];
        v[100] = 0.5;
        v
    })
    .normalized_to(-10.0);
    assert!(spiky.peak() <= Level::PEAK_CEILING + 1e-5);
}

// ---------------------------------------------------------------------------
// Пересчёт частоты
// ---------------------------------------------------------------------------

/// Полоса пропускания: 1 кГц и 8 кГц из клипа 44.1 кГц на выходе 48 кГц — уровень
/// почти без изменений. Посторонние продукты кубической интерполяции: 1 кГц — ниже
/// −70 дБ, 8 кГц — около −33 дБ (измерено; это предел 4-точечного кубика, линейная
/// интерполяция давала около −20 дБ).
#[test]
fn resampler_passband() {
    for (freq, max_loss_db, max_junk_db) in [(1_000.0, 0.05, -70.0), (8_000.0, 1.5, -30.0)] {
        let mut mixer = raw(48_000);
        mixer.play(sine_clip(freq, 0.5, 1.0, RATE), VoiceParams::default());
        let out = left(&render(&mut mixer, 40_000));
        let seg = &out[4_000..4_000 + 32_768];
        let rms = (seg.iter().map(|&v| (v * v) as f64).sum::<f64>() / seg.len() as f64).sqrt();
        let loss = -20.0 * (rms / (0.5 / 2f64.sqrt())).log10();
        assert!(loss < max_loss_db, "{freq} Hz: {loss:.2} dB lost");
        let junk = inharmonic_db(seg, 48_000.0, freq);
        assert!(junk < max_junk_db, "{freq} Hz: {junk:.1} dB of images");
    }
}

/// Полоса задержания: 19 кГц на высоте 1.5 → 28.5 кГц, выше Найквиста выхода (24 кГц).
/// Линейная интерполяция (как было) оставляла отражение на 19.5 кГц на −4.7 дБ;
/// теперь ФНЧ 4-го порядка перед интерполяцией: ≤ −28 дБ.
#[test]
fn resampler_stopband_when_pitching_up() {
    let mut mixer = raw(48_000);
    mixer.play(sine_clip(19_000.0, 0.5, 1.0, RATE), VoiceParams { pitch: 1.5, ..VoiceParams::default() });
    let out = left(&render(&mut mixer, 20_000));
    let seg = &out[2_000..18_000];
    let rms = (seg.iter().map(|&v| (v * v) as f64).sum::<f64>() / seg.len() as f64).sqrt();
    let db = 20.0 * (rms / (0.5 / 2f64.sqrt())).log10();
    assert!(db < -28.0, "alias at {db:.1} dB");
}

// ---------------------------------------------------------------------------
// Плавность и детерминизм
// ---------------------------------------------------------------------------

#[test]
fn volume_ramps_have_no_zipper_steps() {
    let mut mixer = raw(RATE);
    let dc = Arc::new(AudioClip::mono(RATE, vec![1.0; RATE as usize]));
    let id = mixer.play(dc, VoiceParams::default());
    let mut out = left(&render(&mut mixer, 500));
    for v in [0.0, 1.0, 0.3, 0.9] {
        mixer.set_volume(id, v);
        out.extend(left(&render(&mut mixer, 700)));
    }
    mixer.set_bus_volume(engine_audio::Bus::Sfx, 0.2);
    out.extend(left(&render(&mut mixer, 700)));
    // Полный размах 1.0 растянут минимум на блок микширования (256 кадров).
    assert!(max_step(&out) <= 1.0 / 256.0 + 1e-5, "{}", max_step(&out));
    // А монитор редактора — тоже плавно.
    mixer.set_monitor_muted(true);
    let muted = left(&render(&mut mixer, 700));
    assert!(max_step(&muted) <= 1.0 / 256.0 + 1e-5);
    assert_eq!(muted[muted.len() - 1], 0.0);
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(music::adventure(), music::adventure());
    assert_eq!(sfx::explosion(), sfx::explosion());
    let run = || {
        let mut m = Mixer::new(48_000);
        m.play(Arc::new(sfx::laser()), VoiceParams { pitch: 1.3, ..VoiceParams::default() });
        m.play(Arc::new(music::calm()), VoiceParams { looping: true, fade_in: 0.2, ..VoiceParams::default() });
        render(&mut m, 30_000)
    };
    assert_eq!(run(), run());
}

/// Музыка с плотными ударными (как «пещеры» Фонарщика): нормализация по громкости
/// прижимает пики лимитером и никогда не выходит за потолок.
#[test]
fn music_level_with_heavy_drums_stays_under_the_ceiling() {
    let bpm = 96.0;
    let step = 60.0 / bpm / 2.0;
    let drums = "x...h...x...h...x...h...x...hh..";
    let bass = Melody::new(Wave::Triangle, bpm)
        .volume(0.55)
        .notes("A2 _ A2 _ E3 _ A2 _ F2 _ F2 _ C3 _ F2 _ D2 _ D2 _ A2 _ D2 _ E2 _ E2 _ B2 _ E2 _")
        .render_loop();
    let lead = Melody::new(Wave::Triangle, bpm)
        .volume(0.2)
        .lowpass(4_500.0)
        .notes("A4 C5 E5 C5 A4 C5 E5 _ F4 A4 C5 A4 F4 A4 C5 _ D5 F5 A5 F5 D5 F5 A5 _ E5 G#4 B4 G#4 E5 B4 G#4 _")
        .render_loop();
    let pad = Melody::new(Wave::Sine, bpm).volume(0.26).notes("A3 - - - - - - - F3 - - - - - - - D3 - - - - - - - E3 - - - - - - -").render_loop();
    let kick = Tone::new(Wave::Sine, 140.0).sweep_to(42.0).duration(0.12).percussive().render();
    let hat = Tone::noise().duration(0.04).highpass(6_000.0).lowpass(10_000.0).percussive().volume(0.07).render();
    let kicks: String = drums.chars().map(|c| if c == 'x' { 'x' } else { '.' }).collect();
    let hats: String = drums.chars().map(|c| if c == 'h' || c == 'x' { 'x' } else { '.' }).collect();
    let parts = [bass, lead, pad, Sound::pattern(&kick, &kicks, step), Sound::pattern(&hat, &hats, step)];
    let mix = Sound::mix_all(parts).dc_block();
    let clip = mix.render_at(Level::Music);
    assert!(clip.peak() <= Level::PEAK_CEILING + 1e-4, "peak {}", clip.peak());
    assert!((clip.loudness() - Level::MUSIC_LUFS).abs() <= 1.0, "{} LUFS", clip.loudness());
}
