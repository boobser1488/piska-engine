//! «Потерянный скрипт» (Unity: *Missing Script*): компонент из сцены, тип которого
//! в программе больше не зарегистрирован — например, скрипт на Rust удалили или
//! переименовали и редактор перезапустился с новым кодом.
//!
//! Игра по-прежнему считает такую сцену ошибкой ([`SceneError::UnknownComponent`]).
//! Редактор включает [`keep_missing_scripts`]: тогда загрузчик сцен вместо ошибки
//! кладёт на объект [`MissingScript`] с сохранёнными полями. Он ничего не делает,
//! его поля только для чтения, а при записи сцены он сохраняется ровно таким, каким
//! был прочитан (то же имя типа, те же поля) — вернёте тип, и данные вернутся с ним.
//!
//! [`SceneError::UnknownComponent`]: crate::error::SceneError::UnknownComponent

use std::any::Any;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::component::{Component, Hooks};
use crate::error::ReflectError;
use crate::reflect::{Reflect, Value};

static KEEP: AtomicBool = AtomicBool::new(false);

/// Загружать сцены с незарегистрированными компонентами, сохраняя их как
/// [`MissingScript`], а не отказываться от всей сцены. Действует на весь процесс;
/// редактор включает это при старте.
pub fn keep_missing_scripts(keep: bool) {
    KEEP.store(keep, Ordering::Relaxed);
}

/// Включено ли [`keep_missing_scripts`].
pub fn keeps_missing_scripts() -> bool {
    KEEP.load(Ordering::Relaxed)
}

/// Строка, живущая до конца программы. `Reflect` отдаёт имена как `&'static str`, а
/// имя потерянного типа известно только из файла сцены — поэтому имена «интернируются»:
/// каждое различное имя хранится один раз, сколько бы раз сцену ни перезагружали.
fn intern(text: &str) -> &'static str {
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|poison| poison.into_inner());
    let names = names.get_or_insert_with(HashSet::new);
    if let Some(found) = names.get(text) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// То же для списка имён полей.
fn intern_list(list: Vec<&'static str>) -> &'static [&'static str] {
    static LISTS: Mutex<Option<HashSet<&'static [&'static str]>>> = Mutex::new(None);
    let mut lists = LISTS.lock().unwrap_or_else(|poison| poison.into_inner());
    let lists = lists.get_or_insert_with(HashSet::new);
    if let Some(found) = lists.get(list.as_slice()) {
        return found;
    }
    let leaked: &'static [&'static str] = Box::leak(list.into_boxed_slice());
    lists.insert(leaked);
    leaked
}

/// Компонент, тип которого не зарегистрирован: хранит имя типа и поля из файла.
#[derive(Clone, Debug)]
pub struct MissingScript {
    kind: &'static str,
    names: &'static [&'static str],
    values: Vec<Value>,
}

impl MissingScript {
    /// Потерянный компонент `kind` с полями, как они записаны в сцене.
    pub fn new(kind: &str, fields: &BTreeMap<String, Value>) -> Self {
        let names = intern_list(fields.keys().map(|name| intern(name)).collect());
        Self {
            kind: intern(kind),
            names,
            values: fields.values().cloned().collect(),
        }
    }

    /// Имя типа из сцены — под ним компонент и сохраняется.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// Сохранённые поля по порядку имён.
    pub fn fields(&self) -> impl Iterator<Item = (&'static str, &Value)> + '_ {
        self.names.iter().copied().zip(&self.values)
    }
}

impl Reflect for MissingScript {
    fn type_name(&self) -> &'static str {
        self.kind
    }

    fn field_names(&self) -> &'static [&'static str] {
        self.names
    }

    fn get_field(&self, name: &str) -> Option<Value> {
        self.fields().find(|(field, _)| *field == name).map(|(_, value)| value.clone())
    }

    /// Поля только для чтения: компонент ждёт, пока его тип снова зарегистрируют.
    fn set_field(&mut self, name: &str, value: &Value) -> Result<(), ReflectError> {
        match self.names.iter().find(|field| **field == name) {
            Some(field) => Err(ReflectError::TypeMismatch {
                component: self.kind,
                field,
                expected: "no change: the script is missing, so its fields are read-only until \
                           the type is registered again",
                got: value.kind_name(),
            }),
            None => Err(ReflectError::NoSuchField {
                component: self.kind,
                field: name.to_string(),
                available: self.names.join(", "),
            }),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Component for MissingScript {
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_script_keeps_its_name_and_fields_read_only() {
        let mut fields = BTreeMap::new();
        fields.insert("speed".to_string(), Value::Float(2.5));
        fields.insert("label".to_string(), Value::Str("hi".to_string()));
        let mut missing = MissingScript::new("Gone", &fields);
        assert_eq!(missing.type_name(), "Gone");
        assert_eq!(missing.field_names(), ["label", "speed"]);
        assert_eq!(missing.get_field("speed"), Some(Value::Float(2.5)));
        assert!(missing.set_field("speed", &Value::Float(1.0)).is_err());
        assert_eq!(missing.get_field("speed"), Some(Value::Float(2.5)));
        // Имена интернируются: второй экземпляр не выделяет новых строк.
        let again = MissingScript::new("Gone", &fields);
        assert!(std::ptr::eq(missing.kind(), again.kind()));
        assert!(std::ptr::eq(missing.field_names(), again.field_names()));
    }
}
