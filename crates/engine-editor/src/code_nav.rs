//! Навигация вкладки Code (как в Visual Studio): Go to File (Ctrl+P), Go to Symbol в
//! файле (Ctrl+Shift+O) и в проекте (Ctrl+T) — нечёткий поиск; Go to Definition (F12,
//! Ctrl+щелчок) и Peek Definition (Alt+F12) по индексу символов проекта и API движка;
//! Find All References (Shift+F12, по тексту); назад / вперёд (Ctrl+− / Ctrl+Shift+−);
//! переключение вкладок в порядке недавнего использования (Ctrl+Tab), открыть закрытую
//! (Ctrl+Shift+T).

use crate::i18n::{tr_s, trf};
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Key, Modifiers, Rect, Sense};

use crate::code_symbols::{self, Kind, Symbol};
use crate::state::{Action, Editor};
use crate::theme;

/// Что ищет окно быстрого перехода.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickerKind {
    Files,
    FileSymbols,
    ProjectSymbols,
    /// Несколько определений одного имени (F12).
    Definitions,
}

/// Строка списка перехода.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Target {
    pub label: String,
    pub detail: String,
    pub path: PathBuf,
    /// Байт в файле (для символов) или 0.
    pub offset: usize,
    pub line: usize,
    pub kind: Option<Kind>,
}

/// Окно быстрого перехода.
pub(crate) struct Picker {
    pub kind: PickerKind,
    pub query: String,
    pub selected: usize,
    pub focus: bool,
    pub items: Vec<Target>,
}

/// Подсмотренное определение (Alt+F12).
pub(crate) struct Peek {
    pub title: String,
    pub path: PathBuf,
    pub line: usize,
    pub lines: Vec<String>,
}

/// Состояние навигации.
#[derive(Default)]
pub(crate) struct NavState {
    pub picker: Option<Picker>,
    pub peek: Option<Peek>,
    /// История мест (файл, байт) для Ctrl+− / Ctrl+Shift+−.
    pub back: Vec<(PathBuf, usize)>,
    pub forward: Vec<(PathBuf, usize)>,
    /// Порядок недавнего использования вкладок (новые первыми).
    pub mru: Vec<PathBuf>,
    /// Закрытые вкладки (последняя — в конце) для Ctrl+Shift+T.
    pub closed: Vec<PathBuf>,
}

