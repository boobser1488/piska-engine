//! Мгновенная проверка Rust во вкладке Code (без сборки, за миллисекунды).
//!
//! 1. Свой прощающий лексер: незакрытые строки, символы и блочные комментарии, чужие
//!    символы, баланс скобок — с указанием, где открыта незакрытая скобка (как rustc).
//! 2. Если скобки сходятся — настоящий разбор файла `syn::parse_file` (позиции ошибок даёт
//!    `span-locations`): «expected `;`», лишние токены, неполные выражения. `syn` сообщает
//!    первую ошибку файла, как и парсер rustc в большинстве случаев.
//! 3. Дешёвые проверки движка на токенах: тип поля в `component!`, который отражение не
//!    умеет сохранять; `impl Component for X` без `component!`; компонент файла скрипта,
//!    которого нет в `mod.rs` каталога.
//!
//! Чего здесь нет и быть не может без компилятора: типы, имена, заимствования — это
//! `cargo check` после сохранения (`code_cargo.rs`).

use crate::i18n::{tr_s, trf};
use std::ops::Range;
use std::path::Path;

use crate::code_diagnostics::{Diagnostic, LineIndex};

/// Токен лексера Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Ident,
    Lifetime,
    Str,
    Char,
    Number,
    Comment,
    /// Открывающая скобка: `(`, `[`, `{`.
    Open(u8),
    /// Закрывающая скобка: `)`, `]`, `}`.
    Close(u8),
    Punct(u8),
}

/// Лексер Rust: токены с байтовыми диапазонами и ошибки лексики.
pub(crate) fn lex(text: &str) -> (Vec<(Tok, Range<usize>)>, Vec<Diagnostic>) {
    let bytes = text.as_bytes();
    let mut tokens = Vec::with_capacity(text.len() / 4);
    let mut errors = Vec::new();
    let mut i = 0;
    // `#!` в первой строке (shebang), но не `#![attr]`.
    if text.starts_with("#!") && !text[2..].trim_start().starts_with('[') {
        i = text.find('\n').unwrap_or(text.len());
    }
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                tokens.push((Tok::Comment, start..i));
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // Блочные комментарии в Rust вкладываются.
                let mut depth = 0usize;
                while i < bytes.len() {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                if depth > 0 {
                    errors.push(
                        Diagnostic::error(tr_s("unterminated block comment"), start..start + 2)
                            .code("E0758")
                            .hint(tr_s("close it with `*/` (block comments nest in Rust: every `/*` needs its own `*/`)")),
                    );
                }
                tokens.push((Tok::Comment, start..i.min(bytes.len())));
            }
            b'"' => {
                i = lex_quoted(bytes, i + 1, b'"', true);
                if i > bytes.len() {
                    errors.push(unterminated_string(start));
                    i = bytes.len();
                }
                tokens.push((Tok::Str, start..i));
            }
            b'r' | b'b' | b'c' if raw_or_prefixed(bytes, i).is_some() => {
                let (kind, end, ok) = raw_or_prefixed(bytes, i).expect("checked");
                if !ok {
                    errors.push(unterminated_string(start));
                }
                tokens.push((kind, start..end));
                i = end;
            }
            b'\'' => {
                // `'x'`, `'\n'`, `'\u{..}'` — символ; `'a`, `'static` — время жизни.
                let next = text[i + 1..].chars().next();
                match next {
                    Some('\\') => {
                        i = lex_quoted(bytes, i + 1, b'\'', false);
                        if i > bytes.len() || bytes.get(i - 1) != Some(&b'\'') {
                            errors.push(Diagnostic::error(tr_s("unterminated character literal"), start..start + 1).code("E0762"));
                            i = i.min(bytes.len());
                        }
                        tokens.push((Tok::Char, start..i));
                    }
                    Some(ch) if ch != '\n' => {
                        let after = i + 1 + ch.len_utf8();
                        if bytes.get(after) == Some(&b'\'') {
                            i = after + 1;
                            tokens.push((Tok::Char, start..i));
                        } else if ch.is_alphabetic() || ch == '_' {
                            i += 1;
                            while i < bytes.len() && is_ident_byte(text, i) {
                                i += text[i..].chars().next().map_or(1, char::len_utf8);
                            }
                            tokens.push((Tok::Lifetime, start..i));
                        } else {
                            errors.push(Diagnostic::error(tr_s("unterminated character literal"), start..after).code("E0762"));
                            i = after;
                            tokens.push((Tok::Char, start..i));
                        }
                    }
                    _ => {
                        errors.push(Diagnostic::error(tr_s("unterminated character literal"), start..start + 1).code("E0762"));
                        i += 1;
                    }
                }
            }
            b'(' | b'[' | b'{' => {
                tokens.push((Tok::Open(c), start..start + 1));
                i += 1;
            }
            b')' | b']' | b'}' => {
                tokens.push((Tok::Close(c), start..start + 1));
                i += 1;
            }
            b'0'..=b'9' => {
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    let exponent_sign = (b == b'+' || b == b'-') && matches!(bytes[i - 1], b'e' | b'E') && !text[start..i].starts_with("0x");
                    if b.is_ascii_alphanumeric() || b == b'_' || exponent_sign {
                        i += 1;
                    } else if b == b'.' && bytes.get(i + 1).is_some_and(|n| n.is_ascii_digit()) {
                        i += 1;
                    } else if b == b'.' && !matches!(bytes.get(i + 1), Some(b'.') | Some(b'_')) && !bytes.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic()) && !text[start..i].contains('.') {
                        // `1.` — тоже число с плавающей точкой.
                        i += 1;
                    } else {
                        break;
                    }
                }
                tokens.push((Tok::Number, start..i));
            }
            _ if is_ident_byte(text, i) => {
                while i < bytes.len() && is_ident_byte(text, i) {
                    i += text[i..].chars().next().map_or(1, char::len_utf8);
                }
                tokens.push((Tok::Ident, start..i));
            }
            _ if c.is_ascii() && c.is_ascii_punctuation() && c != b'\\' && c != b'`' => {
                tokens.push((Tok::Punct(c), start..start + 1));
                i += 1;
            }
            _ => {
                let ch = text[i..].chars().next().unwrap_or('?');
                let len = ch.len_utf8();
                if !ch.is_whitespace() {
                    errors.push(
                        Diagnostic::error(trf!("unknown start of token: `{ch}`", ch = ch.escape_default()), i..i + len)
                            .hint(lookalike_hint(ch)),
                    );
                }
                i += len;
            }
        }
    }
    (tokens, errors)
}

