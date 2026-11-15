//! Подсветка синтаксиса вкладки Code по строкам. Состояние разбора на начало каждой
//! строки (внутри блочного комментария, строки…) запоминается; после правки строки
//! пересчитываются с изменённой, пока состояние не совпадёт со старым, — и только до
//! видимой части. Так большой файл не размечается заново на каждое нажатие.
//!
//! Разбор лексический и прощающий: незакрытая строка или комментарий просто тянутся до
//! конца — при наборе текст всегда «сломан».

use std::ops::Range;

use egui::Color32;

use crate::code::Language;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Plain,
    Keyword,
    Type,
    Function,
    Macro,
    String,
    Number,
    Comment,
}

pub(crate) fn token_color(token: Token) -> Color32 {
    match token {
        Token::Plain => theme::TEXT,
        Token::Keyword => theme::SYN_KEYWORD,
        Token::Type => theme::SYN_TYPE,
        Token::Function => theme::SYN_FUNCTION,
        Token::Macro => theme::SYN_MACRO,
        Token::String => theme::SYN_STRING,
        Token::Number => theme::SYN_NUMBER,
        Token::Comment => theme::SYN_COMMENT,
    }
}

/// Состояние разбора на конце строки (переходит на следующую).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum LexState {
    #[default]
    Normal,
    /// Внутри `/* … */`.
    BlockComment,
    /// Внутри строки с этой кавычкой (`"…"` может занимать несколько строк, `` `…` `` в Scenario).
    Str(u8),
    /// Внутри `--[[ … ]]` (Scenario).
    LongComment,
}

/// Разметить одну строку (без `\n`) с состоянием на её начало. Отрезки — в байтах строки,
/// соседние одного цвета склеены. Возвращает состояние на её конце.
pub(crate) fn tokenize_line(line: &str, language: Language, state: LexState, tokens: &mut Vec<(Token, Range<usize>)>) -> LexState {
    let bytes = line.as_bytes();
    let keywords = language.keywords();
    let mut push = |token: Token, range: Range<usize>| {
        if range.is_empty() {
            return;
        }
        match tokens.last_mut() {
            Some((last, last_range)) if *last == token && last_range.end == range.start => last_range.end = range.end,
            _ => tokens.push((token, range)),
        }
    };
    if language == Language::Plain {
        push(Token::Plain, 0..bytes.len());
        return LexState::Normal;
    }
    let mut i = 0;
    // Продолжение многострочного куска с прошлой строки.
    match state {
        LexState::Normal => {}
        LexState::BlockComment => match line.find("*/") {
            Some(k) => {
                i = k + 2;
                push(Token::Comment, 0..i);
            }
            None => {
                push(Token::Comment, 0..bytes.len());
                return LexState::BlockComment;
            }
        },
        LexState::LongComment => match line.find("]]") {
            Some(k) => {
                i = k + 2;
                push(Token::Comment, 0..i);
            }
            None => {
                push(Token::Comment, 0..bytes.len());
                return LexState::LongComment;
            }
        },
        LexState::Str(quote) => {
            while i < bytes.len() && bytes[i] != quote {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            if i >= bytes.len() {
                push(Token::String, 0..bytes.len());
                return LexState::Str(quote);
            }
            i += 1;
            push(Token::String, 0..i);
        }
    }
    let mut at_line_start = i == 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c == b' ' || c == b'\t' || c == b'\r' {
            i += 1;
            push(Token::Plain, start..i);
            continue;
        }
        let first = std::mem::replace(&mut at_line_start, false);
        if language == Language::Scenario && c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            // Комментарий Scenario: `-- …` до конца строки или `--[[ … ]]`.
            if bytes.get(i + 2) == Some(&b'[') && bytes.get(i + 3) == Some(&b'[') {
                match line[i + 4..].find("]]") {
                    Some(k) => {
                        i = i + 4 + k + 2;
                        push(Token::Comment, start..i);
                        continue;
                    }
                    None => {
                        push(Token::Comment, start..bytes.len());
                        return LexState::LongComment;
                    }
                }
            }
            push(Token::Comment, start..bytes.len());
            return LexState::Normal;
        } else if language == Language::Scenario && (c == b'\'' || c == b'`') {
            // Строка в одинарных кавычках (до конца строки) или шаблон `…${x}…` (многострочный).
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            if i >= bytes.len() {
                push(Token::String, start..bytes.len());
                return if c == b'`' { LexState::Str(b'`') } else { LexState::Normal };
            }
            i += 1;
            push(Token::String, start..i);
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            push(Token::Comment, start..bytes.len());
            return LexState::Normal;
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            match line[i + 2..].find("*/") {
                Some(k) => {
                    i = i + 2 + k + 2;
                    push(Token::Comment, start..i);
                }
                None => {
                    push(Token::Comment, start..bytes.len());
                    return LexState::BlockComment;
                }
            }
        } else if c == b'#' && language == Language::Cpp && first {
            // Директива препроцессора — до конца строки.
            push(Token::Macro, start..bytes.len());
            return LexState::Normal;
        } else if c == b'#' && language == Language::Rust && matches!(bytes.get(i + 1), Some(b'[') | Some(b'!')) {
            // Атрибут `#[...]`.
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            push(Token::Macro, start..i);
        } else if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            if i >= bytes.len() {
                push(Token::String, start..bytes.len());
                return LexState::Str(b'"');
            }
            i += 1;
            push(Token::String, start..i);
        } else if c == b'\'' && language != Language::Ron {
            // Символьный литерал `'x'` / `'\n'`; время жизни `'a` в Rust — не литерал.
            let close = if bytes.get(i + 1) == Some(&b'\\') { i + 3 } else { i + 2 };
            if bytes.get(close) == Some(&b'\'') {
                i = close + 1;
                push(Token::String, start..i);
            } else {
                i += 1;
                push(Token::Plain, start..i);
            }
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_') {
                i += 1;
            }
            push(Token::Number, start..i);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let next = bytes[i..].iter().copied().find(|b| *b != b' ');
            let token = if keywords.contains(&word) {
                Token::Keyword
            } else if language == Language::Rust && bytes.get(i) == Some(&b'!') && bytes.get(i + 1) != Some(&b'=') {
                // `name!` вплотную — макрос; `x != y` — нет.
                Token::Macro
            } else if next == Some(b'(') {
                Token::Function
            } else if word.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                Token::Type
            } else {
                Token::Plain
            };
            push(token, start..i);
        } else {
            // Знак препинания или не-ASCII символ (целиком, по границе UTF-8).
            i += 1;
            while i < bytes.len() && !line.is_char_boundary(i) {
                i += 1;
            }
            push(Token::Plain, start..i);
        }
    }
    LexState::Normal
}

