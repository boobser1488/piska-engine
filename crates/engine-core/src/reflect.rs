//! Reflection: единый механизм для сериализации сцен и авто-инспектора.
//!
//! Компонент объявляется макросом [`component!`](crate::component!), который
//! генерирует структуру, `Default` и [`Reflect`]. Дальше и сохранение в `.scene`,
//! и инспектор редактора ходят по полям через `field_names` /
//! `get_field` / `set_field`, ничего не зная о конкретном типе.

use std::any::Any;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::component::Component;
use crate::error::{did_you_mean, ReflectError, SceneError};
use crate::math::{Color, Quat, Vec2, Vec3};

/// Динамическое значение поля. Именно в таком виде поля лежат в `.scene`/`.prefab`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f32),
    Str(String),
    Vec2(f32, f32),
    Vec3(f32, f32, f32),
    Quat(f32, f32, f32, f32),
    Color(f32, f32, f32, f32),
    /// Вариант перечисления по имени: `Enum("Dynamic")`.
    Enum(String),
    /// Ссылка на ассет по GUID: `Asset("3f2a…")`. Пустая строка — «не назначено».
    Asset(String),
    List(Vec<Value>),
}

impl Value {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Nil => "Nil",
            Value::Bool(_) => "Bool",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Str(_) => "Str",
            Value::Vec2(..) => "Vec2",
            Value::Vec3(..) => "Vec3",
            Value::Quat(..) => "Quat",
            Value::Color(..) => "Color",
            Value::Enum(_) => "Enum",
            Value::Asset(_) => "Asset",
            Value::List(_) => "List",
        }
    }
}

/// Статические сведения о поле — то, что нужно инспектору сверх значения.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FieldMeta {
    /// Варианты перечисления (для выпадающего списка). Пусто — поле не перечисление.
    pub variants: &'static [&'static str],
    /// Какой ассет ждёт поле: имя импортёра (`"Texture"`, `"Prefab"`…). Пусто — не ассет.
    pub asset_kind: &'static str,
}

/// Тип, который может быть полем компонента.
pub trait FieldValue: Sized {
    /// Что писать в сообщении об ошибке: «expects Float».
    const EXPECTED: &'static str;
    fn to_value(&self) -> Value;
    fn from_value(value: &Value) -> Option<Self>;
    /// Подсказки для инспектора; по умолчанию их нет.
    fn meta() -> FieldMeta {
        FieldMeta::default()
    }
}

impl FieldValue for bool {
    const EXPECTED: &'static str = "Bool";
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

macro_rules! int_field {
    ($($ty:ty),*) => {$(
        impl FieldValue for $ty {
            const EXPECTED: &'static str = "Int";
            fn to_value(&self) -> Value {
                Value::Int(*self as i64)
            }
            fn from_value(value: &Value) -> Option<Self> {
                match value {
                    Value::Int(i) => <$ty>::try_from(*i).ok(),
                    _ => None,
                }
            }
        }
    )*};
}
int_field!(i32, u32, i64);

impl FieldValue for f32 {
    const EXPECTED: &'static str = "Float";
    fn to_value(&self) -> Value {
        Value::Float(*self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f32), // `Float` допускает запись `Int(3)` в файле
            _ => None,
        }
    }
}

impl FieldValue for String {
    const EXPECTED: &'static str = "Str";
    fn to_value(&self) -> Value {
        Value::Str(self.clone())
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl FieldValue for Vec2 {
    const EXPECTED: &'static str = "Vec2";
    fn to_value(&self) -> Value {
        Value::Vec2(self.x, self.y)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Vec2(x, y) => Some(Vec2::new(*x, *y)),
            _ => None,
        }
    }
}

impl FieldValue for Vec3 {
    const EXPECTED: &'static str = "Vec3";
    fn to_value(&self) -> Value {
        Value::Vec3(self.x, self.y, self.z)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Vec3(x, y, z) => Some(Vec3::new(*x, *y, *z)),
            _ => None,
        }
    }
}

impl FieldValue for Quat {
    const EXPECTED: &'static str = "Quat";
    fn to_value(&self) -> Value {
        Value::Quat(self.x, self.y, self.z, self.w)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Quat(x, y, z, w) => Some(Quat::from_xyzw(*x, *y, *z, *w)),
            _ => None,
        }
    }
}

