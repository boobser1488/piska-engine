//! Лёгкий индекс символов для навигации и подсказок вкладки Code — по тексту, без
//! компилятора и без вывода типов (это честный «индекс», а не IntelliSense):
//!
//! * Rust: `fn`, `struct`, `enum`, `trait`, `impl`, `mod`, `const`, `static`, `type`,
//!   `macro_rules!`, `component! { struct … }` и поля компонентов;
//! * C++: классы и структуры, функции, макросы вида `PISKA_COMPONENT(Name)`;
//! * Scenario: функции и переменные верхнего уровня;
//! * API движка — из исходников движка, встроенных в редактор при сборке (`include_str!`):
//!   методы `Ctx`, типы прелюдии, их методы и документация.
//!
//! Go to Definition ищет по имени (сначала тот же файл, потом проект, потом движок);
//! Find All References — поиск слова целиком вне комментариев и строк.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::code_check_rust::{lex, Tok};

/// Вид символа.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Kind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Const,
    Type,
    Macro,
    Component,
    Field,
    Class,
    Variable,
}

/// Вид символа или подсказки для показа (переведён; ключевые слова языка — как есть).
pub(crate) fn kind_text(kind: &'static str) -> &'static str {
    // i18n-keys: "method" "field" "component" "class" "keyword" "snippet" "hook" "word"
    crate::i18n::tr_s(kind)
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Function => "fn",
            Kind::Method => "method",
            Kind::Struct => "struct",
            Kind::Enum => "enum",
            Kind::Trait => "trait",
            Kind::Impl => "impl",
            Kind::Module => "mod",
            Kind::Const => "const",
            Kind::Type => "type",
            Kind::Macro => "macro",
            Kind::Component => "component",
            Kind::Field => "field",
            Kind::Class => "class",
            Kind::Variable => "let",
        }
    }
}

/// Символ файла.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Symbol {
    pub name: String,
    pub kind: Kind,
    /// Тип или `impl`, внутри которого объявлен (для методов и полей).
    pub container: Option<String>,
    pub path: PathBuf,
    /// Байты имени в тексте файла.
    pub range: Range<usize>,
    /// Строка (с единицы).
    pub line: usize,
    /// Однострочная сигнатура: `pub fn spawn(&mut self, prefab: &str) -> GameObjectId`.
    pub signature: String,
    /// Первая строка документации (`///`).
    pub doc: String,
}

fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if out.len() > 160 {
        let cut = crate::code_diagnostics::floor_char(&out, 157);
        out.truncate(cut);
        out.push('…');
    }
    out
}

