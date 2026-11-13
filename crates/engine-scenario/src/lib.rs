//! `engine-scenario` — **Scenario**, маленький встроенный язык сценариев движка Piska:
//! катсцены, обучение, диалоги, события уровня, волны врагов, «клей» между компонентами.
//!
//! Синтаксис — смесь Lua и JavaScript: обе записи принимаются и смешиваются в одном файле
//! (`function f() { … }` и `function f() … end`, `&&` и `and`, `//` и `--`). Руководство —
//! `docs/api/scenario.md`.
//!
//! Ядро не зависит от движка, без потоков и `std::time`, собирается под
//! `wasm32-unknown-unknown`:
//!
//! * [`Script::compile`] — лексер → парсер → компилятор в байткод;
//! * [`Vm`] — встроенные функции, методы типов, классы значений хоста, пределы;
//! * [`Task`] — сопрограмма: [`Task::resume`] исполняет до `wait…`, конца или исчерпания
//!   бюджета инструкций и возвращает управление;
//! * [`Group`] — задачи одного экземпляра скрипта с ожиданиями по времени, кадрам и
//!   сообщениям; хост вызывает [`Group::update`] раз в кадр.
//!
//! ```
//! use engine_scenario::{Script, Vm, Value};
//! let mut vm = Vm::new();
//! let v = vm.run_source("demo.scenario", "let xs = [3, 1, 2]\nxs.sort()\nreturn xs.join(\"-\")").unwrap();
//! assert_eq!(v, Value::str("1-2-3"));
//! ```
#![forbid(unsafe_code)]
// Ошибка скрипта несёт выдержку, подсказку и стек; она возникает редко, а `Box` в каждом
// `Result` компилятора только усложнил бы код.
#![allow(clippy::result_large_err)]

mod ast;
mod bytecode;
mod compiler;
mod error;
mod lexer;
mod parser;
mod sched;
mod stdlib;
mod task;
mod value;
mod vm;

pub use compiler::Script;
pub use error::{ErrorKind, Pos, ScriptError, StackFrame};
pub use lexer::KEYWORDS;
pub use sched::{Group, Report};
pub use stdlib::format;
pub use task::Task;
pub use value::{
    clean, fmt_num, live_values, Args, Handle, Key, List, Map, Module, Native, NativeFn, NativeResult, OrderedMap,
    Throw, Value, TASK_CLASS,
};
pub use vm::{Class, GetFn, Instance, Level, Limits, NativeCtx, Rng, SchedCtx, SetFn, Status, Suspend, TypeTag, Vm};

/// Встроенная функция как значение (например, для таблиц функций вроде `ui.text`).
pub fn function(name: &str, f: impl Fn(&mut NativeCtx, Args) -> NativeResult + 'static) -> Value {
    vm::native(name, false, f)
}

/// Расширение файлов сценариев.
pub const EXTENSION: &str = "scenario";

/// Проверить исходник без запуска: синтаксис и (если дана ВМ) неизвестные имена.
pub fn check(name: &str, source: &str, vm: Option<&Vm>) -> Result<(), ScriptError> {
    let script = Script::compile(name, source)?;
    if let Some(vm) = vm {
        vm.instantiate(&script)?;
    }
    Ok(())
}