impl FieldValue for Color {
    const EXPECTED: &'static str = "Color";
    fn to_value(&self) -> Value {
        Value::Color(self.r, self.g, self.b, self.a)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Color(r, g, b, a) => Some(Color::rgba(*r, *g, *b, *a)),
            _ => None,
        }
    }
}

impl<T: FieldValue> FieldValue for Vec<T> {
    const EXPECTED: &'static str = "List";
    fn to_value(&self) -> Value {
        Value::List(self.iter().map(FieldValue::to_value).collect())
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::List(items) => items.iter().map(T::from_value).collect(),
            _ => None,
        }
    }
}

/// Доступ к полям по имени. Реализуется макросом [`component!`](crate::component!).
pub trait Reflect: Any {
    fn type_name(&self) -> &'static str;
    fn field_names(&self) -> &'static [&'static str];
    fn get_field(&self, name: &str) -> Option<Value>;
    fn set_field(&mut self, name: &str, value: &Value) -> Result<(), ReflectError>;
    /// Подсказки инспектору: варианты перечисления, тип ассета.
    fn field_meta(&self, name: &str) -> FieldMeta {
        let _ = name;
        FieldMeta::default()
    }
    /// Doc-комментарий поля — инспектор показывает его всплывающей подсказкой.
    fn field_doc(&self, name: &str) -> &'static str {
        let _ = name;
        ""
    }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Имя типа, известное без экземпляра (нужно реестру).
pub trait TypeName {
    const TYPE_NAME: &'static str;
}

/// Функция, создающая компонент со значениями по умолчанию.
type Factory = Rc<dyn Fn() -> Box<dyn Component>>;

/// Реестр типов компонентов: «имя из файла сцены → конструктор».
#[derive(Default)]
pub struct TypeRegistry {
    factories: BTreeMap<&'static str, Factory>,
    /// Категория типа для меню Add Component (см. [`categorize`](TypeRegistry::categorize)).
    categories: BTreeMap<&'static str, &'static str>,
}

impl TypeRegistry {
    pub fn register<T: Component + Default + TypeName>(&mut self) -> &mut Self {
        self.register_factory(T::TYPE_NAME, || Box::new(T::default()))
    }

    /// Зарегистрировать тип, который создаётся произвольной функцией, — так в реестр
    /// попадают компоненты из C++-плагинов. Повторная регистрация заменяет прежнюю.
    pub fn register_factory(
        &mut self,
        name: &'static str,
        factory: impl Fn() -> Box<dyn Component> + 'static,
    ) -> &mut Self {
        self.factories.insert(name, Rc::new(factory));
        self
    }

    /// Как [`register`](Self::register), с категорией для меню Add Component в редакторе
    /// (`"Rendering"`, `"Physics 3D"`, `"Audio"`…). Без категории тип считается
    /// скриптом: Rust-компоненты игры идут в «Scripts», C++ — в «Scripts (C++)».
    pub fn register_in<T: Component + Default + TypeName>(&mut self, category: &'static str) -> &mut Self {
        self.register::<T>().categorize(&[T::TYPE_NAME], category)
    }

    /// Назначить категорию уже зарегистрированным типам (по именам).
    pub fn categorize(&mut self, names: &[&'static str], category: &'static str) -> &mut Self {
        for name in names {
            self.categories.insert(name, category);
        }
        self
    }

    /// Категория типа, если она назначена.
    pub fn category_of(&self, kind: &str) -> Option<&'static str> {
        self.categories.get(kind).copied()
    }

    /// Порядок выполнения, который тип объявляет в коде (`Component::execution_order`).
    /// Создаёт для этого временный экземпляр; `None` — типа нет в реестре.
    pub fn default_order_of(&self, kind: &str) -> Option<i32> {
        self.factories.get(kind).map(|factory| factory().execution_order())
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.factories.keys().copied()
    }

    /// Создать компонент со значениями по умолчанию по имени типа.
    pub fn create(&self, kind: &str) -> Result<Box<dyn Component>, SceneError> {
        match self.factories.get(kind) {
            Some(factory) => Ok(factory()),
            None => Err(SceneError::UnknownComponent {
                kind: kind.to_string(),
                hint: did_you_mean(kind, self.names()),
            }),
        }
    }
}

