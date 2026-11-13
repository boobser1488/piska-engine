//! Клипы-ассеты читаются через базу ассетов, поэтому работают и из пака (браузер,
//! Android), где каталога ассетов на диске нет.

use engine_assets::{AssetDatabase, AssetPack, Guid};
use engine_audio::{Audio, AudioClip, ClipRef};

fn meta(guid: Guid, importer: &str) -> Vec<u8> {
    format!("(\n    guid: \"{:032x}\",\n    importer: \"{importer}\",\n    settings: {{}},\n)\n", guid.0).into_bytes()
}

#[test]
fn audio_asset_plays_from_a_pack() {
    let guid = Guid(0xA0D10);
    let clip = AudioClip::from_samples(22_050, 1, vec![0.25; 2_205]);
    let mut builder = AssetPack::builder();
    builder.add("sounds/beep.wav", clip.to_wav_bytes());
    builder.add("sounds/beep.wav.meta", meta(guid, "Audio"));
    let database = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
    assert!(database.is_packed());
    // В паке нет файлов на диске: путь `path_of` ведёт в никуда.
    assert!(!database.path_of(guid).unwrap().exists());

    let mut audio = Audio::new();
    audio.set_asset_database(database);
    let loaded = audio.resolve(ClipRef::Asset(guid)).expect("the clip is decoded from the pack");
    assert_eq!(loaded.frames(), 2_205);
    assert_eq!(loaded.sample_rate(), 22_050);
    // Неизвестный GUID — `None` (и предупреждение в логе), без паники.
    assert!(audio.resolve(ClipRef::Asset(Guid(1))).is_none());
}
