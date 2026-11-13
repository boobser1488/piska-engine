//! Где хаб хранит свои файлы и где лежит движок.
//!
//! Каталог настроек — `%APPDATA%/piska` на Windows, `$XDG_CONFIG_HOME/piska` (или
//! `~/.config/piska`) на Linux и macOS. Переменная `PISKA_HUB_CONFIG_DIR` его заменяет
//! (тесты, переносная установка). Тот же каталог читает редактор (File → Open Recent).

use std::path::{Component, Path, PathBuf};

/// Переменная окружения: свой каталог настроек хаба.
pub const CONFIG_DIR_ENV: &str = "PISKA_HUB_CONFIG_DIR";
/// Переменная окружения: корень рабочего пространства движка.
pub const ENGINE_ROOT_ENV: &str = "PISKA_ENGINE_ROOT";

/// Файл списка проектов и настроек хаба.
pub const CONFIG_FILE: &str = "hub.ron";
/// Недавние проекты для редактора: строки `время<TAB>имя<TAB>путь`.
pub const RECENT_FILE: &str = "recent.txt";
/// Хаб жив: `pid` и время последней отметки (секунды Unix), обновляется раз в секунду.
pub const ALIVE_FILE: &str = "hub.alive";
/// Просьба редактора к работающему хабу: `show` или `open <путь>`.
pub const REQUEST_FILE: &str = "hub.request";

/// Каталог настроек хаба (не создаётся).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(appdata).join("piska"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(xdg).join("piska"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("piska"))
}

/// Это корень рабочего пространства движка: `Cargo.toml` с `[workspace]` и `crates/engine`.
pub fn is_engine_root(dir: &Path) -> bool {
    dir.join("crates").join("engine").join("Cargo.toml").is_file()
        && std::fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|text| text.contains("[workspace]"))
}

/// Найти движок: `PISKA_ENGINE_ROOT`, каталог, из которого собран хаб, затем вверх от
/// текущего каталога и от программы.
pub fn find_engine_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(ENGINE_ROOT_ENV).filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        if is_engine_root(&dir) {
            return Some(clean(&dir));
        }
    }
    let built_from = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    if is_engine_root(&built_from) {
        return Some(clean(&built_from));
    }
    let mut starts = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        starts.push(exe);
    }
    starts
        .iter()
        .flat_map(|start| start.ancestors())
        .find(|dir| is_engine_root(dir))
        .map(clean)
}

/// Абсолютный путь без `.` и `..`, без `\\?\` (его не понимают cargo и проводник).
/// Несуществующий путь только нормализуется.
pub fn clean(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let resolved = absolute.canonicalize().unwrap_or_else(|_| normalize(&absolute));
    let text = resolved.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Ключ для сравнения путей: нормализованный, на Windows — без учёта регистра и с `\`.
pub fn key(path: &Path) -> String {
    let text = clean(path).display().to_string();
    if cfg!(windows) {
        text.replace('/', "\\").trim_end_matches('\\').to_lowercase()
    } else {
        text.trim_end_matches('/').to_string()
    }
}

/// Один и тот же каталог.
pub fn same(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// `inner` лежит внутри `outer` (или совпадает с ним).
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let (inner, outer) = (key(inner), key(outer));
    let sep = if cfg!(windows) { '\\' } else { '/' };
    inner == outer || inner.starts_with(&format!("{outer}{sep}"))
}

/// Путь относительно `base` через `/` (`games/lantern`), если он внутри.
pub fn relative_to(path: &Path, base: &Path) -> Option<String> {
    let path = clean(path);
    let base = clean(base);
    if !is_within(&path, &base) {
        return None;
    }
    let rest: Vec<String> = path
        .components()
        .skip(base.components().count())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(rest.join("/"))
}

/// Путь для показа: в `Cargo.toml` и подписях — через `/`.
pub fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_and_relativize() {
        let base = std::env::temp_dir().join("piska-hub-paths");
        let inner = base.join("games").join("..").join("games").join("lantern");
        assert!(is_within(&inner, &base));
        assert!(!is_within(&base, &inner));
        assert!(same(&base.join("x").join(".."), &base));
        assert_eq!(relative_to(&inner, &base).as_deref(), Some("games/lantern"));
        assert_eq!(relative_to(&base, &inner), None);
        // Соседний каталог с тем же началом имени — не «внутри».
        assert!(!is_within(&std::env::temp_dir().join("piska-hub-paths-2"), &base));
    }

    #[test]
    fn the_engine_checkout_is_found() {
        let root = find_engine_root().expect("the hub is built inside the engine workspace");
        assert!(is_engine_root(&root));
        assert!(root.join("games").is_dir());
    }
}
