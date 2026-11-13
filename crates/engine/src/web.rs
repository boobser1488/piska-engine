//! Браузер (`wasm32-unknown-unknown`): лог в консоль, паника на страницу, пак ассетов
//! по сети, сохранения в `localStorage`, запуск игры из `index.html`.
//!
//! Страницу и JS-обвязку делает `piska-export web`; игровой код ничего из этого
//! модуля не вызывает — всё подключает [`game_main!`](crate::game_main).

use std::sync::Once;

use engine_core::save::{SaveBackend, SaveError};
use engine_render::WindowConfig;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Версия wasm-bindgen, на которой собран движок (для `#[wasm_bindgen]` из макроса).
pub use wasm_bindgen;

use crate::game::BuildFn;

/// Имя файла пака ассетов рядом с `index.html`.
pub const PACK_FILE: &str = "assets.pak";

fn set_body_data(name: &str, value: &str) {
    if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
        let _ = body.set_attribute(name, value);
    }
}

/// Показать ошибку на странице (её текст выводит `index.html`) и в консоли.
pub fn report_error(message: &str) {
    web_sys::console::error_1(&message.into());
    set_body_data("data-piska-error", message);
    set_body_data("data-piska-state", "error");
}

/// Паника → читаемое сообщение в консоли и на странице, а не «unreachable executed».
pub fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            report_error(&format!("the game crashed: {info}"));
        }));
    });
}

/// Лог в консоль браузера с теми же фильтрами, что у десктопного лога.
struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        if target.starts_with("wgpu_hal") {
            return metadata.level() <= log::Level::Error;
        }
        let third_party = ["wgpu", "naga", "winit"].iter().any(|p| target.starts_with(p));
        metadata.level() <= if third_party { log::Level::Warn } else { log::Level::Info }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let text = format!("[{}] {}", record.target(), record.args()).into();
        match record.level() {
            log::Level::Error => web_sys::console::error_1(&text),
            log::Level::Warn => web_sys::console::warn_1(&text),
            log::Level::Info => web_sys::console::info_1(&text),
            _ => web_sys::console::debug_1(&text),
        }
    }

    fn flush(&self) {}
}

static LOGGER: ConsoleLogger = ConsoleLogger;

pub(crate) fn init_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    install_panic_hook();
}

/// Скачать пак ассетов. `Ok(None)` — пака нет (404): у игры нет ассетов.
async fn fetch_pack(url: &str) -> Result<Option<Vec<u8>>, String> {
    let window = web_sys::window().ok_or("no `window` object")?;
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let response = JsFuture::from(window.fetch_with_str(url)).await.map_err(js)?;
    let response: web_sys::Response = response.dyn_into().map_err(js)?;
    if response.status() == 404 {
        return Ok(None);
    }
    if !response.ok() {
        return Err(format!("HTTP {} {}", response.status(), response.status_text()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(js)?).await.map_err(js)?;
    Ok(Some(js_sys::Uint8Array::new(&buffer).to_vec()))
}

/// Запустить игру в браузере: пак ассетов → `build()` → цикл кадров на холсте.
/// Возвращается сразу; ошибки показываются на странице.
pub fn start(build: BuildFn, config: WindowConfig) {
    init_logger();
    set_body_data("data-piska-state", "loading");
    wasm_bindgen_futures::spawn_local(async move {
        match fetch_pack(PACK_FILE).await {
            Ok(Some(bytes)) => match engine_assets::AssetPack::from_bytes(bytes) {
                Ok(pack) => {
                    log::info!("asset pack: {} files, {} KiB", pack.len(), pack.byte_len() / 1024);
                    engine_assets::install_pack(pack);
                }
                Err(err) => return report_error(&err.to_string()),
            },
            Ok(None) => log::info!("no `{PACK_FILE}` next to the page: the game has no assets"),
            Err(err) => return report_error(&format!("cannot download `{PACK_FILE}`: {err}")),
        }
        set_body_data("data-piska-state", "starting");
        let result = build().and_then(|mut app| {
            crate::load_default_scene(&mut app)?;
            crate::run(app, config)
        });
        if let Err(err) = result {
            report_error(&format!("{err:?}"));
        }
    });
}

/// Сохранения в `localStorage` под ключом `piska:<игра>`.
pub struct LocalStorageBackend {
    key: String,
}

impl LocalStorageBackend {
    pub fn new(game: &str) -> Self {
        Self {
            key: format!("piska:{game}"),
        }
    }

    fn storage() -> Result<web_sys::Storage, SaveError> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| SaveError::Io {
                path: "localStorage".to_string(),
                source: std::io::Error::other(
                    "localStorage is unavailable (private mode or blocked cookies)",
                ),
            })
    }
}

impl SaveBackend for LocalStorageBackend {
    fn read(&mut self) -> Result<Option<String>, SaveError> {
        Ok(Self::storage()?.get_item(&self.key).ok().flatten())
    }

    fn write(&mut self, text: &str) -> Result<(), SaveError> {
        Self::storage()?.set_item(&self.key, text).map_err(|e| SaveError::Io {
            path: format!("localStorage[{}]", self.key),
            source: std::io::Error::other(format!("{e:?} (storage quota exceeded?)")),
        })
    }

    fn quarantine(&mut self, text: &str) {
        if let Ok(storage) = Self::storage() {
            let _ = storage.set_item(&format!("{}.corrupt", self.key), text);
        }
    }
}
