//! Модель текста своего виджета кода (`code_widget.rs`): выделения с несколькими
//! курсорами, правки пачкой (по курсору — своя), история Undo / Redo шагами с
//! возвратом курсоров, индекс начал строк, который правится по изменённому куску.
//!
//! Почему текст — один `String`, а не «верёвка» или таблица кусков: всё остальное
//! вкладки Code (проверки, символы, поиск, подсказки, переименование) читает `&str`
//! целиком, а замер на файле в 20 000 строк (≈1 МБ) показал, что вставка в `String` —
//! это `memmove` за десятки микросекунд. Дорогими были раскладка всего текста и полная
//! перекраска — их и убирает виджет (раскладка и подсветка только видимых строк).

use std::ops::Range;

use crate::code_diagnostics::{ceil_char, floor_char};

/// Одно выделение: `anchor` — где начали, `caret` — где курсор (байты).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sel {
    pub anchor: usize,
    pub caret: usize,
    /// Желаемая позиция x курсора при движении вверх / вниз (как в Visual Studio:
    /// курсор возвращается в свой столбец после короткой строки).
    pub goal_x: Option<f32>,
}

impl Sel {
    pub fn one(at: usize) -> Self {
        Self { anchor: at, caret: at, goal_x: None }
    }

    pub fn new(anchor: usize, caret: usize) -> Self {
        Self { anchor, caret, goal_x: None }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.caret
    }
}

/// Все курсоры документа. Всегда хотя бы один; `primary` — главный (строка состояния,
/// прокрутка, подсказки).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Carets {
    pub sels: Vec<Sel>,
    pub primary: usize,
}

impl Default for Carets {
    fn default() -> Self {
        Self::one(0)
    }
}

impl Carets {
    pub fn one(at: usize) -> Self {
        Self { sels: vec![Sel::one(at)], primary: 0 }
    }

    pub fn single(sel: Sel) -> Self {
        Self { sels: vec![sel], primary: 0 }
    }

    pub fn primary(&self) -> Sel {
        self.sels[self.primary.min(self.sels.len() - 1)]
    }

    pub fn primary_mut(&mut self) -> &mut Sel {
        let at = self.primary.min(self.sels.len() - 1);
        &mut self.sels[at]
    }

    pub fn is_multi(&self) -> bool {
        self.sels.len() > 1
    }

    /// Оставить один главный курсор.
    pub fn collapse(&mut self) {
        let primary = self.primary();
        *self = Self::single(primary);
    }

    /// Добавить выделение и сделать его главным.
    pub fn push(&mut self, sel: Sel) {
        self.sels.push(sel);
        self.primary = self.sels.len() - 1;
        self.normalize();
    }

    /// Курсоры — в пределах текста и на границах символов; по порядку; пересекающиеся
    /// (и стоящие в одном месте) сливаются в один.
    pub fn clamp(&mut self, text: &str) {
        for sel in &mut self.sels {
            sel.anchor = floor_char(text, sel.anchor);
            sel.caret = floor_char(text, sel.caret);
        }
        self.normalize();
    }

    /// Упорядочить и слить столкнувшиеся выделения (главное остаётся главным).
    pub fn normalize(&mut self) {
        if self.sels.is_empty() {
            self.sels.push(Sel::one(0));
            self.primary = 0;
            return;
        }
        let primary = self.primary();
        let mut order: Vec<(Sel, bool)> = self.sels.iter().enumerate().map(|(i, s)| (*s, i == self.primary.min(self.sels.len() - 1))).collect();
        order.sort_by_key(|(s, _)| (s.range().start, s.range().end));
        let mut out: Vec<(Sel, bool)> = Vec::with_capacity(order.len());
        for (sel, is_primary) in order {
            if let Some((last, last_primary)) = out.last_mut() {
                let (a, b) = (last.range(), sel.range());
                let touch = b.start < a.end || b.start == a.start || (a.is_empty() && b.start == a.end) || (b.is_empty() && b.start == a.end && a.end == b.end);
                if touch {
                    // Слить: общий охват, направление — как у главного (или у первого).
                    let start = a.start.min(b.start);
                    let end = a.end.max(b.end);
                    let forward = if is_primary { sel.caret >= sel.anchor } else { last.caret >= last.anchor };
                    *last = if forward { Sel::new(start, end) } else { Sel::new(end, start) };
                    *last_primary |= is_primary;
                    continue;
                }
            }
            out.push((sel, is_primary));
        }
        self.primary = out.iter().position(|(_, p)| *p).unwrap_or(0);
        self.sels = out.into_iter().map(|(s, _)| s).collect();
        // Желаемый столбец главного переживает слияние.
        if self.sels[self.primary].caret == primary.caret {
            self.sels[self.primary].goal_x = primary.goal_x;
        }
    }

