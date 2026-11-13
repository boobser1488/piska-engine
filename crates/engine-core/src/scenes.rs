//! Управление сценами из игрового кода: «загрузи уровень 2», «добавь сцену HUD».
//!
//! Скрипт, который просит загрузку, сам живёт в заменяемой сцене, поэтому запрос лишь
//! ставится в очередь ([`SceneManager`]), а выполняется **в конце кадра**, после
//! `on_late_update` и отложенных `destroy`. Все хуки текущего кадра успевают отработать,
//! у уничтожаемых объектов вызывается `on_destroy`, а первый `on_start` новой сцены
//! будет в следующем кадре.
//!
//! ```
//! use engine_core::prelude::*;
//!
//! component! {
//!     pub struct ExitDoor { pub next: String = String::from("level2") }
//! }
//! impl Component for ExitDoor {
//!     fn on_trigger_enter(&mut self, ctx: &mut Ctx, _other: GameObjectId) {
//!         ctx.load_scene(&self.next);
//!     }
//! }
//! ```
//!
//! Где искать сцену по имени — решает [`SceneSource`]: каталог ([`DirScenes`], по умолчанию —
//! текущий) или память ([`MemoryScenes`], для тестов). Подменить:
//! `world.scenes().set_source(DirScenes::new("assets/scenes"))`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::SceneError;
use crate::object::GameObjectId;

/// Откуда брать текст сцены по имени, которое передали в `load_scene`.
pub trait SceneSource: 'static {
    /// RON-текст сцены. Ошибка должна говорить, что сцена не найдена и где искали.
    fn load(&self, name: &str) -> Result<String, SceneError>;
}

/// Сцены лежат в каталоге: `load_scene("level2")` читает `<root>/level2` или `<root>/level2.scene`.
pub struct DirScenes {
    root: PathBuf,
}

impl DirScenes {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl SceneSource for DirScenes {
    fn load(&self, name: &str) -> Result<String, SceneError> {
        let direct = self.root.join(name);
        let with_ext = self.root.join(format!("{name}.scene"));
        let path = if direct.is_file() { direct } else { with_ext };
        std::fs::read_to_string(&path).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

/// Сцены в памяти: имя → RON-текст. Для тестов и встроенных в бинарник уровней (`include_str!`).
#[derive(Default)]
pub struct MemoryScenes {
    scenes: HashMap<String, String>,
}

impl MemoryScenes {
    pub fn with(mut self, name: impl Into<String>, ron: impl Into<String>) -> Self {
        self.scenes.insert(name.into(), ron.into());
        self
    }
}

impl SceneSource for MemoryScenes {
    fn load(&self, name: &str) -> Result<String, SceneError> {
        self.scenes.get(name).cloned().ok_or_else(|| SceneError::Io {
            path: format!("scene `{name}`"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such scene registered in MemoryScenes",
            ),
        })
    }
}

pub(crate) enum Request {
    Load { name: String, additive: bool },
    Reload,
    Unload(String),
}

/// Очередь запросов на смену сцены. Ресурс мира, создаётся при первом обращении.
pub struct SceneManager {
    source: Box<dyn SceneSource>,
    pub(crate) pending: Vec<Request>,
    pub(crate) loaded_name: Option<String>,
    /// Корни, добавленные аддитивными загрузками, по имени сцены.
    pub(crate) additive: HashMap<String, Vec<GameObjectId>>,
}

impl Default for SceneManager {
    fn default() -> Self {
        Self {
            source: Box::new(DirScenes::new(".")),
            pending: Vec::new(),
            loaded_name: None,
            additive: HashMap::new(),
        }
    }
}

impl SceneManager {
    pub fn set_source(&mut self, source: impl SceneSource) {
        self.source = Box::new(source);
    }

    pub(crate) fn read(&self, name: &str) -> Result<String, SceneError> {
        self.source.load(name)
    }

    /// Заменить текущую сцену (в конце кадра). Объекты с «не уничтожать при загрузке» остаются.
    pub fn load(&mut self, name: &str) {
        self.pending.push(Request::Load {
            name: name.to_string(),
            additive: false,
        });
    }

    /// Добавить объекты сцены к текущей, ничего не удаляя (в конце кадра).
    pub fn load_additive(&mut self, name: &str) {
        self.pending.push(Request::Load {
            name: name.to_string(),
            additive: true,
        });
    }

    /// Перезагрузить сцену, загруженную последней через менеджер.
    pub fn reload(&mut self) {
        self.pending.push(Request::Reload);
    }

    /// Убрать объекты, добавленные `load_additive(name)`.
    pub fn unload(&mut self, name: &str) {
        self.pending.push(Request::Unload(name.to_string()));
    }

    /// Имя, с которым последний раз вызывали `load` (не обязательно совпадает с `scene.name` из файла).
    pub fn loaded_name(&self) -> Option<&str> {
        self.loaded_name.as_deref()
    }

    /// Есть ли запрос, который сработает в конце кадра.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// Событие: сцена загружена (читается в следующем кадре).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneLoaded {
    pub name: String,
    pub additive: bool,
}

/// Событие: запрос на загрузку не удался; текущая сцена не тронута.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneLoadFailed {
    pub name: String,
    pub error: String,
}
