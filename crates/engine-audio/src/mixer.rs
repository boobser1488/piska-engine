//! Программный микшер: голоса → шины → мастер (запас + лимитер) → стерео `f32`.
//!
//! Микшер ничего не знает о звуковой карте: он заполняет любой буфер
//! (`render`), поэтому его можно проверять в тестах по готовым сэмплам. Звуковая
//! карта (`backend`) просто вызывает `render` из своего потока.
//!
//! Поток звука не должен выделять память: голоса лежат в заранее выделенном
//! `Vec`, отыгравшие клипы уходят в `retired` (их освобождает игровой поток).
//!
//! Качество звука:
//!
//! * пересчёт частоты — кубическая интерполяция (4 точки); если голос играет быстрее
//!   исходной частоты (высота > 1 или клип 48 кГц на выходе 44.1 кГц), источник
//!   сначала проходит через ФНЧ 4-го порядка, чтобы верхние частоты не отражались вниз;
//! * громкость, панорама и шины меняются плавно (линейно в пределах блока);
//! * старт, остановка и вытеснение голоса — с короткими затуханиями, без щелчков;
//! * на мастер-шине запас −6 дБ ([`Mixer::DEFAULT_HEADROOM_DB`]) и лимитер с
//!   заглядыванием вперёд (потолок −1 дБFS): выход не клиппирует, что бы ни делала игра;
//! * одновременных копий одного клипа — не больше [`Mixer::DEFAULT_MAX_INSTANCES`]:
//!   сотня одинаковых взрывов за секунду не превращается в стену шума.

use std::sync::Arc;

use engine_core::reflect_enum;

use crate::clip::AudioClip;
use crate::dsp::{cubic, equal_power, Limiter, Svf, SvfCoefs, LIMITER_LOOKAHEAD};

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
    /// Секунд нарастания громкости с нуля (равномощная кривая).
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
/// Нарастание голоса, начатого не с начала клипа (`start_offset`): там сигнал не с нуля.
const OFFSET_FADE_SECONDS: f32 = 0.003;
/// Сколько кадров микшируется за один проход: на этом отрезке громкости меняются плавно.
const CHUNK: usize = 256;
/// Потолок лимитера: −1 дБFS.
const LIMITER_CEILING: f32 = 0.891;
/// Время возврата усиления лимитера.
const LIMITER_RELEASE: f32 = 0.12;

/// Добротности звеньев ФНЧ Баттерворта 4-го порядка.
const AA_Q: [f32; 2] = [0.541_196_1, 1.306_563];
/// Срез ФНЧ против наложения — доля новой частоты Найквиста источника.
const AA_CUTOFF: f32 = 0.42;
/// С какого ускорения чтения источника включается ФНЧ против наложения.
const AA_MIN_STEP: f64 = 1.1;

/// Состояние ФНЧ против наложения при ускоренном воспроизведении (4 звена на канал).
#[derive(Clone, Copy, Default)]
struct AntiAlias {
    stages: [[Svf; 2]; 2],
    /// Доля отфильтрованного сигнала (плавное включение / выключение).
    mix: f32,
}

struct Voice {
    id: VoiceId,
    clip: Arc<AudioClip>,
    /// Позиция в кадрах клипа без заворота петли (монотонно растёт).
    pos: f64,
    /// Номер следующего кадра клипа, который попадёт в историю интерполятора.
    fetched: i64,
    /// Четыре последних кадра источника (после ФНЧ): `fetched-4 … fetched-1`.
    hist: [[f32; 2]; 4],
    aa: AntiAlias,
    volume: f32,
    pitch: f32,
    pan: f32,
    looping: bool,
    bus: Bus,
    /// Множитель и сдвиг панорамы от 3D-позиционирования.
    spatial_gain: f32,
    spatial_pan: f32,
    /// Огибающая затухания/нарастания (линейный параметр, слышится через равномощную
    /// кривую) и её шаг на кадр.
    fade: f32,
    fade_target: f32,
    fade_step: f32,
    /// Остановить голос, когда огибающая дойдёт до цели.
    stop_at_target: bool,
    /// Остановка запрошена (`stop`, вытеснение): голос уже «не играет», но ещё затихает.
    stopping: bool,
    paused: bool,
    last_left: f32,
    last_right: f32,
    fresh: bool,
}