    /// Сдвинуть курсоры через правки (курсоры, которых правки не касались).
    pub fn map_through(&mut self, edits: &[Edit]) {
        for sel in &mut self.sels {
            sel.anchor = map_pos(sel.anchor, edits);
            sel.caret = map_pos(sel.caret, edits);
            sel.goal_x = None;
        }
    }
}

/// Замена куска текста.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

impl Edit {
    pub fn new(range: Range<usize>, text: impl Into<String>) -> Self {
        Self { range, text: text.into() }
    }
}

/// Позиция после правок (правки по порядку, не пересекаются, в координатах старого текста).
/// Позиция внутри заменённого куска — к его концу.
pub(crate) fn map_pos(pos: usize, edits: &[Edit]) -> usize {
    let mut delta: isize = 0;
    for e in edits {
        // Правка правее (вставка ровно в позиции — тоже: позиция остаётся перед ней).
        if e.range.start >= pos {
            break;
        }
        if e.range.end <= pos {
            delta += e.text.len() as isize - e.range.len() as isize;
            continue;
        }
        // Внутри заменённого куска — в конец вставки.
        return (e.range.start as isize + delta) as usize + e.text.len();
    }
    (pos as isize + delta).max(0) as usize
}

/// Применить правки (по порядку, без пересечений; пересекающиеся отбрасываются) и вернуть
/// обратные — в координатах нового текста, тоже по порядку.
pub(crate) fn apply(text: &mut String, edits: &[Edit]) -> Vec<Edit> {
    let mut sorted: Vec<&Edit> = edits.iter().filter(|e| e.range.start <= e.range.end && e.range.end <= text.len()).collect();
    sorted.sort_by_key(|e| (e.range.start, e.range.end));
    let mut kept: Vec<&Edit> = Vec::with_capacity(sorted.len());
    for e in sorted {
        if kept.last().is_some_and(|last| e.range.start < last.range.end) {
            continue;
        }
        if e.range.is_empty() && e.text.is_empty() {
            continue;
        }
        kept.push(e);
    }
    // Обратные правки: позиции в новом тексте.
    let mut inverse = Vec::with_capacity(kept.len());
    let mut delta: isize = 0;
    for e in &kept {
        let start = (e.range.start as isize + delta) as usize;
        inverse.push(Edit { range: start..start + e.text.len(), text: text[e.range.clone()].to_string() });
        delta += e.text.len() as isize - e.range.len() as isize;
    }
    // С конца, чтобы диапазоны не съезжали. Одна правка — сразу на месте.
    if kept.len() == 1 {
        text.replace_range(kept[0].range.clone(), &kept[0].text);
    } else if !kept.is_empty() {
        let added: usize = kept.iter().map(|e| e.text.len()).sum();
        let mut out = String::with_capacity(text.len() + added);
        let mut last = 0;
        for e in &kept {
            out.push_str(&text[last..e.range.start]);
            out.push_str(&e.text);
            last = e.range.end;
        }
        out.push_str(&text[last..]);
        *text = out;
    }
    inverse
}

