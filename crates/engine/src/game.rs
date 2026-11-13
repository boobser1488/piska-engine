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
