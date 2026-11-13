//! Декодеры клипов: WAV (генерируется в тесте), OGG (маленькие файлы в `tests/fixtures`,
//! созданные ffmpeg из синусов 440 Гц).

use engine_audio::{AudioClip, ClipError};

fn zero_crossings(samples: &[f32]) -> usize {
    samples.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
}

fn wav_bytes(bits: u16, float: bool, channels: u16, rate: u32, frames: usize) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for i in 0..frames {
            let v = (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 0.5;
            for c in 0..channels {
                let v = if c == 0 { v } else { -v };
                if float {
                    w.write_sample(v).unwrap();
                } else if bits == 8 {
                    w.write_sample((v * 127.0) as i8).unwrap();
                } else if bits == 16 {
                    w.write_sample((v * 32767.0) as i16).unwrap();
                } else {
                    w.write_sample((v * (1u64 << (bits - 1)) as f32) as i32).unwrap();
                }
            }
        }
        w.finalize().unwrap();
    }
    cursor.into_inner()
}

#[test]
fn wav_pcm16_mono_roundtrip() {
    let clip = AudioClip::from_bytes(&wav_bytes(16, false, 1, 22_050, 22_050)).unwrap();
    assert_eq!(clip.sample_rate(), 22_050);
    assert_eq!(clip.channels(), 1);
    assert_eq!(clip.frames(), 22_050);
    assert!((clip.duration() - 1.0).abs() < 1e-3);
    assert!((clip.peak() - 0.5).abs() < 0.01);
    assert!((zero_crossings(clip.samples()) as i32 - 440).abs() <= 1);
}

#[test]
fn wav_other_bit_depths_and_float() {
    for (bits, float) in [(8, false), (24, false), (32, false), (32, true)] {
        let clip = AudioClip::from_bytes(&wav_bytes(bits, float, 1, 8000, 800)).unwrap();
        assert_eq!(clip.frames(), 800, "{bits}-bit float={float}");
        let tolerance = if bits == 8 { 0.02 } else { 0.005 };
        assert!((clip.peak() - 0.5).abs() < tolerance, "{bits}-bit float={float}: {}", clip.peak());
    }
}

#[test]
fn wav_stereo_keeps_channels_apart() {
    let clip = AudioClip::from_bytes(&wav_bytes(16, false, 2, 8000, 100)).unwrap();
    assert_eq!(clip.channels(), 2);
    assert_eq!(clip.frames(), 100);
    let (l, r) = clip.frame(10);
    assert!((l + r).abs() < 1e-3 && l.abs() > 0.1, "{l} {r}");
}

#[test]
fn encode_then_decode_returns_the_same_clip() {
    let original = AudioClip::from_samples(16_000, 2, (0..800).map(|i| (i as f32 * 0.01).sin() * 0.7).collect());
    let decoded = AudioClip::from_bytes(&original.to_wav_bytes()).unwrap();
    assert_eq!(decoded.sample_rate(), 16_000);
    assert_eq!(decoded.channels(), 2);
    assert_eq!(decoded.frames(), original.frames());
    for (a, b) in original.samples().iter().zip(decoded.samples()) {
        assert!((a - b).abs() < 1e-3);
    }
}

#[test]
fn ogg_mono_fixture_decodes_to_a_440_hz_tone() {
    let clip = AudioClip::from_bytes(include_bytes!("fixtures/sine440_mono.ogg")).unwrap();
    assert_eq!(clip.sample_rate(), 22_050);
    assert_eq!(clip.channels(), 1);
    assert!((clip.duration() - 0.5).abs() < 0.06, "{}", clip.duration());
    assert!(clip.peak() > 0.1 && clip.peak() <= 1.01, "{}", clip.peak());
    // Середина клипа (без краёв кодека): частота 440 Гц.
    let mid = &clip.samples()[2000..2000 + 11_025 / 2];
    let hz = zero_crossings(mid) as f32 / (mid.len() as f32 / 22_050.0);
    assert!((hz - 440.0).abs() < 15.0, "{hz}");
}

#[test]
fn ogg_stereo_fixture_decodes_two_channels() {
    let clip = AudioClip::from_bytes(include_bytes!("fixtures/sine440_stereo.ogg")).unwrap();
    assert_eq!(clip.sample_rate(), 44_100);
    assert_eq!(clip.channels(), 2);
    assert!((clip.duration() - 0.25).abs() < 0.06);
    let (l, r) = clip.frame(clip.frames() / 2);
    assert!((l - r).abs() < 0.1, "both channels carry the same sine: {l} {r}");
}

#[test]
fn load_reads_files_and_reports_missing_ones() {
    let dir = std::env::temp_dir().join(format!("engine-audio-decode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone.wav");
    let clip = AudioClip::mono(8000, vec![0.25; 400]);
    clip.save_wav(&path).unwrap();
    let loaded = AudioClip::load(&path).unwrap();
    assert_eq!(loaded.frames(), 400);
    std::fs::remove_dir_all(&dir).unwrap();

    let error = AudioClip::load(dir.join("nope.wav")).unwrap_err();
    assert!(matches!(error, ClipError::Io { .. }));
    assert!(error.to_string().contains("nope.wav"));
}

#[test]
fn garbage_and_unsupported_formats_are_errors_not_panics() {
    assert!(matches!(AudioClip::from_bytes(b"hello world"), Err(ClipError::UnknownFormat { .. })));
    let mp3 = AudioClip::from_bytes(b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap_err().to_string();
    assert!(mp3.contains("mp3") && mp3.contains("Convert"), "{mp3}");
    assert!(matches!(AudioClip::from_bytes(b"RIFFxxxxWAVEfmt "), Err(ClipError::Wav(_))));
    assert!(matches!(AudioClip::from_bytes(b"OggS\x00garbage"), Err(ClipError::Ogg(_))));
    let truncated = &include_bytes!("fixtures/sine440_mono.ogg")[..200];
    assert!(AudioClip::from_bytes(truncated).is_err() || AudioClip::from_bytes(truncated).unwrap().frames() < 11_025);
}

#[test]
fn from_samples_drops_a_partial_trailing_frame() {
    let clip = AudioClip::from_samples(44_100, 2, vec![0.1; 7]);
    assert_eq!(clip.frames(), 3);
    assert_eq!(clip.samples().len(), 6);
}
