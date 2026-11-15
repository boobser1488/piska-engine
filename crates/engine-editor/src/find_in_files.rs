//! Окно Find Results: Find in Files (Ctrl+Shift+F) по файлам кода и ассетам проекта и
//! Find All References (Shift+F12). Результаты — по файлам; щелчок открывает место.
//! Replace in Files: замена во всех найденных местах после подтверждения; в открытых
//! файлах — одним шагом Undo на файл, закрытые файлы записываются на диск.

use crate::i18n::{tr_s, trf, trn};
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Rect, Sense};

use crate::code_find::{self, FindOptions};
use crate::state::{Action, Editor};
use crate::theme;

/// Найденное место.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hit {
    pub range: std::ops::Range<usize>,
    pub line: usize,
    /// Текст строки (обрезанный) и где в нём совпадение (байты).
    pub preview: String,
    pub mark: std::ops::Range<usize>,
}

/// Результаты по одному файлу.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileHits {
    pub path: PathBuf,
    pub shown: String,
    pub hits: Vec<Hit>,
}

/// Состояние окна.
#[derive(Default)]
pub(crate) struct FindInFiles {
    pub query: String,
    pub replacement: String,
    pub options: FindOptions,
    /// Какие файлы: `*.rs; *.scenario` (пусто — все файлы кода и текстовые ассеты).
    pub filter: String,
    pub results: Vec<FileHits>,
    /// Что показано: «Find "speed"» или «References to speed (text search)».
    pub title: String,
    pub error: Option<String>,
    pub focus: bool,
    pub confirm_replace: bool,
    pub collapsed: std::collections::BTreeSet<PathBuf>,
}

