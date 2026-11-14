//! Страница Preferences → Shortcuts: таблица всех команд редактора ([`COMMANDS`]) по
//! разделам. Щелчок по сочетанию включает запись: следующее нажатие клавиш становится
//! новым сочетанием (Esc — отмена; занятое сочетание спрашивает, заменить ли). Поиск
//! окна находит команду по названию, разделу и сочетанию; стрелка строки и Reset page
//! возвращают умолчания.

use crate::i18n::{tr_s, trf};
use egui::{pos2, vec2, Rect, Sense};

use super::SettingsUi;
use crate::icons::{self, Icon};
use crate::shortcuts::COMMANDS;
use crate::state::Editor;
use crate::theme;

/// Ширина поля сочетания.
const KEY_W: f32 = 150.0;

impl Editor {
    /// Поле сочетания команды `id` на странице Shortcuts (щелчок — записать новое).
    pub fn shortcut_button_id(id: &str) -> egui::Id {
        egui::Id::new(("shortcut_button", id.to_string()))
    }

    /// Кнопка «снять сочетание» команды `id`.
    pub fn shortcut_clear_id(id: &str) -> egui::Id {
        egui::Id::new(("shortcut_clear", id.to_string()))
    }
}

pub(super) fn shortcuts(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let mut category = "";
    for def in COMMANDS {
        if def.category != category {
            category = def.category;
            s.group(tr_s(category));
        }
        let current = editor.prefs.shortcuts.binding(def.id);
        let listening = editor.settings.keys.listening == Some(def.id);
        let default = def.default.map_or_else(|| "none".to_string(), |b| b.text());
        let now = current.map_or_else(|| "none".to_string(), |b| b.text());
        // Описание несёт и текущее сочетание: поиск «ctrl+s» находит команду.
        let description = if editor.prefs.shortcuts.is_default(def.id) {
            trf!("{p0} · {now}", p0 = tr_s(def.category), now = now)
        } else {
            trf!("{p0} · {now} · default {default}", p0 = tr_s(def.category), now = now, default = default)
        };
        let row = s.row(tr_s(def.title), &description, !editor.prefs.shortcuts.is_default(def.id), |ui| {
            let text = if listening {
                tr_s("Press keys…").to_string()
            } else {
                current.map_or_else(|| tr_s("None").to_string(), |b| b.text())
            };
            let (rect, _) = ui.allocate_exact_size(vec2(KEY_W, theme::FIELD_H), Sense::hover());
            let response = ui.interact(rect, Editor::shortcut_button_id(def.id), Sense::click());
            let painter = ui.painter();
            let fill = if listening {
                theme::accent_soft()
            } else if response.hovered() {
                theme::BG_HOVER
            } else {
                theme::BG_FIELD
            };
            let stroke = if listening { theme::accent() } else { theme::LINE };
            crate::controls::paint_frame(painter, rect, theme::R_S, fill, stroke);
            let color = if listening {
                theme::TEXT_STRONG
            } else if current.is_some() {
                theme::TEXT
            } else {
                theme::TEXT_DISABLED
            };
            let galley = painter.layout_no_wrap(text, theme::mono(), color);
            painter.galley(pos2(rect.left() + theme::SP_M, rect.center().y - galley.size().y * 0.5), galley, color);
            let clicked = response
                .on_hover_text(if listening {
                    tr_s("Press the new shortcut now. Esc cancels")
                } else {
                    tr_s("Click, then press the new shortcut")
                })
                .clicked();
            let mut clear = false;
            if current.is_some() && !listening {
                let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                let response = ui.interact(rect, Editor::shortcut_clear_id(def.id), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
                }
                let color = if response.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
                icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(theme::ICON_S, theme::ICON_S)), Icon::Close, color);
                clear = response.on_hover_text(tr_s("Remove the shortcut")).clicked();
            }
            (clicked, clear)
        });
        if let Some((clicked, clear)) = row.inner {
            if clicked {
                editor.settings.keys.listening = if listening { None } else { Some(def.id) };
            }
            if clear {
                editor.prefs.shortcuts.set(def.id, None);
            }
        }
        if row.reset {
            editor.prefs.shortcuts.reset(def.id);
            if listening {
                editor.settings.keys.listening = None;
            }
        }
    }
    s.note(tr_s("Shortcuts are kept with your preferences. Keys of the Project window (copy, paste, rename in the file list) and text fields are not listed here."));
}