/// Правка, превращающая `old` в `new` (общий префикс и суффикс), или `None`.
pub(crate) fn diff(old: &str, new: &str) -> Option<Edit> {
    let (start, old_end, new_end) = crate::code_diagnostics::text_change(old, new)?;
    Some(Edit::new(start..old_end, &new[start..new_end]))
}

/// Шаг истории: правки вперёд (в координатах текста до шага) и назад (после шага).
#[derive(Clone, Debug)]
struct Step {
    forward: Vec<Edit>,
    backward: Vec<Edit>,
}

/// Запись Undo: один или несколько шагов (набор подряд склеивается в одну запись).
#[derive(Clone, Debug)]
struct Entry {
    steps: Vec<Step>,
    before: Carets,
    after: Carets,
    typing: bool,
    last_time: f64,
}

/// Набор подряд склеивается в одну запись Undo, пока между нажатиями меньше секунды
/// (как у текстового поля egui: точка Undo — когда текст «устоялся»).
pub(crate) const TYPING_GROUP_SECONDS: f64 = 1.0;
const MAX_UNDO: usize = 1000;

/// История правок документа.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// Следующий набор начинает новую запись (курсор переставили, команда…).
    sealed: bool,
}

impl History {
    /// Записать сделанную правку. `forward` — правки в координатах текста до неё,
    /// `backward` — обратные (что вернул [`apply`]).
    pub fn record(&mut self, forward: Vec<Edit>, backward: Vec<Edit>, before: Carets, after: Carets, typing: bool, now: f64) {
        if forward.is_empty() {
            return;
        }
        self.redo.clear();
        let step = Step { forward, backward };
        if typing && !self.sealed {
            if let Some(last) = self.undo.last_mut().filter(|e| e.typing && now - e.last_time < TYPING_GROUP_SECONDS) {
                last.steps.push(step);
                last.after = after;
                last.last_time = now;
                return;
            }
        }
        self.undo.push(Entry { steps: vec![step], before, after, typing, last_time: now });
        self.sealed = !typing;
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// Следующий набор — новая запись Undo.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Отменить последнюю запись: правки текста по порядку и курсоры до неё.
    pub fn undo(&mut self, text: &mut String) -> Option<(Carets, Vec<Vec<Edit>>)> {
        let entry = self.undo.pop()?;
        let mut applied = Vec::new();
        for step in entry.steps.iter().rev() {
            apply(text, &step.backward);
            applied.push(step.backward.clone());
        }
        let carets = entry.before.clone();
        self.redo.push(entry);
        self.sealed = true;
        Some((carets, applied))
    }

    /// Вернуть отменённое.
    pub fn redo(&mut self, text: &mut String) -> Option<(Carets, Vec<Vec<Edit>>)> {
        let entry = self.redo.pop()?;
        let mut applied = Vec::new();
        for step in &entry.steps {
            apply(text, &step.forward);
            applied.push(step.forward.clone());
        }
        let carets = entry.after.clone();
        self.undo.push(entry);
        self.sealed = true;
        Some((carets, applied))
    }
}

/// Граница слова (буквы, цифры, `_`) — для Ctrl+←/→, двойного щелчка, «следующего вхождения».
pub(crate) fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Ctrl+→: в конец текущего / следующего слова (знаки препинания — отдельные «слова»,
/// пробелы пропускаются), как в Visual Studio.
pub(crate) fn next_word(text: &str, at: usize) -> usize {
    let mut chars = text[at..].char_indices().peekable();
    let Some(&(_, first)) = chars.peek() else { return text.len() };
    let class = |c: char| if c == '\n' { 3 } else if c.is_whitespace() { 0 } else if is_word(c) { 1 } else { 2 };
    let start_class = class(first);
    if start_class == 3 {
        return at + first.len_utf8();
    }
    let mut end = at;
    // Своё «слово».
    while let Some(&(i, c)) = chars.peek() {
        if class(c) != start_class {
            break;
        }
        end = at + i + c.len_utf8();
        chars.next();
    }
    // Пробелы после него.
    while let Some(&(i, c)) = chars.peek() {
        if class(c) != 0 {
            break;
        }
        end = at + i + c.len_utf8();
        chars.next();
    }
    end
}

/// Ctrl+←: в начало текущего / предыдущего слова.
pub(crate) fn prev_word(text: &str, at: usize) -> usize {
    let class = |c: char| if c == '\n' { 3 } else if c.is_whitespace() { 0 } else if is_word(c) { 1 } else { 2 };
    let mut chars = text[..at].char_indices().rev().peekable();
    // Пробелы перед курсором.
    let mut start = at;
    while let Some(&(i, c)) = chars.peek() {
        if class(c) != 0 {
            break;
        }
        start = i;
        chars.next();
    }
    let Some(&(i, c)) = chars.peek() else { return start };
    let word_class = class(c);
    if word_class == 3 {
        return if start == at { i } else { start };
    }
    start = i;
    chars.next();
    while let Some(&(i, c)) = chars.peek() {
        if class(c) != word_class {
            break;
        }
        start = i;
        chars.next();
    }
    start
}

/// Слово под байтом `at` (для двойного щелчка); не слово — один символ.
pub(crate) fn word_range(text: &str, at: usize) -> Range<usize> {
    let at = floor_char(text, at.min(text.len()));
    let after = text[at..].chars().next();
    let before = text[..at].chars().next_back();
    let on_word = after.is_some_and(is_word) || before.is_some_and(is_word);
    if !on_word {
        return at..ceil_char(text, at + after.map_or(0, char::len_utf8).min(text.len() - at));
    }
    let start = text[..at].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(at, |(i, _)| i);
    let end = text[at..].char_indices().find(|(_, c)| !is_word(*c)).map_or(text.len(), |(i, _)| at + i);
    start..end
}

/// Все вхождения `needle` (как в Visual Studio: если искали по слову под курсором — только
/// целые слова) — для Shift+Alt+; и Shift+Alt+.
pub(crate) fn occurrences(text: &str, needle: &str, whole_word: bool) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let ok = !whole_word || (!text[..start].chars().next_back().is_some_and(is_word) && !text[end..].chars().next().is_some_and(is_word));
        if ok {
            out.push(start..end);
        }
        from = ceil_char(text, start + 1);
        if from >= text.len() {
            break;
        }
    }
    out
}

