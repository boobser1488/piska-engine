//! Что такое проект Piska и как его узнать: крейт игры с `Cargo.toml`, `src/lib.rs`,
//! зависимостью `engine` и (для открытия в редакторе) программой `<имя>-editor`.

use std::path::{Path, PathBuf};

use crate::config::HubConfig;
use crate::paths;

/// 2D или 3D — для значка в списке.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    TwoD,
    ThreeD,
    Unknown,
}

impl Dimension {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Dimension::TwoD => Some("2D"),
            Dimension::ThreeD => Some("3D"),
            Dimension::Unknown => None,
        }
    }
}

/// Проект, который прошёл проверку.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectInfo {
    /// Каталог крейта.
    pub dir: PathBuf,
    /// Имя пакета (`cargo -p`).
    pub package: String,
    /// Заголовок: `[package.metadata.piska] title`, иначе имя пакета.
    pub title: String,
    pub description: Option<String>,
    pub dimension: Dimension,
    /// C++-скрипты: `cpp/` с исходниками или `build.rs` с `engine-cpp-build`.
    pub has_cpp: bool,
    /// Скрипты Scenario (`*.scenario`).
    pub has_scenario: bool,
    /// Программа редактора (`[[bin]]` с фичей `editor`).
    pub editor_bin: Option<String>,
    /// Программа игры (`src/main.rs` → имя пакета, или `[[bin]]` без фичи `editor`).
    pub game_bin: Option<String>,
    /// Значок для списка: `icon` из `[package.metadata.piska]` или `assets/icon.png`.
    pub icon: Option<PathBuf>,
    /// Свой `[workspace]`: проект вне рабочего пространства движка.
    pub standalone: bool,
}

/// Почему каталог — не проект Piska. Сообщения объясняют, что сделать.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ProjectError {
    #[error("the folder `{0}` does not exist")]
    Missing(PathBuf),
    #[error("`{0}` is a file; choose the project folder (the one with Cargo.toml)")]
    NotAFolder(PathBuf),
    #[error("there is no Cargo.toml in `{0}`; a Piska project is a Rust crate (see docs/api/export.md §1)")]
    NoManifest(PathBuf),
    #[error("Cargo.toml in `{path}` is not valid TOML: {reason}")]
    BadManifest { path: PathBuf, reason: String },
    #[error("`{0}` is a Cargo workspace, not a game crate; choose one of its game folders (for the engine: games/<name>)")]
    WorkspaceOnly(PathBuf),
    #[error("`{0}` has no src/lib.rs; a Piska game is a library with `pub fn build()` (see docs/api/export.md §1.3)")]
    NoLibrary(PathBuf),
    #[error("`{0}` does not depend on the engine; add `engine = {{ path = \"…/crates/engine\" }}` to [dependencies]")]
    NoEngine(PathBuf),
}

impl ProjectInfo {
    /// Имя библиотеки (дефисы → подчёркивания).
    pub fn lib_name(&self) -> String {
        self.package.replace('-', "_")
    }

    /// Почему проект нельзя открыть в редакторе (если нельзя).
    pub fn editor_problem(&self) -> Option<String> {
        if self.editor_bin.is_some() {
            return None;
        }
        Some(format!(
            "`{}` has no editor program. Add `[features] editor = [\"engine/editor\"]`, \
             `[[bin]] name = \"{}-editor\"` with `required-features = [\"editor\"]` and \
             src/bin/{}-editor.rs with `engine::editor_main!({}::build);` (docs/api/editor.md)",
            self.package,
            self.package,
            self.package,
            self.lib_name()
        ))
    }
}

