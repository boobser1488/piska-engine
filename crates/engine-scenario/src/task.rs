//! Задача — сопрограмма со своим стеком значений и кадров. Исполнение можно прервать на
//! любой инструкции (`wait`, исчерпан бюджет инструкций) и продолжить в следующем кадре.
//! Вызовы скрипта не используют стек Rust: глубокая рекурсия — ошибка, а не падение.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::{Op, Proto, UpSrc};
use crate::error::{suggest, ErrorKind, Pos, ScriptError, StackFrame};
use crate::value::*;
use crate::vm::{NativeCtx, SchedCtx, Status, Suspend, TypeTag, Vm};

struct Frame {
    closure: Rc<Closure>,
    ip: usize,
    base: usize,
}

struct Handler {
    frames: usize,
    stack: usize,
    catch_ip: usize,
}

/// Что делать после шага исполнения.
enum Flow {
    /// Сменился текущий кадр (вызов) — продолжить.
    Next,
    Return(Value),
    Suspend(Suspend),
    Preempt,
    Kill,
    Error(Throw),
}

/// Сопрограмма скрипта.
pub struct Task {
    stack: Vec<Value>,
    frames: Vec<Frame>,
    handlers: Vec<Handler>,
    pending: Option<(Rc<Closure>, usize)>,
    awaiting: bool,
    finished: bool,
}

enum Callee {
    /// Вызвать с получателем первым аргументом.
    WithSelf(Value),
    /// Вызвать без получателя (функция, лежащая в словаре под этим ключом).
    Plain(Value),
}

fn tag_of(v: &Value) -> Option<TypeTag> {
    Some(match v {
        Value::Str(_) => TypeTag::Str,
        Value::List(_) => TypeTag::List,
        Value::Map(_) => TypeTag::Map,
        Value::Num(_) => TypeTag::Num,
        Value::Vec2(..) => TypeTag::Vec2,
        Value::Vec3(..) => TypeTag::Vec3,
        Value::Color(_) => TypeTag::Color,
        Value::Func(_) | Value::Native(_) => TypeTag::Func,
        _ => return None,
    })
}

pub(crate) fn char_len(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().count()
    }
}

fn check_mem(vm: &Vm) -> Result<(), Throw> {
    if live_values() > vm.limits.max_values {
        return Err(mem_limit(vm));
    }
    Ok(())
}

fn mem_limit(vm: &Vm) -> Throw {
    Throw::Limit(format!(
        "out of script memory: lists and maps hold more than {} values (a list that grows forever?)",
        vm.limits.max_values
    ))
}

pub(crate) fn check_str(vm: &Vm, s: &str) -> Result<(), Throw> {
    if s.len() > vm.limits.max_string_len {
        return Err(Throw::Limit(format!(
            "string too long: more than {} bytes (a string that grows forever?)",
            vm.limits.max_string_len
        )));
    }
    Ok(())
}

fn type_err(verb: &str, a: &Value, b: &Value) -> Throw {
    let which = if matches!(a, Value::Num(_)) {
        format!("the right side is {}", b.describe())
    } else {
        format!("the left side is {}", a.describe())
    };
    Throw::msg(format!("can't {verb} {} and {}: {which}", a.type_name(), b.type_name()))
}

/// Арифметика для нечисловых операндов: векторы и цвета.
fn arith(a: &Value, b: &Value, op: char) -> Result<Value, Throw> {
    let f = |x: f32, y: f32| -> f32 {
        match op {
            '+' => x + y,
            '-' => x - y,
            '*' => x * y,
            _ => x / y,
        }
    };
    let verb = match op {
        '+' => "add",
        '-' => "subtract",
        '*' => "multiply",
        _ => "divide",
    };
    Ok(match (a, b) {
        (Value::Vec2(x, y), Value::Vec2(u, v)) => Value::Vec2(f(*x, *u), f(*y, *v)),
        (Value::Vec3(x, y, z), Value::Vec3(u, v, w)) => Value::Vec3(f(*x, *u), f(*y, *v), f(*z, *w)),
        (Value::Color(c), Value::Color(d)) => Value::color(f(c[0], d[0]), f(c[1], d[1]), f(c[2], d[2]), f(c[3], d[3])),
        (Value::Vec2(x, y), Value::Num(n)) if op == '*' || op == '/' => Value::Vec2(f(*x, *n as f32), f(*y, *n as f32)),
        (Value::Vec3(x, y, z), Value::Num(n)) if op == '*' || op == '/' => {
            Value::Vec3(f(*x, *n as f32), f(*y, *n as f32), f(*z, *n as f32))
        }
        (Value::Num(n), Value::Vec2(x, y)) if op == '*' => Value::Vec2(*x * *n as f32, *y * *n as f32),
        (Value::Num(n), Value::Vec3(x, y, z)) if op == '*' => Value::Vec3(*x * *n as f32, *y * *n as f32, *z * *n as f32),
        (Value::Color(c), Value::Num(n)) if op == '*' || op == '/' => {
            let n = *n as f32;
            Value::color(f(c[0], n), f(c[1], n), f(c[2], n), c[3])
        }
        (Value::Num(n), Value::Color(c)) if op == '*' => {
            let n = *n as f32;
            Value::color(c[0] * n, c[1] * n, c[2] * n, c[3])
        }
        _ => return Err(type_err(verb, a, b)),
    })
}

fn of_what(note: &Option<Rc<str>>) -> String {
    note.as_ref().map(|n| format!(" (`{n}`)")).unwrap_or_default()
}

