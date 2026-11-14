//! Мгновенная проверка C / C++ во вкладке Code: прощающий лексер без препроцессора.
//!
//! Ловит: незакрытые строки, символы и блочные комментарии; баланс скобок (ветки `#else`
//! / `#elif` пропускаются — в них скобки часто «дублируют» первую ветку); `#if` без
//! `#endif` и `#endif` / `#else` без `#if`; пропущенную `;` после определения
//! `class` / `struct` / `union` / `enum`.
//!
//! Не ловит (это дело компилятора после Build): типы, необъявленные имена, ошибки в
//! раскрытых макросах, шаблоны.

use crate::i18n::{tr_s, trf};
use std::ops::Range;

use crate::code_diagnostics::{Diagnostic, LineIndex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Ident,
    Literal,
    Open(u8),
    Close(u8),
    Punct(u8),
}

fn closer(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        _ => b'}',
    }
}

/// Строка препроцессора: директива и её диапазон (с продолжениями `\`).
struct Directive {
    name: String,
    range: Range<usize>,
}

pub(crate) fn check(text: &str) -> Vec<Diagnostic> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut tokens: Vec<(Tok, Range<usize>)> = Vec::new();
    // Стек условной компиляции: (диапазон `#if`, пропускаем ли текущую ветку).
    let mut conditions: Vec<(Range<usize>, bool)> = Vec::new();
    let skipping = |conditions: &Vec<(Range<usize>, bool)>| conditions.iter().any(|(_, skip)| *skip);
    let mut i = 0;
    let mut line_start = true;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c == b'\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' {
            i += 1;
            continue;
        }
        let at_line_start = std::mem::replace(&mut line_start, false);
        if c == b'#' && at_line_start {
            let directive = read_directive(text, i);
            i = directive.range.end;
            match directive.name.as_str() {
                "if" | "ifdef" | "ifndef" => conditions.push((directive.range.clone(), false)),
                "elif" | "else" | "elifdef" | "elifndef" => match conditions.last_mut() {
                    Some((_, skip)) => *skip = true,
                    None => out.push(
                        Diagnostic::error(trf!("`#{name}` without `#if`", name = directive.name), directive.range.clone())
                            .hint(tr_s("every `#else` / `#elif` belongs to an `#if`, `#ifdef` or `#ifndef` above it")),
                    ),
                },
                "endif" => {
                    if conditions.pop().is_none() {
                        out.push(
                            Diagnostic::error(tr_s("`#endif` without `#if`"), directive.range.clone())
                                .hint(tr_s("delete it, or add the `#if` / `#ifdef` it closes")),
                        );
                    }
                }
                _ => {}
            }
            continue;
        }
        let skip = skipping(&conditions);
        let mut push = |tok: Tok, range: Range<usize>| {
            if !skip {
                tokens.push((tok, range));
            }
        };
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            // Однострочный комментарий (с продолжением `\` в конце строки).
            while i < bytes.len() && !(bytes[i] == b'\n' && bytes[i.saturating_sub(1)] != b'\\') {
                i += 1;
            }
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            match text[i + 2..].find("*/") {
                Some(end) => i = i + 2 + end + 2,
                None => {
                    out.push(Diagnostic::error(tr_s("unterminated comment"), start..start + 2).hint(tr_s("close it with `*/`")));
                    i = bytes.len();
                }
            }
        } else if c == b'R' && bytes.get(i + 1) == Some(&b'"') || (matches!(c, b'u' | b'U' | b'L') && raw_prefix(bytes, i)) {
            // Сырая строка R"delim( … )delim".
            let quote = i + bytes[i..].iter().position(|b| *b == b'"').unwrap_or(0);
            let open = text[quote + 1..].find('(').map(|p| quote + 1 + p);
            match open {
                Some(open) => {
                    let delimiter = &text[quote + 1..open];
                    let end = format!("){delimiter}\"");
                    match text[open..].find(&end) {
                        Some(p) => i = open + p + end.len(),
                        None => {
                            out.push(Diagnostic::error(tr_s("unterminated raw string"), start..quote + 1));
                            i = bytes.len();
                        }
                    }
                }
                None => {
                    out.push(Diagnostic::error(tr_s("unterminated raw string"), start..quote + 1));
                    i = bytes.len();
                }
            }
            push(Tok::Literal, start..i);
        } else if c == b'"' || c == b'\'' {
            i += 1;
            let mut closed = false;
            while i < bytes.len() && bytes[i] != b'\n' {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == c {
                    closed = true;
                    i += 1;
                    break;
                }
                i += 1;
            }
            i = i.min(bytes.len());
            if !closed {
                let what = if c == b'"' { tr_s("missing terminating `\"` character") } else { tr_s("missing terminating `'` character") };
                out.push(Diagnostic::error(what, start..start + 1));
            }
            push(Tok::Literal, start..i);
        } else if c.is_ascii_digit() || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            // Числа, в том числе с разделителями `1'000'000` и экспонентой `1e-3`.
            i += 1;
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
                    i += 1;
                } else if b == b'\'' && bytes.get(i + 1).is_some_and(u8::is_ascii_alphanumeric) {
                    i += 1;
                } else if (b == b'+' || b == b'-') && matches!(bytes[i - 1], b'e' | b'E' | b'p' | b'P') {
                    i += 1;
                } else {
                    break;
                }
            }
            push(Tok::Literal, start..i);
        } else if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) {
                i += 1;
            }
            push(Tok::Ident, start..i);
        } else {
            i += 1;
            let tok = match c {
                b'(' | b'[' | b'{' => Tok::Open(c),
                b')' | b']' | b'}' => Tok::Close(c),
                _ => Tok::Punct(c),
            };
            push(tok, start..i);
        }
    }
    for (range, _) in conditions {
        out.push(
            Diagnostic::error(tr_s("unterminated conditional directive"), range)
                .hint(tr_s("add `#endif` for this `#if` / `#ifdef` / `#ifndef`")),
        );
    }
    out.extend(delimiters(text, &tokens));
    out.extend(missing_semicolons(text, &tokens));
    out
}

