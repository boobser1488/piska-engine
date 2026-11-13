//! Именованные сервисы: мост от C++ к частям движка, от которых этот крейт не зависит
//! (физика, звук, интерфейс…).
//!
//! `engine-cpp-bridge` знает только `engine-core`. Всё остальное фасад `engine` кладёт в
//! ресурс [`CppServices`] при старте (`engine::app()`), а C++ зовёт сервис по имени:
//! `engine::service("physics3d.raycast", args)`. Аргументы и результаты — [`Value`]
//! (объект передаётся как `Value::Int(GameObjectId::to_bits())`, см. [`object_arg`] и
//! [`object_value`]).
//!
//! Если сервиса нет (приложение собрано из `App::new()` без фасада, физика не подключена),
//! вызов возвращает `ENGINE_SERVICE_MISSING`, в лог один раз идёт понятная ошибка, а
//! C++-обёртки отдают нейтральное значение — игра продолжает работать.

use std::collections::BTreeMap;
use std::rc::Rc;

use engine_core::prelude::*;

/// Обработчик сервиса: мир, аргументы → результаты или текст ошибки.
pub type ServiceFn = dyn Fn(&mut World, &[Value]) -> Result<Vec<Value>, String>;

/// Таблица сервисов в `world.resources`.
#[derive(Default)]
pub struct CppServices {
    handlers: BTreeMap<String, Rc<ServiceFn>>,
}

impl CppServices {
    /// Зарегистрировать сервис; повторное имя заменяет прежний обработчик.
    pub fn register(
        &mut self,
        name: &str,
        handler: impl Fn(&mut World, &[Value]) -> Result<Vec<Value>, String> + 'static,
    ) -> &mut Self {
        self.handlers.insert(name.to_string(), Rc::new(handler));
        self
    }

    pub fn contains(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Имена всех сервисов по алфавиту.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.handlers.keys().map(String::as_str)
    }

    pub(crate) fn get(&self, name: &str) -> Option<Rc<ServiceFn>> {
        self.handlers.get(name).cloned()
    }
}

/// Объект-аргумент сервиса (`None` — не число или не похоже на объект).
pub fn object_arg(value: &Value) -> Option<GameObjectId> {
    match value {
        Value::Int(bits) if *bits != 0 => Some(GameObjectId::from_bits(*bits as u64)),
        _ => None,
    }
}

/// Объект как результат сервиса.
pub fn object_value(id: GameObjectId) -> Value {
    Value::Int(id.to_bits() as i64)
}

/// Установить таблицу сервисов в мир (если её ещё нет) и вернуть её для регистрации.
pub fn services_mut(world: &mut World) -> &mut CppServices {
    world.resources.get_or_insert_with(CppServices::default)
}
