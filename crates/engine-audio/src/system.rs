//! Подсистема звука: переносит состояние сцены в микшер каждый кадр.

use engine_assets::AssetDatabase;
use engine_core::prelude::*;

use crate::audio::{spatialize, Audio, ClipRef, DeviceStatus, ListenerState};
use crate::backend;
use crate::components::{AudioListener, AudioSource};
use crate::mixer::{VoiceId, VoiceParams};

/// Звуковая подсистема. Добавляется в приложение вызовом [`crate::install`].
pub struct AudioSystem {
    output: Option<backend::Output>,
    /// Корень подключённой базы ассетов и число ассетов — чтобы заметить смену базы.
    database_stamp: Option<(std::path::PathBuf, usize)>,
    /// Какая сцена была в конце последнего кадра (`Scene::instance_id`, `None` — пустая):
    /// по ней `idle` замечает, что сцену заменили (Stop в редакторе), и обрывает звуки старой.
    last_scene: Option<u64>,
}

impl AudioSystem {
    pub fn new() -> Self {
        Self {
            output: None,
            database_stamp: None,
            last_scene: None,
        }
    }
}

impl Default for AudioSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Какая сцена сейчас в мире (`None` — пустая). Без обхода объектов.
fn scene_stamp(scene: &Scene) -> Option<u64> {
    (!scene.is_empty()).then(|| scene.instance_id())
}

fn audio_disabled_by_env() -> bool {
    let off = std::env::var("ENGINE_AUDIO")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "off" | "0" | "none" | "false"))
        .unwrap_or(false);
    // Скриншот-режим — это прогон без окна и без людей: звук там не нужен.
    off || std::env::var_os("ENGINE_SCREENSHOT").is_some()
}

impl Subsystem for AudioSystem {
    fn name(&self) -> &'static str {
        "audio"
    }

    fn init(&mut self, world: &mut World) {
        if audio_disabled_by_env() {
            if let Some(audio) = world.resources.get_mut::<Audio>() {
                audio.set_device_enabled(false);
            }
        }
    }

    fn pre_update(&mut self, world: &mut World) {
        self.sync(world);
    }

    fn post_update(&mut self, world: &mut World) {
        self.sync(world);
        let dt = world.time.delta;
        self.last_scene = scene_stamp(&world.scene);
        let Some(audio) = world.resources.get_mut::<Audio>() else {
            return;
        };
        self.service_device(audio);
        // Без устройства микшер «играет вхолостую»: время идёт, голоса заканчиваются.
        if self.output.is_none() {
            audio.with_mixer_mut(|m| m.advance(dt));
        }
    }

    /// Игра стоит. Звук замирает (Pause); если сцену заменили (Stop — сцена
    /// восстанавливается из снимка), всё, что играло, обрывается.
    fn idle(&mut self, world: &mut World) {
        let current = scene_stamp(&world.scene);
        let replaced = match (current, self.last_scene) {
            (Some(now), Some(last)) => now != last,
            (None, Some(_)) => true,
            _ => false,
        };
        self.last_scene = current;
        let Some(audio) = world.resources.get_mut::<Audio>() else {
            return;
        };
        if replaced {
            audio.stop_all();
            audio.sources.clear();
            audio.forced_play.clear();
        }
        audio.set_idle_paused(true);
        // Голоса объектов, которых больше нет, обрываем и на паузе.
        cleanup_dead_sources(audio, &world.scene);
    }
}

impl AudioSystem {
    /// Один проход синхронизации сцены с микшером.
    fn sync(&mut self, world: &mut World) {
        // База ассетов могла появиться или смениться после `install`.
        let database = world.resources.get::<AssetDatabase>();
        let stamp = database.map(|db| (db.root().to_path_buf(), db.len()));
        let fresh_database = if stamp != self.database_stamp {
            self.database_stamp = stamp;
            database.cloned()
        } else {
            None
        };

        let focus_muted = focus_muted(world);
        let World { scene, resources, .. } = world;
        let Some(audio) = resources.get_mut::<Audio>() else {
            return;
        };
        if let Some(database) = fresh_database {
            audio.set_asset_database(database);
        }
        audio.set_idle_paused(false);
        if audio.is_focus_muted() != focus_muted {
            audio.set_focus_muted(focus_muted);
        }

        update_listener(audio, scene);
        cleanup_dead_sources(audio, scene);
        sync_sources(audio, scene);
        audio.update_spatial_voices();
        audio.with_mixer_mut(|m| m.drain_retired());
    }

