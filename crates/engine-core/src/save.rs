//! Сохранения игрока (Unity: `PlayerPrefs`): ключ → значение, RON-файл в каталоге данных игры.
//!
//! ```
//! use engine_core::prelude::*;
//! use engine_core::save::{MemoryBackend, SaveData};
//!
//! // В настоящей игре: `app.world.resources.insert(SaveData::open("MyGame"));`
//! let mut save = SaveData::with_backend(MemoryBackend::default());
//! assert_eq!(save.int("best_score", 0), 0);       // нет ключа — значение по умолчанию
//! save.set_int("best_score", 1200);
//! save.set_vec2("last_checkpoint", Vec2::new(3.0, 4.0));
//! save.save().unwrap();                            // на диск — только явно
//! ```
//!
//! Хранилище спрятано за [`SaveBackend`]: на десктопе это файл ([`FileBackend`]), для веба
//! можно подставить `localStorage`, не трогая игровой код. Повреждённый файл не роняет игру:
//! в лог уходит предупреждение, значения — по умолчанию, а испорченный файл сохраняется
//! рядом как `*.corrupt`, чтобы его можно было разобрать.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::math::{Vec2, Vec3};
use crate::reflect::{FieldValue, Value};

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("cannot access save file `{path}`: {source}. Check that the directory is writable")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot encode the save data: {0}")]
    Encode(String),
}

/// Где физически лежит текст сохранения.
pub trait SaveBackend: 'static {
    /// Прочитать сохранение; `None`, если его ещё нет.
    fn read(&mut self) -> Result<Option<String>, SaveError>;
    fn write(&mut self, text: &str) -> Result<(), SaveError>;
    /// Сохранить копию нечитаемого текста для разбора (по умолчанию ничего не делает).
    fn quarantine(&mut self, text: &str) {
        let _ = text;
    }
}

/// Файл на диске. Запись атомарна: сначала временный файл, затем переименование.
pub struct FileBackend {
    path: PathBuf,
}

