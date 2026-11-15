//! Короткие формы звука для скриптов: `ctx.play("jump")`, `ctx.play_at(..)`,
//! `ctx.music("theme")`, `ctx.stop_music()`. Обёртки над ресурсом [`Audio`]; длинная форма
//! (`ctx.resource_mut::<Audio>()`) остаётся.
//!
//! Имя — клип, зарегистрированный `audio.add_clip(..)`, или путь к файлу в ассетах
//! (`"sounds/jump.wav"`). Неизвестное имя — подсказка в лог один раз (её пишет [`Audio`]).

use engine_assets::AssetDatabase;
use engine_core::prelude::*;
use engine_core::script::AsVec3;

use crate::{Audio, ClipRef, VoiceId};

/// Звук из скрипта: `use engine::prelude::*;` уже подключает этот трейт.
pub trait AudioCtx {
    /// Проиграть звук один раз: `ctx.play("coin")` или `ctx.play("sounds/coin.wav")`.
    fn play(&mut self, clip: &str) -> VoiceId;
    /// Проиграть в точке мира (громкость и панорама — от слушателя).
    fn play_at(&mut self, clip: &str, position: impl AsVec3) -> VoiceId;
    /// Включить музыку по кругу (с плавным переходом от прежней).
    fn music(&mut self, clip: &str) -> VoiceId;
    /// Плавно выключить музыку.
    fn stop_music(&mut self);
}

/// Имя клипа или путь к файлу ассетов → то, что понимает [`Audio`].
fn clip_ref(world: &World, clip: &str) -> ClipRef {
    let registered = world.resources.get::<Audio>().is_some_and(|a| a.has_clip(clip));
    if !registered && clip.contains('.') {
        if let Some(guid) = world.resources.get::<AssetDatabase>().and_then(|db| db.guid_of(clip)) {
            return ClipRef::Asset(guid);
        }
    }
    ClipRef::Name(clip.to_string())
}

impl AudioCtx for Ctx<'_> {
    fn play(&mut self, clip: &str) -> VoiceId {
        let clip = clip_ref(self.world, clip);
        self.resource_mut::<Audio>().map_or(VoiceId::NONE, |audio| audio.play(clip))
    }

    fn play_at(&mut self, clip: &str, position: impl AsVec3) -> VoiceId {
        let clip = clip_ref(self.world, clip);
        let at = position.with_z(0.0);
        self.resource_mut::<Audio>().map_or(VoiceId::NONE, |audio| audio.play_at(clip, at))
    }

    fn music(&mut self, clip: &str) -> VoiceId {
        let clip = clip_ref(self.world, clip);
        self.resource_mut::<Audio>()
            .map_or(VoiceId::NONE, |audio| audio.play_music(clip, 1.0))
    }

    fn stop_music(&mut self) {
        if let Some(audio) = self.resource_mut::<Audio>() {
            audio.stop_music(1.0);
        }
    }
}