/// Нечёткое совпадение (буквы запроса по порядку): очки и позиции. Больше очков — у
/// подряд идущих букв, начала слова и совпадения с именем файла.
pub(crate) fn fuzzy(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let c: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = candidate.to_lowercase().chars().collect();
    if lower.len() != c.len() {
        return candidate.to_lowercase().contains(&query.to_lowercase()).then_some(1);
    }
    let mut score = 0;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    let name_start = candidate.rfind(['/', '\\']).map_or(0, |p| candidate[..p + 1].chars().count());
    for (i, ch) in lower.iter().enumerate() {
        if qi < q.len() && *ch == q[qi] {
            score += 1;
            if last.is_some_and(|l| l + 1 == i) {
                score += 4;
            }
            let boundary = i == 0 || !c[i - 1].is_alphanumeric() || (c[i].is_uppercase() && c[i - 1].is_lowercase());
            if boundary {
                score += 3;
            }
            if i >= name_start {
                score += 2;
            }
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then(|| score * 100 - c.len() as i32)
}

fn kind_text(kind: Option<Kind>) -> &'static str {
    kind.map_or("", Kind::label)
}

impl Editor {
    /// Все символы проекта (файлы каталогов кода; открытые — по их текущему тексту).
    pub(crate) fn project_symbols(&self) -> Vec<Symbol> {
        let mut out = Vec::new();
        for (_, path) in &self.code.files {
            let text = match self.code.documents.iter().find(|d| &d.path == path) {
                Some(doc) => doc.text.clone(),
                None => match std::fs::read_to_string(path) {
                    Ok(text) if text.len() < 2_000_000 => text.replace("\r\n", "\n"),
                    _ => continue,
                },
            };
            out.extend(code_symbols::symbols(path, &text));
        }
        out
    }

    /// Открыть окно быстрого перехода.
    pub(crate) fn open_picker(&mut self, kind: PickerKind) {
        let items: Vec<Target> = match kind {
            PickerKind::Files => self
                .code
                .files
                .iter()
                .map(|(shown, path)| Target { label: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), detail: shown.clone(), path: path.clone(), offset: 0, line: 1, kind: None })
                .collect(),
            PickerKind::FileSymbols => match self.code.documents.get(self.code.active) {
                Some(doc) => code_symbols::symbols(&doc.path, &doc.text).into_iter().map(symbol_target).collect(),
                None => Vec::new(),
            },
            PickerKind::ProjectSymbols => self.project_symbols().into_iter().map(symbol_target).collect(),
            PickerKind::Definitions => Vec::new(),
        };
        self.code.nav.picker = Some(Picker { kind, query: String::new(), selected: 0, focus: true, items });
    }

    /// Запомнить текущее место перед переходом (для Ctrl+−).
    pub(crate) fn remember_place(&mut self) {
        if let Some(doc) = self.code.documents.get(self.code.active) {
            let index = crate::code_diagnostics::LineIndex::new(&doc.text);
            let (line, column) = self.code.cursor;
            let at = index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1));
            let place = (doc.path.clone(), at);
            if self.code.nav.back.last() != Some(&place) {
                self.code.nav.back.push(place);
                if self.code.nav.back.len() > 100 {
                    self.code.nav.back.remove(0);
                }
            }
            self.code.nav.forward.clear();
        }
    }

    /// Перейти к байту `offset` файла `path` (открыть его во вкладке).
    pub(crate) fn go_to_place(&mut self, path: &Path, offset: usize, remember: bool) {
        if remember {
            self.remember_place();
        }
        self.code.reveal = Some((path.to_path_buf(), offset..offset));
        self.actions.push(Action::OpenCode(path.to_path_buf()));
    }

    /// Ctrl+− / Ctrl+Shift+−.
    pub(crate) fn navigate_history(&mut self, back: bool) {
        let current = self.code.documents.get(self.code.active).map(|doc| {
            let index = crate::code_diagnostics::LineIndex::new(&doc.text);
            let (line, column) = self.code.cursor;
            (doc.path.clone(), index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1)))
        });
        let (from, to) = if back { (&mut self.code.nav.back, &mut self.code.nav.forward) } else { (&mut self.code.nav.forward, &mut self.code.nav.back) };
        let Some(place) = from.pop() else { return };
        if let Some(current) = current {
            to.push(current);
        }
        self.go_to_place(&place.0.clone(), place.1, false);
    }

    /// F12 / Alt+F12: определение слова под курсором.
    pub(crate) fn go_to_definition(&mut self, peek: bool) {
        let Some(doc) = self.code.documents.get(self.code.active) else { return };
        let index = crate::code_diagnostics::LineIndex::new(&doc.text);
        let (line, column) = self.code.cursor;
        let at = index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1));
        let Some(range) = code_symbols::ident_at(&doc.text, at).or_else(|| code_symbols::ident_at(&doc.text, at.saturating_sub(1))) else { return };
        let name = doc.text[range].to_string();
        let file = code_symbols::symbols(&doc.path, &doc.text);
        let project = self.project_symbols();
        let found: Vec<Symbol> = code_symbols::definitions(&name, &file, &project).into_iter().cloned().collect();
        if !found.is_empty() {
            if peek {
                self.peek_symbol(&found[0]);
            } else if found.len() == 1 {
                self.go_to_place(&found[0].path.clone(), found[0].range.start, true);
            } else {
                let items = found.into_iter().map(symbol_target).collect();
                self.code.nav.picker = Some(Picker { kind: PickerKind::Definitions, query: String::new(), selected: 0, focus: true, items });
            }
            return;
        }
        // API движка: исходник в рабочем пространстве движка или подсказка с документацией.
        if let Some(symbol) = code_symbols::engine_definitions(&name).first().copied() {
            match code_symbols::engine_source_path(&symbol.path) {
                Some(path) if !peek => {
                    log::info!("{}: {} — {}", symbol.path.display(), symbol.signature, symbol.doc); // i18n-skip: журнал
                    self.go_to_place(&path, symbol.range.start, true);
                }
                _ => self.peek_symbol(symbol),
            }
            return;
        }
        log::info!("no definition of `{name}` in the project or the engine API index"); // i18n-skip: журнал
    }

    fn peek_symbol(&mut self, symbol: &Symbol) {
        let text = code_symbols::engine_source_text(&symbol.path)
            .map(str::to_string)
            .or_else(|| self.code.documents.iter().find(|d| d.path == symbol.path).map(|d| d.text.clone()))
            .or_else(|| std::fs::read_to_string(&symbol.path).ok())
            .unwrap_or_default();
        let first = symbol.line.saturating_sub(3);
        let lines: Vec<String> = text.lines().skip(first).take(14).map(|l| l.replace('\t', "    ")).collect();
        let path = code_symbols::engine_source_path(&symbol.path).unwrap_or_else(|| symbol.path.clone());
        self.code.nav.peek = Some(Peek { title: format!("{} — {}", symbol.name, symbol.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), path, line: first + 1, lines });
    }

    /// Ctrl+Tab: предыдущая по времени вкладка.
    pub(crate) fn switch_tab_mru(&mut self) {
        let active = self.code.documents.get(self.code.active).map(|d| d.path.clone());
        let next = self.code.nav.mru.iter().find(|p| Some(*p) != active.as_ref() && self.code.documents.iter().any(|d| &d.path == *p)).cloned();
        if let Some(path) = next {
            self.actions.push(Action::OpenCode(path));
        }
    }

    /// Ctrl+Shift+T: открыть последнюю закрытую вкладку.
    pub(crate) fn reopen_closed_tab(&mut self) {
        while let Some(path) = self.code.nav.closed.pop() {
            if path.is_file() {
                self.actions.push(Action::OpenCode(path));
                return;
            }
        }
    }
}

