//! Диагностики вкладки Code: ошибки, предупреждения и сообщения с точным местом в тексте.
//!
//! Два источника:
//! * **мгновенная проверка** при наборе (`code_check_*`): синтаксис Rust / C++ / Scenario /
//!   RON / JSON / TOML / WGSL и дешёвые проверки движка — через ~300 мс после последней
//!   правки, большие файлы — в фоновом потоке;
//! * **компилятор** после сохранения (`code_cargo.rs`): `cargo check` для Rust, вывод
//!   MSVC / GCC / Clang после сборки C++-плагина.
//!
//! Диагностика хранит байтовый диапазон в тексте буфера (переводы строк — `\n`) и строку /
//! столбец (с единицы, столбец — в символах). Когда текст правят, диапазоны сдвигаются
//! ([`shift_range`]), а диагностики компилятора помечаются устаревшими (`stale`) до
//! следующей проверки. Интерфейс «поставщика диагностик» ([`Provider`]) общий: позже его
//! может реализовать клиент rust-analyzer, ничего не меняя в показе.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Важность диагностики (порядок — по убыванию важности).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Severity {
    Error,
    Warning,
    /// Сообщение: заметка, подсказка, «так можно короче».
    Info,
}

/// Откуда диагностика.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Origin {
    /// Мгновенная проверка при наборе.
    Syntax,
    /// `cargo check` или сборка C++-плагина.
    Compiler,
}

/// Правка текста: заменить байты `range` на `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextEdit {
    pub range: Range<usize>,
    pub text: String,
}

/// Быстрое исправление (Quick Fix, Ctrl+.): одна или несколько правок — один шаг Undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuickFix {
    pub title: String,
    pub edits: Vec<TextEdit>,
}

/// Одна диагностика.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Diagnostic {
    pub severity: Severity,
    pub origin: Origin,
    /// Код ошибки: `E0425`, `C2065`, `unused_variables`.
    pub code: Option<String>,
    pub message: String,
    /// Подсказка: что сделать.
    pub hint: Option<String>,
    /// Заметки компилятора и вторичные места (`line 12: first borrow here`).
    pub notes: Vec<String>,
    /// Байты в тексте буфера; у файла, который не открыт, — по строке и столбцу.
    pub range: Range<usize>,
    /// Строка и столбец начала (с единицы, столбец — в символах).
    pub line: usize,
    pub column: usize,
    pub fixes: Vec<QuickFix>,
    /// Текст правили после проверки: место сдвинуто, но диагностика могла устареть.
    pub stale: bool,
}

impl Diagnostic {
    pub fn new(severity: Severity, origin: Origin, message: impl Into<String>, range: Range<usize>) -> Self {
        Self {
            severity,
            origin,
            code: None,
            message: message.into(),
            hint: None,
            notes: Vec::new(),
            range,
            line: 1,
            column: 1,
            fixes: Vec::new(),
            stale: false,
        }
    }

    pub fn error(message: impl Into<String>, range: Range<usize>) -> Self {
        Self::new(Severity::Error, Origin::Syntax, message, range)
    }

    pub fn warning(message: impl Into<String>, range: Range<usize>) -> Self {
        Self::new(Severity::Warning, Origin::Syntax, message, range)
    }

    pub fn info(message: impl Into<String>, range: Range<usize>) -> Self {
        Self::new(Severity::Info, Origin::Syntax, message, range)
    }

    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn fix(mut self, title: impl Into<String>, range: Range<usize>, text: impl Into<String>) -> Self {
        self.fixes.push(QuickFix { title: title.into(), edits: vec![TextEdit { range, text: text.into() }] });
        self
    }

    /// Пересчитать строку и столбец по тексту.
    pub fn locate(&mut self, index: &LineIndex, text: &str) {
        let (line, column) = index.line_col(text, self.range.start);
        self.line = line + 1;
        self.column = column + 1;
    }
}

/// Начала строк текста — перевод «байт ↔ строка, столбец» без прохода по всему тексту.
#[derive(Clone, Debug, Default)]
pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    /// Байт начала строки `line` (с нуля).
    pub fn line_start(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or_else(|| *self.starts.last().unwrap_or(&0))
    }

    /// Байт конца строки `line` без перевода строки.
    pub fn line_end(&self, text: &str, line: usize) -> usize {
        match self.starts.get(line + 1) {
            Some(next) => next - 1,
            None => text.len(),
        }
    }

    /// Строка (с нуля) байта `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        }
    }

    /// Строка и столбец (с нуля, столбец — в символах) байта `offset`.
    pub fn line_col(&self, text: &str, offset: usize) -> (usize, usize) {
        let offset = floor_char(text, offset.min(text.len()));
        let line = self.line_of(offset);
        let start = self.line_start(line).min(offset);
        (line, text[start..offset].chars().count())
    }

    /// Байт по строке и столбцу (с нуля, столбец — в символах); за концом строки — её конец.
    pub fn offset(&self, text: &str, line: usize, column: usize) -> usize {
        if line >= self.starts.len() {
            return text.len();
        }
        let start = self.starts[line];
        let end = self.line_end(text, line);
        text[start..end].char_indices().nth(column).map_or(end, |(i, _)| start + i)
    }
}

