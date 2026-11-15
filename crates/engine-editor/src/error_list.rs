//! Окно Error List (как в Visual Studio): все ошибки, предупреждения и сообщения
//! мгновенной проверки и компилятора — по файлам проекта, с фильтрами по важности и по
//! области (текущий файл, открытые файлы, весь проект), поиском и счётчиками.
//! Двойной щелчок или Enter — перейти к месту; Ctrl+C — скопировать строку; F8 /
//! Shift+F8 — следующая / предыдущая ошибка (и из вкладки Code).

use crate::i18n::{tr_s, trf};
use egui::{pos2, vec2, Align2, Rect, Sense};

use crate::code_checks::{entries, entry_text, Entry};
use crate::code_diagnostics::{Counts, Severity};
use crate::code_view::{severity_color, severity_icon};
use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::theme;

/// Какие файлы показывает список.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Scope {
    CurrentFile,
    OpenFiles,
    /// Все файлы, о которых что-то известно (и не открытые — ошибки компилятора).
    #[default]
    Project,
}

/// Состояние окна.
pub(crate) struct ErrorListState {
    pub show_errors: bool,
    pub show_warnings: bool,
    pub show_infos: bool,
    pub scope: Scope,
    pub filter: String,
    /// Выбранная строка: (файл, байт начала, текст) — переживает пересчёт списка.
    pub selected: Option<(std::path::PathBuf, usize, String)>,
}

impl Default for ErrorListState {
    fn default() -> Self {
        Self { show_errors: true, show_warnings: true, show_infos: true, scope: Scope::Project, filter: String::new(), selected: None }
    }
}

impl Editor {
    /// Строка Error List с номером `index` (для тестов: щелчок по ней).
    pub fn error_list_row_id(index: usize) -> egui::Id {
        egui::Id::new(("error_list_row", index))
    }

    /// Ошибки, предупреждения и сообщения вкладки Code: (ошибок, предупреждений, сообщений).
    pub fn code_problem_counts(&self) -> (usize, usize, usize) {
        let c = Counts::of(entries(&self.code).iter().map(|e| &e.diagnostic));
        (c.errors, c.warnings, c.infos)
    }

    /// Текст строк, которые сейчас показывает Error List (как «Copy»).
    pub fn error_list_rows(&self) -> Vec<String> {
        visible(self).iter().map(entry_text).collect()
    }
}

/// Строки после фильтров.
fn visible(editor: &Editor) -> Vec<Entry> {
    let state = &editor.code.errors;
    let active = editor.code.documents.get(editor.code.active).map(|d| d.path.clone());
    let needle = state.filter.trim().to_lowercase();
    entries(&editor.code)
        .into_iter()
        .filter(|e| match e.diagnostic.severity {
            Severity::Error => state.show_errors,
            Severity::Warning => state.show_warnings,
            Severity::Info => state.show_infos,
        })
        .filter(|e| match state.scope {
            Scope::CurrentFile => active.as_deref() == Some(e.path.as_path()),
            Scope::OpenFiles => e.open,
            Scope::Project => true,
        })
        .filter(|e| {
            needle.is_empty()
                || e.diagnostic.message.to_lowercase().contains(&needle)
                || e.diagnostic.code.as_deref().is_some_and(|c| c.to_lowercase().contains(&needle))
                || e.path.to_string_lossy().to_lowercase().contains(&needle)
        })
        .collect()
}

fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::CurrentFile => tr_s("Current File"),
        Scope::OpenFiles => tr_s("Open Files"),
        Scope::Project => tr_s("Project"),
    }
}

/// Перейти к месту диагностики: открыть файл и выделить диапазон.
pub(crate) fn go_to(editor: &mut Editor, entry: &Entry) {
    editor.code.reveal = Some((entry.path.clone(), entry.diagnostic.range.clone()));
    editor.actions.push(crate::state::Action::OpenCode(entry.path.clone()));
}

