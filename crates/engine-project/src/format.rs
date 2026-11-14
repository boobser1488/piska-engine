//! Раскладка файла проекта (формат версии 1) — подробно в `docs/api/project-file.md`.
//!
//! ```text
//! заголовок   8 байт  сигнатура 89 50 52 4A 53 0D 0A 1A   (как у PNG: ловит текстовый режим)
//!             2       версия формата (u16, LE)
//!             2       флаги (0)
//!             4       зарезервировано (0)
//! запись*     1       0x01
//!             1       способ: 0 — без сжатия, 1 — deflate
//!             2       длина имени (u16)   + имя в UTF-8, `/`
//!             …       блоки: [u32 длина > 0][данные]… и [u32 0] в конце
//!             8       размер без сжатия (u64)
//!             4       CRC-32 содержимого
//! конец       1       0x00
//!             4       длина списка + список (JSON) + 4 байта CRC-32 списка
//! трейлер    24       смещение байта 0x00 (u64), длина списка (u32), CRC-32 списка,
//!                     сигнатура конца 89 50 52 4A 45 0D 0A 1A
//! ```
//!
//! Список (манифест) нужен, чтобы показать содержимое, не читая данные: имя проекта, версия
//! движка, время создания, список файлов с размерами и контрольными суммами, какие части
//! есть. Данные каждого файла сжаты отдельно — повреждение одного не прячет остальные, и
//! ошибка называет запись.

use serde::{Deserialize, Serialize};

/// Сигнатура начала. Без названия продукта: файлы должны читаться всегда, что бы ни
/// случилось с названием.
pub const MAGIC: [u8; 8] = [0x89, b'P', b'R', b'J', b'S', 0x0D, 0x0A, 0x1A];
pub(crate) const END_MAGIC: [u8; 8] = [0x89, b'P', b'R', b'J', b'E', 0x0D, 0x0A, 0x1A];
/// Версия формата, которую пишет и читает эта сборка.
pub const FORMAT_VERSION: u16 = 1;
pub(crate) const HEADER_LEN: u64 = 16;
pub(crate) const TRAILER_LEN: u64 = 24;
pub(crate) const MARK_FILE: u8 = 1;
pub(crate) const MARK_END: u8 = 0;
pub(crate) const METHOD_STORED: u8 = 0;
pub(crate) const METHOD_DEFLATE: u8 = 1;
/// Размер блока данных в файле.
pub(crate) const FRAME: usize = 32 * 1024;
/// Блок длиннее этого — файл повреждён.
pub(crate) const MAX_FRAME: u32 = 16 * 1024 * 1024;
/// Список файлов больше этого не читается.
pub(crate) const MAX_MANIFEST: u32 = 512 * 1024 * 1024;

/// Список файлов проекта и сведения о нём.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    /// Версия формата (дублирует заголовок).
    pub format: u16,
    pub project: ProjectMeta,
    pub writer: Writer,
    /// Время создания, секунды Unix; 0 — не записано (воспроизводимый файл).
    pub created: u64,
    /// Какие необязательные части есть: `assets`, `tests`, `cpp`, `tools`, `examples`,
    /// `builds`, `readme`, `license`, `icon`, `settings`.
    pub parts: Vec<String>,
    pub files: Vec<FileRecord>,
    pub totals: Totals,
}

/// О проекте.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectMeta {
    /// Имя пакета Cargo (или каталога).
    pub name: String,
    /// Заголовок для списка проектов.
    pub title: String,
}

/// Кто записал.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Writer {
    /// Программа (`piska-hub`, `piska-export`, `piska-editor`).
    pub tool: String,
    /// Версия движка.
    pub engine_version: String,
}

/// Запись о файле.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileRecord {
    pub path: String,
    pub size: u64,
    /// CRC-32 (IEEE) содержимого.
    pub crc32: u32,
    /// Размер данных в файле проекта.
    pub packed: u64,
    /// 0 — без сжатия, 1 — deflate.
    pub method: u8,
}

/// Итоги.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub files: u64,
    pub bytes: u64,
    pub packed: u64,
}

/// Расширения, которые уже сжаты: deflate их не уменьшит.
pub(crate) fn already_compressed(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "ogg" | "mp3" | "flac" | "opus" | "mp4" | "webm" | "zip" | "gz" | "7z" | "xz" | "zst"
            | "woff2" | "pak"
    )
}
