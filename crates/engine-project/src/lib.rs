//! `engine-project` — файл с исходниками проекта.
//!
//! Один переносимый файл (`.pks` — расширение берётся из `engine_i18n::brand`), в котором
//! лежит всё, что нужно, чтобы собрать проект заново: `Cargo.toml`, `build.rs`, `src/`,
//! `cpp/`, `assets/` с `.meta`, `tests/`, `tools/`, `examples/`, README и лицензия — и ничего
//! производного (`target/`, `dist/`, `.git/`, кэши, временные файлы редактора, мусор ОС).
//!
//! ```no_run
//! use engine_project::{pack, unpack, read_info, Control, PackOptions, UnpackOptions};
//! # fn main() -> engine_project::Result<()> {
//! let control = Control::default();
//! let summary = pack("games/lantern".as_ref(), "lantern.pks".as_ref(), &PackOptions::default(), &control, &mut |_| {})?;
//! println!("{} files, {} bytes", summary.files, summary.bytes);
//! let info = read_info("lantern.pks".as_ref())?;
//! assert_eq!(info.manifest.project.name, "lantern");
//! unpack("lantern.pks".as_ref(), "restored".as_ref(), &UnpackOptions::default(), &control, &mut |_| {})?;
//! # Ok(()) }
//! ```
//!
//! Формат описан в `docs/api/project-file.md`. Главное: запись и чтение потоковые (проект на
//! сотни мегабайт не загружается в память), вывод детерминирован (одинаковый проект и
//! параметры — одинаковые байты), чтение проверяет контрольные суммы и называет повреждённую
//! запись, пути с `..` и абсолютные отвергаются, число файлов и объём ограничены.
#![forbid(unsafe_code)]

mod collect;
mod error;
mod format;
mod ignore;
mod merge;
mod pack;
mod path;
mod read;

pub use collect::{collect, Candidate, Collected, CollectOptions, SkipReason, Skipped};
pub use error::{Error, Result};
pub use format::{FileRecord, Manifest, ProjectMeta, Totals, Writer, FORMAT_VERSION, MAGIC};
pub use ignore::Rules as GitignoreRules;
pub use merge::{merge_tree, resolve_target, Conflict, MergeSummary, Target};
pub use pack::{pack, PackOptions, PackSummary};
pub use path::validate_relative;
pub use read::{read_info, read_manifest_entries, unpack, verify, Include, Info, UnpackOptions, UnpackSummary};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Отмена длинной работы из другого потока: `clone()` отдаётся тому, кто нажимает «Cancel».
#[derive(Clone, Debug, Default)]
pub struct Control {
    flag: Arc<AtomicBool>,
}

impl Control {
    pub fn new() -> Self {
        Self::default()
    }

    /// Попросить остановиться: работа прервётся на ближайшем блоке и вернёт [`Error::Cancelled`].
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Этап работы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Обход каталога проекта, составление списка файлов.
    Scanning,
    /// Запись (или чтение) файлов.
    Working,
    /// Проверка целого файла проекта.
    Verifying,
}

/// Ход работы: для полосы прогресса и счётчиков.
#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Путь файла, с которым работаем сейчас (относительно проекта).
    pub current: String,
}

/// Пределы против «бомб» и мусора: превышение — ошибка с понятным текстом, а не нехватка
/// памяти или диска.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Больше файлов — ошибка.
    pub max_files: u64,
    /// Больше байт (распакованных, всего) — ошибка.
    pub max_total: u64,
    /// Длина пути в байтах.
    pub max_path: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 250_000,
            max_total: 64 * 1024 * 1024 * 1024,
            max_path: 1024,
        }
    }
}
