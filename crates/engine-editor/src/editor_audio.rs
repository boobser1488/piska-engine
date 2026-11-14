//! Звук игры в редакторе: кнопка-динамик на полосе окна Game и настройки
//! Preferences → Game View → Audio («Mute audio in editor», «Editor volume»). Значения
//! живут в [`Preferences`] (переживают перезапуск) и каждый кадр передаются в
//! `engine_audio::Audio` — громкость прослушивания поверх всех шин игры: код игры её не
//! перебивает, а экспортированная игра о ней не знает.

use crate::i18n::tr_s;
use crate::icons::Icon;
use crate::prefs::Preferences;
use crate::state::Editor;
use crate::theme;

/// Передать настройки звука редактора в аудио-подсистему (если она есть).
pub(crate) fn apply(editor: &mut Editor) {
    let (muted, volume) = (editor.prefs.editor_audio_muted, editor.prefs.editor_volume.clamp(0.0, 1.0));
    if let Some(audio) = editor.app.world.resources.get_mut::<engine_audio::Audio>() {
        if audio.is_editor_muted() != muted {
            audio.set_editor_muted(muted);
        }
        if (audio.editor_volume() - volume).abs() > 1e-4 {
            audio.set_editor_volume(volume);
        }
    }
}

/// Кнопка-динамик на полосе окна Game: включить / заглушить звук игры в редакторе.
pub(crate) fn speaker_toggle(ui: &mut egui::Ui, prefs: &mut Preferences) {
    let mut sound_on = !prefs.editor_audio_muted;
    let tip = if sound_on {
        tr_s("Game audio is on in the editor. Click to mute it (Preferences → Game View → Audio)")
    } else {
        tr_s("Game audio is muted in the editor. Click to turn it on")
    };
    theme::toggle_button(ui, &mut sound_on, Icon::Audio, tr_s("Audio"), tip);
    prefs.editor_audio_muted = !sound_on;
}

/// Строки страницы Preferences → Game View: группа Audio.
pub(crate) fn preferences_rows(s: &mut crate::settings::SettingsUi<'_>, prefs: &mut Preferences) {
    let defaults = Preferences::default();
    s.group(tr_s("Audio"));
    s.toggle(
        tr_s("Mute audio in editor"),
        tr_s("Silence the game in Play mode inside the editor. The game's own volume settings are not changed."),
        &mut prefs.editor_audio_muted,
        defaults.editor_audio_muted,
    );
    s.percent(
        tr_s("Editor volume"),
        tr_s("How loud the game plays inside the editor, on top of the game's own volume. Exported games are not affected."),
        &mut prefs.editor_volume,
        defaults.editor_volume,
    );
}

impl Editor {
    /// Звук игры в редакторе: (заглушен, громкость 0..1) — как в Preferences → Game View.
    pub fn editor_audio(&self) -> (bool, f32) {
        (self.prefs.editor_audio_muted, self.prefs.editor_volume)
    }

    /// Задать звук игры в редакторе (как кнопка-динамик и Preferences → Game View).
    pub fn set_editor_audio(&mut self, muted: bool, volume: f32) {
        self.prefs.editor_audio_muted = muted;
        self.prefs.editor_volume = volume.clamp(0.0, 1.0);
        apply(self);
    }
}
