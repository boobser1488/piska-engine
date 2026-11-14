//! Аудиоклип — готовые сэмплы в памяти: из файла `.wav` / `.ogg` или из кода.

#[cfg(any(feature = "wav", feature = "ogg"))]
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

/// Ошибки загрузки клипа. Сообщения говорят, что делать.
#[derive(Debug, thiserror::Error)]
pub enum ClipError {
    #[error("cannot read `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("unsupported audio format{hint}. Convert the file to .wav (PCM or float) or .ogg (Vorbis)")]
    UnknownFormat { hint: String },
    #[error("cannot decode .wav data: {0}. Re-export the file as 16-bit PCM .wav")]
    Wav(String),
    #[error("cannot decode .ogg data: {0}. Re-export the file as Ogg Vorbis (not Opus)")]
    Ogg(String),
    #[error("the audio file contains no samples")]
    Empty,
    /// Формат известен, но декодер не вкомпилирован (возможности `wav` / `ogg` крейта).
    #[error("`.{format}` decoding is not compiled into this build. Enable the `{feature}` feature, or convert the file to a format that is")]
    Disabled { format: &'static str, feature: &'static str },
}

/// Клип: чередующиеся (interleaved) сэмплы `f32` в диапазоне -1..1.
///
/// Клип неизменяем и дёшево разделяется через `Arc` — один и тот же клип могут
/// играть сотни голосов.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioClip {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
}

impl AudioClip {
    /// Клип из готовых сэмплов. `samples` — чередующиеся каналы: `L R L R …` для стерео.
    /// Неполный хвост кадра отбрасывается.
    pub fn from_samples(sample_rate: u32, channels: u16, mut samples: Vec<f32>) -> Self {
        let channels = channels.max(1);
        let whole = samples.len() / channels as usize * channels as usize;
        samples.truncate(whole);
        Self {
            sample_rate: sample_rate.max(1),
            channels,
            samples,
        }
    }

    /// Моно-клип.
    pub fn mono(sample_rate: u32, samples: Vec<f32>) -> Self {
        Self::from_samples(sample_rate, 1, samples)
    }

