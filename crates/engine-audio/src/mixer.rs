//! Программный микшер: голоса → шины → стерео `f32`.
//!
//! Микшер ничего не знает о звуковой карте: он заполняет любой буфер
//! (`render`), поэтому его можно проверять в тестах по готовым сэмплам. Звуковая
//! карта (`backend`) просто вызывает `render` из своего потока.
//!
//! Поток звука не должен выделять память: голоса лежат в заранее выделенном
//! `Vec`, отыгравшие клипы уходят в `retired` (их освобождает игровой поток).

use std::sync::Arc;

use engine_core::reflect_enum;

use crate::clip::AudioClip;

reflect_enum! {
    /// Шина громкости. У `Master` громкость умножается на все остальные.
    #[derive(Hash)]
    pub enum Bus { Master, Music, Sfx, Ui }
}

impl Bus {
    pub const ALL: [Bus; 4] = [Bus::Master, Bus::Music, Bus::Sfx, Bus::Ui];

    fn index(self) -> usize {
        match self {
            Bus::Master => 0,
            Bus::Music => 1,
            Bus::Sfx => 2,
            Bus::Ui => 3,
        }
    }
}

/// Идентификатор голоса. Устаревший (голос уже доиграл) безвреден: все операции
/// с ним ничего не делают, `is_playing` даёт `false`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct VoiceId(pub u64);

impl VoiceId {
    /// «Нет голоса»: возвращается, когда клип не нашёлся.
    pub const NONE: VoiceId = VoiceId(0);

    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// Параметры нового голоса.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    pub volume: f32,
    /// Скорость воспроизведения: 2.0 — октавой выше и вдвое быстрее.
    pub pitch: f32,
    /// -1 (слева) … +1 (справа).
    pub pan: f32,
    pub looping: bool,
    pub bus: Bus,
    /// Секунд нарастания громкости с нуля.
    pub fade_in: f32,
    /// С какой секунды клипа начать.
    pub start_offset: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            looping: false,
            bus: Bus::Sfx,
            fade_in: 0.0,
            start_offset: 0.0,
        }
    }
}

/// Длительность «антищёлчковой» затухающей остановки голоса.
const DECLICK_SECONDS: f32 = 0.004;
/// Сколько кадров микшируется за один проход: на этом отрезке громкости меняются плавно.
const CHUNK: usize = 256;

struct Voice {
    id: VoiceId,
    clip: Arc<AudioClip>,
    /// Позиция воспроизведения в кадрах клипа.
    pos: f64,
    volume: f32,
    pitch: f32,
    pan: f32,
    looping: bool,
    bus: Bus,
    /// Множитель и сдвиг панорамы от 3D-позиционирования.
    spatial_gain: f32,
    spatial_pan: f32,
    /// Текущая огибающая затухания/нарастания и её шаг на кадр.
    fade: f32,
    fade_target: f32,
    fade_step: f32,
    /// Остановить голос, когда огибающая дойдёт до цели.
    stop_at_target: bool,
    /// Остановка запрошена (`stop`): голос уже «не играет», но ещё затихает.
    stopping: bool,
    paused: bool,
    last_left: f32,
    last_right: f32,
    fresh: bool,
}

/// Равномощная панорама, нормированная так, чтобы в центре усиление было 1.0.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (
        (angle.cos() * std::f32::consts::SQRT_2).min(1.0),
        (angle.sin() * std::f32::consts::SQRT_2).min(1.0),
    )
}

/// Микшер.
pub struct Mixer {
    sample_rate: u32,
    voices: Vec<Voice>,
    max_voices: usize,
    bus_volume: [f32; 4],
    bus_muted: [bool; 4],
    /// Множитель от `AudioListener::volume`.
    listener_gain: f32,
    /// Заглушено, пока окно игры не в фокусе (`AudioSettings::mute_when_unfocused`).
    /// Отдельно от `bus_muted`: не трогает то, что заглушила сама игра.
    focus_muted: bool,
    paused: bool,
    next_id: u64,
    retired: Vec<Arc<AudioClip>>,
    scratch: Vec<f32>,
}

impl Mixer {
    /// Одновременно играющих голосов по умолчанию.
    pub const DEFAULT_VOICES: usize = 64;

