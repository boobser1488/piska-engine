//! Рабочее пространство и крейт игры: где лежит, как называется, что в ассетах.

use std::path::{Path, PathBuf};

use engine_assets::{AssetDatabase, AssetPack};

use crate::{Error, Platform, Result};

/// Найти корень рабочего пространства движка: каталог с `Cargo.toml`, где есть
/// `[workspace]`, вверх от `start` (или от текущего каталога).
pub fn workspace_root(start: Option<&Path>) -> Result<PathBuf> {
    let start = match start {
        Some(dir) => dir.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let start = start.canonicalize().unwrap_or(start);
    let mut dir = Some(start.as_path());
    while let Some(current) = dir {
        let manifest = current.join("Cargo.toml");
        if manifest.is_file()
            && std::fs::read_to_string(&manifest).is_ok_and(|text| text.contains("[workspace]"))
        {
            return Ok(strip_verbatim(current));
        }
        dir = current.parent();
    }
    Err(Error(format!(
        "`{}` is not inside the engine workspace (no Cargo.toml with [workspace] above it); \
         run from the workspace or pass --root",
        start.display()
    )))
}

/// `\\?\C:\…` (результат `canonicalize` на Windows) не понимают cargo и браузеры.
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

/// Игры рабочего пространства: подкаталоги `games/` с `Cargo.toml`.
pub fn list_games(root: &Path) -> Result<Vec<String>> {
    let mut games = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("games")) {
        for entry in entries.flatten() {
            if entry.path().join("Cargo.toml").is_file() {
                games.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    games.sort();
    Ok(games)
}

/// Возможности движка, которые крейт игры включает в зависимости `engine`
/// (`docs/design/engine-features.md`): то, что не включено, в exe / wasm / apk не попадает.
#[derive(Clone, Debug, Default)]
pub struct EngineFeatures {
    /// Как записано в `features = [...]` зависимости `engine`.
    pub declared: Vec<String>,
    /// Не выключены ли возможности `default` движка (`default-features = false` — выключены).
    pub default_features: bool,
    /// Итог: объявленные возможности и всё, что они за собой тянут (`full`, `physics`, …).
    pub effective: Vec<String>,
}

impl EngineFeatures {
    /// Прочитать из манифеста игры и из `[features]` манифеста движка (путь зависимости).
    fn read(game_dir: &Path, manifest: &toml::Table) -> Self {
        let dependency = manifest.get("dependencies").and_then(|d| d.get("engine"));
        let mut declared = Vec::new();
        let mut default_features = true;
        let mut engine_dir = None;
        if let Some(toml::Value::Table(table)) = dependency {
            if let Some(list) = table.get("features").and_then(|f| f.as_array()) {
                declared = list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
            }
            default_features = table.get("default-features").and_then(|v| v.as_bool()).unwrap_or(true);
            engine_dir = table.get("path").and_then(|p| p.as_str()).map(|p| game_dir.join(p));
        }
        let table = engine_dir
            .and_then(|dir| std::fs::read_to_string(dir.join("Cargo.toml")).ok())
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|m| m.get("features").and_then(|f| f.as_table()).cloned())
            .unwrap_or_default();
        let mut queue = declared.clone();
        if default_features {
            queue.push("default".to_string());
        }
        let mut seen = std::collections::BTreeSet::new();
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            for entry in table.get(&name).and_then(|v| v.as_array()).into_iter().flatten() {
                // `dep:x` и `крейт/возможность` — не возможности самого движка.
                if let Some(next) = entry.as_str().filter(|s| !s.contains('/') && !s.starts_with("dep:")) {
                    queue.push(next.to_string());
                }
            }
        }
        seen.remove("default");
        Self { declared, default_features, effective: seen.into_iter().collect() }
    }

    /// Включена ли возможность (прямо или через другую).
    pub fn enables(&self, name: &str) -> bool {
        self.effective.iter().any(|f| f == name)
    }

    /// Одна строка для консоли: `audio, gamepad, ui` (или `none`).
    pub fn summary(&self) -> String {
        if self.effective.is_empty() {
            "none".to_string()
        } else {
            self.effective.join(", ")
        }
    }
}

/// Крейт игры, найденный в `games/<name>/`.
pub struct Project {
    /// Возможности движка, включённые игрой.
    pub engine: EngineFeatures,
    pub root: PathBuf,
    pub game: String,
    pub dir: PathBuf,
    /// Имя пакета из `Cargo.toml` игры (`-p <package>`).
    pub package: String,
    /// Имя библиотеки (дефисы → подчёркивания): так называется `.wasm`.
    pub lib_name: String,
    /// Заголовок для страницы и окна.
    pub title: String,
    /// Версия пакета (`version.workspace = true` — версия рабочего пространства).
    pub version: String,
    /// Таблица `[package.metadata.piska]` из `Cargo.toml` игры (пустая, если её нет).
    pub piska: toml::Table,
}

impl Project {
    pub fn open(root: Option<&Path>, game: &str) -> Result<Self> {
        let root = workspace_root(root)?;
        let dir = root.join("games").join(game);
        let manifest_path = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest_path).map_err(|_| {
            let known = list_games(&root).unwrap_or_default();
            Error(format!(
                "no game `{game}`: `games/{game}/Cargo.toml` does not exist. Known games: {}",
                if known.is_empty() { "none".to_string() } else { known.join(", ") }
            ))
        })?;
        let manifest: toml::Table = text
            .parse()
            .map_err(|e| Error(format!("`games/{game}/Cargo.toml` is not valid TOML: {e}")))?;
        let package = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .ok_or_else(|| Error(format!("`games/{game}/Cargo.toml` has no [package] name")))?
            .to_string();
        if !dir.join("src").join("lib.rs").is_file() {
            return Err(Error(format!(
                "`games/{game}/src/lib.rs` is missing: a game crate is a library with \
                 `engine::game_main!(build, …)` (see docs/api/export.md)"
            )));
        }
        let piska = manifest
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("piska"))
            .and_then(|p| p.as_table())
            .cloned()
            .unwrap_or_default();
        let title = piska
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| package.clone());
        let version = match manifest.get("package").and_then(|p| p.get("version")) {
            Some(toml::Value::String(version)) => version.clone(),
            // `version.workspace = true` — из `[workspace.package]` корня.
            _ => std::fs::read_to_string(root.join("Cargo.toml"))
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .and_then(|t| {
                    t.get("workspace")?
                        .get("package")?
                        .get("version")?
                        .as_str()
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "0.0.0".to_string()),
        };
        let engine = EngineFeatures::read(&dir, &manifest);
        Ok(Self {
            engine,
            version,
            piska,
            lib_name: package.replace('-', "_"),
            package,
            title,
            game: game.to_string(),
            dir,
            root,
        })
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.dir.join("assets")
    }

    /// Значение `[package.metadata.piska]` по пути через точку: `"android.package"`.
    pub fn setting(&self, path: &str) -> Option<&toml::Value> {
        let mut parts = path.split('.');
        let mut value = self.piska.get(parts.next()?)?;
        for part in parts {
            value = value.get(part)?;
        }
        Some(value)
    }

    /// Строковая настройка; не строка — ошибка с подсказкой.
    pub fn setting_str(&self, path: &str) -> Result<Option<&str>> {
        match self.setting(path) {
            None => Ok(None),
            Some(toml::Value::String(text)) => Ok(Some(text)),
            Some(other) => Err(Error(format!(
                "`{path}` in [package.metadata.piska] of games/{}/Cargo.toml must be a string, not `{other}`",
                self.game
            ))),
        }
    }

    /// Значок игры: PNG из ассетов по пути `icon = "sprites/icon.png"`
    /// в `[package.metadata.piska]`, а без настройки — `assets/icon.png`, если он есть;
    /// `None` — значок не задан (берётся значок движка).
    pub fn icon_png(&self) -> Result<Option<Vec<u8>>> {
        let Some(path) = self.setting_str("icon")? else {
            return Ok(std::fs::read(self.assets_dir().join("icon.png")).ok().filter(|b| b.starts_with(b"\x89PNG")));
        };
        let file = self.assets_dir().join(path);
        let bytes = std::fs::read(&file).map_err(|e| {
            Error(format!(
                "icon `{path}` (from [package.metadata.piska] icon) cannot be read at `{}`: {e}",
                file.display()
            ))
        })?;
        if !bytes.starts_with(b"\x89PNG") {
            return Err(Error(format!("icon `{path}` must be a PNG image")));
        }
        Ok(Some(bytes))
    }

    /// Каталог результата: `--out` или `dist/<game>/<platform>`.
    pub fn out_dir(&self, out: Option<&Path>, platform: Platform) -> PathBuf {
        match out {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            Some(dir) => std::env::current_dir().map(|cwd| cwd.join(dir)).unwrap_or_else(|_| dir.to_path_buf()),
            None => self.root.join("dist").join(&self.game).join(platform.name()),
        }
    }

    /// Проверить ассеты (создать недостающие `.meta`, поймать дубли GUID) и собрать пак.
    pub fn pack_assets(&self) -> Result<Vec<u8>> {
        let dir = self.assets_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let database = AssetDatabase::open(&dir)?;
        let mut builder = AssetPack::builder();
        builder.add_dir(&dir)?;
        let bytes = builder.to_bytes();
        // Пак должен открываться так же, как его откроет игра.
        let packed = AssetDatabase::from_pack(AssetPack::from_bytes(bytes.clone())?)?;
        if packed.len() != database.len() {
            return Err(Error(format!(
                "the pack has {} assets but the directory has {}",
                packed.len(),
                database.len()
            )));
        }
        Ok(bytes)
    }
}

/// Пустой каталог результата: старые файлы прошлой сборки не должны попасть в новую.
pub fn fresh_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        // Удаляем только то, что сами кладём, — чужие файлы в выбранном каталоге не трогаем.
        for entry in std::fs::read_dir(dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let ours = matches!(
                name.as_str(),
                "index.html" | "assets.pak" | "assets" | "snippets" | "game.js" | "game_bg.wasm" | "game.d.ts"
                    | "game_bg.wasm.d.ts" | "icon.png" | "favicon.png"
            ) || [".exe", ".apk", ".apk.idsig", ".desktop", ".tar.gz"]
                .iter()
                .any(|ext| name.ends_with(ext));
            if ours {
                let path = entry.path();
                if path.is_dir() {
                    std::fs::remove_dir_all(&path)?;
                } else {
                    std::fs::remove_file(&path)?;
                }
            }
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| Error(format!("cannot create `{}`: {e}", dir.display())))
}

/// Скопировать каталог рекурсивно (скрытые файлы пропускаются).
pub fn copy_dir(from: &Path, to: &Path) -> Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut total = 0;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            total += copy_dir(&path, &to.join(&name))?;
        } else {
            total += std::fs::copy(&path, to.join(&name))?;
        }
    }
    Ok(total)
}