/// Проверить каталог и прочитать о проекте всё, что нужно списку.
pub fn inspect(dir: &Path) -> Result<ProjectInfo, ProjectError> {
    if !dir.exists() {
        return Err(ProjectError::Missing(dir.to_path_buf()));
    }
    if !dir.is_dir() {
        return Err(ProjectError::NotAFolder(dir.to_path_buf()));
    }
    let dir = paths::clean(dir);
    let manifest_path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path).map_err(|_| ProjectError::NoManifest(dir.clone()))?;
    let manifest: toml::Table = text.parse().map_err(|e: toml::de::Error| ProjectError::BadManifest {
        path: dir.clone(),
        reason: e.message().to_string(),
    })?;
    let Some(package) = manifest.get("package").and_then(|p| p.as_table()) else {
        return Err(if manifest.contains_key("workspace") {
            ProjectError::WorkspaceOnly(dir)
        } else {
            ProjectError::BadManifest {
                path: dir,
                reason: "there is no [package] table".to_string(),
            }
        });
    };
    let name = package
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| ProjectError::BadManifest {
            path: dir.clone(),
            reason: "[package] has no name".to_string(),
        })?
        .to_string();
    if !dir.join("src").join("lib.rs").is_file() {
        return Err(ProjectError::NoLibrary(dir));
    }
    let depends_on_engine = manifest
        .get("dependencies")
        .and_then(|d| d.as_table())
        .is_some_and(|deps| deps.contains_key("engine"));
    if !depends_on_engine {
        return Err(ProjectError::NoEngine(dir));
    }

    let piska = package
        .get("metadata")
        .and_then(|m| m.get("piska"))
        .and_then(|p| p.as_table());
    let setting = |key: &str| piska.and_then(|p| p.get(key)).and_then(|v| v.as_str()).map(str::to_string);
    let title = setting("title").unwrap_or_else(|| name.clone());
    let description = package.get("description").and_then(|d| d.as_str()).map(str::to_string);

    let bins: Vec<&toml::Table> = manifest
        .get("bin")
        .and_then(|b| b.as_array())
        .map(|array| array.iter().filter_map(|b| b.as_table()).collect())
        .unwrap_or_default();
    let needs_editor = |bin: &toml::Table| {
        bin.get("required-features")
            .and_then(|f| f.as_array())
            .is_some_and(|features| features.iter().any(|f| f.as_str() == Some("editor")))
    };
    let editor_bin = bins
        .iter()
        .filter(|bin| needs_editor(bin))
        .find_map(|bin| bin.get("name").and_then(|n| n.as_str()))
        .map(str::to_string);
    let game_bin = if dir.join("src").join("main.rs").is_file() {
        Some(name.clone())
    } else {
        bins.iter()
            .filter(|bin| !needs_editor(bin))
            .find_map(|bin| bin.get("name").and_then(|n| n.as_str()))
            .map(str::to_string)
    };

    let dimension = match setting("dimension").map(|d| d.to_ascii_lowercase()).as_deref() {
        Some("2d") => Dimension::TwoD,
        Some("3d") => Dimension::ThreeD,
        _ => guess_dimension(&dir),
    };
    let has_cpp = dir.join("cpp").is_dir() && has_file_with(&dir.join("cpp"), &["cpp", "cc", "hpp", "h"], 3)
        || std::fs::read_to_string(dir.join("build.rs")).is_ok_and(|text| text.contains("engine_cpp_build"));
    let has_scenario = has_file_with(&dir.join("assets"), &["scenario"], 6) || has_file_with(&dir.join("src"), &["scenario"], 4);
    let icon = setting("icon")
        .map(|path| dir.join("assets").join(path))
        .filter(|path| path.is_file())
        .or_else(|| Some(dir.join("assets").join("icon.png")).filter(|path| path.is_file()))
        .or_else(|| Some(dir.join("icon.png")).filter(|path| path.is_file()));

    Ok(ProjectInfo {
        standalone: manifest.contains_key("workspace"),
        package: name,
        title,
        description,
        dimension,
        has_cpp,
        has_scenario,
        editor_bin,
        game_bin,
        icon,
        dir,
    })
}

/// Есть ли в каталоге (до глубины `depth`) файл с одним из расширений.
fn has_file_with(dir: &Path, extensions: &[&str], depth: usize) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 && has_file_with(&path, extensions, depth - 1) {
                return true;
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| extensions.iter().any(|wanted| ext.eq_ignore_ascii_case(wanted)))
        {
            return true;
        }
    }
    false
}

/// 2D или 3D по тому, какие компоненты встречаются в коде и сценах.
fn guess_dimension(dir: &Path) -> Dimension {
    const THREE: [&str; 5] = ["MeshRenderer", "Rigidbody3D", "Collider3D", "Perspective", "CharacterController3D"];
    const TWO: [&str; 5] = ["SpriteRenderer", "Rigidbody2D", "Collider2D", "Tilemap", "SpriteSheet"];
    let mut budget: usize = 3_000_000;
    let mut score = (0usize, 0usize);
    for root in [dir.join("src"), dir.join("assets")] {
        scan(&root, 5, &mut budget, &mut |text| {
            score.0 += TWO.iter().map(|w| text.matches(w).count()).sum::<usize>();
            score.1 += THREE.iter().map(|w| text.matches(w).count()).sum::<usize>();
        });
    }
    match score {
        (0, 0) => Dimension::Unknown,
        (two, three) if three > two => Dimension::ThreeD,
        _ => Dimension::TwoD,
    }
}