#[cfg(test)]
/// Разбить весь текст на окрашенные отрезки (байты текста; переводы строк — `Plain`).
pub(crate) fn tokenize(text: &str, language: Language) -> Vec<(Token, Range<usize>)> {
    let mut out: Vec<(Token, Range<usize>)> = Vec::new();
    let mut state = LexState::Normal;
    let mut line_tokens = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        line_tokens.clear();
        state = tokenize_line(line, language, state, &mut line_tokens);
        for (token, range) in line_tokens.drain(..) {
            let range = offset + range.start..offset + range.end;
            match out.last_mut() {
                Some((last, last_range)) if *last == token && last_range.end == range.start => last_range.end = range.end,
                _ => out.push((token, range)),
            }
        }
        offset += line.len();
        if offset < text.len() {
            let range = offset..offset + 1;
            match out.last_mut() {
                // Перевод строки внутри комментария / строки — того же цвета (как раньше).
                Some((last, last_range)) if last_range.end == range.start && (*last == Token::Plain || state != LexState::Normal) => last_range.end = range.end,
                _ => out.push((Token::Plain, range)),
            }
            offset += 1;
        }
    }
    out
}

#[cfg(test)]
/// Весь текст одной раскладкой (для старого пути и замеров).
pub(crate) fn highlight(text: &str, language: Language, font: egui::FontId) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for (token, range) in tokenize(text, language) {
        job.append(&text[range], 0.0, TextFormat { font_id: font.clone(), color: token_color(token), ..TextFormat::default() });
    }
    job
}

/// Состояния разбора на начало строк документа с «грязным» хвостом после правок.
#[derive(Clone, Debug, Default)]
pub(crate) struct Highlighter {
    /// `states[k]` — состояние на начало строки `k` (длина = число строк).
    states: Vec<LexState>,
    /// Строки `>= dirty_from` могут быть неверны; `dirty_to` — последняя правленая строка
    /// (после неё совпадение со старым состоянием значит «дальше всё верно»).
    dirty_from: usize,
    dirty_to: usize,
    language: Option<Language>,
}

impl Highlighter {
    /// Всё заново (другой язык или файл).
    pub fn reset(&mut self, lines: usize, language: Language) {
        self.states = vec![LexState::Normal; lines.max(1)];
        self.dirty_from = 1;
        self.dirty_to = usize::MAX;
        self.language = Some(language);
    }

