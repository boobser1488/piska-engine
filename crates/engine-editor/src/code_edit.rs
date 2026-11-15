//! Правки текста вкладки Code в духе Visual Studio — чистые функции над текстом и
//! выделением (байты; `anchor` — где выделение начали, `caret` — где курсор):
//! строки (дублировать, удалить, вырезать, переместить, копировать вверх / вниз,
//! склеить, сортировать), комментарии (строчные и блочные, по языку), регистр,
//! отступ выделения, «умный» Home, Enter с отступом по языку, парные скобки и кавычки с
//! «перешагиванием», парная скобка (Ctrl+]), расширение / сужение выделения.
//!
//! Интерфейс (`code.rs`) переводит курсор egui в байты, вызывает функцию и кладёт
//! результат одним шагом Undo.

use std::ops::Range;

use crate::code::Language;

/// Текст и выделение после правки.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edited {
    pub text: String,
    pub anchor: usize,
    pub caret: usize,
}

impl Edited {
    fn new(text: String, anchor: usize, caret: usize) -> Self {
        Self { text, anchor, caret }
    }
}

fn sel_range(anchor: usize, caret: usize) -> Range<usize> {
    anchor.min(caret)..anchor.max(caret)
}

/// Начало строки, в которой лежит байт `at`.
pub(crate) fn line_start(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
}

/// Конец строки (байт перед `\n` или конец текста).
pub(crate) fn line_end(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// Целые строки, которые задевает выделение: `start..end` (end — после `\n`, если он есть).
fn covered_lines(text: &str, anchor: usize, caret: usize) -> Range<usize> {
    let r = sel_range(anchor, caret);
    let start = line_start(text, r.start);
    // Выделение, кончающееся в начале строки, эту строку не захватывает.
    let last = if r.end > r.start && line_start(text, r.end) == r.end { r.end - 1 } else { r.end };
    let end = line_end(text, last);
    let end = if end < text.len() { end + 1 } else { end };
    start..end
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Ctrl+D (Visual Studio): дублировать строку (или выделенный текст).
pub(crate) fn duplicate(text: &str, anchor: usize, caret: usize) -> Edited {
    let r = sel_range(anchor, caret);
    if !r.is_empty() {
        let piece = &text[r.clone()];
        let mut out = String::with_capacity(text.len() + piece.len());
        out.push_str(&text[..r.end]);
        out.push_str(piece);
        out.push_str(&text[r.end..]);
        return Edited::new(out, r.end, r.end + piece.len());
    }
    let lines = covered_lines(text, anchor, caret);
    let mut piece = text[lines.clone()].to_string();
    let mut out = String::with_capacity(text.len() + piece.len() + 1);
    out.push_str(&text[..lines.end]);
    if !piece.ends_with('\n') {
        out.push('\n');
        piece.pop();
        piece = text[lines.clone()].to_string();
    }
    out.push_str(&piece);
    out.push_str(&text[lines.end..]);
    let shift = out.len() - text.len();
    Edited::new(out, caret + shift, caret + shift)
}

/// Ctrl+Shift+K: удалить строки выделения. Возвращает и удалённый текст (для Ctrl+L).
pub(crate) fn delete_lines(text: &str, anchor: usize, caret: usize) -> (Edited, String) {
    let mut lines = covered_lines(text, anchor, caret);
    // Последняя строка без `\n` — забрать перевод строки перед ней.
    if lines.end == text.len() && !text[lines.clone()].ends_with('\n') && lines.start > 0 {
        lines.start -= 1;
    }
    let removed = text[lines.clone()].to_string();
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..lines.start]);
    out.push_str(&text[lines.end..]);
    let at = lines.start.min(out.len());
    let at = if at > 0 && removed.starts_with('\n') { line_start(&out, at.min(out.len())) } else { at };
    let removed = removed.strip_prefix('\n').map(|r| format!("{r}\n")).unwrap_or(removed);
    (Edited::new(out, at, at), removed)
}