fn lookalike_hint(ch: char) -> String {
    let ascii = match ch {
        '“' | '”' | '«' | '»' => Some('"'),
        '‘' | '’' => Some('\''),
        '；' => Some(';'),
        '（' => Some('('),
        '）' => Some(')'),
        '−' | '–' | '—' => Some('-'),
        _ => None,
    };
    match ascii {
        Some(a) => trf!("`{ch}` looks like `{a}` but is a different character; type the ASCII one", ch = ch, a = a),
        None => tr_s("Rust source may use such characters only inside strings, characters and comments").to_string(),
    }
}

fn unterminated_string(start: usize) -> Diagnostic {
    Diagnostic::error(tr_s("unterminated double quote string"), start..start + 1)
        .code("E0765")
        .hint(tr_s("add the closing `\"`"))
}

fn is_ident_byte(text: &str, i: usize) -> bool {
    text[i..].chars().next().is_some_and(|c| c == '_' || c.is_alphanumeric())
}

/// Строка в кавычках с `\`-экранированием, начиная после открывающей кавычки.
/// Возвращает индекс после закрывающей; `> len` — строка не закрыта.
fn lex_quoted(bytes: &[u8], mut i: usize, quote: u8, multiline: bool) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' if !multiline => return i,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len() + 1
}

