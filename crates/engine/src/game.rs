//! Игра как крейт: одна функция `build() -> Result<App, EngineError>` и макрос
//! [`game_main!`](crate::game_main), который делает из неё программу для каждой
//! платформы. Подробно — `docs/api/export.md`.

use std::path::PathBuf;

use engine_core::app::App;
use engine_core::save::SaveData;
use engine_render::WindowConfig;

use crate::EngineError;

/// Функция, которая собирает приложение игры.
pub type BuildFn = fn() -> Result<App, EngineError>;

/// Каталог ассетов игры на десктопе: `assets/` рядом с исполняемым файлом (так
/// раскладывает экспорт `piska-export windows`), иначе `<manifest_dir>/assets` —
/// каталог крейта при `cargo run`. В браузере и на Android путь не используется:
/// ассеты приходят паком (см. [`crate::mount_assets`]). Обычно вызывается через
/// [`assets_dir!`](crate::assets_dir).
pub fn game_assets(manifest_dir: &str) -> PathBuf {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("assets")))
        .filter(|dir| dir.is_dir());
    beside_exe.unwrap_or_else(|| PathBuf::from(manifest_dir).join("assets"))
}

/// Каталог ассетов вызывающего крейта: `engine::mount_assets(&mut app, engine::assets_dir!())`.
#[macro_export]
macro_rules! assets_dir {
    () => {
        $crate::game_assets(env!("CARGO_MANIFEST_DIR"))
    };
}

/// Подставить окну игры её собственный значок: `icon.png` в каталоге ассетов
/// (см. [`game_assets`]). Значок, заданный в конфиге явно, не трогается; нет файла —
/// остаётся значок движка. Вызывается из [`game_main!`](crate::game_main).
pub fn with_game_icon(mut config: WindowConfig, manifest_dir: &str) -> WindowConfig {
    if config.icon.is_none() {
        if let Ok(png) = std::fs::read(game_assets(manifest_dir).join("icon.png")) {
            config.icon = Some(png);
        }
    }
    config
}

/// Сохранения игры там, где они переживают перезапуск на этой платформе:
/// файл в каталоге данных ОС на десктопе, `localStorage` в браузере, внутренний
/// каталог приложения на Android.
pub fn save_data(game: &str) -> SaveData {
    #[cfg(target_arch = "wasm32")]
    {
        SaveData::with_backend(crate::web::LocalStorageBackend::new(game))
    }
    #[cfg(target_os = "android")]
    {
        SaveData::with_backend(engine_core::save::FileBackend::new(crate::android::save_path(game)))
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        SaveData::open(game)
    }
}

/// Собрать игру и запустить её в окне (десктоп). Ошибку сборки печатает лог.
pub fn run_game(build: BuildFn, config: WindowConfig) -> Result<(), EngineError> {
    crate::init_logger();
    let mut app = build()?;
    // Пустой мир + `PlayerSettings::default_scene` → загрузить её.
    crate::load_default_scene(&mut app)?;
    crate::run(app, config)
}

/// Собрать игру из уже созданного приложения с зарегистрированными компонентами: положить
/// хранилище сохранений, подключить ассеты (каталог `assets` — на десктопе, пак — в браузере
/// и на Android, см. [`crate::mount_assets`]) и загрузить стартовую сцену. Это тело
/// функции `build_with`, которую пишет [`game!`](crate::game).
pub fn build_game(mut app: App, save: SaveData, assets: impl AsRef<std::path::Path>, scene: &str) -> Result<App, EngineError> {
    app.world.resources.insert(save);
    crate::mount_assets(&mut app, assets)?;
    crate::load_scene(&mut app, scene)?;
    Ok(app)
}

/// Вызвать шаг `setup:` из [`game!`](crate::game) (отдельная функция — чтобы тип замыкания
/// выводился из её сигнатуры).
#[doc(hidden)]
pub fn run_game_setup(app: &mut App, setup: impl FnOnce(&mut App) -> Result<(), EngineError>) -> Result<(), EngineError> {
    setup(app)
}

/// Окно игры из [`game!`](crate::game): заданное в `window:` или «имя игры, 1280 × 720».
#[doc(hidden)]
#[macro_export]
macro_rules! __game_window {
    ($name:expr) => {
        $crate::render::WindowConfig::titled($name).with_size(1280, 720)
    };
    ($name:expr, $window:expr) => {
        $window
    };
}