/// Alt+↑ / Alt+↓: переместить строки выделения.
pub(crate) fn move_lines(text: &str, anchor: usize, caret: usize, up: bool) -> Option<Edited> {
    let lines = covered_lines(text, anchor, caret);
    let mut block = text[lines.clone()].to_string();
    let had_newline = block.ends_with('\n');
    if !had_newline {
        block.push('\n');
    }
    if up {
        if lines.start == 0 {
            return None;
        }
        let prev_start = line_start(text, lines.start - 1);
        let prev = &text[prev_start..lines.start];
        let mut out = String::with_capacity(text.len() + 1);
        out.push_str(&text[..prev_start]);
        out.push_str(&block);
        let mut prev = prev.to_string();
        if !had_newline {
            prev.pop();
        }
        out.push_str(&prev);
        out.push_str(&text[lines.end..]);
        let shift = lines.start - prev_start;
        Some(Edited::new(out, anchor - shift, caret - shift))
    } else {
        if lines.end >= text.len() {
            return None;
        }
        let next_end = line_end(text, lines.end);
        let mut next = text[lines.end..next_end].to_string();
        let tail_newline = next_end < text.len();
        next.push('\n');
        let mut out = String::with_capacity(text.len() + 1);
        out.push_str(&text[..lines.start]);
        out.push_str(&next);
        if !tail_newline {
            block.pop();
        }
        out.push_str(&block);
        if tail_newline {
            out.push_str(&text[next_end + 1..]);
        }
        let shift = next.len();
        Some(Edited::new(out, anchor + shift, caret + shift))
    }
}

/// Shift+Alt+↑ / ↓: копировать строки выделения вверх или вниз.
pub(crate) fn copy_lines(text: &str, anchor: usize, caret: usize, up: bool) -> Edited {
    let lines = covered_lines(text, anchor, caret);
    let mut block = text[lines.clone()].to_string();
    let mut out = String::with_capacity(text.len() + block.len() + 1);
    if !block.ends_with('\n') {
        out.push_str(&text[..lines.end]);
        out.push('\n');
        out.push_str(&block);
        let shift = if up { 0 } else { block.len() + 1 };
        return Edited::new(out, anchor + shift, caret + shift);
    }
    out.push_str(&text[..lines.start]);
    out.push_str(&block);
    out.push_str(&text[lines.start..]);
    let shift = if up { 0 } else { block.len() };
    block.clear();
    Edited::new(out, anchor + shift, caret + shift)
}

/// Склеить строки выделения (или текущую со следующей) через один пробел.
pub(crate) fn join_lines(text: &str, anchor: usize, caret: usize) -> Edited {
    let r = sel_range(anchor, caret);
    let lines = covered_lines(text, anchor, caret);
    let mut end = lines.end;
    if r.is_empty() || !text[lines.start..lines.end].trim_end_matches('\n').contains('\n') {
        // Текущая строка со следующей.
        end = line_end(text, line_end(text, r.start) + 1);
    }
    let start = lines.start;
    let body = &text[start..end.min(text.len())];
    let mut joined = String::new();
    for (i, line) in body.split('\n').enumerate() {
        if i == 0 {
            joined.push_str(line.trim_end());
        } else if !line.trim().is_empty() {
            joined.push(' ');
            joined.push_str(line.trim());
        }
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(&joined);
    out.push_str(&text[end.min(text.len())..]);
    let at = start + text[start..line_end(text, start)].trim_end().len();
    Edited::new(out, at, at)
}

/// Отсортировать строки выделения.
pub(crate) fn sort_lines(text: &str, anchor: usize, caret: usize) -> Edited {
    let lines = covered_lines(text, anchor, caret);
    let body = &text[lines.clone()];
    let newline = body.ends_with('\n');
    let mut parts: Vec<&str> = body.trim_end_matches('\n').split('\n').collect();
    parts.sort_by_key(|l| l.trim().to_lowercase());
    let mut sorted = parts.join("\n");
    if newline {
        sorted.push('\n');
    }
    let out = format!("{}{sorted}{}", &text[..lines.start], &text[lines.end..]);
    Edited::new(out, lines.start, lines.start + sorted.len())
}

/// Строчный комментарий языка.
pub(crate) fn line_comment(language: Language) -> Option<&'static str> {
    match language {
        Language::Rust | Language::Cpp | Language::Wgsl | Language::Ron => Some("//"),
        Language::Scenario => Some("--"),
        Language::Plain => None,
    }
}

/// Что делать с комментариями строк.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommentMode {
    Toggle,
    Comment,
    Uncomment,
}

