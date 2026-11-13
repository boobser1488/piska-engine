//! Виртуальная машина: встроенные функции, методы типов, классы хоста, пределы и
//! контекст встроенных функций. Само исполнение — в [`crate::task`].

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::compiler::Script;
use crate::error::{suggest, ErrorKind, Pos, ScriptError};
use crate::value::*;

/// Ограничения, защищающие игру и редактор от скрипта.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Инструкций за одно возобновление задачи; дальше задача откладывается до
    /// следующего кадра (с одним предупреждением на скрипт).
    pub instructions_per_resume: u32,
    /// Сколько кадров подряд задача может работать без `wait…`, прежде чем её остановят.
    pub max_busy_frames: u32,
    /// Глубина вложенных вызовов.
    pub max_call_depth: usize,
    /// Мягкий предел памяти: элементов во всех списках и словарях.
    pub max_values: usize,
    /// Длина строки в байтах.
    pub max_string_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            instructions_per_resume: 200_000,
            max_busy_frames: 600,
            max_call_depth: 200,
            max_values: 4_000_000,
            max_string_len: 4 << 20,
        }
    }
}

/// Почему задача приостановилась.
#[derive(Clone, Debug)]
pub enum Suspend {
    /// `wait(seconds)`.
    Seconds(f64),
    /// `wait_frame()` — `n` кадров.
    Frames(u32),
    /// `wait_message(name)` — до сообщения; результат — значение сообщения.
    Message(Rc<str>),
    /// `wait_task(t)` — до завершения другой задачи.
    Task(u64),
    /// Ожидание, которое проверяет хост (движок): см. [`crate::Group::update`].
    Host(Value),
}

/// Итог возобновления задачи.
#[derive(Clone, Debug)]
pub enum Status {
    Done(Value),
    Suspended(Suspend),
    /// Исчерпан бюджет инструкций: продолжить в следующем кадре.
    Preempted,
}

/// Уровень сообщения `print` / `log` / `warn`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
}

/// Свойство ручки: `Ok(None)` — такого свойства нет.
pub type GetFn = dyn Fn(&mut NativeCtx, u64, &str) -> Result<Option<Value>, Throw>;
/// Запись свойства ручки: `Ok(false)` — такого свойства нет.
pub type SetFn = dyn Fn(&mut NativeCtx, u64, &str, Value) -> Result<bool, Throw>;

/// Класс значений хоста (объекты игры и т. п.).
pub struct Class {
    pub name: Rc<str>,
    pub(crate) get: Option<Rc<GetFn>>,
    pub(crate) set: Option<Rc<SetFn>>,
    pub(crate) methods: HashMap<Rc<str>, Value>,
}

/// Куда пишут `print` / `log` / `warn`.
pub type Printer = Box<dyn FnMut(Level, &str)>;

/// Встроенные типы, у которых есть методы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeTag {
    Str = 0,
    List = 1,
    Map = 2,
    Num = 3,
    Vec2 = 4,
    Vec3 = 5,
    Color = 6,
    Func = 7,
}

const TYPE_COUNT: usize = 8;

/// Генератор случайных чисел (xorshift64*): детерминированный, с зерном.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng(0);
        r.seed(seed);
        r
    }

    pub fn seed(&mut self, seed: u64) {
        // splitmix64, чтобы маленькие зёрна давали хорошее начальное состояние.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        self.0 = (z ^ (z >> 31)) | 1;
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// [0, 1).
    pub fn float(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Целое в [a, b] включительно.
    pub fn int(&mut self, a: i64, b: i64) -> i64 {
        if b <= a {
            return a;
        }
        let span = (b - a) as u64 + 1;
        a + (self.next_u64() % span) as i64
    }
}

/// Планировочные данные группы задач, доступные встроенным функциям.
#[derive(Default)]
pub struct SchedCtx {
    pub(crate) current: u64,
    pub(crate) user: u64,
    pub(crate) next_id: u64,
    pub(crate) live: HashSet<u64>,
    pub(crate) spawned: Vec<(u64, Value, Vec<Value>)>,
    pub(crate) stopped: Vec<u64>,
    pub(crate) stop_others: bool,
    pub(crate) kill_current: bool,
    pub(crate) handlers: Vec<(Rc<str>, Value)>,
    pub(crate) posts: Vec<(Rc<str>, Value)>,
    pub(crate) suspend: Option<Suspend>,
    pub(crate) time: f64,
    pub(crate) dt: f64,
    pub(crate) frame: u64,
}

/// Контекст встроенной функции: хост, ВМ и планировщик задач.
pub struct NativeCtx<'a> {
    pub(crate) vm: &'a mut Vm,
    pub(crate) host: &'a mut dyn Any,
    pub(crate) sched: &'a mut SchedCtx,
}