fn throw_text(t: &Throw) -> String {
    match t {
        Throw::Msg(m) | Throw::Limit(m) => m.clone(),
        Throw::Value(Value::Str(s)) => s.to_string(),
        Throw::Value(v) => format!("uncaught error: {}", v.to_text()),
    }
}

fn index_of(key: &Value, len: usize, what: &str) -> Result<Option<usize>, Throw> {
    match key {
        Value::Num(n) => {
            if n.fract() != 0.0 {
                return Err(Throw::msg(format!("{what} index must be a whole number, got {}", fmt_num(*n))));
            }
            if *n < 0.0 || *n >= len as f64 {
                return Ok(None);
            }
            Ok(Some(*n as usize))
        }
        other => Err(Throw::msg(format!("{what} index must be a number, got {}", other.describe()))),
    }
}

impl Task {
    /// Новая задача, которая вызовет `func(args…)` при первом возобновлении.
    pub fn new(func: &Value, args: Vec<Value>) -> Result<Task, String> {
        let Value::Func(c) = func else {
            return Err(format!("a task needs a script function, got {}", func.describe()));
        };
        let argc = args.len();
        let mut stack = Vec::with_capacity(32);
        stack.push(func.clone());
        stack.extend(args);
        Ok(Task { stack, frames: Vec::new(), handlers: Vec::new(), pending: Some((c.clone(), argc)), awaiting: false, finished: false })
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Ошибка предела с текущим местом задачи (для «работает слишком долго»).
    pub(crate) fn limit_error(&self, msg: &str) -> ScriptError {
        self.build_error(&Throw::Limit(msg.into()))
    }

    /// Продолжить выполнение. `input` — результат ожидания, на котором задача стояла.
    pub fn resume(&mut self, vm: &mut Vm, host: &mut dyn Any, sched: &mut SchedCtx, input: Value) -> Result<Status, ScriptError> {
        if self.finished {
            return Ok(Status::Done(Value::Nil));
        }
        if let Some((c, argc)) = self.pending.take() {
            if let Err(t) = self.enter(vm, c, argc) {
                self.finished = true;
                let proto_file: Rc<str> = "<task>".into();
                return Err(ScriptError::new(ErrorKind::Limit, throw_text(&t), &proto_file, "", Pos::new(0, 0)));
            }
        } else if self.awaiting {
            self.awaiting = false;
            self.stack.push(input);
        }
        let mut budget = vm.limits.instructions_per_resume;
        loop {
            match self.exec(vm, host, sched, &mut budget) {
                Flow::Next => {}
                Flow::Return(v) => {
                    let Some(frame) = self.frames.pop() else {
                        self.finished = true;
                        return Ok(Status::Done(v));
                    };
                    self.stack.truncate(frame.base.saturating_sub(1));
                    let depth = self.frames.len();
                    while self.handlers.last().is_some_and(|h| h.frames > depth) {
                        self.handlers.pop();
                    }
                    if self.frames.is_empty() {
                        self.finished = true;
                        self.stack.clear();
                        return Ok(Status::Done(v));
                    }
                    self.stack.push(v);
                }
                Flow::Suspend(s) => return Ok(Status::Suspended(s)),
                Flow::Preempt => return Ok(Status::Preempted),
                Flow::Kill => {
                    self.finished = true;
                    self.stack.clear();
                    self.frames.clear();
                    return Ok(Status::Done(Value::Nil));
                }
                Flow::Error(t) => self.handle_error(t)?,
            }
        }
    }

    #[inline]
    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or_default()
    }

    fn save_ip(&mut self, ip: usize) {
        if let Some(f) = self.frames.last_mut() {
            f.ip = ip;
        }
    }

    /// Входит в функцию скрипта: вызываемое на `len - argc - 1`, аргументы над ним.
    fn enter(&mut self, vm: &Vm, closure: Rc<Closure>, argc: usize) -> Result<(), Throw> {
        if self.frames.len() >= vm.limits.max_call_depth {
            return Err(Throw::Limit(format!(
                "too much recursion: more than {} nested calls (does a function call itself forever?)",
                vm.limits.max_call_depth
            )));
        }
        let proto = &closure.proto;
        let base = self.stack.len() - argc;
        let nparams = proto.nparams as usize;
        if proto.rest {
            let fixed = nparams - 1;
            let extras = if argc > fixed { self.stack.split_off(base + fixed) } else { Vec::new() };
            while self.stack.len() < base + fixed {
                self.stack.push(Value::Nil);
            }
            self.stack.push(Value::list(extras));
        } else {
            if argc > nparams {
                self.stack.truncate(base + nparams);
            }
            while self.stack.len() < base + nparams {
                self.stack.push(Value::Nil);
            }
        }
        self.stack.resize(base + proto.nslots as usize, Value::Nil);
        self.frames.push(Frame { closure, ip: 0, base });
        Ok(())
    }