/// Ctrl+/ (переключить), Ctrl+K Ctrl+C (закомментировать), Ctrl+K Ctrl+U (раскомментировать).
pub(crate) fn comment_lines(text: &str, anchor: usize, caret: usize, prefix: &str, mode: CommentMode) -> Edited {
    let lines = covered_lines(text, anchor, caret);
    let body = &text[lines.clone()];
    let rows: Vec<&str> = body.split_inclusive('\n').collect();
    let code_rows = || rows.iter().filter(|l| !l.trim().is_empty());
    let all_commented = code_rows().all(|l| l.trim_start().starts_with(prefix));
    let comment = match mode {
        CommentMode::Comment => true,
        CommentMode::Uncomment => false,
        CommentMode::Toggle => !all_commented || code_rows().next().is_none(),
    };
    let min_indent = code_rows().map(|l| indent_of(l).len()).min().unwrap_or(0);
    let mut out_body = String::with_capacity(body.len() + rows.len() * 3);
    // Сдвиг курсора и якоря: сколько байт добавили / убрали до них.
    let delta_before = |at: usize, added: isize, pos: usize| -> isize { if pos > at { added } else { 0 } };
    let (mut da, mut dc) = (0isize, 0isize);
    let mut offset = lines.start;
    for row in &rows {
        if row.trim().is_empty() {
            out_body.push_str(row);
            offset += row.len();
            continue;
        }
        if comment {
            let at = offset + min_indent;
            out_body.push_str(&row[..min_indent]);
            out_body.push_str(prefix);
            out_body.push(' ');
            out_body.push_str(&row[min_indent..]);
            let added = prefix.len() as isize + 1;
            da += delta_before(at, added, anchor);
            dc += delta_before(at, added, caret);
        } else {
            let indent = indent_of(row).len();
            let rest = &row[indent..];
            if let Some(after) = rest.strip_prefix(prefix) {
                let after = after.strip_prefix(' ').unwrap_or(after);
                let removed = (rest.len() - after.len()) as isize;
                out_body.push_str(&row[..indent]);
                out_body.push_str(after);
                let at = offset + indent;
                da -= delta_before(at, removed, anchor).min(anchor.saturating_sub(at) as isize);
                dc -= delta_before(at, removed, caret).min(caret.saturating_sub(at) as isize);
            } else {
                out_body.push_str(row);
            }
        }
        offset += row.len();
    }
    let out = format!("{}{out_body}{}", &text[..lines.start], &text[lines.end..]);
    let fix = |pos: usize, d: isize| ((pos as isize + d).max(lines.start as isize) as usize).min(out.len());
    let (a, c) = (fix(anchor, da), fix(caret, dc));
    Edited::new(out, a, c)
}

/// Shift+Alt+A: блочный комментарий вокруг выделения (или снять его).
pub(crate) fn toggle_block_comment(text: &str, anchor: usize, caret: usize, language: Language) -> Option<Edited> {
    let (open, close) = match language {
        Language::Scenario => ("--[[", "]]"),
        Language::Plain => return None,
        _ => ("/*", "*/"),
    };
    let mut r = sel_range(anchor, caret);
    if r.is_empty() {
        r = line_start(text, r.start)..line_end(text, r.start);
        let indent = indent_of(&text[r.clone()]).len();
        r.start += indent;
    }
    let piece = &text[r.clone()];
    if let Some(inner) = piece.strip_prefix(open).and_then(|p| p.strip_suffix(close)) {
        let inner = inner.strip_prefix(' ').unwrap_or(inner);
        let inner = inner.strip_suffix(' ').unwrap_or(inner);
        let out = format!("{}{inner}{}", &text[..r.start], &text[r.end..]);
        return Some(Edited::new(out, r.start, r.start + inner.len()));
    }
    let wrapped = format!("{open} {piece} {close}");
    let out = format!("{}{wrapped}{}", &text[..r.start], &text[r.end..]);
    Some(Edited::new(out, r.start, r.start + wrapped.len()))
}

/// Ctrl+Shift+U / Ctrl+U: регистр выделения (или слова под курсором).
pub(crate) fn change_case(text: &str, anchor: usize, caret: usize, upper: bool) -> Edited {
    let mut r = sel_range(anchor, caret);
    if r.is_empty() {
        r = crate::code_diagnostics::word_at(text, r.start);
    }
    let piece = &text[r.clone()];
    let changed = if upper { piece.to_uppercase() } else { piece.to_lowercase() };
    let out = format!("{}{changed}{}", &text[..r.start], &text[r.end..]);
    Edited::new(out, r.start, r.start + changed.len())
}

