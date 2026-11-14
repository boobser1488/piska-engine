//! Звук игры: шаги, прыжок, приземление, отметки полосы, щелчки интерфейса. Клипы лежат
//! в `assets/audio/*.wav` (их один раз рендерит `examples/bake_assets.rs` из синтезатора по
//! политике громкости движка: эффекты — пик −2 дБFS, интерфейс тише; музыки нет), список —
//! `assets/data/sounds.ron`.

use engine::prelude::*;

use super::data::{load_ron, SoundList};

/// Зарегистрировать клипы из ассетов в `Audio` (повторный вызов ничего не делает).
pub fn load(world: &mut World) {
    let already = world.resources.get::<Audio>().is_none_or(|audio| audio.has_clip("ui_click"));
    if already {
        return;
    }
    let Some(list) = load_ron::<SoundList>(world, "data/sounds.ron") else { return };
    let Some(database) = world.resources.get::<AssetDatabase>().cloned() else { return };
    let Some(audio) = world.resources.get_mut::<Audio>() else { return };
    for (name, path) in &list.effects {
        let clip = database
            .read_path(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| AudioClip::from_bytes(&bytes).map_err(|e| e.to_string()));
        match clip {
            Ok(clip) => audio.add_clip(name.as_str(), clip),
            Err(err) => log::error!("sound `{name}` ({path}): {err}"),
        }
    }
}

/// Проиграть эффект с громкостью `volume` и высотой `pitch` (нет звука — ничего).
pub fn play(world: &mut World, name: &str, volume: f32, pitch: f32) {
    if let Some(audio) = world.resources.get_mut::<Audio>() {
        if audio.has_clip(name) {
            audio.play_with(name, PlayParams::default().volume(volume).pitch(pitch));
        }
    }
}

/// Щелчок интерфейса.
pub fn click(world: &mut World) {
    if let Some(audio) = world.resources.get_mut::<Audio>() {
        if audio.has_clip("ui_click") {
            audio.play_ui("ui_click");
        }
    }
}