    /// Открыть устройство при первом звуке; заметить его потерю.
    fn service_device(&mut self, audio: &mut Audio) {
        if let Some(output) = &self.output {
            if output.is_lost() {
                log::warn!("audio device was lost; the game keeps running without sound");
                self.output = None;
                audio.device = DeviceStatus::Unavailable("the output device was lost".to_string());
            }
            return;
        }
        if audio.device != DeviceStatus::NotOpened || audio.voice_count() == 0 {
            return;
        }
        match backend::open(audio.mixer_handle()) {
            Ok(output) => {
                log::info!(
                    "audio output: {} ({} Hz, {} ch)",
                    output.name,
                    output.sample_rate,
                    output.channels
                );
                audio.device = DeviceStatus::Active {
                    name: output.name.clone(),
                    sample_rate: output.sample_rate,
                    channels: output.channels,
                };
                self.output = Some(output);
            }
            Err(reason) => {
                // Один раз в лог и дальше тишина: игре звук не обязателен.
                log::warn!("audio is off: {reason}");
                audio.device = DeviceStatus::Unavailable(reason);
            }
        }
    }
}

/// Глушить ли звук из-за потери фокуса: настройка `mute_when_unfocused` включена, а
/// оконный цикл сообщил, что окно не в фокусе (без окна ресурса нет — не глушим).
fn focus_muted(world: &World) -> bool {
    let unfocused = world
        .resources
        .get::<engine_core::settings::WindowFocus>()
        .is_some_and(|focus| !focus.focused);
    unfocused
        && world
            .resources
            .get::<engine_core::settings::ProjectSettings>()
            .is_some_and(|settings| settings.audio.mute_when_unfocused)
}

fn update_listener(audio: &mut Audio, scene: &Scene) {
    // Поиск по индексу сцены: стоит столько, сколько слушателей, а не объектов.
    let listener = scene
        .find_with::<AudioListener>()
        .into_iter()
        .find(|&id| scene.is_active(id));
    match listener {
        Some(id) => {
            let volume = scene.get_component::<AudioListener>(id).map_or(1.0, |l| l.volume);
            let state = ListenerState {
                position: scene.world_position(id),
                right: scene.world_rotation(id) * Vec3::X,
            };
            audio.set_listener(Some(state), volume);
        }
        None => audio.set_listener(None, 1.0),
    }
}

/// Остановить голоса источников, объекты которых уничтожены, выключены или потеряли компонент.
fn cleanup_dead_sources(audio: &mut Audio, scene: &Scene) {
    if audio.sources.is_empty() {
        return;
    }
    let dead: Vec<GameObjectId> = audio
        .sources
        .keys()
        .copied()
        .filter(|&id| {
            !(scene.contains(id) && scene.is_active(id) && scene.get_component::<AudioSource>(id).is_some())
        })
        .collect();
    for id in dead {
        if let Some(state) = audio.sources.remove(&id) {
            audio.stop(state.voice);
        }
        audio.forced_play.remove(&id);
    }
}

fn sync_sources(audio: &mut Audio, scene: &Scene) {
    if scene.count_with::<AudioSource>() == 0 {
        return;
    }
    for id in scene.find_with::<AudioSource>() {
        if !scene.is_active(id) {
            continue;
        }
        let Some(source) = scene.get_component::<AudioSource>(id).cloned() else {
            continue;
        };
        let position = scene.world_position(id);
        let forced = audio.forced_play.remove(&id);
        let (started, old_voice) = {
            let state = audio.sources.entry(id).or_default();
            (state.started, state.voice)
        };

        if forced || (!started && source.play_on_start) {
            let clip_ref = if source.clip_name.is_empty() {
                ClipRef::Asset(source.clip.guid())
            } else {
                ClipRef::Name(source.clip_name.clone())
            };
            audio.stop(old_voice);
            // Клип мог ещё не появиться (регистрируется позже) — тогда пробуем в следующем кадре.
            if let Some(clip) = audio.resolve(clip_ref) {
                let params = VoiceParams {
                    volume: source.volume,
                    pitch: source.pitch,
                    looping: source.looping,
                    bus: source.bus,
                    ..VoiceParams::default()
                };
                let voice = audio.with_mixer_mut(|m| m.play(clip, params));
                let state = audio.sources.entry(id).or_default();
                state.voice = voice;
                state.started = true;
            } else {
                audio.sources.entry(id).or_default().voice = VoiceId::NONE;
            }
        }

        // Живые параметры компонента — в голос.
        let voice = audio.sources.get(&id).map_or(VoiceId::NONE, |s| s.voice);
        if voice.is_none() {
            continue;
        }
        let (gain, pan) = match (&audio.listener, source.spatial) {
            (Some(listener), true) => spatialize(listener, position, source.min_distance, source.max_distance),
            _ => (1.0, 0.0),
        };
        audio.with_mixer_mut(|m| {
            m.set_volume(voice, source.volume);
            m.set_pitch(voice, source.pitch);
            m.set_looping(voice, source.looping);
            m.set_voice_bus(voice, source.bus);
            m.set_spatial(voice, gain, pan);
        });
    }
}
