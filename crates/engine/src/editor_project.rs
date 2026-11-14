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

// ---------------------------------------------------------------------------
// Любой проект без компиляции: piska-editor / piska-player
// ---------------------------------------------------------------------------

/// Что общий редактор и проигрыватель узнали о каталоге проекта.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectDir {
    /// Каталог проекта (с `assets/`, обычно и с `Cargo.toml`).
    pub dir: PathBuf,
    /// Имя пакета из `Cargo.toml` (`None` — проект без крейта: только ассеты).
    pub package: Option<String>,
    /// Заголовок: `[package.metadata.piska] title`, иначе имя пакета или каталога.
    pub title: String,
    /// Программа редактора проекта (`[[bin]]` с фичей `editor`).
    pub editor_bin: Option<String>,
}

impl ProjectDir {
    /// Прочитать `Cargo.toml` проекта (без разбора TOML-зависимостей: только нужные строки).
    pub fn read(dir: &Path) -> Result<Self, EngineError> {
        if !dir.join("assets").is_dir() {
            return Err(EngineError::Editor(engine_editor::EditorError::Other(format!(
                "`{}` has no assets/ folder; open the project folder (the one with Cargo.toml and assets/)",
                dir.display()
            ))));
        }
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
        let mut section = String::new();
        let (mut package, mut title, mut bin_name, mut editor_bin) = (None, None, None, None);
        let value = |line: &str| line.split_once('=').map(|(_, v)| v.trim().trim_matches('"').to_string());
        for line in manifest.lines().map(str::trim) {
            if line.starts_with('[') {
                section = line.to_string();
                bin_name = None;
                continue;
            }
            match section.as_str() {
                "[package]" if line.starts_with("name") => package = value(line),
                "[package.metadata.piska]" if line.starts_with("title") => title = value(line),
                "[[bin]]" if line.starts_with("name") => bin_name = value(line),
                "[[bin]]" if line.starts_with("required-features") && line.contains("\"editor\"") => {
                    editor_bin = bin_name.clone().or(editor_bin);
                }
                _ => {}
            }
        }
        let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self {
            title: title.or_else(|| package.clone()).unwrap_or(folder),
            dir: dir.to_path_buf(),
            package,
            editor_bin,
        })
    }

    /// Стартовая сцена: `PlayerSettings::default_scene` из `project.settings`, иначе
    /// `scenes/main.scene`, иначе первая `.scene` в ассетах (по алфавиту).
    pub fn start_scene(&self, app: &engine_core::app::App) -> Option<String> {
        let from_settings = app
            .world
            .resources
            .get::<engine_core::settings::ProjectSettings>()
            .map(|s| s.player.default_scene.trim().to_string())
            .filter(|s| !s.is_empty());
        if from_settings.is_some() {
            return from_settings;
        }
        let assets = self.dir.join("assets");
        if assets.join("scenes").join("main.scene").is_file() {
            return Some("scenes/main.scene".to_string());
        }
        let mut scenes = Vec::new();
        collect_scenes(&assets, &assets, &mut scenes, 4);
        scenes.sort();
        scenes.into_iter().next()
    }
}

fn collect_scenes(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth > 0 {
            collect_scenes(root, &path, out, depth - 1);
        } else if path.extension().is_some_and(|e| e == "scene") {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// Приложение проекта без его кода: все компоненты движка и Scenario, ассеты проекта,
/// стартовая сцена. Компоненты, которых нет в движке (скрипты проекта на Rust / C++),
/// остаются в сцене как «missing script» и сохраняются как были.
pub fn project_app(project: &ProjectDir) -> Result<(engine_core::app::App, Option<String>), EngineError> {
    engine_core::missing::keep_missing_scripts(true);
    let mut app = crate::app();
    crate::mount_assets(&mut app, project.dir.join("assets"))?;
    let scene = project.start_scene(&app);
    if let Some(scene) = &scene {
        crate::load_scene(&mut app, scene)?;
    }
    Ok((app, scene))
}

/// Общий редактор (`piska-editor --project <каталог>`): открыть проект, **ничего не
/// компилируя**. Если у проекта есть крейт с программой редактора, настройки те же,
/// что у неё: New Rust Script / Build собирают программу редактора проекта и
/// перезапускаются в неё с той же сценой (как Build в `editor_main!`).
///
/// `build_now` — сразу начать эту сборку (у проекта есть свой код на Rust / C++,
/// а его программа редактора ещё не собрана: пока она собирается, работает общий
/// редактор, потом он перезапускается в программу проекта).
pub fn open_project(dir: &Path, target_dir: Option<&'static str>, build_now: bool) -> Result<(), EngineError> {
    crate::init_logger();
    let dir = dir.canonicalize().map(|d| strip_verbatim(&d)).unwrap_or_else(|_| dir.to_path_buf());
    let project = ProjectDir::read(&dir)?;
    let (app, scene) = project_app(&project)?;
    let mut config = match (&project.package, &project.editor_bin) {
        (Some(package), Some(bin)) => GameProject {
            // Строки живут до конца программы: их один набор на процесс.
            package: Box::leak(package.clone().into_boxed_str()),
            bin: Box::leak(bin.clone().into_boxed_str()),
            manifest_dir: Box::leak(dir.display().to_string().into_boxed_str()),
            target_dir,
        }
        .editor_config(),
        _ => EditorConfig::titled(format!("Piska Editor: {}", project.title)),
    };
    if let Some(scene) = scene {
        config = config.open_scene(scene);
    }
    if build_now && project.editor_bin.is_some() {
        log::info!("the project's own scripts are building; this editor switches to them when the build is done");
        std::env::set_var("PISKA_BUILD_ON_START", "1");
    }
    crate::edit(app, config)
}

/// Общий проигрыватель (`piska-player --project <каталог>`): запустить игру проекта без
/// компиляции — компоненты движка и Scenario, ассеты и стартовая сцена проекта.
pub fn play_project(dir: &Path) -> Result<(), EngineError> {
    crate::init_logger();
    let project = ProjectDir::read(dir)?;
    // В игре неизвестный компонент — ошибка сцены, как и в собранной игре.
    let mut app = crate::app();
    crate::mount_assets(&mut app, project.dir.join("assets"))?;
    match project.start_scene(&app) {
        Some(scene) => crate::load_scene(&mut app, &scene)?,
        None => {
            return Err(EngineError::Editor(engine_editor::EditorError::Other(format!(
                "`{}` has no scene to start: add assets/scenes/main.scene or set the default scene in the project settings",
                project.dir.display()
            ))))
        }
    }
    crate::run(app, engine_render::WindowConfig::titled(project.title.clone()).with_size(1280, 720))
}

/// `\\?\C:\…` (результат `canonicalize` на Windows) не понимают cargo и проводник.
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
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
