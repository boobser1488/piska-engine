//! Графический установщик движка — мастер поверх скриптов `install/install.ps1` и
//! `install/install.sh` (они вшиты в программу и запускаются в машиночитаемом режиме
//! `-Json` / `--json`; установщик сам ничего не ставит). Страницы: Welcome, Components,
//! Options, Licences, Review (точный план — сухой прогон скриптов), Installing, Finish
//! (таблица `doctor`); при найденном манифесте — Modify / Repair / Uninstall.
//!
//! ```text
//! cargo run -p piska-installer                      # окно
//! cargo run -p piska-installer -- --dry-run         # всё то же, но ничего не меняется
//! piska-installer --unattended --components core    # без окна: события JSON в stdout (CI)
//! ```
//!
//! Как и хаб, это библиотека плюс маленькая программа: интерфейс строится методом
//! [`Installer::ui`] без GPU, всё, что делает мышь, — [`Action`], скрипты запускаются через
//! [`runner::Backend`] (настоящий или поддельный), поэтому мастер проверяется тестами без окна
//! и без настоящей установки. Подробно — `docs/install.md`.

pub mod app;
pub mod capture;
pub mod cli;
pub mod diag;
pub mod instance;
pub mod manifest;
pub mod packages;
pub mod protocol;
pub mod runner;
pub mod scripts;
pub mod settings;
pub mod startup;
mod ui;

/// Диагностика `piska-export doctor` — тот же файл, вшитый сюда (один источник проверок).
#[path = "../../piska-export/src/doctor.rs"]
#[allow(clippy::all)]
pub mod doctor;

pub use app::{Action, Installer, InstallerOptions, Mode, Page};
pub use ui::{component_name, step_title};

/// Ошибка `doctor.rs` (в piska-export это ошибка всей программы): текст для человека.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<E: std::error::Error> From<E> for Error {
    fn from(error: E) -> Self {
        Error(error.to_string())
    }
}

/// Результат `doctor.rs`.
pub type Result<T> = std::result::Result<T, Error>;
