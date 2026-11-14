//! Звук игры: проигрывание эффектов и музыки. Сами клипы — файлы `assets/audio/*.wav`
//! (их рендерит из синтезатора `examples/bake_assets`, грузит `audio_bank`).

use engine::prelude::*;

/// Сыграть эффект (шина Sfx) с небольшой случайной высотой.
pub fn play(ctx: &mut Ctx, name: &str) {
    let jitter = ctx.random().range(-0.05, 0.05);
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_with(name, PlayParams::new().pitch(1.0 + jitter));
    }
}

/// Сыграть эффект интерфейса (шина Ui).
pub fn play_ui(ctx: &mut Ctx, name: &str) {
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_ui(name);
    }
}

/// Включить музыку с плавным переходом.
pub fn music(ctx: &mut Ctx, name: &str, fade: f32) {
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_music(name, fade);
    }
}

/// Применить громкости из настроек к шинам.
pub fn apply_volumes(audio: &mut Audio, music: f32, sfx: f32) {
    audio.set_bus_volume(Bus::Music, music);
    audio.set_bus_volume(Bus::Sfx, sfx);
    audio.set_bus_volume(Bus::Ui, sfx);
}
