//! Компоненты звука: `AudioSource` и `AudioListener`.

use engine_assets::AssetHandle;
use engine_core::prelude::*;

use crate::mixer::Bus;

component! {
    /// Источник звука на объекте. Клип задаётся ссылкой на ассет (`clip`) или именем
    /// клипа, добавленного из кода (`clip_name`, см. `Audio::add_clip`).
    pub struct AudioSource {
        /// Аудиофайл (.wav / .ogg) из проекта.
        pub clip: AssetHandle<engine_assets::AudioClip> = AssetHandle::none(),
        /// Имя клипа, зарегистрированного из кода. Если задано, берётся вместо `clip`.
        pub clip_name: String = String::new(),
        /// Громкость 0..1 (можно больше — но возможен клиппинг).
        pub volume: f32 = 1.0,
        /// Скорость воспроизведения: 2.0 — вдвое быстрее и на октаву выше.
        pub pitch: f32 = 1.0,
        /// Повторять клип по кругу.
        pub looping: bool = false,
        /// Начать играть сразу, как объект появился в сцене (Unity: Play On Awake).
        pub play_on_start: bool = true,
        /// 3D-звук: громкость падает с расстоянием до `AudioListener`, панорама следует за положением.
        pub spatial: bool = false,
        /// Ближе этого расстояния громкость полная.
        pub min_distance: f32 = 1.0,
        /// Дальше этого расстояния звука не слышно.
        pub max_distance: f32 = 30.0,
        /// Шина громкости.
        pub bus: Bus = Bus::Sfx,
    }
}
impl Component for AudioSource {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl AudioSource {
    /// Источник клипа, зарегистрированного по имени: `AudioSource::named("coin")`.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            clip_name: name.into(),
            ..Self::default()
        }
    }

    /// Источник аудиофайла из базы ассетов.
    pub fn from_asset(clip: AssetHandle<engine_assets::AudioClip>) -> Self {
        Self {
            clip,
            ..Self::default()
        }
    }

    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub fn with_pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }

    pub fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    pub fn with_bus(mut self, bus: Bus) -> Self {
        self.bus = bus;
        self
    }

    /// Включить 3D-звук с заданным диапазоном расстояний.
    pub fn with_spatial(mut self, min_distance: f32, max_distance: f32) -> Self {
        self.spatial = true;
        self.min_distance = min_distance;
        self.max_distance = max_distance;
        self
    }

    /// Не играть автоматически: запускайте через `Audio::play_source`.
    pub fn manual(mut self) -> Self {
        self.play_on_start = false;
        self
    }
}

component! {
    /// «Уши» игрока: относительно этого объекта считаются расстояние и панорама
    /// пространственных источников. Обычно вешается на камеру или на персонажа.
    /// Если слушателя в сцене нет, пространственные источники звучат как обычные.
    pub struct AudioListener {
        /// Общий множитель громкости, пока слушатель активен.
        pub volume: f32 = 1.0,
    }
}
impl Component for AudioListener {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}
