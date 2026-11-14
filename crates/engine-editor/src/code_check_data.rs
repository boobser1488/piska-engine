//! Мгновенная проверка остальных файлов вкладки Code тем же разборщиком, которым их
//! читает движок: Scenario (компилятор `engine_scenario`), RON / `.scene` / `.prefab` /
//! `.material` (`ron` + типы сцен и материалов), JSON (`serde_json`), TOML (`toml_edit`),
//! WGSL (`naga`: разбор и валидация). Для сцен и материалов ещё и схема: неизвестный тип
//! компонента или поле — с «did you mean».
//!
//! Здесь же — выбор проверки по расширению файла ([`check_file`]).

use crate::i18n::{tr_s, trf};
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::code_diagnostics::{ceil_char, did_you_mean, word_at, Diagnostic, LineIndex, Severity};

/// Что проверка знает о проекте: компоненты реестра и их поля.
#[derive(Clone, Debug, Default)]
pub(crate) struct Schema {
    /// Тип компонента → имена отражённых полей.
    pub components: BTreeMap<String, Vec<String>>,
}

/// Какой проверкой проверять файл.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckKind {
    Rust,
    Cpp,
    Scenario,
    Ron,
    Scene,
    Prefab,
    Material,
    Json,
    Toml,
    Wgsl,
}

impl CheckKind {
    pub fn of(path: &Path) -> Option<Self> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => CheckKind::Rust,
            "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" | "hh" | "hxx" => CheckKind::Cpp,
            "scenario" => CheckKind::Scenario,
            "ron" | "meta" => CheckKind::Ron,
            "scene" => CheckKind::Scene,
            "prefab" => CheckKind::Prefab,
            "material" => CheckKind::Material,
            "json" => CheckKind::Json,
            "toml" => CheckKind::Toml,
            "wgsl" => CheckKind::Wgsl,
            _ => return None,
        })
    }

    /// Имя проверки для строки состояния.
    pub fn label(self) -> &'static str {
        match self {
            CheckKind::Rust => tr_s("Rust syntax"),
            CheckKind::Cpp => tr_s("C++ syntax"),
            CheckKind::Scenario => tr_s("Scenario compiler"),
            CheckKind::Ron => "RON",
            CheckKind::Scene => tr_s("scene schema"),
            CheckKind::Prefab => tr_s("prefab schema"),
            CheckKind::Material => tr_s("material schema"),
            CheckKind::Json => "JSON",
            CheckKind::Toml => "TOML",
            CheckKind::Wgsl => tr_s("WGSL validator"),
        }
    }
}

/// Проверить файл по его расширению. Пустой список — ошибок нет (или проверки нет).
pub(crate) fn check_file(path: &Path, text: &str, schema: &Schema) -> Vec<Diagnostic> {
    let mut out = match CheckKind::of(path) {
        None => Vec::new(),
        Some(CheckKind::Rust) => crate::code_check_rust::check(path, text),
        Some(CheckKind::Cpp) => crate::code_check_cpp::check(text),
        Some(CheckKind::Scenario) => scenario(path, text),
        Some(CheckKind::Ron) => ron_value(text),
        Some(CheckKind::Scene) => {
            let mut out = typed_ron::<engine_core::serialize::SceneData>(text);
            if out.is_empty() {
                out = scene_schema(text, schema);
            }
            out
        }
        Some(CheckKind::Prefab) => {
            let mut out = typed_ron::<engine_core::serialize::NodeData>(text);
            if out.is_empty() {
                out = scene_schema(text, schema);
            }
            out
        }
        Some(CheckKind::Material) => material(text),
        Some(CheckKind::Json) => json(text),
        Some(CheckKind::Toml) => toml(text),
        Some(CheckKind::Wgsl) => wgsl(path, text),
    };
    let index = LineIndex::new(text);
    for d in &mut out {
        d.range.start = d.range.start.min(text.len());
        d.range.end = d.range.end.clamp(d.range.start, text.len());
        d.locate(&index, text);
    }
    out.sort_by_key(|d| (d.range.start, d.severity));
    out
}

/// Диапазон токена под позицией (строка и столбец с единицы, столбец — в символах).
fn token_at(text: &str, index: &LineIndex, line: usize, column: usize) -> Range<usize> {
    let offset = index.offset(text, line.saturating_sub(1), column.saturating_sub(1));
    if offset >= text.len() {
        // Конец файла: последний непробельный символ.
        let end = text.trim_end().len();
        let start = text[..end].char_indices().next_back().map_or(0, |(i, _)| i);
        return start..end.max(start);
    }
    let range = word_at(text, offset);
    if range.is_empty() {
        offset..ceil_char(text, offset + 1)
    } else {
        range
    }
}

