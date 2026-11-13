//! Звук без окна и без звуковой карты: компоненты → микшер.

use engine::prelude::*;

const DT: f32 = 1.0 / 60.0;

fn tone(seconds: f32) -> AudioClip {
    Tone::new(Wave::Sine, 300.0)
        .duration(seconds)
        .adsr(0.0, 0.0, 1.0, 0.0)
        .volume(0.5)
        .render()
        .into_clip()
}

/// Приложение со звуком, который не трогает устройство.
fn silent_app() -> App {
    let mut app = engine::app();
    let audio = app.world.resources.get_mut::<Audio>().expect("engine::app installs Audio");
    audio.set_device_enabled(false);
    audio.add_clip("loop", tone(2.0));
    audio.add_clip("short", tone(0.1));
    app
}

fn audio(app: &mut App) -> &mut Audio {
    app.world.resources.get_mut::<Audio>().unwrap()
}

fn peaks(app: &mut App, seconds: f32) -> (f32, f32) {
    let mut buf = vec![0.0; (48_000.0 * seconds) as usize * 2];
    audio(app).render(&mut buf);
    buf.chunks(2).fold((0.0, 0.0), |(l, r), f| (f32::max(l, f[0].abs()), f32::max(r, f[1].abs())))
}

#[test]
fn play_on_start_source_makes_a_voice_and_destroying_the_object_stops_it() {
    let mut app = silent_app();
    let id = app.spawn("Music box").with(AudioSource::named("loop").with_looping(true)).id();
    assert_eq!(audio(&mut app).voice_count(), 0, "nothing starts before the first frame");

    app.tick(DT);
    assert_eq!(audio(&mut app).voice_count(), 1);
    assert!(audio(&mut app).is_source_playing(id));

    app.world.scene.destroy(id);
    app.run_headless(2, DT);
    assert_eq!(audio(&mut app).voice_count(), 0);
    assert!(!audio(&mut app).is_source_playing(id));
}

#[test]
fn one_shot_source_ends_by_itself_and_does_not_restart() {
    let mut app = silent_app();
    let id = app.spawn("Blip").with(AudioSource::named("short")).id();
    app.tick(DT);
    assert!(audio(&mut app).is_source_playing(id));
    // Клип 0.1 с, кадров по 1/60: к 12-му кадру он доигран.
    app.run_headless(12, DT);
    assert!(!audio(&mut app).is_source_playing(id));
    app.run_headless(30, DT);
    assert_eq!(audio(&mut app).voice_count(), 0, "play_on_start fires only once");

    // Ручной перезапуск.
    audio(&mut app).play_source(id);
    app.tick(DT);
    assert!(audio(&mut app).is_source_playing(id));
}

#[test]
fn manual_source_waits_for_play_source_and_stop_source_works() {
    let mut app = silent_app();
    let id = app.spawn("Engine").with(AudioSource::named("loop").with_looping(true).manual()).id();
    app.run_headless(3, DT);
    assert_eq!(audio(&mut app).voice_count(), 0);
    audio(&mut app).play_source(id);
    app.tick(DT);
    assert!(audio(&mut app).is_source_playing(id));
    audio(&mut app).stop_source(id);
    app.run_headless(2, DT);
    assert!(!audio(&mut app).is_source_playing(id));
    assert_eq!(audio(&mut app).voice_count(), 0);
}

#[test]
fn source_that_is_registered_late_starts_when_the_clip_appears() {
    let mut app = silent_app();
    let id = app.spawn("Late").with(AudioSource::named("later").with_looping(true)).id();
    app.run_headless(3, DT);
    assert_eq!(audio(&mut app).voice_count(), 0, "unknown clip: silent, no panic");
    audio(&mut app).add_clip("later", tone(1.0));
    app.tick(DT);
    assert!(audio(&mut app).is_source_playing(id));
}

