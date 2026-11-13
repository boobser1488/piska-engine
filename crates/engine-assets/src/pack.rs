//! Пак ассетов: весь каталог `assets/` (вместе с `.meta`) одним файлом.
//!
//! Нужен там, где нет файловой системы: в браузере пак скачивается при старте,
//! в APK лежит среди ресурсов. Формат намеренно простой — его пишет
//! `piska-export`, читает [`AssetPack::from_bytes`] без копирования содержимого:
//!
//! ```text
//! "PISKAPK1"                      магия, 8 байт
//! u32 LE                          число записей
//! для каждой записи:
//!   u16 LE  длина пути, путь UTF-8 (относительно корня, через `/`)
//!   u32 LE  длина данных, данные
//! ```
//!
//! ```
//! use engine_assets::AssetPack;
//!
//! let mut builder = AssetPack::builder();
//! builder.add("sprites/hero.png", b"png bytes".to_vec());
//! let pack = AssetPack::from_bytes(builder.to_bytes()).unwrap();
//! assert_eq!(pack.get("sprites/hero.png"), Some(&b"png bytes"[..]));
//! ```

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use crate::AssetError;

const MAGIC: &[u8; 8] = b"PISKAPK1";

/// Файлы каталога ассетов в памяти: путь → байты.
#[derive(Clone)]
pub struct AssetPack {
    data: Cow<'static, [u8]>,
    entries: BTreeMap<String, Range<usize>>,
}

impl std::fmt::Debug for AssetPack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetPack")
            .field("files", &self.entries.len())
            .field("bytes", &self.data.len())
            .finish()
    }
}

impl AssetPack {
    /// Сборщик пака (им пользуется `piska-export`).
    pub fn builder() -> AssetPackBuilder {
        AssetPackBuilder::default()
    }

    /// Прочитать пак из байтов. `&'static [u8]` (из `include_bytes!`) не копируется.
    pub fn from_bytes(bytes: impl Into<Cow<'static, [u8]>>) -> Result<Self, AssetError> {
        let data = bytes.into();
        let entries = parse_index(&data).map_err(|reason| AssetError::BadPack { reason })?;
        Ok(Self { data, entries })
    }

    /// Содержимое файла по пути относительно корня ассетов (`\` тоже понимается).
    pub fn get(&self, relative_path: &str) -> Option<&[u8]> {
        let key = relative_path.replace('\\', "/");
        self.entries.get(&key).map(|range| &self.data[range.clone()])
    }

    pub fn contains(&self, relative_path: &str) -> bool {
        self.entries.contains_key(&relative_path.replace('\\', "/"))
    }

    /// Все пути пака в алфавитном порядке (вместе с `.meta`).
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Размер пака в байтах.
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }
}

fn parse_index(data: &[u8]) -> Result<BTreeMap<String, Range<usize>>, String> {
    if data.len() < 12 || &data[..8] != MAGIC {
        return Err("the file does not start with `PISKAPK1`; rebuild it with `piska-export`".to_string());
    }
    let count = u32::from_le_bytes(data[8..12].try_into().expect("4 bytes")) as usize;
    let mut at: usize = 12;
    let mut take = |len: usize, what: &str| -> Result<Range<usize>, String> {
        let end = at
            .checked_add(len)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| format!("truncated {what} at byte {at}; the pack is damaged, rebuild it"))?;
        let range = at..end;
        at = end;
        Ok(range)
    };
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        let len = take(2, "path length")?;
        let path_len = u16::from_le_bytes(data[len].try_into().expect("2 bytes")) as usize;
        let path = take(path_len, "path")?;
        let path = std::str::from_utf8(&data[path])
            .map_err(|_| "a path in the pack is not UTF-8".to_string())?
            .to_string();
        let len = take(4, "data length")?;
        let data_len = u32::from_le_bytes(data[len].try_into().expect("4 bytes")) as usize;
        let range = take(data_len, "file data")?;
        entries.insert(path, range);
    }
    Ok(entries)
}

/// Сборщик пака: добавить файлы, получить байты.
#[derive(Default)]
pub struct AssetPackBuilder {
    files: BTreeMap<String, Vec<u8>>,
}

impl AssetPackBuilder {
    /// Добавить файл (путь относительно корня ассетов).
    pub fn add(&mut self, relative_path: &str, bytes: Vec<u8>) -> &mut Self {
        self.files.insert(relative_path.replace('\\', "/"), bytes);
        self
    }

    /// Добавить весь каталог рекурсивно: все файлы, включая `.meta`. Скрытые файлы и
    /// каталоги (`.git`, `.DS_Store`) пропускаются.
    pub fn add_dir(&mut self, root: impl AsRef<Path>) -> Result<&mut Self, AssetError> {
        fn walk(builder: &mut AssetPackBuilder, root: &Path, dir: &Path) -> Result<(), AssetError> {
            let io = |path: &Path, source| AssetError::Io {
                path: path.display().to_string(),
                source,
            };
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .map_err(|e| io(dir, e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .collect();
            entries.sort();
            for path in entries {
                let hidden = path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'));
                if hidden {
                    continue;
                }
                if path.is_dir() {
                    walk(builder, root, &path)?;
                } else {
                    let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
                    let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                    builder.add(&relative, bytes);
                }
            }
            Ok(())
        }
        let root = root.as_ref();
        walk(self, root, root)?;
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Байты пака. Файлы больше 4 ГиБ и пути длиннее 65535 байт не поддерживаются.
    pub fn to_bytes(&self) -> Vec<u8> {
        let total: usize = self.files.iter().map(|(p, d)| 6 + p.len() + d.len()).sum();
        let mut out = Vec::with_capacity(12 + total);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        for (path, data) in &self.files {
            out.extend_from_slice(&(path.len() as u16).to_le_bytes());
            out.extend_from_slice(path.as_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
        }
        out
    }
}
