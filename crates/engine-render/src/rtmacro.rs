//! `component_rt!` — как `component!` из ядра, но с одним скрытым полем `rt`:
//! рабочим состоянием компонента (частицы, разобранные клипы, кэш карты тайлов).
//!
//! Поля, перечисленные в теле, попадают в reflection (сцены, инспектор), а `rt` — нет:
//! оно не сохраняется и в инспекторе не видно. Для конструкторов с `..Default::default()`
//! поле открыто (`pub`), но скрыто из документации.

macro_rules! component_rt {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[doc = $fdoc:expr])*
                $fvis:vis $field:ident : $ty:ty = $default:expr
            ),* $(,)?
        }
        runtime: $rt:ty
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        $vis struct $name {
            $( $(#[doc = $fdoc])* $fvis $field: $ty, )*
            /// Рабочее состояние: не сохраняется в сценах.
            #[doc(hidden)]
            pub rt: $rt,
        }

        impl ::core::default::Default for $name {
            fn default() -> Self {
                Self {
                    $( $field: $default, )*
                    rt: ::core::default::Default::default(),
                }
            }
        }

        impl ::engine_core::reflect::TypeName for $name {
            const TYPE_NAME: &'static str = stringify!($name);
        }

        impl ::engine_core::reflect::Reflect for $name {
            fn type_name(&self) -> &'static str {
                stringify!($name)
            }

            fn field_names(&self) -> &'static [&'static str] {
                &[ $( stringify!($field), )* ]
            }

            fn get_field(&self, name: &str) -> ::core::option::Option<::engine_core::reflect::Value> {
                match name {
                    $( stringify!($field) => ::core::option::Option::Some(
                        ::engine_core::reflect::FieldValue::to_value(&self.$field)
                    ), )*
                    _ => ::core::option::Option::None,
                }
            }

            fn set_field(
                &mut self,
                name: &str,
                value: &::engine_core::reflect::Value,
            ) -> ::core::result::Result<(), ::engine_core::error::ReflectError> {
                let _ = value;
                match name {
                    $( stringify!($field) => {
                        self.$field = <$ty as ::engine_core::reflect::FieldValue>::from_value(value)
                            .ok_or(::engine_core::error::ReflectError::TypeMismatch {
                                component: stringify!($name),
                                field: stringify!($field),
                                expected: <$ty as ::engine_core::reflect::FieldValue>::EXPECTED,
                                got: value.kind_name(),
                            })?;
                        Ok(())
                    } )*
                    _ => Err(::engine_core::error::ReflectError::NoSuchField {
                        component: stringify!($name),
                        field: name.to_string(),
                        available: ::engine_core::reflect::Reflect::field_names(self).join(", "),
                    }),
                }
            }

            fn field_meta(&self, name: &str) -> ::engine_core::reflect::FieldMeta {
                match name {
                    $( stringify!($field) => <$ty as ::engine_core::reflect::FieldValue>::meta(), )*
                    _ => ::engine_core::reflect::FieldMeta::default(),
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