impl NativeCtx<'_> {
    /// Данные хоста (в движке — `World`).
    pub fn host<T: 'static>(&mut self) -> Option<&mut T> {
        self.host.downcast_mut::<T>()
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.vm.rng
    }

    pub fn limits(&self) -> &Limits {
        &self.vm.limits
    }

    /// ВМ (только чтение).
    pub fn vm_ref(&self) -> &Vm {
        self.vm
    }

    /// Пишет в вывод ВМ (`print`).
    pub fn print(&mut self, level: Level, text: &str) {
        (self.vm.printer)(level, text);
    }

    /// Приостановить текущую задачу после возврата из функции. Значение, с которым
    /// задачу возобновят, станет результатом вызова.
    pub fn suspend(&mut self, s: Suspend) {
        self.sched.suspend = Some(s);
    }

    /// Запустить функцию скрипта параллельной задачей; возвращает ручку задачи.
    pub fn spawn(&mut self, func: Value, args: Vec<Value>) -> Result<Value, Throw> {
        if !matches!(func, Value::Func(_)) {
            return Err(Throw::msg(format!("`start` needs a script function, got {}", func.describe())));
        }
        self.sched.next_id += 1;
        let id = self.sched.next_id;
        self.sched.live.insert(id);
        self.sched.spawned.push((id, func, args));
        Ok(Value::Handle(Handle { class: TASK_CLASS, id }))
    }

    /// Остановить задачу группы (текущую — тоже).
    pub fn stop(&mut self, id: u64) {
        if id == self.sched.current {
            self.sched.kill_current = true;
        } else {
            self.sched.stopped.push(id);
            self.sched.live.remove(&id);
        }
    }

    /// Остановить все задачи группы, кроме текущей, и снять обработчики сообщений.
    pub fn stop_others(&mut self) {
        self.sched.stop_others = true;
        self.sched.handlers.clear();
    }

    /// Задача ещё работает?
    pub fn is_running(&self, id: u64) -> bool {
        self.sched.live.contains(&id)
    }

    /// Подписать обработчик на сообщение группы.
    pub fn on(&mut self, name: Rc<str>, handler: Value) {
        self.sched.handlers.push((name, handler));
    }

    /// Снять обработчики сообщения.
    pub fn off(&mut self, name: &str) {
        self.sched.handlers.retain(|(n, _)| &**n != name);
    }

    /// Отправить сообщение своей же группе (доставка в следующем кадре).
    pub fn post(&mut self, name: Rc<str>, value: Value) {
        self.sched.posts.push((name, value));
    }

    /// Id текущей задачи.
    pub fn task_id(&self) -> u64 {
        self.sched.current
    }

    /// Пользовательские данные группы (в движке — id объекта с `ScenarioRunner`).
    pub fn user(&self) -> u64 {
        self.sched.user
    }

    /// Время группы (секунды с запуска) и шаг последнего кадра.
    pub fn group_time(&self) -> (f64, f64) {
        (self.sched.time, self.sched.dt)
    }

    /// Номер класса ручек по имени.
    /// Имя класса ручки (для сообщений).
    pub fn class_name(&self, class: u32) -> String {
        self.vm.class_name(class)
    }

    pub fn class_id(&self, name: &str) -> Option<u32> {
        self.vm.class_id(name)
    }
}

/// Экземпляр скрипта: глобальные переменные и главная функция.
pub struct Instance {
    pub module: Rc<Module>,
    pub main: Value,
}