fn scenario(path: &Path, text: &str) -> Vec<Diagnostic> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match engine_scenario::Script::compile(&name, text) {
        Ok(_) => Vec::new(),
        Err(error) => {
            let index = LineIndex::new(text);
            let range = token_at(text, &index, error.pos.line as usize, error.pos.col as usize);
            let mut d = Diagnostic::error(error.message.clone(), range);
            if let Some(hint) = &error.hint {
                d = d.hint(hint.clone());
            }
            vec![d]
        }
    }
}

fn ron_error(text: &str, error: &ron::error::SpannedError) -> Diagnostic {
    let index = LineIndex::new(text);
    let range = token_at(text, &index, error.position.line, error.position.col);
    Diagnostic::error(error.code.to_string(), range)
}

fn ron_value(text: &str) -> Vec<Diagnostic> {
    match ron::from_str::<ron::Value>(text) {
        Ok(_) => Vec::new(),
        Err(error) => vec![ron_error(text, &error)],
    }
}

fn typed_ron<T: serde::de::DeserializeOwned>(text: &str) -> Vec<Diagnostic> {
    match ron::from_str::<T>(text) {
        Ok(_) => Vec::new(),
        Err(error) => vec![ron_error(text, &error)],
    }
}

/// Строки и ключи RON: (диапазон без кавычек, текст), с пропуском комментариев.
fn ron_strings(text: &str) -> Vec<(Range<usize>, &str, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(bytes.len(), |p| i + 2 + p + 2);
            }
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                let end = i.min(bytes.len());
                out.push((start..end, &text[start..end], depth));
                i += 1;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Сцена / префаб: `kind: "Тип"` и ключи `fields: { "поле": … }` против реестра.
fn scene_schema(text: &str, schema: &Schema) -> Vec<Diagnostic> {
    if schema.components.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let strings = ron_strings(text);
    let mut k = 0;
    while k < strings.len() {
        let (range, value, depth) = strings[k].clone();
        let before = text[..range.start.saturating_sub(1)].trim_end();
        if !(before.ends_with(':') && before[..before.len() - 1].trim_end().ends_with("kind")) {
            k += 1;
            continue;
        }
        let fields = schema.components.get(value);
        if fields.is_none() {
            let names = schema.components.keys().map(String::as_str);
            let mut d = Diagnostic::warning(trf!("unknown component type `{kind}`", kind = value), range.clone()).hint(tr_s(
                "this editor has no such component registered: it is misspelt, or it is a script that is not built yet (Build, Ctrl+B)",
            ));
            if let Some(better) = did_you_mean(value, names) {
                d = d.note(trf!("did you mean `{better}`?", better = better)).fix(trf!("Change to `{better}`", better = better), range.clone(), better);
            }
            out.push(d);
        }
        // Ключи карты `fields: { … }` этого компонента — строки на глубину ниже, до
        // следующего `kind`.
        let mut j = k + 1;
        while j < strings.len() {
            let (key_range, key, key_depth) = strings[j].clone();
            let before = text[..key_range.start.saturating_sub(1)].trim_end();
            if before.ends_with(':') && before[..before.len() - 1].trim_end().ends_with("kind") {
                break;
            }
            let after = text[(key_range.end + 1).min(text.len())..].trim_start();
            if key_depth == depth + 1 && after.starts_with(':') {
                if let Some(fields) = fields {
                    if !fields.iter().any(|f| f == key) {
                        let mut d = Diagnostic::warning(trf!("`{kind}` has no field `{field}`", kind = value, field = key), key_range.clone())
                            .hint(trf!("its fields: {list}", list = fields.join(", ")));
                        if let Some(better) = did_you_mean(key, fields.iter().map(String::as_str)) {
                            d = d.note(trf!("did you mean `{better}`?", better = better)).fix(trf!("Change to `{better}`", better = better), key_range.clone(), better);
                        }
                        out.push(d);
                    }
                }
            }
            j += 1;
        }
        k = j;
    }
    out
}

/// Поля верхнего уровня `.material` — их пишет сам движок (`Material::to_ron`).
fn material_fields() -> &'static [String] {
    static FIELDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FIELDS.get_or_init(|| {
        let text = engine_assets::Material::default().to_ron();
        let mut fields = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some((key, _)) = line.split_once(':') {
                if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    fields.push(key.to_string());
                }
            }
        }
        fields
    })
}