    /// Вызов значения: функции скрипта или встроенной. Аргументы — с `args_start` до
    /// вершины, результат ляжет на место `result_at`.
    #[allow(clippy::too_many_arguments)]
    fn call_value(
        &mut self,
        vm: &mut Vm,
        host: &mut dyn Any,
        sched: &mut SchedCtx,
        callee: Value,
        args_start: usize,
        result_at: usize,
        note: Option<Rc<str>>,
    ) -> Flow {
        match callee {
            Value::Func(c) => {
                // Вызываемое лежит на `result_at`, аргументы — сразу над ним.
                debug_assert_eq!(args_start, result_at + 1);
                let argc = self.stack.len() - args_start;
                match self.enter(vm, c, argc) {
                    Ok(()) => Flow::Next,
                    Err(t) => Flow::Error(t),
                }
            }
            Value::Native(nat) => {
                let res = {
                    let mut ctx = NativeCtx { vm, host, sched };
                    let args = Args { values: &self.stack[args_start..], name: &nat.name, offset: usize::from(nat.method) };
                    (nat.func)(&mut ctx, args)
                };
                self.stack.truncate(result_at);
                let v = match res {
                    Ok(v) => v,
                    Err(t) => return Flow::Error(t),
                };
                if sched.kill_current {
                    sched.kill_current = false;
                    sched.suspend = None;
                    return Flow::Kill;
                }
                if live_values() > vm.limits.max_values {
                    return Flow::Error(mem_limit(vm));
                }
                if let Some(s) = sched.suspend.take() {
                    self.awaiting = true;
                    return Flow::Suspend(s);
                }
                self.stack.push(v);
                Flow::Next
            }
            other => {
                let what = match &note {
                    Some(n) => format!("tried to call `{n}`, but it is {}", other.describe()),
                    None => format!("tried to call {}, which is not a function", other.describe()),
                };
                Flow::Error(Throw::msg(what))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_method(
        &mut self,
        vm: &mut Vm,
        host: &mut dyn Any,
        sched: &mut SchedCtx,
        name: &Value,
        at: usize,
        colon: bool,
        note: Option<Rc<str>>,
    ) -> Flow {
        let Value::Str(name) = name else {
            return Flow::Error(Throw::msg("internal error: method name"));
        };
        let recv = self.stack[at].clone();
        if recv.is_nil() {
            return Flow::Error(Throw::msg(format!(
                "tried to call `{}{}()` on nil{}",
                if colon { ":" } else { "." },
                name,
                of_what(&note).replace(" (`", " — `").replace("`)", "` is nil")
            )));
        }
        let Some(callee) = self.find_method(vm, &recv, name, colon) else {
            let names = self.method_names(vm, &recv);
            let hint = suggest(name, names.iter().map(|n| &**n)).map(|s| format!("\nhint: did you mean `{s}`?"));
            let what = match &recv {
                Value::Map(_) => format!("the map{} has no key or method `{name}`", of_what(&note)),
                Value::Handle(h) => {
                    let class = vm.class_name(h.class);
                    format!("{class}{} has no method `{name}`", of_what(&note))
                }
                other => format!("{}{} has no method `{name}`", other.type_name(), of_what(&note)),
            };
            return Flow::Error(Throw::msg(format!("{what}{}", hint.unwrap_or_default())));
        };
        let desc: Option<Rc<str>> = Some(match &note {
            Some(n) => format!("{n}.{name}").into(),
            None => name.clone(),
        });
        match callee {
            Callee::WithSelf(f) => {
                if let Value::Func(_) = f {
                    // Функция скрипта: вставить вызываемое под получателем.
                    self.stack.insert(at, f.clone());
                    self.call_value(vm, host, sched, f, at + 1, at, desc)
                } else {
                    self.call_value(vm, host, sched, f, at, at, desc)
                }
            }
            Callee::Plain(f) => {
                self.stack[at] = f.clone();
                self.call_value(vm, host, sched, f, at + 1, at, desc)
            }
        }
    }

    fn find_method(&self, vm: &Vm, recv: &Value, name: &str, colon: bool) -> Option<Callee> {
        match recv {
            Value::Map(m) => {
                if let Some(v) = m.borrow().get_str(name) {
                    return Some(if colon { Callee::WithSelf(v.clone()) } else { Callee::Plain(v.clone()) });
                }
                vm.methods[TypeTag::Map as usize].get(name).map(|v| Callee::WithSelf(v.clone()))
            }
            Value::Handle(h) => vm.classes.get(h.class as usize).and_then(|c| c.methods.get(name)).map(|v| Callee::WithSelf(v.clone())),
            other => {
                let tag = tag_of(other)?;
                vm.methods[tag as usize].get(name).map(|v| Callee::WithSelf(v.clone()))
            }
        }
    }

    fn method_names(&self, vm: &Vm, recv: &Value) -> Vec<Rc<str>> {
        let mut names: Vec<Rc<str>> = match recv {
            Value::Handle(h) => vm.classes.get(h.class as usize).map(|c| c.methods.keys().cloned().collect()).unwrap_or_default(),
            other => tag_of(other).map(|t| vm.methods[t as usize].keys().cloned().collect()).unwrap_or_default(),
        };
        if let Value::Map(m) = recv {
            for (k, _) in m.borrow().iter() {
                if let Key::Str(s) = k {
                    names.push(s.clone());
                }
            }
        }
        names
    }

    fn get_field(&mut self, vm: &mut Vm, host: &mut dyn Any, sched: &mut SchedCtx, obj: &Value, name: &Rc<str>, note: Option<Rc<str>>) -> Result<Value, Throw> {
        Ok(match obj {
            Value::Map(m) => m.borrow().get(&Key::Str(name.clone())).cloned().unwrap_or_default(),
            Value::List(l) if &**name == "length" => Value::Num(l.len() as f64),
            Value::Str(s) if &**name == "length" => Value::Num(char_len(s) as f64),
            Value::Vec2(x, y) => match &**name {
                "x" => Value::Num(clean(*x)),
                "y" => Value::Num(clean(*y)),
                _ => return Err(no_field(obj, name, &note, &["x", "y"])),
            },
            Value::Vec3(x, y, z) => match &**name {
                "x" => Value::Num(clean(*x)),
                "y" => Value::Num(clean(*y)),
                "z" => Value::Num(clean(*z)),
                _ => return Err(no_field(obj, name, &note, &["x", "y", "z"])),
            },
            Value::Color(c) => match &**name {
                "r" => Value::Num(clean(c[0])),
                "g" => Value::Num(clean(c[1])),
                "b" => Value::Num(clean(c[2])),
                "a" => Value::Num(clean(c[3])),
                _ => return Err(no_field(obj, name, &note, &["r", "g", "b", "a"])),
            },
            Value::Handle(h) if h.class == TASK_CLASS => match &**name {
                "running" => Value::Bool(sched.live.contains(&{ h.id })),
                "done" => Value::Bool(!sched.live.contains(&{ h.id })),
                "id" => Value::Num(h.id as f64),
                _ => return Err(no_field(obj, name, &note, &["running", "done", "id"])),
            },
            Value::Handle(h) => {
                let getter = vm.classes.get(h.class as usize).and_then(|c| c.get.clone());
                let id = h.id;
                let found = match getter {
                    Some(g) => {
                        let mut ctx = NativeCtx { vm, host, sched };
                        g(&mut ctx, id, name)?
                    }
                    None => None,
                };
                match found {
                    Some(v) => v,
                    None => {
                        let class = vm.class_name(h.class);
                        return Err(Throw::msg(format!("{class}{} has no property `{name}`", of_what(&note))));
                    }
                }
            }
            Value::Nil | Value::Uninit => {
                let who = note.as_ref().map(|n| format!(" — `{n}` is nil")).unwrap_or_default();
                return Err(Throw::msg(format!("tried to read `.{name}` of nil{who}")));
            }
            other => return Err(no_field(other, name, &note, &[])),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn get_index(&mut self, vm: &mut Vm, host: &mut dyn Any, sched: &mut SchedCtx, obj: &Value, key: &Value, note: Option<Rc<str>>) -> Result<Value, Throw> {
        match obj {
            Value::List(l) => Ok(match index_of(key, l.len(), "a list")? {
                Some(i) => l.get(i).unwrap_or_default(),
                None => Value::Nil,
            }),
            Value::Map(m) => {
                let k = Key::from_value(key).map_err(Throw::Msg)?;
                Ok(m.borrow().get(&k).cloned().unwrap_or_default())
            }
            Value::Str(s) => {
                if let Value::Str(name) = key {
                    return self.get_field(vm, host, sched, obj, name, note);
                }
                let n = char_len(s);
                Ok(match index_of(key, n, "a string")? {
                    Some(i) => Value::Str(s.chars().nth(i).map(|c| c.to_string()).unwrap_or_default().into()),
                    None => Value::Nil,
                })
            }
            Value::Nil | Value::Uninit => {
                let who = note.as_ref().map(|n| format!(" — `{n}` is nil")).unwrap_or_default();
                Err(Throw::msg(format!("tried to index nil with {}{who}", key.describe())))
            }
            other => match key {
                Value::Str(name) => self.get_field(vm, host, sched, other, name, note),
                _ => Err(Throw::msg(format!("can't index {}{} with {}", other.describe(), of_what(&note), key.describe()))),
            },
        }
    }

    /// Запись `obj[key] = val`. Для значений-векторов возвращает новое значение.
    #[allow(clippy::too_many_arguments)]
    fn set_index(&mut self, vm: &mut Vm, host: &mut dyn Any, sched: &mut SchedCtx, obj: &Value, key: &Value, val: Value, note: Option<Rc<str>>) -> Result<Option<Value>, Throw> {
        let num = |v: &Value, what: &str| -> Result<f32, Throw> {
            match v {
                Value::Num(n) => Ok(*n as f32),
                other => Err(Throw::msg(format!("`.{what}` must be a number, got {}", other.describe()))),
            }
        };
        let field = |k: &Value| -> String {
            match k {
                Value::Str(s) => s.to_string(),
                other => other.to_text(),
            }
        };
        match obj {
            Value::Map(m) => {
                let k = Key::from_value(key).map_err(Throw::Msg)?;
                if val.is_nil() {
                    m.borrow_mut().remove(&k);
                } else {
                    m.borrow_mut().insert(k, val);
                }
                Ok(None)
            }
            Value::List(l) => {
                let len = l.len();
                let i = match key {
                    Value::Num(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= len as f64 => *n as usize,
                    Value::Num(n) => {
                        return Err(Throw::msg(format!(
                            "index {} is out of range for a list of length {len} (use `push` to add items)",
                            fmt_num(*n)
                        )))
                    }
                    other => return Err(Throw::msg(format!("a list index must be a number, got {}", other.describe()))),
                };
                l.set(i, val);
                Ok(None)
            }
            Value::Vec2(x, y) => {
                let (mut x, mut y) = (*x, *y);
                match &*field(key) {
                    "x" => x = num(&val, "x")?,
                    "y" => y = num(&val, "y")?,
                    f => return Err(no_field(obj, f, &note, &["x", "y"])),
                }
                Ok(Some(Value::Vec2(x, y)))
            }
            Value::Vec3(x, y, z) => {
                let (mut x, mut y, mut z) = (*x, *y, *z);
                match &*field(key) {
                    "x" => x = num(&val, "x")?,
                    "y" => y = num(&val, "y")?,
                    "z" => z = num(&val, "z")?,
                    f => return Err(no_field(obj, f, &note, &["x", "y", "z"])),
                }
                Ok(Some(Value::Vec3(x, y, z)))
            }
            Value::Color(c) => {
                let mut c = **c;
                let f = field(key);
                let i = match &*f {
                    "r" => 0,
                    "g" => 1,
                    "b" => 2,
                    "a" => 3,
                    other => return Err(no_field(obj, other, &note, &["r", "g", "b", "a"])),
                };
                c[i] = num(&val, &f)?;
                Ok(Some(Value::Color(Rc::new(c))))
            }
            Value::Handle(h) if h.class != TASK_CLASS => {
                let setter = vm.classes.get(h.class as usize).and_then(|c| c.set.clone());
                let name = field(key);
                let ok = match setter {
                    Some(s) => {
                        let mut ctx = NativeCtx { vm, host, sched };
                        s(&mut ctx, h.id, &name, val)?
                    }
                    None => false,
                };
                if !ok {
                    let class = vm.class_name(h.class);
                    return Err(Throw::msg(format!("{class}{} has no property `{name}` that can be set", of_what(&note))));
                }
                Ok(None)
            }
            Value::Nil | Value::Uninit => {
                let who = note.as_ref().map(|n| format!(" — `{n}` is nil")).unwrap_or_default();
                Err(Throw::msg(format!("tried to set `{}` on nil{who}", field(key))))
            }
            Value::Str(_) => Err(Throw::msg("strings can't be changed in place — build a new string instead")),
            other => Err(Throw::msg(format!("can't set `{}` on {}{}", field(key), other.describe(), of_what(&note)))),
        }
    }

    fn after_set(&mut self, new: Option<Value>, skip: u8, ip: &mut usize) {
        match new {
            Some(v) if skip > 0 => self.stack.push(v),
            _ => *ip += skip as usize,
        }
    }

    /// Ошибка: перехват `try`, иначе — [`ScriptError`] с местом и стеком.
    fn handle_error(&mut self, t: Throw) -> Result<(), ScriptError> {
        if !matches!(t, Throw::Limit(_)) {
            if let Some(h) = self.handlers.pop() {
                if h.frames <= self.frames.len() && h.frames > 0 {
                    self.frames.truncate(h.frames);
                    self.stack.truncate(h.stack);
                    let v = match t {
                        Throw::Value(v) => v,
                        Throw::Msg(m) => Value::Str(m.split("\nhint: ").next().unwrap_or("").into()),
                        Throw::Limit(m) => Value::Str(m.into()),
                    };
                    self.stack.push(v);
                    self.awaiting = false;
                    if let Some(f) = self.frames.last_mut() {
                        f.ip = h.catch_ip;
                    }
                    return Ok(());
                }
            }
        }
        self.finished = true;
        Err(self.build_error(&t))
    }

    fn build_error(&self, t: &Throw) -> ScriptError {
        let text = throw_text(t);
        let (message, hint) = match text.split_once("\nhint: ") {
            Some((m, h)) => (m.to_string(), Some(h.to_string())),
            None => (text, None),
        };
        let kind = if matches!(t, Throw::Limit(_)) { ErrorKind::Limit } else { ErrorKind::Runtime };
        let user: Vec<(&Proto, Pos)> = self
            .frames
            .iter()
            .rev()
            .filter(|f| !f.closure.proto.internal)
            .map(|f| (&*f.closure.proto, f.closure.proto.pos_at(f.ip.saturating_sub(1))))
            .collect();
        let Some(&(top, pos)) = user.first() else {
            let file: Rc<str> = "<scenario>".into();
            return ScriptError::new(kind, message, &file, "", Pos::new(0, 0)).with_hint(hint);
        };
        let mut e = ScriptError::new(kind, message, &top.file, &top.source, pos).with_hint(hint);
        if user.len() > 1 || &*top.name != "<main>" {
            for (p, at) in user.iter().take(10) {
                e.stack.push(StackFrame { function: p.name.to_string(), pos: *at });
            }
        }
        e
    }

    /// Исполняет текущий кадр до вызова, возврата, приостановки или ошибки.
    fn exec(&mut self, vm: &mut Vm, host: &mut dyn Any, sched: &mut SchedCtx, budget_out: &mut u32) -> Flow {
        // Счётчик в локальной переменной (в регистре), обратно — при любом выходе.
        let mut budget = Budget { left: *budget_out, out: budget_out };
        let Some(frame) = self.frames.last() else {
            return Flow::Return(Value::Nil);
        };
        let closure = frame.closure.clone();
        let base = frame.base;
        let mut ip = frame.ip;
        let proto = &*closure.proto;
        let code = &proto.code[..];
        let note = |ip: usize| proto.notes.get(&(ip as u32 - 1)).cloned();

        macro_rules! tri {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(t) => {
                        self.save_ip(ip);
                        return Flow::Error(t);
                    }
                }
            };
        }
        macro_rules! fail {
            ($t:expr) => {{
                self.save_ip(ip);
                return Flow::Error($t);
            }};
        }
        macro_rules! jump {
            ($o:expr) => {
                ip = (ip as i64 + $o as i64) as usize
            };
        }
        macro_rules! arith_op {
            ($op:tt, $c:literal) => {{
                let b = self.pop();
                let a = self.pop();
                let v = match (&a, &b) {
                    (Value::Num(x), Value::Num(y)) => Value::Num(x $op y),
                    _ => tri!(arith(&a, &b, $c)),
                };
                self.stack.push(v);
            }};
        }

        loop {
            if budget.left == 0 {
                self.save_ip(ip);
                return Flow::Preempt;
            }
            budget.left -= 1;
            let Some(&op) = code.get(ip) else {
                fail!(Throw::msg("internal error: ran past the end of a function"));
            };
            ip += 1;
            match op {
                Op::Nil => self.stack.push(Value::Nil),
                Op::True => self.stack.push(Value::Bool(true)),
                Op::False => self.stack.push(Value::Bool(false)),
                Op::Const(k) => self.stack.push(proto.consts[k as usize].clone()),
                Op::Int(i) => self.stack.push(Value::Num(i as f64)),
                Op::Pop => {
                    self.stack.pop();
                }
                Op::Dup => {
                    let v = self.stack.last().cloned().unwrap_or_default();
                    self.stack.push(v);
                }
                Op::Dup2 => {
                    let n = self.stack.len();
                    let (a, b) = (self.stack[n - 2].clone(), self.stack[n - 1].clone());
                    self.stack.push(a);
                    self.stack.push(b);
                }
                Op::GetLocal(s) => {
                    let v = self.stack[base + s as usize].clone();
                    self.stack.push(v);
                }
                Op::SetLocal(s) => {
                    let v = self.pop();
                    self.stack[base + s as usize] = v;
                }
                Op::NewCell(s) => {
                    self.stack[base + s as usize] = Value::Cell(Rc::new(RefCell::new(Value::Nil)));
                }
                Op::GetCell(s) => {
                    let v = match &self.stack[base + s as usize] {
                        Value::Cell(c) => c.borrow().clone(),
                        other => other.clone(),
                    };
                    self.stack.push(v);
                }
                Op::SetCell(s) => {
                    let v = self.pop();
                    match &self.stack[base + s as usize] {
                        Value::Cell(c) => *c.borrow_mut() = v,
                        _ => self.stack[base + s as usize] = v,
                    }
                }
                Op::GetUp(i) => {
                    let v = closure.upvals[i as usize].borrow().clone();
                    self.stack.push(v);
                }
                Op::SetUp(i) => {
                    let v = self.pop();
                    *closure.upvals[i as usize].borrow_mut() = v;
                }
                Op::GetGlobal(i) => {
                    let v = closure.module.slots.borrow().get(i as usize).cloned();
                    match v {
                        Some(Value::Uninit) => {
                            let name = closure.module.names.get(i as usize).cloned().unwrap_or_else(|| "?".into());
                            fail!(Throw::msg(format!("`{name}` is used before its declaration")));
                        }
                        Some(v) => self.stack.push(v),
                        None => fail!(Throw::msg("this script was unloaded")),
                    }
                }
                Op::SetGlobal(i) => {
                    let v = self.pop();
                    if let Some(slot) = closure.module.slots.borrow_mut().get_mut(i as usize) {
                        *slot = v;
                    }
                }
                Op::NewList(n) => {
                    let items = self.stack.split_off(self.stack.len() - n as usize);
                    self.stack.push(Value::list(items));
                    tri!(check_mem(vm));
                }
                Op::ListPush => {
                    let v = self.pop();
                    if let Some(Value::List(l)) = self.stack.last() {
                        l.push(v);
                    }
                    tri!(check_mem(vm));
                }
                Op::ListExtend => {
                    let v = self.pop();
                    let items: Vec<Value> = match &v {
                        Value::List(o) => o.to_vec(),
                        Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string().into())).collect(),
                        other => fail!(Throw::msg(format!("`...` can only spread a list or a string, not {}", other.describe()))),
                    };
                    if let Some(Value::List(l)) = self.stack.last() {
                        for it in items {
                            l.push(it);
                        }
                    }
                    tri!(check_mem(vm));
                }
                Op::NewMap(n) => {
                    let pairs = self.stack.split_off(self.stack.len() - 2 * n as usize);
                    let mut m = OrderedMap::default();
                    let mut it = pairs.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        let key = tri!(Key::from_value(&k).map_err(Throw::Msg));
                        if !v.is_nil() {
                            m.insert(key, v);
                        }
                    }
                    self.stack.push(Value::Map(Map::new(m)));
                    tri!(check_mem(vm));
                }
                Op::GetIndex => {
                    let key = self.pop();
                    let obj = self.pop();
                    let v = tri!(self.get_index(vm, host, sched, &obj, &key, note(ip)));
                    self.stack.push(v);
                }
                Op::GetField(k) => {
                    let obj = self.pop();
                    // Быстрый путь: поле словаря.
                    if let (Value::Map(m), Value::Str(name)) = (&obj, &proto.consts[k as usize]) {
                        let v = m.borrow().get(&Key::Str(name.clone())).cloned().unwrap_or_default();
                        self.stack.push(v);
                        continue;
                    }
                    let Value::Str(name) = &proto.consts[k as usize] else {
                        fail!(Throw::msg("internal error: field name"));
                    };
                    let v = tri!(self.get_field(vm, host, sched, &obj, name, note(ip)));
                    self.stack.push(v);
                }
                Op::SetIndex(skip) => {
                    let val = self.pop();
                    let key = self.pop();
                    let obj = self.pop();
                    let new = tri!(self.set_index(vm, host, sched, &obj, &key, val, note(ip)));
                    self.after_set(new, skip, &mut ip);
                    tri!(check_mem(vm));
                }
                Op::SetField(k, skip) => {
                    let val = self.pop();
                    let obj = self.pop();
                    let key = proto.consts[k as usize].clone();
                    let new = tri!(self.set_index(vm, host, sched, &obj, &key, val, note(ip)));
                    self.after_set(new, skip, &mut ip);
                    tri!(check_mem(vm));
                }
                Op::Add => {
                    let b = self.pop();
                    let a = self.pop();
                    let v = match (&a, &b) {
                        (Value::Num(x), Value::Num(y)) => Value::Num(x + y),
                        (Value::Str(_), _) | (_, Value::Str(_)) => {
                            let mut s = a.to_text();
                            s.push_str(&b.to_text());
                            tri!(check_str(vm, &s));
                            Value::Str(s.into())
                        }
                        _ => tri!(arith(&a, &b, '+')),
                    };
                    self.stack.push(v);
                }
                Op::Sub => arith_op!(-, '-'),
                Op::Mul => arith_op!(*, '*'),
                Op::Div => arith_op!(/, '/'),
                Op::Mod => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (Value::Num(x), Value::Num(y)) => self.stack.push(Value::Num(x - (x / y).floor() * y)),
                        _ => fail!(type_err("take the remainder of", &a, &b)),
                    }
                }
                Op::Pow => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (Value::Num(x), Value::Num(y)) => self.stack.push(Value::Num(x.powf(*y))),
                        _ => fail!(type_err("raise to a power", &a, &b)),
                    }
                }
                Op::Concat => {
                    let b = self.pop();
                    let a = self.pop();
                    let mut s = a.to_text();
                    s.push_str(&b.to_text());
                    tri!(check_str(vm, &s));
                    self.stack.push(Value::Str(s.into()));
                }
                Op::Neg => {
                    let a = self.pop();
                    let v = match a {
                        Value::Num(x) => Value::Num(-x),
                        Value::Vec2(x, y) => Value::Vec2(-x, -y),
                        Value::Vec3(x, y, z) => Value::Vec3(-x, -y, -z),
                        other => fail!(Throw::msg(format!("can't negate {}", other.describe()))),
                    };
                    self.stack.push(v);
                }
                Op::Not => {
                    let a = self.pop();
                    self.stack.push(Value::Bool(!a.truthy()));
                }
                Op::Len => {
                    let a = self.pop();
                    let n = match &a {
                        Value::List(l) => l.len(),
                        Value::Str(s) => char_len(s),
                        Value::Map(m) => m.borrow().len(),
                        other => fail!(Throw::msg(format!(
                            "can't take the length of {}{} — `#` works on lists, strings and maps",
                            other.describe(),
                            of_what(&note(ip))
                        ))),
                    };
                    self.stack.push(Value::Num(n as f64));
                }
                Op::Typeof => {
                    let a = self.pop();
                    self.stack.push(Value::Str(a.type_name().into()));
                }
                Op::Eq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(a.equals(&b)));
                }
                Op::Ne => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(!a.equals(&b)));
                }
                Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                    let b = self.pop();
                    let a = self.pop();
                    if let (Value::Num(x), Value::Num(y)) = (&a, &b) {
                        let r = match op {
                            Op::Lt => x < y,
                            Op::Le => x <= y,
                            Op::Gt => x > y,
                            _ => x >= y,
                        };
                        self.stack.push(Value::Bool(r));
                        continue;
                    }
                    let ord = match (&a, &b) {
                        (Value::Num(x), Value::Num(y)) => x.partial_cmp(y),
                        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
                        _ => fail!(Throw::msg(format!("can't compare {} with {}", a.describe(), b.describe()))),
                    };
                    use std::cmp::Ordering::*;
                    let r = match (op, ord) {
                        (_, None) => false,
                        (Op::Lt, Some(o)) => o == Less,
                        (Op::Le, Some(o)) => o != Greater,
                        (Op::Gt, Some(o)) => o == Greater,
                        (_, Some(o)) => o != Less,
                    };
                    self.stack.push(Value::Bool(r));
                }
                Op::Jump(o) => jump!(o),
                Op::JumpIfFalse(o) => {
                    let v = self.pop();
                    if !v.truthy() {
                        jump!(o);
                    }
                }
                Op::AndJump(o) => {
                    if !self.stack.last().map(Value::truthy).unwrap_or(false) {
                        jump!(o);
                    } else {
                        self.stack.pop();
                    }
                }
                Op::OrJump(o) => {
                    if self.stack.last().map(Value::truthy).unwrap_or(false) {
                        jump!(o);
                    } else {
                        self.stack.pop();
                    }
                }
                Op::NilJump(o) => {
                    if !self.stack.last().map(Value::is_nil).unwrap_or(true) {
                        jump!(o);
                    } else {
                        self.stack.pop();
                    }
                }
                Op::Call(n) => {
                    let at = self.stack.len() - n as usize - 1;
                    let callee = self.stack[at].clone();
                    self.save_ip(ip);
                    return self.call_value(vm, host, sched, callee, at + 1, at, note(ip));
                }
                Op::CallList => {
                    let args = self.pop();
                    let Value::List(l) = args else {
                        fail!(Throw::msg("internal error: spread call"));
                    };
                    let at = self.stack.len() - 1;
                    self.stack.extend(l.to_vec());
                    let callee = self.stack[at].clone();
                    self.save_ip(ip);
                    return self.call_value(vm, host, sched, callee, at + 1, at, note(ip));
                }
                Op::Method(k, n, colon) => {
                    let at = self.stack.len() - n as usize - 1;
                    self.save_ip(ip);
                    return self.call_method(vm, host, sched, &proto.consts[k as usize], at, colon, note(ip));
                }
                Op::MethodList(k, colon) => {
                    let args = self.pop();
                    let Value::List(l) = args else {
                        fail!(Throw::msg("internal error: spread call"));
                    };
                    let at = self.stack.len() - 1;
                    self.stack.extend(l.to_vec());
                    self.save_ip(ip);
                    return self.call_method(vm, host, sched, &proto.consts[k as usize], at, colon, note(ip));
                }
                Op::Closure(i) => {
                    let child = proto.protos[i as usize].clone();
                    let mut ups = Vec::with_capacity(child.upvals.len());
                    for src in &child.upvals {
                        ups.push(match *src {
                            UpSrc::Local(s) => {
                                let slot = &mut self.stack[base + s as usize];
                                match slot {
                                    Value::Cell(c) => c.clone(),
                                    other => {
                                        let c = Rc::new(RefCell::new(std::mem::take(other)));
                                        *other = Value::Cell(c.clone());
                                        c
                                    }
                                }
                            }
                            UpSrc::Up(j) => closure.upvals[j as usize].clone(),
                        });
                    }
                    self.stack.push(Value::Func(Rc::new(Closure {
                        proto: child,
                        upvals: ups.into_boxed_slice(),
                        module: closure.module.clone(),
                    })));
                }
                Op::Return => {
                    let v = self.pop();
                    self.save_ip(ip);
                    return Flow::Return(v);
                }
                Op::ReturnNil => {
                    self.save_ip(ip);
                    return Flow::Return(Value::Nil);
                }
                Op::IterPrep(mode) => {
                    let coll = self.pop();
                    let src = match coll {
                        Value::List(l) => IterSrc::List(l),
                        Value::Map(m) => IterSrc::Map(m),
                        Value::Str(s) => IterSrc::Str(s.chars().collect()),
                        other => fail!(Throw::msg(format!(
                            "can't loop over {}{} — `for` works on lists, maps and strings",
                            other.describe(),
                            of_what(&note(ip))
                        ))),
                    };
                    self.stack.push(Value::Iter(Rc::new(RefCell::new(IterState { src, pos: 0, of: mode == 0 }))));
                }
                Op::IterNext(slot, n) => {
                    let Value::Iter(state) = self.stack[base + slot as usize].clone() else {
                        fail!(Throw::msg("internal error: iterator"));
                    };
                    let mut st = state.borrow_mut();
                    let of = st.of;
                    let pos = st.pos;
                    let mut skipped = 0;
                    let item: Option<(Value, Value)> = match &st.src {
                        IterSrc::List(l) => l.get(pos).map(|v| (Value::Num(pos as f64), v)),
                        IterSrc::Str(c) => c.get(pos).map(|ch| (Value::Num(pos as f64), Value::Str(ch.to_string().into()))),
                        IterSrc::Map(m) => {
                            let m = m.borrow();
                            let mut found = None;
                            while let Some(e) = m.slot(pos + skipped) {
                                if let Some((k, v)) = e {
                                    found = Some((k.to_value(), v.clone()));
                                    break;
                                }
                                skipped += 1;
                            }
                            found
                        }
                    };
                    st.pos += skipped;
                    if let Some((k, v)) = item {
                        st.pos += 1;
                        let is_map = matches!(st.src, IterSrc::Map(_));
                        drop(st);
                        if of {
                            if is_map {
                                self.stack.push(Value::list(vec![k, v]));
                            } else {
                                self.stack.push(v);
                            }
                        } else {
                            self.stack.push(k);
                            if n >= 2 {
                                self.stack.push(v);
                            }
                        }
                        ip += 1;
                    }
                }
                Op::NumForCheck(s) => {
                    let s = base + s as usize;
                    let (Value::Num(i), Value::Num(limit), Value::Num(step)) = (&self.stack[s], &self.stack[s + 1], &self.stack[s + 2]) else {
                        fail!(Throw::msg(format!(
                            "a numeric `for` needs numbers: start {}, end {}, step {}",
                            self.stack[s].describe(),
                            self.stack[s + 1].describe(),
                            self.stack[s + 2].describe()
                        )));
                    };
                    if *step == 0.0 {
                        fail!(Throw::msg("the step of a numeric `for` can't be 0"));
                    }
                    if if *step > 0.0 { i <= limit } else { i >= limit } {
                        ip += 1;
                    }
                }
                Op::NumForStep(s) => {
                    let s = base + s as usize;
                    if let (Value::Num(i), Value::Num(step)) = (&self.stack[s], &self.stack[s + 2]) {
                        self.stack[s] = Value::Num(i + step);
                    }
                }
                Op::Template(n) => {
                    let parts = self.stack.split_off(self.stack.len() - n as usize);
                    let mut s = String::new();
                    for p in &parts {
                        match p {
                            Value::Str(x) => s.push_str(x),
                            other => s.push_str(&other.to_text()),
                        }
                    }
                    tri!(check_str(vm, &s));
                    self.stack.push(Value::Str(s.into()));
                }
                Op::Unpack(n) => {
                    let v = self.pop();
                    match v {
                        Value::List(l) => {
                            for i in 0..n as usize {
                                self.stack.push(l.get(i).unwrap_or_default());
                            }
                        }
                        other => {
                            self.stack.push(other);
                            for _ in 1..n {
                                self.stack.push(Value::Nil);
                            }
                        }
                    }
                }
                Op::TryBegin(o) => {
                    self.handlers.push(Handler {
                        frames: self.frames.len(),
                        stack: self.stack.len(),
                        catch_ip: (ip as i64 + o as i64) as usize,
                    });
                }
                Op::TryEnd => {
                    self.handlers.pop();
                }
                Op::Throw => {
                    let v = self.pop();
                    fail!(Throw::Value(v));
                }
                Op::Fail(k) => {
                    fail!(Throw::msg(proto.consts[k as usize].to_text()));
                }
            }
        }
    }
}

struct Budget<'a> {
    left: u32,
    out: &'a mut u32,
}

impl Drop for Budget<'_> {
    fn drop(&mut self) {
        *self.out = self.left;
    }
}

fn no_field(obj: &Value, name: &str, note: &Option<Rc<str>>, fields: &[&str]) -> Throw {
    let hint = suggest(name, fields.iter().copied()).map(|s| format!("\nhint: did you mean `{s}`?")).unwrap_or_default();
    let extra = match obj {
        Value::List(_) => " — lists have `.length`; read items with `list[i]`",
        Value::Str(_) => " — strings have `.length`; read characters with `s[i]`",
        _ => "",
    };
    Throw::msg(format!("{}{} has no field `{name}`{extra}{hint}", obj.type_name(), of_what(note)))
}