/// Символы файла Rust.
pub(crate) fn rust_symbols(path: &Path, text: &str) -> Vec<Symbol> {
    let (tokens, _) = lex(text);
    let mut out = Vec::new();
    let lines = crate::code_diagnostics::LineIndex::new(text);
    let line_of = |offset: usize| lines.line_of(offset) + 1;
    // Документация: подряд идущие `///` перед токеном.
    let doc_before = |k: usize| -> String {
        let mut lines = Vec::new();
        let mut j = k;
        while j > 0 {
            j -= 1;
            let (tok, r) = &tokens[j];
            match tok {
                Tok::Comment if text[r.clone()].starts_with("///") => lines.push(text[r.clone()].trim_start_matches('/').trim().to_string()),
                Tok::Punct(b'#') | Tok::Open(b'[') | Tok::Close(b']') | Tok::Ident | Tok::Punct(_) | Tok::Str if lines.is_empty() && !matches!(tok, Tok::Punct(b';') | Tok::Punct(b'}')) => {
                    // Атрибуты и `pub` между документацией и именем.
                    if matches!(tok, Tok::Ident) && !matches!(&text[r.clone()], "pub" | "crate" | "unsafe" | "async" | "const" | "extern") {
                        break;
                    }
                }
                _ => break,
            }
        }
        lines.reverse();
        lines.into_iter().find(|l| !l.is_empty()).unwrap_or_default()
    };
    // Стек контейнеров: (имя, глубина скобок, на которой он открыт).
    let mut containers: Vec<(String, i32)> = Vec::new();
    let mut depth = 0i32;
    let mut pending_container: Option<String> = None;
    let word = |k: usize| tokens.get(k).filter(|(t, _)| *t == Tok::Ident).map(|(_, r)| &text[r.clone()]);
    let mut k = 0;
    while k < tokens.len() {
        let (tok, range) = tokens[k].clone();
        match tok {
            Tok::Open(b'{') => {
                depth += 1;
                if let Some(name) = pending_container.take() {
                    containers.push((name, depth));
                }
            }
            Tok::Close(b'}') => {
                if containers.last().is_some_and(|(_, d)| *d == depth) {
                    containers.pop();
                }
                depth -= 1;
            }
            Tok::Punct(b';') => pending_container = None,
            Tok::Ident => {
                let w = &text[range.clone()];
                let kind = match w {
                    "fn" => Some(if containers.is_empty() { Kind::Function } else { Kind::Method }),
                    "struct" => Some(Kind::Struct),
                    "enum" => Some(Kind::Enum),
                    "trait" => Some(Kind::Trait),
                    "mod" => Some(Kind::Module),
                    "const" | "static" if word(k + 1) != Some("fn") && word(k + 1) != Some("unsafe") => Some(Kind::Const),
                    "type" => Some(Kind::Type),
                    _ => None,
                };
                if w == "macro_rules" && matches!(tokens.get(k + 1).map(|t| t.0), Some(Tok::Punct(b'!'))) {
                    if let Some(name) = word(k + 2) {
                        let r = tokens[k + 2].1.clone();
                        out.push(Symbol { name: name.to_string(), kind: Kind::Macro, container: None, path: path.to_path_buf(), line: line_of(r.start), range: r, signature: format!("macro_rules! {name}"), doc: doc_before(k) }); // i18n-skip: код
                    }
                }
                if w == "component" && matches!(tokens.get(k + 1).map(|t| t.0), Some(Tok::Punct(b'!'))) {
                    // `component! { … struct Name { field: Type = default, … } }`
                    let mut j = k + 2;
                    while j < tokens.len() && word(j) != Some("struct") {
                        j += 1;
                    }
                    if let Some(name) = word(j + 1) {
                        let r = tokens[j + 1].1.clone();
                        let doc = Some(doc_before(j)).filter(|d| !d.is_empty()).unwrap_or_else(|| doc_before(k));
                        out.push(Symbol { name: name.to_string(), kind: Kind::Component, container: None, path: path.to_path_buf(), line: line_of(r.start), range: r, signature: format!("component! {{ struct {name} }}"), doc }); // i18n-skip: код
                        // Поля.
                        let mut m = j + 2;
                        let mut d = 0;
                        while m < tokens.len() {
                            match tokens[m].0 {
                                Tok::Open(_) => d += 1,
                                Tok::Close(_) => {
                                    d -= 1;
                                    if d == 0 {
                                        break;
                                    }
                                }
                                Tok::Ident if d == 1 && matches!(tokens.get(m + 1).map(|t| t.0), Some(Tok::Punct(b':'))) && !matches!(tokens.get(m + 2).map(|t| t.0), Some(Tok::Punct(b':'))) => {
                                    let fr = tokens[m].1.clone();
                                    let end = (m + 2..tokens.len()).find(|&e| matches!(tokens[e].0, Tok::Punct(b'=') | Tok::Punct(b','))).unwrap_or(m + 2);
                                    let ty = if end > m + 2 { text[tokens[m + 2].1.start..tokens[end - 1].1.end].trim().to_string() } else { String::new() };
                                    let fname = &text[fr.clone()];
                                    out.push(Symbol { name: fname.to_string(), kind: Kind::Field, container: Some(name.to_string()), path: path.to_path_buf(), line: line_of(fr.start), range: fr, signature: format!("{fname}: {ty}"), doc: doc_before(m) });
                                }
                                _ => {}
                            }
                            m += 1;
                        }
                        k = m;
                    }
                }
                if w == "impl" {
                    // `impl [<…>] [Trait for] Type {`
                    let mut j = k + 1;
                    let mut names = Vec::new();
                    let mut angle = 0;
                    while j < tokens.len() && !matches!(tokens[j].0, Tok::Open(b'{') | Tok::Punct(b';')) {
                        match tokens[j].0 {
                            Tok::Punct(b'<') => angle += 1,
                            Tok::Punct(b'>') => angle -= 1,
                            Tok::Ident if angle == 0 => names.push(&text[tokens[j].1.clone()]),
                            _ => {}
                        }
                        j += 1;
                    }
                    let ty = names.iter().rposition(|n| *n == "for").map_or_else(|| names.last().copied(), |p| names.get(p + 1).copied());
                    if let Some(ty) = ty.filter(|t| *t != "where") {
                        pending_container = Some(ty.to_string());
                    }
                }
                if let Some(kind) = kind {
                    if let Some(name) = word(k + 1).filter(|n| *n != "mut") {
                        let r = tokens[k + 1].1.clone();
                        // Сигнатура — до `{` или `;` на глубине 0.
                        let start = (0..k).rev().take_while(|&j| matches!(&text[tokens[j].1.clone()], "pub" | "crate" | "(" | ")" | "async" | "unsafe" | "const" | "extern")).last().map_or(range.start, |j| tokens[j].1.start);
                        let mut end = k + 1;
                        let mut d = 0;
                        while end < tokens.len() {
                            match tokens[end].0 {
                                Tok::Open(b'{') if d == 0 => break,
                                Tok::Punct(b';') if d == 0 => break,
                                Tok::Open(_) => d += 1,
                                Tok::Close(_) => d -= 1,
                                _ => {}
                            }
                            end += 1;
                        }
                        let stop = tokens.get(end.saturating_sub(1)).map_or(r.end, |t| t.1.end).max(r.end);
                        let signature = one_line(&text[start..stop]);
                        let container = containers.last().map(|(n, _)| n.clone());
                        out.push(Symbol { name: name.to_string(), kind, container, path: path.to_path_buf(), line: line_of(r.start), range: r, signature, doc: doc_before(k) });
                        if matches!(kind, Kind::Struct | Kind::Enum | Kind::Trait | Kind::Module) {
                            pending_container = Some(name.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
        k += 1;
    }
    out
}

/// Символы файла C++ (простая лексика: комментарии и строки пропускаются).
pub(crate) fn cpp_symbols(path: &Path, text: &str) -> Vec<Symbol> {
    let bytes = text.as_bytes();
    let mut idents: Vec<(Range<usize>, u8)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = text[i + 2..].find("*/").map_or(bytes.len(), |p| i + 4 + p);
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != c && bytes[i] != b'\n' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            idents.push((s..i, 0));
        } else {
            if !c.is_ascii_whitespace() {
                idents.push((i..i + 1, c));
            }
            i += 1;
        }
    }
    let lines = crate::code_diagnostics::LineIndex::new(text);
    let line_of = |offset: usize| lines.line_of(offset) + 1;
    let word = |k: usize| idents.get(k).filter(|(_, p)| *p == 0).map(|(r, _)| &text[r.clone()]);
    let punct = |k: usize| idents.get(k).map_or(0, |(_, p)| *p);
    let mut out = Vec::new();
    let mut depth = 0;
    let mut class: Vec<(String, i32)> = Vec::new();
    let mut pending: Option<String> = None;
    for k in 0..idents.len() {
        match punct(k) {
            b'{' => {
                depth += 1;
                if let Some(name) = pending.take() {
                    class.push((name, depth));
                }
            }
            b'}' => {
                if class.last().is_some_and(|(_, d)| *d == depth) {
                    class.pop();
                }
                depth -= 1;
            }
            b';' => pending = None,
            _ => {}
        }
        let Some(w) = word(k) else { continue };
        if matches!(w, "class" | "struct") && word(k.wrapping_sub(1)) != Some("enum") {
            if let Some(name) = word(k + 1) {
                // Определение: до `{` нет `;` и `(`.
                let mut j = k + 2;
                while j < idents.len() && !matches!(punct(j), b'{' | b';' | b'(' | b')') {
                    j += 1;
                }
                if punct(j) == b'{' {
                    let r = idents[k + 1].0.clone();
                    let signature = one_line(&text[idents[k].0.start..idents[j].0.start]);
                    out.push(Symbol { name: name.to_string(), kind: Kind::Class, container: None, path: path.to_path_buf(), line: line_of(r.start), range: r, signature, doc: String::new() });
                    pending = Some(name.to_string());
                }
            }
        } else if w.contains("COMPONENT") && punct(k + 1) == b'(' {
            if let Some(name) = word(k + 2) {
                let r = idents[k + 2].0.clone();
                out.push(Symbol { name: name.to_string(), kind: Kind::Component, container: None, path: path.to_path_buf(), line: line_of(r.start), range: r, signature: format!("{w}({name})"), doc: String::new() });
            }
        } else if punct(k + 1) == b'(' && !matches!(w, "if" | "for" | "while" | "switch" | "return" | "catch" | "sizeof" | "defined" | "decltype" | "static_assert") {
            // Функция: `name(…)` и затем `{` (или `const {`, `override {`) на внешнем уровне.
            let mut j = k + 2;
            let mut d = 1;
            while j < idents.len() && d > 0 {
                match punct(j) {
                    b'(' => d += 1,
                    b')' => d -= 1,
                    _ => {}
                }
                j += 1;
            }
            while j < idents.len() && word(j).is_some_and(|w| matches!(w, "const" | "override" | "noexcept" | "final")) {
                j += 1;
            }
            if punct(j) == b'{' && (depth == 0 || class.last().is_some_and(|(_, cd)| *cd == depth)) && k > 0 && (word(k - 1).is_some() || matches!(punct(k - 1), b'*' | b'&' | b'>' | b':')) {
                let r = idents[k].0.clone();
                let line_start = text[..r.start].rfind('\n').map_or(0, |p| p + 1);
                let signature = one_line(&text[line_start..idents[j].0.start]);
                let container = class.last().map(|(n, _)| n.clone());
                let kind = if container.is_some() { Kind::Method } else { Kind::Function };
                out.push(Symbol { name: w.to_string(), kind, container, path: path.to_path_buf(), line: line_of(r.start), range: r, signature, doc: String::new() });
            }
        }
    }
    out
}

/// Символы сценария: функции и переменные верхнего уровня (строка без отступа).
pub(crate) fn scenario_symbols(path: &Path, text: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut offset = 0;
    for (n, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let rest = trimmed.strip_prefix("local ").unwrap_or(trimmed);
        let (kind, after) = if let Some(after) = rest.strip_prefix("function ") {
            (Some(Kind::Function), after)
        } else if indent == 0 {
            match ["let ", "const ", "var "].iter().find_map(|k| trimmed.strip_prefix(k)) {
                Some(after) => (Some(Kind::Variable), after),
                None if trimmed.starts_with("local ") => (Some(Kind::Variable), rest),
                None => (None, ""),
            }
        } else {
            (None, "")
        };
        if let Some(kind) = kind {
            let name_len = after.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == ':')).unwrap_or(after.len());
            if name_len > 0 {
                let start = offset + (line.len() - after.len());
                let name = &after[..name_len];
                out.push(Symbol { name: name.to_string(), kind, container: None, path: path.to_path_buf(), line: n + 1, range: start..start + name_len, signature: one_line(trimmed), doc: String::new() });
            }
        }
        offset += line.len() + 1;
    }
    out
}

/// Символы файла по расширению.
pub(crate) fn symbols(path: &Path, text: &str) -> Vec<Symbol> {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => rust_symbols(path, text),
        "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => cpp_symbols(path, text),
        "scenario" => scenario_symbols(path, text),
        _ => Vec::new(),
    }
}

/// Исходники движка, встроенные при сборке: (путь в рабочем пространстве, текст).
const ENGINE_SOURCES: &[(&str, &str)] = &[
    ("crates/engine-core/src/component.rs", include_str!("../../engine-core/src/component.rs")),
    ("crates/engine-core/src/script.rs", include_str!("../../engine-core/src/script.rs")),
    ("crates/engine-core/src/input.rs", include_str!("../../engine-core/src/input.rs")),
    ("crates/engine-core/src/mathf.rs", include_str!("../../engine-core/src/mathf.rs")),
    ("crates/engine-core/src/timer.rs", include_str!("../../engine-core/src/timer.rs")),
    ("crates/engine-core/src/tween.rs", include_str!("../../engine-core/src/tween.rs")),
    ("crates/engine-core/src/random.rs", include_str!("../../engine-core/src/random.rs")),
    ("crates/engine-core/src/object.rs", include_str!("../../engine-core/src/object.rs")),
    ("crates/engine-core/src/objref.rs", include_str!("../../engine-core/src/objref.rs")),
    ("crates/engine-core/src/scene.rs", include_str!("../../engine-core/src/scene.rs")),
    ("crates/engine-core/src/scenes.rs", include_str!("../../engine-core/src/scenes.rs")),
    ("crates/engine-core/src/schedule.rs", include_str!("../../engine-core/src/schedule.rs")),
    ("crates/engine-core/src/state.rs", include_str!("../../engine-core/src/state.rs")),
    ("crates/engine-core/src/save.rs", include_str!("../../engine-core/src/save.rs")),
    ("crates/engine-core/src/world.rs", include_str!("../../engine-core/src/world.rs")),
];

/// Индекс API движка (строится один раз). Только `pub` элементы.
pub(crate) fn engine_index() -> &'static [Symbol] {
    static INDEX: std::sync::OnceLock<Vec<Symbol>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut all = Vec::new();
        for (path, text) in ENGINE_SOURCES {
            all.extend(rust_symbols(Path::new(path), text));
        }
        let public = |s: &Symbol| s.signature.starts_with("pub ") && !s.signature.starts_with("pub(crate)") && !s.signature.starts_with("pub(super)");
        // Методы публичных трейтов (хуки `Component`) не пишут `pub`, но они — API.
        let traits: Vec<String> = all.iter().filter(|s| s.kind == Kind::Trait && public(s)).map(|s| s.name.clone()).collect();
        all.into_iter()
            .filter(|s| public(s) || (s.kind == Kind::Method && s.container.as_ref().is_some_and(|c| traits.contains(c))))
            .collect()
    })
}