/// Ближайшая граница символа не правее `offset`.
pub(crate) fn floor_char(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Ближайшая граница символа не левее `offset`.
pub(crate) fn ceil_char(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

/// Что поменялось между двумя версиями текста: общий префикс и суффикс.
/// `(начало, конец в старом, конец в новом)` — заменённый кусок.
pub(crate) fn text_change(old: &str, new: &str) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    prefix = floor_char(old, prefix).min(floor_char(new, prefix));
    let max_suffix = a.len().min(b.len()) - prefix;
    let mut suffix = a.iter().rev().zip(b.iter().rev()).take(max_suffix).take_while(|(x, y)| x == y).count();
    while suffix > 0 && (!old.is_char_boundary(a.len() - suffix) || !new.is_char_boundary(b.len() - suffix)) {
        suffix -= 1;
    }
    Some((prefix, a.len() - suffix, b.len() - suffix))
}

/// Сдвинуть диапазон после правки `(start, old_end, new_end)`. `true` — правка задела его.
pub(crate) fn shift_range(range: &mut Range<usize>, change: (usize, usize, usize)) -> bool {
    let (start, old_end, new_end) = change;
    if range.end < start || (range.end == start && range.start < start) {
        return false;
    }
    if range.start >= old_end {
        range.start = range.start - old_end + new_end;
        range.end = range.end - old_end + new_end;
        return false;
    }
    // Пересекается с изменённым куском: сжать к нему.
    let new_start = range.start.min(start);
    let new_stop = if range.end > old_end { range.end - old_end + new_end } else { new_end.max(new_start) };
    *range = new_start..new_stop.max(new_start);
    true
}

/// Сдвинуть диагностики после правки текста. Задетые компиляторные — устаревшие, их
/// быстрые исправления больше не применимы; все компиляторные — «до проверки» (stale).
pub(crate) fn shift_all(diagnostics: &mut [Diagnostic], change: (usize, usize, usize)) {
    for diagnostic in diagnostics.iter_mut() {
        let touched = shift_range(&mut diagnostic.range, change);
        for fix in &mut diagnostic.fixes {
            for edit in &mut fix.edits {
                shift_range(&mut edit.range, change);
            }
        }
        if diagnostic.origin == Origin::Compiler {
            diagnostic.stale = true;
            if touched {
                diagnostic.fixes.clear();
            }
        }
    }
}

/// Применить правки к тексту (с конца, чтобы диапазоны не съезжали). Пересекающиеся
/// правки пропускаются. Возвращает новый текст.
pub(crate) fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<&TextEdit> = edits.iter().filter(|e| e.range.start <= e.range.end && e.range.end <= text.len()).collect();
    sorted.sort_by_key(|e| (e.range.start, e.range.end));
    let mut out = String::with_capacity(text.len() + 16);
    let mut last = 0;
    for edit in sorted {
        if edit.range.start < last || !text.is_char_boundary(edit.range.start) || !text.is_char_boundary(edit.range.end) {
            continue;
        }
        out.push_str(&text[last..edit.range.start]);
        out.push_str(&edit.text);
        last = edit.range.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Поставщик диагностик для одного файла (мгновенные проверки; позже — LSP).
pub(crate) trait Provider: Send + Sync {
    /// Проверить текст файла `path`. Вызывается из фонового потока для больших файлов.
    fn check(&self, path: &Path, text: &str) -> Vec<Diagnostic>;
}

/// Счётчики для строки состояния, заголовка вкладки и фильтров Error List.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

impl Counts {
    pub fn add(&mut self, severity: Severity) {
        match severity {
            Severity::Error => self.errors += 1,
            Severity::Warning => self.warnings += 1,
            Severity::Info => self.infos += 1,
        }
    }

    pub fn of<'a>(diagnostics: impl IntoIterator<Item = &'a Diagnostic>) -> Self {
        let mut counts = Self::default();
        for d in diagnostics {
            counts.add(d.severity);
        }
        counts
    }
}

/// Диагностики компилятора по файлам проекта (в том числе не открытым).
#[derive(Default)]
pub(crate) struct CompilerDiagnostics {
    pub by_file: BTreeMap<PathBuf, Vec<Diagnostic>>,
}

impl CompilerDiagnostics {
    pub fn total(&self) -> Counts {
        Counts::of(self.by_file.values().flatten())
    }
}

/// Сравнить пути без учёта регистра и разделителей (Windows): компилятор пишет
/// `src\\scripts\\a.rs`, редактор — `src/scripts/a.rs`.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let norm = |p: &Path| {
        let text = p.to_string_lossy().replace('\\', "/");
        let text = text.trim_start_matches("//?/").to_string();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    norm(a) == norm(b)
}

