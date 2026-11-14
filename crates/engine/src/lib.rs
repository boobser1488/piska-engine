//! `engine` — фасад: собирает ядро, рендер, физику и ассеты в готовое приложение.
//!
//! ```no_run
//! use engine::prelude::*;
//!
//! fn main() -> Result<(), EngineError> {
//!     let mut app = engine::app();
//!     app.spawn("Camera").with(Camera::default());
//!     app.spawn("Box").with(SpriteRenderer::colored(Color::hex(0xff7a59)));
//!     engine::run(app, WindowConfig::titled("Hello"))
//! }
//! ```
#![forbid(unsafe_code)]

use std::fmt;
use std::path::Path;

// Какая возможность за что отвечает — docs/design/engine-features.md. Выключенная
// подсистема исчезает из фасада целиком (модуль, prelude, `app()`) и не линкуется.
pub use engine_assets as assets;
/// Звук. Нужна возможность `audio`.
#[cfg(feature = "audio")]
pub use engine_audio as audio;
pub use engine_core as core;
/// Граница Rust ↔ C++. Нужна возможность `cpp`.
#[cfg(feature = "cpp")]
pub use engine_cpp_bridge as cpp;
#[cfg(feature = "editor")]
pub use engine_editor as editor;
/// Физика. Нужна возможность `physics2d` и / или `physics3d`.
#[cfg(any(feature = "physics2d", feature = "physics3d"))]
pub use engine_physics as physics;
pub use engine_render as render;
/// Игровой интерфейс. Нужна возможность `ui`.
#[cfg(feature = "ui")]
pub use engine_ui as ui;

#[cfg(feature = "cpp")]
pub mod cpp_services;
#[cfg(feature = "editor")]
pub mod editor_project;
/// Геймпады. Нужна возможность `gamepad`.
#[cfg(feature = "gamepad")]
pub mod gamepad;
mod game;
/// Язык сценариев Scenario. Нужна возможность `scenario`.
#[cfg(feature = "scenario")]
pub mod scenario;
/// Коллайдеры рельефа. Нужна возможность `physics3d`.
#[cfg(feature = "physics3d")]
pub mod terrain_physics;
/// Контроллер от первого лица (`FpsController`). Нужна возможность `physics3d`.
#[cfg(feature = "physics3d")]
pub mod fps;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_os = "android")]
pub mod android;

pub use game::{game_assets, run_game, save_data, with_game_icon, BuildFn};

use engine_assets::{AssetDatabase, AssetScenes};
use engine_core::app::App;
use engine_render::WindowConfig;

pub mod prelude {
    pub use crate::EngineError;
    pub use engine_assets::{AlphaMode, AssetDatabase, AssetHandle, Guid, Material, Materials, Mesh, Texture};
    #[cfg(feature = "audio")]
    #[allow(unused_imports)]
    pub use engine_audio::prelude::*;
    #[cfg(feature = "ui")]
    #[allow(unused_imports)]
    pub use engine_ui::prelude::*;
    pub use engine_core::prelude::*;
    #[cfg(any(feature = "physics2d", feature = "physics3d"))]
    pub use engine_physics::BodyType;
    #[cfg(feature = "physics2d")]
    pub use engine_physics::{Collider2D, Physics2D, RaycastHit2D, Rigidbody2D, Shape2D};
    #[cfg(feature = "physics3d")]
    pub use engine_physics::{
        CharacterController, Collider, JointId, Physics3D, RaycastHit3D, Rigidbody, Shape3D,
    };
    #[cfg(feature = "physics3d")]
    pub use engine_physics::{CastShape3D, QueryFilter3D, ShapeHit3D};
    #[cfg(feature = "physics3d")]
    pub use crate::fps::{FpsController, FpsRuntime, UserCmd};
    #[cfg(feature = "scenario")]
    pub use crate::scenario::ScenarioRunner;
    pub use engine_core::settings::{FullscreenMode, GraphicsSettings, QualityLevel};
    pub use engine_render::{
        Background, Camera, ColorGrading, Environment, Fog, Light, LightKind, MeshRenderer, MeshShape, Projection,
        SkyColors,
        RenderStats,
        SpriteRenderer, WindowConfig,
    };
    pub use engine_render::{
        AnimationClip, AnimationFinished, CameraController2D, ClipMode, EmitterShape, ParticleBlend,
        ParticleShape, ParticleSystem, SimulationSpace, SpriteAnimator, TileCollision, Tilemap,
        TrailRenderer,
    };
    pub use engine_render::{
        snap_to_terrain, terrain_height, terrain_normal, terrain_raycast, BrushTool, Terrain, TerrainBrush,
        TerrainHit, TerrainSource, Water,
    };
    #[cfg(feature = "editor")]
    pub use engine_editor::{BuildCommand, EditorConfig, Relaunch};
}