fn material(text: &str) -> Vec<Diagnostic> {
    if let Err(error) = engine_assets::Material::from_ron(text) {
        // `MaterialParseError` — текст ошибки ron: `line:col: message`.
        let message = error.0.clone();
        let mut parts = message.splitn(3, ':');
        let line = parts.next().and_then(|l| l.trim().parse::<usize>().ok());
        let col = parts.next().and_then(|c| c.trim().parse::<usize>().ok());
        let index = LineIndex::new(text);
        let (range, message) = match (line, col, parts.next()) {
            (Some(line), Some(col), Some(rest)) => (token_at(text, &index, line, col), rest.trim().to_string()),
            _ => (0..0, message),
        };
        return vec![Diagnostic::error(message, range)];
    }
    // Ключи верхнего уровня: `ключ:` на глубине 1.
    let fields = material_fields();
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            c if depth == 1 && (c.is_ascii_alphabetic() || c == b'_') && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let key = &text[start..i];
                if text[i..].trim_start().starts_with(':') && !fields.iter().any(|f| f == key) {
                    let mut d = Diagnostic::warning(trf!("unknown material field `{key}`", key = key), start..i)
                        .hint(tr_s("the engine ignores it; check the spelling"));
                    if let Some(better) = did_you_mean(key, fields.iter().map(String::as_str)) {
                        d = d.note(trf!("did you mean `{better}`?", better = better)).fix(trf!("Change to `{better}`", better = better), start..i, better);
                    }
                    out.push(d);
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn json(text: &str) -> Vec<Diagnostic> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(_) => Vec::new(),
        Err(error) => {
            let index = LineIndex::new(text);
            let range = token_at(text, &index, error.line(), error.column());
            let message = error.to_string();
            // `… at line 3 column 5` — место и так подчёркнуто.
            let message = message.split(" at line ").next().unwrap_or(&message).to_string();
            vec![Diagnostic::error(message, range)]
        }
    }
}

fn toml(text: &str) -> Vec<Diagnostic> {
    match text.parse::<toml_edit::DocumentMut>() {
        Ok(_) => Vec::new(),
        Err(error) => {
            let range = error.span().unwrap_or(0..0);
            let range = if range.is_empty() { range.start..ceil_char(text, range.start + 1) } else { range };
            vec![Diagnostic::error(error.message().trim().to_string(), range)]
        }
    }
}

fn wgsl(path: &Path, text: &str) -> Vec<Diagnostic> {
    // Шейдеры движка собираются из кусков: `common.wgsl` + файл. Если рядом есть
    // `common.wgsl` и файл без него не разбирается, проверяем склейку.
    let common = path
        .parent()
        .map(|dir| dir.join("common.wgsl"))
        .filter(|common| common.is_file() && path.file_name().is_some_and(|n| n != "common.wgsl"))
        .and_then(|common| std::fs::read_to_string(common).ok());
    let mut result = wgsl_once(text, 0);
    if !result.is_empty() {
        if let Some(common) = common {
            let joined = format!("{common}\n{text}");
            let offset = common.len() + 1;
            let again = wgsl_once(&joined, offset);
            if again.iter().all(|d| d.range.start >= offset) {
                result = again
                    .into_iter()
                    .map(|mut d| {
                        d.range = d.range.start - offset..d.range.end - offset;
                        for fix in &mut d.fixes {
                            fix.edits.clear();
                        }
                        d
                    })
                    .collect();
            }
        }
    }
    result
}

/// Разбор и валидация WGSL. `offset` — начало проверяемого файла в `text`.
fn wgsl_once(text: &str, offset: usize) -> Vec<Diagnostic> {
    match naga::front::wgsl::parse_str(text) {
        Err(error) => {
            let mut labels = error.labels();
            let (range, label) = labels
                .next()
                .and_then(|(span, label)| span.to_range().map(|r| (r, label.to_string())))
                .unwrap_or((offset..offset, String::new()));
            let range = if range.is_empty() { range.start..ceil_char(text, range.start + 1) } else { range };
            let mut d = Diagnostic::error(error.message().to_string(), range);
            if !label.is_empty() && label != error.message() {
                d = d.note(label);
            }
            for (span, label) in labels {
                if let Some(r) = span.to_range() {
                    if r.start >= offset {
                        let line = LineIndex::new(text).line_of(r.start) - LineIndex::new(&text[..offset]).lines() + 2;
                        d = d.note(trf!("line {line}: {label}", line = line, label = label));
                    }
                }
            }
            vec![d]
        }
        Ok(module) => {
            let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
            match validator.validate(&module) {
                Ok(_) => Vec::new(),
                Err(error) => {
                    let Some((span, label)) = error.spans().next() else { return Vec::new() };
                    let Some(range) = span.to_range() else { return Vec::new() };
                    let mut message = error.as_inner().to_string();
                    let mut source = std::error::Error::source(error.as_inner());
                    while let Some(inner) = source {
                        message.push_str(": ");
                        message.push_str(&inner.to_string());
                        source = inner.source();
                    }
                    let mut d = Diagnostic::error(message, range);
                    if !label.is_empty() {
                        d = d.note(label.clone());
                    }
                    vec![d]
                }
            }
        }
    }
}

