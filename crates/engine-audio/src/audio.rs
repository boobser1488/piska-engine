//! Ресурс `Audio` — то, чем звук пользуется игровой код.
//!
//! Живёт в `world.resources`: `ctx.resource_mut::<Audio>()` в хуке компонента или
//! `app.world.resources.get_mut::<Audio>()` снаружи.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use engine_assets::{AssetDatabase, AssetHandle, Guid};
use engine_core::prelude::*;

use crate::clip::{AudioClip, ClipError};
use crate::mixer::{Bus, Mixer, VoiceId, VoiceParams};

/// Ссылка на клип в вызовах `Audio`: имя, сам клип или ассет.
#[derive(Clone)]
pub enum ClipRef {
    /// Клип, зарегистрированный через [`Audio::add_clip`].
    Name(String),
    Clip(Arc<AudioClip>),
    /// Файл из базы ассетов (декодируется при первом использовании).
    Asset(Guid),
}

impl From<&str> for ClipRef {
    fn from(name: &str) -> Self {
        ClipRef::Name(name.to_string())
    }
}
impl From<String> for ClipRef {
    fn from(name: String) -> Self {
        ClipRef::Name(name)
    }
}
impl From<&String> for ClipRef {
    fn from(name: &String) -> Self {
        ClipRef::Name(name.clone())
    }
}
impl From<Arc<AudioClip>> for ClipRef {
    fn from(clip: Arc<AudioClip>) -> Self {
        ClipRef::Clip(clip)
    }
}
impl From<&Arc<AudioClip>> for ClipRef {
    fn from(clip: &Arc<AudioClip>) -> Self {
        ClipRef::Clip(Arc::clone(clip))
    }
}
impl From<AudioClip> for ClipRef {
    fn from(clip: AudioClip) -> Self {
        ClipRef::Clip(Arc::new(clip))
    }
}
impl From<AssetHandle<engine_assets::AudioClip>> for ClipRef {
    fn from(handle: AssetHandle<engine_assets::AudioClip>) -> Self {
        ClipRef::Asset(handle.guid())
    }
}

/// Параметры запуска звука. Строятся цепочкой: `PlayParams::new().volume(0.5).pitch(1.2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayParams {
    pub voice: VoiceParams,
    /// Мировая позиция: звук становится пространственным (громкость и панорама
    /// считаются относительно `AudioListener`).
    pub position: Option<Vec3>,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl Default for PlayParams {
    fn default() -> Self {
        Self {
            voice: VoiceParams::default(),
            position: None,
            min_distance: 1.0,
            max_distance: 30.0,
        }
    }
}

impl PlayParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn volume(mut self, volume: f32) -> Self {
        self.voice.volume = volume;
        self
    }

    pub fn pitch(mut self, pitch: f32) -> Self {
        self.voice.pitch = pitch;
        self
    }

    /// -1 (слева) … +1 (справа).
    pub fn pan(mut self, pan: f32) -> Self {
        self.voice.pan = pan;
        self
    }

    pub fn looping(mut self, looping: bool) -> Self {
        self.voice.looping = looping;
        self
    }

    pub fn bus(mut self, bus: Bus) -> Self {
        self.voice.bus = bus;
        self
    }

    pub fn fade_in(mut self, seconds: f32) -> Self {
        self.voice.fade_in = seconds;
        self
    }

    pub fn start_at(mut self, seconds: f32) -> Self {
        self.voice.start_offset = seconds;
        self
    }

    /// Пространственный звук в точке `position`.
    pub fn at(mut self, position: Vec3) -> Self {
        self.position = Some(position);
        self
    }

    pub fn range(mut self, min_distance: f32, max_distance: f32) -> Self {
        self.min_distance = min_distance;
        self.max_distance = max_distance;
        self
    }
}

/// Состояние звукового устройства.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Устройство откроется при первом звуке.
    NotOpened,
    Active {
        name: String,
        sample_rate: u32,
        channels: u16,
    },
    /// Устройства нет или оно сломалось: игра идёт дальше без звука. Причина — в строке.
    Unavailable(String),
    /// Отключено (`ENGINE_AUDIO=off`, `set_device_enabled(false)` или скриншот-режим).
    Disabled,
}