    /// Строки `first..=old_last` правки стали строками `first..=new_last`.
    pub fn edited(&mut self, first: usize, old_last: usize, new_last: usize) {
        if self.language.is_none() {
            return;
        }
        let was_dirty = self.dirty_from != usize::MAX;
        let prev_to = self.dirty_to;
        let first = first.min(self.states.len());
        let old_last = old_last.min(self.states.len().saturating_sub(1)).max(first);
        let added = new_last as isize - old_last as isize;
        if added > 0 {
            let at = (old_last + 1).min(self.states.len());
            self.states.splice(at..at, std::iter::repeat_n(LexState::Normal, added as usize));
        } else if added < 0 {
            let remove = (-added) as usize;
            let at = (first + 1).min(self.states.len());
            let end = (at + remove).min(self.states.len());
            self.states.drain(at..end);
        }
        if self.states.is_empty() {
            self.states.push(LexState::Normal);
        }
        // Состояние строки `first` не меняется (оно — от строк выше); пересчёт — с `first + 1`.
        self.dirty_from = self.dirty_from.min(first + 1);
        let to = new_last.max(first);
        self.dirty_to = if !was_dirty {
            to
        } else if prev_to == usize::MAX {
            usize::MAX
        } else {
            let shifted = if prev_to > old_last { (prev_to as isize + added).max(0) as usize } else { prev_to };
            shifted.max(to)
        };
    }

    /// Состояния верны до строки `line` включительно (дальше не считаем — не видно).
    pub fn ensure(&mut self, text: &str, index: &crate::code_diagnostics::LineIndex, language: Language, line: usize) {
        if self.language != Some(language) || self.states.len() != index.lines() {
            self.reset(index.lines(), language);
        }
        let last = line.min(self.states.len() - 1);
        let mut scratch = Vec::new();
        while self.dirty_from <= last {
            let k = self.dirty_from - 1;
            let start = index.line_start(k);
            let end = index.line_end(text, k);
            scratch.clear();
            let next = tokenize_line(&text[start..end], language, self.states[k], &mut scratch);
            let converged = self.dirty_to != usize::MAX && k >= self.dirty_to && self.states[k + 1] == next;
            self.states[k + 1] = next;
            self.dirty_from += 1;
            if converged {
                self.dirty_from = usize::MAX;
                self.dirty_to = 0;
                break;
            }
        }
        if self.dirty_from >= self.states.len() {
            self.dirty_from = usize::MAX;
            self.dirty_to = 0;
        } else if self.dirty_from != usize::MAX && self.dirty_to != usize::MAX {
            // Остановились, не сойдясь: дальше — старые состояния, и сходиться с ними можно
            // только после пересчитанного.
            self.dirty_to = self.dirty_to.max(self.dirty_from - 1);
        }
    }

    /// Состояние на начало строки `line` (после [`Self::ensure`]).
    pub fn state(&self, line: usize) -> LexState {
        self.states.get(line).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states_from_scratch(text: &str, language: Language) -> Vec<LexState> {
        let mut out = vec![LexState::Normal];
        let mut state = LexState::Normal;
        let mut scratch = Vec::new();
        for line in text.split('\n') {
            scratch.clear();
            state = tokenize_line(line, language, state, &mut scratch);
            out.push(state);
        }
        out.pop();
        out
    }

    #[test]
    fn incremental_states_match_a_full_pass_after_random_edits() {
        let pieces = ["/*", "*/", "\"", "\n", "x", "`", "--[[", "]]", "// c", "\\", "ж"];
        let mut seed: u64 = 3;
        let mut rnd = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 33) as usize) % n.max(1)
        };
        for language in [Language::Rust, Language::Scenario, Language::Cpp] {
            let mut text = String::from("fn a() {}\n/* x */\nlet s = \"q\";\n");
            let mut index = crate::code_diagnostics::LineIndex::new(&text);
            let mut hl = Highlighter::default();
            hl.ensure(&text, &index, language, usize::MAX);
            for _ in 0..400 {
                let at = crate::code_diagnostics::floor_char(&text, rnd(text.len() + 1));
                let end = crate::code_diagnostics::floor_char(&text, (at + rnd(4)).min(text.len()));
                let insert: String = (0..rnd(3)).map(|_| pieces[rnd(pieces.len())]).collect();
                let first = index.line_of(at);
                let old_last = index.line_of(end);
                let old = text.clone();
                text.replace_range(at..end, &insert);
                if let Some(change) = crate::code_diagnostics::text_change(&old, &text) {
                    index.apply_change(&text, change);
                }
                let new_last = index.line_of(at + insert.len());
                hl.edited(first, old_last, new_last);
                // Иногда смотрим только на верх файла — хвост остаётся «грязным».
                let upto = if rnd(3) == 0 { rnd(index.lines()) } else { usize::MAX };
                hl.ensure(&text, &index, language, upto);
                let full = states_from_scratch(&text, language);
                let check = upto.min(full.len() - 1);
                assert_eq!(&hl.states[..=check], &full[..=check], "{language:?} {text:?}");
            }
        }
    }
}