#[test]
fn component_fields_reach_the_voice_live() {
    let mut app = silent_app();
    let id = app
        .spawn("Source")
        .with(AudioSource::named("loop").with_looping(true).with_volume(1.0))
        .id();
    app.tick(DT);
    let (_, loud) = peaks(&mut app, 0.1);
    app.world.scene.get_component_mut::<AudioSource>(id).unwrap().volume = 0.25;
    app.run_headless(2, DT);
    peaks(&mut app, 0.05); // даём громкости перейти плавно
    let (_, quiet) = peaks(&mut app, 0.1);
    assert!(loud > 0.45, "{loud}");
    assert!((quiet - loud * 0.25).abs() < 0.02, "{quiet} vs {loud}");

    // Шина тоже меняется на лету.
    audio(&mut app).set_bus_muted(Bus::Sfx, true);
    peaks(&mut app, 0.05);
    assert!(peaks(&mut app, 0.05).1 < 1e-4);
}

#[test]
fn spatial_source_pans_toward_its_side_and_fades_with_distance() {
    let mut app = silent_app();
    app.spawn("Ears").with(AudioListener::default());
    let id = app
        .spawn("Right speaker")
        .at(6.0, 0.0)
        .with(AudioSource::named("loop").with_looping(true).with_spatial(1.0, 20.0))
        .id();
    app.run_headless(2, DT);
    peaks(&mut app, 0.05);
    let (l, r) = peaks(&mut app, 0.1);
    assert!(r > l * 3.0, "right {r} vs left {l}");
    let at_six = r;

    app.world.scene.get_mut(id).unwrap().transform.position = Vec3::new(15.0, 0.0, 0.0);
    app.run_headless(2, DT);
    peaks(&mut app, 0.05);
    let (_, farther) = peaks(&mut app, 0.1);
    assert!(farther < at_six * 0.6, "{farther} vs {at_six}");

    app.world.scene.get_mut(id).unwrap().transform.position = Vec3::new(-0.5, 0.0, 0.0);
    app.run_headless(2, DT);
    peaks(&mut app, 0.05);
    let (l, r) = peaks(&mut app, 0.1);
    assert!(l > 0.45 && l > r, "inside min distance: full volume, panned left ({l}, {r})");
}

#[test]
fn spatial_works_in_3d_with_a_rotated_listener() {
    let mut app = silent_app();
    // Слушатель смотрит вдоль +X (повёрнут на 90° вокруг Y): «вправо» для него — это -Z... проверяем по формуле.
    let ears = app.spawn("Ears").with(AudioListener::default()).id();
    app.world.scene.get_mut(ears).unwrap().transform.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let right = app.world.scene.world_rotation(ears) * Vec3::X;
    app.spawn("Source")
        .at3(right.x * 5.0, 0.0, right.z * 5.0)
        .with(AudioSource::named("loop").with_looping(true).with_spatial(1.0, 30.0));
    app.run_headless(2, DT);
    peaks(&mut app, 0.05);
    let (l, r) = peaks(&mut app, 0.1);
    assert!(r > l * 3.0, "{l} {r}");
}

#[test]
fn deactivating_the_object_silences_it_and_reactivating_restarts_it() {
    let mut app = silent_app();
    let id = app.spawn("Source").with(AudioSource::named("loop").with_looping(true)).id();
    app.tick(DT);
    assert_eq!(audio(&mut app).voice_count(), 1);
    app.world.scene.get_mut(id).unwrap().active = false;
    app.run_headless(2, DT);
    assert_eq!(audio(&mut app).voice_count(), 0);
    app.world.scene.get_mut(id).unwrap().active = true;
    app.run_headless(2, DT);
    assert_eq!(audio(&mut app).voice_count(), 1);
}

