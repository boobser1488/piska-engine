//! Политика для файлов, которые уже есть в каталоге назначения, и перенос подготовленного
//! дерева в каталог (хаб готовит проект во временном каталоге рядом и переносит его сюда).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::collect::is_link;
use crate::{Control, Error, Result};

/// Что делать, если файл уже существует.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Conflict {
    /// Остановиться с ошибкой (по умолчанию: ничего не затирается молча).
    #[default]
    Fail,
    /// Оставить имеющийся файл, новый не записывать.
    Skip,
    /// Заменить имеющийся файл.
    Overwrite,
    /// Оставить оба: новый получает имя `имя.imported.расширение`.
    KeepBoth,
}

/// Куда писать.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Write(PathBuf),
    Skip,
}

/// Имя «оставить оба»: `a.png` → `a.imported.png`, потом `a.imported-2.png`…
pub(crate) fn keep_both_name(path: &Path) -> PathBuf {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Расширение — после последней точки, но не у скрытых файлов (`.gitignore`).
    let (stem, ext) = match name.rfind('.') {
        Some(at) if at > 0 => (&name[..at], &name[at..]),
        _ => (name.as_str(), ""),
    };
    for n in 1u32.. {
        let candidate = if n == 1 {
            format!("{stem}.imported{ext}")
        } else {
            format!("{stem}.imported-{n}{ext}")
        };
        let full = dir.join(candidate);
        if std::fs::symlink_metadata(&full).is_err() {
            return full;
        }
    }
    unreachable!("the counter does not run out")
}

/// Куда записать файл `path` с учётом политики. Нет такого файла — туда же.
pub fn resolve_target(path: &Path, conflict: Conflict) -> Result<Target> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Target::Write(path.to_path_buf())),
        Err(e) => Err(Error::io(path, e)),
        Ok(meta) => {
            if meta.is_dir() {
                // Файл не может занять место каталога ни при какой политике.
                return Err(Error::Exists(path.display().to_string()));
            }
            match conflict {
                Conflict::Fail => Err(Error::Exists(path.display().to_string())),
                Conflict::Skip => Ok(Target::Skip),
                Conflict::Overwrite => Ok(Target::Write(path.to_path_buf())),
                Conflict::KeepBoth => Ok(Target::Write(keep_both_name(path))),
            }
        }
    }
}

/// Создать каталоги до `target` внутри `root`, не проходя через ссылки.
pub(crate) fn ensure_parent(root: &Path, target: &Path, verified: &mut HashSet<PathBuf>) -> Result<()> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    let relative = parent.strip_prefix(root).map_err(|_| Error::LinkInTheWay {
        path: target.display().to_string(),
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if verified.contains(&current) {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if is_link(&meta) => {
                return Err(Error::LinkInTheWay {
                    path: current.display().to_string(),
                })
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(Error::Exists(current.display().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|e| Error::io(&current, e))?;
            }
            Err(e) => return Err(Error::io(&current, e)),
        }
        verified.insert(current.clone());
    }
    Ok(())
}

/// Что сделал [`merge_tree`].
#[derive(Clone, Debug, Default)]
pub struct MergeSummary {
    pub placed: Vec<String>,
    pub skipped: Vec<String>,
    /// Записаны под другим именем: было и стало.
    pub renamed: Vec<(String, String)>,
}

fn list_files(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}{name}");
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            list_files(&entry.path(), &format!("{rel}/"), out)?;
        } else if meta.is_file() {
            out.push(rel);
        }
    }
    Ok(())
}

/// Перенести все файлы дерева `staging` в `dest` по политике `conflict`. Дерево `staging`
/// после этого пустеет. Отмена проверяется между файлами: перенос — переименования на одном
/// диске, быстрые; уже перенесённые файлы остаются.
pub fn merge_tree(staging: &Path, dest: &Path, conflict: Conflict, control: &Control) -> Result<MergeSummary> {
    let mut files = Vec::new();
    list_files(staging, "", &mut files)?;
    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;
    let mut summary = MergeSummary::default();
    let mut renamed: HashMap<String, PathBuf> = HashMap::new();
    let mut verified = HashSet::new();
    for rel in files {
        control.check()?;
        let source = staging.join(&rel);
        let mut target = dest.join(&rel);
        if let Some(base) = rel.strip_suffix(".meta") {
            if let Some(new_base) = renamed.get(base) {
                target = PathBuf::from(format!("{}.meta", new_base.display()));
            }
        }
        ensure_parent(dest, &target, &mut verified)?;
        match resolve_target(&target, conflict)? {
            Target::Skip => {
                summary.skipped.push(rel);
            }
            Target::Write(final_path) => {
                if final_path != dest.join(&rel) {
                    let shown = final_path.strip_prefix(dest).unwrap_or(&final_path).to_string_lossy().replace('\\', "/");
                    summary.renamed.push((rel.clone(), shown));
                    renamed.insert(rel.clone(), final_path.clone());
                }
                if conflict == Conflict::Overwrite && final_path.exists() {
                    // Windows не заменяет файл переименованием поверх только для чтения.
                    let _ = std::fs::remove_file(&final_path);
                }
                if std::fs::rename(&source, &final_path).is_err() {
                    std::fs::copy(&source, &final_path).map_err(|e| Error::io(&final_path, e))?;
                    let _ = std::fs::remove_file(&source);
                }
                summary.placed.push(rel);
            }
        }
    }
    Ok(summary)
}