/// `r"…"`, `r#"…"#`, `b"…"`, `br"…"`, `b'x'`, `c"…"`, `cr#"…"#`. `None` — это не литерал
/// (просто идентификатор, начинающийся на r / b / c; в том числе `r#ident`).
fn raw_or_prefixed(bytes: &[u8], i: usize) -> Option<(Tok, usize, bool)> {
    let mut j = i;
    let first = bytes[j];
    if first == b'b' || first == b'c' {
        j += 1;
        if first == b'b' && bytes.get(j) == Some(&b'\'') {
            let end = lex_quoted(bytes, j + 1, b'\'', false);
            return Some((Tok::Char, end.min(bytes.len()), end <= bytes.len()));
        }
        if bytes.get(j) == Some(&b'"') {
            let end = lex_quoted(bytes, j + 1, b'"', true);
            return Some((Tok::Str, end.min(bytes.len()), end <= bytes.len()));
        }
    }
    if bytes.get(j) != Some(&b'r') {
        return None;
    }
    // Перед буквой r не должно быть части идентификатора (`for"` — не префикс).
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        return None;
    }
    j += 1;
    let hashes = bytes[j..].iter().take_while(|b| **b == b'#').count();
    j += hashes;
    if bytes.get(j) != Some(&b'"') {
        return None;
    }
    j += 1;
    while j < bytes.len() {
        if bytes[j] == b'"' && bytes[j + 1..].iter().take(hashes).filter(|b| **b == b'#').count() == hashes && bytes.len() >= j + 1 + hashes {
            return Some((Tok::Str, j + 1 + hashes, true));
        }
        j += 1;
    }
    Some((Tok::Str, bytes.len(), false))
}

fn closer(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        _ => b'}',
    }
}

/// Баланс скобок. Несовпадение — как в rustc: закрывающая не к той открывающей,
/// незакрытая скобка (с местом открытия), лишняя закрывающая.
pub(crate) fn check_delimiters(text: &str, tokens: &[(Tok, Range<usize>)]) -> Vec<Diagnostic> {
    let index = LineIndex::new(text);
    let line_of = |offset: usize| index.line_of(offset) + 1;
    let mut stack: Vec<(u8, Range<usize>)> = Vec::new();
    let mut out = Vec::new();
    for (tok, range) in tokens {
        match *tok {
            Tok::Open(c) => stack.push((c, range.clone())),
            Tok::Close(c) => {
                if stack.last().is_some_and(|(open, _)| closer(*open) == c) {
                    stack.pop();
                    continue;
                }
                // Закрывает что-то глубже в стеке — значит, пропущены закрывающие между.
                if let Some(depth) = stack.iter().rposition(|(open, _)| closer(*open) == c) {
                    let (open, at) = stack.last().cloned().expect("non-empty");
                    out.push(
                        Diagnostic::error(trf!("mismatched closing delimiter: `{c}`", c = c as char), range.clone())
                            .note(trf!("unclosed delimiter `{open}` on line {line}", open = open as char, line = line_of(at.start)))
                            .hint(trf!("close `{open}` with `{close}` before this `{c}`", open = open as char, close = closer(open) as char, c = c as char)),
                    );
                    stack.truncate(depth);
                } else {
                    out.push(
                        Diagnostic::error(trf!("unexpected closing delimiter: `{c}`", c = c as char), range.clone())
                            .hint(tr_s("there is no opening bracket for it: delete it or add the opening one")),
                    );
                }
            }
            _ => {}
        }
    }
    for (open, at) in stack.into_iter().rev() {
        let close = closer(open);
        out.push(
            Diagnostic::error(trf!("this `{open}` is never closed", open = open as char), at.clone())
                .hint(trf!("add the matching `{close}`", close = close as char))
                .note(tr_s("the file ends before the delimiter is closed")),
        );
    }
    out
}

/// Типы полей, которые `component!` точно не умеет отражать, и что поставить вместо.
fn unreflectable(ty: &str) -> Option<(&'static str, Option<&'static str>)> {
    let head = ty.split(['<', ' ', '[', '(']).next().unwrap_or(ty).trim();
    Some(match head {
        "f64" => ("f32", Some("f32")),
        "usize" | "u64" | "u16" | "u8" => ("u32", Some("u32")),
        "isize" | "i16" | "i8" | "i128" | "u128" => ("i32 or i64", Some("i32")),
        "char" => ("String", Some("String")),
        "Option" => ("a value with a default (for an object reference: ObjectRef)", None),
        "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet" | "VecDeque" => ("Vec<T>", None),
        "Box" | "Rc" | "Arc" | "RefCell" | "Cell" => ("a plain field", None),
        "&" | "" if ty.starts_with('&') => ("an owned type", None),
        "" if ty.starts_with('[') => ("Vec<T>", None),
        "" if ty.starts_with('(') => ("Vec2 / Vec3 or separate fields", None),
        _ => return None,
    })
}