#[test]
fn editor_pause_freezes_and_stop_silences_everything() {
    let mut app = silent_app();
    app.spawn("Source").with(AudioSource::named("loop").with_looping(true));
    app.tick(DT);
    let one_shot = audio(&mut app).play("loop");
    audio(&mut app).play_music("short", 0.0);
    assert_eq!(audio(&mut app).voice_count(), 3);

    // Pause в редакторе: приложение зовёт idle, звук замирает, голоса остаются.
    let before = audio(&mut app).position(one_shot).unwrap();
    app.idle();
    assert!(audio(&mut app).with_mixer_mut(|m| m.is_paused()));
    assert!(peaks(&mut app, 0.05) == (0.0, 0.0));
    assert_eq!(audio(&mut app).voice_count(), 3);
    assert_eq!(audio(&mut app).position(one_shot).unwrap(), before);
    // Play снова: звук продолжается.
    app.tick(DT);
    assert!(!audio(&mut app).with_mixer_mut(|m| m.is_paused()));
    assert!(peaks(&mut app, 0.05).0 > 0.1);

    // Stop: сцена восстановлена из снимка, ничто не должно звучать.
    let snapshot = app.world.snapshot();
    app.world.restore(&snapshot).unwrap();
    app.idle();
    assert_eq!(audio(&mut app).voice_count(), 0, "everything is silent after Stop");
    assert_eq!(peaks(&mut app, 0.05), (0.0, 0.0));

    // Следующий Play: источник сцены стартует заново.
    app.tick(DT);
    assert_eq!(audio(&mut app).voice_count(), 1);
}

#[test]
fn audio_source_is_saved_in_scenes_and_visible_to_the_inspector() {
    let mut app = silent_app();
    app.spawn("Source").with(
        AudioSource::named("coin")
            .with_volume(0.5)
            .with_bus(Bus::Ui)
            .with_spatial(2.0, 9.0)
            .manual(),
    );
    app.spawn("Ears").with(AudioListener { volume: 0.8 });
    let text = app.world.scene_to_string();
    assert!(text.contains("AudioSource") && text.contains("AudioListener"), "{text}");
    assert!(text.contains("coin") && text.contains("Ui"), "{text}");

    let mut other = engine::app();
    other.world.load_scene_str(&text, "test").unwrap();
    let id = other.world.scene.find_by_name("Source").unwrap();
    let source = other.world.scene.get_component::<AudioSource>(id).unwrap();
    assert_eq!(source.clip_name, "coin");
    assert_eq!(source.volume, 0.5);
    assert_eq!(source.bus, Bus::Ui);
    assert!(source.spatial && !source.play_on_start);
    assert_eq!((source.min_distance, source.max_distance), (2.0, 9.0));
    assert_eq!(source.reflect_clip_kind(), "Audio");
}

trait ClipKind {
    fn reflect_clip_kind(&self) -> &'static str;
}
impl ClipKind for AudioSource {
    fn reflect_clip_kind(&self) -> &'static str {
        // Инспектор по этому полю предлагает аудиофайлы проекта.
        <AudioSource as Reflect>::field_meta(self, "clip").asset_kind
    }
}

#[test]
fn script_api_music_crossfade_and_bus_volumes() {
    let mut app = silent_app();
    audio(&mut app).add_clip("theme_a", tone(1.0));
    audio(&mut app).add_clip("theme_b", tone(1.0));

    let a = audio(&mut app).play_music("theme_a", 0.5);
    assert!(audio(&mut app).is_music_playing());
    // Тот же клип второй раз — не перезапуск.
    assert_eq!(audio(&mut app).play_music("theme_a", 0.5), a);

    app.run_headless(60, DT);
    let b = audio(&mut app).play_music("theme_b", 0.5);
    assert_ne!(a, b);
    assert!(audio(&mut app).is_playing(a) && audio(&mut app).is_playing(b), "both are audible during the crossfade");
    app.run_headless(45, DT); // 0.75 с > 0.5 с
    assert!(!audio(&mut app).is_playing(a), "the old track has faded out");
    assert!(audio(&mut app).is_playing(b));
    assert_eq!(audio(&mut app).music_voice(), Some(b));

    audio(&mut app).stop_music(0.2);
    app.run_headless(20, DT);
    assert!(!audio(&mut app).is_music_playing());
    assert_eq!(audio(&mut app).voice_count(), 0);

    // Шины.
    audio(&mut app).set_bus_volume(Bus::Music, 0.3);
    audio(&mut app).set_bus_muted(Bus::Ui, true);
    assert_eq!(audio(&mut app).bus_volume(Bus::Music), 0.3);
    assert!(audio(&mut app).is_bus_muted(Bus::Ui));
    assert!(!audio(&mut app).is_bus_muted(Bus::Sfx));
}