/// Равномощная панорама, нормированная так, чтобы в центре усиление было 1.0.
/// Сумма каналов в моно у центра — ровно исходный сигнал (без +3/+6 дБ).
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
    /// Мест под голоса вместе с затихающими (вытесненные доигрывают затухание).
    capacity: usize,
    max_instances: usize,
    bus_volume: [f32; 4],
    bus_muted: [bool; 4],
    /// Множитель от `AudioListener::volume`.
    listener_gain: f32,
    /// Заглушено, пока окно игры не в фокусе (`AudioSettings::mute_when_unfocused`).
    /// Отдельно от `bus_muted`: не трогает то, что заглушила сама игра.
    focus_muted: bool,
    /// Громкость «монитора» (редактор: кнопка звука в Game View) — поверх всех шин.
    monitor_volume: f32,
    monitor_muted: bool,
    /// Запас мастер-шины (линейный множитель) — целевое и текущее значения.
    headroom: f32,
    headroom_now: f32,
    limiter: Limiter,
    limiter_enabled: bool,
    paused: bool,
    next_id: u64,
    retired: Vec<Arc<AudioClip>>,
    scratch: Vec<f32>,
}

impl Mixer {
    /// Одновременно играющих голосов по умолчанию.
    pub const DEFAULT_VOICES: usize = 64;
    /// Запас мастер-шины по умолчанию, дБ: два эффекта на полной громкости
    /// складываются без перегрузки, лимитер работает только в крайних случаях.
    pub const DEFAULT_HEADROOM_DB: f32 = -6.0;
    /// Одновременных копий одного клипа по умолчанию.
    pub const DEFAULT_MAX_INSTANCES: usize = 6;

    pub fn new(sample_rate: u32) -> Self {
        Self::with_voices(sample_rate, Self::DEFAULT_VOICES)
    }