fn scan(dir: &Path, depth: usize, budget: &mut usize, visit: &mut impl FnMut(&str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                scan(&path, depth - 1, budget, visit);
            }
            continue;
        }
        let wanted = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| matches!(ext, "rs" | "scene" | "prefab"));
        if !wanted {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            *budget = budget.saturating_sub(text.len());
            visit(&text);
        }
    }
}

/// Откуда проект в списке.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `games/*` рабочего пространства движка.
    Workspace,
    /// Добавлен пользователем (или создан хабом вне `games/`).
    Added,
}

/// Строка списка проектов.
#[derive(Clone, Debug)]
pub struct Listed {
    pub path: PathBuf,
    pub info: Result<ProjectInfo, ProjectError>,
    pub source: Source,
    pub favorite: bool,
    pub last_opened: Option<u64>,
    /// `games/lantern` — расположение относительно движка; `None` — вне его.
    pub location: Option<String>,
    /// Ключ пути ([`paths::key`]) — сравнение без обращения к диску в каждом кадре.
    pub key: String,
    /// Каталог игры-примера движка (`swarm`), если это он.
    pub sample: Option<&'static str>,
    /// Собранные программы и их свежесть.
    pub builds: crate::builds::Builds,
}

impl Listed {
    /// Каталога больше нет.
    pub fn missing(&self) -> bool {
        matches!(self.info, Err(ProjectError::Missing(_)))
    }

    /// Имя для показа: заголовок проекта или имя каталога.
    pub fn title(&self) -> String {
        match &self.info {
            Ok(info) => info.title.clone(),
            Err(_) => self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string()),
        }
    }

    /// Имя каталога (то, что знает `piska-export --game`).
    pub fn folder(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// Игры рабочего пространства движка: подкаталоги `games/` с `Cargo.toml`.
pub fn workspace_games(engine_root: &Path) -> Vec<PathBuf> {
    let mut games: Vec<PathBuf> = std::fs::read_dir(engine_root.join("games"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.join("Cargo.toml").is_file())
                .collect()
        })
        .unwrap_or_default();
    games.sort();
    games
}

/// Все проекты для списка: игры движка (кроме убранных) и записи файла хаба.
pub fn collect(engine_root: Option<&Path>, config: &HubConfig) -> Vec<Listed> {
    let mut listed = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut push = |path: &Path, source: Source, listed: &mut Vec<Listed>| {
        let key = paths::key(path);
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        let entry = config.entry(path);
        if entry.is_some_and(|e| e.hidden) {
            return;
        }
        let path = paths::clean(path);
        let info = inspect(&path);
        let builds = match &info {
            Ok(info) => crate::builds::inspect_builds(
                info,
                engine_root,
                entry.and_then(|e| e.editor_exe.as_deref()),
                entry.and_then(|e| e.game_exe.as_deref()),
            ),
            Err(_) => crate::builds::Builds::none(),
        };
        let sample = engine_root.and_then(|root| {
            crate::samples::SAMPLES
                .iter()
                .find(|s| paths::key(&root.join("games").join(s.folder)) == seen[seen.len() - 1])
                .map(|s| s.folder)
        });
        listed.push(Listed {
            info,
            source,
            favorite: entry.is_some_and(|e| e.favorite),
            last_opened: entry.and_then(|e| e.last_opened),
            location: engine_root.and_then(|root| paths::relative_to(&path, root)),
            key: seen[seen.len() - 1].clone(),
            sample,
            builds,
            path,
        });
    };
    if let Some(root) = engine_root {
        for game in workspace_games(root) {
            push(&game, Source::Workspace, &mut listed);
        }
    }
    for entry in &config.projects {
        let in_workspace = engine_root.is_some_and(|root| paths::is_within(&entry.path, &root.join("games")));
        // Убранная из списка игра движка, которой больше нет, не всплывает «потерянной».
        if in_workspace && !entry.path.exists() {
            continue;
        }
        push(&entry.path, if in_workspace { Source::Workspace } else { Source::Added }, &mut listed);
    }
    listed
}

/// Отсортировать: закреплённые сверху, затем по выбранному порядку.
pub fn sort(listed: &mut [Listed], order: crate::config::SortOrder) {
    listed.sort_by(|a, b| {
        b.favorite.cmp(&a.favorite).then_with(|| match order {
            crate::config::SortOrder::LastOpened => b
                .last_opened
                .cmp(&a.last_opened)
                .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase())),
            crate::config::SortOrder::Name => a.title().to_lowercase().cmp(&b.title().to_lowercase()),
        })
    });
}

