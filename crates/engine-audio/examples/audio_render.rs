//! Оффлайн-рендер звука для анализа (`tools/audio_report.py`). Ничего не играет:
//! всё пишется в WAV (float32) в каталог из первого аргумента.
//!
//! ```text
//! cargo run -p engine-audio --example audio_render -- <out_dir>
//! python tools/audio_report.py <out_dir>
//! ```
//!
//! Музыка и эффекты игр повторены здесь дословно (игры рендерят их тем же синтезатором),
//! чтобы мерить то, что слышит игрок, не собирая сами игры. Источники: `games/swarm/
//! examples/bake_assets/synth.rs`, `games/marble/tools/bake/sounds.rs`,
//! `games/lantern/examples/bake_assets/audio.rs` — при их изменении обновите копии.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use engine_audio::synth::*;
use engine_audio::{AudioClip, Bus, Level, Mixer, VoiceParams};

/// Записать float32 WAV (без ограничения уровня: анализ видит настоящие пики).
fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[f32]) {
    let data_len = (samples.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 4);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * channels as u32 * 4).to_le_bytes());
    bytes.extend_from_slice(&(channels * 4).to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

fn save(dir: &Path, name: &str, clip: &AudioClip) {
    write_wav(&dir.join(format!("{name}.wav")), clip.sample_rate(), clip.channels(), clip.samples());
}

// ---------------------------------------------------------------------------
// Рой (games/swarm/src/sound.rs)
// ---------------------------------------------------------------------------
mod swarm {
    use super::*;

    fn noise_hit(len: f32, from: f32, to: f32, vol: f32) -> Sound {
        Tone::noise().duration(len).lowpass_sweep(from, to).percussive().volume(vol).render()
    }

    fn thump(freq: f32, end: f32, len: f32, vol: f32) -> Sound {
        Tone::new(Wave::Sine, freq).sweep_to(end).duration(len).percussive().volume(vol).render()
    }

    fn fit(mut sound: Sound, len: usize) -> Sound {
        sound.samples.resize(len, 0.0);
        sound
    }

    pub fn sfx() -> Vec<(&'static str, AudioClip)> {
        vec![
            ("shoot", Tone::new(Wave::Saw, 1500.0).sweep_to(420.0).duration(0.06).adsr(0.002, 0.03, 0.2, 0.03).volume(0.5).lowpass(5200.0).render().into_clip()),
            ("hit", noise_hit(0.05, 7000.0, 900.0, 0.55).mix(Tone::new(Wave::Square, 520.0).sweep_to(260.0).duration(0.04).percussive().volume(0.25).render()).into_clip()),
            ("crit", Tone::new(Wave::Square, 900.0).sweep_to(1800.0).duration(0.07).adsr(0.002, 0.04, 0.2, 0.05).volume(0.45).render().mix(noise_hit(0.06, 8000.0, 1500.0, 0.5)).into_clip()),
            ("die", noise_hit(0.2, 4200.0, 300.0, 0.7).mix(thump(210.0, 60.0, 0.16, 0.9)).render_at(Level::Sfx)),
            ("boom", Tone::noise().duration(0.7).adsr(0.002, 0.6, 0.0, 0.1).lowpass_sweep(3000.0, 120.0).render().mix(thump(100.0, 30.0, 0.6, 1.0)).render_at(Level::Sfx)),
            ("big_boom", Tone::noise().duration(1.6).adsr(0.002, 1.4, 0.0, 0.2).lowpass_sweep(2600.0, 70.0).render().mix(thump(80.0, 24.0, 1.4, 1.0)).echo(0.18, 0.35, 0.5).render_at(Level::Sfx)),
            ("ricochet", Tone::new(Wave::Square, 1600.0).sweep_to(2400.0).duration(0.04).percussive().volume(0.3).render().into_clip()),
            ("enemy_shot", Tone::new(Wave::Square, 560.0).sweep_to(240.0).duration(0.1).adsr(0.002, 0.05, 0.3, 0.04).volume(0.35).duty(0.3).render().into_clip()),
            ("dash", Tone::noise().duration(0.2).adsr(0.01, 0.12, 0.0, 0.05).lowpass_sweep(600.0, 4200.0).volume(0.6).render().mix(Tone::new(Wave::Sine, 300.0).sweep_to(900.0).duration(0.16).percussive().volume(0.3).render()).into_clip()),
            ("core_hurt", Tone::new(Wave::Square, 130.0).duration(0.12).adsr(0.002, 0.05, 0.6, 0.06).volume(0.45).render().then(Tone::new(Wave::Square, 98.0).duration(0.16).adsr(0.002, 0.08, 0.5, 0.08).volume(0.45).render()).into_clip()),
            ("wave_clear", Sound::concat(["C5", "E5", "G5", "C6"].iter().map(|n| Tone::note(Wave::Square, n).duration(0.075).duty(0.25).volume(0.4).render())).echo(0.14, 0.3, 0.4).into_clip()),
        ]
    }

    pub fn menu_music() -> AudioClip {
        let bpm = 84.0;
        let pad = Melody::new(Wave::Triangle, bpm)
            .volume(0.36)
            .adsr(0.05, 0.2, 0.7, 0.3)
            .notes("A2 - - - - - - - F2 - - - - - - - C3 - - - - - - - G2 - - - - - - -")
            .render_loop();
        let arp = Melody::new(Wave::Sine, bpm)
            .volume(0.28)
            .adsr(0.01, 0.2, 0.3, 0.3)
            .notes("A4 C5 E5 C5 A4 C5 E5 G5 F4 A4 C5 A4 F4 A4 C5 E5 C4 E4 G4 E4 C4 E4 G4 B4 G4 B4 D5 B4 G4 B4 D5 A4")
            .render_loop();
        let lead = Melody::new(Wave::Square, bpm)
            .volume(0.1)
            .duty(0.2)
            .lowpass(4_000.0)
            .notes("E5 - - - - - D5 - C5 - - - - - - - G5 - - - E5 - - - D5 - - - - - - -")
            .render_loop();
        Sound::mix_all([pad, arp, lead]).echo_loop(0.36, 0.3, 0.3).dc_block().render_at(Level::Music)
    }

    pub fn game_music() -> AudioClip {
        let bpm = 128.0;
        let bass_a = "A2 A2 A3 A2 A2 A3 A2 A2 F2 F2 F3 F2 F2 F3 F2 F2";
        let bass_b = "C3 C3 C4 C3 C3 C4 C3 C3 G2 G2 G3 G2 G2 G3 G2 G2";
        let bass = Melody::new(Wave::Saw, bpm)
            .volume(0.34)
            .gate(0.7)
            .adsr(0.005, 0.08, 0.5, 0.05)
            .notes(bass_a)
            .notes(bass_b)
            .notes(bass_a)
            .notes(bass_b)
            .render_loop();
        let lead = Melody::new(Wave::Square, bpm)
            .volume(0.16)
            .duty(0.25)
            .lowpass(4_500.0)
            .adsr(0.005, 0.06, 0.4, 0.05)
            .notes("A4 - C5 - E5 - C5 - A4 - C5 E5 A5 - E5 -")
            .notes("F4 - A4 - C5 - A4 - F4 - A4 C5 F5 - C5 -")
            .notes("E4 - G4 - C5 - G4 - E4 - G4 C5 E5 - C5 -")
            .notes("D4 - G4 - B4 - G4 - D4 - G4 B4 D5 - B4 -")
            .render_loop();
        let arp_a = "E5 _ A4 _ E5 _ A4 _ C5 _ F4 _ C5 _ F4 _";
        let arp_b = "G4 _ C4 _ G4 _ C4 _ D5 _ G4 _ D5 _ G4 _";
        let arp = Melody::new(Wave::Triangle, bpm)
            .volume(0.22)
            .notes(arp_a)
            .notes(arp_b)
            .notes(arp_a)
            .notes(arp_b)
            .render_loop();
        let step = 60.0 / bpm / 2.0;
        let kick = thump(150.0, 45.0, 0.13, 1.0);
        let snare = Tone::noise().duration(0.1).highpass(300.0).lowpass_sweep(6000.0, 1800.0).percussive().volume(0.4).render();
        let hat = Tone::noise().duration(0.03).highpass(6_000.0).lowpass(10_000.0).percussive().volume(0.07).render();
        let kick_p = format!("{}x_x_____", "x_______".repeat(7));
        let drums = Sound::mix_all([
            Sound::pattern(&kick, &kick_p, step),
            Sound::pattern(&snare, &"____x___".repeat(8), step),
            Sound::pattern(&hat, &"x_".repeat(32), step),
        ]);
        let len = bass.len();
        Sound::mix_all([bass, lead, arp, fit(drums, len)]).dc_block().render_at(Level::Music)
    }

    pub fn boss_music() -> AudioClip {
        let bpm = 150.0;
        let b1 = "D2 D2 D3 D2 D2 D3 D2 D2 D2 D2 D3 D2 F2 F3 F2 E2";
        let b2 = "Bb1 Bb1 Bb2 Bb1 Bb1 Bb2 Bb1 Bb1 A1 A1 A2 A1 A1 A2 C2 E2";
        let bass = Melody::new(Wave::Saw, bpm)
            .volume(0.36)
            .gate(0.6)
            .adsr(0.003, 0.05, 0.5, 0.04)
            .notes(b1)
            .notes(b2)
            .notes(b1)
            .notes(b2)
            .render_loop();
        let l1 = "D5 - D5 F5 A5 - G5 F5 D5 - D5 F5 E5 - C#5 -";
        let l2 = "Bb4 - Bb4 D5 F5 - E5 D5 A4 - A4 C#5 E5 - A5 -";
        let lead = Melody::new(Wave::Square, bpm)
            .volume(0.17)
            .duty(0.125)
            .lowpass(4_500.0)
            .adsr(0.003, 0.05, 0.4, 0.04)
            .notes(l1)
            .notes(l2)
            .notes(l1)
            .notes(l2)
            .render_loop();
        let step = 60.0 / bpm / 2.0;
        let kick = thump(160.0, 40.0, 0.12, 1.0);
        let snare = Tone::noise().duration(0.09).highpass(300.0).lowpass_sweep(6500.0, 2200.0).percussive().volume(0.45).render();
        let hat = Tone::noise().duration(0.025).highpass(6_000.0).lowpass(10_000.0).percussive().volume(0.06).render();
        let len = bass.len();
        let drums = Sound::mix_all([
            Sound::pattern(&kick, &"x__x__x_".repeat(8), step),
            Sound::pattern(&snare, &"____x___".repeat(8), step),
            Sound::pattern(&hat, &"x".repeat(64), step),
        ]);
        Sound::mix_all([bass, lead, fit(drums, len)]).dc_block().render_at(Level::Music)
    }
}

// ---------------------------------------------------------------------------
// Шарикоподшипниковый завод (games/marble/src/sounds.rs)
// ---------------------------------------------------------------------------
mod marble {
    use super::*;

    pub fn sfx() -> Vec<(&'static str, AudioClip)> {
        let clang = {
            let ring = Tone::new(Wave::Sine, 880.0).duration(0.25).adsr(0.001, 0.2, 0.0, 0.1).volume(0.5).render();
            let ring2 = Tone::new(Wave::Sine, 1383.0).duration(0.2).adsr(0.001, 0.15, 0.0, 0.08).volume(0.35).render();
            let tick = Tone::noise().duration(0.03).percussive().lowpass(6000.0).volume(0.6).render();
            ring.mix(ring2).mix(tick).render_at(Level::Sfx)
        };
        let crack = {
            let a = Tone::noise().duration(0.18).percussive().lowpass_sweep(7000.0, 900.0).volume(0.8).render();
            let b = Tone::new(Wave::Square, 140.0).sweep_to(60.0).duration(0.15).percussive().volume(0.4).render();
            a.mix(b).render_at(Level::Sfx)
        };
        let roll = Tone::noise()
            .duration(1.2)
            .adsr(0.0, 0.0, 1.0, 0.0)
            .lowpass(520.0)
            .volume(0.9)
            .seed(5)
            .render()
            .fade_in(0.05)
            .fade_out(0.05)
            .normalize(0.5)
            .into_clip();
        vec![
            ("clang", clang),
            ("crack", crack),
            ("roll", roll),
            ("beep", Tone::new(Wave::Square, 660.0).duration(0.12).duty(0.4).adsr(0.004, 0.05, 0.5, 0.05).volume(0.5).render().into_clip()),
            ("go", Tone::new(Wave::Square, 990.0).sweep_to(1320.0).duration(0.28).duty(0.3).adsr(0.004, 0.08, 0.6, 0.1).volume(0.55).render().render_at(Level::Sfx)),
            ("boost", Tone::new(Wave::Saw, 180.0).sweep_to(1100.0).duration(0.35).lowpass_sweep(500.0, 5200.0).adsr(0.01, 0.1, 0.6, 0.15).volume(0.5).render().into_clip()),
        ]
    }

    pub fn music_menu() -> AudioClip {
        let bpm = 84.0;
        let bass = Melody::new(Wave::Triangle, bpm)
            .volume(0.5)
            .notes("A2 _ _ _ E3 _ _ _ F2 _ _ _ C3 _ _ _")
            .notes("D2 _ _ _ A2 _ _ _ E2 _ _ _ B2 _ _ _")
            .render_loop();
        let pad = Melody::new(Wave::Sine, bpm)
            .volume(0.28)
            .adsr(0.05, 0.2, 0.7, 0.3)
            .notes("A4 - - - - - - - F4 - - - - - - -")
            .notes("D4 - - - - - - - E4 - - - - - - -")
            .render_loop();
        let lead = Melody::new(Wave::Square, bpm)
            .volume(0.12)
            .duty(0.25)
            .lowpass(4_000.0)
            .notes("E5 _ C5 _ A4 _ C5 _ C5 _ A4 _ F4 _ A4 _")
            .notes("F4 _ A4 _ D5 _ A4 _ B4 _ E5 _ B4 _ G#4 _")
            .render_loop();
        Sound::mix_all([bass, pad, lead]).dc_block().render_at(Level::Music)
    }

    pub fn factory(bpm: f32, drum_pattern: &str, tense: bool) -> AudioClip {
        let (bass_a, bass_b, lead_a, lead_b) = if tense {
            (
                "E2 E2 _ E2 G2 _ E2 _ D2 D2 _ D2 F2 _ D2 _",
                "C2 C2 _ C2 E2 _ C2 _ B1 B1 _ B1 D2 _ B1 _",
                "E5 _ G5 E5 _ B4 _ D5 C5 _ E5 C5 _ G4 _ B4",
                "B4 _ D5 B4 _ F4 _ A4 G4 _ B4 G4 _ D5 _ E5",
            )
        } else {
            (
                "A2 _ A2 _ E3 _ A2 _ F2 _ F2 _ C3 _ F2 _",
                "D2 _ D2 _ A2 _ D2 _ E2 _ E2 _ B2 _ E2 _",
                "A4 _ C5 E5 _ C5 A4 _ F4 _ A4 C5 _ A4 F4 _",
                "D4 _ F4 A4 _ F4 D4 _ E4 _ G#4 B4 _ G#4 E4 _",
            )
        };
        let bass = Melody::new(Wave::Triangle, bpm).volume(0.55).notes(bass_a).notes(bass_b).render_loop();
        let lead = Melody::new(Wave::Square, bpm).volume(0.14).duty(0.25).lowpass(4_500.0).notes(lead_a).notes(lead_b).render_loop();
        let step = 60.0 / bpm / 2.0;
        let kick = Tone::new(Wave::Sine, 150.0).sweep_to(45.0).duration(0.12).percussive().render();
        let hat = Tone::noise().duration(0.03).percussive().highpass(6_000.0).lowpass(10_000.0).volume(0.07).render();
        let drums = Sound::pattern(&kick, drum_pattern, step).repeat(2);
        let hats = Sound::pattern(&hat, "..x...x...x...x.", step).repeat(2);
        Sound::mix_all([bass, lead, drums, hats]).dc_block().render_at(Level::Music)
    }
}

// ---------------------------------------------------------------------------
// Фонарщик (games/lantern/examples/bake_assets/audio.rs)
// ---------------------------------------------------------------------------
mod lantern {
    use super::*;

    fn note(wave: Wave, name: &str, dur: f32, vol: f32) -> Sound {
        Tone::note(wave, name).duration(dur).adsr(0.004, dur * 0.5, 0.3, dur * 0.8).volume(vol).render()
    }

    pub fn track(bpm: f32, bass: &str, lead: &str, pad: &str, drums: &str, lead_wave: Wave, _volume: f32) -> AudioClip {
        let step = 60.0 / bpm / 2.0;
        let bass = Melody::new(Wave::Triangle, bpm).volume(0.55).notes(bass).render_loop();
        let lead = Melody::new(lead_wave, bpm).volume(0.2).duty(0.25).lowpass(4_500.0).notes(lead).render_loop();
        let pad = Melody::new(Wave::Sine, bpm).volume(0.26).notes(pad).render_loop();
        let mut parts = vec![bass, lead, pad];
        if !drums.is_empty() {
            let kick = Tone::new(Wave::Sine, 140.0).sweep_to(42.0).duration(0.12).percussive().render();
            let hat = Tone::noise().duration(0.04).highpass(6_000.0).lowpass(10_000.0).percussive().volume(0.07).render();
            let kicks: String = drums.chars().map(|c| if c == 'x' { 'x' } else { '.' }).collect();
            let hats: String = drums.chars().map(|c| if c == 'h' || c == 'x' { 'x' } else { '.' }).collect();
            parts.push(Sound::pattern(&kick, &kicks, step));
            parts.push(Sound::pattern(&hat, &hats, step));
        }
        Sound::mix_all(parts).dc_block().render_at(Level::Music)
    }

    pub fn sfx() -> Vec<(&'static str, AudioClip)> {
        vec![
            ("jump", Tone::new(Wave::Square, 300.0).sweep_to(640.0).duration(0.13).duty(0.3).adsr(0.003, 0.05, 0.5, 0.04).volume(0.5).render().normalize(0.55).into_clip()),
            ("dash", Tone::noise().duration(0.18).lowpass_sweep(500.0, 5200.0).adsr(0.01, 0.1, 0.25, 0.07).volume(0.7).render().normalize(0.5).into_clip()),
            ("golden", Sound::concat(["C6", "E6", "G6", "C7", "E7"].iter().map(|n| note(Wave::Triangle, n, 0.1, 0.7))).echo(0.18, 0.4, 0.5).normalize(0.6).into_clip()),
            ("gate", Tone::noise().duration(0.5).lowpass_sweep(2200.0, 200.0).adsr(0.02, 0.4, 0.0, 0.1).volume(0.5).render()
                .mix(Tone::new(Wave::Saw, 80.0).sweep_to(150.0).duration(0.45).adsr(0.02, 0.3, 0.2, 0.1).lowpass(900.0).volume(0.6).render())
                .normalize(0.55)
                .into_clip()),
            ("shoot", Tone::new(Wave::Saw, 900.0).sweep_to(240.0).duration(0.16).lowpass(3500.0).percussive().volume(0.4).render().normalize(0.4).into_clip()),
        ]
    }

    pub fn music() -> Vec<(&'static str, AudioClip)> {
        vec![
            ("lantern_menu", track(78.0,
                "A2 _ _ _ E3 _ _ _ F2 _ _ _ C3 _ _ _ D2 _ _ _ A2 _ _ _ E2 _ _ _ E3 _ _ _",
                "E5 - - - C5 - - - A4 - - - C5 - - - D5 - - - F5 - - - E5 - - - B4 - - -",
                "A3 - - - - - - - F3 - - - - - - - D3 - - - - - - - E3 - - - - - - -",
                "", Wave::Sine, 0.7)),
            ("lantern_cave", track(96.0,
                "A2 _ A2 _ E3 _ A2 _ F2 _ F2 _ C3 _ F2 _ D2 _ D2 _ A2 _ D2 _ E2 _ E2 _ B2 _ E2 _",
                "A4 C5 E5 C5 A4 C5 E5 _ F4 A4 C5 A4 F4 A4 C5 _ D5 F5 A5 F5 D5 F5 A5 _ E5 G#4 B4 G#4 E5 B4 G#4 _",
                "A3 - - - - - - - F3 - - - - - - - D3 - - - - - - - E3 - - - - - - -",
                "x...h...x...h...x...h...x...hh..", Wave::Triangle, 0.7)),
            ("lantern_deep", track(112.0,
                "D2 _ D2 D3 _ D2 _ D3 Bb1 _ Bb1 Bb2 _ Bb1 _ Bb2 G1 _ G1 G2 _ G1 _ G2 A1 _ A1 A2 _ A1 A2 _",
                "D5 - F5 A5 - F5 D5 - Bb4 - D5 F5 - D5 Bb4 - G4 - Bb4 D5 - Bb4 G4 - A4 C#5 E5 A5 - E5 C#5 -",
                "D3 - - - - - - - Bb2 - - - - - - - G2 - - - - - - - A2 - - - - - - -",
                "x.h.x.h.x.h.x.hhx.h.x.h.x.h.x.hh", Wave::Square, 0.7)),
            ("lantern_boss", track(132.0,
                "E2 E2 _ E2 E3 _ E2 E2 C2 C2 _ C2 C3 _ C2 C2 D2 D2 _ D2 D3 _ D2 D2 B1 B1 _ B1 B2 _ B1 B1",
                "E5 G5 B5 G5 E5 G5 B5 _ C5 E5 G5 E5 C5 E5 G5 _ D5 F#5 A5 F#5 D5 F#5 A5 _ B4 D#5 F#5 D#5 B4 D#5 F#5 _",
                "E3 - - - E3 - - - C3 - - - C3 - - - D3 - - - D3 - - - B2 - - - B2 - - -",
                "x.hhx.hhx.hhx.hhx.hhx.hhx.hhx.hh", Wave::Saw, 0.75)),
        ]
    }
}

/// Музыка: (имя, клип, громкость шины Music по умолчанию в игре).
fn all_music() -> Vec<(String, AudioClip, f32)> {
    let mut v: Vec<(String, AudioClip, f32)> = vec![
        ("engine_adventure".into(), music::adventure(), 1.0),
        ("engine_calm".into(), music::calm(), 1.0),
        ("swarm_menu".into(), swarm::menu_music(), 0.6),
        ("swarm_game".into(), swarm::game_music(), 0.6),
        ("swarm_boss".into(), swarm::boss_music(), 0.6),
        ("marble_menu".into(), marble::music_menu(), 0.7),
        ("marble_factory_a".into(), marble::factory(112.0, "x...x...x...x.x.", false), 0.7),
        ("marble_factory_b".into(), marble::factory(132.0, "x..xx...x.x.x..x", true), 0.7),
    ];
    for (name, clip) in lantern::music() {
        v.push((name.to_string(), clip, 0.6));
    }
    v
}

fn all_sfx() -> Vec<(String, AudioClip)> {
    let mut v: Vec<(String, AudioClip)> = vec![
        ("preset_jump".into(), sfx::jump()),
        ("preset_land".into(), sfx::land()),
        ("preset_coin".into(), sfx::coin()),
        ("preset_powerup".into(), sfx::powerup()),
        ("preset_hit".into(), sfx::hit()),
        ("preset_hurt".into(), sfx::hurt()),
        ("preset_laser".into(), sfx::laser()),
        ("preset_explosion".into(), sfx::explosion()),
        ("preset_blip".into(), sfx::blip()),
        ("preset_ui_click".into(), sfx::ui_click()),
        ("preset_ui_hover".into(), sfx::ui_hover()),
        ("preset_ui_confirm".into(), sfx::ui_confirm()),
        ("preset_ui_error".into(), sfx::ui_error()),
        ("preset_footstep".into(), sfx::footstep()),
        ("preset_win".into(), sfx::win()),
        ("preset_lose".into(), sfx::lose()),
    ];
    for (n, c) in swarm::sfx() {
        v.push((format!("swarm_{n}"), c));
    }
    for (n, c) in marble::sfx() {
        v.push((format!("marble_{n}"), c));
    }
    for (n, c) in lantern::sfx() {
        v.push((format!("lantern_{n}"), c));
    }
    v
}

/// Генератор для сценариев: детерминированный xorshift.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as f32 / u32::MAX as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

const OUT_RATE: u32 = 48_000;
/// Кадр игры и блок устройства — как в жизни: 60 к/с, блоки по 480 кадров (10 мс).
const BLOCK: usize = 480;

/// Отрисовать `seconds` микшера блоками, вызывая `on_frame` раз в 1/60 с.
fn run_mixer(mixer: &mut Mixer, seconds: f32, mut on_frame: impl FnMut(&mut Mixer, usize)) -> Vec<f32> {
    let total = (seconds * OUT_RATE as f32) as usize;
    let mut out = vec![0.0f32; total * 2];
    let frame_len = OUT_RATE as usize / 60;
    let mut next_frame = 0usize;
    let mut game_frame = 0usize;
    let mut pos = 0usize;
    while pos < total {
        if pos >= next_frame {
            on_frame(mixer, game_frame);
            game_frame += 1;
            next_frame += frame_len;
        }
        let n = BLOCK.min(total - pos).min(next_frame - pos);
        mixer.render(&mut out[pos * 2..(pos + n) * 2]);
        pos += n;
    }
    out
}

/// Худший случай «Роя»: боевая музыка и бой, где каждый кадр упирается в лимиты звуков.
/// `old_limits` — прежние лимиты игры на кадр (2 выстрела врагов, 3 попадания, 4 смерти);
/// иначе — новые (1, 1, 2).
fn swarm_stress(music_clip: Arc<AudioClip>, sfx: &[(String, AudioClip)], old_limits: bool) -> Vec<f32> {
    let find = |name: &str| -> Arc<AudioClip> {
        Arc::new(sfx.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone()).expect(name))
    };
    let shoot = find("swarm_shoot");
    let enemy_shot = find("swarm_enemy_shot");
    let hit = find("swarm_hit");
    let crit = find("swarm_crit");
    let die = find("swarm_die");
    let boom = find("swarm_boom");
    let big_boom = find("swarm_big_boom");
    let mut mixer = Mixer::new(OUT_RATE);
    mixer.set_bus_volume(Bus::Music, 0.6);
    mixer.set_bus_volume(Bus::Sfx, 0.8);
    mixer.set_bus_volume(Bus::Ui, 0.8);
    mixer.play(music_clip, VoiceParams { looping: true, bus: Bus::Music, fade_in: 1.2, ..VoiceParams::default() });
    let mut rng = Rng(0x1234_5678);
    let play = |m: &mut Mixer, clip: &Arc<AudioClip>, volume: f32, pitch: f32| {
        m.play(Arc::clone(clip), VoiceParams { volume, pitch, ..VoiceParams::default() });
    };
    run_mixer(&mut mixer, 12.0, |m, frame| {
        // Первые 2 с — только музыка (вступление), затем бой.
        if frame < 120 {
            return;
        }
        if frame % 6 == 0 {
            play(m, &shoot, 0.32, rng.range(0.92, 1.1));
        }
        let (shots, hits, dies) = if old_limits { (2, 3, 4) } else { (1, 1, 2) };
        let (mut shot_n, mut hit_n, mut die_n) = (0, 0, 0);
        for _ in 0..2 {
            if rng.next() < 0.3 && shot_n < shots {
                shot_n += 1;
                play(m, &enemy_shot, 0.3, rng.range(0.55, 0.66));
            }
        }
        for _ in 0..3 {
            if rng.next() < 0.6 && hit_n < hits {
                hit_n += 1;
                if rng.next() < 0.2 {
                    play(m, &crit, 0.4, rng.range(0.92, 1.1));
                } else {
                    play(m, &hit, 0.32, rng.range(0.92, 1.1));
                }
            }
        }
        for _ in 0..4 {
            if rng.next() < 0.4 && die_n < dies {
                die_n += 1;
                play(m, &die, 0.5, rng.range(0.5, 1.4));
            }
        }
        if rng.next() < 0.05 {
            play(m, &boom, 0.6, rng.range(0.92, 1.1));
        }
        if frame % 180 == 0 {
            play(m, &big_boom, 0.7, 1.1);
        }
    })
}