/// Строки текста `start_line..=end_line`, которые можно свернуть.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FoldRegion {
    /// Строка-заголовок (видна) и последняя скрываемая строка.
    pub start_line: usize,
    pub end_line: usize,
    pub kind: FoldKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FoldKind {
    /// `{ … }` (Rust, C++, WGSL, RON, JSON-подобные; Scenario в форме JavaScript).
    Braces,
    /// `function … end`, `if … then … end`, `while … do … end`, `repeat … until` (Scenario).
    Block,
    /// `// region` … `// endregion`, `#region` / `#pragma region` … `#endregion`.
    Region,
}

/// Области сворачивания текста по языку: скобочные блоки, блоки Scenario, `region`-метки.
/// Строки и комментарии пропускаются. Область — от строки открытия до строки закрытия
/// (одна строка — не область). Порядок — по строке заголовка, вложенные после внешних.
pub(crate) fn fold_regions(text: &str, language: crate::code::Language) -> Vec<FoldRegion> {
    use crate::code::Language;
    let bytes = text.as_bytes();
    let scenario = language == Language::Scenario;
    let plain = language == Language::Plain;
    let mut out = Vec::new();
    let mut braces: Vec<(u8, usize)> = Vec::new();
    let mut blocks: Vec<usize> = Vec::new();
    let mut regions: Vec<usize> = Vec::new();
    let mut line = 0usize;
    let mut line_start = 0usize;
    // `elseif … then` не открывает новый блок.
    let mut skip_then = false;
    let mut i = 0;
    let close_brace = |open_line: usize, line: usize, out: &mut Vec<FoldRegion>, kind: FoldKind| {
        if line > open_line {
            out.push(FoldRegion { start_line: open_line, end_line: line, kind });
        }
    };
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\n' {
            line += 1;
            line_start = i + 1;
            i += 1;
            continue;
        }
        if plain {
            i += 1;
            continue;
        }
        // Комментарии: строчный (с меткой region), блочный.
        let line_comment = (c == b'/' && bytes.get(i + 1) == Some(&b'/')) || (scenario && c == b'-' && bytes.get(i + 1) == Some(&b'-') && !(bytes.get(i + 2) == Some(&b'[') && bytes.get(i + 3) == Some(&b'[')));
        if line_comment {
            let end = text[i..].find('\n').map_or(text.len(), |k| i + k);
            let body = text[i + 2..end].trim_start().trim_start_matches(['/', '-']).trim_start();
            let lower = body.to_ascii_lowercase();
            if lower.starts_with("endregion") || lower.starts_with("#endregion") {
                if let Some(open) = regions.pop() {
                    close_brace(open, line, &mut out, FoldKind::Region);
                }
            } else if lower.starts_with("region") || lower.starts_with("#region") {
                regions.push(line);
            }
            i = end;
            continue;
        }
        if scenario && c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            // `--[[ … ]]`.
            let end = text[i + 4..].find("]]").map_or(text.len(), |k| i + 4 + k + 2);
            line += text[i..end].matches('\n').count();
            if let Some(k) = text[..end].rfind('\n') {
                line_start = k + 1;
            }
            i = end;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = text[i + 2..].find("*/").map_or(text.len(), |k| i + 2 + k + 2);
            line += text[i..end].matches('\n').count();
            if let Some(k) = text[..end].rfind('\n') {
                line_start = k + 1;
            }
            i = end;
            continue;
        }
        // `#region` / `#pragma region` (C++), `#endregion`.
        if c == b'#' && text[line_start..i].trim().is_empty() && language == Language::Cpp {
            let end = text[i..].find('\n').map_or(text.len(), |k| i + k);
            let directive = text[i + 1..end].trim_start();
            let directive = directive.strip_prefix("pragma").map_or(directive, str::trim_start);
            if directive.starts_with("endregion") {
                if let Some(open) = regions.pop() {
                    close_brace(open, line, &mut out, FoldKind::Region);
                }
            } else if directive.starts_with("region") {
                regions.push(line);
            }
            i = end;
            continue;
        }
        // Строки и символьные литералы.
        let quote = c == b'"' || (scenario && (c == b'\'' || c == b'`'));
        if quote {
            i += 1;
            while i < bytes.len() && bytes[i] != c && (c != b'\'' || bytes[i] != b'\n') {
                if bytes[i] == b'\n' {
                    line += 1;
                    line_start = i + 1;
                }
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if c == b'\'' && matches!(language, Language::Rust | Language::Cpp) {
            // `'x'` / `'\n'`; время жизни `'a` — пропустить один знак.
            let close = if bytes.get(i + 1) == Some(&b'\\') { i + 3 } else { i + 2 };
            i = if bytes.get(close) == Some(&b'\'') { close + 1 } else { i + 1 };
            continue;
        }
        match c {
            b'{' | b'[' | b'(' => {
                braces.push((c, line));
                i += 1;
                continue;
            }
            b'}' | b']' | b')' => {
                let open = match c {
                    b'}' => b'{',
                    b']' => b'[',
                    _ => b'(',
                };
                // Непарная скобка (текст ломаный) — снять до парной, если она есть.
                if let Some(at) = braces.iter().rposition(|(b, _)| *b == open) {
                    let (_, open_line) = braces[at];
                    braces.truncate(at);
                    // Свёртка — для `{}` всегда, для `[]` / `()` — только в данных (RON, JSON).
                    if open == b'{' || matches!(language, Language::Ron) {
                        close_brace(open_line, line, &mut out, FoldKind::Braces);
                    }
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        if scenario && (c.is_ascii_alphabetic() || c == b'_') {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            // Поле `obj.end` — не ключевое слово.
            let after_dot = start > 0 && bytes[start - 1] == b'.';
            if !after_dot {
                match &text[start..i] {
                    "function" | "repeat" => blocks.push(line),
                    "do" => blocks.push(line),
                    "elseif" => skip_then = true,
                    "then" => {
                        if std::mem::take(&mut skip_then) {
                        } else {
                            blocks.push(line);
                        }
                    }
                    "end" | "until" => {
                        if let Some(open) = blocks.pop() {
                            close_brace(open, line, &mut out, FoldKind::Block);
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        i += 1;
    }
    // `while … do` и `for … do` в одной строке открывают один блок (`do`), а `if … then` —
    // `then`; `function` открывает свой. Остальное уже учтено.
    out.sort_by_key(|r| (r.start_line, std::cmp::Reverse(r.end_line)));
    out.dedup_by_key(|r| r.start_line);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::Language;

    /// Детерминированный генератор (без внешних крейтов).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    const PIECES: [&str; 12] = ["a", "fn ", "\n", "{", "}", "ж", "😀", "  ", "\r\n", "x = 1;", "// c", "\""];

    fn random_edit(rng: &mut Rng, text: &str) -> Edit {
        let a = floor_char(text, rng.below(text.len() + 1));
        let b = floor_char(text, (a + rng.below(8)).min(text.len()));
        let mut insert = String::new();
        for _ in 0..rng.below(3) {
            insert.push_str(PIECES[rng.below(PIECES.len())]);
        }
        Edit::new(a..b.max(a), insert)
    }

    #[test]
    fn random_edits_match_a_plain_string_model_and_undo_round_trips() {
        let mut rng = Rng(42);
        for round in 0..200 {
            let mut text = String::from("fn main() {\n    let x = 1;\n}\n");
            let mut model = text.clone();
            let mut history = History::default();
            let mut snapshots = vec![text.clone()];
            let mut index = crate::code_diagnostics::LineIndex::new(&text);
            for step in 0..30 {
                // Пачка из 1–3 непересекающихся правок.
                let mut edits: Vec<Edit> = (0..1 + rng.below(3)).map(|_| random_edit(&mut rng, &text)).collect();
                edits.sort_by_key(|e| e.range.start);
                let mut kept: Vec<Edit> = Vec::new();
                for e in edits {
                    if kept.last().is_none_or(|l| e.range.start >= l.range.end && !(e.range.is_empty() && l.range.end == e.range.start && l.range.is_empty())) {
                        kept.push(e);
                    }
                }
                // Модель: применять с конца по одной.
                for e in kept.iter().rev() {
                    model.replace_range(e.range.clone(), &e.text);
                }
                let old = text.clone();
                let back = apply(&mut text, &kept);
                assert_eq!(text, model, "round {round} step {step}: {kept:?}");
                if let Some(change) = crate::code_diagnostics::text_change(&old, &text) {
                    index.apply_change(&text, change);
                }
                assert_eq!(index, crate::code_diagnostics::LineIndex::new(&text), "line index after {kept:?}");
                // Обратные правки возвращают старый текст.
                let mut undone = text.clone();
                apply(&mut undone, &back);
                assert_eq!(undone, old);
                history.record(kept, back, Carets::one(0), Carets::one(0), false, step as f64);
                snapshots.push(text.clone());
            }
            // Undo до начала, Redo до конца.
            for expected in snapshots.iter().rev().skip(1) {
                history.undo(&mut text).expect("an undo step");
                assert_eq!(&text, expected);
            }
            assert!(history.undo(&mut text).is_none());
            for expected in snapshots.iter().skip(1) {
                history.redo(&mut text).expect("a redo step");
                assert_eq!(&text, expected);
            }
        }
    }

    #[test]
    fn typing_groups_into_one_undo_entry_until_a_pause_or_a_command() {
        let mut text = String::new();
        let mut history = History::default();
        let mut t = 0.0;
        for c in ["a", "b", "c"] {
            let at = text.len();
            let edits = vec![Edit::new(at..at, c)];
            let back = apply(&mut text, &edits);
            history.record(edits, back, Carets::one(at), Carets::one(at + 1), true, t);
            t += 0.2;
        }
        // Пауза больше секунды — новая запись.
        t += 2.0;
        let edits = vec![Edit::new(3..3, "d")];
        let back = apply(&mut text, &edits);
        history.record(edits, back, Carets::one(3), Carets::one(4), true, t);
        assert_eq!(text, "abcd");
        let (carets, _) = history.undo(&mut text).unwrap();
        assert_eq!((text.as_str(), carets.primary().caret), ("abc", 3));
        let (carets, _) = history.undo(&mut text).unwrap();
        assert_eq!((text.as_str(), carets.primary().caret), ("", 0));
        history.redo(&mut text).unwrap();
        assert_eq!(text, "abc");
    }

    #[test]
    fn carets_merge_when_they_collide() {
        let mut carets = Carets { sels: vec![Sel::one(5), Sel::new(2, 6), Sel::one(9), Sel::one(9)], primary: 2 };
        carets.normalize();
        assert_eq!(carets.sels, vec![Sel::new(2, 6), Sel::one(9)]);
        assert_eq!(carets.primary, 1);
        assert_eq!(map_pos(4, &[Edit::new(1..2, "xyz")]), 6);
        assert_eq!(map_pos(4, &[Edit::new(4..4, "xyz")]), 4, "an insertion at the caret stays after it only when applied to that caret");
        assert_eq!(map_pos(4, &[Edit::new(2..6, "")]), 2);
    }

    #[test]
    fn word_motion_like_visual_studio() {
        let text = "let speed = a.b;  // x\nnext";
        assert_eq!(next_word(text, 0), 4);
        assert_eq!(next_word(text, 4), 10);
        assert_eq!(prev_word(text, 10), 4);
        assert_eq!(prev_word(text, 4), 0);
        assert_eq!(word_range(text, 6), 4..9);
        let ru = "привет мир";
        assert_eq!(&ru[word_range(ru, 3)], "привет");
        assert_eq!(occurrences("a ab a_ a", "a", true), vec![0..1, 8..9]);
        assert_eq!(occurrences("a ab a_ a", "a", false).len(), 4);
    }

    #[test]
    fn fold_regions_per_language() {
        let rust = "fn main() {\n    if x {\n        y();\n    }\n    let s = \"{\";\n}\n// region Setup\nfn a() {}\n// endregion\n";
        let regions: Vec<(usize, usize)> = fold_regions(rust, Language::Rust).iter().map(|r| (r.start_line, r.end_line)).collect();
        assert_eq!(regions, vec![(0, 5), (1, 3), (6, 8)]);

        let cpp = "#pragma region Math\nstruct A {\n  int x;\n};\n#pragma endregion\n/* {\n */\n";
        let regions: Vec<(usize, usize, FoldKind)> = fold_regions(cpp, Language::Cpp).iter().map(|r| (r.start_line, r.end_line, r.kind)).collect();
        assert_eq!(regions, vec![(0, 4, FoldKind::Region), (1, 3, FoldKind::Braces)]);

        let scenario = "function go()\n  if x then\n    print(1)\n  elseif y then\n    print(2)\n  end\n  while t do\n    wait(1)\n  end\nend\n-- region Tail\nlet s = 'end'\n-- endregion\n";
        let regions: Vec<(usize, usize, FoldKind)> = fold_regions(scenario, Language::Scenario).iter().map(|r| (r.start_line, r.end_line, r.kind)).collect();
        assert_eq!(regions, vec![(0, 9, FoldKind::Block), (1, 5, FoldKind::Block), (6, 8, FoldKind::Block), (10, 12, FoldKind::Region)]);

        // Однострочные блоки и незакрытые скобки — не области, и ничего не ломают.
        assert!(fold_regions("fn a() { b(); }\n{ never closed\n", Language::Rust).is_empty());
        assert!(fold_regions("{\n}\n", Language::Plain).is_empty());
    }
}