/// Слушатель: откуда «слышим».
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListenerState {
    pub position: Vec3,
    /// Направление «вправо» в мире (для панорамы).
    pub right: Vec3,
}

/// Громкость и панорама по положению источника относительно слушателя.
///
/// Громкость 1 ближе `min_distance`, 0 дальше `max_distance`, между ними спадает
/// плавно (квадратично). Панорама — проекция направления на «правую» ось слушателя;
/// внутри `min_distance` она ослабляется, чтобы звук «у самого уха» не метался.
pub fn spatialize(
    listener: &ListenerState,
    source: Vec3,
    min_distance: f32,
    max_distance: f32,
) -> (f32, f32) {
    let min = min_distance.max(0.0);
    let max = max_distance.max(min + 1e-3);
    let offset = source - listener.position;
    let distance = offset.length();
    let gain = if distance <= min {
        1.0
    } else if distance >= max {
        0.0
    } else {
        let t = (distance - min) / (max - min);
        (1.0 - t) * (1.0 - t)
    };
    let pan = if distance > 1e-5 {
        (offset.dot(listener.right) / distance.max(min).max(1e-5)).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (gain, pan)
}

struct MusicState {
    voice: VoiceId,
    clip: Arc<AudioClip>,
}

struct SpatialVoice {
    voice: VoiceId,
    position: Vec3,
    min: f32,
    max: f32,
}

/// Что система знает про `AudioSource` объекта.
#[derive(Default)]
pub(crate) struct SourceState {
    pub voice: VoiceId,
    /// Источник уже запускался (автозапуск не повторяется).
    pub started: bool,
}

/// Звук игры. Лежит в `world.resources`.
pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    clips: HashMap<String, Arc<AudioClip>>,
    asset_clips: HashMap<Guid, Option<Arc<AudioClip>>>,
    assets: Option<AssetDatabase>,
    warned: HashSet<String>,
    music: Option<MusicState>,
    spatial_voices: Vec<SpatialVoice>,
    pub(crate) listener: Option<ListenerState>,
    pub(crate) sources: HashMap<GameObjectId, SourceState>,
    pub(crate) forced_play: HashSet<GameObjectId>,
    user_paused: bool,
    idle_paused: bool,
    pub(crate) device: DeviceStatus,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    /// Звук с микшером на 48 кГц; частота подстроится под устройство при его открытии.
    pub fn new() -> Self {
        Self::with_mixer(Mixer::new(48_000))
    }

    /// Звук с готовым микшером (тесты и нестандартные настройки).
    pub fn with_mixer(mixer: Mixer) -> Self {
        Self {
            mixer: Arc::new(Mutex::new(mixer)),
            clips: HashMap::new(),
            asset_clips: HashMap::new(),
            assets: None,
            warned: HashSet::new(),
            music: None,
            spatial_voices: Vec::new(),
            listener: None,
            sources: HashMap::new(),
            forced_play: HashSet::new(),
            user_paused: false,
            idle_paused: false,
            device: DeviceStatus::NotOpened,
        }
    }

    pub(crate) fn mixer_handle(&self) -> Arc<Mutex<Mixer>> {
        Arc::clone(&self.mixer)
    }

    fn lock(&self) -> MutexGuard<'_, Mixer> {
        self.mixer.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Прямой доступ к микшеру (тесты, нестандартные задачи). Не держите замок долго:
    /// пока он взят, поток звука ждёт.
    pub fn with_mixer_mut<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        f(&mut self.lock())
    }

    // ------------------------------------------------------------------
    // Клипы
    // ------------------------------------------------------------------

    /// Зарегистрировать клип под именем: `audio.add_clip("coin", sfx::coin())`.
    /// Повторная регистрация заменяет клип (играющие голоса доиграют старый).
    pub fn add_clip(&mut self, name: impl Into<String>, clip: impl Into<Arc<AudioClip>>) {
        self.clips.insert(name.into(), clip.into());
    }

    /// Загрузить `.wav` / `.ogg` с диска и зарегистрировать под именем.
    pub fn load_clip(&mut self, name: impl Into<String>, path: impl AsRef<std::path::Path>) -> Result<(), ClipError> {
        let clip = AudioClip::load(path)?;
        self.add_clip(name, clip);
        Ok(())
    }

    pub fn remove_clip(&mut self, name: &str) -> bool {
        self.clips.remove(name).is_some()
    }

    /// Есть ли клип с таким именем: зарегистрированный (`add_clip`) или встроенный
    /// (`"sfx:coin"`, `"music:calm"` — список в [`synth::BUILTIN`](crate::synth::BUILTIN)).
    pub fn has_clip(&self, name: &str) -> bool {
        self.clips.contains_key(name) || crate::synth::BUILTIN.contains(&name)
    }

    pub fn clip(&self, name: &str) -> Option<Arc<AudioClip>> {
        self.clips.get(name).cloned()
    }

    /// Подключить базу ассетов, чтобы играть клипы по GUID. Подсистема делает это сама,
    /// когда в `world.resources` появляется `AssetDatabase`.
    pub fn set_asset_database(&mut self, database: AssetDatabase) {
        self.assets = Some(database);
    }

    /// Декодировать файл заранее, чтобы первый звук не «подвисал» на загрузке.
    pub fn preload(&mut self, clip: impl Into<ClipRef>) -> bool {
        self.resolve(clip.into()).is_some()
    }

    /// Найти клип по ссылке. Неудача (нет имени, битый файл) пишется в лог один раз.
    pub fn resolve(&mut self, clip: ClipRef) -> Option<Arc<AudioClip>> {
        match clip {
            ClipRef::Clip(clip) => Some(clip),
            ClipRef::Name(name) => {
                if name.is_empty() {
                    return None;
                }
                let mut found = self.clips.get(&name).cloned();
                if found.is_none() {
                    // Встроенный звук (`"sfx:coin"`): синтезируется при первом обращении и
                    // запоминается под своим именем — дальше это тот же клип (музыка не
                    // перезапускается, если её включают снова).
                    if let Some(clip) = crate::synth::builtin(&name) {
                        let clip = Arc::new(clip);
                        self.clips.insert(name.clone(), Arc::clone(&clip));
                        found = Some(clip);
                    }
                }
                if found.is_none() {
                    self.warn_once(
                        format!("name:{name}"),
                        format!(
                            "audio clip `{name}` is not registered. Call `audio.add_clip(\"{name}\", clip)` first, use a file path (\"sounds/hit.wav\") or a built-in sound (\"sfx:coin\", \"music:calm\"){}",
                            did_you_mean_clip(&name, self.clips.keys().map(String::as_str).chain(crate::synth::BUILTIN.iter().copied()))
                        ),
                    );
                }
                found
            }
            ClipRef::Asset(guid) => {
                if guid.is_nil() {
                    return None;
                }
                if let Some(cached) = self.asset_clips.get(&guid) {
                    return cached.clone();
                }
                let loaded = self.load_asset(guid);
                self.asset_clips.insert(guid, loaded.clone());
                loaded
            }
        }
    }

    fn load_asset(&mut self, guid: Guid) -> Option<Arc<AudioClip>> {
        let Some(database) = &self.assets else {
            self.warn_once(
                format!("guid:{guid}"),
                format!("audio asset {guid} cannot be loaded: no asset database is mounted. Call `engine::mount_assets` first"),
            );
            return None;
        };
        let Some(name) = database.relative_path_of(guid).map(str::to_string) else {
            let message = format!("audio asset {guid} is not in the asset database. Was the file deleted?");
            self.warn_once(format!("guid:{guid}"), message);
            return None;
        };
        // Байты — из базы ассетов: так клип читается и из каталога, и из пака (браузер, Android).
        let loaded = database
            .read(guid)
            .map_err(|error| error.to_string())
            .and_then(|bytes| AudioClip::from_bytes(&bytes).map_err(|error| error.to_string()));
        match loaded {
            Ok(clip) => Some(Arc::new(clip)),
            Err(error) => {
                self.warn_once(format!("guid:{guid}"), format!("audio asset `{name}`: {error}"));
                None
            }
        }
    }

    fn warn_once(&mut self, key: String, message: String) {
        if self.warned.insert(key) {
            log::warn!("{message}");
        }
    }

    // ------------------------------------------------------------------
    // Запуск звуков
    // ------------------------------------------------------------------

    /// Проиграть один раз с параметрами по умолчанию (шина Sfx).
    /// Если клип не нашёлся, вернётся `VoiceId::NONE` и в лог попадёт подсказка.
    pub fn play(&mut self, clip: impl Into<ClipRef>) -> VoiceId {
        self.play_with(clip, PlayParams::default())
    }

    /// Проиграть в точке мира: громкость и панорама — относительно слушателя.
    pub fn play_at(&mut self, clip: impl Into<ClipRef>, position: Vec3) -> VoiceId {
        self.play_with(clip, PlayParams::default().at(position))
    }

    /// Проиграть на шине интерфейса.
    pub fn play_ui(&mut self, clip: impl Into<ClipRef>) -> VoiceId {
        self.play_with(clip, PlayParams::default().bus(Bus::Ui))
    }

    /// Проиграть с параметрами; возвращает ручку голоса.
    pub fn play_with(&mut self, clip: impl Into<ClipRef>, params: PlayParams) -> VoiceId {
        let Some(clip) = self.resolve(clip.into()) else {
            return VoiceId::NONE;
        };
        let voice = self.lock().play(Arc::clone(&clip), params.voice);
        if let (false, Some(position)) = (voice.is_none(), params.position) {
            let entry = SpatialVoice {
                voice,
                position,
                min: params.min_distance,
                max: params.max_distance,
            };
            self.apply_spatial(&entry);
            self.spatial_voices.push(entry);
        }
        voice
    }

    fn apply_spatial(&self, entry: &SpatialVoice) {
        let (gain, pan) = match &self.listener {
            Some(listener) => spatialize(listener, entry.position, entry.min, entry.max),
            None => (1.0, 0.0),
        };
        self.lock().set_spatial(entry.voice, gain, pan);
    }

    /// Пересчитать пространственные голоса (слушатель или источник могли сдвинуться).
    pub(crate) fn update_spatial_voices(&mut self) {
        let listener = self.listener;
        let mut mixer = self.mixer.lock().unwrap_or_else(|p| p.into_inner());
        self.spatial_voices.retain(|entry| {
            if !mixer.is_playing(entry.voice) {
                return false;
            }
            let (gain, pan) = match &listener {
                Some(l) => spatialize(l, entry.position, entry.min, entry.max),
                None => (1.0, 0.0),
            };
            mixer.set_spatial(entry.voice, gain, pan);
            true
        });
    }

    // ------------------------------------------------------------------
    // Управление голосами
    // ------------------------------------------------------------------

    pub fn stop(&mut self, voice: VoiceId) {
        self.lock().stop(voice);
    }

    pub fn set_volume(&mut self, voice: VoiceId, volume: f32) {
        self.lock().set_volume(voice, volume);
    }

    pub fn set_pitch(&mut self, voice: VoiceId, pitch: f32) {
        self.lock().set_pitch(voice, pitch);
    }

    pub fn set_pan(&mut self, voice: VoiceId, pan: f32) {
        self.lock().set_pan(voice, pan);
    }

    pub fn set_looping(&mut self, voice: VoiceId, looping: bool) {
        self.lock().set_looping(voice, looping);
    }

    /// Плавно заглушить голос за `seconds` и остановить его.
    pub fn fade_out(&mut self, voice: VoiceId, seconds: f32) {
        self.lock().fade_out(voice, seconds);
    }

    /// Плавно вернуть громкость голоса (после `fade_out`, пока он ещё не затих).
    pub fn fade_in(&mut self, voice: VoiceId, seconds: f32) {
        self.lock().fade_in(voice, seconds);
    }

    /// Голос ещё звучит (после `stop` — уже нет).
    pub fn is_playing(&self, voice: VoiceId) -> bool {
        self.lock().is_playing(voice)
    }

    /// Секунда клипа, на которой сейчас голос.
    pub fn position(&self, voice: VoiceId) -> Option<f32> {
        self.lock().position(voice)
    }

    /// Сколько голосов звучит сейчас.
    pub fn voice_count(&self) -> usize {
        self.lock().voice_count()
    }

    pub fn stop_all(&mut self) {
        self.lock().stop_all();
        self.music = None;
        self.spatial_voices.clear();
    }

    /// Остановить все голоса шины.
    pub fn stop_bus(&mut self, bus: Bus) {
        self.lock().stop_bus(bus);
    }

    /// Пауза всего звука (микшер замирает, голоса сохраняются).
    pub fn pause_all(&mut self) {
        self.user_paused = true;
        self.apply_pause();
    }

    pub fn resume_all(&mut self) {
        self.user_paused = false;
        self.apply_pause();
    }

    pub fn is_paused(&self) -> bool {
        self.user_paused
    }

    pub(crate) fn set_idle_paused(&mut self, paused: bool) {
        self.idle_paused = paused;
        self.apply_pause();
    }

    fn apply_pause(&self) {
        self.lock().set_paused(self.user_paused || self.idle_paused);
    }

    // ------------------------------------------------------------------
    // Шины
    // ------------------------------------------------------------------

    /// Громкость шины (0 — тишина, 1 — как есть). `Bus::Master` влияет на все.
    pub fn set_bus_volume(&mut self, bus: Bus, volume: f32) {
        self.lock().set_bus_volume(bus, volume);
    }

    pub fn bus_volume(&self, bus: Bus) -> f32 {
        self.lock().bus_volume(bus)
    }

    pub fn set_bus_muted(&mut self, bus: Bus, muted: bool) {
        self.lock().set_bus_muted(bus, muted);
    }

    pub fn is_bus_muted(&self, bus: Bus) -> bool {
        self.lock().is_bus_muted(bus)
    }

    /// Громкости шин из раздела Audio настроек проекта (Master, Music, Sfx, Ui).
    /// Вызывается при применении настроек (`engine::mount_assets`, редактор); игра,
    /// которая потом меняет громкость сама (`set_bus_volume`), перекрывает настройки.
    pub fn apply_settings(&mut self, settings: &engine_core::settings::AudioSettings) {
        let volume = |v: f32| if v.is_finite() { v.max(0.0) } else { 1.0 };
        let mut mixer = self.lock();
        mixer.set_bus_volume(Bus::Master, volume(settings.master_volume));
        mixer.set_bus_volume(Bus::Music, volume(settings.music_volume));
        mixer.set_bus_volume(Bus::Sfx, volume(settings.sfx_volume));
        mixer.set_bus_volume(Bus::Ui, volume(settings.ui_volume));
    }

    /// Заглушить весь звук, пока окно не в фокусе. Это делает [`AudioSystem`](crate::AudioSystem)
    /// по `AudioSettings::mute_when_unfocused`; заглушки шин игры не трогаются.
    pub fn set_focus_muted(&mut self, muted: bool) {
        self.lock().set_focus_muted(muted);
    }

    pub fn is_focus_muted(&self) -> bool {
        self.lock().is_focus_muted()
    }

    // ------------------------------------------------------------------
    // Монитор (редактор) и мастер-шина
    // ------------------------------------------------------------------

    /// Громкость прослушивания в редакторе (0..1) — поверх всех шин. Её ставит
    /// редактор (кнопка звука в Game View, Preferences → Game View); игра своими
    /// `set_bus_volume` её не перебивает. Меняется плавно.
    pub fn set_editor_volume(&mut self, volume: f32) {
        self.lock().set_monitor_volume(volume);
    }

    pub fn editor_volume(&self) -> f32 {
        self.lock().monitor_volume()
    }

    /// «Mute audio in editor»: заглушить весь звук игры, не трогая её шины.
    pub fn set_editor_muted(&mut self, muted: bool) {
        self.lock().set_monitor_muted(muted);
    }

    pub fn is_editor_muted(&self) -> bool {
        self.lock().is_monitor_muted()
    }

    /// Запас мастер-шины, дБ (по умолчанию −6; см. «Политика громкости» в `docs/api/audio.md`).
    pub fn set_headroom_db(&mut self, db: f32) {
        self.lock().set_headroom_db(db);
    }

    pub fn headroom_db(&self) -> f32 {
        self.lock().headroom_db()
    }

    /// Сколько копий одного клипа может звучать одновременно (по умолчанию 6).
    pub fn set_max_instances(&mut self, count: usize) {
        self.lock().set_max_instances(count);
    }

    // ------------------------------------------------------------------
    // Музыка
    // ------------------------------------------------------------------

    /// Самое короткое нарастание / перекрёстное затухание музыки, секунд: музыка
    /// никогда не начинается и не обрывается «в полную силу».
    pub const MIN_MUSIC_FADE: f32 = 0.5;

    /// Включить музыку (шина Music, по кругу). Предыдущая музыка затухает за
    /// `crossfade` секунд, а новая за это же время нарастает (равномощные кривые; не
    /// короче [`Self::MIN_MUSIC_FADE`]). Если этот клип уже играет как музыка, ничего
    /// не происходит.
    pub fn play_music(&mut self, clip: impl Into<ClipRef>, crossfade: f32) -> VoiceId {
        let Some(clip) = self.resolve(clip.into()) else {
            return VoiceId::NONE;
        };
        if let Some(current) = &self.music {
            if Arc::ptr_eq(&current.clip, &clip) && self.is_playing(current.voice) {
                return current.voice;
            }
        }
        let crossfade = if crossfade.is_finite() { crossfade.max(Self::MIN_MUSIC_FADE) } else { Self::MIN_MUSIC_FADE };
        if let Some(old) = self.music.take() {
            self.lock().fade_out(old.voice, crossfade);
        }
        let params = VoiceParams {
            looping: true,
            bus: Bus::Music,
            fade_in: crossfade,
            ..VoiceParams::default()
        };
        let voice = self.lock().play(Arc::clone(&clip), params);
        self.music = Some(MusicState { voice, clip });
        voice
    }

    /// Плавно выключить музыку.
    pub fn stop_music(&mut self, fade: f32) {
        if let Some(old) = self.music.take() {
            self.lock().fade_out(old.voice, fade);
        }
    }

    pub fn is_music_playing(&self) -> bool {
        self.music.as_ref().is_some_and(|m| self.is_playing(m.voice))
    }

    /// Голос текущей музыки.
    pub fn music_voice(&self) -> Option<VoiceId> {
        self.music.as_ref().map(|m| m.voice).filter(|v| self.is_playing(*v))
    }

    // ------------------------------------------------------------------
    // AudioSource
    // ------------------------------------------------------------------

    /// Запустить `AudioSource` объекта с начала (даже если `play_on_start` выключен).
    /// Звук начнётся в ближайшем кадре.
    pub fn play_source(&mut self, object: GameObjectId) {
        self.forced_play.insert(object);
    }

    /// Остановить `AudioSource` объекта.
    pub fn stop_source(&mut self, object: GameObjectId) {
        self.forced_play.remove(&object);
        let state = self.sources.entry(object).or_default();
        state.started = true;
        let voice = std::mem::take(&mut state.voice);
        self.lock().stop(voice);
    }

    /// Играет ли сейчас `AudioSource` объекта.
    pub fn is_source_playing(&self, object: GameObjectId) -> bool {
        self.sources.get(&object).is_some_and(|s| self.is_playing(s.voice))
    }

    /// Голос `AudioSource` объекта (чтобы, например, сменить высоту тона).
    pub fn source_voice(&self, object: GameObjectId) -> Option<VoiceId> {
        self.sources.get(&object).map(|s| s.voice).filter(|v| self.is_playing(*v))
    }

    // ------------------------------------------------------------------
    // Устройство и оффлайн-рендер
    // ------------------------------------------------------------------

    pub fn device_status(&self) -> &DeviceStatus {
        &self.device
    }

    /// Разрешить / запретить открывать звуковую карту. Без устройства звук «играет
    /// вхолостую»: голоса идут, затухания работают, `is_playing` честный.
    pub fn set_device_enabled(&mut self, enabled: bool) {
        if enabled {
            if self.device == DeviceStatus::Disabled {
                self.device = DeviceStatus::NotOpened;
            }
        } else {
            self.device = DeviceStatus::Disabled;
        }
    }

    /// Отрисовать следующие сэмплы микшера в `out` (чередующееся стерео). Нужно для
    /// записи звука в файл и тестов; при открытом устройстве звук забирает оно.
    pub fn render(&mut self, out: &mut [f32]) {
        self.lock().render(out);
    }

    pub(crate) fn set_listener(&mut self, listener: Option<ListenerState>, gain: f32) {
        self.listener = listener;
        self.lock().set_listener_gain(gain);
    }
}

fn did_you_mean_clip<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> String {
    let lower = name.to_ascii_lowercase();
    let best = known
        .filter(|k| {
            let k = k.to_ascii_lowercase();
            k.contains(&lower) || lower.contains(&k)
        })
        .min();
    match best {
        Some(k) => format!(". Did you mean `{k}`?"),
        None => String::new(),
    }
}
