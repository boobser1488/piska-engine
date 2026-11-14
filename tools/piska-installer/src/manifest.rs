//! Манифест прежней установки: скрипты записывают в него всё, что добавили (программы,
//! переменные, ярлыки, каталоги). Если он есть, установщик предлагает Modify / Repair /
//! Uninstall. Где он лежит, знают скрипты; здесь — те же правила:
//!
//! * Windows: `%LOCALAPPDATA%/<state_dir_name>/install/manifest.json` (+ `state.json`);
//! * Linux / macOS: `${XDG_STATE_HOME:-~/.local/state}/<state_dir_name>/install/manifest.tsv`
//!   (+ файл `components`).

use std::path::{Path, PathBuf};

/// Запись манифеста.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `winget`, `rustup`, `rust-target`, `cargo-crate`, `env`, `shortcut`, `fileassoc`, `dir`,
    /// `file`, `block`, `prefs`, `pkg`, `cargo-config` …
    pub kind: String,
    pub id: String,
    pub detail: String,
    pub component: String,
}

/// Найденная установка.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Existing {
    pub manifest: PathBuf,
    pub entries: Vec<Entry>,
    /// Компоненты последней установки (из файла состояния).
    pub components: Vec<String>,
}

/// Каталог состояния скриптов; `var` — переменные окружения (тесты подставляют свои).
pub fn state_dir(var: &dyn Fn(&str) -> Option<String>, state_dir_name: &str, windows: bool) -> Option<PathBuf> {
    if windows {
        let local = var("LOCALAPPDATA").map(PathBuf::from).or_else(|| var("USERPROFILE").map(|h| PathBuf::from(h).join("AppData").join("Local")))?;
        Some(local.join(state_dir_name).join("install"))
    } else {
        let base = var("XDG_STATE_HOME").map(PathBuf::from).or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))?;
        Some(base.join(state_dir_name).join("install"))
    }
}

fn parse_json(text: &str) -> Vec<Entry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        return Vec::new();
    };
    let list = match value.get("entries") {
        Some(serde_json::Value::Array(a)) => a.clone(),
        // ConvertTo-Json одного элемента пишет объект, а не массив.
        Some(obj @ serde_json::Value::Object(_)) => vec![obj.clone()],
        _ => Vec::new(),
    };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    list.iter().map(|e| Entry { kind: s(e, "kind"), id: s(e, "id"), detail: s(e, "detail"), component: s(e, "component") }).collect()
}

fn parse_tsv(text: &str) -> Vec<Entry> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let g = |i: usize| f.get(i).copied().unwrap_or_default().to_string();
            Entry { kind: g(0), id: g(1), detail: g(2), component: g(4) }
        })
        .collect()
}

fn components_of(dir: &Path, windows: bool) -> Vec<String> {
    if windows {
        let Ok(text) = std::fs::read_to_string(dir.join("state.json")) else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
            return Vec::new();
        };
        match value.get("components") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    } else {
        std::fs::read_to_string(dir.join("components")).map(|t| t.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
    }
}

/// Прежняя установка, если манифест есть и не пуст.
pub fn find(var: &dyn Fn(&str) -> Option<String>, state_dir_name: &str, windows: bool) -> Option<Existing> {
    let dir = state_dir(var, state_dir_name, windows)?;
    let manifest = dir.join(if windows { "manifest.json" } else { "manifest.tsv" });
    let text = std::fs::read_to_string(&manifest).ok()?;
    let entries = if windows { parse_json(&text) } else { parse_tsv(&text) };
    if entries.is_empty() {
        return None;
    }
    let mut components = components_of(&dir, windows);
    if components.is_empty() {
        for e in &entries {
            if !e.component.is_empty() && e.component != "config" && !components.contains(&e.component) {
                components.push(e.component.clone());
            }
        }
    }
    Some(Existing { manifest, entries, components })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_of_both_scripts_are_read() {
        let root = std::env::temp_dir().join(format!("piska-installer-manifest-{}", std::process::id()));
        let win = root.join("Local").join("Piska").join("install");
        std::fs::create_dir_all(&win).unwrap();
        std::fs::write(
            win.join("manifest.json"),
            "\u{feff}{\"version\":1,\"entries\":[{\"kind\":\"winget\",\"id\":\"Kitware.CMake\",\"detail\":\"\",\"previous\":\"\",\"component\":\"cpp\",\"time\":\"t\"}]}",
        )
        .unwrap();
        std::fs::write(win.join("state.json"), "{\"version\":1,\"components\":[\"core\",\"cpp\"],\"steps\":[]}").unwrap();
        let local = root.join("Local").display().to_string();
        let var = move |k: &str| (k == "LOCALAPPDATA").then(|| local.clone());
        let found = find(&var, "Piska", true).unwrap();
        assert_eq!(found.entries[0].id, "Kitware.CMake");
        assert_eq!(found.components, ["core", "cpp"]);

        let unix = root.join("state").join("Piska").join("install");
        std::fs::create_dir_all(&unix).unwrap();
        std::fs::write(unix.join("manifest.tsv"), "rust-target\twasm32-unknown-unknown\t\t\tweb\tt\n").unwrap();
        let state = root.join("state").display().to_string();
        let var = move |k: &str| (k == "XDG_STATE_HOME").then(|| state.clone());
        let found = find(&var, "Piska", false).unwrap();
        assert_eq!((found.entries[0].kind.as_str(), found.components.clone()), ("rust-target", vec!["web".to_string()]));
        assert!(find(&|_: &str| None, "Piska", true).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
