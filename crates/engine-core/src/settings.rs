//! Настройки проекта (Unity: Project Settings). Сейчас — порядок выполнения скриптов
//! (Script Execution Order): файл `project.settings` в корне ассетов, RON:
//!
//! ```ron
//! (
//!     script_order: {
//!         "InputReader": -100,
//!         "CameraFollow": 100,
//!     },
//! )
//! ```
//!
//! Файл читает `engine::mount_assets` (он же попадает в пак ассетов экспортированной
//! игры), редактор пишет его из окна Project Settings. Незнакомые поля и отсутствующий
//! файл не ошибка: отсутствующее поле берёт значение по умолчанию.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::world::World;

pub use crate::settings_sections::*;

/// Имя файла настроек относительно корня ассетов.
pub const PROJECT_SETTINGS_FILE: &str = "project.settings";

/// Ошибки чтения и записи настроек проекта.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot parse project settings `{origin}`: {message}. Fix the file or delete it to reset the settings")]
    Parse { origin: String, message: String },
    #[error("cannot access `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Настройки проекта. Лежат в `world.resources`, пока их туда кладёт
/// [`World::apply_project_settings`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// Порядок выполнения по имени типа компонента: меньше — раньше. Перекрывает
    /// `Component::execution_order`. Тип, которого здесь нет, берёт порядок из кода.
    pub script_order: BTreeMap<String, i32>,
    /// Окно, название, версия, сцена по умолчанию.
    pub player: PlayerSettings,
    pub time: TimeSettings,
    pub physics2d: Physics2dSettings,
    pub physics3d: Physics3dSettings,
    pub tags_and_layers: TagsAndLayers,
    /// Действия ввода (`Input::button` / `Input::axis`).
    pub input: InputSettings,
    pub graphics: GraphicsSettings,
    pub audio: AudioSettings,
}

impl ProjectSettings {
    /// Разобрать текст файла `project.settings`. Пустой текст — настройки по умолчанию.
    pub fn from_ron(text: &str, origin: &str) -> Result<Self, SettingsError> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        ron::from_str(text).map_err(|e| SettingsError::Parse {
            origin: origin.to_string(),
            message: e.to_string(),
        })
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new())
            .expect("project settings are always serializable")
    }

    /// Прочитать файл; нет файла — настройки по умолчанию.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SettingsError> {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_ron(&text, &path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(SettingsError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SettingsError> {
        let path = path.as_ref();
        std::fs::write(path, self.to_ron()).map_err(|source| SettingsError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    /// Порядок типа `name` из настроек (`None` — не задан, берётся из кода).
    pub fn order_of(&self, name: &str) -> Option<i32> {
        self.script_order.get(name).copied()
    }

    /// Задать порядок типа.
    pub fn set_order(&mut self, name: &str, order: i32) {
        self.script_order.insert(name.to_string(), order);
    }

    /// Убрать тип из настроек: он вернётся к порядку из кода.
    pub fn clear_order(&mut self, name: &str) -> bool {
        self.script_order.remove(name).is_some()
    }

    /// Таблица для [`Scene::set_execution_orders`](crate::scene::Scene::set_execution_orders).
    pub fn orders(&self) -> HashMap<String, i32> {
        self.script_order.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
}

impl World {
    /// Применить настройки проекта: положить в ресурсы и задать порядок выполнения
    /// сцене. Работает и с уже созданными объектами (список вызова пересортируется один
    /// раз), и с теми, что появятся при смене сцены.
    ///
    /// Здесь применяется то, что принадлежит самому ядру: порядок скриптов, время,
    /// действия ввода. Остальное читают подсистемы из ресурса [`ProjectSettings`] (физика,
    /// звук) или из отдельных ресурсов ([`GraphicsSettings`] читает рендерер).
    pub fn apply_project_settings(&mut self, settings: ProjectSettings) {
        self.scene.set_execution_orders(settings.orders());
        let time = settings.time.sanitized();
        self.time.fixed_delta = time.fixed_timestep;
        self.time.max_delta = time.max_timestep;
        self.time.time_scale = time.time_scale;
        settings.input.apply(&mut self.input);
        self.resources.insert(settings.graphics.clone());
        self.resources.insert(settings);
        // Разделы подсистем (физика, звук): их применяют сами подсистемы через хуки,
        // зарегистрированные при сборке приложения (`engine::app()`).
        let hooks: Vec<SettingsHook> =
            self.resources.get::<SettingsHooks>().map(|h| h.0.iter().map(|(_, f)| *f).collect()).unwrap_or_default();
        for hook in hooks {
            hook(self);
        }
    }

    /// Действующие настройки проекта (по умолчанию — пустые).
    pub fn project_settings(&self) -> ProjectSettings {
        self.resources.get::<ProjectSettings>().cloned().unwrap_or_default()
    }

    /// Зарегистрировать применение раздела настроек подсистемой: `hook` вызывается в
    /// конце каждого [`apply_project_settings`](Self::apply_project_settings) (при
    /// `engine::mount_assets` и когда редактор меняет настройки), ресурс
    /// [`ProjectSettings`] к этому моменту уже лежит в мире. Хук с тем же `name`
    /// заменяется — повторная регистрация безвредна.
    pub fn add_settings_hook(&mut self, name: &'static str, hook: SettingsHook) {
        if !self.resources.contains::<SettingsHooks>() {
            self.resources.insert(SettingsHooks::default());
        }
        let hooks = &mut self.resources.get_mut::<SettingsHooks>().expect("inserted above").0;
        match hooks.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = hook,
            None => hooks.push((name, hook)),
        }
    }

    /// Имена зарегистрированных хуков настроек (по порядку вызова).
    pub fn settings_hooks(&self) -> Vec<&'static str> {
        self.resources.get::<SettingsHooks>().map(|h| h.0.iter().map(|(n, _)| *n).collect()).unwrap_or_default()
    }
}

/// Применение раздела настроек подсистемой (см. [`World::add_settings_hook`]).
pub type SettingsHook = fn(&mut World);

#[derive(Default)]
struct SettingsHooks(Vec<(&'static str, SettingsHook)>);

/// Окно игры в фокусе? Ресурс кладёт оконный цикл (`engine-render`) при смене фокуса;
/// его читает звук (`AudioSettings::mute_when_unfocused`). Нет ресурса (тесты, без
/// окна) — считается, что в фокусе.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowFocus {
    pub focused: bool,
}
