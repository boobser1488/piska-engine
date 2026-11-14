//! Звук игры: клипы лежат в `assets/audio/*.wav` (их один раз рендерит инструмент
//! `examples/bake_assets.rs` из синтезатора — можно заменить своими файлами), список —
//! `assets/data/sounds.ron`. Эффекты регистрируются по именам, по ним же их зовут скрипты
//! механизмов (`util::play_at(ctx, "clang", …)`).

use engine::prelude::*;

use super::data::{load_ron, SoundList};

/// Имена музыкальных петель.
pub const MUSIC_MENU: &str = "music_menu";
pub const MUSIC_FACTORY_A: &str = "music_factory_a";
pub const MUSIC_FACTORY_B: &str = "music_factory_b";

/// Зарегистрировать клипы из ассетов в `Audio` (повторный вызов ничего не делает).
pub fn load(world: &mut World) {
    let already = world.resources.get::<Audio>().is_none_or(|audio| audio.has_clip("ui_click"));
    if already {
        return;
    }
    let Some(list) = load_ron::<SoundList>(world, "data/sounds.ron") else { return };
    let Some(database) = world.resources.get::<AssetDatabase>().cloned() else { return };
    let Some(audio) = world.resources.get_mut::<Audio>() else { return };
    for (name, path) in list.effects.iter().chain(list.music.iter()) {
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
