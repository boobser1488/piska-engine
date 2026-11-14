//! Рендер звуков игры из синтезатора движка в `assets/audio/*.wav`.
//!
//! Уровни — по политике громкости движка (`docs/api/audio.md`): музыка
//! `render_at(Level::Music)` (−14 LUFS — меню, пещеры и босс одинаково громкие),
//! хэты — полоса шума 6–10 кГц, мелодии с мягким срезом верхов.

use engine::audio::Level;
use engine::prelude::*;

fn note(wave: Wave, name: &str, dur: f32, vol: f32) -> Sound {
    Tone::note(wave, name).duration(dur).adsr(0.004, dur * 0.5, 0.3, dur * 0.8).volume(vol).render()
}

/// Музыка из трёх голосов: бас, мелодия и ударные (все по 32 шага).
/// `_volume` — прежний пиковый уровень; громкость теперь задаёт политика (`Level::Music`).
fn track(bpm: f32, bass: &str, lead: &str, pad: &str, drums: &str, lead_wave: Wave, _volume: f32) -> AudioClip {
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

/// Все клипы игры: (имя файла без расширения, звук).
pub fn render_all() -> Vec<(String, AudioClip)> {
    let mut out: Vec<(String, AudioClip)> = Vec::new();
    let mut add = |name: &str, clip: AudioClip| out.push((name.to_string(), clip));
    // --- эффекты ---
    add(
        "jump",
        Tone::new(Wave::Square, 300.0).sweep_to(640.0).duration(0.13).duty(0.3).adsr(0.003, 0.05, 0.5, 0.04).volume(0.5).render().normalize(0.55).into_clip(),
    );
    add(
        "djump",
        Tone::new(Wave::Square, 520.0).sweep_to(1100.0).duration(0.12).duty(0.25).adsr(0.003, 0.05, 0.4, 0.05).volume(0.5).render().normalize(0.5).into_clip(),
    );
    add(
        "land",
        Tone::noise().duration(0.07).lowpass_sweep(2600.0, 300.0).percussive().render()
            .mix(Tone::new(Wave::Sine, 130.0).sweep_to(60.0).duration(0.08).percussive().volume(0.7).render())
            .normalize(0.35)
            .into_clip(),
    );
    add(
        "dash",
        Tone::noise().duration(0.18).lowpass_sweep(500.0, 5200.0).adsr(0.01, 0.1, 0.25, 0.07).volume(0.7).render().normalize(0.5).into_clip(),
    );
    add("stomp", synth::sfx::hit());
    add("hurt", synth::sfx::hurt());
    add(
        "firefly",
        note(Wave::Triangle, "E6", 0.08, 0.6).then(note(Wave::Triangle, "B6", 0.2, 0.6)).normalize(0.5).into_clip(),
    );
    add(
        "golden",
        Sound::concat(["C6", "E6", "G6", "C7", "E7"].iter().map(|n| note(Wave::Triangle, n, 0.1, 0.7)))
            .echo(0.18, 0.4, 0.5)
            .normalize(0.6)
            .into_clip(),
    );
    add("checkpoint", synth::sfx::powerup());
    add(
        "spring",
        Tone::new(Wave::Sine, 180.0).sweep_to(760.0).duration(0.2).adsr(0.004, 0.1, 0.3, 0.08).vibrato(18.0, 0.3).volume(0.7).render().normalize(0.55).into_clip(),
    );
    add(
        "gate",
        Tone::noise().duration(0.5).lowpass_sweep(2200.0, 200.0).adsr(0.02, 0.4, 0.0, 0.1).volume(0.5).render()
            .mix(Tone::new(Wave::Saw, 80.0).sweep_to(150.0).duration(0.45).adsr(0.02, 0.3, 0.2, 0.1).lowpass(900.0).volume(0.6).render())
            .normalize(0.55)
            .into_clip(),
    );
    add("exit", synth::sfx::win());
    add(
        "crumble",
        Tone::noise().duration(0.3).lowpass_sweep(1800.0, 150.0).adsr(0.005, 0.25, 0.0, 0.05).volume(0.6).render().normalize(0.4).into_clip(),
    );
    add("shoot", Tone::new(Wave::Saw, 900.0).sweep_to(240.0).duration(0.16).lowpass(3500.0).percussive().volume(0.4).render().normalize(0.4).into_clip());
    add("boss_hit", synth::sfx::explosion());
    add(
        "boss_die",
        Tone::noise().duration(1.4).lowpass_sweep(3200.0, 80.0).adsr(0.01, 1.2, 0.0, 0.15).render()
            .mix(Tone::new(Wave::Sine, 100.0).sweep_to(24.0).duration(1.2).adsr(0.01, 1.0, 0.0, 0.15).render())
            .echo(0.22, 0.4, 0.5)
            .normalize(0.8)
            .into_clip(),
    );
    add("click", synth::sfx::ui_click());
    add("hover", synth::sfx::ui_hover());
    add("confirm", synth::sfx::ui_confirm());
    add("error", synth::sfx::ui_error());
    add("win", synth::sfx::win());
    add("lose", synth::sfx::lose());
    add("warn", Tone::new(Wave::Square, 440.0).duration(0.09).duty(0.5).volume(0.35).percussive().render().normalize(0.3).into_clip());

    // --- музыка (A минор и около) ---
    add(
        "music_menu",
        track(
            78.0,
            "A2 _ _ _ E3 _ _ _ F2 _ _ _ C3 _ _ _ D2 _ _ _ A2 _ _ _ E2 _ _ _ E3 _ _ _",
            "E5 - - - C5 - - - A4 - - - C5 - - - D5 - - - F5 - - - E5 - - - B4 - - -",
            "A3 - - - - - - - F3 - - - - - - - D3 - - - - - - - E3 - - - - - - -",
            "",
            Wave::Sine,
            0.7,
        ),
    );
    add(
        "music_cave",
        track(
            96.0,
            "A2 _ A2 _ E3 _ A2 _ F2 _ F2 _ C3 _ F2 _ D2 _ D2 _ A2 _ D2 _ E2 _ E2 _ B2 _ E2 _",
            "A4 C5 E5 C5 A4 C5 E5 _ F4 A4 C5 A4 F4 A4 C5 _ D5 F5 A5 F5 D5 F5 A5 _ E5 G#4 B4 G#4 E5 B4 G#4 _",
            "A3 - - - - - - - F3 - - - - - - - D3 - - - - - - - E3 - - - - - - -",
            "x...h...x...h...x...h...x...hh..",
            Wave::Triangle,
            0.7,
        ),
    );
    add(
        "music_deep",
        track(
            112.0,
            "D2 _ D2 D3 _ D2 _ D3 Bb1 _ Bb1 Bb2 _ Bb1 _ Bb2 G1 _ G1 G2 _ G1 _ G2 A1 _ A1 A2 _ A1 A2 _",
            "D5 - F5 A5 - F5 D5 - Bb4 - D5 F5 - D5 Bb4 - G4 - Bb4 D5 - Bb4 G4 - A4 C#5 E5 A5 - E5 C#5 -",
            "D3 - - - - - - - Bb2 - - - - - - - G2 - - - - - - - A2 - - - - - - -",
            "x.h.x.h.x.h.x.hhx.h.x.h.x.h.x.hh",
            Wave::Square,
            0.7,
        ),
    );
    add(
        "music_boss",
        track(
            132.0,
            "E2 E2 _ E2 E3 _ E2 E2 C2 C2 _ C2 C3 _ C2 C2 D2 D2 _ D2 D3 _ D2 D2 B1 B1 _ B1 B2 _ B1 B1",
            "E5 G5 B5 G5 E5 G5 B5 _ C5 E5 G5 E5 C5 E5 G5 _ D5 F#5 A5 F#5 D5 F#5 A5 _ B4 D#5 F#5 D#5 B4 D#5 F#5 _",
            "E3 - - - E3 - - - C3 - - - C3 - - - D3 - - - D3 - - - B2 - - - B2 - - -",
            "x.hhx.hhx.hhx.hhx.hhx.hhx.hhx.hh",
            Wave::Saw,
            0.75,
        ),
    );
    out
}
