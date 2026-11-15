//! Файлы вкладки Code: переводы строк, BOM, отступы и `.editorconfig`.
//!
//! Буфер редактора всегда хранит `\n`; при сохранении файл получает свои прежние
//! переводы строк (`\r\n` остаётся `\r\n`) и BOM, если он был. Отступ (табуляция или
//! пробелы и их число) определяется по самому файлу, а `.editorconfig` (если он есть
//! выше по каталогам) имеет приоритет.

use crate::i18n::{tr_s, trf};
use std::path::{Path, PathBuf};

/// Переводы строк файла.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
        }
    }
}

/// Отступ файла.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Indent {
    pub tabs: bool,
    pub size: usize,
}

impl Default for Indent {
    fn default() -> Self {
        Self { tabs: false, size: 4 }
    }
}

impl Indent {
    pub fn label(self) -> String {
        if self.tabs {
            trf!("Tabs: {n}", n = self.size)
        } else {
            trf!("Spaces: {n}", n = self.size)
        }
    }
}

/// Как файл лежит на диске — чтобы записать его так же.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileFormat {
    pub line_ending: LineEnding,
    pub bom: bool,
    pub indent: Indent,
    /// Отступ виден по самому файлу или задан `.editorconfig` (иначе — из настроек).
    pub indent_known: bool,
    /// `.editorconfig`: добавить перевод строки в конец при сохранении.
    pub final_newline: Option<bool>,
    /// `.editorconfig`: убрать пробелы в концах строк при сохранении.
    pub trim_trailing: Option<bool>,
}

/// Прочитать файл как текст: BOM снимается, `\r\n` → `\n`, формат запоминается.
pub(crate) fn read(path: &Path) -> Result<(String, FileFormat), String> {
    let bytes = std::fs::read(path).map_err(|e| trf!("cannot open `{p0}` as text: {e}", p0 = path.display(), e = e))?;
    let (bom, body) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (true, rest),
        None => (false, &bytes[..]),
    };
    let text = String::from_utf8(body.to_vec())
        .map_err(|_| trf!("cannot open `{p0}` as text: it is not UTF-8", p0 = path.display()))?;
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    let line_ending = if crlf > lf { LineEnding::CrLf } else { LineEnding::Lf };
    let text = text.replace("\r\n", "\n");
    let indent_known = text.lines().any(|l| l.starts_with([' ', '\t']) && !l.trim().is_empty());
    let mut format = FileFormat { line_ending, bom, indent: detect_indent(&text), indent_known, ..FileFormat::default() };
    apply_editorconfig(path, &mut format);
    Ok((text, format))
}

/// Текст для записи на диск: свои переводы строк, BOM и правила сохранения.
pub(crate) fn encode(text: &str, format: &FileFormat, trim_trailing: bool, final_newline: bool) -> Vec<u8> {
    let mut text = text.to_string();
    if format.trim_trailing.unwrap_or(trim_trailing) {
        text = trim_trailing_whitespace(&text);
    }
    if format.final_newline.unwrap_or(final_newline) && !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if format.line_ending == LineEnding::CrLf {
        text = text.replace('\n', "\r\n");
    }
    let mut bytes = Vec::with_capacity(text.len() + 3);
    if format.bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// Убрать пробелы и табуляции в концах строк.
pub(crate) fn trim_trailing_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    out
}

/// Отступ по строкам файла: табуляции или пробелы, и самый частый шаг пробелов.
pub(crate) fn detect_indent(text: &str) -> Indent {
    let mut tabs = 0;
    let mut spaces = 0;
    let mut steps = [0usize; 9];
    let mut previous = 0usize;
    for line in text.lines().take(2000) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let width = line.len() - line.trim_start_matches(' ').len();
        if width > 0 {
            spaces += 1;
        }
        let step = width.abs_diff(previous);
        if (2..=8).contains(&step) {
            steps[step] += 1;
        }
        previous = width;
    }
    if tabs > spaces {
        return Indent { tabs: true, size: 4 };
    }
    let size = (2..=8).max_by_key(|s| (steps[*s], *s == 4)).filter(|s| steps[*s] > 0).unwrap_or(4);
    Indent { tabs: false, size }
}

/// Подходит ли имя файла под шаблон секции `.editorconfig` (`*`, `*.rs`, `*.{rs,toml}`, `Makefile`).
fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim().trim_start_matches("**/").trim_start_matches('/');
    if pattern == "*" {
        return true;
    }
    if let Some(rest) = pattern.strip_prefix("*.") {
        let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("");
        if let Some(list) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            return list.split(',').any(|e| e.trim() == ext);
        }
        return rest == ext;
    }
    if let Some(list) = pattern.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        return list.split(',').any(|p| glob_matches(p, name));
    }
    pattern == name
}