fn raw_prefix(bytes: &[u8], i: usize) -> bool {
    let rest = &bytes[i..];
    rest.starts_with(b"u8R\"") || rest.starts_with(b"uR\"") || rest.starts_with(b"UR\"") || rest.starts_with(b"LR\"")
}

fn read_directive(text: &str, start: usize) -> Directive {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    let name_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let name = text[name_start..i].to_string();
    // До конца строки, с продолжениями `\`; блочный комментарий тоже может начаться здесь.
    while i < bytes.len() {
        if bytes[i] == b'\n' && (i == 0 || bytes[i - 1] != b'\\') && !(i >= 2 && bytes[i - 1] == b'\r' && bytes[i - 2] == b'\\') {
            break;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            match text[i + 2..].find("*/") {
                Some(end) => {
                    i = i + 2 + end + 2;
                    continue;
                }
                None => break,
            }
        }
        i += 1;
    }
    let end_of_name = text[start..i].trim_end().len() + start;
    Directive { name, range: start..end_of_name.max(start + 1) }
}

fn delimiters(text: &str, tokens: &[(Tok, Range<usize>)]) -> Vec<Diagnostic> {
    let index = LineIndex::new(text);
    let mut stack: Vec<(u8, Range<usize>)> = Vec::new();
    let mut out = Vec::new();
    for (tok, range) in tokens {
        match *tok {
            Tok::Open(c) => stack.push((c, range.clone())),
            Tok::Close(c) => {
                if stack.last().is_some_and(|(open, _)| closer(*open) == c) {
                    stack.pop();
                } else if let Some(depth) = stack.iter().rposition(|(open, _)| closer(*open) == c) {
                    let (open, at) = stack.last().cloned().expect("non-empty");
                    out.push(
                        Diagnostic::error(trf!("expected `{close}` before `{c}`", close = closer(open) as char, c = c as char), range.clone())
                            .note(trf!("to match this `{open}` on line {line}", open = open as char, line = index.line_of(at.start) + 1)),
                    );
                    stack.truncate(depth);
                } else {
                    out.push(Diagnostic::error(trf!("extraneous closing `{c}`", c = c as char), range.clone()).hint(tr_s("there is no opening bracket for it")));
                }
            }
            _ => {}
        }
    }
    for (open, at) in stack.into_iter().rev() {
        out.push(
            Diagnostic::error(trf!("`{open}` is never closed", open = open as char), at)
                .hint(trf!("add the matching `{close}`", close = closer(open) as char)),
        );
    }
    out
}

