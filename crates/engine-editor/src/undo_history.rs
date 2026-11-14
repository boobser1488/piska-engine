//! Окно Edit → Undo History: шаги Undo сцены по порядку, «вы здесь» — текущая позиция,
//! шаги после неё (их вернёт Redo) — приглушённо. Щелчок по строке — перейти к ней.

use crate::i18n::{tr_s, trf};

use egui::{pos2, vec2, Align2, Rect, Sense};

use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::theme;

impl Editor {
    pub(crate) fn undo_history_window(&mut self, ctx: &egui::Context) {
        if !self.undo_history_open {
            return;
        }
        let (steps, position) = self.undo_history();
        let mut open = true;
        let mut jump = None;
        crate::window_kit::tool_window(egui::Id::new("undo_history_window"), tr_s("Undo History"))
            .default_size(vec2(280.0, 320.0))
            .show(ctx, &mut open, |ui| {
                ui.label(
                    egui::RichText::new(trf!("{done} of {total} steps applied", done = position, total = steps.len()))
                        .size(theme::FONT_CAPTION)
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(theme::SP_S);
                egui::ScrollArea::vertical().id_salt("undo_history_list").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    // Строка 0 — состояние до первого шага (открытая сцена).
                    let rows = std::iter::once(tr_s("Scene opened").to_string()).chain(steps.iter().cloned());
                    for (index, label) in rows.enumerate() {
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
                        let row = ui.interact(rect, Editor::undo_history_row_id(index), Sense::click());
                        let current = index == position;
                        let future = index > position;
                        theme::list_row(ui.painter(), rect, current, true, row.hovered());
                        let color = if current {
                            theme::TEXT_STRONG
                        } else if future {
                            theme::TEXT_DISABLED
                        } else {
                            theme::TEXT
                        };
                        let icon = if index == 0 { Icon::Scene } else { Icon::Undo };
                        icons::paint(
                            ui.painter(),
                            Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
                            icon,
                            if future { theme::TEXT_DISABLED } else { theme::TEXT_MUTED },
                        );
                        ui.painter().text(pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, label, theme::body(), color);
                        let tip = if future { tr_s("Redo up to this step") } else { tr_s("Undo back to this step") };
                        if row.on_hover_text(tip).clicked() && !current {
                            jump = Some(index);
                        }
                    }
                });
            });
        if let Some(index) = jump {
            self.jump_in_history(index);
        }
        if !open {
            self.undo_history_open = false;
        }
    }

    /// Строка окна Undo History (для тестов): 0 — открытая сцена, дальше — шаги.
    pub fn undo_history_row_id(index: usize) -> egui::Id {
        egui::Id::new(("undo_history_row", index))
    }

    /// Открыть или закрыть окно Undo History.
    pub fn show_undo_history(&mut self, open: bool) {
        self.undo_history_open = open;
    }

    /// Открыто ли окно Undo History.
    pub fn is_undo_history_open(&self) -> bool {
        self.undo_history_open
    }

    /// Открыто ли окно File → Build….
    pub fn is_build_dialog_open(&self) -> bool {
        self.export.open
    }
}