    pub fn with_voices(sample_rate: u32, max_voices: usize) -> Self {
        let max_voices = max_voices.max(1);
        let capacity = max_voices + max_voices / 4 + 8;
        let sample_rate = sample_rate.max(1);
        let headroom = db_to_gain(Self::DEFAULT_HEADROOM_DB);
        Self {
            sample_rate,
            voices: Vec::with_capacity(capacity),
            max_voices,
            capacity,
            max_instances: Self::DEFAULT_MAX_INSTANCES,
            bus_volume: [1.0; 4],
            bus_muted: [false; 4],
            listener_gain: 1.0,
            focus_muted: false,
            monitor_volume: 1.0,
            monitor_muted: false,
            headroom,
            headroom_now: headroom,
            limiter: Limiter::new(LIMITER_CEILING, LIMITER_RELEASE, sample_rate),
            limiter_enabled: true,
            paused: false,
            next_id: 1,
            retired: Vec::with_capacity(capacity),
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
        self.limiter.set_release(LIMITER_RELEASE, self.sample_rate);
    }

    // ------------------------------------------------------------------
    // Мастер-шина
    // ------------------------------------------------------------------

    /// Запас мастер-шины в дБ (по умолчанию −6). 0 — «как есть» (тесты, измерения).
    pub fn set_headroom_db(&mut self, db: f32) {
        self.headroom = db_to_gain(db.min(24.0));
    }

    pub fn headroom_db(&self) -> f32 {
        20.0 * self.headroom.max(1e-9).log10()
    }

    /// Включить / выключить лимитер мастер-шины (по умолчанию включён). Без него выход
    /// просто ограничивается ±1.0 (жёстко, с искажениями) — только для измерений.
    pub fn set_limiter_enabled(&mut self, enabled: bool) {
        self.limiter_enabled = enabled;
    }

    pub fn is_limiter_enabled(&self) -> bool {
        self.limiter_enabled
    }

    /// Текущее усиление лимитера (1.0 — не работает, 0.5 — давит на 6 дБ).
    pub fn limiter_gain(&self) -> f32 {
        if self.limiter_enabled {
            self.limiter.gain()
        } else {
            1.0
        }
    }

    /// Задержка выхода из-за заглядывания лимитера, кадров.
    pub fn latency_frames(&self) -> usize {
        if self.limiter_enabled {
            LIMITER_LOOKAHEAD - 1
        } else {
            0
        }
    }

    /// Громкость «монитора» поверх всего (0..1): её крутит редактор (кнопка звука в
    /// Game View, Preferences), а не игра — игра не может её перебить своими шинами.
    pub fn set_monitor_volume(&mut self, volume: f32) {
        self.monitor_volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
    }

    pub fn monitor_volume(&self) -> f32 {
        self.monitor_volume
    }

    pub fn set_monitor_muted(&mut self, muted: bool) {
        self.monitor_muted = muted;
    }

    pub fn is_monitor_muted(&self) -> bool {
        self.monitor_muted
    }

    /// Сколько копий одного клипа может звучать одновременно (≥ 1). Лишняя копия
    /// вытесняет самую старую копию этого же клипа (с коротким затуханием).
    pub fn set_max_instances(&mut self, count: usize) {
        self.max_instances = count.max(1);
    }

    pub fn max_instances(&self) -> usize {
        self.max_instances
    }

    // ------------------------------------------------------------------
    // Голоса
    // ------------------------------------------------------------------

    /// Запустить голос. Если все места заняты, вытесняется самый тихий из самых старых
    /// (он затухает за несколько миллисекунд, а не обрывается).
    pub fn play(&mut self, clip: Arc<AudioClip>, params: VoiceParams) -> VoiceId {
        if clip.is_empty() {
            return VoiceId::NONE;
        }
        // Лимит копий одного клипа: вытеснить самую старую.
        let copies = self
            .voices
            .iter()
            .filter(|v| !v.stopping && Arc::ptr_eq(&v.clip, &clip))
            .count();
        if copies >= self.max_instances {
            let rate = self.sample_rate as f32;
            if let Some(v) = self.voices.iter_mut().find(|v| !v.stopping && Arc::ptr_eq(&v.clip, &clip)) {
                v.begin_fade(0.0, DECLICK_SECONDS, rate);
                v.stopping = true;
            }
        }
        if self.voice_count() >= self.max_voices {
            self.steal_voice();
        }
        if self.voices.len() >= self.capacity {
            self.drop_quietest_stopping();
        }
        let id = VoiceId(self.next_id);
        self.next_id += 1;
        let frames = clip.frames() as f64;
        let offset = (params.start_offset.max(0.0) as f64 * clip.sample_rate() as f64).min(frames - 1.0);
        let rate = self.sample_rate as f32;
        let (fade, fade_step) = if params.fade_in > 0.0 {
            (0.0, 1.0 / (params.fade_in * rate))
        } else if offset > 0.0 {
            (0.0, 1.0 / (OFFSET_FADE_SECONDS * rate))
        } else {
            (1.0, 0.0)
        };
        self.voices.push(Voice {
            id,
            clip,
            pos: offset,
            fetched: offset.floor() as i64 - 1,
            hist: [[0.0; 2]; 4],
            aa: AntiAlias::default(),
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
            .filter(|(_, v)| !v.stopping)
            .min_by(|(_, a), (_, b)| {
                let loud = |v: &Voice| v.volume * v.spatial_gain;
                loud(a).partial_cmp(&loud(b)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);
        if let Some(i) = victim {
            let rate = self.sample_rate as f32;
            let v = &mut self.voices[i];
            v.begin_fade(0.0, DECLICK_SECONDS, rate);
            v.stopping = true;
        }
    }

    /// Мест не осталось даже для затихающих: убрать самый тихий из них (он почти
    /// беззвучен — щелчок пренебрежимо мал). Бывает при десятках запусков за 4 мс.
    fn drop_quietest_stopping(&mut self) {
        let victim = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.stopping)
            .min_by(|(_, a), (_, b)| a.fade.partial_cmp(&b.fade).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let voice = self.voices.remove(victim);
        self.retire(voice.clip);
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

    /// Плавно заглушить голос за `seconds` (равномощная кривая), затем остановить.
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
            if v.looping && !looping {
                // Петлю выключили: позиция отсчитывается от начала текущего круга,
                // иначе голос сразу оказался бы «за концом».
                let total = v.clip.frames() as f64;
                let turns = (v.pos / total).floor();
                v.pos -= turns * total;
                v.fetched -= (turns * total) as i64;
            }
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
        self.voices.iter().find(|v| v.id == id && !v.stopping).map(|v| {
            let total = v.clip.frames() as f64;
            let pos = if v.looping { v.pos.rem_euclid(total) } else { v.pos };
            (pos / v.clip.sample_rate() as f64) as f32
        })
    }

    /// Сколько голосов звучит (без тех, что уже останавливаются).
    pub fn voice_count(&self) -> usize {
        self.voices.iter().filter(|v| !v.stopping).count()
    }

    // ------------------------------------------------------------------
    // Шины и общая пауза
    // ------------------------------------------------------------------

    pub fn set_bus_volume(&mut self, bus: Bus, volume: f32) {
        self.bus_volume[bus.index()] = if volume.is_finite() { volume.max(0.0) } else { 1.0 };
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
            if self.bus_muted[b.index()] || self.focus_muted || self.monitor_muted {
                0.0
            } else {
                self.bus_volume[b.index()]
            }
        };
        let master = one(Bus::Master) * self.listener_gain * self.monitor_volume;
        match bus {
            Bus::Master => master,
            other => one(other) * master,
        }
    }

    // ------------------------------------------------------------------
    // Рендер
    // ------------------------------------------------------------------

    /// Заполнить `out` (чередующееся стерео `L R L R …`) следующими сэмплами.
    /// Содержимое `out` перезаписывается. Выделений памяти нет. Выход проходит через
    /// запас и лимитер мастер-шины и никогда не выходит за ±1.0.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if self.paused {
            return;
        }
        let total_frames = out.len() / 2;
        let mut done = 0;
        while done < total_frames {
            let n = (total_frames - done).min(CHUNK);
            let chunk = &mut out[done * 2..(done + n) * 2];
            self.render_chunk(chunk);
            self.master(chunk);
            done += n;
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
            // Через мастер — чтобы задержка лимитера не хранила старый звук.
            self.master(&mut scratch[..n * 2]);
            frames -= n;
        }
        self.scratch = scratch;
    }

    /// Мастер-шина: запас (плавно) → лимитер → страховка ±1.
    fn master(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        if frames == 0 {
            return;
        }
        let from = self.headroom_now;
        let step = (self.headroom - from) / frames as f32;
        for (i, frame) in out.chunks_exact_mut(2).enumerate() {
            let g = from + step * (i + 1) as f32;
            let (l, r) = (frame[0] * g, frame[1] * g);
            let (l, r) = if self.limiter_enabled { self.limiter.process(l, r) } else { (l, r) };
            frame[0] = if l.is_finite() { l.clamp(-1.0, 1.0) } else { 0.0 };
            frame[1] = if r.is_finite() { r.clamp(-1.0, 1.0) } else { 0.0 };
        }
        self.headroom_now = self.headroom;
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

fn db_to_gain(db: f32) -> f32 {
    if db.is_finite() {
        10f32.powf(db / 20.0)
    } else {
        1.0
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

    /// Кадр клипа с номером `index` (за пределами — тишина или заворот петли).
    #[inline]
    fn source(&self, index: i64, total: i64) -> [f32; 2] {
        let i = if index < 0 {
            return [0.0; 2];
        } else if index >= total {
            if !self.looping {
                return [0.0; 2];
            }
            index % total
        } else {
            index
        };
        let (l, r) = self.clip.frame(i as usize);
        [l, r]
    }

    /// Домешать до `frames` кадров в `out`. `true` — голос закончился.
    fn mix_into(&mut self, out: &mut [f32], frames: usize, mixer_rate: f64, bus_gain: f32) -> bool {
        if self.paused {
            return false;
        }
        let (pl, pr) = pan_gains(self.pan + self.spatial_pan);
        let gain = self.volume * self.spatial_gain * bus_gain;
        let (target_l, target_r) = (gain * pl, gain * pr);
        let first_block = self.fresh;
        if self.fresh {
            self.last_left = target_l;
            self.last_right = target_r;
            self.fresh = false;
        }
        let step_l = (target_l - self.last_left) / frames as f32;
        let step_r = (target_r - self.last_right) / frames as f32;
        let (mut gl, mut gr) = (self.last_left, self.last_right);

        let total = self.clip.frames() as i64;
        let step = self.pitch as f64 * self.clip.sample_rate() as f64 / mixer_rate;
        let stereo = self.clip.channels() > 1;

        // ФНЧ против наложения: нужен, когда источник читается быстрее одного кадра
        // за кадр выхода. Срез — 0.42 от новой частоты Найквиста. До ускорения в 1.1 раза
        // фильтр не включается: отражаться могут лишь частоты из верхних 10 % полосы
        // (выше ~20 кГц), а цена фильтра на каждом голосе ощутима (телефоны, wasm).
        let want_aa = step > AA_MIN_STEP;
        let aa_on = want_aa || self.aa.mix > 0.0;
        if want_aa && self.aa.mix == 0.0 {
            // Включение без переходного процесса: история — с текущего места клипа,
            // состояния фильтров «уже стоят» на последнем сэмпле.
            let base = self.pos as i64; // pos ≥ 0: отсечение = floor, без вызова libm
            for k in 0..4 {
                self.hist[k] = self.source(base - 1 + k as i64, total);
            }
            self.fetched = base + 3;
            let last = self.hist[3];
            for (c, stages) in self.aa.stages.iter_mut().enumerate() {
                *stages = [Svf::settled(last[c]); 2];
            }
            // Новый голос сразу с фильтром (плавное включение — только для уже звучащего).
            if first_block {
                self.aa.mix = 1.0;
            }
        }
        let coefs = if aa_on {
            let cutoff = AA_CUTOFF / step.max(1.0) as f32;
            AA_Q.map(|q| SvfCoefs::normalized(cutoff, q))
        } else {
            [SvfCoefs::normalized(AA_CUTOFF, 1.0); 2]
        };
        let aa_target = if want_aa { 1.0 } else { 0.0 };
        let clip = Arc::clone(&self.clip);
        let samples = clip.samples();
        let ch = clip.channels() as usize;

        let mut finished = false;
        for i in 0..frames {
            if self.pos >= total as f64 {
                if !self.looping {
                    finished = true;
                    break;
                }
                // Петля: позиция и счётчик истории сдвигаются на круг вместе.
                self.pos -= total as f64;
                self.fetched -= total;
            }
            let base = self.pos as i64; // pos ≥ 0: отсечение = floor, без вызова libm
            let t = (self.pos - base as f64) as f32;
            let (l, r) = if aa_on {
                // Через историю: каждый кадр источника проходит ФНЧ ровно один раз.
                while self.fetched <= base + 2 {
                    let mut frame = self.source(self.fetched, total);
                    let m = self.aa.mix;
                    let [s0, s1] = &mut self.aa.stages[0];
                    let y = s1.lowpass(&coefs[1], s0.lowpass(&coefs[0], frame[0]));
                    frame[0] += (y - frame[0]) * m;
                    if stereo {
                        let [s0, s1] = &mut self.aa.stages[1];
                        let y = s1.lowpass(&coefs[1], s0.lowpass(&coefs[0], frame[1]));
                        frame[1] += (y - frame[1]) * m;
                    } else {
                        frame[1] = frame[0];
                    }
                    // Плавно, за ~64 кадра источника.
                    if m != aa_target {
                        let next = if aa_target > m { m + 1.0 / 64.0 } else { m - 1.0 / 64.0 };
                        self.aa.mix = next.clamp(0.0, 1.0);
                        if (aa_target - self.aa.mix).abs() < 1.0 / 128.0 {
                            self.aa.mix = aa_target;
                        }
                    }
                    self.hist = [self.hist[1], self.hist[2], self.hist[3], frame];
                    self.fetched += 1;
                }
                let h = &self.hist;
                let l = cubic(h[0][0], h[1][0], h[2][0], h[3][0], t);
                (l, if stereo { cubic(h[0][1], h[1][1], h[2][1], h[3][1], t) } else { l })
            } else if base >= 1 && base + 2 < total {
                // Быстрый путь: четыре соседних кадра прямо из клипа.
                let i0 = (base - 1) as usize * ch;
                if stereo {
                    let p = &samples[i0..i0 + 3 * ch + 2];
                    (
                        cubic(p[0], p[ch], p[2 * ch], p[3 * ch], t),
                        cubic(p[1], p[ch + 1], p[2 * ch + 1], p[3 * ch + 1], t),
                    )
                } else {
                    let p = &samples[i0..i0 + 4];
                    let v = cubic(p[0], p[1], p[2], p[3], t);
                    (v, v)
                }
            } else {
                // Края клипа: тишина до начала и после конца, заворот у петли.
                let p = [
                    self.source(base - 1, total),
                    self.source(base, total),
                    self.source(base + 1, total),
                    self.source(base + 2, total),
                ];
                let l = cubic(p[0][0], p[1][0], p[2][0], p[3][0], t);
                (l, if stereo { cubic(p[0][1], p[1][1], p[2][1], p[3][1], t) } else { l })
            };

            gl += step_l;
            gr += step_r;
            let env = if self.fade >= 1.0 { 1.0 } else { equal_power(self.fade) };
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