/// Любая ошибка движка. `Debug` печатает человекочитаемую цепочку причин, поэтому
/// `fn main() -> Result<(), EngineError>` даёт понятное сообщение, а не дамп структуры.
#[derive(thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Scene(#[from] engine_core::error::SceneError),
    #[error(transparent)]
    Asset(#[from] engine_assets::AssetError),
    #[error(transparent)]
    Render(#[from] engine_render::RenderError),
    #[cfg(feature = "cpp")]
    #[error(transparent)]
    Plugin(#[from] engine_cpp_bridge::PluginError),
    #[cfg(feature = "cpp")]
    #[error(transparent)]
    Noise(#[from] engine_cpp_bridge::noise::NoiseError),
    #[cfg(feature = "editor")]
    #[error(transparent)]
    Editor(#[from] engine_editor::EditorError),
    #[error("cannot write image `{path}`: {reason}")]
    Image { path: String, reason: String },
}

impl fmt::Debug for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")?;
        let mut source = std::error::Error::source(self);
        while let Some(cause) = source {
            write!(f, "\n  caused by: {cause}")?;
            source = cause.source();
        }
        Ok(())
    }
}

/// Возможности движка, включённые в эту сборку (имена из `[features]` крейта `engine`):
/// `piska-export` печатает их при экспорте, игра может записать их в лог.
pub fn enabled_features() -> Vec<&'static str> {
    let all: [(&str, bool); 12] = [
        ("physics2d", cfg!(feature = "physics2d")),
        ("physics3d", cfg!(feature = "physics3d")),
        ("audio", cfg!(feature = "audio")),
        ("audio-wav", cfg!(feature = "audio-wav")),
        ("audio-ogg", cfg!(feature = "audio-ogg")),
        ("ui", cfg!(feature = "ui")),
        ("cpp", cfg!(feature = "cpp")),
        ("scenario", cfg!(feature = "scenario")),
        ("gamepad", cfg!(feature = "gamepad")),
        ("mesh-gltf", cfg!(feature = "mesh-gltf")),
        ("image-jpeg", cfg!(feature = "image-jpeg")),
        ("webgl", cfg!(feature = "webgl")),
    ];
    all.into_iter().filter(|(_, on)| *on).map(|(name, _)| name).collect()
}

/// Приложение «как новый проект Unity»: компоненты рендера и физики
/// зарегистрированы, 2D- и 3D-физика подключены, лог пишется в stderr.
pub fn app() -> App {
    init_logger();
    let mut app = App::new();
    engine_render::register_components(&mut app.world.registry);
    #[cfg(any(feature = "physics2d", feature = "physics3d"))]
    engine_physics::register_components(&mut app.world.registry);
    #[cfg(feature = "gamepad")]
    app.add_subsystem(gamepad::GamepadSubsystem::new());
    // Ресурсы физики есть сразу, а не с первого кадра: игра настраивает гравитацию
    // прямо в своей `build()`.
    #[cfg(feature = "physics2d")]
    {
        app.world.resources.insert(engine_physics::Physics2D::default());
        app.add_subsystem(engine_physics::Physics2DSystem);
        // Разделы Physics 2D / Physics настроек проекта применяются при `mount_assets` и при
        // каждом `World::apply_project_settings` (редактор).
        app.world.add_settings_hook("physics2d", engine_physics::apply_project_settings_2d);
    }
    #[cfg(feature = "physics3d")]
    {
        app.world.resources.insert(engine_physics::Physics3D::default());
        app.add_subsystem(engine_physics::Physics3DSystem);
        app.world.add_settings_hook("physics3d", engine_physics::apply_project_settings_3d);
        // Коллайдеры рельефа (`Terrain`) для 3D-физики.
        app.add_subsystem(terrain_physics::TerrainPhysicsSystem::default());
        // Контроллер от первого лица.
        fps::register(&mut app.world.registry);
        #[cfg(feature = "cpp")]
        terrain_physics::install_services(&mut app.world);
    }
    #[cfg(feature = "audio")]
    engine_audio::install(&mut app);
    #[cfg(feature = "ui")]
    engine_ui::install(&mut app);
    #[cfg(feature = "cpp")]
    cpp_services::install(&mut app);
    #[cfg(feature = "scenario")]
    scenario::install(&mut app);
    app
}

/// Подключить ассеты игры: положить базу в `world.resources`, научить загрузчик сцен
/// находить в ней префабы, а `ctx.load_scene("level2")` — сцены (`level2.scene`,
/// `scenes/level2.scene` относительно корня ассетов).
///
/// На десктопе это каталог `root` (недостающие `.meta` создаются). В браузере и на
/// Android каталога нет: ассеты приходят паком, установленным до старта игры
/// ([`engine_assets::install_pack`]), и `root` не используется — код игры один.
pub fn mount_assets(app: &mut App, root: impl AsRef<Path>) -> Result<(), EngineError> {
    let database = AssetDatabase::mount(root)?;
    app.world.prefabs = Box::new(database.clone());
    app.world.scenes().set_source(AssetScenes::new(database.clone()));
    // Настройки проекта (`project.settings`): файла нет — всё по умолчанию. Он же лежит
    // в паке ассетов, поэтому экспортированная игра получает их тоже. Ядро применяет
    // время, ввод и порядок скриптов, подсистемы — свои разделы (хуки: физика, звук);
    // графику рендер читает из ресурса каждый кадр. См. docs/api/settings.md.
    if let Ok(bytes) = database.read_path(engine_core::settings::PROJECT_SETTINGS_FILE) {
        let text = String::from_utf8_lossy(&bytes);
        match engine_core::settings::ProjectSettings::from_ron(&text, engine_core::settings::PROJECT_SETTINGS_FILE) {
            Ok(settings) => app.world.apply_project_settings(settings),
            Err(error) => log::warn!("{error}; using the default project settings"),
        }
    }
    app.world.resources.insert(database);
    Ok(())
}

/// Конфиг окна игры: `config` из кода + раздел Player настроек проекта (если
/// настройки подключены [`mount_assets`]): заголовок, размер, resizable, полный экран,
/// vsync, ограничение FPS (см. [`WindowConfig::with_player_settings`]) и значок
/// `PlayerSettings::icon` из базы ассетов. Вызывается из [`run`] сам.
pub fn player_window_config(world: &engine_core::world::World, config: WindowConfig) -> WindowConfig {
    let Some(settings) = world.resources.get::<engine_core::settings::ProjectSettings>() else {
        return config;
    };
    let player = &settings.player;
    let mut config = config.with_player_settings(player);
    let icon = player.icon.trim();
    if !icon.is_empty() {
        match world.resources.get::<AssetDatabase>().map(|db| db.read_path(icon)) {
            Some(Ok(png)) => config.icon = Some(png.into_owned()),
            Some(Err(error)) => log::warn!("window icon `{icon}` from the project settings: {error}"),
            None => log::warn!("window icon `{icon}`: no assets mounted (call `engine::mount_assets`)"),
        }
    }
    config
}

/// Сцена по умолчанию (`PlayerSettings::default_scene`): загружается, только если
/// `build()` игры не создала ни одного объекта — сцену, собранную кодом, настройка не
/// заменяет. Возвращает `true`, если сцена загружена. Вызывается из
/// [`run_game`] (и точек входа браузера и Android) после `build()`.
pub fn load_default_scene(app: &mut App) -> Result<bool, EngineError> {
    let Some(settings) = app.world.resources.get::<engine_core::settings::ProjectSettings>() else {
        return Ok(false);
    };
    let path = settings.player.default_scene.trim().to_string();
    if path.is_empty() || !app.world.scene.is_empty() {
        return Ok(false);
    }
    load_scene(app, &path)?;
    log::info!("loaded the default scene `{path}` from the project settings");
    Ok(true)
}

/// Загрузить сцену из подключённых ассетов (путь относительно корня ассетов:
/// `"scenes/level1.scene"`). В отличие от `world.load_scene(path)` работает и с
/// паком (браузер, Android). Сначала — [`mount_assets`].
pub fn load_scene(app: &mut App, path: &str) -> Result<(), EngineError> {
    let database = app.world.resources.get::<AssetDatabase>().ok_or_else(|| {
        engine_assets::AssetError::NotFound {
            path: path.to_string(),
            root: "(no assets mounted)".to_string(),
            hint: " Call `engine::mount_assets` first.".to_string(),
        }
    })?;
    let guid = database.handle::<engine_assets::SceneAsset>(path)?.guid();
    let text = database.read_text(guid)?;
    app.world.load_scene_str(&text, path)?;
    Ok(())
}

/// Запустить игру в окне.
///
/// Переменные окружения для автоматических прогонов (CI, смоук-тесты):
/// * `ENGINE_FRAMES=N` — закрыться после N кадров;
/// * `ENGINE_SCREENSHOT=out.png` — не открывать окно: прогнать `ENGINE_FRAMES`
///   кадров (по умолчанию 1) с шагом 1/60 с и сохранить кадр в файл.
///
/// В браузере функция возвращается сразу, а кадры идут из `requestAnimationFrame`.
pub fn run(app: App, config: WindowConfig) -> Result<(), EngineError> {
    // Раздел Player настроек проекта поверх конфига из кода.
    let config = player_window_config(&app.world, config);
    #[cfg(target_arch = "wasm32")]
    return Ok(engine_render::run(app, config)?);
    #[cfg(not(target_arch = "wasm32"))]
    run_native(app, config)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_native(mut app: App, mut config: WindowConfig) -> Result<(), EngineError> {
    gui_startup();
    use engine_render::Renderer;
    let frames = std::env::var("ENGINE_FRAMES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());

    if let Ok(path) = std::env::var("ENGINE_SCREENSHOT") {
        app.run_headless(frames.unwrap_or(1) as u32, 1.0 / 60.0);
        let mut renderer = Renderer::headless()?;
        let image = renderer.capture(&app.world, config.width, config.height)?;
        image.save(&path).map_err(|e| EngineError::Image {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        log::info!("screenshot saved to {path}");
        return Ok(());
    }

    if frames.is_some() {
        config.max_frames = frames;
    }
    // `ENGINE_VSYNC=1` / `0` переопределяет настройку окна — для замеров.
    if let Ok(value) = std::env::var("ENGINE_VSYNC") {
        config.vsync = value != "0";
    }
    Ok(engine_render::run(app, config)?)
}

/// Подключить игровую логику на C++: загрузить плагины с горячей перезагрузкой и
/// зарегистрировать их компоненты, чтобы сцены и редактор видели их наравне с
/// компонентами на Rust. Пересоберите плагин — новая версия подхватится сама.
#[cfg(feature = "cpp")]
pub fn add_cpp_plugins<P: AsRef<Path>>(
    app: &mut App,
    paths: impl IntoIterator<Item = P>,
) -> Result<(), EngineError> {
    let mut plugins = engine_cpp_bridge::CppPlugins::default();
    for path in paths {
        plugins.load_hot(path)?;
    }
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    Ok(())
}

/// Подключить C++-скрипты самой игры, вкомпилированные в её крейт (`engine-cpp-build`):
/// компоненты регистрируются в реестре, сцены и инспектор видят их наравне с Rust-компонентами.
///
/// ```ignore
/// engine::add_static_cpp(&mut app, engine::cpp_scripts!())?;
/// ```
///
/// В браузере C++ не собирается: вернётся [`EngineError::Plugin`] с объяснением (скрипты
/// игры там заменяют на Rust или обходятся без них: `cfg(engine_game_cpp)`).
/// Горячей перезагрузки нет — для неё `add_cpp_plugins` и DLL.
#[cfg(feature = "cpp")]
pub fn add_static_cpp(app: &mut App, scripts: engine_cpp_bridge::StaticScripts) -> Result<(), EngineError> {
    let mut plugins = engine_cpp_bridge::CppPlugins::default();
    plugins.add_static_scripts(scripts)?;
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    Ok(())
}

/// Статически слинкованные C++-скрипты этой игры — аргумент [`add_static_cpp`].
/// Нужен `build.rs` с `engine_cpp_build::compile("cpp")`, он пишет файл, который
/// подключает этот макрос.
#[cfg(feature = "cpp")]
#[macro_export]
macro_rules! cpp_scripts {
    () => {
        include!(concat!(env!("OUT_DIR"), "/engine_cpp_scripts.rs"))
    };
}

/// Открыть игру в редакторе: иерархия, инспектор, вьюпорт с гизмо, Play / Pause / Step.
///
/// Компоненты игры должны быть зарегистрированы, каталог ассетов — подключён
/// ([`mount_assets`]). Те же переменные окружения, что у [`run`]:
/// `ENGINE_FRAMES=N` закрывает окно после N кадров, `ENGINE_SCREENSHOT=out.png`
/// рисует редактор в файл без окна; `ENGINE_PLAY=1` сразу включает режим Play.
///
/// Запущенный с `--editor-session <файл>` (так редактор перезапускает себя после
/// Build с новым кодом на Rust) — возвращает сцену, выделение, открытые файлы и
/// камеру из этого файла.
#[cfg(feature = "editor")]
pub fn edit(app: App, mut config: engine_editor::EditorConfig) -> Result<(), EngineError> {
    let frames = std::env::var("ENGINE_FRAMES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    if frames.is_some() {
        config.max_frames = frames;
    }
    let (width, height) = (config.width, config.height);
    let mut editor = engine_editor::Editor::new(app, config)?;
    if let Some(session) = engine_editor::session_from_args() {
        // Ошибка уже в консоли редактора; работаем дальше с тем, что загрузилось.
        let _ = editor.resume_session(&session);
    }
    if std::env::var("ENGINE_PLAY").is_ok_and(|v| v != "0") {
        editor.apply(engine_editor::Action::Play);
    }
    // Общий редактор открыл проект со своим кодом, который ещё не собран
    // (`editor_project::open_project`): сразу Build — собрать программу проекта и
    // перезапуститься в неё. Перезапущенному процессу переменная уже не нужна.
    if std::env::var("PISKA_BUILD_ON_START").is_ok_and(|v| v == "1") && engine_editor::session_from_args().is_none() {
        std::env::remove_var("PISKA_BUILD_ON_START");
        editor.apply(engine_editor::Action::Build);
    }

    if let Ok(path) = std::env::var("ENGINE_SCREENSHOT") {
        let image = engine_editor::capture(&mut editor, width, height, frames.unwrap_or(3) as u32)?;
        image.save(&path).map_err(|e| EngineError::Image {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        log::info!("editor screenshot saved to {path}");
        return Ok(());
    }

    engine_editor::run(editor)?;
    Ok(())
}

/// Включить лог: stderr + буфер в памяти, который читает консоль редактора; в
/// браузере — консоль браузера и перехват паники; на Android — logcat (тег `piska`)
/// и перехват паники. Повторные вызовы безвредны.
pub fn init_logger() {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    gui_startup();
    #[cfg(target_arch = "wasm32")]
    web::init_logger();
    #[cfg(target_os = "android")]
    android::init_logger();
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    engine_core::logging::init();
}

/// Первое, что делает программа с окном (на Windows у неё нет своей консоли, см.
/// `engine_build::windows_app`): запущена из терминала — подключиться к нему, чтобы
/// `--help`, журнал и паника печатались туда; запущена из проводника — stdout и stderr в
/// `%APPDATA%/piska/logs/<программа>.log`, паника показывает окно с этим путём.
/// Повторные вызовы безвредны; на других системах ничего не делает.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn gui_startup() -> engine_build::host::Output {
    static OUTPUT: std::sync::OnceLock<engine_build::host::Output> = std::sync::OnceLock::new();
    OUTPUT
        .get_or_init(|| {
            let program = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "piska".to_string());
            let output = engine_build::host::gui_startup(&program, engine_i18n::brand::CONFIG_DIR_NAME);
            if let engine_build::host::Output::LogFile(path) = &output {
                // Журнал ещё не настроен: путь — в него чуть позже (его видно и в Console).
                let path = path.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    log::info!("this program has no console; its log is written to {}", path.display());
                });
            }
            output
        })
        .clone()
}
