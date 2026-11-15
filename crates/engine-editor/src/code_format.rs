//! Format Document (Ctrl+K Ctrl+D): Rust — `rustfmt` через stdin (настройки — ближайший
//! `rustfmt.toml` проекта: процесс запускается в каталоге файла); C / C++ — `clang-format`,
//! если он есть в PATH, иначе простой отступ по скобкам; Scenario — отступ по блокам
//! (`{ }`, `function … end`, `then` / `do` … `end`).

use crate::i18n::trf;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use crate::code::Language;

/// Отформатировать текст внешней программой: stdin → stdout.
fn run_formatter(program: &str, args: &[&str], dir: Option<&Path>, text: &str) -> Result<String, String> {
    let mut command = engine_build::host::command(program);
    command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = dir.filter(|d| d.is_dir()) {
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|e| trf!("cannot start `{program}`: {e}. Is it installed and on PATH?", program = program, e = e))?;
    let mut stdin = child.stdin.take().ok_or_else(|| "no stdin".to_string())?; // i18n-skip: внутренняя ошибка
    let input = text.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        return Err(trf!("`{program}` could not format the file: {first}", program = program, first = first));
    }
    String::from_utf8(output.stdout).map(|s| s.replace("\r\n", "\n")).map_err(|e| e.to_string())
}

/// Есть ли программа в PATH (запускается `--version`).
pub(crate) fn available(program: &str) -> bool {
    engine_build::host::command(program).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Отформатировать документ. `Ok(None)` — для этого языка форматирования нет.
pub(crate) fn format(path: &Path, text: &str, language: Language, unit: &str) -> Result<Option<String>, String> {
    let dir = path.parent();
    match language {
        Language::Rust => run_formatter("rustfmt", &["--emit", "stdout", "--edition", "2021", "--quiet"], dir, text).map(Some),
        Language::Cpp => {
            if available("clang-format") {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                run_formatter("clang-format", &[&format!("--assume-filename={name}")], dir, text).map(Some)
            } else {
                Ok(Some(reindent(text, unit, Language::Cpp)))
            }
        }
        Language::Scenario => Ok(Some(reindent(text, unit, Language::Scenario))),
        _ => Ok(None),
    }
}

/// Простой отступ по блокам: строки внутри `{ … }` (и `function … end` в Scenario) —
/// на уровень глубже. Содержимое строк не трогается, многострочные строки и
/// комментарии — тоже (строки внутри них оставляются как есть).
pub(crate) fn reindent(text: &str, unit: &str, language: Language) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth: i32 = 0;
    let mut in_block_comment = false;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim();
        if in_block_comment || trimmed.is_empty() {
            out.push_str(if trimmed.is_empty() { "" } else { line });
            if trimmed.contains("*/") || trimmed.contains("]]") {
                in_block_comment = false;
            }
            continue;
        }
        // Код строки без строк и комментариев — для подсчёта скобок.
        let code = strip(trimmed, language);
        let words: Vec<&str> = code.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
        let lua_close = language == Language::Scenario && matches!(words.first().copied(), Some("end") | Some("until") | Some("else") | Some("elseif"));
        let leading_close = code.chars().take_while(|c| matches!(c, '}' | ')' | ']')).count() as i32;
        let level = (depth - leading_close - i32::from(lua_close)).max(0);
        let preprocessor = language == Language::Cpp && trimmed.starts_with('#');
        if !preprocessor {
            for _ in 0..level {
                out.push_str(unit);
            }
        }
        out.push_str(trimmed);
        let opens = code.chars().filter(|c| matches!(c, '{' | '(' | '[')).count() as i32;
        let closes = code.chars().filter(|c| matches!(c, '}' | ')' | ']')).count() as i32;
        depth += opens - closes;
        if language == Language::Scenario {
            let lua_open = words.iter().filter(|w| matches!(**w, "function" | "then" | "do" | "repeat")).count() as i32;
            let lua_end = words.iter().filter(|w| matches!(**w, "end" | "until")).count() as i32;
            // `function f() { … }` — блок фигурными скобками, а не `end`.
            let brace_function = words.contains(&"function") && code.contains('{');
            depth += lua_open - lua_end - i32::from(brace_function);
            // `elseif … then` продолжает блок `if`, а не открывает новый.
            if words.first() == Some(&"elseif") {
                depth -= 1;
            }
        }
        depth = depth.max(0);
        if (language == Language::Cpp && trimmed.contains("/*") && !trimmed.contains("*/")) || (language == Language::Scenario && trimmed.contains("--[[") && !trimmed.contains("]]")) {
            in_block_comment = true;
        }
    }
    out
}

fn strip(line: &str, language: Language) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' | '`' => {
                while let Some(n) = chars.next() {
                    if n == '\\' {
                        chars.next();
                    } else if n == c {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => break,
            '-' if language == Language::Scenario && chars.peek() == Some(&'-') => break,
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindent_scenario_and_cpp() {
        let text = "function go(a)\nif a then\nprint(1)\nelse\nprint(2)\nend\nend\nlet t = {\nx = 1,\n}";
        assert_eq!(reindent(text, "  ", Language::Scenario), "function go(a)\n  if a then\n    print(1)\n  else\n    print(2)\n  end\nend\nlet t = {\n  x = 1,\n}");
        let text = "struct A {\nvoid f() {\n// {\nint x = 1;\n}\n};\n#define X 1";
        assert_eq!(reindent(text, "    ", Language::Cpp), "struct A {\n    void f() {\n        // {\n        int x = 1;\n    }\n};\n#define X 1");
    }

    #[test]
    fn rustfmt_formats_through_stdin_when_installed() {
        if !available("rustfmt") {
            eprintln!("rustfmt is not installed: skipped");
            return;
        }
        let out = format(Path::new("x.rs"), "fn  main( ){let x=1;}", Language::Rust, "    ").unwrap().unwrap();
        assert_eq!(out, "fn main() {\n    let x = 1;\n}\n");
        let err = format(Path::new("x.rs"), "fn main( {", Language::Rust, "    ").unwrap_err();
        assert!(err.contains("rustfmt"), "{err}");
    }
}