/// Пустая схема (для файлов без реестра) — общая, чтобы не собирать её каждый раз.
pub(crate) fn empty_schema() -> Arc<Schema> {
    static EMPTY: std::sync::OnceLock<Arc<Schema>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(Schema::default())).clone()
}

/// Сводка для строки состояния: «3 errors, 1 warning».
pub(crate) fn summary(errors: usize, warnings: usize) -> String {
    match (errors, warnings) {
        (0, 0) => tr_s("No issues").to_string(),
        (e, 0) => crate::i18n::trn!("{n} error", "{n} errors", e),
        (0, w) => crate::i18n::trn!("{n} warning", "{n} warnings", w),
        (e, w) => format!("{}, {}", crate::i18n::trn!("{n} error", "{n} errors", e), crate::i18n::trn!("{n} warning", "{n} warnings", w)),
    }
}

/// Самая важная диагностика списка (для значка строки состояния).
pub(crate) fn worst(diagnostics: &[Diagnostic]) -> Option<Severity> {
    diagnostics.iter().map(|d| d.severity).min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn check(name: &str, text: &str) -> Vec<Diagnostic> {
        let mut schema = Schema::default();
        schema.components.insert("Camera".into(), vec!["ortho_size".into(), "clear_color".into()]);
        schema.components.insert("Rigidbody".into(), vec!["mass".into()]);
        check_file(&PathBuf::from(name), text, &schema)
    }

    fn at(text: &str, needle: &str) -> Range<usize> {
        let s = text.find(needle).unwrap();
        s..s + needle.len()
    }

    #[test]
    fn scenario_errors_come_from_the_real_compiler() {
        assert!(check("a.scenario", "let x = 1\nprint(x)\n").is_empty());
        let text = "let x = (1 +\nprint(x)\n";
        let d = check("a.scenario", text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].line, 2);
        let text = "function f()\n  return 1\n";
        let d = check("a.scenario", text);
        assert_eq!(d.len(), 1, "{d:?}");
    }

    #[test]
    fn data_files_use_their_parsers() {
        assert!(check("a.json", "{\"a\": [1, 2]}").is_empty());
        let text = "{\"a\": [1, 2,]}";
        let d = check("a.json", text);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].range, at(text, "]"));

        assert!(check("Cargo.toml", "[package]\nname = \"x\"\n").is_empty());
        let text = "[package]\nname = \"x\nversion = 1\n";
        let d = check("Cargo.toml", text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].line, 2);

        assert!(check("a.ron", "(a: 1, b: [2, 3])").is_empty());
        let text = "(a: 1, b: [2, 3)";
        let d = check("a.ron", text);
        assert_eq!(d.len(), 1, "{d:?}");
    }

    #[test]
    fn scenes_are_checked_against_the_registry() {
        let text = "(\n    name: \"L\",\n    objects: [\n        (name: \"Cam\", components: [\n            (kind: \"Camra\", fields: {\"ortho_size\": Float(5.0)}),\n            (kind: \"Camera\", fields: {\"orto_size\": Float(5.0), \"clear_color\": Color(0.0, 0.0, 0.0, 1.0)}),\n        ]),\n    ],\n)\n";
        let d = check("a.scene", text);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0].range, at(text, "Camra"));
        assert_eq!(d[0].fixes[0].edits[0].text, "Camera");
        assert_eq!(d[1].range, at(text, "orto_size"));
        assert_eq!(d[1].fixes[0].edits[0].text, "ortho_size");
        // Испорченный RON — ошибка разбора типом сцены, на месте.
        let d = check("a.scene", "(name: \"L\", objects: [)");
        assert_eq!(d.len(), 1, "{d:?}");
    }

    #[test]
    fn materials_report_unknown_fields() {
        assert!(check("a.material", "(\n    roughness: 0.5,\n    uv_scale: (2.0, 2.0),\n)").is_empty());
        let text = "(\n    rougness: 0.5,\n)";
        let d = check("a.material", text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].range, at(text, "rougness"));
        assert_eq!(d[0].fixes[0].edits[0].text, "roughness");
        let d = check("a.material", "(\n    roughness: \"x\",\n)");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].line, 2);
    }

    #[test]
    fn wgsl_is_parsed_and_validated() {
        assert!(check("a.wgsl", "@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }").is_empty());
        let text = "fn f() -> f32 { return 1.0 }";
        let d = check("a.wgsl", text);
        assert_eq!(d.len(), 1, "{d:?}");
        let text = "fn f() -> f32 { return vec2<f32>(1.0); }";
        let d = check("a.wgsl", text);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].range.start > 0);
    }
}