fn symbol_target(s: Symbol) -> Target {
    let container = s.container.as_deref().map(|c| format!("{c} · ")).unwrap_or_default();
    let file = s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Target { label: s.name.clone(), detail: format!("{container}{} · {file}:{}", s.signature, s.line), path: s.path, offset: s.range.start, line: s.line, kind: Some(s.kind) } // i18n-skip: код
}

impl Editor {
    /// Строка окна быстрого перехода с номером `index` (для тестов).
    pub fn code_picker_row_id(index: usize) -> egui::Id {
        egui::Id::new(("code_picker_row", index))
    }
}

/// Окна навигации: быстрый переход и подсмотренное определение.
pub(crate) fn windows(editor: &mut Editor, ctx: &egui::Context) {
    picker_window(editor, ctx);
    peek_window(editor, ctx);
    crate::code_rename::window(editor, ctx);
}

fn picker_window(editor: &mut Editor, ctx: &egui::Context) {
    let Some(picker) = editor.code.nav.picker.as_mut() else { return };
    let title = match picker.kind {
        PickerKind::Files => tr_s("Go to File"),
        PickerKind::FileSymbols => tr_s("Go to Symbol in File"),
        PickerKind::ProjectSymbols => tr_s("Go to Symbol in Project"),
        PickerKind::Definitions => tr_s("Go to Definition"),
    };
    let hint = match picker.kind {
        PickerKind::Files => tr_s("File name (letters in order: `plmv` finds player_move.rs)"),
        _ => tr_s("Symbol name"),
    };
    // Отбор и порядок.
    let mut shown: Vec<(i32, usize)> = picker
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let key = if picker.kind == PickerKind::Files { t.detail.as_str() } else { t.label.as_str() };
            fuzzy(&picker.query, key).map(|s| (s, i))
        })
        .collect();
    shown.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    shown.truncate(200);
    let (up, down, enter) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter)));
    if up {
        picker.selected = picker.selected.saturating_sub(1);
    }
    if down {
        picker.selected = (picker.selected + 1).min(shown.len().saturating_sub(1));
    }
    picker.selected = picker.selected.min(shown.len().saturating_sub(1));
    let mut chosen = enter.then_some(picker.selected).filter(|_| !shown.is_empty());
    let response = crate::window_kit::dialog("code_picker", title).width(620.0).show(ctx, |ui| {
        let field = crate::controls::search_field(ui, &mut picker.query, hint, ui.available_width());
        if std::mem::take(&mut picker.focus) {
            field.request_focus();
        }
        if field.changed() {
            picker.selected = 0;
        }
        ui.add_space(theme::ROW_GAP);
        if shown.is_empty() {
            crate::controls::muted(ui, tr_s("Nothing matches"));
            return;
        }
        let rows = shown.len().min(14);
        egui::ScrollArea::vertical().id_salt("code_picker_list").max_height(rows as f32 * theme::ROW).auto_shrink([false, true]).show_rows(ui, theme::ROW, shown.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                let item = &picker.items[shown[row].1];
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
                let response = ui.interact(rect, Editor::code_picker_row_id(row), Sense::click());
                theme::list_row(ui.painter(), rect, row == picker.selected, true, response.hovered());
                let painter = ui.painter_at(rect);
                let mut x = rect.left() + theme::SP_4;
                if item.kind.is_some() {
                    painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, kind_text(item.kind), theme::mono(), theme::INFO);
                    x += 72.0;
                }
                let name = painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, &item.label, theme::body(), theme::TEXT_STRONG);
                painter
                    .with_clip_rect(Rect::from_min_max(pos2(name.right() + theme::SP_6, rect.top()), rect.max))
                    .text(pos2(name.right() + theme::SP_6, rect.center().y), Align2::LEFT_CENTER, &item.detail, theme::caption(), theme::TEXT_MUTED);
                if response.clicked() {
                    chosen = Some(row);
                }
                if row == picker.selected && (up || down) {
                    ui.scroll_to_rect(rect, None);
                }
            }
        });
    });
    let close = response.should_close();
    if let Some(row) = chosen {
        let target = picker.items[shown[row].1].clone();
        editor.code.nav.picker = None;
        editor.go_to_place(&target.path, target.offset, true);
    } else if close {
        editor.code.nav.picker = None;
    }
}

