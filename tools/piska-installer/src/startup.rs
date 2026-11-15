//! Запуск установщика: окно с заставкой появляется сразу, а сам установщик (настройки,
//! поиск уже установленного, проверка инструментов) собирается в фоновом потоке. Кадр рисует
//! [`frame`]: заставка с настоящим шагом, потом мастер, при сбое — экран ошибки с текстом.
//!
//! И окно (`main.rs`), и тесты ведут запуск одними и теми же [`start`] / [`frame`].

use engine_editor::kit::boot::{Boot, Progress, Task};
use engine_editor::kit::i18n::{brand, tr_s, trf};

use crate::{Installer, InstallerOptions};

/// Запуск установщика с параметрами командной строки; он собирается в фоне.
pub fn start(options: InstallerOptions) -> Boot<Installer> {
    start_with(move |progress| {
        progress.set_step(tr_s("Reading the settings and looking for installed tools…"));
        Ok(Installer::new(options))
    })
}

/// То же с заданной работой (тесты подставляют медленную или неудачную).
pub fn start_with(load: impl FnOnce(&Progress) -> Result<Installer, String> + Send + 'static) -> Boot<Installer> {
    let task = Task::spawn("installer-start", tr_s("Starting…"), load);
    Boot::start(&trf!("{engine} Setup", engine = brand::ENGINE_NAME), &trf!("Version {version}", version = env!("CARGO_PKG_VERSION")), engine_editor::kit::window::paint_engine_mark, task)
}

/// Один кадр окна: заставка, пока установщик собирается; потом сам мастер (проверка, что уже
/// установлено, идёт в нём со своими строками хода).
pub fn frame(boot: &mut Boot<Installer>, ctx: &egui::Context) {
    boot.frame(ctx, |ctx, installer| {
        installer.ui(ctx);
        if let Some(text) = installer.clipboard.take() {
            ctx.copy_text(text);
        }
        None
    });
}
