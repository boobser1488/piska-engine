//! `engine-audio` — звук: микшер, синтезатор, источники и слушатель.
//!
//! Схема:
//!
//! * [`Mixer`] — программный микшер: голоса → шины → стерео `f32`. Не зависит от
//!   устройства, проверяется в тестах по готовым сэмплам.
//! * [`AudioClip`] — сэмплы в памяти: из `.wav` / `.ogg`, из кода (`from_samples`)
//!   или из синтезатора ([`synth`]).
//! * [`Audio`] — ресурс в `world.resources`: `play`, музыка, шины, пауза.
//! * [`AudioSource`] / [`AudioListener`] — компоненты для сцены и редактора.
//! * [`AudioSystem`] — подсистема: каждый кадр переносит сцену в микшер. Звуковая
//!   карта открывается лениво, при первом звуке; нет устройства — игра идёт молча.
//!
//! Подробное руководство с рецептами: `docs/api/audio.md`.
#![forbid(unsafe_code)]

mod audio;
mod backend;
mod clip;
mod components;
pub mod dsp;
mod mixer;
/// Короткие формы для скриптов: `ctx.play("jump")`, `ctx.music("theme")`.
mod script;
pub mod synth;
mod system;

pub use audio::{spatialize, Audio, ClipRef, DeviceStatus, ListenerState, PlayParams};
pub use clip::{AudioClip, ClipError, Level};
pub use components::{AudioListener, AudioSource};
pub use mixer::{pan_gains, Bus, Mixer, VoiceId, VoiceParams};
pub use script::AudioCtx;
pub use system::AudioSystem;

use engine_core::app::App;

/// Всё, что нужно игре: попадает в `engine::prelude`.
pub mod prelude {
    pub use crate::script::AudioCtx;
    pub use crate::synth::{self, Melody, Sound, Tone, Wave};
    pub use crate::{
        Audio, AudioClip, AudioListener, AudioSource, Bus, ClipRef, DeviceStatus, PlayParams, VoiceId,
        VoiceParams,
    };
}

/// Зарегистрировать компоненты звука в реестре типов (для загрузки сцен).
pub fn register_components(registry: &mut engine_core::reflect::TypeRegistry) {
    registry.register::<AudioSource>().register::<AudioListener>();
    registry.categorize(&["AudioSource", "AudioListener"], "Audio");
}

/// Подключить звук к приложению: компоненты, ресурс `Audio`, подсистема, хук
/// раздела Audio настроек проекта. Вызывается из `engine::app()`. Повторный вызов безвреден.
pub fn install(app: &mut App) {
    register_components(&mut app.world.registry);
    if !app.world.resources.contains::<Audio>() {
        app.world.resources.insert(Audio::new());
        app.add_subsystem(AudioSystem::new());
    }
    app.world.add_settings_hook("audio", apply_project_settings);
}

/// Применить раздел Audio из ресурса `ProjectSettings` к ресурсу [`Audio`]: громкости
/// шин. `mute_when_unfocused` читает [`AudioSystem`] каждый кадр. Вызывается сам из
/// `World::apply_project_settings` (хук, см. [`install`]).
pub fn apply_project_settings(world: &mut engine_core::world::World) {
    let Some(settings) = world.resources.get::<engine_core::settings::ProjectSettings>() else {
        return;
    };
    let section = settings.audio.clone();
    if let Some(audio) = world.resources.get_mut::<Audio>() {
        audio.apply_settings(&section);
    }
}