#[test]
fn script_api_handles_pause_and_stop_all() {
    let mut app = silent_app();
    let one = audio(&mut app).play_with("loop", PlayParams::new().volume(0.5).pitch(2.0).looping(true));
    let two = audio(&mut app).play_at("loop", Vec3::new(3.0, 0.0, 0.0));
    app.tick(DT);
    assert!(audio(&mut app).is_playing(one) && audio(&mut app).is_playing(two));

    audio(&mut app).pause_all();
    let t = audio(&mut app).position(one).unwrap();
    app.run_headless(10, DT);
    assert_eq!(audio(&mut app).position(one).unwrap(), t, "paused voices do not advance");
    audio(&mut app).resume_all();
    app.run_headless(10, DT);
    assert!(audio(&mut app).position(one).unwrap() > t);

    audio(&mut app).set_pitch(one, 1.0);
    audio(&mut app).fade_out(one, 0.1);
    app.run_headless(8, DT);
    assert!(!audio(&mut app).is_playing(one));

    audio(&mut app).stop(two);
    assert!(!audio(&mut app).is_playing(two));
    audio(&mut app).play("loop");
    audio(&mut app).stop_all();
    assert_eq!(audio(&mut app).voice_count(), 0);
}

#[test]
fn missing_clip_is_not_fatal_and_headless_run_has_no_device() {
    let mut app = silent_app();
    let voice = audio(&mut app).play("does-not-exist");
    assert!(voice.is_none());
    assert!(!audio(&mut app).is_playing(voice));
    audio(&mut app).stop(voice);
    audio(&mut app).set_volume(voice, 1.0);
    app.run_headless(5, DT);
    assert_eq!(audio(&mut app).device_status(), &DeviceStatus::Disabled);
}

#[test]
fn scripts_can_play_sounds_from_hooks() {
    component! {
        /// Играет звук при каждом нажатии пробела.
        pub struct Jumper {
            pub jumps: i32 = 0,
        }
    }
    impl Component for Jumper {
        fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
            if ctx.input().key_down(Key::Space) {
                self.jumps += 1;
                if let Some(audio) = ctx.resource_mut::<Audio>() {
                    audio.play("short");
                }
            }
        }
    }

    let mut app = silent_app();
    let id = app.spawn("Hero").with(Jumper::default()).id();
    app.tick(DT);
    assert_eq!(audio(&mut app).voice_count(), 0);
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<Jumper>(id).unwrap().jumps, 1);
    assert_eq!(audio(&mut app).voice_count(), 1);
}

#[test]
fn synthesized_presets_register_and_play_by_name() {
    let mut app = silent_app();
    let audio = audio(&mut app);
    audio.add_clip("jump", synth::sfx::jump());
    audio.add_clip("boom", synth::sfx::explosion());
    let jump = audio.play("jump");
    let boom = audio.play_with("boom", PlayParams::new().volume(0.7).bus(Bus::Sfx));
    assert!(audio.is_playing(jump) && audio.is_playing(boom));
    let mut buf = vec![0.0; 4800 * 2];
    audio.render(&mut buf);
    assert!(buf.iter().any(|s| s.abs() > 0.05));
}