/// F8 / Shift+F8: следующая / предыдущая ошибка после курсора (ошибки — раньше
/// предупреждений), по кругу через все файлы.
pub(crate) fn step(editor: &mut Editor, forward: bool) {
    let list = entries(&editor.code);
    let here = editor.code.documents.get(editor.code.active).map(|doc| {
        let index = crate::code_diagnostics::LineIndex::new(&doc.text);
        let (line, column) = editor.code.cursor;
        (doc.path.clone(), index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1)))
    });
    let current = here.as_ref().map(|(p, o)| (p.as_path(), *o));
    if let Some(entry) = crate::code_checks::next_entry(&list, current, forward, true) {
        editor.code.errors.selected = Some((entry.path.clone(), entry.diagnostic.range.start, entry.diagnostic.message.clone()));
        go_to(editor, &entry);
    }
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    let all = entries(&editor.code);
    let counts = Counts::of(all.iter().map(|e| &e.diagnostic));
    let checking = editor.code.checks.compiler.is_running();
    let mut check_now = false;
    {
        let state = &mut editor.code.errors;
        theme::toolbar_strip(ui, |ui| {
            crate::controls::combo(ui, "error_list_scope", scope_label(state.scope), 120.0, |ui| { // i18n-skip: имя типа / ключ
                for scope in [Scope::CurrentFile, Scope::OpenFiles, Scope::Project] {
                    ui.selectable_value(&mut state.scope, scope, scope_label(scope));
                }
            });
            theme::toolbar_separator(ui);
            let label = if checking { tr_s("Checking…") } else { tr_s("Check Now") };
            let tip = trf!("Run `cargo check` for the project now ({keys})", keys = crate::shortcuts::label(ui.ctx(), "check-code"));
            if ui.add_enabled_ui(!checking, |ui| theme::icon_text_button(ui, Icon::Refresh, label)).inner.on_hover_text(tip).clicked() {
                check_now = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme::SP_1;
                theme::pill_toggle(ui, &mut state.show_infos, Icon::Info, theme::INFO, &counts.infos.to_string(), tr_s("Show messages"));
                theme::pill_toggle(ui, &mut state.show_warnings, Icon::Warn, theme::WARN, &counts.warnings.to_string(), tr_s("Show warnings"));
                theme::pill_toggle(ui, &mut state.show_errors, Icon::Error, theme::ERROR, &counts.errors.to_string(), tr_s("Show errors"));
                ui.add_space(theme::SP_S);
                theme::search_field(ui, &mut state.filter, tr_s("Search Error List"), 220.0);
            });
        });
    }
    if check_now {
        let now = ui.input(|i| i.time);
        crate::code_checks::request_compiler(editor, None, now);
    }

    let rows = visible(editor);
    if rows.is_empty() {
        let (title, hint) = if all.is_empty() {
            (tr_s("No problems"), tr_s("Syntax errors appear here while you type in the Code tab; compiler errors after you save (cargo check) or press Check Now"))
        } else {
            (tr_s("Nothing matches"), tr_s("Change the search text, the scope or turn the severity filters back on"))
        };
        theme::empty_state(ui, Icon::Check, title, hint);
        return;
    }

    // Заголовок столбцов.
    let width = ui.available_width();
    let columns = Columns::new(width);
    let (header, _) = ui.allocate_exact_size(vec2(width, theme::ROW), Sense::hover());
    ui.painter().rect_filled(header, 0.0, theme::BG_HEADER);
    for (x, title) in [(columns.code, tr_s("Code")), (columns.message, tr_s("Description")), (columns.file, tr_s("File")), (columns.line, crate::i18n::tr_ctx_s("error list", "Line"))] {
        ui.painter().text(pos2(header.left() + x, header.center().y), Align2::LEFT_CENTER, title, theme::strong(theme::FONT_CAPTION), theme::TEXT_MUTED);
    }

    let selected_index = editor
        .code
        .errors
        .selected
        .as_ref()
        .and_then(|(path, start, message)| rows.iter().position(|e| &e.path == path && e.diagnostic.range.start == *start && &e.diagnostic.message == message));
    let mut select = None;
    let mut open = None;
    egui::ScrollArea::vertical().id_salt("error_list_scroll").auto_shrink([false, false]).show_rows(ui, theme::ROW, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in range {
            let entry = &rows[index];
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
            let response = ui.interact(rect, Editor::error_list_row_id(index), Sense::click());
            let selected = selected_index == Some(index);
            theme::list_row(ui.painter(), rect, selected, true, response.hovered());
            let d = &entry.diagnostic;
            let mut color = severity_color(d.severity);
            if d.stale {
                color = color.gamma_multiply(0.5);
            }
            let painter = ui.painter_at(rect);
            icons::paint(&painter, Rect::from_center_size(pos2(rect.left() + theme::SP_6, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)), severity_icon(d.severity), color);
            let text_color = if d.stale { theme::TEXT_MUTED } else { theme::TEXT };
            let clip = |from: f32, to: f32| painter.with_clip_rect(Rect::from_min_max(pos2(rect.left() + from, rect.top()), pos2(rect.left() + to - theme::SP_2, rect.bottom())));
            clip(columns.code, columns.message).text(pos2(rect.left() + columns.code, rect.center().y), Align2::LEFT_CENTER, d.code.as_deref().unwrap_or(""), theme::mono(), theme::TEXT_MUTED);
            clip(columns.message, columns.file).text(pos2(rect.left() + columns.message, rect.center().y), Align2::LEFT_CENTER, d.message.lines().next().unwrap_or(""), theme::body(), text_color);
            let file = entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            clip(columns.file, columns.line).text(pos2(rect.left() + columns.file, rect.center().y), Align2::LEFT_CENTER, file, theme::body(), theme::TEXT_MUTED);
            clip(columns.line, width).text(pos2(rect.left() + columns.line, rect.center().y), Align2::LEFT_CENTER, d.line.to_string(), theme::mono(), theme::TEXT_MUTED);
            let tip = crate::code_checks::tooltip_text(d, &crate::shortcuts::label(ui.ctx(), "quick-fix"));
            let response = response.on_hover_text(format!("{}\n{}", entry.path.display(), tip));
            if response.clicked() {
                select = Some(index);
            }
            if response.double_clicked() {
                open = Some(index);
            }
            response.context_menu(|ui| {
                theme::menu_body(ui);
                if theme::item(tr_s("Go to Location")).icon(Icon::External).show(ui) {
                    open = Some(index);
                    ui.close_menu();
                }
                if theme::item(tr_s("Copy")).icon(Icon::Copy).show(ui) {
                    ui.ctx().copy_text(entry_text(entry));
                    ui.close_menu();
                }
            });
        }
    });
    // Клавиатура: стрелки, Enter, Ctrl+C — когда указатель над окном.
    let hovered = ui.rect_contains_pointer(ui.max_rect()) && !ui.ctx().wants_keyboard_input();
    let mut current = select.or(selected_index);
    if hovered {
        let (up, down, enter, copy) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
            )
        });
        if up {
            current = Some(current.map_or(0, |c| c.saturating_sub(1)));
        }
        if down {
            current = Some(current.map_or(0, |c| (c + 1).min(rows.len() - 1)));
        }
        if enter {
            open = current;
        }
        if copy {
            if let Some(index) = current {
                ui.ctx().copy_text(entry_text(&rows[index]));
            }
        }
    }
    if let Some(index) = current {
        let e = &rows[index];
        editor.code.errors.selected = Some((e.path.clone(), e.diagnostic.range.start, e.diagnostic.message.clone()));
    }
    if let Some(index) = open {
        let entry = rows[index].clone();
        go_to(editor, &entry);
    }
}

/// Где начинаются столбцы (от левого края строки).
struct Columns {
    code: f32,
    message: f32,
    file: f32,
    line: f32,
}

impl Columns {
    fn new(width: f32) -> Self {
        let line = (width - 56.0).max(300.0);
        let file = (line - 170.0).max(240.0);
        Self { code: theme::SP_6 * 2.0, message: theme::SP_6 * 2.0 + 120.0, file, line }
    }
}