impl FileBackend {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<каталог данных ОС>/<игра>/save.ron`: `%APPDATA%` на Windows,
    /// `~/Library/Application Support` на macOS, `$XDG_DATA_HOME` или `~/.local/share` на Linux.
    pub fn for_game(game: &str) -> Self {
        Self::new(data_dir(game).join("save.ron"))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// Каталог данных игры. Если переменных окружения нет (Android, песочница) — текущий каталог.
pub fn data_dir(game: &str) -> PathBuf {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    let base = if cfg!(target_os = "windows") {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    };
    let clean: String = game
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' })
        .collect();
    base.unwrap_or_else(|| PathBuf::from(".")).join(clean.trim_matches('.'))
}

impl SaveBackend for FileBackend {
    fn read(&mut self) -> Result<Option<String>, SaveError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(SaveError::Io {
                path: self.path.display().to_string(),
                source,
            }),
        }
    }

    fn write(&mut self, text: &str) -> Result<(), SaveError> {
        let io = |source| SaveError::Io {
            path: self.path.display().to_string(),
            source,
        };
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let temp = self.path.with_extension("ron.tmp");
        std::fs::write(&temp, text).map_err(io)?;
        std::fs::rename(&temp, &self.path).map_err(io)
    }

    fn quarantine(&mut self, text: &str) {
        let _ = std::fs::write(self.path.with_extension("ron.corrupt"), text);
    }
}

/// Сохранение в памяти — для тестов и игр без сохранений. Копия (`handle`) видит тот же текст.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    text: Rc<RefCell<Option<String>>>,
    corrupt: Rc<RefCell<Option<String>>>,
}

impl MemoryBackend {
    /// Заранее положить текст (например, повреждённый — для проверки восстановления).
    pub fn with_text(text: &str) -> Self {
        let backend = Self::default();
        *backend.text.borrow_mut() = Some(text.to_string());
        backend
    }

    /// Что сейчас «записано».
    pub fn contents(&self) -> Option<String> {
        self.text.borrow().clone()
    }

    /// Что было отложено как нечитаемое.
    pub fn quarantined(&self) -> Option<String> {
        self.corrupt.borrow().clone()
    }
}

impl SaveBackend for MemoryBackend {
    fn read(&mut self) -> Result<Option<String>, SaveError> {
        Ok(self.text.borrow().clone())
    }

    fn write(&mut self, text: &str) -> Result<(), SaveError> {
        *self.text.borrow_mut() = Some(text.to_string());
        Ok(())
    }

    fn quarantine(&mut self, text: &str) {
        *self.corrupt.borrow_mut() = Some(text.to_string());
    }
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    values: BTreeMap<String, Value>,
}

const FORMAT_VERSION: u32 = 1;

/// Ресурс мира: все сохранённые значения. Читать и писать можно в любой момент;
/// на диск уходит только по [`save`](Self::save).
pub struct SaveData {
    values: BTreeMap<String, Value>,
    backend: Box<dyn SaveBackend>,
    dirty: bool,
}

impl SaveData {
    /// Сохранения игры `game` в каталоге данных ОС; сразу читает файл.
    pub fn open(game: &str) -> Self {
        Self::with_backend(FileBackend::for_game(game))
    }

    /// Только в памяти: ничего не переживает перезапуск.
    pub fn in_memory() -> Self {
        Self::with_backend(MemoryBackend::default())
    }

    /// Сохранения в произвольном бэкенде; сразу читает его. Нет данных или они
    /// повреждены — пустой набор (повреждение — предупреждение в логе).
    pub fn with_backend(backend: impl SaveBackend) -> Self {
        let mut data = Self {
            values: BTreeMap::new(),
            backend: Box::new(backend),
            dirty: false,
        };
        data.reload();
        data
    }

    /// Выбросить несохранённые изменения и прочитать бэкенд заново.
    pub fn reload(&mut self) {
        self.values.clear();
        self.dirty = false;
        let text = match self.backend.read() {
            Ok(Some(text)) => text,
            Ok(None) => return,
            Err(e) => {
                log::warn!("save data not loaded, starting with defaults: {e}");
                return;
            }
        };
        match ron::from_str::<SaveFile>(&text) {
            Ok(file) => self.values = file.values,
            Err(e) => {
                log::warn!(
                    "save data is corrupt ({e}); starting with defaults. The unreadable file is kept as a `.corrupt` copy"
                );
                self.backend.quarantine(&text);
            }
        }
    }

    /// Записать в бэкенд. Ошибку (диск полон, нет прав) стоит показать игроку или хотя бы залогировать.
    pub fn save(&mut self) -> Result<(), SaveError> {
        let file = SaveFile {
            version: FORMAT_VERSION,
            values: self.values.clone(),
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .map_err(|e| SaveError::Encode(e.to_string()))?;
        self.backend.write(&text)?;
        self.dirty = false;
        Ok(())
    }

    /// Есть изменения, которых нет в бэкенде.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn delete_key(&mut self, key: &str) -> bool {
        let removed = self.values.remove(key).is_some();
        self.dirty |= removed;
        removed
    }

    pub fn delete_all(&mut self) {
        self.dirty |= !self.values.is_empty();
        self.values.clear();
    }

    /// Значение любого поддерживаемого типа (`bool`, `i32`, `i64`, `u32`, `f32`, `String`,
    /// `Vec2`, `Vec3`, `Quat`, `Color`, `Vec<T>`…). `None` — ключа нет или тип другой.
    pub fn get<T: FieldValue>(&self, key: &str) -> Option<T> {
        T::from_value(self.values.get(key)?)
    }

    pub fn set<T: FieldValue>(&mut self, key: &str, value: &T) {
        let value = value.to_value();
        if self.values.get(key) != Some(&value) {
            self.values.insert(key.to_string(), value);
            self.dirty = true;
        }
    }

    /// Сырое значение ключа.
    pub fn get_value(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.get(key).unwrap_or(default)
    }

    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.get(key).unwrap_or(default)
    }

    pub fn float(&self, key: &str, default: f32) -> f32 {
        self.get(key).unwrap_or(default)
    }

    pub fn string(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or_else(|| default.to_string())
    }

    pub fn vec2(&self, key: &str, default: Vec2) -> Vec2 {
        self.get(key).unwrap_or(default)
    }

    pub fn vec3(&self, key: &str, default: Vec3) -> Vec3 {
        self.get(key).unwrap_or(default)
    }

    pub fn set_bool(&mut self, key: &str, value: bool) {
        self.set(key, &value);
    }

    pub fn set_int(&mut self, key: &str, value: i64) {
        self.set(key, &value);
    }

    pub fn set_float(&mut self, key: &str, value: f32) {
        self.set(key, &value);
    }

    pub fn set_string(&mut self, key: &str, value: &str) {
        self.set(key, &value.to_string());
    }

    pub fn set_vec2(&mut self, key: &str, value: Vec2) {
        self.set(key, &value);
    }

    pub fn set_vec3(&mut self, key: &str, value: Vec3) {
        self.set(key, &value);
    }
}
