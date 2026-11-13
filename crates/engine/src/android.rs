//! Android (NativeActivity): точка входа `android_main`, пак ассетов из APK, лог в
//! logcat, сохранения во внутреннем каталоге приложения, экранная клавиатура для
//! полей ввода. APK собирает `piska-export android` (docs/api/export.md).
//!
//! Жизненный цикл окна (поверхность пересоздаётся на `resumed`, освобождается на
//! `suspended`, звук на это время замирает) реализован в `engine_render::run`.

use std::path::PathBuf;
use std::sync::OnceLock;

pub use engine_render::AndroidApp;
use engine_core::app::Subsystem;
use engine_core::world::World;
use engine_render::WindowConfig;
#[cfg(feature = "ui")]
use engine_ui::UiContext;
use log::Level;

use crate::game::BuildFn;

/// Имя пака ассетов в каталоге `assets/` APK.
pub const PACK_FILE: &str = "assets.pak";
/// Тег сообщений движка в logcat: `adb logcat -s piska`.
pub const LOG_TAG: &str = "piska";

/// Внутренний каталог данных приложения (`/data/user/0/<id>/files`).
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Каталог сохранений игры `game` на Android: внутренний каталог приложения (его
/// не видят другие приложения, он удаляется вместе с игрой).
pub fn save_path(game: &str) -> PathBuf {
    // Имя каталога очищается так же, как на десктопе.
    let name = engine_core::save::data_dir(game)
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "game".into());
    let base = DATA_DIR.get().cloned().unwrap_or_else(|| PathBuf::from("."));
    base.join(name).join("save.ron")
}

/// Лог в logcat. Фильтр — как у лога движка на десктопе: свои сообщения от Info,
/// сторонние библиотеки от Warn, `wgpu_hal` — только ошибки.
struct LogcatLogger {
    inner: OnceLock<android_logger::AndroidLogger>,
}

impl LogcatLogger {
    fn inner(&self) -> &android_logger::AndroidLogger {
        self.inner.get_or_init(|| {
            android_logger::AndroidLogger::new(
                android_logger::Config::default()
                    .with_max_level(log::LevelFilter::Info)
                    .with_tag(LOG_TAG),
            )
        })
    }
}

impl log::Log for LogcatLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        if target.starts_with("wgpu_hal") {
            return metadata.level() <= Level::Error;
        }
        if ["wgpu", "naga", "winit", "egui", "android_activity"]
            .iter()
            .any(|prefix| target.starts_with(prefix))
        {
            return metadata.level() <= Level::Warn;
        }
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.inner().log(record);
        }
    }

    fn flush(&self) {}
}

static LOGGER: LogcatLogger = LogcatLogger { inner: OnceLock::new() };

/// Лог в logcat и паника туда же (stderr на Android никуда не попадает).
pub(crate) fn init_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}\n{}", std::backtrace::Backtrace::force_capture());
            previous(info);
        }));
    }
}

fn load_pack(app: &AndroidApp) -> Result<Option<engine_assets::AssetPack>, String> {
    let name = std::ffi::CString::new(PACK_FILE).expect("no NUL in the file name");
    let Some(mut asset) = app.asset_manager().open(&name) else {
        return Ok(None);
    };
    let bytes = asset
        .buffer()
        .map_err(|e| format!("cannot read `{PACK_FILE}` from the APK: {e}"))?
        .to_vec();
    engine_assets::AssetPack::from_bytes(bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Экранная клавиатура: открыта, пока в интерфейсе редактируется поле ввода
/// (`UiEvents::wants_keyboard`). Набранный текст приходит обычными событиями клавиш.
#[cfg(feature = "ui")]
struct SoftKeyboard {
    app: AndroidApp,
    shown: bool,
}

#[cfg(feature = "ui")]
impl Subsystem for SoftKeyboard {
    fn name(&self) -> &'static str {
        "SoftKeyboard"
    }

    fn post_update(&mut self, world: &mut World) {
        let wanted = world.ui().wants_keyboard();
        if wanted != self.shown {
            self.shown = wanted;
            if wanted {
                self.app.show_soft_input(true);
            } else {
                self.app.hide_soft_input(true);
            }
        }
    }
}

/// Запустить игру: пак из APK → `build()` → цикл кадров на экране. Когда игра
/// закончилась (`ctx.quit()` или ошибка), процесс завершается: окно winit в одном
/// процессе создаётся только раз, а Android мог бы запустить `android_main` снова.
pub fn start(app: AndroidApp, build: BuildFn, config: WindowConfig) {
    crate::init_logger();
    if let Some(dir) = app.internal_data_path() {
        let _ = DATA_DIR.set(dir);
    }
    match load_pack(&app) {
        Ok(Some(pack)) => engine_assets::install_pack(pack),
        Ok(None) => log::info!("no `{PACK_FILE}` in the APK: the game has no assets"),
        Err(err) => {
            log::error!("{err}");
            std::process::exit(1);
        }
    }
    engine_render::set_android_app(app.clone());
    let result = build().and_then(|mut game| {
        crate::load_default_scene(&mut game)?;
        #[cfg(feature = "ui")]
        game.add_subsystem(SoftKeyboard { app, shown: false });
        crate::run(game, config)
    });
    match result {
        Ok(()) => std::process::exit(0),
        Err(err) => {
            log::error!("{err:?}");
            std::process::exit(1);
        }
    }
}
