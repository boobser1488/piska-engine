//! Синтез звуков и музыки «Роя» (запекатель пишет результат в `assets/audio/*.wav`).
//!
//! Уровни — по политике громкости движка (`docs/api/audio.md`): громкие эффекты
//! `render_at(Level::Sfx)` (пик 0.8), музыка `render_at(Level::Music)` (−14 LUFS), поэтому
//! меню, бой и босс звучат одинаково громко. Хэты — полоса шума 6–10 кГц, а не весь шум.

use engine::prelude::*;
use engine::audio::Level;

fn noise_hit(len: f32, from: f32, to: f32, vol: f32) -> Sound {
    Tone::noise().duration(len).lowpass_sweep(from, to).percussive().volume(vol).render()
}

fn thump(freq: f32, end: f32, len: f32, vol: f32) -> Sound {
    Tone::new(Wave::Sine, freq).sweep_to(end).duration(len).percussive().volume(vol).render()
}

/// Сыграть синтезатором все клипы игры (звуки + три музыкальные петли) и отдать их `add`.
/// Нужна только запекателю (`examples/bake_assets`): в игре звуки читаются из `assets/audio`.
pub fn render_clips(mut add: impl FnMut(&str, AudioClip)) {
    add(
        "shoot",
        Tone::new(Wave::Saw, 1500.0).sweep_to(420.0).duration(0.06).adsr(0.002, 0.03, 0.2, 0.03).volume(0.5).lowpass(5200.0).render().into_clip(),
    );
    add("hit", noise_hit(0.05, 7000.0, 900.0, 0.55).mix(Tone::new(Wave::Square, 520.0).sweep_to(260.0).duration(0.04).percussive().volume(0.25).render()).into_clip());
    add("crit", Tone::new(Wave::Square, 900.0).sweep_to(1800.0).duration(0.07).adsr(0.002, 0.04, 0.2, 0.05).volume(0.45).render().mix(noise_hit(0.06, 8000.0, 1500.0, 0.5)).into_clip());
    add("die", noise_hit(0.2, 4200.0, 300.0, 0.7).mix(thump(210.0, 60.0, 0.16, 0.9)).render_at(Level::Sfx));
    add(
        "boom",
        Tone::noise().duration(0.7).adsr(0.002, 0.6, 0.0, 0.1).lowpass_sweep(3000.0, 120.0).render().mix(thump(100.0, 30.0, 0.6, 1.0)).render_at(Level::Sfx),
    );
    add(
        "big_boom",
        Tone::noise().duration(1.6).adsr(0.002, 1.4, 0.0, 0.2).lowpass_sweep(2600.0, 70.0).render().mix(thump(80.0, 24.0, 1.4, 1.0)).echo(0.18, 0.35, 0.5).render_at(Level::Sfx),
    );
    add("pickup", Tone::new(Wave::Triangle, 1100.0).sweep_to(1900.0).duration(0.05).adsr(0.002, 0.03, 0.2, 0.03).volume(0.45).render().into_clip());
    add("repair", Sound::concat(["E5", "B5"].iter().map(|n| Tone::note(Wave::Triangle, n).duration(0.07).volume(0.5).render())).into_clip());
    add("levelup", synth::sfx::powerup());
    add(
        "dash",
        Tone::noise().duration(0.2).adsr(0.01, 0.12, 0.0, 0.05).lowpass_sweep(600.0, 4200.0).volume(0.6).render().mix(Tone::new(Wave::Sine, 300.0).sweep_to(900.0).duration(0.16).percussive().volume(0.3).render()).into_clip(),
    );
    add("player_hurt", synth::sfx::hurt());
    add(
        "core_hurt",
        Tone::new(Wave::Square, 130.0).duration(0.12).adsr(0.002, 0.05, 0.6, 0.06).volume(0.45).render().then(Tone::new(Wave::Square, 98.0).duration(0.16).adsr(0.002, 0.08, 0.5, 0.08).volume(0.45).render()).into_clip(),
    );
    add("player_die", noise_hit(0.5, 5000.0, 200.0, 0.8).mix(thump(180.0, 40.0, 0.5, 1.0)).echo(0.12, 0.3, 0.4).render_at(Level::Sfx));
    add("respawn", Sound::concat(["C4", "G4", "C5"].iter().map(|n| Tone::note(Wave::Square, n).duration(0.07).duty(0.25).volume(0.4).render())).into_clip());
    add(
        "wave_start",
        Tone::note(Wave::Saw, "A3").duration(0.22).adsr(0.01, 0.1, 0.6, 0.1).volume(0.5).lowpass(2400.0).render().then(Tone::note(Wave::Saw, "E4").duration(0.34).adsr(0.01, 0.1, 0.6, 0.2).volume(0.5).lowpass(2400.0).render()).into_clip(),
    );
    add(
        "wave_clear",
        Sound::concat(["C5", "E5", "G5", "C6"].iter().map(|n| Tone::note(Wave::Square, n).duration(0.075).duty(0.25).volume(0.4).render())).echo(0.14, 0.3, 0.4).into_clip(),
    );
    add(
        "boss_warn",
        Sound::concat((0..3).map(|_| Tone::new(Wave::Saw, 180.0).sweep_to(300.0).duration(0.4).adsr(0.02, 0.1, 0.7, 0.1).volume(0.5).lowpass(2000.0).render().pad(0.12))).into_clip(),
    );
    add("boss_die", Tone::noise().duration(2.0).adsr(0.002, 1.8, 0.0, 0.2).lowpass_sweep(3200.0, 60.0).render().mix(thump(70.0, 20.0, 1.8, 1.0)).mix_at(Sound::concat(["C5", "G5", "C6"].iter().map(|n| Tone::note(Wave::Square, n).duration(0.12).duty(0.25).volume(0.35).render())), 0.9).echo(0.2, 0.4, 0.5).render_at(Level::Sfx));
    add("shield", Tone::new(Wave::Triangle, 1700.0).sweep_to(1100.0).duration(0.06).percussive().volume(0.45).render().into_clip());
    add("missile", Tone::noise().duration(0.3).adsr(0.02, 0.2, 0.0, 0.08).lowpass_sweep(1200.0, 4200.0).volume(0.55).render().into_clip());
    add("ricochet", Tone::new(Wave::Square, 1600.0).sweep_to(2400.0).duration(0.04).percussive().volume(0.3).render().into_clip());
    add("enemy_shot", Tone::new(Wave::Square, 560.0).sweep_to(240.0).duration(0.1).adsr(0.002, 0.05, 0.3, 0.04).volume(0.35).duty(0.3).render().into_clip());
    add("mine", Tone::new(Wave::Triangle, 700.0).duration(0.05).percussive().volume(0.4).render().then(Tone::new(Wave::Triangle, 700.0).duration(0.05).percussive().volume(0.4).render().pad(0.06)).into_clip());
    add("spawn", Tone::new(Wave::Sine, 220.0).sweep_to(440.0).duration(0.1).percussive().volume(0.18).render().into_clip());
    add("ui_click", synth::sfx::ui_click());
    add("ui_hover", synth::sfx::ui_hover());
    add("ui_confirm", synth::sfx::ui_confirm());
    add("ui_error", synth::sfx::ui_error());
    add("win", synth::sfx::win());
    add("lose", synth::sfx::lose());

    add("music_menu", menu_music());
    add("music_game", game_music());
    add("music_boss", boss_music());
}

/// Спокойная фоновая петля меню: медленные аккорды и редкая мелодия.
fn menu_music() -> AudioClip {
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
    // Эхо заворачивается внутрь петли: длина ровно 32 шага, стык без скачка.
    Sound::mix_all([pad, arp, lead]).echo_loop(0.36, 0.3, 0.3).dc_block().render_at(Level::Music)
}

/// Основная боевая петля: бас, ударные и мелодия, 128 BPM, 64 шага (8 тактов).
fn game_music() -> AudioClip {
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

/// Тревожная петля для боссов: быстрее и ниже, 64 шага.
fn boss_music() -> AudioClip {
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

/// Подогнать длину звука под `len` сэмплов (обрезать или добить тишиной).
fn fit(mut sound: Sound, len: usize) -> Sound {
    sound.samples.resize(len, 0.0);
    sound
}