    /// Разобрать `.wav` или `.ogg` из памяти (формат определяется по заголовку).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ClipError> {
        if bytes.starts_with(b"RIFF") {
            Self::decode_wav(bytes)
        } else if bytes.starts_with(b"OggS") {
            Self::decode_ogg(bytes)
        } else {
            let hint = if bytes.starts_with(b"ID3") || bytes.first() == Some(&0xff) {
                " (looks like .mp3)".to_string()
            } else if bytes.starts_with(b"fLaC") {
                " (looks like .flac)".to_string()
            } else {
                String::new()
            };
            Err(ClipError::UnknownFormat { hint })
        }
    }

    /// Загрузить файл `.wav` / `.ogg`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ClipError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| ClipError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_bytes(&bytes)
    }

    /// Без возможности `wav` WAV не читается: всегда [`ClipError::Disabled`].
    #[cfg(not(feature = "wav"))]
    pub fn decode_wav(_bytes: &[u8]) -> Result<Self, ClipError> {
        Err(ClipError::Disabled { format: "wav", feature: "audio-wav" })
    }

    /// Разобрать WAV: целочисленный PCM 8/16/24/32 бит и float32.
    #[cfg(feature = "wav")]
    pub fn decode_wav(bytes: &[u8]) -> Result<Self, ClipError> {
        let wav_error = |e: hound::Error| ClipError::Wav(e.to_string());
        let mut reader = hound::WavReader::new(Cursor::new(bytes)).map_err(wav_error)?;
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<Result<_, _>>()
                .map_err(wav_error)?,
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                if !(8..=32).contains(&bits) {
                    return Err(ClipError::Wav(format!("{bits}-bit samples are not supported")));
                }
                let scale = 1.0 / (1u64 << (bits - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<_, _>>()
                    .map_err(wav_error)?
            }
        };
        if samples.is_empty() {
            return Err(ClipError::Empty);
        }
        Ok(Self::from_samples(spec.sample_rate, spec.channels, samples))
    }

    /// Без возможности `ogg` Ogg Vorbis не читается: всегда [`ClipError::Disabled`].
    #[cfg(not(feature = "ogg"))]
    pub fn decode_ogg(_bytes: &[u8]) -> Result<Self, ClipError> {
        Err(ClipError::Disabled { format: "ogg", feature: "audio-ogg" })
    }

    /// Разобрать Ogg Vorbis.
    #[cfg(feature = "ogg")]
    pub fn decode_ogg(bytes: &[u8]) -> Result<Self, ClipError> {
        let ogg_error = |e: lewton::VorbisError| ClipError::Ogg(e.to_string());
        let mut reader =
            lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes)).map_err(ogg_error)?;
        let rate = reader.ident_hdr.audio_sample_rate;
        let channels = reader.ident_hdr.audio_channels as u16;
        let mut samples = Vec::new();
        while let Some(packet) = reader.read_dec_packet_itl().map_err(ogg_error)? {
            samples.extend(packet.iter().map(|&s| s as f32 / 32768.0));
        }
        if samples.is_empty() {
            return Err(ClipError::Empty);
        }
        Ok(Self::from_samples(rate, channels, samples))
    }

    /// Закодировать в 16-битный WAV (для сохранения сгенерированных клипов и отладки).
    /// Нужна возможность `wav`.
    #[cfg(feature = "wav")]
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("in-memory WAV header");
            for &s in &self.samples {
                writer
                    .write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
                    .expect("in-memory WAV write");
            }
            writer.finalize().expect("in-memory WAV finalize");
        }
        cursor.into_inner()
    }

    /// Сохранить как `.wav`. Нужна возможность `wav`.
    #[cfg(feature = "wav")]
    pub fn save_wav(&self, path: impl AsRef<Path>) -> Result<(), ClipError> {
        let path = path.as_ref();
        std::fs::write(path, self.to_wav_bytes()).map_err(|source| ClipError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Сэмплы, чередуются по каналам.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Число кадров (по одному сэмплу на канал).
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn duration(&self) -> f32 {
        self.frames() as f32 / self.sample_rate as f32
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Наибольшая по модулю амплитуда.
    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    /// Левый и правый сэмплы кадра `index` (моно — один и тот же в обоих).
    #[inline]
    pub fn frame(&self, index: usize) -> (f32, f32) {
        let ch = self.channels as usize;
        let base = index * ch;
        let left = self.samples[base];
        let right = if ch > 1 { self.samples[base + 1] } else { left };
        (left, right)
    }

    /// Положить в `Arc` — так клипы раздаются голосам и реестру.
    pub fn shared(self) -> Arc<AudioClip> {
        Arc::new(self)
    }

    /// Интегральная громкость, LUFS (BS.1770). Тишина — `-inf`.
    pub fn loudness(&self) -> f32 {
        crate::dsp::integrated_loudness(&self.samples, self.channels as usize, self.sample_rate)
    }

    /// Привести клип к уровню политики громкости: `clip.normalized_to(Level::Music)`
    /// или к громкости в LUFS: `clip.normalized_to(-14.0)`. Пик — не выше −1 дБFS.
    pub fn normalized_to(mut self, level: impl Into<Level>) -> Self {
        apply_level(&mut self.samples, self.channels as usize, self.sample_rate, level.into());
        self
    }
}

/// Уровень по политике громкости движка (см. «Политика громкости» в `docs/api/audio.md`).
///
/// Уровни задаются для клипа; на выходе микшер добавляет запас −6 дБ
/// ([`Mixer::DEFAULT_HEADROOM_DB`](crate::Mixer::DEFAULT_HEADROOM_DB)), поэтому при
/// громкостях шин 1.0 игрок слышит: музыку около −20 LUFS, пики эффектов около −8 дБFS,
/// пики интерфейса около −14 дБFS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    /// Фоновая музыка: [`Level::MUSIC_LUFS`].
    Music,
    /// Игровые эффекты: пик [`Level::SFX_PEAK`].
    Sfx,
    /// Звуки интерфейса — тише эффектов: пик [`Level::UI_PEAK`].
    Ui,
    /// Своя громкость, LUFS.
    Lufs(f32),
    /// Свой пик (линейный, 0..1).
    Peak(f32),
}

impl Level {
    /// Громкость музыки в клипе, LUFS.
    pub const MUSIC_LUFS: f32 = -14.0;
    /// Пик игрового эффекта в клипе (−2 дБFS).
    pub const SFX_PEAK: f32 = 0.8;
    /// Пик звука интерфейса в клипе (−8 дБFS).
    pub const UI_PEAK: f32 = 0.4;
    /// Выше этого пика нормализация по громкости не поднимает (−1 дБFS).
    pub const PEAK_CEILING: f32 = 0.891;

    /// Сколько дБ пиков может срезать лимитер при нормализации по громкости. Больше —
    /// звук слишком «пиковый» (одинокий удар): он останется тише цели, но не «сплющится».
    pub const MAX_LIMITING_DB: f32 = 9.0;
}

/// Привести сэмплы (чередующиеся каналы) к уровню. По громкости: усиление до цели;
/// если пик при этом вышел бы за [`Level::PEAK_CEILING`], пики мягко прижимает лимитер с
/// заглядыванием (по кругу — петли остаются бесшовными), но не больше чем на
/// [`Level::MAX_LIMITING_DB`].
pub(crate) fn apply_level(samples: &mut [f32], channels: usize, rate: u32, level: Level) {
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak <= 1e-9 {
        return;
    }
    let by_peak = |samples: &mut [f32], target: f32| {
        let gain = target.clamp(0.0, 1.0) / peak;
        for s in samples.iter_mut() {
            *s *= gain;
        }
    };
    let target = match level {
        Level::Sfx => return by_peak(samples, Level::SFX_PEAK),
        Level::Ui => return by_peak(samples, Level::UI_PEAK),
        Level::Peak(p) => return by_peak(samples, p),
        Level::Music => Level::MUSIC_LUFS,
        Level::Lufs(target) => target,
    };
    // Лимитер срезает пики и вместе с ними часть громкости (особенно у плотных ударных),
    // поэтому несколько итераций: усилить до цели → прижать пики → перемерить.
    // Общее усиление сверх «пикового» потолка — не больше MAX_LIMITING_DB.
    let max_gain = Level::PEAK_CEILING / peak * 10f32.powf(Level::MAX_LIMITING_DB / 20.0);
    let mut total = 1.0f32;
    for _ in 0..4 {
        let loudness = crate::dsp::integrated_loudness(samples, channels, rate);
        if !loudness.is_finite() {
            return;
        }
        let current = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let on_target = (loudness - target).abs() < 0.2;
        if on_target && current <= Level::PEAK_CEILING {
            return;
        }
        let gain = if on_target { 1.0 } else { 10f32.powf((target - loudness) / 20.0).min(max_gain / total) };
        if (gain - 1.0).abs() < 1e-3 && current <= Level::PEAK_CEILING {
            return;
        }
        total *= gain;
        let mut current_peak = 0.0f32;
        for s in samples.iter_mut() {
            *s *= gain;
            current_peak = current_peak.max(s.abs());
        }
        if current_peak > Level::PEAK_CEILING {
            crate::dsp::limit_loop(samples, channels, Level::PEAK_CEILING, rate);
        }
    }
    // Итерации кончились: пик всё равно не выше потолка.
    if samples.iter().any(|s| s.abs() > Level::PEAK_CEILING) {
        crate::dsp::limit_loop(samples, channels, Level::PEAK_CEILING, rate);
    }
}

/// Число — громкость в LUFS: `normalized_to(-16.0)`.
impl From<f32> for Level {
    fn from(lufs: f32) -> Self {
        Level::Lufs(lufs)
    }
}
