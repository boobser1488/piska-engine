//! Собранные программы проекта: где лежит программа редактора (и игры), свежая ли она.
//!
//! «Свежая» — новее всех *собственных* входов проекта: `src/`, `cpp/`, `Cargo.toml`,
//! `build.rs`. Изменения движка в фоне на это не влияют: Open запускает готовую программу
//! сразу, а пересобирать — по явной команде (Rebuild and Open) или когда менялся сам проект.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::project::ProjectInfo;

/// Состояние программы проекта.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildState {
    /// Собрана после последней правки проекта (время сборки — секунды Unix).
    Fresh { built: u64 },
    /// Собрана, но проект менялся после (время сборки).
    Stale { built: u64 },
    /// Ни разу не собиралась (программы нет).
    Never,
}

impl BuildState {
    /// Когда собрана (если собрана).
    pub fn built(self) -> Option<u64> {
        match self {
            BuildState::Fresh { built } | BuildState::Stale { built } => Some(built),
            BuildState::Never => None,
        }
    }
}

/// Программы проекта и их состояние.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Builds {
    pub editor: BuildState,
    pub editor_exe: Option<PathBuf>,
    pub game: BuildState,
    pub game_exe: Option<PathBuf>,
}

impl Builds {
    pub fn none() -> Self {
        Self {
            editor: BuildState::Never,
            editor_exe: None,
            game: BuildState::Never,
            game_exe: None,
        }
    }
}

/// Каталог сборки проекта: `CARGO_TARGET_DIR` хаба (относительный — от каталога, где
/// запускается cargo), иначе `target/` рабочего пространства: движка для игр из `games/`,
/// свой — у отдельного проекта.
pub fn target_dir(info: &ProjectInfo, engine_root: Option<&Path>) -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR").filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        return if dir.is_absolute() { dir } else { info.dir.join(dir) };
    }
    match (info.standalone, engine_root) {
        (false, Some(root)) => root.join("target"),
        _ => info.dir.join("target"),
    }
}

/// Где cargo кладёт программу `bin` (профиль dev).
pub fn exe_path(info: &ProjectInfo, bin: &str, engine_root: Option<&Path>) -> PathBuf {
    target_dir(info, engine_root)
        .join("debug")
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX))
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

fn unix(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Самая поздняя правка собственных входов проекта.
pub fn newest_input(dir: &Path) -> Option<SystemTime> {
    let mut newest = None;
    let mut consider = |time: Option<SystemTime>| {
        if let Some(time) = time {
            newest = Some(newest.map_or(time, |n: SystemTime| n.max(time)));
        }
    };
    consider(mtime(&dir.join("Cargo.toml")));
    consider(mtime(&dir.join("build.rs")));
    for sub in ["src", "cpp"] {
        walk(&dir.join(sub), 8, &mut |path| consider(mtime(path)));
    }
    newest
}

fn walk(dir: &Path, depth: usize, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                walk(&path, depth - 1, visit);
            }
        } else {
            visit(&path);
        }
    }
}

/// Состояние программы по её файлу и последней правке проекта.
pub fn state_of(exe: Option<&Path>, newest_input: Option<SystemTime>) -> BuildState {
    let Some(built) = exe.and_then(mtime) else {
        return BuildState::Never;
    };
    match newest_input {
        Some(input) if input > built => BuildState::Stale { built: unix(built) },
        _ => BuildState::Fresh { built: unix(built) },
    }
}

/// Программа: запомненная после прошлой сборки (если файл есть), иначе там, где её
/// положил бы cargo (если файл есть).
fn find_exe(remembered: Option<&Path>, computed: PathBuf) -> Option<PathBuf> {
    remembered
        .filter(|p| p.is_file())
        .map(Path::to_path_buf)
        .or_else(|| computed.is_file().then_some(computed))
}

/// Программы редактора и игры проекта. Читает диск — вызывается в фоновом потоке.
pub fn inspect_builds(
    info: &ProjectInfo,
    engine_root: Option<&Path>,
    remembered_editor: Option<&Path>,
    remembered_game: Option<&Path>,
) -> Builds {
    let newest = newest_input(&info.dir);
    let editor_exe = info
        .editor_bin
        .as_deref()
        .and_then(|bin| find_exe(remembered_editor, exe_path(info, bin, engine_root)));
    let game_exe = info
        .game_bin
        .as_deref()
        .and_then(|bin| find_exe(remembered_game, exe_path(info, bin, engine_root)));
    Builds {
        editor: state_of(editor_exe.as_deref(), newest),
        game: state_of(game_exe.as_deref(), newest),
        editor_exe,
        game_exe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn touch(path: &Path, time: SystemTime) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        if !path.exists() {
            std::fs::write(path, "x").unwrap();
        }
        std::fs::File::options().write(true).open(path).unwrap().set_modified(time).unwrap();
    }

    #[test]
    fn a_binary_newer_than_the_projects_own_inputs_is_fresh() {
        let dir = std::env::temp_dir().join(format!("piska-hub-builds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let base = SystemTime::now() - Duration::from_secs(3600);
        let exe = dir.join("target").join("game-editor.exe");
        touch(&dir.join("Cargo.toml"), base);
        touch(&dir.join("src").join("lib.rs"), base);
        touch(&dir.join("src").join("scripts").join("mod.rs"), base);
        assert_eq!(state_of(Some(&exe), newest_input(&dir)), BuildState::Never);

        touch(&exe, base + Duration::from_secs(60));
        let fresh = state_of(Some(&exe), newest_input(&dir));
        assert!(matches!(fresh, BuildState::Fresh { .. }), "{fresh:?}");
        assert_eq!(fresh.built(), Some(unix(base + Duration::from_secs(60))));

        // Ассеты не входят: их читает уже собранная программа.
        touch(&dir.join("assets").join("scenes").join("main.scene"), base + Duration::from_secs(600));
        assert!(matches!(state_of(Some(&exe), newest_input(&dir)), BuildState::Fresh { .. }));

        // Правка скрипта глубоко в src/ — нужна сборка.
        touch(&dir.join("src").join("scripts").join("mod.rs"), base + Duration::from_secs(120));
        assert!(matches!(state_of(Some(&exe), newest_input(&dir)), BuildState::Stale { .. }));
        // Как и C++ или build.rs.
        touch(&exe, base + Duration::from_secs(200));
        touch(&dir.join("cpp").join("Wobble.cpp"), base + Duration::from_secs(300));
        assert!(matches!(state_of(Some(&exe), newest_input(&dir)), BuildState::Stale { .. }));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_remembered_binary_wins_and_missing_files_mean_never_built() {
        let dir = std::env::temp_dir().join(format!("piska-hub-builds-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let remembered = dir.join("old.exe");
        let computed = dir.join("new.exe");
        assert_eq!(find_exe(Some(&remembered), computed.clone()), None);
        std::fs::write(&computed, "x").unwrap();
        assert_eq!(find_exe(Some(&remembered), computed.clone()), Some(computed.clone()));
        std::fs::write(&remembered, "x").unwrap();
        assert_eq!(find_exe(Some(&remembered), computed), Some(remembered));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
