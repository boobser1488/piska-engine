//! `engine-cpp-bridge` — единственный крейт движка, где разрешён `unsafe`.
//!
//! Два направления границы:
//!
//! * **C++ → Rust (статически).** `cpp/*.cpp` компилируются в `build.rs` и линкуются
//!   в крейт. Сырые объявления — в приватном модуле `ffi`, наружу — безопасные
//!   обёртки ([`noise`]). Так подключаются тяжёлые вычисления и готовые C++-библиотеки.
//! * **Rust ↔ C++ (динамически).** Плагины — `.dll`/`.so`/`.dylib` с точкой входа
//!   `engine_plugin_entry`. ABI описан в [`abi`], заголовок для C++ генерирует
//!   `cbindgen` → `include/engine_api.h`. Загрузчик — [`plugin::Plugin`].
//! * **Игровая логика на C++.** Плагин объявляет компоненты с хуками и полями
//!   (`include/engine_component.hpp`); [`CppComponent`] делает их обычными
//!   компонентами движка, а [`CppPlugins`] перезагружает плагин при пересборке.
//!   Из C++ доступны время, ввод, объекты и иерархия, поля любых компонентов, сообщения,
//!   таймеры, твины, сцены, сохранения, случайные числа; физика, звук и интерфейс — через
//!   именованные [`services`], которые регистрирует фасад `engine`. Руководство: `docs/api/cpp.md`.
//!
//! Каждый блок `unsafe` снабжён комментарием `SAFETY:` с инвариантом, на который он опирается.
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod abi;
mod api;
mod component;
// Статические C++-модули есть не на всех целях (в браузере их нет) — см. build.rs.
#[cfg(engine_static_cpp)]
mod ffi;
mod dylib;
pub mod noise;
pub mod plugin;
mod scope;
pub mod services;

pub use component::CppComponent;
pub use scope::reset_complaints;
pub use services::{object_arg, object_value, services_mut, CppServices, ServiceFn};
pub use plugin::{plugin_file_name, CppPlugins, Plugin, PluginError, StaticScripts};
#[cfg(engine_static_cpp)]
pub use plugin::builtin_gameplay;