/// Tab / Shift+Tab над многострочным выделением: отступ строк.
pub(crate) fn indent_lines(text: &str, anchor: usize, caret: usize, unit: &str, outdent: bool) -> Edited {
    let lines = covered_lines(text, anchor, caret);
    let body = &text[lines.clone()];
    let mut out_body = String::with_capacity(body.len() + 64);
    for row in body.split_inclusive('\n') {
        if outdent {
            let indent = indent_of(row);
            let remove = if indent.starts_with('\t') { 1 } else { indent.len().min(unit.len().max(1)) };
            out_body.push_str(&row[remove.min(indent.len())..]);
        } else if row.trim().is_empty() {
            out_body.push_str(row);
        } else {
            out_body.push_str(unit);
            out_body.push_str(row);
        }
    }
    let out = format!("{}{out_body}{}", &text[..lines.start], &text[lines.end..]);
    let end = lines.start + out_body.len();
    let end = if out_body.ends_with('\n') { end - 1 } else { end };
    Edited::new(out, lines.start, end)
}

/// «Умный» Home: к первому непробельному символу строки, повторно — к её началу.
pub(crate) fn smart_home(text: &str, caret: usize) -> usize {
    let start = line_start(text, caret);
    let first = start + indent_of(&text[start..line_end(text, start)]).len();
    if caret == first {
        start
    } else {
        first
    }
}

/// Enter: перевод строки с отступом текущей строки, плюс уровень после `{` / `(` / `[`
/// (и после `then` / `do` / `function …` / `else` в Scenario). Курсор между `{` и `}`
/// — закрывающая на своей строке.
pub(crate) fn newline(text: &str, anchor: usize, caret: usize, unit: &str, language: Language) -> Edited {
    let r = sel_range(anchor, caret);
    let start = line_start(text, r.start);
    let before = &text[start..r.start];
    let indent = indent_of(before).to_string();
    let trimmed = before.trim_end();
    let opens = trimmed.ends_with(['{', '(', '['])
        || (language == Language::Rust && trimmed.ends_with("=>"))
        || (language == Language::Scenario && {
            let last = trimmed.rsplit(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
            matches!(last, "then" | "do" | "else" | "repeat") || (trimmed.trim_start().starts_with("function") || trimmed.contains("function(") || trimmed.contains("function (")) && trimmed.ends_with(')')
        });
    let after = &text[r.end..line_end(text, r.end)];
    let closes_next = after.trim_start().starts_with(['}', ')', ']']);
    let mut insert = String::from("\n");
    insert.push_str(&indent);
    if opens {
        insert.push_str(unit);
    }
    let caret_at = r.start + insert.len();
    if opens && closes_next {
        insert.push('\n');
        insert.push_str(&indent);
    }
    // Пробелы после курсора не переносятся на новую строку.
    let skip = after.len() - after.trim_start_matches([' ', '\t']).len();
    let skip = if closes_next && !opens { 0 } else { skip };
    let out = format!("{}{insert}{}", &text[..r.start], &text[r.end + skip..]);
    Edited::new(out, caret_at, caret_at)
}

fn closing_of(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        '\'' => '\'',
        '`' => '`',
        _ => return None,
    })
}

/// Набран символ `c`: парная скобка / кавычка или «перешагнуть» закрывающую.
/// `None` — обычная вставка (пусть делает поле).
pub(crate) fn type_char(text: &str, anchor: usize, caret: usize, c: char, language: Language) -> Option<Edited> {
    let r = sel_range(anchor, caret);
    let next = text[r.end..].chars().next();
    let prev = text[..r.start].chars().next_back();
    // Перешагнуть закрывающую, которую уже поставили.
    if r.is_empty() && matches!(c, ')' | ']' | '}' | '"' | '\'' | '`') && next == Some(c) {
        return Some(Edited::new(text.to_string(), r.start + 1, r.start + 1));
    }
    let close = closing_of(c)?;
    if language == Language::Plain {
        return None;
    }
    let quote = matches!(c, '"' | '\'' | '`');
    if c == '`' && language != Language::Scenario {
        return None;
    }
    // `'` в Rust — чаще время жизни (`&'a`), в словах — апостроф.
    if c == '\'' && (language == Language::Rust || prev.is_some_and(|p| p.is_alphanumeric())) {
        return None;
    }
    if quote && prev.is_some_and(|p| p.is_alphanumeric() || p == c) {
        return None;
    }
    // Выделение — обернуть.
    if !r.is_empty() {
        let out = format!("{}{c}{}{close}{}", &text[..r.start], &text[r.clone()], &text[r.end..]);
        return Some(Edited::new(out, r.start + 1, r.end + 1));
    }
    let ok_next = next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';' | ':' | '.'));
    if !ok_next {
        return None;
    }
    let out = format!("{}{c}{close}{}", &text[..r.start], &text[r.end..]);
    Some(Edited::new(out, r.start + 1, r.start + 1))
}