/// Ближайшее по написанию имя из `candidates` («did you mean»), если оно достаточно похоже.
pub(crate) fn did_you_mean<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).max(1);
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        if candidate == name {
            continue;
        }
        let d = edit_distance(&name.to_lowercase(), &candidate.to_lowercase());
        if d <= limit && best.is_none_or(|(bd, bn)| d < bd || (d == bd && candidate < bn)) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, n)| n)
}

/// Расстояние Левенштейна (по символам).
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        current[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            current[j] = (previous[j] + 1).min(current[j - 1] + 1).min(previous[j - 1] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// Диапазон «слова» под байтом `offset` (буквы, цифры, `_`); пусто — один символ.
pub(crate) fn word_at(text: &str, offset: usize) -> Range<usize> {
    let offset = floor_char(text, offset);
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = offset;
    while let Some(c) = text[..start].chars().next_back().filter(|c| is_word(*c)) {
        start -= c.len_utf8();
    }
    let mut end = offset;
    while let Some(c) = text[end..].chars().next().filter(|c| is_word(*c)) {
        end += c.len_utf8();
    }
    if start == end {
        let next = text[offset..].chars().next().filter(|c| *c != '\n').map_or(0, char::len_utf8);
        return offset..offset + next;
    }
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_maps_both_ways() {
        let text = "ab\nпривет\n\nx";
        let index = LineIndex::new(text);
        assert_eq!(index.lines(), 4);
        assert_eq!(index.line_col(text, 0), (0, 0));
        let p = text.find('в').unwrap();
        assert_eq!(index.line_col(text, p), (1, 3));
        assert_eq!(index.offset(text, 1, 3), p);
        assert_eq!(index.offset(text, 1, 99), text.find("\n\n").unwrap());
        assert_eq!(index.offset(text, 3, 0), text.len() - 1);
        assert_eq!(index.offset(text, 9, 0), text.len());
    }

    #[test]
    fn ranges_follow_edits() {
        // Вставка до диапазона сдвигает его, после — не трогает, внутри — растягивает.
        let mut r = 10..14;
        assert!(!shift_range(&mut r, (2, 2, 5)) && r == (13..17));
        let mut r = 10..14;
        assert!(!shift_range(&mut r, (20, 20, 25)) && r == (10..14));
        let mut r = 10..14;
        assert!(shift_range(&mut r, (11, 12, 12)));
        assert_eq!(r, 10..13);
        let change = text_change("let x = 1;", "let xy = 1;").unwrap();
        assert_eq!(change, (5, 5, 6));
        assert_eq!(text_change("ab", "ab"), None);
        assert_eq!(text_change("aжb", "aзb"), Some((1, 3, 3)));
    }

    #[test]
    fn compiler_diagnostics_become_stale_and_lose_touched_fixes() {
        let mut d = Diagnostic::new(Severity::Error, Origin::Compiler, "x", 4..5).fix("t", 4..5, "y");
        let mut list = vec![d.clone()];
        shift_all(&mut list, (0, 0, 2));
        assert!(list[0].stale && list[0].range == (6..7) && list[0].fixes[0].edits[0].range == (6..7));
        d.range = 4..5;
        let mut list = vec![d];
        shift_all(&mut list, (4, 5, 6));
        assert!(list[0].fixes.is_empty());
    }

    #[test]
    fn edits_apply_from_the_end() {
        let text = "a b c";
        let edits = [TextEdit { range: 4..5, text: "C".into() }, TextEdit { range: 0..1, text: "AA".into() }];
        assert_eq!(apply_edits(text, &edits), "AA b C");
    }

    #[test]
    fn suggestions_and_words() {
        assert_eq!(did_you_mean("Rigidbdy", ["Rigidbody", "Camera"]), Some("Rigidbody"));
        assert_eq!(did_you_mean("zzz", ["Rigidbody"]), None);
        assert_eq!(word_at("let speed = 1", 6), 4..9);
        assert_eq!(word_at("a + b", 2), 2..3);
        assert!(same_path(Path::new("C:\\A\\b.rs"), Path::new("c:/a/b.rs")) || !cfg!(windows));
    }
}