/// Микшер: µс на блок 1024 кадра при 64 голосах.
fn bench(sfx: &[(String, AudioClip)], base_pitch: f32) -> f64 {
    let mut mixer = Mixer::new(OUT_RATE);
    let clips: Vec<Arc<AudioClip>> = sfx.iter().map(|(_, c)| Arc::new(c.clone())).collect();
    for i in 0..64 {
        let clip = Arc::clone(&clips[i % clips.len()]);
        mixer.play(clip, VoiceParams { looping: true, pitch: base_pitch + 0.003 * i as f32, pan: (i as f32 / 32.0) - 1.0, ..VoiceParams::default() });
    }
    let mut buf = vec![0.0f32; 1024 * 2];
    for _ in 0..50 {
        mixer.render(&mut buf);
    }
    // Лучший из пяти замеров: на занятой машине одиночный замер шумит в разы.
    let blocks = 200;
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..blocks {
            mixer.render(&mut buf);
        }
        best = best.min(start.elapsed().as_secs_f64() * 1e6 / blocks as f64);
    }
    best
}

fn main() {
    let out: PathBuf = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("audio_out"));
    for sub in ["music", "sfx", "mix", "osc"] {
        std::fs::create_dir_all(out.join(sub)).expect("create output dir");
    }

    let started = Instant::now();
    let music_list = all_music();
    let synth_ms = started.elapsed().as_secs_f64() * 1000.0;
    for (name, clip, _) in &music_list {
        save(&out.join("music"), name, clip);
    }
    let sfx_list = all_sfx();
    for (name, clip) in &sfx_list {
        save(&out.join("sfx"), name, clip);
    }

    // Чистые осцилляторы для оценки наложения спектров: 3520 Гц, 1 с, без огибающей.
    for (name, wave) in [("sine", Wave::Sine), ("square", Wave::Square), ("saw", Wave::Saw), ("triangle", Wave::Triangle)] {
        let s = Tone::new(wave, 3520.0).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).volume(0.5).render();
        write_wav(&out.join("osc").join(format!("{name}_3520.wav")), s.sample_rate, 1, &s.samples);
    }
    let s = Tone::new(Wave::Square, 3520.0).duty(0.125).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).volume(0.5).render();
    write_wav(&out.join("osc").join("pulse125_3520.wav"), s.sample_rate, 1, &s.samples);

    // Музыка каждой игры на выходе микшера (48 кГц, громкость шины по умолчанию в игре).
    for (name, clip, bus) in &music_list {
        let mut mixer = Mixer::new(OUT_RATE);
        mixer.set_bus_volume(Bus::Music, *bus);
        mixer.play(Arc::new(clip.clone()), VoiceParams { looping: true, bus: Bus::Music, fade_in: 1.0, ..VoiceParams::default() });
        let seconds = (clip.duration() * 2.0).clamp(8.0, 40.0);
        let buf = run_mixer(&mut mixer, seconds, |_, _| {});
        write_wav(&out.join("mix").join(format!("out_{name}.wav")), OUT_RATE, 2, &buf);
    }

    // Худший случай.
    let game = music_list.iter().find(|(n, _, _)| n == "swarm_game").map(|(_, c, _)| Arc::new(c.clone())).unwrap();
    let stress = swarm_stress(Arc::clone(&game), &sfx_list, true);
    write_wav(&out.join("mix").join("worst_swarm_stress.wav"), OUT_RATE, 2, &stress);
    let stress = swarm_stress(game, &sfx_list, false);
    write_wav(&out.join("mix").join("worst_swarm_stress_newlimits.wav"), OUT_RATE, 2, &stress);
    // Отражение при ускорении: 19 кГц на высоте 1.5 (выше Найквиста выхода).
    let tone = Tone::new(Wave::Sine, 19_000.0).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).volume(0.5).render().into_clip();
    let mut mixer = Mixer::new(OUT_RATE);
    mixer.set_headroom_db(0.0);
    mixer.play(Arc::new(tone), VoiceParams { pitch: 1.5, ..VoiceParams::default() });
    let buf = run_mixer(&mut mixer, 0.6, |_, _| {});
    write_wav(&out.join("mix").join("stopband_sine19k_pitch1.5.wav"), OUT_RATE, 2, &buf);

    // Резкая высота: один голос «shoot» с pitch 1.1 → проверка ресемплера.
    let tone = Tone::new(Wave::Sine, 9000.0).duration(1.0).adsr(0.0, 0.0, 1.0, 0.0).volume(0.5).render().into_clip();
    let mut mixer = Mixer::new(OUT_RATE);
    mixer.play(Arc::new(tone), VoiceParams { pitch: 1.5, ..VoiceParams::default() });
    let buf = run_mixer(&mut mixer, 0.6, |_, _| {});
    write_wav(&out.join("mix").join("resample_sine9k_pitch1.5.wav"), OUT_RATE, 2, &buf);

    let us = bench(&sfx_list, 1.0);
    let us_up = bench(&sfx_list, 1.3);
    let report = format!(
        "mixer_us_per_1024_block_64_voices={us:.1}\nmixer_us_64_voices_all_pitched_up_1.3={us_up:.1}\nsynth_all_music_ms={synth_ms:.0}\n"
    );
    print!("{report}");
    std::fs::write(out.join("perf.txt"), report).expect("write perf");
}
