//! Вывод на звуковую карту через `cpal`.
//!
//! Платформы: Windows (WASAPI), Linux (ALSA), Android (AAudio), macOS/iOS (CoreAudio),
//! браузер (Web Audio, `wasm32` — требует жеста пользователя, поэтому устройство
//! открывается лениво, при первом звуке). Без фичи `device` бэкенд недоступен, и
//! `open` честно возвращает ошибку.
//!
//! Поток cpal только вызывает `Mixer::render` под замком: ни выделений памяти,
//! ни ожидания чего-либо, кроме короткого замка игрового потока.

use std::sync::atomic::AtomicBool;
#[cfg(feature = "device")]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::mixer::Mixer;

/// Открытое устройство вывода. Пока объект жив, звук идёт; `Drop` закрывает поток.
pub(crate) struct Output {
    #[cfg(feature = "device")]
    _stream: cpal::Stream,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    lost: Arc<AtomicBool>,
}

impl Output {
    /// Устройство пропало (отключили наушники, сервис звука упал).
    #[cfg(feature = "device")]
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    #[cfg(not(feature = "device"))]
    pub fn is_lost(&self) -> bool {
        let _ = &self.lost;
        false
    }
}

#[cfg(not(feature = "device"))]
pub(crate) fn open(_mixer: Arc<Mutex<Mixer>>) -> Result<Output, String> {
    Err("this build has no audio backend (feature `device` is off)".to_string())
}

#[cfg(feature = "device")]
pub(crate) fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Output, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::SampleFormat;

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no audio output device found. Plug in speakers or headphones".to_string())?;
    let name = device.to_string();
    let supported = device
        .default_output_config()
        .map_err(|e| format!("cannot query the default output format of `{name}`: {e}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (sample_rate, channels) = (config.sample_rate, config.channels);
    if channels == 0 {
        return Err(format!("`{name}` reports zero output channels"));
    }

    // Микшер должен знать частоту выхода до первого сэмпла.
    mixer
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .set_sample_rate(sample_rate);

    let lost = Arc::new(AtomicBool::new(false));
    macro_rules! build {
        ($t:ty) => {
            build_stream::<$t>(&device, config, Arc::clone(&mixer), Arc::clone(&lost))
        };
    }
    let stream = match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::U16 => build!(u16),
        SampleFormat::I32 => build!(i32),
        SampleFormat::U32 => build!(u32),
        SampleFormat::F64 => build!(f64),
        SampleFormat::I8 => build!(i8),
        SampleFormat::U8 => build!(u8),
        other => return Err(format!("`{name}` uses the sample format {other}, which is not supported")),
    }
    .map_err(|e| format!("cannot open `{name}`: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("cannot start playback on `{name}`: {e}"))?;

    Ok(Output {
        _stream: stream,
        name,
        sample_rate,
        channels,
        lost,
    })
}

#[cfg(feature = "device")]
fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
    lost: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + cpal::FromSample<f32> + Send + 'static,
{
    use cpal::traits::DeviceTrait;

    let channels = config.channels as usize;
    // Буфер под стерео-микс; растёт только если устройство попросит больше, чем раньше.
    let mut scratch: Vec<f32> = vec![0.0; 4096 * 2];
    let silence = T::from_sample(0.0f32);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels;
            if scratch.len() < frames * 2 {
                scratch.resize(frames * 2, 0.0);
            }
            let mix = &mut scratch[..frames * 2];
            mixer
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .render(mix);
            for (frame, lr) in data.chunks_exact_mut(channels).zip(mix.chunks_exact(2)) {
                if channels == 1 {
                    frame[0] = T::from_sample((lr[0] + lr[1]) * 0.5);
                } else {
                    frame[0] = T::from_sample(lr[0]);
                    frame[1] = T::from_sample(lr[1]);
                    for extra in &mut frame[2..] {
                        *extra = silence;
                    }
                }
            }
        },
        move |error: cpal::Error| {
            use cpal::ErrorKind;
            if matches!(
                error.kind(),
                ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated | ErrorKind::BackendError
            ) {
                lost.store(true, Ordering::Relaxed);
            }
        },
        None,
    )
}
