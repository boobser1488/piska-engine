//! Звук «Роя»: эффекты и музыка лежат в `assets/audio/*.wav` (их один раз рендерит из
//! синтезатора движка запекатель `examples/bake_assets`, файлы можно заменить своими).

use engine::prelude::*;

use crate::profile::Options;

pub const CLIPS: [&str; 33] = ["shoot", "hit", "crit", "die", "boom", "big_boom", "pickup", "repair", "levelup", "dash", "player_hurt", "core_hurt", "player_die", "respawn", "wave_start", "wave_clear", "boss_warn", "boss_die", "shield", "missile", "ricochet", "enemy_shot", "mine", "spawn", "ui_click", "ui_hover", "ui_confirm", "ui_error", "win", "lose", "music_menu", "music_game", "music_boss"];

/// Зарегистрировать все клипы: читаются из базы ассетов (каталог или пак).
pub fn register(audio: &mut Audio, db: &AssetDatabase) {
    for name in CLIPS {
        let path = format!("audio/{name}.wav");
        let clip = db.read_path(&path).map_err(|e| e.to_string()).and_then(|bytes| AudioClip::from_bytes(&bytes).map_err(|e| e.to_string()));
        match clip {
            Ok(clip) => audio.add_clip(name, clip),
            Err(e) => log::error!("sound `{path}` is not loaded: {e}. Re-bake assets: cargo run -p swarm --example bake_assets"),
        }
    }
}

/// Громкости шин из настроек: музыка отдельно, эффекты и интерфейс — по ползунку звука.
pub fn apply_volumes(audio: &mut Audio, opts: &Options) {
    audio.set_bus_volume(Bus::Master, 1.0);
    audio.set_bus_volume(Bus::Music, opts.music);
    audio.set_bus_volume(Bus::Sfx, opts.sfx);
    audio.set_bus_volume(Bus::Ui, opts.sfx);
}

/// Проиграть эффект с небольшим разбросом высоты.
pub fn play(audio: &mut Audio, clip: &str, volume: f32, pitch: f32) {
    audio.play_with(clip, PlayParams::new().volume(volume).pitch(pitch));
}