/// Backspace между только что поставленной парой `(|)` — удалить обе.
pub(crate) fn backspace_pair(text: &str, anchor: usize, caret: usize) -> Option<Edited> {
    if anchor != caret || caret == 0 {
        return None;
    }
    let prev = text[..caret].chars().next_back()?;
    let next = text[caret..].chars().next()?;
    if closing_of(prev) == Some(next) {
        let out = format!("{}{}", &text[..caret - prev.len_utf8()], &text[caret + next.len_utf8()..]);
        let at = caret - prev.len_utf8();
        return Some(Edited::new(out, at, at));
    }
    None
}

/// Байт парной скобки к скобке у курсора (справа от него или слева).
pub(crate) fn matching_bracket(text: &str, caret: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let at = [caret, caret.wrapping_sub(1)].into_iter().find(|&i| i < bytes.len() && b"()[]{}".contains(&bytes[i]))?;
    let c = bytes[at];
    let (open, close, forward) = match c {
        b'(' => (b'(', b')', true),
        b'[' => (b'[', b']', true),
        b'{' => (b'{', b'}', true),
        b')' => (b'(', b')', false),
        b']' => (b'[', b']', false),
        _ => (b'{', b'}', false),
    };
    let mut depth = 0i32;
    if forward {
        for (i, b) in bytes.iter().enumerate().skip(at) {
            if *b == open {
                depth += 1;
            } else if *b == close {
                depth -= 1;
                if depth == 0 {
                    return Some((at, i));
                }
            }
        }
    } else {
        for i in (0..=at).rev() {
            if bytes[i] == close {
                depth += 1;
            } else if bytes[i] == open {
                depth -= 1;
                if depth == 0 {
                    return Some((at, i));
                }
            }
        }
    }
    None
}

