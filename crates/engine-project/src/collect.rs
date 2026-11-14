//! Что входит в файл проекта: обход каталога с `.gitignore`, встроенным списком «производного»
//! и мусора и без символических ссылок.

use std::path::{Path, PathBuf};

use crate::ignore::Rules;
use crate::{Control, Error, Result};

/// Какие необязательные части входят в файл.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectOptions {
    /// `tests/`.
    pub tests: bool,
    /// `cpp/` (исходники C++).
    pub cpp: bool,
    /// `dist/` проекта: последние собранные (экспортированные) программы. По умолчанию выключено.
    pub builds: bool,
    /// Настройки IDE: `.vscode/`, `.idea/`, `.vs/`, `*.code-workspace`. По умолчанию выключено.
    pub ide: bool,
    /// Учитывать `.gitignore` проекта (всегда вместе со встроенным списком).
    pub gitignore: bool,
}

impl Default for CollectOptions {
    fn default() -> Self {
        Self {
            tests: true,
            cpp: true,
            builds: false,
            ide: false,
            gitignore: true,
        }
    }
}

/// Файл, который попадёт в архив.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Путь от каталога проекта, через `/`.
    pub rel: String,
    pub path: PathBuf,
    pub size: u64,
}

/// Почему что-то не вошло.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Производное: `target/`, `.git/`, кэши, папки сборки плагинов.
    Derived,
    /// Мусор ОС и временные файлы.
    Junk,
    /// Исключено `.gitignore` проекта.
    Gitignore,
    /// Символическая ссылка (не вынимается за пределы проекта).
    Link,
    /// Часть выключена в параметрах (`tests`, `cpp`, `builds`, `ide`).
    Option(&'static str),
    /// Имя не из Unicode или небезопасное для переноса.
    Name,
}

/// Пропущенный файл или каталог.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub rel: String,
    pub reason: SkipReason,
    /// Каталог пропущен целиком.
    pub dir: bool,
}

/// Результат обхода.
#[derive(Clone, Debug, Default)]
pub struct Collected {
    /// Файлы по возрастанию пути (порядок записи в архив).
    pub files: Vec<Candidate>,
    pub skipped: Vec<Skipped>,
    /// Сумма размеров.
    pub bytes: u64,
}

/// Обойти проект. Ошибка — только если нет `Cargo.toml` или отмена.
pub fn collect(root: &Path, options: &CollectOptions, control: &Control) -> Result<Collected> {
    if !root.join("Cargo.toml").is_file() {
        return Err(Error::NoProject(root.display().to_string()));
    }
    let mut out = Collected::default();
    let mut stack: Vec<(String, Rules)> = Vec::new();
    walk(root, "", options, control, &mut stack, &mut out)?;
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    out.skipped.sort_by(|a, b| a.rel.cmp(&b.rel));
    out.bytes = out.files.iter().map(|f| f.size).sum();
    Ok(out)
}

pub(crate) fn is_link(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT: junction и прочие точки повторной обработки.
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

/// Файлы, которые `.gitignore` не вправе исключить: без них проект не собрать.
fn must_keep(rel: &str) -> bool {
    matches!(rel, "Cargo.toml" | "Cargo.lock" | "assets/project.settings" | "project.settings")
}

fn junk_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(), ".ds_store" | "thumbs.db" | "ehthumbs.db" | "desktop.ini")
        || name.ends_with('~')
        || name.starts_with(".#")
        || name.contains(".live-")
        || lower
            .rsplit_once('.')
            .is_some_and(|(_, ext)| matches!(ext, "tmp" | "temp" | "swp" | "swo" | "part"))
}

fn ide_name(name: &str) -> bool {
    matches!(name, ".vscode" | ".idea" | ".vs") || name.ends_with(".code-workspace")
}

fn walk(
    dir: &Path,
    prefix: &str,
    options: &CollectOptions,
    control: &Control,
    stack: &mut Vec<(String, Rules)>,
    out: &mut Collected,
) -> Result<()> {
    control.check()?;
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    let pushed = if options.gitignore {
        let file = dir.join(".gitignore");
        let regular = std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file() && m.len() < 1 << 20);
        match regular.then(|| std::fs::read_to_string(&file).ok()).flatten() {
            Some(text) => {
                let rules = Rules::parse(&text);
                if rules.is_empty() {
                    false
                } else {
                    stack.push((prefix.to_string(), rules));
                    true
                }
            }
            None => false,
        }
    } else {
        false
    };
    let has_cargo = dir.join("Cargo.toml").is_file();
    let has_cmake = dir.join("CMakeLists.txt").is_file();
    let parent_name = prefix.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let grandparent = prefix.trim_end_matches('/').rsplit('/').nth(1).unwrap_or("");

    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            out.skipped.push(Skipped {
                rel: format!("{prefix}{}", entry.file_name().to_string_lossy()),
                reason: SkipReason::Name,
                dir: false,
            });
            continue;
        };
        let rel = format!("{prefix}{name}");
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let is_dir = metadata.is_dir();
        let skip = |reason: SkipReason, out: &mut Collected| {
            out.skipped.push(Skipped {
                rel: rel.clone(),
                reason,
                dir: is_dir,
            });
        };
        if is_link(&metadata) {
            skip(SkipReason::Link, out);
            continue;
        }
        if !is_dir && !metadata.is_file() {
            continue;
        }
        let top = prefix.is_empty();
        if is_dir {
            let derived = matches!(name.as_str(), ".git" | ".hg" | ".svn" | "node_modules" | "__pycache__" | "CMakeFiles")
                || name.starts_with("cmake-build-")
                || (name == "target" && (top || has_cargo))
                || (name == "build" && (has_cmake || parent_name == "cpp"))
                || (matches!(name.as_str(), "build" | "bin") && grandparent == "plugins");
            if derived {
                skip(SkipReason::Derived, out);
                continue;
            }
            if top {
                let off = match name.as_str() {
                    "tests" if !options.tests => Some("tests"),
                    "cpp" if !options.cpp => Some("cpp"),
                    "dist" if !options.builds => Some("builds"),
                    _ => None,
                };
                if let Some(part) = off {
                    skip(SkipReason::Option(part), out);
                    continue;
                }
            }
        } else if junk_file(&name) {
            skip(SkipReason::Junk, out);
            continue;
        }
        if !options.ide && ide_name(&name) {
            skip(SkipReason::Option("ide"), out);
            continue;
        }
        if !stack.is_empty() && !must_keep(&rel) {
            let mut verdict = None;
            for (base, rules) in stack.iter() {
                let inside = &rel[base.len()..];
                if let Some(v) = rules.verdict(inside, is_dir) {
                    verdict = Some(v);
                }
            }
            if verdict == Some(true) {
                skip(SkipReason::Gitignore, out);
                continue;
            }
        }
        if is_dir {
            walk(&path, &format!("{rel}/"), options, control, stack, out)?;
        } else {
            out.files.push(Candidate {
                rel,
                path,
                size: metadata.len(),
            });
        }
    }
    if pushed {
        stack.pop();
    }
    Ok(())
}