/// Применить `.editorconfig` из каталогов выше файла (ближний — главнее, до `root = true`).
pub(crate) fn apply_editorconfig(path: &Path, format: &mut FileFormat) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut configs: Vec<PathBuf> = Vec::new();
    for dir in path.ancestors().skip(1).take(12) {
        let config = dir.join(".editorconfig");
        if config.is_file() {
            let root = std::fs::read_to_string(&config).is_ok_and(|t| {
                t.lines().take_while(|l| !l.trim_start().starts_with('[')).any(|l| l.replace(' ', "").eq_ignore_ascii_case("root=true"))
            });
            configs.push(config);
            if root {
                break;
            }
        }
    }
    // Дальние сначала: ближние перекрывают.
    for config in configs.into_iter().rev() {
        let Ok(text) = std::fs::read_to_string(&config) else { continue };
        let mut active = false;
        let mut size_from_tab = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                active = glob_matches(section, &name);
                continue;
            }
            if !active {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim().to_ascii_lowercase();
            match key.trim().to_ascii_lowercase().as_str() {
                "indent_style" => {
                    format.indent.tabs = value == "tab";
                    format.indent_known = true;
                }
                "indent_size" => {
                    if let Ok(n) = value.parse::<usize>() {
                        format.indent.size = n.clamp(1, 16);
                    }
                }
                "tab_width" => size_from_tab = value.parse::<usize>().ok(),
                "end_of_line" => match value.as_str() {
                    "crlf" => format.line_ending = LineEnding::CrLf,
                    "lf" => format.line_ending = LineEnding::Lf,
                    _ => {}
                },
                "insert_final_newline" => format.final_newline = Some(value == "true"),
                "trim_trailing_whitespace" => format.trim_trailing = Some(value == "true"),
                "charset" => {
                    if value == "utf-8-bom" {
                        format.bom = true;
                    } else if value == "utf-8" {
                        format.bom = false;
                    }
                }
                _ => {}
            }
        }
        if format.indent.tabs {
            if let Some(n) = size_from_tab {
                format.indent.size = n.clamp(1, 16);
            }
        }
    }
}

/// Подпись кодировки для строки состояния.
pub(crate) fn encoding_label(format: &FileFormat) -> &'static str {
    if format.bom {
        tr_s("UTF-8 with BOM")
    } else {
        "UTF-8"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piska_code_files_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn line_endings_and_bom_survive_a_round_trip() {
        let dir = temp("crlf");
        let path = dir.join("a.cpp");
        let bytes = b"\xEF\xBB\xBFint a;\r\nint b;\r\n";
        std::fs::write(&path, bytes).unwrap();
        let (text, format) = read(&path).unwrap();
        assert_eq!(text, "int a;\nint b;\n");
        assert_eq!((format.line_ending, format.bom), (LineEnding::CrLf, true));
        let out = encode(&(text + "int c;\n"), &format, false, false);
        assert_eq!(out, b"\xEF\xBB\xBFint a;\r\nint b;\r\nint c;\r\n");
        // Файл с `\n` так и остаётся.
        std::fs::write(&path, b"x\ny\n").unwrap();
        let (text, format) = read(&path).unwrap();
        assert_eq!(encode(&text, &format, false, false), b"x\ny\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indentation_is_detected() {
        assert_eq!(detect_indent("fn a() {\n  b();\n  if x {\n    c();\n  }\n}\n"), Indent { tabs: false, size: 2 });
        assert_eq!(detect_indent("fn a() {\n    b();\n}\n"), Indent { tabs: false, size: 4 });
        assert!(detect_indent("void a() {\n\tb();\n\tc();\n}\n").tabs);
        assert_eq!(detect_indent(""), Indent::default());
    }

    #[test]
    fn editorconfig_overrides_detection() {
        let dir = temp("ec");
        std::fs::write(dir.join(".editorconfig"), "root = true\n[*]\nindent_style = space\nindent_size = 2\n[*.{cpp,hpp}]\nindent_style = tab\ntab_width = 8\nend_of_line = crlf\ninsert_final_newline = true\ntrim_trailing_whitespace = true\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "fn a() {\n    b();\n}\n").unwrap();
        std::fs::write(dir.join("src/b.cpp"), "int a;\n").unwrap();
        let (_, rs) = read(&dir.join("src/a.rs")).unwrap();
        assert_eq!(rs.indent, Indent { tabs: false, size: 2 });
        let (text, cpp) = read(&dir.join("src/b.cpp")).unwrap();
        assert_eq!(cpp.indent, Indent { tabs: true, size: 8 });
        assert_eq!(cpp.line_ending, LineEnding::CrLf);
        assert_eq!(encode(&(text.trim_end().to_string() + "  "), &cpp, false, false), b"int a;\r\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trailing_whitespace_is_trimmed() {
        assert_eq!(trim_trailing_whitespace("a  \n\tb\t\n  \nc"), "a\n\tb\n\nc");
    }
}