/// Shift+Alt+→: следующее по размеру выделение — слово, строка в кавычках, содержимое
/// скобок, скобки целиком, … весь текст.
pub(crate) fn expand_selection(text: &str, anchor: usize, caret: usize) -> Range<usize> {
    let r = sel_range(anchor, caret);
    let word = crate::code_diagnostics::word_at(text, r.start);
    if r.is_empty() && !word.is_empty() && text[word.clone()].chars().all(|c| c.is_alphanumeric() || c == '_') {
        return word;
    }
    // Ближайшие объемлющие скобки.
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = r.start;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'(' | b'[' | b'{' => {
                if let Some((_, close)) = matching_bracket(text, i) {
                    if close + 1 >= r.end {
                        let inner = i + 1..close;
                        if inner.start < r.start || inner.end > r.end {
                            return inner;
                        }
                        return i..close + 1;
                    }
                }
            }
            _ => {}
        }
    }
    0..text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret(text: &str) -> (String, usize) {
        let at = text.find('|').unwrap();
        (text.replacen('|', "", 1), at)
    }

    fn show(e: &Edited) -> String {
        let mut s = e.text.clone();
        s.insert(e.caret, '|');
        s
    }

    #[test]
    fn line_operations() {
        let (t, c) = caret("a\nb|b\nc\n");
        assert_eq!(show(&duplicate(&t, c, c)), "a\nbb\nb|b\nc\n");
        assert_eq!(delete_lines(&t, c, c).0.text, "a\nc\n");
        assert_eq!(delete_lines(&t, c, c).1, "bb\n");
        let (last, c2) = caret("a\nb|");
        assert_eq!(delete_lines(&last, c2, c2).0.text, "a");
        assert_eq!(show(&move_lines(&t, c, c, true).unwrap()), "b|b\na\nc\n");
        assert_eq!(show(&move_lines(&t, c, c, false).unwrap()), "a\nc\nb|b\n");
        assert!(move_lines(&t, 0, 0, true).is_none());
        let (end, c3) = caret("a\nb|");
        assert_eq!(show(&move_lines(&end, c3, c3, true).unwrap()), "b|\na");
        assert_eq!(copy_lines(&t, c, c, false).text, "a\nbb\nbb\nc\n");
        assert_eq!(copy_lines(&t, c, c, true).caret, c);
        assert_eq!(join_lines("fn a() {\n    x;\n}", 3, 3).text, "fn a() { x;\n}");
        assert_eq!(sort_lines("c\na\nb\n", 0, 5).text, "a\nb\nc\n");
    }

    #[test]
    fn comments_per_language() {
        let text = "fn a() {\n    x();\n}\n";
        let e = comment_lines(text, 0, text.len() - 1, "//", CommentMode::Toggle);
        assert_eq!(e.text, "// fn a() {\n//     x();\n// }\n");
        let back = comment_lines(&e.text, 0, e.text.len() - 1, "//", CommentMode::Toggle);
        assert_eq!(back.text, text);
        let s = "local x = 1\n  print(x)\n";
        assert_eq!(comment_lines(s, 13, 13, line_comment(Language::Scenario).unwrap(), CommentMode::Comment).text, "local x = 1\n  -- print(x)\n");
        assert_eq!(comment_lines("x", 0, 0, "//", CommentMode::Uncomment).text, "x");
        let b = toggle_block_comment("int a = 1;", 4, 5, Language::Cpp).unwrap();
        assert_eq!(b.text, "int /* a */ = 1;");
        let un = toggle_block_comment(&b.text, b.anchor, b.caret, Language::Cpp).unwrap();
        assert_eq!(un.text, "int a = 1;");
        assert_eq!(toggle_block_comment("x = 1", 0, 0, Language::Scenario).unwrap().text, "--[[ x = 1 ]]");
    }

    #[test]
    fn case_indent_home_and_enter() {
        assert_eq!(change_case("let speed", 4, 9, true).text, "let SPEED");
        assert_eq!(change_case("let SPEED", 6, 6, false).text, "let speed");
        assert_eq!(indent_lines("a\nb\n", 0, 3, "    ", false).text, "    a\n    b\n");
        assert_eq!(indent_lines("    a\n\tb\n", 0, 8, "    ", true).text, "a\nb\n");
        assert_eq!(smart_home("    x = 1", 8), 4);
        assert_eq!(smart_home("    x = 1", 4), 0);
        let (t, c) = caret("fn a() {|}");
        assert_eq!(show(&newline(&t, c, c, "    ", Language::Rust)), "fn a() {\n    |\n}");
        let (t, c) = caret("    let x = 1;|");
        assert_eq!(show(&newline(&t, c, c, "    ", Language::Rust)), "    let x = 1;\n    |");
        let (t, c) = caret("match x {\n    A =>|");
        assert_eq!(show(&newline(&t, c, c, "    ", Language::Rust)), "match x {\n    A =>\n        |");
        let (t, c) = caret("if x then|");
        assert_eq!(show(&newline(&t, c, c, "  ", Language::Scenario)), "if x then\n  |");
        let (t, c) = caret("function go(a)|");
        assert_eq!(show(&newline(&t, c, c, "  ", Language::Scenario)), "function go(a)\n  |");
    }

    #[test]
    fn brackets_and_quotes() {
        let (t, c) = caret("f|");
        let e = type_char(&t, c, c, '(', Language::Rust).unwrap();
        assert_eq!(show(&e), "f(|)");
        let over = type_char(&e.text, e.caret, e.caret, ')', Language::Rust).unwrap();
        assert_eq!(show(&over), "f()|");
        assert_eq!(show(&backspace_pair(&e.text, e.caret, e.caret).unwrap()), "f|");
        assert!(type_char("x|y", 1, 1, '(', Language::Rust).is_none(), "no pair before a word");
        assert!(type_char("&", 1, 1, '\'', Language::Rust).is_none(), "lifetimes in Rust");
        assert_eq!(show(&type_char("a = ", 4, 4, '"', Language::Rust).unwrap()), "a = \"|\"");
        assert_eq!(type_char("ab", 0, 2, '[', Language::Cpp).unwrap().text, "[ab]");
        assert_eq!(matching_bracket("f(a[1])", 1), Some((1, 6)));
        assert_eq!(matching_bracket("f(a[1])", 7), Some((6, 1)));
        let r = expand_selection("foo(bar, baz)", 5, 5);
        assert_eq!(r, 4..7);
        let r = expand_selection("foo(bar, baz)", 4, 7);
        assert_eq!(r, 4..12);
        let r = expand_selection("foo(bar, baz)", 4, 12);
        assert_eq!(r, 3..13);
    }
}
