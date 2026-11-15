//! Запуск хаба: окно с заставкой появляется сразу, а сам хаб (настройки, список проектов,
//! поиск папки движка) собирается в фоновом потоке. Кадр рисует [`frame`]: заставка с
//! настоящим шагом, потом хаб, при сбое — экран ошибки с текстом, а не зависшее окно.
//!
//! И окно (`main.rs`), и тесты ведут запуск одними и теми же [`start`] / [`frame`].

use engine_editor::kit::boot::{Boot, Progress, Task};
use engine_editor::kit::i18n::{brand, tr_s};

use crate::{Hub, HubOptions};

/// Запуск хаба с параметрами командной строки; хаб собирается в фоне.
pub fn start(options: HubOptions) -> Boot<Hub> {
    start_with(move |progress| {
        progress.set_step(tr_s("Reading settings and the project list…"));
        Ok(Hub::new(options))
    })
}

/// То же с заданной работой (тесты подставляют медленную или неудачную).
pub fn start_with(load: impl FnOnce(&Progress) -> Result<Hub, String> + Send + 'static) -> Boot<Hub> {
    let task = Task::spawn("hub-start", tr_s("Starting…"), load);
    Boot::start(brand::HUB_NAME, &format!("{} {}", brand::ENGINE_NAME, env!("CARGO_PKG_VERSION")), engine_editor::kit::window::paint_engine_mark, task)
}

/// Один кадр окна: заставка, пока хаб собирается или обходит проекты; потом сам хаб.
pub fn frame(boot: &mut Boot<Hub>, ctx: &egui::Context) {
    boot.frame(ctx, |ctx, hub| {
        hub.ui(ctx);
        hub.startup_step()
    });
}