/// Где лежит исходник движка на этой машине (для Go to Definition на символ движка).
pub(crate) fn engine_source_path(relative: &Path) -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(relative);
    path.is_file().then_some(path)
}

/// Встроенный текст исходника движка.
pub(crate) fn engine_source_text(relative: &Path) -> Option<&'static str> {
    ENGINE_SOURCES.iter().find(|(p, _)| Path::new(p) == relative).map(|(_, t)| *t)
}

/// Слово (идентификатор) вокруг байта `at`.
pub(crate) fn ident_at(text: &str, at: usize) -> Option<Range<usize>> {
    let r = crate::code_diagnostics::word_at(text, at);
    let w = &text[r.clone()];
    (!w.is_empty() && w.chars().all(|c| c.is_alphanumeric() || c == '_') && !w.starts_with(|c: char| c.is_ascii_digit())).then_some(r)
}

/// Определения имени: сначала в этом файле, затем в проекте, затем в движке.
pub(crate) fn definitions<'a>(name: &str, file: &'a [Symbol], project: &'a [Symbol]) -> Vec<&'a Symbol> {
    let defining = |s: &&Symbol| s.name == name && s.kind != Kind::Impl;
    let local: Vec<&Symbol> = file.iter().filter(defining).collect();
    if !local.is_empty() {
        return local;
    }
    let found: Vec<&Symbol> = project.iter().filter(defining).collect();
    if !found.is_empty() {
        return found;
    }
    Vec::new()
}

