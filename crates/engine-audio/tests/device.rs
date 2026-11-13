//! Проверка настоящей звуковой карты. По умолчанию выключена (`cargo test -- --ignored`):
//! обычные тесты устройство не трогают. Звук здесь беззвучный (громкость 0).

use engine_audio::{install, Audio, AudioClip, DeviceStatus, PlayParams};
use engine_core::app::App;

#[test]
#[ignore = "opens the real audio device"]
fn device_opens_lazily_or_fails_quietly() {
    let mut app = App::new();
    install(&mut app);
    app.run_headless(2, 1.0 / 60.0);
    assert_eq!(
        app.world.resources.get::<Audio>().unwrap().device_status(),
        &DeviceStatus::NotOpened,
        "the device stays closed until the first sound"
    );

    app.world
        .resources
        .get_mut::<Audio>()
        .unwrap()
        .add_clip("silence", AudioClip::mono(44_100, vec![0.3; 44_100 * 4]));
    let voice = app
        .world
        .resources
        .get_mut::<Audio>()
        .unwrap()
        .play_with("silence", PlayParams::new().volume(0.0));
    app.run_headless(3, 1.0 / 60.0);

    let audio = app.world.resources.get_mut::<Audio>().unwrap();
    println!("device status: {:?}", audio.device_status());
    match audio.device_status() {
        DeviceStatus::Active { sample_rate, channels, .. } => {
            assert!(*sample_rate >= 8000 && *channels >= 1);
            // Играет настоящий поток: позиция голоса уходит вперёд сама, без кадров игры.
            // Bluetooth-наушникам нужно время, чтобы поток пошёл: ждём, потом меряем скорость.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let before = audio.position(voice).unwrap_or(0.0);
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let after = audio.position(voice).unwrap_or(4.0);
            println!("voice position {before} -> {after} over 1 s of wall time");
            assert!((after - before - 1.0).abs() < 0.2, "the device thread plays in real time");
        }
        DeviceStatus::Unavailable(reason) => assert!(!reason.is_empty()),
        other => panic!("unexpected status {other:?}"),
    }
}