/// Объявляет компонент: структуру, значения по умолчанию и reflection.
///
/// ```
/// use engine_core::prelude::*;
///
/// component! {
///     /// Вращает объект вокруг оси Z.
///     pub struct Spinner {
///         pub speed_deg: f32 = 90.0,
///     }
/// }
///
/// impl Component for Spinner {
///     fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
///         ctx.transform().rotate_z(self.speed_deg.to_radians() * dt);
///     }
/// }
/// ```
#[macro_export]
macro_rules! component {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[doc = $fdoc:expr])*
                $fvis:vis $field:ident : $ty:ty = $default:expr
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        $vis struct $name {
            $( $(#[doc = $fdoc])* $fvis $field: $ty, )*
        }

        impl ::core::default::Default for $name {
            fn default() -> Self {
                Self { $( $field: $default, )* }
            }
        }

        impl $crate::reflect::TypeName for $name {
            const TYPE_NAME: &'static str = stringify!($name);
        }

        impl $crate::reflect::Reflect for $name {
            fn type_name(&self) -> &'static str {
                stringify!($name)
            }

            fn field_names(&self) -> &'static [&'static str] {
                &[ $( stringify!($field), )* ]
            }

            fn get_field(&self, name: &str) -> ::core::option::Option<$crate::reflect::Value> {
                match name {
                    $( stringify!($field) => ::core::option::Option::Some(
                        $crate::reflect::FieldValue::to_value(&self.$field)
                    ), )*
                    _ => ::core::option::Option::None,
                }
            }

            fn set_field(
                &mut self,
                name: &str,
                value: &$crate::reflect::Value,
            ) -> ::core::result::Result<(), $crate::error::ReflectError> {
                let _ = value;
                match name {
                    $( stringify!($field) => {
                        self.$field = <$ty as $crate::reflect::FieldValue>::from_value(value)
                            .ok_or($crate::error::ReflectError::TypeMismatch {
                                component: stringify!($name),
                                field: stringify!($field),
                                expected: <$ty as $crate::reflect::FieldValue>::EXPECTED,
                                got: value.kind_name(),
                            })?;
                        Ok(())
                    } )*
                    _ => Err($crate::error::ReflectError::NoSuchField {
                        component: stringify!($name),
                        field: name.to_string(),
                        available: $crate::reflect::Reflect::field_names(self).join(", "),
                    }),
                }
            }

            fn field_meta(&self, name: &str) -> $crate::reflect::FieldMeta {
                match name {
                    $( stringify!($field) => <$ty as $crate::reflect::FieldValue>::meta(), )*
                    _ => $crate::reflect::FieldMeta::default(),
                }
            }

            fn field_doc(&self, name: &str) -> &'static str {
                match name {
                    $( stringify!($field) => concat!($( $fdoc, "\n", )*), )*
                    _ => "",
                }
            }

            fn as_any(&self) -> &dyn ::core::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any {
                self
            }
        }
    };
}

/// Объявляет перечисление, пригодное как поле компонента (`Enum("Variant")` в файле).
#[macro_export]
macro_rules! reflect_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident { $( $variant:ident ),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name { $( $variant, )* }

        impl $crate::reflect::FieldValue for $name {
            const EXPECTED: &'static str =
                concat!("Enum(", stringify!($name), ": ", $( stringify!($variant), " ", )* ")");

            fn to_value(&self) -> $crate::reflect::Value {
                let name = match self { $( Self::$variant => stringify!($variant), )* };
                $crate::reflect::Value::Enum(name.to_string())
            }

            fn from_value(value: &$crate::reflect::Value) -> ::core::option::Option<Self> {
                match value {
                    $crate::reflect::Value::Enum(s) | $crate::reflect::Value::Str(s) => {
                        match s.as_str() {
                            $( stringify!($variant) => Some(Self::$variant), )*
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }

            fn meta() -> $crate::reflect::FieldMeta {
                $crate::reflect::FieldMeta {
                    variants: &[ $( stringify!($variant), )* ],
                    asset_kind: "",
                }
            }
        }
    };
}