/// Виртуальная машина: встроенные функции, методы типов, классы хоста, пределы.
pub struct Vm {
    pub(crate) builtins: HashMap<Rc<str>, Value>,
    pub(crate) methods: Vec<HashMap<Rc<str>, Value>>,
    pub(crate) classes: Vec<Class>,
    pub limits: Limits,
    pub(crate) rng: Rng,
    pub(crate) printer: Printer,
    pub(crate) output: Rc<RefCell<Vec<String>>>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    /// ВМ со стандартной библиотекой.
    pub fn new() -> Self {
        let output: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = output.clone();
        let mut vm = Vm {
            builtins: HashMap::new(),
            methods: (0..TYPE_COUNT).map(|_| HashMap::new()).collect(),
            classes: Vec::new(),
            limits: Limits::default(),
            rng: Rng::new(0x5CE_A210),
            printer: Box::new(move |level, text| {
                let mut out = sink.borrow_mut();
                if out.len() < 10_000 {
                    out.push(if level == Level::Warn { format!("warning: {text}") } else { text.to_string() });
                }
            }),
            output,
        };
        vm.classes.push(Class { name: "task".into(), get: None, set: None, methods: HashMap::new() });
        crate::stdlib::install(&mut vm);
        vm
    }

    /// Куда идёт `print`/`log`/`warn`. По умолчанию — в буфер [`Vm::take_output`].
    pub fn set_printer(&mut self, f: impl FnMut(Level, &str) + 'static) {
        self.printer = Box::new(f);
    }

