//! Звук игры: всё синтезируется из кода (файлов нет). Эффекты регистрируются по именам —
//! по ним же их зовут механизмы на C++ (`audio::play_at("clang", …)`).

use engine::audio::Level;
use engine::prelude::*;

/// Имена музыкальных петель.
pub const MUSIC_MENU: &str = "music_menu";
pub const MUSIC_FACTORY_A: &str = "music_factory_a";
pub const MUSIC_FACTORY_B: &str = "music_factory_b";

/// Все звуковые эффекты игры (имя → клип): их записывает инструмент сборки в `assets/audio/*.wav`.
pub fn effects() -> Vec<(&'static str, AudioClip)> {
    let mut out: Vec<(&'static str, AudioClip)> = Vec::new();
    // Интерфейс.
    out.push(("ui_click", synth::sfx::ui_click()));
    out.push(("ui_hover", synth::sfx::ui_hover()));
    out.push(("ui_confirm", synth::sfx::ui_confirm()));
    out.push(("ui_error", synth::sfx::ui_error()));

    // Шар.
    out.push(("jump", synth::sfx::jump()));
    out.push(("land", synth::sfx::land()));
    out.push(("roll", roll_loop()));
    out.push(("fell", synth::sfx::hurt()));
    out.push(("respawn", synth::sfx::blip()));

    // Предметы и правила.
    out.push(("gear", synth::sfx::coin()));
    out.push(("golden", synth::sfx::powerup()));
    out.push(("win", synth::sfx::win()));
    out.push(("lose", synth::sfx::lose()));
    out.push(("beep", Tone::new(Wave::Square, 660.0).duration(0.12).duty(0.4).adsr(0.004, 0.05, 0.5, 0.05).volume(0.5).render().into_clip()));
    out.push(("go", Tone::new(Wave::Square, 990.0).sweep_to(1320.0).duration(0.28).duty(0.3).adsr(0.004, 0.08, 0.6, 0.1).volume(0.55).render().render_at(Level::Sfx)));

    // Механизмы (их зовёт C++).
    out.push(("clang", clang()));
    out.push(("bumper", Tone::new(Wave::Triangle, 260.0).sweep_to(760.0).duration(0.14).vibrato(18.0, 0.6).adsr(0.003, 0.06, 0.4, 0.08).volume(0.7).render().into_clip()));
    out.push(("boost", Tone::new(Wave::Saw, 180.0).sweep_to(1100.0).duration(0.35).lowpass_sweep(500.0, 5200.0).adsr(0.01, 0.1, 0.6, 0.15).volume(0.5).render().into_clip()));
    out.push(("crack", crack()));
    out.push(("thud", thud()));
    out.push(("chime", chime()));
    out
}

/// Тихий гул качения: низкочастотный шум без щелчков на стыке петли.
fn roll_loop() -> AudioClip {
    Tone::noise()
        .duration(1.2)
        .adsr(0.0, 0.0, 1.0, 0.0)
        .lowpass(520.0)
        .volume(0.9)
        .seed(5)
        .render()
        .fade_in(0.05)
        .fade_out(0.05)
        .normalize(0.5)
        .into_clip()
}

/// Звон металла: две негармоничные синусоиды и щелчок.
fn clang() -> AudioClip {
    let ring = Tone::new(Wave::Sine, 880.0).duration(0.25).adsr(0.001, 0.2, 0.0, 0.1).volume(0.5).render();
    let ring2 = Tone::new(Wave::Sine, 1383.0).duration(0.2).adsr(0.001, 0.15, 0.0, 0.08).volume(0.35).render();
    let tick = Tone::noise().duration(0.03).percussive().lowpass(6000.0).volume(0.6).render();
    ring.mix(ring2).mix(tick).render_at(Level::Sfx)
}

/// Треск ломающейся плиты.
fn crack() -> AudioClip {
    let a = Tone::noise().duration(0.18).percussive().lowpass_sweep(7000.0, 900.0).volume(0.8).render();
    let b = Tone::new(Wave::Square, 140.0).sweep_to(60.0).duration(0.15).percussive().volume(0.4).render();
    a.mix(b).render_at(Level::Sfx)
}

/// Тяжёлый удар пресса: низкий «бум» и шорох.
fn thud() -> AudioClip {
    let boom = Tone::new(Wave::Sine, 95.0).sweep_to(32.0).duration(0.35).adsr(0.002, 0.3, 0.0, 0.1).volume(1.0).render();
    let hiss = Tone::noise().duration(0.25).percussive().lowpass_sweep(2500.0, 200.0).volume(0.5).render();
    boom.mix(hiss).render_at(Level::Sfx)
}

/// Чистый двухнотный сигнал контрольной точки и финиша.
fn chime() -> AudioClip {
    Sound::concat(["E5", "B5"].iter().map(|note| {
        Tone::note(Wave::Triangle, note).duration(0.09).adsr(0.002, 0.2, 0.2, 0.25).volume(0.6).render()
    }))
    .echo(0.14, 0.3, 0.35)
    .normalize(0.8)
    .into_clip()
}

/// Музыка меню: спокойная, минорная.
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
    // Громкость по политике (−14 LUFS): меню не громче цехов.
    Sound::mix_all([bass, pad, lead]).dc_block().render_at(Level::Music)
}

/// Музыка цехов 1–4: бодрый марш на заводском ритме.
pub fn music_factory_a() -> AudioClip {
    factory_music(112.0, "x...x...x...x.x.", false)
}

/// Музыка цехов 5–8: быстрее и злее.
pub fn music_factory_b() -> AudioClip {
    factory_music(132.0, "x..xx...x.x.x..x", true)
}

fn factory_music(bpm: f32, drum_pattern: &str, tense: bool) -> AudioClip {
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
    // Хэт — полоса шума 6–10 кГц, тихо: ритм без шипения.
    let hat = Tone::noise().duration(0.03).percussive().highpass(6_000.0).lowpass(10_000.0).volume(0.07).render();
    // Рисунок рассчитан на 16 шагов; такт мелодии выше — два такта по 16 шагов, поэтому повторяем.
    let drums = Sound::pattern(&kick, drum_pattern, step).repeat(2);
    let hats = Sound::pattern(&hat, "..x...x...x...x.", step).repeat(2);
    Sound::mix_all([bass, lead, drums, hats]).dc_block().render_at(Level::Music)
}