    pub fn new(sample_rate: u32) -> Self {
        Self::with_voices(sample_rate, Self::DEFAULT_VOICES)
    }

    pub fn with_voices(sample_rate: u32, max_voices: usize) -> Self {
        let max_voices = max_voices.max(1);
        Self {
            sample_rate: sample_rate.max(1),
            voices: Vec::with_capacity(max_voices),
            max_voices,
            bus_volume: [1.0; 4],
            bus_muted: [false; 4],
            listener_gain: 1.0,
            focus_muted: false,
            paused: false,
            next_id: 1,
            retired: Vec::with_capacity(max_voices),
            scratch: vec![0.0; CHUNK * 2],
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Сменить частоту выхода (когда открылась звуковая карта с другой частотой).
    /// Играющие голоса пересчитывают шаг сами.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
    }

    // ------------------------------------------------------------------
    // Голоса
    // ------------------------------------------------------------------

    /// Запустить голос. Если все места заняты, вытесняется самый тихий из самых старых.
    pub fn play(&mut self, clip: Arc<AudioClip>, params: VoiceParams) -> VoiceId {
        if clip.is_empty() {
            return VoiceId::NONE;
        }
        if self.voices.len() >= self.max_voices {
            self.steal_voice();
        }
        let id = VoiceId(self.next_id);
        self.next_id += 1;
        let frames = clip.frames() as f64;
        let offset = (params.start_offset.max(0.0) as f64 * clip.sample_rate() as f64).min(frames - 1.0);
        let (fade, fade_step) = if params.fade_in > 0.0 {
            (0.0, 1.0 / (params.fade_in * self.sample_rate as f32))
        } else {
            (1.0, 0.0)
        };
        self.voices.push(Voice {
            id,
            clip,
            pos: offset,
            volume: params.volume.max(0.0),
            pitch: params.pitch.max(0.0),
            pan: params.pan.clamp(-1.0, 1.0),
            looping: params.looping,
            bus: params.bus,
            spatial_gain: 1.0,
            spatial_pan: 0.0,
            fade,
            fade_target: 1.0,
            fade_step,
            stop_at_target: false,
            stopping: false,
            paused: false,
            last_left: 0.0,
            last_right: 0.0,
            fresh: true,
        });
        id
    }

    fn steal_voice(&mut self) {
        // Самый тихий; среди равных — самый старый (он раньше в списке).
        let victim = self
            .voices
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let loud = |v: &Voice| v.volume * v.spatial_gain * if v.stopping { 0.0 } else { 1.0 };
                loud(a).partial_cmp(&loud(b)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);
        if let Some(i) = victim {
            let voice = self.voices.remove(i);
            self.retire(voice.clip);
        }
    }

    fn retire(&mut self, clip: Arc<AudioClip>) {
        if self.retired.len() < self.retired.capacity() {
            self.retired.push(clip);
        }
        // Иначе клип освободится прямо здесь: редкий случай, зато без роста памяти.
    }

    /// Забрать отыгравшие клипы, чтобы освободить их вне потока звука.
    pub fn drain_retired(&mut self) {
        self.retired.clear();
    }

    fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        if id.is_none() {
            return None;
        }
        self.voices.iter_mut().find(|v| v.id == id)
    }

    /// Остановить голос (с коротким затуханием против щелчка).
    pub fn stop(&mut self, id: VoiceId) {
        let rate = self.sample_rate as f32;
        if let Some(v) = self.voice_mut(id) {
            v.begin_fade(0.0, DECLICK_SECONDS, rate);
            v.stopping = true;
        }
    }

    pub fn stop_all(&mut self) {
        let rate = self.sample_rate as f32;
        for v in &mut self.voices {
            v.begin_fade(0.0, DECLICK_SECONDS, rate);
            v.stopping = true;
        }
    }

    /// Остановить все голоса шины.
    pub fn stop_bus(&mut self, bus: Bus) {
        let rate = self.sample_rate as f32;
        for v in self.voices.iter_mut().filter(|v| v.bus == bus) {
            v.begin_fade(0.0, DECLICK_SECONDS, rate);
            v.stopping = true;
        }
    }

