//! Проверки микшера по отрисованным сэмплам: никакого устройства.

use std::sync::Arc;

use engine_audio::{spatialize, AudioClip, Bus, ListenerState, Mixer, VoiceParams};
use engine_core::prelude::Vec3;

const RATE: u32 = 44_100;

fn sine(freq: f32, amplitude: f32, seconds: f32) -> Arc<AudioClip> {
    let n = (RATE as f32 * seconds) as usize;
    let samples = (0..n)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin())
        .collect();
    Arc::new(AudioClip::mono(RATE, samples))
}

fn dc(value: f32, seconds: f32) -> Arc<AudioClip> {
    Arc::new(AudioClip::mono(RATE, vec![value; (RATE as f32 * seconds) as usize]))
}

fn render(mixer: &mut Mixer, seconds: f32) -> Vec<f32> {
    let mut out = vec![0.0; (RATE as f32 * seconds) as usize * 2];
    mixer.render(&mut out);
    out
}

fn left(buf: &[f32]) -> Vec<f32> {
    buf.chunks(2).map(|f| f[0]).collect()
}

fn right(buf: &[f32]) -> Vec<f32> {
    buf.chunks(2).map(|f| f[1]).collect()
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn crossings(buf: &[f32]) -> usize {
    buf.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
}

#[test]
fn silence_when_nothing_plays() {
    let mut mixer = Mixer::new(RATE);
    let out = render(&mut mixer, 0.1);
    assert!(out.iter().all(|&s| s == 0.0));
}

#[test]
fn sine_has_the_right_frequency_and_amplitude() {
    let mut mixer = Mixer::new(RATE);
    mixer.play(sine(440.0, 0.5, 2.0), VoiceParams::default());
    let out = render(&mut mixer, 1.0);
    let l = left(&out);
    assert!((crossings(&l) as i32 - 440).abs() <= 1, "{}", crossings(&l));
    assert!((peak(&l) - 0.5).abs() < 0.01, "{}", peak(&l));
    // Моно-клип по центру: оба канала одинаковы.
    assert_eq!(l, right(&out));
}

#[test]
fn mixer_converts_between_sample_rates() {
    // Клип 22050 Гц играет на выходе 44100 Гц без смены высоты тона.
    let n = 22_050;
    let samples = (0..n).map(|i| (std::f32::consts::TAU * 200.0 * i as f32 / 22_050.0).sin()).collect();
    let clip = Arc::new(AudioClip::mono(22_050, samples));
    let mut mixer = Mixer::new(RATE);
    mixer.play(clip, VoiceParams::default());
    let out = render(&mut mixer, 0.5);
    assert!((crossings(&left(&out)) as i32 - 100).abs() <= 1);
}

#[test]
fn pitch_two_doubles_the_frequency() {
    let clip = sine(220.0, 0.5, 2.0);
    let count = |pitch: f32| {
        let mut mixer = Mixer::new(RATE);
        mixer.play(Arc::clone(&clip), VoiceParams { pitch, ..VoiceParams::default() });
        crossings(&left(&render(&mut mixer, 0.5)))
    };
    let (normal, double, half) = (count(1.0), count(2.0), count(0.5));
    assert!((normal as i32 - 110).abs() <= 1, "{normal}");
    assert!((double as i32 - 220).abs() <= 2, "{double}");
    assert!((half as i32 - 55).abs() <= 1, "{half}");
}

#[test]
fn pan_moves_sound_between_channels() {
    let play = |pan: f32| {
        let mut mixer = Mixer::new(RATE);
        mixer.play(sine(300.0, 0.5, 1.0), VoiceParams { pan, ..VoiceParams::default() });
        render(&mut mixer, 0.2)
    };
    let center = play(0.0);
    assert!((peak(&left(&center)) - 0.5).abs() < 0.01);
    assert!((peak(&right(&center)) - 0.5).abs() < 0.01);

    let hard_left = play(-1.0);
    assert!(peak(&left(&hard_left)) > 0.49);
    assert!(peak(&right(&hard_left)) < 0.01);

    let hard_right = play(1.0);
    assert!(peak(&right(&hard_right)) > 0.49);
    assert!(peak(&left(&hard_right)) < 0.01);

    let slight = play(0.3);
    assert!(peak(&right(&slight)) > peak(&left(&slight)));
    assert!(peak(&left(&slight)) > 0.2);
}

#[test]
fn stereo_clips_keep_their_channels() {
    let n = 4410;
    let mut samples = Vec::new();
    for i in 0..n {
        let s = (std::f32::consts::TAU * 100.0 * i as f32 / RATE as f32).sin();
        samples.push(s * 0.8); // L
        samples.push(0.0); // R
    }
    let mut mixer = Mixer::new(RATE);
    mixer.play(Arc::new(AudioClip::from_samples(RATE, 2, samples)), VoiceParams::default());
    let out = render(&mut mixer, 0.05);
    assert!(peak(&left(&out)) > 0.7);
    assert_eq!(peak(&right(&out)), 0.0);
}

#[test]
fn distance_attenuates_and_pan_follows_the_side() {
    let listener = ListenerState { position: Vec3::ZERO, right: Vec3::X };
    let gain_at = |x: f32| spatialize(&listener, Vec3::new(x, 0.0, 0.0), 1.0, 11.0).0;
    assert_eq!(gain_at(0.5), 1.0);
    assert_eq!(gain_at(1.0), 1.0);
    assert!(gain_at(3.0) < gain_at(2.0));
    assert!(gain_at(6.0) > 0.0 && gain_at(6.0) < 0.5);
    assert_eq!(gain_at(11.0), 0.0);
    assert_eq!(gain_at(50.0), 0.0);

    let pan_of = |x: f32, y: f32| spatialize(&listener, Vec3::new(x, y, 0.0), 1.0, 50.0).1;
    assert!(pan_of(10.0, 0.0) > 0.9);
    assert!(pan_of(-10.0, 0.0) < -0.9);
    assert!(pan_of(0.0, 10.0).abs() < 0.01);
    // Слушатель, повёрнутый на 90°, меняет понятие «справа».
    let turned = ListenerState { position: Vec3::ZERO, right: Vec3::Y };
    assert!(spatialize(&turned, Vec3::new(0.0, 10.0, 0.0), 1.0, 50.0).1 > 0.9);

    // И в микшере: далёкий источник тише и смещён вправо.
    let mut mixer = Mixer::new(RATE);
    let near = mixer.play(sine(300.0, 0.8, 1.0), VoiceParams::default());
    let (g, p) = spatialize(&listener, Vec3::new(6.0, 0.0, 0.0), 1.0, 11.0);
    mixer.set_spatial(near, g, p);
    let out = render(&mut mixer, 0.2);
    assert!(peak(&left(&out)) < peak(&right(&out)));
    assert!(peak(&right(&out)) < 0.4);
}

#[test]
fn non_looping_voice_ends_and_looping_voice_continues() {
    let mut mixer = Mixer::new(RATE);
    let once = mixer.play(sine(440.0, 0.5, 0.1), VoiceParams::default());
    let looped = mixer.play(sine(440.0, 0.5, 0.1), VoiceParams { looping: true, ..VoiceParams::default() });
    assert_eq!(mixer.voice_count(), 2);
    let out = render(&mut mixer, 0.35);
    assert!(!mixer.is_playing(once));
    assert!(mixer.is_playing(looped));
    assert_eq!(mixer.voice_count(), 1);
    // Закольцованный клип звучит до самого конца буфера.
    assert!(peak(&left(&out)[out.len() / 2 - 400..]) > 0.4);
    // Одиночный клип после конца даёт ровно тишину: сигнал одного голоса — вдвое слабее.
    let tail_only_loop = render(&mut mixer, 0.1);
    assert!((peak(&tail_only_loop) - 0.5).abs() < 0.02);
}

#[test]
fn stop_silences_quickly_and_invalidates_the_handle() {
    let mut mixer = Mixer::new(RATE);
    let id = mixer.play(sine(440.0, 0.5, 5.0), VoiceParams { looping: true, ..VoiceParams::default() });
    render(&mut mixer, 0.05);
    mixer.stop(id);
    assert!(!mixer.is_playing(id));
    let out = render(&mut mixer, 0.05);
    // Затухание занимает считанные миллисекунды, дальше — тишина.
    assert!(peak(&out[out.len() / 2..]) == 0.0);
    assert_eq!(mixer.voice_count(), 0);
    // Операции с протухшим голосом безвредны.
    mixer.set_volume(id, 1.0);
    mixer.stop(id);
}

#[test]
fn fade_in_is_monotonic_and_reaches_full_volume() {
    let mut mixer = Mixer::new(RATE);
    mixer.play(dc(0.8, 1.0), VoiceParams { fade_in: 0.2, ..VoiceParams::default() });
    let l = left(&render(&mut mixer, 0.4));
    assert!(l[0].abs() < 0.001);
    assert!(l.windows(2).all(|w| w[1] >= w[0] - 1e-6), "fade-in is not monotonic");
    assert!((l[l.len() - 1] - 0.8).abs() < 1e-3);
    // Посередине фейда — примерно половина.
    let mid = l[(0.1 * RATE as f32) as usize];
    assert!((mid - 0.4).abs() < 0.03, "{mid}");
}

#[test]
fn fade_out_is_monotonic_and_stops_the_voice() {
    let mut mixer = Mixer::new(RATE);
    let id = mixer.play(dc(0.8, 2.0), VoiceParams::default());
    render(&mut mixer, 0.05);
    mixer.fade_out(id, 0.2);
    assert!(mixer.is_playing(id), "a voice that is fading out is still playing");
    let l = left(&render(&mut mixer, 0.3));
    let fading = &l[..(0.2 * RATE as f32) as usize];
    assert!(fading.windows(2).all(|w| w[1] <= w[0] + 1e-6), "fade-out is not monotonic");
    assert!(fading[0] > 0.75);
    assert!(l[l.len() - 1] == 0.0);
    assert!(!mixer.is_playing(id));
    assert_eq!(mixer.voice_count(), 0);
}

#[test]
fn bus_volume_mute_and_master() {
    let render_with = |setup: &dyn Fn(&mut Mixer)| {
        let mut mixer = Mixer::new(RATE);
        mixer.play(dc(0.8, 1.0), VoiceParams { bus: Bus::Sfx, ..VoiceParams::default() });
        mixer.play(dc(0.2, 1.0), VoiceParams { bus: Bus::Music, ..VoiceParams::default() });
        setup(&mut mixer);
        // Дадим громкостям установиться (они меняются плавно).
        render(&mut mixer, 0.05);
        let out = render(&mut mixer, 0.05);
        left(&out)[left(&out).len() - 1]
    };
    assert!((render_with(&|_| {}) - 1.0).abs() < 1e-3, "0.8 + 0.2 sum");
    assert!((render_with(&|m| m.set_bus_volume(Bus::Sfx, 0.5)) - 0.6).abs() < 1e-3);
    assert!((render_with(&|m| m.set_bus_muted(Bus::Sfx, true)) - 0.2).abs() < 1e-3);
    assert!((render_with(&|m| m.set_bus_volume(Bus::Master, 0.5)) - 0.5).abs() < 1e-3);
    assert!(render_with(&|m| m.set_bus_muted(Bus::Master, true)).abs() < 1e-6);
    assert!((render_with(&|m| {
        m.set_bus_volume(Bus::Music, 0.0);
        m.set_bus_volume(Bus::Master, 0.5);
    }) - 0.4)
        .abs()
        < 1e-3);
}

#[test]
fn volume_changes_are_smooth() {
    let mut mixer = Mixer::new(RATE);
    let id = mixer.play(dc(1.0, 1.0), VoiceParams::default());
    render(&mut mixer, 0.01);
    mixer.set_volume(id, 0.0);
    let l = left(&render(&mut mixer, 0.01));
    // Скачка нет: соседние сэмплы отличаются на малую величину.
    assert!(l.windows(2).all(|w| (w[0] - w[1]).abs() < 0.05));
    assert!(l[l.len() - 1] < 0.01);
}

#[test]
fn pause_freezes_and_resume_continues() {
    let mut mixer = Mixer::new(RATE);
    let id = mixer.play(sine(440.0, 0.5, 1.0), VoiceParams::default());
    render(&mut mixer, 0.1);
    let before = mixer.position(id).unwrap();
    mixer.set_paused(true);
    assert!(render(&mut mixer, 0.1).iter().all(|&s| s == 0.0));
    assert_eq!(mixer.position(id).unwrap(), before);
    mixer.set_paused(false);
    assert!(peak(&render(&mut mixer, 0.1)) > 0.4);
    assert!(mixer.position(id).unwrap() > before);
}

#[test]
fn start_offset_skips_the_beginning() {
    let mut samples = vec![0.0; 4410];
    samples.extend(vec![0.7; 4410]);
    let clip = Arc::new(AudioClip::mono(RATE, samples));
    let mut mixer = Mixer::new(RATE);
    mixer.play(clip, VoiceParams { start_offset: 0.1, ..VoiceParams::default() });
    let l = left(&render(&mut mixer, 0.01));
    assert!((l[10] - 0.7).abs() < 1e-3);
}

#[test]
fn too_many_voices_steal_the_quietest() {
    let mut mixer = Mixer::with_voices(RATE, 4);
    let clip = dc(0.5, 1.0);
    let quiet = mixer.play(Arc::clone(&clip), VoiceParams { volume: 0.1, ..VoiceParams::default() });
    for _ in 0..3 {
        mixer.play(Arc::clone(&clip), VoiceParams::default());
    }
    assert_eq!(mixer.voice_count(), 4);
    let newcomer = mixer.play(Arc::clone(&clip), VoiceParams::default());
    assert_eq!(mixer.voice_count(), 4);
    assert!(!mixer.is_playing(quiet));
    assert!(mixer.is_playing(newcomer));
}

#[test]
fn output_never_exceeds_full_scale() {
    let mut mixer = Mixer::new(RATE);
    for _ in 0..10 {
        mixer.play(dc(0.9, 0.5), VoiceParams::default());
    }
    assert!(peak(&render(&mut mixer, 0.1)) <= 1.0);
}

#[test]
fn advance_runs_voices_to_completion_without_a_buffer() {
    let mut mixer = Mixer::new(RATE);
    let id = mixer.play(sine(440.0, 0.5, 0.25), VoiceParams::default());
    mixer.advance(0.1);
    assert!(mixer.is_playing(id));
    mixer.advance(0.2);
    assert!(!mixer.is_playing(id));
}

#[test]
fn rendering_in_odd_block_sizes_matches_one_big_block() {
    let clip = sine(500.0, 0.5, 1.0);
    let mut a = Mixer::new(RATE);
    a.play(Arc::clone(&clip), VoiceParams::default());
    let whole = render(&mut a, 0.1);

    let mut b = Mixer::new(RATE);
    b.play(clip, VoiceParams::default());
    let mut pieces = Vec::new();
    while pieces.len() < whole.len() {
        let n = 2 * 97.min((whole.len() - pieces.len()) / 2);
        let mut buf = vec![0.0; n];
        b.render(&mut buf);
        pieces.extend(buf);
    }
    for (x, y) in whole.iter().zip(&pieces) {
        assert!((x - y).abs() < 1e-4);
    }
}