/// Подходит ли проект под строку поиска (имя, заголовок, путь; без учёта регистра).
pub fn matches(listed: &Listed, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let mut haystack = format!("{} {}", listed.title(), listed.path.display()).to_lowercase();
    if let Ok(info) = &listed.info {
        haystack.push(' ');
        haystack.push_str(&info.package.to_lowercase());
        if let Some(d) = info.dimension.label() {
            haystack.push(' ');
            haystack.push_str(&d.to_lowercase());
        }
    }
    query.split_whitespace().all(|word| haystack.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piska-hub-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_engine_games_are_valid_projects() {
        let root = paths::find_engine_root().unwrap();
        let template = inspect(&root.join("games").join("template")).unwrap();
        assert_eq!(template.package, "template");
        assert_eq!(template.editor_bin.as_deref(), Some("template-editor"));
        assert_eq!(template.game_bin.as_deref(), Some("template"));
        assert_eq!(template.dimension, Dimension::TwoD);
        assert!(!template.standalone);
        if root.join("games").join("marble").is_dir() {
            let marble = inspect(&root.join("games").join("marble")).unwrap();
            assert!(marble.has_cpp, "marble builds cpp/*.cpp");
            assert_eq!(marble.dimension, Dimension::ThreeD);
        }
    }

    #[test]
    fn validation_explains_what_is_wrong() {
        let dir = temp("validate");
        let missing = dir.join("nope");
        assert_eq!(inspect(&missing), Err(ProjectError::Missing(missing.clone())));
        let message = inspect(&dir).unwrap_err().to_string();
        assert!(message.contains("no Cargo.toml"), "{message}");

        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        assert!(inspect(&dir).unwrap_err().to_string().contains("is a Cargo workspace"));

        std::fs::write(dir.join("Cargo.toml"), "[package\nname = ").unwrap();
        assert!(inspect(&dir).unwrap_err().to_string().contains("not valid TOML"));

        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"g\"\n").unwrap();
        assert!(inspect(&dir).unwrap_err().to_string().contains("no src/lib.rs"));

        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src").join("lib.rs"), "").unwrap();
        assert!(inspect(&dir).unwrap_err().to_string().contains("does not depend on the engine"));

        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"g\"\n[dependencies]\nengine = { path = \"x\" }\n").unwrap();
        let info = inspect(&dir).unwrap();
        assert_eq!(info.editor_bin, None);
        assert!(info.editor_problem().unwrap().contains("g-editor"));
        assert_eq!(info.dimension, Dimension::Unknown);

        let file = dir.join("Cargo.toml");
        assert!(inspect(&file).unwrap_err().to_string().contains("is a file"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn discovery_lists_games_and_added_folders_and_hides_removed_ones() {
        let root = paths::find_engine_root().unwrap();
        let outside = temp("discover");
        let mut config = HubConfig::default();
        config.entry_mut(&outside.join("gone")).last_opened = Some(5);
        config.entry_mut(&root.join("games").join("template")).hidden = true;
        let listed = collect(Some(&root), &config);
        assert!(listed.iter().all(|l| l.folder() != "template"), "removed from the list");
        let gone = listed.iter().find(|l| l.folder() == "gone").expect("added folders are listed");
        assert!(gone.missing());
        assert_eq!(gone.source, Source::Added);
        assert_eq!(gone.location, None);
        if let Some(swarm) = listed.iter().find(|l| l.folder() == "swarm") {
            assert_eq!(swarm.location.as_deref(), Some("games/swarm"));
            assert_eq!(swarm.source, Source::Workspace);
            assert!(matches(swarm, "SWARM"));
            assert!(matches(swarm, "games swarm"));
            assert!(!matches(swarm, "lantern"));
        }

        // Закреплённые сверху, затем недавние.
        let mut config = HubConfig::default();
        config.touch(&root.join("games").join("template"), 100);
        if root.join("games").join("swarm").is_dir() {
            config.entry_mut(&root.join("games").join("swarm")).favorite = true;
        }
        let mut listed = collect(Some(&root), &config);
        sort(&mut listed, crate::config::SortOrder::LastOpened);
        let order: Vec<String> = listed.iter().map(Listed::folder).collect();
        if root.join("games").join("swarm").is_dir() {
            assert_eq!(order[0], "swarm");
            assert_eq!(order[1], "template");
        } else {
            assert_eq!(order[0], "template");
        }
        std::fs::remove_dir_all(&outside).unwrap();
    }
}