fn filter_matches(filter: &str, path: &std::path::Path) -> bool {
    let patterns: Vec<&str> = filter.split([';', ',', ' ']).map(str::trim).filter(|p| !p.is_empty()).collect();
    if patterns.is_empty() {
        return true;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    patterns.iter().any(|p| {
        let p = p.to_lowercase();
        match p.strip_prefix("*.") {
            Some(ext) => name.ends_with(&format!(".{ext}")),
            None => name.contains(&p),
        }
    })
}

fn preview(text: &str, index: &crate::code_diagnostics::LineIndex, range: &std::ops::Range<usize>) -> (usize, String, std::ops::Range<usize>) {
    let start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[range.start..].find('\n').map_or(text.len(), |i| range.start + i);
    let line = index.line_of(range.start) + 1;
    let raw = &text[start..end];
    let lead = raw.len() - raw.trim_start().len();
    let shown = raw.trim_start();
    let mark = (range.start - start).saturating_sub(lead)..(range.end.min(end) - start).saturating_sub(lead);
    // Длинные строки — окно вокруг совпадения.
    if shown.len() > 200 {
        let from = crate::code_diagnostics::floor_char(shown, mark.start.saturating_sub(60));
        let to = crate::code_diagnostics::floor_char(shown, (mark.end + 100).min(shown.len()));
        return (line, format!("…{}", &shown[from..to]), mark.start - from + "…".len()..mark.end - from + "…".len());
    }
    (line, shown.to_string(), mark)
}

impl Editor {
    /// Тексты файлов поиска: открытые документы — их текущий текст.
    fn search_corpus(&self, filter: &str) -> Vec<(PathBuf, String, String)> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (shown, path) in &self.code.files {
            if !filter_matches(filter, path) || !seen.insert(path.clone()) {
                continue;
            }
            let text = match self.code.documents.iter().find(|d| &d.path == path) {
                Some(doc) => doc.text.clone(),
                None => match std::fs::read_to_string(path) {
                    Ok(text) if text.len() < 4_000_000 => text.replace("\r\n", "\n"),
                    _ => continue,
                },
            };
            out.push((path.clone(), shown.clone(), text));
        }
        out
    }

    /// Найти строку во всех файлах проекта.
    pub(crate) fn find_in_files(&mut self) {
        let state = &self.code.search;
        let (query, options, filter) = (state.query.clone(), state.options, state.filter.clone());
        let mut results = Vec::new();
        let mut error = None;
        for (path, shown, text) in self.search_corpus(&filter) {
            let index = crate::code_diagnostics::LineIndex::new(&text);
            match code_find::find_all(&text, &query, options) {
                Ok(found) if !found.is_empty() => {
                    let hits = found
                        .into_iter()
                        .map(|m| {
                            let (line, preview, mark) = preview(&text, &index, &m.range);
                            Hit { range: m.range, line, preview, mark }
                        })
                        .collect();
                    results.push(FileHits { path, shown, hits });
                }
                Ok(_) => {}
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        let state = &mut self.code.search;
        state.title = trf!("Find “{query}”", query = query);
        state.results = results;
        state.error = error;
    }

    /// Shift+F12: все вхождения слова под курсором как отдельного слова вне комментариев
    /// и строк — по тексту, а не по смыслу.
    pub(crate) fn find_all_references(&mut self) {
        let Some(doc) = self.code.documents.get(self.code.active) else { return };
        let index = crate::code_diagnostics::LineIndex::new(&doc.text);
        let (line, column) = self.code.cursor;
        let at = index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1));
        let Some(range) = crate::code_symbols::ident_at(&doc.text, at).or_else(|| crate::code_symbols::ident_at(&doc.text, at.saturating_sub(1))) else { return };
        let name = doc.text[range].to_string();
        let mut results = Vec::new();
        for (path, shown, text) in self.search_corpus("") {
            let language = crate::code::Language::of(&path);
            let index = crate::code_diagnostics::LineIndex::new(&text);
            let refs = crate::code_symbols::references(&text, &name, language);
            if refs.is_empty() {
                continue;
            }
            let hits = refs
                .into_iter()
                .map(|r| {
                    let (line, preview, mark) = preview(&text, &index, &r);
                    Hit { range: r, line, preview, mark }
                })
                .collect();
            results.push(FileHits { path, shown, hits });
        }
        let search = &mut self.code.search;
        search.query = name.clone();
        search.options = FindOptions { match_case: true, whole_word: true, ..FindOptions::default() };
        search.title = trf!("References to `{name}` (text search: whole word, outside comments and strings)", name = name);
        search.results = results;
        search.error = None;
        self.actions.push(Action::Layout(crate::dock::LayoutAction::Focus(crate::dock::Panel::FindResults)));
    }

    /// Ctrl+Shift+F: окно результатов с полем поиска (слово под курсором — в поле).
    pub(crate) fn open_find_in_files(&mut self) {
        if let Some(doc) = self.code.documents.get(self.code.active) {
            let index = crate::code_diagnostics::LineIndex::new(&doc.text);
            let (line, column) = self.code.cursor;
            let at = index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1));
            if let Some(r) = crate::code_symbols::ident_at(&doc.text, at) {
                if self.code.search.query.is_empty() {
                    self.code.search.query = doc.text[r].to_string();
                }
            }
        }
        self.code.search.focus = true;
        self.actions.push(Action::Layout(crate::dock::LayoutAction::Focus(crate::dock::Panel::FindResults)));
    }

    /// Заменить во всех найденных местах. Открытые файлы — шаг Undo на файл.
    pub(crate) fn replace_in_files(&mut self, ctx: &egui::Context) -> Result<usize, String> {
        let search = &self.code.search;
        let (query, replacement, options) = (search.query.clone(), search.replacement.clone(), search.options);
        let paths: Vec<PathBuf> = search.results.iter().map(|f| f.path.clone()).collect();
        let mut count = 0;
        for path in paths {
            if let Some(doc) = self.code.documents.iter_mut().find(|d| d.path == path) {
                let found = code_find::find_all(&doc.text, &query, options)?;
                count += found.len();
                let text = code_find::replace_all(&doc.text, &found, &replacement, options);
                let id = doc.text_id();
                crate::code::replace_text_undoable(ctx, id, doc, text, 0..0);
            } else {
                let (text, format) = crate::code_files::read(&path)?;
                let found = code_find::find_all(&text, &query, options)?;
                count += found.len();
                let new_text = code_find::replace_all(&text, &found, &replacement, options);
                std::fs::write(&path, crate::code_files::encode(&new_text, &format, false, false)).map_err(|e| trf!("cannot save `{p0}`: {e}", p0 = path.display(), e = e))?;
            }
        }
        self.find_in_files();
        Ok(count)
    }

    /// Строка результата: файл `file` (номер в списке), место `hit` (для тестов).
    pub fn find_result_row_id(file: usize, hit: usize) -> egui::Id {
        egui::Id::new(("find_result_row", file, hit))
    }

    /// Сколько мест нашёл последний поиск в файлах (файлов, мест).
    pub fn find_in_files_counts(&self) -> (usize, usize) {
        (self.code.search.results.len(), self.code.search.results.iter().map(|f| f.hits.len()).sum())
    }
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    let mut run = false;
    let mut ask_replace = false;
    {
        let s = &mut editor.code.search;
        theme::toolbar_strip(ui, |ui| {
            let field = crate::controls::search_field(ui, &mut s.query, tr_s("Find in files"), 190.0);
            if std::mem::take(&mut s.focus) {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run = true;
            }
            for (label, tip, which) in [("Aa", tr_s("Match case"), 0), ("W", tr_s("Match whole word"), 1), (".*", tr_s("Use regular expressions"), 2)] { // i18n-skip: значки
                let on = [s.options.match_case, s.options.whole_word, s.options.regex][which];
                if crate::controls::toggle_button(ui, on, label).on_hover_text(tip).clicked() {
                    match which {
                        0 => s.options.match_case = !on,
                        1 => s.options.whole_word = !on,
                        _ => s.options.regex = !on,
                    }
                }
            }
            crate::controls::text_field(ui, &mut s.filter, tr_s("Files: *.rs; *.scenario"), 120.0).on_hover_text(tr_s("Which files to search: patterns separated by `;` (empty — all code and text asset files)"));
            if theme::icon_text_button(ui, crate::icons::Icon::Search, tr_s("Find All")).clicked() {
                run = true;
            }
            theme::toolbar_separator(ui);
            crate::controls::text_field(ui, &mut s.replacement, tr_s("Replace with"), 120.0);
            let any = !s.results.is_empty();
            if ui.add_enabled_ui(any, |ui| theme::button(ui, tr_s("Replace All…"))).inner.on_hover_text(tr_s("Replace every match listed below (asks first)")).clicked() {
                ask_replace = true;
            }
        });
    }
    if run && !editor.code.search.query.is_empty() {
        editor.find_in_files();
    }
    if ask_replace {
        editor.code.search.confirm_replace = true;
    }
    confirm_dialog(editor, ui.ctx());

    let s = &editor.code.search;
    if let Some(error) = &s.error {
        ui.label(egui::RichText::new(error).color(theme::ERROR));
    }
    if s.results.is_empty() {
        let hint = if s.title.is_empty() { tr_s("Type text and press Enter (Ctrl+Shift+F). Shift+F12 in the Code tab lists the references of the word under the caret") } else { tr_s("Nothing found") };
        theme::empty_state(ui, crate::icons::Icon::Search, tr_s("Find in Files"), hint);
        return;
    }
    let (files, hits) = (s.results.len(), s.results.iter().map(|f| f.hits.len()).sum::<usize>());
    theme::section_label(ui, &format!("{} · {} · {}", s.title, trn!("{n} match", "{n} matches", hits), trn!("{n} file", "{n} files", files)));
    let mut open: Option<(PathBuf, usize)> = None;
    let mut toggle: Option<PathBuf> = None;
    egui::ScrollArea::vertical().id_salt("find_results_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (fi, file) in s.results.iter().enumerate() {
            let collapsed = s.collapsed.contains(&file.path);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click());
            theme::list_row(ui.painter(), rect, false, true, response.hovered());
            crate::icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + theme::SP_6, rect.center().y), vec2(9.0, 9.0)), if collapsed { crate::icons::Icon::ChevronRight } else { crate::icons::Icon::ChevronDown }, theme::TEXT_MUTED);
            let name = ui.painter().text(pos2(rect.left() + theme::SP_6 * 2.0, rect.center().y), Align2::LEFT_CENTER, &file.shown, theme::strong(theme::FONT_BODY), theme::TEXT);
            ui.painter().text(pos2(name.right() + theme::SP_6, rect.center().y), Align2::LEFT_CENTER, file.hits.len().to_string(), theme::caption(), theme::TEXT_MUTED);
            if response.clicked() {
                toggle = Some(file.path.clone());
            }
            if collapsed {
                continue;
            }
            for (hi, hit) in file.hits.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
                let response = ui.interact(rect, Editor::find_result_row_id(fi, hi), Sense::click());
                theme::list_row(ui.painter(), rect, false, true, response.hovered());
                let painter = ui.painter_at(rect);
                let x = rect.left() + theme::SP_6 * 3.0;
                painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, format!("{:>5}", hit.line), theme::mono(), theme::TEXT_DISABLED);
                // Строка с подсвеченным совпадением.
                let mut job = egui::text::LayoutJob::default();
                let fmt = |color, background| egui::TextFormat { font_id: theme::mono(), color, background, ..Default::default() };
                let (a, b) = (hit.mark.start.min(hit.preview.len()), hit.mark.end.min(hit.preview.len()));
                let a = crate::code_diagnostics::floor_char(&hit.preview, a);
                let b = crate::code_diagnostics::floor_char(&hit.preview, b).max(a);
                job.append(&hit.preview[..a], 0.0, fmt(theme::TEXT, egui::Color32::TRANSPARENT));
                job.append(&hit.preview[a..b], 0.0, fmt(theme::TEXT_STRONG, egui::Color32::from_rgb(0x4a, 0x41, 0x22)));
                job.append(&hit.preview[b..], 0.0, fmt(theme::TEXT, egui::Color32::TRANSPARENT));
                let galley = painter.layout_job(job);
                painter.galley(pos2(x + 56.0, rect.center().y - galley.size().y * 0.5), galley, theme::TEXT);
                if response.clicked() {
                    open = Some((file.path.clone(), hit.range.start));
                }
            }
        }
    });
    if let Some(path) = toggle {
        let c = &mut editor.code.search.collapsed;
        if !c.remove(&path) {
            c.insert(path);
        }
    }
    if let Some((path, at)) = open {
        editor.go_to_place(&path, at, true);
    }
}