/// `struct A { … }` без `;` после `}`.
fn missing_semicolons(text: &str, tokens: &[(Tok, Range<usize>)]) -> Vec<Diagnostic> {
    let word = |k: usize| tokens.get(k).filter(|(t, _)| *t == Tok::Ident).map(|(_, r)| &text[r.clone()]);
    let mut out = Vec::new();
    for k in 0..tokens.len() {
        let Some(keyword) = word(k).filter(|w| matches!(*w, "class" | "struct" | "union" | "enum")) else { continue };
        // `enum class`, `template <class T>`, `friend class X;`, параметр `struct A* a`.
        if keyword == "class" && (word(k.wrapping_sub(1)) == Some("enum") || matches!(tokens.get(k.wrapping_sub(1)).map(|t| t.0), Some(Tok::Punct(b'<')) | Some(Tok::Punct(b',')))) {
            continue;
        }
        if matches!(tokens.get(k.wrapping_sub(1)).map(|t| t.0), Some(Tok::Open(b'(')) | Some(Tok::Punct(b','))) {
            continue;
        }
        // До `{`: только имена, `::`, `:`, `,`, `<…>` — иначе это не определение типа.
        let mut j = k + 1;
        let mut angle = 0;
        let mut definition = false;
        while j < tokens.len() {
            match tokens[j].0 {
                Tok::Ident => {}
                Tok::Punct(b':') | Tok::Punct(b',') => {}
                Tok::Punct(b'<') => angle += 1,
                Tok::Punct(b'>') if angle > 0 => angle -= 1,
                Tok::Literal | Tok::Punct(_) if angle > 0 => {}
                Tok::Open(b'(') | Tok::Close(b')') if angle > 0 => {}
                Tok::Open(b'{') => {
                    definition = true;
                    break;
                }
                _ => break,
            }
            j += 1;
        }
        if !definition {
            continue;
        }
        // Парная `}`.
        let mut depth = 0;
        let mut end = None;
        for (m, (tok, _)) in tokens.iter().enumerate().skip(j) {
            match tok {
                Tok::Open(_) => depth += 1,
                Tok::Close(_) => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(m);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { continue };
        if tokens[end].0 != Tok::Close(b'}') {
            continue;
        }
        // После `}` годится `;`, объявитель (`} name;`, `} *p;`) или атрибут; всё прочее
        // (следующее объявление, `}` объемлющего блока, конец файла) — пропущена `;`.
        let next = tokens.get(end + 1);
        let ok = match next {
            Some((Tok::Punct(b';'), _)) => true,
            Some((Tok::Ident, r)) => !matches!(&text[r.clone()], "class" | "struct" | "union" | "enum" | "namespace" | "template" | "using" | "typedef" | "static" | "void" | "int" | "inline" | "extern" | "public" | "private" | "protected"),
            Some((Tok::Punct(b'*'), _)) | Some((Tok::Punct(b'&'), _)) | Some((Tok::Open(b'['), _)) => true,
            _ => false,
        };
        if !ok {
            let close = tokens[end].1.clone();
            out.push(
                Diagnostic::error(trf!("expected `;` after {keyword} definition", keyword = keyword), close.clone())
                    .hint(trf!("a {keyword} definition ends with `}};`", keyword = keyword))
                    .fix(tr_s("Insert `;`"), close.end..close.end, ";"),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, needle: &str) -> Range<usize> {
        let s = text.find(needle).unwrap();
        s..s + needle.len()
    }

    #[test]
    fn valid_cpp_is_clean() {
        let text = r#"#include "engine_component.hpp"
#ifdef __cplusplus
extern "C" {
#endif
// comment with ' and "
template <class T, typename U = int> struct Box { T value; };
enum class Mode : int { A = 1'000, B };
struct Spinner : engine::Component {
    float speed = 90.0f; /* deg */
    const char* s = R"x(raw ") text)x";
    void update(float dt) override { angle += speed * dt; char c = '\''; (void)c; }
} spinner;
#if defined(A)
    int f() {
#else
    int f() {
#endif
        return 0;
    }
typedef struct { int x; } Point;
#ifdef __cplusplus
}
#endif
"#;
        let d = check(text);
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn broken_cpp_is_reported_with_places() {
        let text = "struct A { int x; }\nstruct B { int y; };\n";
        let d = check(text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].range, 18..19);
        assert_eq!(d[0].fixes[0].edits[0].range, 19..19);

        let text = "void f() {\n  if (x) {\n}\n";
        let d = check(text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].range, 9..10);

        let text = "#ifdef X\nint a;\n";
        let d = check(text);
        assert_eq!(d[0].message, "unterminated conditional directive");
        assert_eq!(d[0].range, at(text, "#ifdef X"));

        let text = "int a;\n#endif\n";
        assert_eq!(check(text)[0].message, "`#endif` without `#if`");

        let text = "const char* s = \"open;\nint x = (1];";
        let d = check(text);
        assert!(d.iter().any(|d| d.message.starts_with("missing terminating") && d.range == (16..17)), "{d:?}");
        assert!(d.iter().any(|d| d.range == at(text, "]")), "{d:?}");

        let text = "/* never";
        assert_eq!(check(text)[0].message, "unterminated comment");
    }
}