/// Игра одним объявлением: имя, стартовая сцена, компоненты на Rust. Пишет за вас то,
/// что раньше писали руками (и что по-прежнему можно написать руками — см. `docs/api/export.md`):
///
/// ```ignore
/// pub mod scripts; // скрипты, созданные в редакторе (New Rust Script)
/// use engine::prelude::*;
///
/// engine::game! {
///     name: "Piska Starfall",
///     scene: "scenes/main.scene",
///     components: [GameManager],
/// }
/// ```
///
/// Получается (в корне крейта игры):
/// * `pub const GAME_NAME` — имя (каталог сохранений, ключ `localStorage`, заголовок окна);
/// * `pub fn register(app)` — `scripts::register(app)` (каталог `src/scripts/`, им управляет
///   редактор) и `app.register::<T>()` для каждого из `components`;
/// * `pub fn build_with(save)` — [`crate::app`], `register`, необязательный `setup`, затем
///   [`build_game`]: сохранения, ассеты, сцена. Тесты передают `SaveData::in_memory()`;
/// * `pub fn build()` — `build_with(engine::save_data(GAME_NAME))`; её открывает
///   `engine::editor_main!` (программа редактора) и экспорт;
/// * точки входа [`game_main!`](crate::game_main): `main` для десктопа, браузер, Android.
///
/// Необязательные поля после `components` (в этом порядке):
/// * `window: WindowConfig::titled("…").with_size(800, 600)` — окно (по умолчанию «имя, 1280 × 720»);
/// * `setup: |app| engine::add_static_cpp(app, engine::cpp_scripts!())` — что ещё сделать с
///   приложением до загрузки ассетов (`FnOnce(&mut App) -> Result<(), EngineError>`).
#[macro_export]
macro_rules! game {
    (
        name: $name:expr,
        scene: $scene:expr,
        components: [$($component:ty),* $(,)?]
        $(, window: $window:expr)?
        $(, setup: $setup:expr)?
        $(,)?
    ) => {
        /// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере, заголовок окна.
        pub const GAME_NAME: &str = $name;

        /// Зарегистрировать компоненты игры (сцены и префабы ссылаются на них по имени):
        /// скрипты из `src/scripts/` и перечисленные в `engine::game!`.
        pub fn register(app: &mut $crate::core::app::App) {
            scripts::register(app);
            $(app.register::<$component>();)*
        }

        /// Собрать игру с заданным хранилищем сохранений (тесты передают `SaveData::in_memory()`).
        pub fn build_with(
            save: $crate::core::save::SaveData,
        ) -> ::core::result::Result<$crate::core::app::App, $crate::EngineError> {
            #[allow(unused_mut)]
            let mut app = $crate::app();
            register(&mut app);
            $($crate::run_game_setup(&mut app, $setup)?;)?
            $crate::build_game(app, save, $crate::assets_dir!(), $scene)
        }

        /// Собрать игру: сохранения — там, где они живут на этой платформе.
        pub fn build() -> ::core::result::Result<$crate::core::app::App, $crate::EngineError> {
            build_with($crate::save_data(GAME_NAME))
        }

        $crate::game_main!(build, $crate::__game_window!(GAME_NAME $(, $window)?));
    };
}

/// Точки входа игры для всех платформ из одной функции `build`.
///
/// ```ignore
/// // games/<name>/src/lib.rs
/// pub fn build() -> Result<App, EngineError> { /* … */ }
/// engine::game_main!(build, WindowConfig::titled("My Game"));
///
/// // games/<name>/src/main.rs
/// fn main() -> Result<(), engine::EngineError> { my_game::main() }
/// ```
///
/// Что получается:
/// * десктоп (Windows, Linux, macOS) — `pub fn main()`, его вызывает `src/main.rs`;
/// * браузер — экспорт `piska_start()` для `index.html`: скачать пак ассетов,
///   собрать игру, привязать к `<canvas id="piska-canvas">`;
/// * Android — `android_main` для NativeActivity: пак ассетов из APK, окно — экран.
#[macro_export]
macro_rules! game_main {
    ($build:path, $config:expr $(,)?) => {
        /// Точка входа на десктопе: вызовите из `src/main.rs`.
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        pub fn main() -> ::core::result::Result<(), $crate::EngineError> {
            $crate::run_game($build, $crate::with_game_icon($config, env!("CARGO_MANIFEST_DIR")))
        }

        /// В браузере и на Android программа запускается не через `main`
        /// (см. `piska_start` / `android_main`); пустой `main` нужен лишь затем, чтобы
        /// `src/main.rs` компилировался для любой цели (`cargo check --target …`).
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        pub fn main() -> ::core::result::Result<(), $crate::EngineError> {
            ::core::result::Result::Ok(())
        }

        /// Точка входа в браузере: её вызывает `index.html` после щелчка «Play».
        #[cfg(target_arch = "wasm32")]
        #[$crate::web::wasm_bindgen::prelude::wasm_bindgen(wasm_bindgen = $crate::web::wasm_bindgen)]
        pub fn piska_start() {
            $crate::web::start($build, $config);
        }

        /// Точка входа на Android (NativeActivity).
        #[cfg(target_os = "android")]
        #[no_mangle]
        pub fn android_main(app: $crate::android::AndroidApp) {
            $crate::android::start(app, $build, $config);
        }
    };
}
