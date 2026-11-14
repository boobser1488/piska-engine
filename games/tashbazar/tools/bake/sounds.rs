//! Звуки из синтезатора движка по его политике громкости (docs/api/audio.md): эффекты —
//! `render_at(Level::Sfx)` (пик −2 дБFS), интерфейс — `Level::Ui` (тише). Музыки нет.

use engine::audio::synth::Level;
use engine::prelude::*;

/// (имя, клип).
pub fn effects() -> Vec<(&'static str, AudioClip)> {
    // Шаг по камню и песку: короткий шум с низким «тук», две разные версии.
    let step = |seed: u32, low: f32| {
        Tone::noise()
            .seed(seed)
            .duration(0.06)
            .lowpass_sweep(2400.0, 500.0)
            .highpass(120.0)
            .percussive()
            .render()
            .mix(Tone::new(Wave::Sine, low).sweep_to(low * 0.6).duration(0.05).percussive().volume(0.6).render())
            .render_at(Level::Sfx)
    };
    let land = Tone::noise()
        .seed(7)
        .duration(0.12)
        .lowpass_sweep(1600.0, 180.0)
        .percussive()
        .render()
        .mix(Tone::new(Wave::Sine, 110.0).sweep_to(55.0).duration(0.12).percussive().volume(0.9).render())
        .render_at(Level::Sfx);
    let jump = Tone::noise().seed(3).duration(0.08).highpass(400.0).lowpass(3000.0).percussive().volume(0.5).render().render_at(Level::Sfx);
    let checkpoint = Tone::new(Wave::Triangle, 880.0)
        .duration(0.07)
        .adsr(0.003, 0.05, 0.4, 0.06)
        .render()
        .then(Tone::new(Wave::Triangle, 1318.5).duration(0.14).adsr(0.003, 0.1, 0.2, 0.1).render())
        .render_at(Level::Sfx);
    let finish = Tone::new(Wave::Triangle, 659.3)
        .duration(0.09)
        .render()
        .then(Tone::new(Wave::Triangle, 880.0).duration(0.09).render())
        .then(Tone::new(Wave::Triangle, 1318.5).duration(0.3).adsr(0.004, 0.2, 0.3, 0.2).render())
        .render_at(Level::Sfx);
    let click = Tone::new(Wave::Triangle, 1500.0).sweep_to(900.0).duration(0.03).percussive().render_at(Level::Ui);
    vec![
        ("step_a", step(11, 150.0)),
        ("step_b", step(29, 132.0)),
        ("land", land),
        ("jump", jump),
        ("checkpoint", checkpoint),
        ("finish", finish),
        ("ui_click", click),
    ]
}