/// Определения имени в API движка.
pub(crate) fn engine_definitions(name: &str) -> Vec<&'static Symbol> {
    engine_index().iter().filter(|s| s.name == name && s.kind != Kind::Impl).collect()
}

/// Найти все вхождения слова `name` (целиком) вне комментариев и строк — по тексту, не
/// по смыслу: одноимённые разные сущности тоже попадут.
pub(crate) fn references(text: &str, name: &str, language: crate::code::Language) -> Vec<Range<usize>> {
    use crate::code::Language;
    if name.is_empty() {
        return Vec::new();
    }
    let code_only: Vec<Range<usize>> = match language {
        Language::Rust => lex(text).0.into_iter().filter(|(t, _)| *t == Tok::Ident).map(|(_, r)| r).collect(),
        _ => {
            // C++ / Scenario / прочее: слова вне `//`, `--`, `/* */` и строк.
            let mut out = Vec::new();
            let bytes = text.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let c = bytes[i];
                let line_comment = (c == b'/' && bytes.get(i + 1) == Some(&b'/')) || (language == Language::Scenario && c == b'-' && bytes.get(i + 1) == Some(&b'-'));
                if line_comment {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    i = text[i + 2..].find("*/").map_or(bytes.len(), |p| i + 4 + p);
                } else if c == b'"' || c == b'\'' || (c == b'`' && language == Language::Scenario) {
                    i += 1;
                    while i < bytes.len() && bytes[i] != c {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                    i += 1;
                } else if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
                    let s = i;
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) {
                        i += 1;
                    }
                    out.push(s..i);
                } else {
                    i += 1;
                }
            }
            out
        }
    };
    code_only.into_iter().filter(|r| r.end <= text.len() && &text[r.clone()] == name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_symbols_cover_items_components_and_fields() {
        let text = "use x;\n/// Moves things.\ncomponent! {\n    pub struct Mover {\n        /// Speed.\n        pub speed: f32 = 4.0,\n        pub target: ObjectRef = ObjectRef::NONE,\n    }\n}\nimpl Component for Mover {\n    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {}\n}\npub fn helper(a: i32) -> i32 { a }\nstruct Plain { x: f32 }\nenum Mode { A, B }\nconst MAX: usize = 3;\nmacro_rules! m { () => {} }\n";
        let s = rust_symbols(Path::new("m.rs"), text);
        let find = |n: &str| s.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n}: {s:?}"));
        assert_eq!(find("Mover").kind, Kind::Component);
        assert_eq!(find("Mover").doc, "Moves things.");
        assert_eq!(find("speed").kind, Kind::Field);
        assert_eq!(find("speed").signature, "speed: f32");
        assert_eq!(find("speed").container.as_deref(), Some("Mover"));
        assert_eq!(find("on_update").kind, Kind::Method);
        assert_eq!(find("on_update").container.as_deref(), Some("Mover"));
        assert_eq!(find("helper").signature, "pub fn helper(a: i32) -> i32");
        assert_eq!(find("helper").line, 13);
        assert_eq!(find("Plain").kind, Kind::Struct);
        assert_eq!(find("Mode").kind, Kind::Enum);
        assert_eq!(find("MAX").kind, Kind::Const);
        assert_eq!(find("m").kind, Kind::Macro);
        assert_eq!(&text[find("helper").range.clone()], "helper");
    }

    #[test]
    fn cpp_and_scenario_symbols() {
        let text = "// c\nstruct Spinner : engine::Component {\n    float speed = 90.0f;\n    void update(float dt) override { angle += speed; }\n};\nPISKA_COMPONENT(Spinner)\nstatic int helper(int a) { return a; }\nvoid decl(int);\n";
        let s = cpp_symbols(Path::new("a.cpp"), text);
        let names: Vec<(&str, Kind)> = s.iter().map(|x| (x.name.as_str(), x.kind)).collect();
        assert!(names.contains(&("Spinner", Kind::Class)), "{names:?}");
        assert!(names.contains(&("update", Kind::Method)), "{names:?}");
        assert!(names.contains(&("Spinner", Kind::Component)), "{names:?}");
        assert!(names.contains(&("helper", Kind::Function)), "{names:?}");
        assert!(!names.iter().any(|(n, _)| *n == "decl"), "declarations are not definitions");
        let text = "let lives = 3\nfunction start()\n  local x = 1\nend\nlocal function hit(n) { }\n";
        let s = scenario_symbols(Path::new("a.scenario"), text);
        let names: Vec<(&str, Kind)> = s.iter().map(|x| (x.name.as_str(), x.kind)).collect();
        assert_eq!(names, [("lives", Kind::Variable), ("start", Kind::Function), ("hit", Kind::Function)]);
    }

    #[test]
    fn the_engine_index_knows_ctx() {
        let index = engine_index();
        assert!(index.len() > 100, "{}", index.len());
        let spawn = index.iter().filter(|s| s.container.as_deref() == Some("Ctx")).count();
        assert!(spawn > 5, "Ctx methods are indexed");
        assert!(index.iter().any(|s| s.name == "Timer" && s.kind == Kind::Struct));
        assert!(engine_source_path(Path::new("crates/engine-core/src/component.rs")).is_some());
    }

    #[test]
    fn references_skip_comments_and_strings() {
        let text = "let speed = 1; // speed\nlet s = \"speed\"; speed += speedy;";
        let refs = references(text, "speed", crate::code::Language::Rust);
        assert_eq!(refs.len(), 2);
        let refs = references("local speed = 1 -- speed\nspeed = 2", "speed", crate::code::Language::Scenario);
        assert_eq!(refs.len(), 2);
    }
}