fn confirm_dialog(editor: &mut Editor, ctx: &egui::Context) {
    if !editor.code.search.confirm_replace {
        return;
    }
    let (files, hits) = editor.find_in_files_counts();
    let open = editor.code.search.results.iter().filter(|f| editor.code.documents.iter().any(|d| d.path == f.path)).count();
    let mut apply = false;
    let mut cancel = false;
    let response = crate::window_kit::dialog("replace_in_files", tr_s("Replace in Files")).width(440.0).show(ctx, |ui| {
        ui.label(trf!(
            "Replace {hits} with “{with}” in {files}?",
            hits = trn!("{n} match", "{n} matches", hits),
            with = editor.code.search.replacement,
            files = trn!("{n} file", "{n} files", files)
        ));
        crate::controls::muted(ui, &trf!("{open} of them are open: there each file is one Undo step. The others are written to disk at once.", open = open));
        crate::controls::dialog_actions(ui, |ui| {
            if crate::controls::primary_action(ui, tr_s("Replace")).clicked() {
                apply = true;
            }
            if crate::controls::action_button(ui, tr_s("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if apply {
        editor.code.search.confirm_replace = false;
        match editor.replace_in_files(ctx) {
            Ok(n) => log::info!("replaced {n} match(es) in files"), // i18n-skip: журнал
            Err(e) => log::error!("{e}"),
        }
    } else if cancel || response.should_close() {
        editor.code.search.confirm_replace = false;
    }
}
