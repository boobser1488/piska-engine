//! Rename (F2, Ctrl+R Ctrl+R) во вкладке Code: имя под курсором — во всём файле как целое
//! слово вне комментариев и строк; для символов из индекса проекта — и в других файлах
//! проекта (по тексту, со списком файлов для подтверждения). Открытые файлы — одним шагом
//! Undo каждый, закрытые записываются на диск.

use crate::i18n::{tr_s, trf, trn};
use std::path::PathBuf;

use crate::code_symbols;
use crate::state::Editor;
use crate::theme;

/// Открытый диалог переименования.
pub(crate) struct Rename {
    pub old: String,
    pub new: String,
    pub path: PathBuf,
    /// Другие файлы проекта с этим именем: (путь, число вхождений).
    pub others: Vec<(PathBuf, usize)>,
    pub in_project: bool,
    /// Вхождений в текущем файле.
    pub here: usize,
    pub focus: bool,
}

fn valid_identifier(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn replace_words(text: &str, ranges: &[std::ops::Range<usize>], with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for r in ranges {
        out.push_str(&text[last..r.start]);
        out.push_str(with);
        last = r.end;
    }
    out.push_str(&text[last..]);
    out
}

impl Editor {
    /// F2: начать переименование слова под курсором.
    pub(crate) fn begin_code_rename(&mut self) {
        let Some(doc) = self.code.documents.get(self.code.active) else { return };
        let index = crate::code_diagnostics::LineIndex::new(&doc.text);
        let (line, column) = self.code.cursor;
        let at = index.offset(&doc.text, line.saturating_sub(1), column.saturating_sub(1));
        let Some(range) = code_symbols::ident_at(&doc.text, at).or_else(|| code_symbols::ident_at(&doc.text, at.saturating_sub(1))) else { return };
        let name = doc.text[range].to_string();
        let here = code_symbols::references(&doc.text, &name, doc.language).len();
        let path = doc.path.clone();
        // В проекте — только если имя объявлено где-то в проекте (символ индекса).
        let project = self.project_symbols();
        let declared = project.iter().any(|s| s.name == name);
        let mut others = Vec::new();
        if declared {
            for (_, other) in &self.code.files {
                if *other == path {
                    continue;
                }
                let text = match self.code.documents.iter().find(|d| &d.path == other) {
                    Some(d) => d.text.clone(),
                    None => std::fs::read_to_string(other).map(|t| t.replace("\r\n", "\n")).unwrap_or_default(),
                };
                let n = code_symbols::references(&text, &name, crate::code::Language::of(other)).len();
                if n > 0 {
                    others.push((other.clone(), n));
                }
            }
        }
        self.code.rename = Some(Rename { new: name.clone(), old: name, path, others, in_project: false, here, focus: true });
    }

    /// Выполнить переименование. Возвращает число заменённых мест.
    pub(crate) fn apply_code_rename(&mut self, ctx: &egui::Context) -> Result<usize, String> {
        let Some(rename) = self.code.rename.take() else { return Ok(0) };
        if !valid_identifier(&rename.new) {
            return Err(trf!("`{name}` is not a valid name", name = rename.new));
        }
        let mut paths = vec![rename.path.clone()];
        if rename.in_project {
            paths.extend(rename.others.iter().map(|(p, _)| p.clone()));
        }
        let mut count = 0;
        for path in paths {
            if let Some(doc) = self.code.documents.iter_mut().find(|d| d.path == path) {
                let refs = code_symbols::references(&doc.text, &rename.old, doc.language);
                count += refs.len();
                let text = replace_words(&doc.text, &refs, &rename.new);
                let id = doc.text_id();
                let caret = refs.first().map_or(0, |r| r.start);
                crate::code::replace_text_undoable(ctx, id, doc, text, caret..caret + rename.new.len());
            } else {
                let (text, format) = crate::code_files::read(&path)?;
                let refs = code_symbols::references(&text, &rename.old, crate::code::Language::of(&path));
                count += refs.len();
                let text = replace_words(&text, &refs, &rename.new);
                std::fs::write(&path, crate::code_files::encode(&text, &format, false, false)).map_err(|e| trf!("cannot save `{p0}`: {e}", p0 = path.display(), e = e))?;
            }
        }
        Ok(count)
    }
}

/// Диалог переименования.
pub(crate) fn window(editor: &mut Editor, ctx: &egui::Context) {
    let Some(rename) = editor.code.rename.as_mut() else { return };
    let mut apply = false;
    let mut cancel = false;
    let title = trf!("Rename `{name}`", name = rename.old);
    let response = crate::window_kit::dialog("code_rename", &title).width(460.0).show(ctx, |ui| {
        let field = crate::controls::text_field(ui, &mut rename.new, tr_s("New name"), ui.available_width());
        if std::mem::take(&mut rename.focus) {
            field.request_focus();
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            apply = true;
        }
        ui.add_space(theme::ROW_GAP);
        crate::controls::muted(ui, &trn!("{n} place in this file (whole word, outside comments and strings)", "{n} places in this file (whole word, outside comments and strings)", rename.here));
        if !rename.others.is_empty() {
            let total: usize = rename.others.iter().map(|(_, n)| n).sum();
            crate::controls::checkbox(ui, &mut rename.in_project, &trf!("Also rename in other project files: {places}", places = trn!("{n} place in", "{n} places in", total) + " " + &trn!("{n} file", "{n} files", rename.others.len())));
            if rename.in_project {
                for (path, n) in rename.others.iter().take(12) {
                    crate::controls::muted(ui, &format!("{} · {n}", path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
                }
                crate::controls::muted(ui, tr_s("Text-based: the same word naming something else is renamed too. Closed files are written to disk at once."));
            }
        }
        let valid = valid_identifier(&rename.new) && rename.new != rename.old;
        crate::controls::dialog_actions(ui, |ui| {
            if ui.add_enabled_ui(valid, |ui| crate::controls::primary_action(ui, tr_s("Rename"))).inner.clicked() {
                apply = true;
            }
            if crate::controls::action_button(ui, tr_s("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if apply && editor.code.rename.as_ref().is_some_and(|r| valid_identifier(&r.new) && r.new != r.old) {
        if let Err(e) = editor.apply_code_rename(ctx) {
            log::error!("{e}");
        }
    } else if cancel || response.should_close() {
        editor.code.rename = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_word_replacement() {
        assert!(valid_identifier("speed_2") && !valid_identifier("2x") && !valid_identifier("a-b") && !valid_identifier(""));
        let text = "let speed = 1; // speed\nspeed += 1;";
        let refs = code_symbols::references(text, "speed", crate::code::Language::Rust);
        assert_eq!(replace_words(text, &refs, "pace"), "let pace = 1; // speed\npace += 1;");
    }
}