    /// Забрать накопленный вывод (если printer не заменён).
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.output.borrow_mut())
    }

    /// Зерно генератора случайных чисел.
    pub fn seed(&mut self, seed: u64) {
        self.rng.seed(seed);
    }

    /// Глобальная встроенная функция.
    pub fn register(&mut self, name: &str, f: impl Fn(&mut NativeCtx, Args) -> NativeResult + 'static) {
        let v = native(name, false, f);
        self.builtins.insert(name.into(), v);
    }

    /// Глобальное значение (константа, таблица функций).
    pub fn register_value(&mut self, name: &str, v: Value) {
        self.builtins.insert(name.into(), v);
    }

    /// Встроенное значение по имени.
    pub fn builtin(&self, name: &str) -> Option<&Value> {
        self.builtins.get(name)
    }

    /// Имена всех встроенных значений (для подсказок и подсветки).
    pub fn builtin_names(&self) -> Vec<Rc<str>> {
        let mut v: Vec<_> = self.builtins.keys().filter(|n| !n.starts_with("__")).cloned().collect();
        v.sort();
        v
    }

    /// Метод встроенного типа: `list.shuffle()`. Аргумент 0 — получатель.
    pub fn register_method(&mut self, ty: TypeTag, name: &str, f: impl Fn(&mut NativeCtx, Args) -> NativeResult + 'static) {
        let v = native(name, true, f);
        self.methods[ty as usize].insert(name.into(), v);
    }

    pub(crate) fn method_value(&mut self, ty: TypeTag, name: &str, v: Value) {
        self.methods[ty as usize].insert(name.into(), v);
    }

    /// Новый класс значений хоста; возвращает его номер для [`Handle`].
    pub fn register_class(&mut self, name: &str) -> u32 {
        if let Some(i) = self.class_id(name) {
            return i;
        }
        self.classes.push(Class { name: name.into(), get: None, set: None, methods: HashMap::new() });
        (self.classes.len() - 1) as u32
    }

    /// Имя класса ручки (для сообщений).
    pub fn class_name(&self, class: u32) -> String {
        self.classes.get(class as usize).map(|c| c.name.to_string()).unwrap_or_else(|| "object".into())
    }

    pub fn class_id(&self, name: &str) -> Option<u32> {
        self.classes.iter().position(|c| &*c.name == name).map(|i| i as u32)
    }

    /// Метод класса: аргумент 0 — ручка.
    pub fn class_method(&mut self, class: u32, name: &str, f: impl Fn(&mut NativeCtx, Args) -> NativeResult + 'static) {
        let v = native(name, true, f);
        if let Some(c) = self.classes.get_mut(class as usize) {
            c.methods.insert(name.into(), v);
        }
    }

    /// Чтение свойств класса (`obj.position`).
    pub fn class_getter(&mut self, class: u32, f: impl Fn(&mut NativeCtx, u64, &str) -> Result<Option<Value>, Throw> + 'static) {
        if let Some(c) = self.classes.get_mut(class as usize) {
            c.get = Some(Rc::new(f));
        }
    }

    /// Запись свойств класса (`obj.position = …`).
    pub fn class_setter(&mut self, class: u32, f: impl Fn(&mut NativeCtx, u64, &str, Value) -> Result<bool, Throw> + 'static) {
        if let Some(c) = self.classes.get_mut(class as usize) {
            c.set = Some(Rc::new(f));
        }
    }

    /// Связывает скрипт со встроенными функциями: неизвестные имена — ошибка здесь,
    /// до запуска (с подсказкой «did you mean …?»).
    pub fn instantiate(&self, script: &Rc<Script>) -> Result<Instance, ScriptError> {
        let mut slots = Vec::with_capacity(script.globals.len());
        let mut missing: Option<(Pos, &Rc<str>)> = None;
        for (name, info) in script.globals.iter().zip(&script.global_info) {
            if info.declared {
                slots.push(Value::Uninit);
            } else if let Some(v) = self.builtins.get(name) {
                slots.push(v.clone());
            } else {
                slots.push(Value::Uninit);
                let earlier = match missing {
                    None => true,
                    Some((p, _)) => (info.first_use.line, info.first_use.col) < (p.line, p.col),
                };
                if earlier {
                    missing = Some((info.first_use, name));
                }
            }
        }
        if let Some((pos, name)) = missing {
            let declared = script.globals.iter().zip(&script.global_info).filter(|(_, i)| i.declared).map(|(n, _)| &**n);
            let candidates: Vec<&str> = declared.chain(self.builtins.keys().map(|k| &**k)).collect();
            let hint = suggest(name, candidates).map(|s| format!("did you mean `{s}`?"));
            return Err(ScriptError::new(
                ErrorKind::Link,
                format!("`{name}` is not defined"),
                &script.name,
                &script.source,
                pos,
            )
            .with_hint(hint.or_else(|| Some(format!("declare it first: `let {name} = …`")))));
        }
        let module = Rc::new(Module { slots: RefCell::new(slots), names: script.globals.clone() });
        let main = Value::Func(Rc::new(Closure { proto: script.main.clone(), upvals: Box::new([]), module: module.clone() }));
        Ok(Instance { module, main })
    }

    /// Скомпилировать и выполнить скрипт целиком (с ожиданиями — шагами по `dt`, не
    /// больше `max_frames` кадров). Удобно для тестов и инструментов.
    pub fn run_source(&mut self, name: &str, source: &str) -> Result<Value, ScriptError> {
        let script = Script::compile(name, source)?;
        self.run_script(&script, &mut (), 1.0 / 60.0, 100_000)
    }

    /// Выполнить скрипт до конца в отдельной группе задач.
    pub fn run_script(&mut self, script: &Rc<Script>, host: &mut dyn Any, dt: f64, max_frames: u32) -> Result<Value, ScriptError> {
        let mut group = crate::Group::new(0);
        group.start(self, script)?;
        for _ in 0..max_frames {
            let report = group.update(self, host, dt, &mut |_, _| None);
            if let Some(e) = report.errors.into_iter().next() {
                return Err(e);
            }
            if let Some(v) = group.main_result.take() {
                group.stop_all();
                return Ok(v);
            }
            if group.is_idle() {
                break;
            }
        }
        group.stop_all();
        Ok(group.main_result.take().unwrap_or(Value::Nil))
    }
}

pub(crate) fn native(name: &str, method: bool, f: impl Fn(&mut NativeCtx, Args) -> NativeResult + 'static) -> Value {
    Value::Native(Rc::new(Native { name: name.into(), func: Box::new(f), method }))
}

