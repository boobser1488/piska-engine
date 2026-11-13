//! Игра в редакторе: программа `<game>-editor` из той же функции `build`, что и
//! сама игра. Подробно — `docs/api/editor.md`, раздел «Скрипты на Rust».
//!
//! ```ignore
//! // games/<name>/src/bin/<name>-editor.rs
//! engine::editor_main!(my_game::build, |config| config.open_scene("scenes/main.scene"));
//! ```
//!
//! Запуск: `cargo run -p <name> --features editor --bin <name>-editor`.

use std::path::{Path, PathBuf};

use engine_editor::{EditorConfig, Relaunch};

use crate::{BuildFn, EngineError};

/// Что макрос [`editor_main!`](crate::editor_main) знает о крейте игры (из `env!`
/// в месте вызова).
#[derive(Clone, Copy, Debug)]
pub struct GameProject {
    /// Имя пакета (`template`).
    pub package: &'static str,
    /// Имя программы редактора (`template-editor`).
    pub bin: &'static str,
    /// Каталог крейта игры.
    pub manifest_dir: &'static str,
    /// `CARGO_TARGET_DIR`, с которым программа была собрана (если был задан).
    pub target_dir: Option<&'static str>,
}

impl GameProject {
    /// Настройки редактора для игры: код из `src/`, скрипты на Rust в `src/scripts/`,
    /// C++ из `cpp/` (если он есть), Build пересобирает `<bin>` с `--features editor`
    /// и перезапускает редактор, File → Build… экспортирует этот крейт.
    pub fn editor_config(&self) -> EditorConfig {
        let dir = Path::new(self.manifest_dir);
        let src = dir.join("src");
        let mut relaunch = Relaunch::bin(self.package, self.bin, &["editor"]).dir(dir);
        if let Some(target) = self.target_dir {
            relaunch = relaunch.target_dir(target);
        }
        let mut config = EditorConfig::titled(format!("Piska Editor: {}", self.package))
            .code_dir(&src)
            .rust_scripts(src.join("scripts"))
            .relaunch(relaunch)
            .game_crate(self.package);
        // Игры живут в `<workspace>/games/<name>/`: экспорт ищет их от корня.
        if let Some(workspace) = workspace_of(dir) {
            config = config.workspace(workspace);
        }
        // C++, вкомпилированный в игру (`build.rs` + `engine-cpp-build`): новые скрипты
        // ложатся в `cpp/`, и Build пересобирает их вместе с Rust.
        let cpp = dir.join("cpp");
        if cpp.is_dir() {
            config = config.code_dir(&cpp).cpp_scripts(&cpp);
        }
        config
    }
}

fn workspace_of(crate_dir: &Path) -> Option<PathBuf> {
    let games = crate_dir.parent()?;
    if games.file_name()? != "games" {
        return None;
    }
    let root = games.parent()?;
    root.join("Cargo.toml").is_file().then(|| root.to_path_buf())
}

/// Собрать игру и открыть её в редакторе. Вызывается из [`editor_main!`](crate::editor_main).
///
/// Перезапущенный после Build редактор (`--editor-session <файл>`) возвращает
/// прежнее состояние сам (см. [`crate::edit`]).
pub fn run_editor(
    build: BuildFn,
    project: GameProject,
    configure: impl FnOnce(EditorConfig) -> EditorConfig,
) -> Result<(), EngineError> {
    crate::init_logger();
    // `build()` загружает сцену: компонент, которого больше нет в коде, не должен её губить.
    engine_core::missing::keep_missing_scripts(true);
    let app = build()?;
    crate::edit(app, configure(project.editor_config()))
}

/// Программа редактора для крейта игры: `fn main`, который собирает игру функцией
/// `build` и открывает её в редакторе с пересборкой на Rust (Build → перезапуск).
///
/// ```ignore
/// // games/<name>/src/bin/<name>-editor.rs
/// engine::editor_main!(my_game::build);
/// // или с настройкой: какую сцену открыть, что ещё показать.
/// engine::editor_main!(my_game::build, |config| config.open_scene("scenes/main.scene"));
/// ```
///
/// В `Cargo.toml` игры: `[features] editor = ["engine/editor"]` и
/// `[[bin]] name = "<name>-editor"`, `required-features = ["editor"]` — тогда экспорт
/// и обычная сборка редактор не собирают.
#[macro_export]
macro_rules! editor_main {
    ($build:path $(,)?) => {
        $crate::editor_main!($build, |config| config);
    };
    ($build:path, $configure:expr $(,)?) => {
        fn main() -> ::core::result::Result<(), $crate::EngineError> {
            $crate::editor_project::run_editor(
                $build,
                $crate::editor_project::GameProject {
                    package: env!("CARGO_PKG_NAME"),
                    bin: env!("CARGO_BIN_NAME"),
                    manifest_dir: env!("CARGO_MANIFEST_DIR"),
                    target_dir: option_env!("CARGO_TARGET_DIR"),
                },
                $configure,
            )
        }
    };
}
