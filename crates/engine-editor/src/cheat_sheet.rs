//! Help → Scripting Cheat Sheet: короткие формы скриптов с примерами в одну строку и
//! длинными формами, которые они оборачивают. Данные — `engine_core::script::CHEAT_SHEET`,
//! тот же источник, что у таблицы в `docs/api/gameplay.md` (их сверяет тест).

use egui::vec2;
use engine_core::script::{CheatEntry, CHEAT_SHEET};

use crate::controls;
use crate::i18n::tr_s;
use crate::icons::Icon;
use crate::state::{Action, Editor};
use crate::theme::{self, item};

/// Открыто ли окно и что ищут (живёт в памяти egui: окну не нужно поле в `Editor`).
#[derive(Clone, Default)]
struct CheatState {
    open: bool,
    search: String,
}

fn state_id() -> egui::Id {
    egui::Id::new("editor_scripting_cheat_sheet")
}

/// Открыть или закрыть окно (меню и тесты).
pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<CheatState>(state_id()).open = open);
}

/// Открыто ли окно.
pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<CheatState>(state_id()).is_some_and(|s| s.open))
}

/// Строки шпаргалки, подходящие под поиск (без учёта регистра, по всем колонкам).
pub fn matching(search: &str) -> Vec<&'static CheatEntry> {
    let needle = search.trim().to_lowercase();
    CHEAT_SHEET
        .iter()
        .filter(|e| {
            needle.is_empty()
                || [e.topic, e.short, e.example, e.wraps].iter().any(|s| s.to_lowercase().contains(&needle))
        })
        .collect()
}

/// Пункт меню Help.
pub(crate) fn menu_item(ui: &mut egui::Ui) {
    if item(tr_s("Scripting Cheat Sheet")).icon(Icon::Script).show(ui) {
        set_open(ui.ctx(), true);
    }
}

/// Окно (вызывается вместе с остальными окнами справки).
pub(crate) fn window(editor: &mut Editor, ctx: &egui::Context) {
    let mut state = ctx.data(|d| d.get_temp::<CheatState>(state_id()).unwrap_or_default());
    if !state.open {
        return;
    }
    let doc = editor.doc_file("gameplay.md");
    let mut open_doc = false;
    crate::window_kit::tool_window(state_id(), tr_s("Scripting Cheat Sheet"))
        .default_size(vec2(820.0, 560.0))
        .show(ctx, &mut state.open, |ui| {
            controls::muted(
                ui,
                tr_s("Short forms for scripts. The long forms on the right keep working; a script may mix both."),
            );
            ui.add_space(theme::SP_S);
            controls::row(ui, |ui| {
                controls::search_field(ui, &mut state.search, tr_s("Search: jump, spawn, sound, ui…"), 320.0);
                if doc.is_some() && controls::button(ui, tr_s("Open the scripting guide")).clicked() {
                    open_doc = true;
                }
            });
            ui.add_space(theme::SP_S);
            let rows = matching(&state.search);
            if rows.is_empty() {
                controls::muted(ui, tr_s("Nothing matches the search."));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut topic = "";
                for entry in rows {
                    if entry.topic != topic {
                        topic = entry.topic;
                        ui.add_space(theme::SP_S);
                        controls::section(ui, topic);
                    }
                    entry_row(ui, entry);
                }
            });
        });
    if open_doc {
        if let Some(doc) = doc {
            editor.apply(Action::OpenCode(doc));
        }
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

/// Строка: короткая форма, пример, длинная форма (приглушённо).
fn entry_row(ui: &mut egui::Ui, entry: &CheatEntry) {
    let width = ui.available_width();
    let short_w = (width * 0.32).clamp(180.0, 300.0);
    ui.horizontal_top(|ui| {
        ui.add_sized(
            vec2(short_w, theme::ROW),
            egui::Label::new(egui::RichText::new(entry.short).font(theme::mono()).color(theme::TEXT_STRONG)).truncate(),
        )
        .on_hover_text(entry.short);
        ui.vertical(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(entry.example).font(theme::mono()).color(theme::TEXT)).wrap());
            ui.add(
                egui::Label::new(egui::RichText::new(entry.wraps).font(theme::caption()).color(theme::TEXT_MUTED)).wrap(),
            );
        });
    });
    ui.add_space(theme::SP_XS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_filters_every_column() {
        assert_eq!(matching("").len(), CHEAT_SHEET.len());
        let jump = matching("JUMP");
        assert!(!jump.is_empty() && jump.iter().all(|e| format!("{e:?}").to_lowercase().contains("jump")));
        assert!(matching("instantiate_at").iter().any(|e| e.short.starts_with("ctx.spawn")), "long forms are searchable");
        assert!(matching("no such words at all").is_empty());
    }
}