fn peek_window(editor: &mut Editor, ctx: &egui::Context) {
    let Some(peek) = editor.code.nav.peek.as_ref() else { return };
    let mut open = true;
    let mut go = false;
    let title = trf!("Peek Definition: {title}", title = peek.title);
    crate::window_kit::tool_window("code_peek", &title).default_size(vec2(620.0, 300.0)).show(ctx, &mut open, |ui| {
        egui::ScrollArea::both().id_salt("code_peek_text").auto_shrink([false, false]).max_height(260.0).show(ui, |ui| {
            for (i, line) in peek.lines.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{:>5} ", peek.line + i)).font(theme::mono()).color(theme::TEXT_DISABLED));
                    ui.label(egui::RichText::new(line).font(theme::mono()).color(theme::TEXT));
                });
            }
        });
        ui.add_space(theme::ROW_GAP);
        crate::controls::button_row(ui, |ui| {
            if peek.path.is_file() && crate::controls::button(ui, tr_s("Open File")).clicked() {
                go = true;
            }
        });
    });
    if go {
        let path = peek.path.clone();
        let line = peek.line + 2;
        editor.code.nav.peek = None;
        editor.remember_place();
        editor.code.goto_line(&path, line);
        editor.actions.push(Action::OpenCode(path));
    } else if !open {
        editor.code.nav.peek = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_word_starts_and_file_names() {
        assert!(fuzzy("plmv", "src/scripts/player_move.rs").is_some());
        assert!(fuzzy("xyz", "player_move.rs").is_none());
        let a = fuzzy("move", "src/scripts/player_move.rs").unwrap();
        let b = fuzzy("move", "src/remove_all/x.rs").unwrap();
        assert!(a > b, "{a} {b}");
        assert!(fuzzy("pm", "PlayerMove").unwrap() > fuzzy("pm", "upmost").unwrap());
    }
}
