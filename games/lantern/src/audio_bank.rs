//! Звуковой банк: все `assets/audio/*.wav` регистрируются в `Audio` под именем файла
//! (`jump.wav` → `"jump"`, `music_cave.wav` → `"music_cave"`). Читаем через базу ассетов —
//! так работает и пак (браузер, Android).

use engine::prelude::*;

/// Загрузить клипы из каталога `audio/` ассетов.
pub fn install(app: &mut App) -> Result<(), engine::assets::AssetError> {
    let db = app
        .world
        .resources
        .get::<AssetDatabase>()
        .expect("mount_assets inserted the database")
        .clone();
    let names: Vec<String> = db
        .assets()
        .filter_map(|(_, path)| path.strip_prefix("audio/").and_then(|p| p.strip_suffix(".wav")).map(str::to_string))
        .collect();
    let Some(audio) = app.world.resources.get_mut::<Audio>() else {
        return Ok(());
    };
    for name in names {
        let path = format!("audio/{name}.wav");
        let bytes = db.read_path(&path)?;
        let clip = AudioClip::from_bytes(&bytes)
            .map_err(|e| engine::assets::AssetError::Import { path: path.clone(), reason: e.to_string() })?;
        audio.add_clip(name, clip);
    }
    Ok(())
}