/// Проверки движка по токенам: `component!`, `impl Component for`, `mod.rs`.
pub(crate) fn engine_lints(path: &Path, text: &str, tokens: &[(Tok, Range<usize>)]) -> Vec<Diagnostic> {
    let code: Vec<&(Tok, Range<usize>)> = tokens.iter().filter(|(t, _)| *t != Tok::Comment).collect();
    let word = |k: usize| code.get(k).filter(|(t, _)| *t == Tok::Ident).map(|(_, r)| &text[r.clone()]);
    let is = |k: usize, t: Tok| code.get(k).is_some_and(|(tok, _)| *tok == t);
    let mut out = Vec::new();
    let mut components: Vec<String> = Vec::new();
    let mut plain_structs: Vec<(String, Range<usize>)> = Vec::new();
    let mut implemented: Vec<(String, Range<usize>)> = Vec::new();
    let mut reflected: Vec<String> = Vec::new();
    let mut k = 0;
    let mut component_body_end = 0;
    while k < code.len() {
        // `component! { … pub struct Name { field: Type = default, … } }`
        if word(k) == Some("component") && is(k + 1, Tok::Punct(b'!')) && matches!(code.get(k + 2).map(|t| t.0), Some(Tok::Open(_))) {
            let body_start = k + 3;
            let mut depth = 1;
            let mut j = body_start;
            while j < code.len() && depth > 0 {
                match code[j].0 {
                    Tok::Open(_) => depth += 1,
                    Tok::Close(_) => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            component_body_end = j;
            if let Some(s) = (body_start..j).find(|&s| word(s) == Some("struct")) {
                if let Some(name) = word(s + 1) {
                    components.push(name.to_string());
                    // Поля внутри `{ … }` после имени.
                    if is(s + 2, Tok::Open(b'{')) {
                        out.extend(component_fields(text, &code, s + 3, name));
                    }
                }
            }
            k = j;
            continue;
        }
        if word(k) == Some("struct") && k >= component_body_end {
            if let Some(name) = word(k + 1) {
                plain_structs.push((name.to_string(), code[k + 1].1.clone()));
            }
        }
        if word(k) == Some("impl") {
            // `impl [<…>] Trait for Type`
            let mut j = k + 1;
            if is(j, Tok::Punct(b'<')) {
                let mut depth = 0;
                while j < code.len() {
                    match code[j].0 {
                        Tok::Punct(b'<') => depth += 1,
                        Tok::Punct(b'>') => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            // Путь трейта: последнее имя перед `for`.
            let mut trait_name = None;
            while j < code.len() && word(j) != Some("for") && !matches!(code[j].0, Tok::Open(b'{')) {
                if let Some(w) = word(j) {
                    trait_name = Some(w);
                }
                j += 1;
            }
            if word(j) == Some("for") {
                if let (Some(trait_name), Some(ty)) = (trait_name, word(j + 1)) {
                    match trait_name {
                        "Component" => implemented.push((ty.to_string(), code[j + 1].1.clone())),
                        "Reflect" | "TypeName" => reflected.push(ty.to_string()),
                        _ => {}
                    }
                }
            }
        }
        if word(k) == Some("reflect_enum") || word(k) == Some("macro_rules") {
            // Тело макросов не разбираем.
        }
        k += 1;
    }
    for (ty, range) in &implemented {
        let plain = plain_structs.iter().any(|(name, _)| name == ty);
        if plain && !components.contains(ty) && !reflected.contains(ty) {
            out.push(
                Diagnostic::warning(
                    trf!("`{ty}` implements `Component` but is not declared with `component!`", ty = ty),
                    range.clone(),
                )
                .hint(trf!(
                    "declare it as `component! {{ pub struct {ty} {{ field: Type = default, … }} }}`: that gives the reflection the editor and scenes need (otherwise `Component` does not compile: it requires `Reflect`)",
                    ty = ty
                )),
            );
        }
    }
    out.extend(mod_rs_registration(path, text, &code));
    out
}

/// Поля `component!`, начиная с токена после `{`: `[атрибуты] [pub] name: Type = default,`.
fn component_fields(text: &str, code: &[&(Tok, Range<usize>)], mut k: usize, component: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    while k < code.len() {
        // Атрибуты и документация полей.
        while code.get(k).is_some_and(|t| t.0 == Tok::Punct(b'#')) {
            k += 1;
            if code.get(k).is_some_and(|t| t.0 == Tok::Open(b'[')) {
                let mut depth = 0;
                while k < code.len() {
                    match code[k].0 {
                        Tok::Open(_) => depth += 1,
                        Tok::Close(_) => depth -= 1,
                        _ => {}
                    }
                    k += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
        let Some((tok, range)) = code.get(k).map(|t| (t.0, t.1.clone())) else { break };
        if matches!(tok, Tok::Close(b'}')) {
            break;
        }
        if tok == Tok::Ident && &text[range.clone()] == "pub" {
            k += 1;
            if code.get(k).is_some_and(|t| t.0 == Tok::Open(b'(')) {
                k += 3; // pub(crate)
            }
            continue;
        }
        // name : Type = default ,
        let field = if tok == Tok::Ident { &text[range] } else { "" };
        if !code.get(k + 1).is_some_and(|t| t.0 == Tok::Punct(b':')) {
            break;
        }
        let ty_start = k + 2;
        let mut j = ty_start;
        let mut depth = 0i32;
        while j < code.len() {
            match code[j].0 {
                Tok::Punct(b'<') | Tok::Open(_) => depth += 1,
                Tok::Punct(b'>') | Tok::Close(_) if depth > 0 => depth -= 1,
                Tok::Punct(b'=') if depth == 0 => break,
                Tok::Punct(b',') | Tok::Close(_) if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        if j > ty_start {
            let span = code[ty_start].1.start..code[j - 1].1.end;
            let ty = text[span.clone()].trim();
            if let Some((instead, replacement)) = unreflectable(ty) {
                let mut d = Diagnostic::warning(
                    trf!("field `{field}` of `{component}`: `component!` cannot store `{ty}`", field = field, component = component, ty = ty),
                    span.clone(),
                )
                .hint(trf!(
                    "reflected fields are f32, i32, u32, i64, bool, String, Vec2, Vec3, Quat, Color, Rect, Vec<T>, ObjectRef, asset handles and `reflect_enum!` enums; use {instead}",
                    instead = instead
                ));
                if let Some(replacement) = replacement {
                    if !ty.contains('<') {
                        d = d.fix(trf!("Change the type to `{replacement}`", replacement = replacement), span, replacement);
                    }
                }
                out.push(d);
            }
        }
        // До запятой после значения по умолчанию.
        let mut depth = 0i32;
        while j < code.len() {
            match code[j].0 {
                Tok::Open(_) => depth += 1,
                Tok::Close(_) if depth == 0 => return out,
                Tok::Close(_) => depth -= 1,
                Tok::Punct(b',') if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        k = j + 1;
    }
    out
}

/// Компоненты файла скрипта, которых нет в `mod.rs` каталога (его пишет редактор).
fn mod_rs_registration(path: &Path, text: &str, code: &[&(Tok, Range<usize>)]) -> Vec<Diagnostic> {
    let Some(dir) = path.parent() else { return Vec::new() };
    let module = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if module == "mod" || module == "lib" || module == "main" {
        return Vec::new();
    }
    let Ok(mod_rs) = std::fs::read_to_string(dir.join("mod.rs")) else { return Vec::new() };
    // Только `mod.rs`, который пишет редактор: в нём `app.register::<module::Name>()`.
    if !mod_rs.contains("pub fn register(") || !mod_rs.contains(&format!("pub mod {module};")) {
        return Vec::new();
    }
    let registered: Vec<String> = mod_rs
        .match_indices("register::<")
        .filter_map(|(at, m)| {
            let rest = &mod_rs[at + m.len()..];
            let path = rest.split('>').next()?.trim();
            let (m, name) = path.rsplit_once("::")?;
            (m.trim() == module).then(|| name.trim().to_string())
        })
        .collect();
    let Ok(declared) = crate::rust_scripts::declared_components(text) else { return Vec::new() };
    let mut out = Vec::new();
    for component in declared.into_iter().filter(|c| c.public) {
        if registered.contains(&component.name) {
            continue;
        }
        let at = code
            .windows(2)
            .find(|w| &text[w[0].1.clone()] == "struct" && &text[w[1].1.clone()] == component.name)
            .map(|w| w[1].1.clone())
            .unwrap_or(0..0);
        out.push(
            Diagnostic::info(
                trf!("`{name}` is not registered in `mod.rs` yet", name = component.name),
                at,
            )
            .hint(tr_s("Build (Ctrl+B) rewrites `mod.rs` of the scripts folder and registers it; until then the editor cannot add it to objects")),
        );
    }
    out
}

/// Ошибка `syn` → диагностика с точным местом.
fn syn_error(text: &str, index: &LineIndex, error: &syn::Error) -> Diagnostic {
    let span = error.span();
    let (start, end) = (span.start(), span.end());
    let from = index.offset(text, start.line.saturating_sub(1), start.column);
    let mut to = index.offset(text, end.line.saturating_sub(1), end.column);
    if to <= from {
        to = crate::code_diagnostics::ceil_char(text, from + 1);
    }
    let message = error.to_string();
    // «expected `;`» на первом токене следующей строки — показать в конце предыдущей
    // (там его и забыли) и предложить вставить.
    if message.starts_with("expected `;`") {
        let before = text[..from].trim_end();
        let gap = &text[before.len()..from];
        if gap.contains('\n') && !before.is_empty() {
            let at = before.len();
            let prev = crate::code_diagnostics::floor_char(text, at - 1);
            return Diagnostic::error(trf!("expected `;`, found `{found}`", found = &text[from..to]), prev..at)
                .hint(tr_s("add `;` at the end of the line"))
                .fix(tr_s("Insert `;`"), at..at, ";");
        }
        return Diagnostic::error(message, from..to).fix(tr_s("Insert `;`"), from..from, ";");
    }
    Diagnostic::error(message, from..to)
}

/// Полная мгновенная проверка файла Rust.
pub(crate) fn check(path: &Path, text: &str) -> Vec<Diagnostic> {
    let (tokens, mut out) = lex(text);
    out.extend(check_delimiters(text, &tokens));
    if out.is_empty() {
        let index = LineIndex::new(text);
        if let Err(error) = syn::parse_file(text) {
            out.push(syn_error(text, &index, &error));
        }
    }
    out.extend(engine_lints(path, text, &tokens));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_diagnostics::Severity;

    fn diags(text: &str) -> Vec<(String, Range<usize>)> {
        check(Path::new("x.rs"), text).into_iter().map(|d| (d.message, d.range)).collect()
    }

    fn at(text: &str, needle: &str) -> Range<usize> {
        let s = text.find(needle).unwrap();
        s..s + needle.len()
    }

    #[test]
    fn valid_code_has_no_diagnostics() {
        let text = r##"#![allow(dead_code)]
use engine::prelude::*;
component! {
    /// Doc.
    pub struct Mover {
        /// Speed.
        pub speed: f32 = 4.0,
        pub path: Vec<Vec3> = Vec::new(),
        pub(crate) target: ObjectRef = ObjectRef::NONE,
    }
}
impl Component for Mover {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let s = r#"raw "str""#; let c = '}'; let l: &'static str = "a"; let b = b'x'; let x = 1e-3 + 2.0f32 as f64;
        let _ = (s, c, l, b, x, 1..2, 1.5, 0x1F, ctx, dt, "esc \" {");
        /* nested /* comment */ { */
        'outer: loop { break 'outer; }
    }
}
"##;
        assert_eq!(diags(text), []);
    }

    #[test]
    fn delimiters_point_at_the_opener_and_the_bad_closer() {
        let text = "fn a() {\n    let v = (1, 2];\n}\n";
        let d = check(Path::new("x.rs"), text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].range, at(text, "]"));
        assert!(d[0].message.contains("mismatched closing delimiter"), "{}", d[0].message);
        assert!(d[0].notes[0].contains("line 2"));

        let text = "fn a() {\n    if x {\n}\n";
        let d = check(Path::new("x.rs"), text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].range, 7..8, "the outer `{{` is the unclosed one");
        assert!(d[0].message.contains("never closed"));

        let text = "fn a() {}\n}\n";
        assert_eq!(diags(text), [("unexpected closing delimiter: `}`".to_string(), 10..11)]);
    }

    #[test]
    fn unterminated_literals_and_comments() {
        let text = "fn a() { let s = \"open; }\n";
        let d = check(Path::new("x.rs"), text);
        assert!(d.iter().any(|d| d.message == "unterminated double quote string" && d.range == (17..18)), "{d:?}");
        let text = "/* never closed\nfn a() {}";
        let d = check(Path::new("x.rs"), text);
        assert_eq!(d[0].message, "unterminated block comment");
        assert_eq!(d[0].range, 0..2);
        let text = "fn a() { let q = “x”; }";
        let d = check(Path::new("x.rs"), text);
        assert!(d[0].message.starts_with("unknown start of token"), "{d:?}");
        assert!(d[0].hint.as_deref().unwrap().contains("`\"`"));
    }

    #[test]
    fn missing_semicolon_is_reported_at_the_end_of_the_line_with_a_fix() {
        let text = "fn a() {\n    let x = 5\n    let y = 6;\n}\n";
        let d = check(Path::new("x.rs"), text);
        assert_eq!(d.len(), 1, "{d:?}");
        let end = text.find("5\n").unwrap() + 1;
        assert_eq!(d[0].range, end - 1..end);
        assert!(d[0].message.starts_with("expected `;`"), "{}", d[0].message);
        let fix = &d[0].fixes[0];
        assert_eq!(fix.edits[0].range, end..end);
        let fixed = crate::code_diagnostics::apply_edits(text, &fix.edits);
        assert_eq!(check(Path::new("x.rs"), &fixed), []);
    }

    #[test]
    fn parser_errors_have_positions() {
        let text = "fn a() {\n    let = 3;\n}\n";
        let d = check(Path::new("x.rs"), text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Severity::Error);
        assert_eq!(d[0].range, at(text, "="));
    }

    #[test]
    fn engine_lints_catch_component_mistakes() {
        let text = "component! {\n    pub struct Gun {\n        pub rate: f64 = 2.0,\n        pub ammo: usize = 3,\n        pub target: Option<String> = None,\n    }\n}\nstruct Plain { x: f32 }\nimpl Component for Plain {}\n";
        let d = check(Path::new("x.rs"), text);
        let messages: Vec<&str> = d.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(d.len(), 4, "{messages:?}");
        assert_eq!(d[0].range, at(text, "f64"));
        assert_eq!(d[0].fixes[0].edits[0].text, "f32");
        assert_eq!(d[1].range, at(text, "usize"));
        assert!(d[2].message.contains("Option<String>"));
        assert!(d[2].fixes.is_empty());
        assert!(d[3].message.contains("`Plain` implements `Component`"));
        assert!(d.iter().all(|d| d.severity == Severity::Warning));
    }

    #[test]
    fn script_components_missing_from_mod_rs_are_reported() {
        let dir = std::env::temp_dir().join(format!("piska_lint_mod_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mod.rs"), "pub mod gun;\npub fn register(app: &mut engine::prelude::App) {\n    app.register::<gun::Gun>();\n}\n").unwrap();
        let text = "component! { pub struct Gun { pub rate: f32 = 2.0 } }\ncomponent! { pub struct Laser { pub rate: f32 = 2.0 } }\n";
        let d = check(&dir.join("gun.rs"), text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Severity::Info);
        assert_eq!(d[0].range, at(text, "Laser"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