    /// Плавно заглушить голос за `seconds`, затем остановить.
    pub fn fade_out(&mut self, id: VoiceId, seconds: f32) {
        if seconds <= 0.0 {
            self.stop(id);
            return;
        }
        let rate = self.sample_rate as f32;
        if let Some(v) = self.voice_mut(id) {
            v.begin_fade(0.0, seconds, rate);
        }
    }

    /// Плавно вернуть голос к полной громкости за `seconds`.
    pub fn fade_in(&mut self, id: VoiceId, seconds: f32) {
        let rate = self.sample_rate as f32;
        if let Some(v) = self.voice_mut(id) {
            if !v.stopping {
                v.begin_fade(1.0, seconds, rate);
            }
        }
    }

    pub fn set_volume(&mut self, id: VoiceId, volume: f32) {
        if let Some(v) = self.voice_mut(id) {
            v.volume = volume.max(0.0);
        }
    }

    pub fn set_pitch(&mut self, id: VoiceId, pitch: f32) {
        if let Some(v) = self.voice_mut(id) {
            v.pitch = pitch.max(0.0);
        }
    }

    pub fn set_pan(&mut self, id: VoiceId, pan: f32) {
        if let Some(v) = self.voice_mut(id) {
            v.pan = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn set_looping(&mut self, id: VoiceId, looping: bool) {
        if let Some(v) = self.voice_mut(id) {
            v.looping = looping;
        }
    }

    pub fn set_voice_bus(&mut self, id: VoiceId, bus: Bus) {
        if let Some(v) = self.voice_mut(id) {
            v.bus = bus;
        }
    }

    /// Пауза одного голоса.
    pub fn set_voice_paused(&mut self, id: VoiceId, paused: bool) {
        if let Some(v) = self.voice_mut(id) {
            v.paused = paused;
        }
    }

    /// Результат 3D-позиционирования: множитель громкости и сдвиг панорамы.
    pub fn set_spatial(&mut self, id: VoiceId, gain: f32, pan: f32) {
        if let Some(v) = self.voice_mut(id) {
            v.spatial_gain = gain.clamp(0.0, 1.0);
            v.spatial_pan = pan.clamp(-1.0, 1.0);
        }
    }

    /// Голос ещё звучит (и его не просили остановить).
    pub fn is_playing(&self, id: VoiceId) -> bool {
        !id.is_none() && self.voices.iter().any(|v| v.id == id && !v.stopping)
    }

    /// Позиция воспроизведения в секундах клипа.
    pub fn position(&self, id: VoiceId) -> Option<f32> {
        self.voices
            .iter()
            .find(|v| v.id == id && !v.stopping)
            .map(|v| (v.pos / v.clip.sample_rate() as f64) as f32)
    }

    /// Сколько голосов звучит (без тех, что уже останавливаются).
    pub fn voice_count(&self) -> usize {
        self.voices.iter().filter(|v| !v.stopping).count()
    }

    // ------------------------------------------------------------------
    // Шины и общая пауза
    // ------------------------------------------------------------------

    pub fn set_bus_volume(&mut self, bus: Bus, volume: f32) {
        self.bus_volume[bus.index()] = volume.max(0.0);
    }

    pub fn bus_volume(&self, bus: Bus) -> f32 {
        self.bus_volume[bus.index()]
    }

    pub fn set_bus_muted(&mut self, bus: Bus, muted: bool) {
        self.bus_muted[bus.index()] = muted;
    }

    pub fn is_bus_muted(&self, bus: Bus) -> bool {
        self.bus_muted[bus.index()]
    }

    pub fn set_listener_gain(&mut self, gain: f32) {
        self.listener_gain = gain.max(0.0);
    }

    /// Заглушить весь звук на время потери фокуса окна (голоса идут дальше).
    pub fn set_focus_muted(&mut self, muted: bool) {
        self.focus_muted = muted;
    }

    pub fn is_focus_muted(&self) -> bool {
        self.focus_muted
    }

    /// Пауза: микшер выдаёт тишину, голоса не двигаются.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    fn bus_gain(&self, bus: Bus) -> f32 {
        let one = |b: Bus| {
            if self.bus_muted[b.index()] || self.focus_muted {
                0.0
            } else {
                self.bus_volume[b.index()]
            }
        };
        match bus {
            Bus::Master => one(Bus::Master) * self.listener_gain,
            other => one(other) * one(Bus::Master) * self.listener_gain,
        }
    }

    // ------------------------------------------------------------------
    // Рендер
    // ------------------------------------------------------------------

    /// Заполнить `out` (чередующееся стерео `L R L R …`) следующими сэмплами.
    /// Содержимое `out` перезаписывается. Выделений памяти нет.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if self.paused {
            return;
        }
        let total_frames = out.len() / 2;
        let mut done = 0;
        while done < total_frames {
            let n = (total_frames - done).min(CHUNK);
            self.render_chunk(&mut out[done * 2..(done + n) * 2]);
            done += n;
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }

    /// Продвинуть время на `seconds`, не получая звука: так «играет» микшер без звуковой
    /// карты, и игровая логика (`is_playing`, затухания) не замечает её отсутствия.
    pub fn advance(&mut self, seconds: f32) {
        if self.paused || seconds <= 0.0 {
            return;
        }
        let mut frames = (seconds * self.sample_rate as f32).round() as usize;
        let mut scratch = std::mem::take(&mut self.scratch);
        while frames > 0 {
            let n = frames.min(CHUNK);
            self.render_chunk(&mut scratch[..n * 2]);
            frames -= n;
        }
        self.scratch = scratch;
    }

    fn render_chunk(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let rate = self.sample_rate as f64;
        let mut index = 0;
        while index < self.voices.len() {
            let bus_gain = self.bus_gain(self.voices[index].bus);
            let finished = self.voices[index].mix_into(out, frames, rate, bus_gain);
            if finished {
                let voice = self.voices.remove(index);
                self.retire(voice.clip);
            } else {
                index += 1;
            }
        }
    }
}

impl Voice {
    fn begin_fade(&mut self, target: f32, seconds: f32, mixer_rate: f32) {
        if seconds <= 0.0 {
            self.fade = target;
            self.fade_step = 0.0;
            self.fade_target = target;
        } else {
            self.fade_target = target;
            self.fade_step = (target - self.fade) / (seconds * mixer_rate);
        }
        self.stop_at_target = target <= 0.0;
    }

    /// Домешать до `frames` кадров в `out`. `true` — голос закончился.
    fn mix_into(&mut self, out: &mut [f32], frames: usize, mixer_rate: f64, bus_gain: f32) -> bool {
        if self.paused {
            return false;
        }
        let (pl, pr) = pan_gains(self.pan + self.spatial_pan);
        let gain = self.volume * self.spatial_gain * bus_gain;
        let (target_l, target_r) = (gain * pl, gain * pr);
        if self.fresh {
            self.last_left = target_l;
            self.last_right = target_r;
            self.fresh = false;
        }
        let step_l = (target_l - self.last_left) / frames as f32;
        let step_r = (target_r - self.last_right) / frames as f32;
        let (mut gl, mut gr) = (self.last_left, self.last_right);

        let clip = Arc::clone(&self.clip);
        let total = clip.frames();
        let step = self.pitch as f64 * clip.sample_rate() as f64 / mixer_rate;
        let looping = self.looping;
        let mut finished = false;

        for i in 0..frames {
            let base = self.pos as usize;
            if base >= total {
                if looping {
                    self.pos -= total as f64;
                } else {
                    finished = true;
                    break;
                }
            }
            let base = self.pos as usize;
            let frac = (self.pos - base as f64) as f32;
            let (l0, r0) = clip.frame(base);
            let next = base + 1;
            let (l1, r1) = if next < total {
                clip.frame(next)
            } else if looping {
                clip.frame(0)
            } else {
                (l0, r0)
            };
            let l = l0 + (l1 - l0) * frac;
            let r = r0 + (r1 - r0) * frac;

            gl += step_l;
            gr += step_r;
            let env = self.fade;
            out[i * 2] += l * gl * env;
            out[i * 2 + 1] += r * gr * env;

            if self.fade_step != 0.0 {
                self.fade += self.fade_step;
                let reached = if self.fade_step > 0.0 {
                    self.fade >= self.fade_target
                } else {
                    self.fade <= self.fade_target
                };
                if reached {
                    self.fade = self.fade_target;
                    self.fade_step = 0.0;
                    if self.stop_at_target {
                        finished = true;
                        break;
                    }
                }
            }
            self.pos += step;
        }
        self.last_left = target_l;
        self.last_right = target_r;
        finished
    }
}
