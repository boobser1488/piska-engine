//! Значения Scenario: `nil`, логические, числа (f64), строки (UTF-8), списки, словари
//! (порядок вставки), функции, векторы `vec2`/`vec3`, цвета и значения хоста (ручки).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use crate::bytecode::Proto;
use crate::vm::NativeCtx;

/// Значение скрипта. Копирование дешёвое: строки и коллекции — за `Rc`.
#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    List(Rc<List>),
    Map(Rc<Map>),
    Func(Rc<Closure>),
    Native(Rc<Native>),
    Vec2(f32, f32),
    Vec3(f32, f32, f32),
    Color(Rc<[f32; 4]>),
    /// Значение хоста: объект игры, задача и т. п. Класс задаёт свойства и методы.
    Handle(Handle),
    // Внутренние значения ВМ, скрипту не видны.
    #[doc(hidden)]
    Cell(Rc<RefCell<Value>>),
    #[doc(hidden)]
    Iter(Rc<RefCell<IterState>>),
    #[doc(hidden)]
    Uninit,
}

/// Ручка значения хоста: номер класса (см. [`crate::Vm::register_class`]) и id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    pub class: u32,
    pub id: u64,
}

/// Класс ручки задач (`start(fn)` возвращает такую ручку).
pub const TASK_CLASS: u32 = 0;

thread_local! {
    /// Сколько элементов сейчас лежит во всех списках и словарях (мягкий предел памяти).
    static LIVE: Cell<isize> = const { Cell::new(0) };
}

/// Число элементов во всех живых списках и словарях этого потока.
pub fn live_values() -> usize {
    LIVE.with(|c| c.get().max(0) as usize)
}

fn live_add(n: isize) {
    LIVE.with(|c| c.set(c.get() + n));
}

/// Список. Изменяется только через методы — они ведут учёт памяти.
pub struct List {
    items: RefCell<Vec<Value>>,
}

impl List {
    pub fn new(items: Vec<Value>) -> Rc<List> {
        live_add(items.len() as isize);
        Rc::new(List { items: RefCell::new(items) })
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, Vec<Value>> {
        self.items.borrow()
    }

    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, i: usize) -> Option<Value> {
        self.items.borrow().get(i).cloned()
    }

    pub fn set(&self, i: usize, v: Value) -> bool {
        let mut items = self.items.borrow_mut();
        if i < items.len() {
            items[i] = v;
            true
        } else if i == items.len() {
            items.push(v);
            live_add(1);
            true
        } else {
            false
        }
    }

    pub fn push(&self, v: Value) {
        self.items.borrow_mut().push(v);
        live_add(1);
    }

    pub fn pop(&self) -> Option<Value> {
        let v = self.items.borrow_mut().pop();
        if v.is_some() {
            live_add(-1);
        }
        v
    }

    pub fn insert(&self, i: usize, v: Value) -> bool {
        let mut items = self.items.borrow_mut();
        if i > items.len() {
            return false;
        }
        items.insert(i, v);
        live_add(1);
        true
    }

    pub fn remove(&self, i: usize) -> Option<Value> {
        let mut items = self.items.borrow_mut();
        if i < items.len() {
            live_add(-1);
            Some(items.remove(i))
        } else {
            None
        }
    }

    /// Заменяет всё содержимое.
    pub fn replace(&self, new: Vec<Value>) {
        let old = {
            let mut items = self.items.borrow_mut();
            live_add(new.len() as isize - items.len() as isize);
            std::mem::replace(&mut *items, new)
        };
        drop_values(old);
    }

    /// Копия элементов.
    pub fn to_vec(&self) -> Vec<Value> {
        self.items.borrow().clone()
    }

    /// Изменение на месте без смены длины (сортировка, разворот).
    pub fn modify(&self, f: impl FnOnce(&mut Vec<Value>)) {
        let mut items = self.items.borrow_mut();
        let before = items.len();
        f(&mut items);
        live_add(items.len() as isize - before as isize);
    }
}

impl Drop for List {
    fn drop(&mut self) {
        let items = std::mem::take(self.items.get_mut());
        live_add(-(items.len() as isize));
        drop_values(items);
    }
}

/// Освобождает значения без рекурсии: глубоко вложенные списки не переполняют стек.
fn drop_values(mut stack: Vec<Value>) {
    while let Some(v) = stack.pop() {
        match v {
            Value::List(rc) => {
                if let Ok(list) = Rc::try_unwrap(rc) {
                    let mut list = list;
                    let items = std::mem::take(list.items.get_mut());
                    live_add(-(items.len() as isize));
                    stack.extend(items);
                }
            }
            Value::Map(rc) => {
                if let Ok(map) = Rc::try_unwrap(rc) {
                    let mut map = map;
                    let inner = std::mem::take(map.inner.get_mut());
                    live_add(-(inner.len as isize));
                    stack.extend(inner.entries.into_iter().flatten().map(|(_, v)| v));
                }
            }
            _ => {}
        }
    }
}

/// Ключ словаря: строка, число, логическое или ручка.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Str(Rc<str>),
    Num(u64),
    Bool(bool),
    Handle(u32, u64),
}

impl Key {
    pub fn from_value(v: &Value) -> Result<Key, String> {
        Ok(match v {
            Value::Str(s) => Key::Str(s.clone()),
            Value::Num(n) => {
                if n.is_nan() {
                    return Err("a map key can't be NaN".into());
                }
                let n = if *n == 0.0 { 0.0 } else { *n };
                Key::Num(n.to_bits())
            }
            Value::Bool(b) => Key::Bool(*b),
            Value::Handle(h) => Key::Handle(h.class, h.id),
            other => {
                return Err(format!(
                    "a map key must be a string, number, boolean or object, not {}",
                    other.type_name()
                ))
            }
        })
    }

    pub fn to_value(&self) -> Value {
        match self {
            Key::Str(s) => Value::Str(s.clone()),
            Key::Num(b) => Value::Num(f64::from_bits(*b)),
            Key::Bool(b) => Value::Bool(*b),
            Key::Handle(c, id) => Value::Handle(Handle { class: *c, id: *id }),
        }
    }
}

/// Словарь с порядком вставки.
#[derive(Default)]
pub struct OrderedMap {
    entries: Vec<Option<(Key, Value)>>,
    index: HashMap<Key, usize>,
    len: usize,
}

impl OrderedMap {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, k: &Key) -> Option<&Value> {
        self.index.get(k).and_then(|&i| self.entries[i].as_ref()).map(|(_, v)| v)
    }

    pub fn get_str(&self, k: &str) -> Option<&Value> {
        self.get(&Key::Str(k.into()))
    }

    pub fn contains(&self, k: &Key) -> bool {
        self.index.contains_key(k)
    }

    /// Вставляет; `true`, если ключ новый.
    pub fn insert(&mut self, k: Key, v: Value) -> bool {
        if let Some(&i) = self.index.get(&k) {
            if let Some(e) = self.entries[i].as_mut() {
                e.1 = v;
            }
            false
        } else {
            self.index.insert(k.clone(), self.entries.len());
            self.entries.push(Some((k, v)));
            self.len += 1;
            live_add(1);
            true
        }
    }

    pub fn remove(&mut self, k: &Key) -> Option<Value> {
        let i = self.index.remove(k)?;
        let (_, v) = self.entries[i].take()?;
        self.len -= 1;
        live_add(-1);
        if self.entries.len() > 16 && self.len * 2 < self.entries.len() {
            self.compact();
        }
        Some(v)
    }

    fn compact(&mut self) {
        self.entries.retain(|e| e.is_some());
        self.index.clear();
        for (i, e) in self.entries.iter().enumerate() {
            if let Some((k, _)) = e {
                self.index.insert(k.clone(), i);
            }
        }
    }

    /// Пары в порядке вставки.
    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Value)> {
        self.entries.iter().flatten().map(|(k, v)| (k, v))
    }

    /// Запись по «слоту» — для итерации, переживающей изменения словаря.
    pub(crate) fn slot(&self, i: usize) -> Option<Option<(&Key, &Value)>> {
        self.entries.get(i).map(|e| e.as_ref().map(|(k, v)| (k, v)))
    }

    pub fn clear(&mut self) {
        live_add(-(self.len as isize));
        let old = std::mem::take(&mut self.entries);
        self.index.clear();
        self.len = 0;
        drop_values(old.into_iter().flatten().map(|(_, v)| v).collect());
    }
}

pub struct Map {
    inner: RefCell<OrderedMap>,
}

impl Map {
    pub fn new(inner: OrderedMap) -> Rc<Map> {
        Rc::new(Map { inner: RefCell::new(inner) })
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, OrderedMap> {
        self.inner.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, OrderedMap> {
        self.inner.borrow_mut()
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        let inner = std::mem::take(self.inner.get_mut());
        live_add(-(inner.len as isize));
        drop_values(inner.entries.into_iter().flatten().map(|(_, v)| v).collect());
    }
}

/// Замыкание: прототип функции + захваченные переменные + глобальные скрипта.
pub struct Closure {
    pub proto: Rc<Proto>,
    pub upvals: Box<[Rc<RefCell<Value>>]>,
    pub module: Rc<Module>,
}

/// Глобальные переменные одного экземпляра скрипта.
pub struct Module {
    pub(crate) slots: RefCell<Vec<Value>>,
    pub(crate) names: Rc<[Rc<str>]>,
}

impl Module {
    /// Значение глобальной переменной скрипта по имени.
    pub fn get(&self, name: &str) -> Option<Value> {
        let i = self.names.iter().position(|n| &**n == name)?;
        match self.slots.borrow().get(i) {
            Some(Value::Uninit) | None => None,
            Some(v) => Some(v.clone()),
        }
    }

    /// Разрывает циклы `модуль → функция → модуль`, чтобы память освободилась.
    pub fn clear(&self) {
        let old = std::mem::take(&mut *self.slots.borrow_mut());
        drop(old);
    }
}

/// Результат встроенной функции.
pub type NativeResult = Result<Value, Throw>;

/// Тип встроенной функции.
pub type NativeFn = dyn Fn(&mut NativeCtx, Args) -> NativeResult;

/// Встроенная функция (из Rust).
pub struct Native {
    pub name: Rc<str>,
    pub func: Box<NativeFn>,
    /// Это метод: аргумент 0 — получатель, нумерация в сообщениях с него не начинается.
    pub method: bool,
}

/// Ошибка, выброшенная из встроенной функции или скрипта.
#[derive(Clone)]
pub enum Throw {
    /// Сообщение (будет строкой в `catch (e)`).
    Msg(String),
    /// Значение из `error(v)` / `throw v`.
    Value(Value),
    /// Нарушен предел — не ловится `try`.
    Limit(String),
}

impl Throw {
    pub fn msg(s: impl Into<String>) -> Self {
        Throw::Msg(s.into())
    }
}

impl From<String> for Throw {
    fn from(s: String) -> Self {
        Throw::Msg(s)
    }
}

impl From<&str> for Throw {
    fn from(s: &str) -> Self {
        Throw::Msg(s.into())
    }
}

/// Состояние цикла `for … of/in`.
pub struct IterState {
    pub(crate) src: IterSrc,
    pub(crate) pos: usize,
    /// `of` — значения; иначе ключ (индекс) и значение.
    pub(crate) of: bool,
}

pub(crate) enum IterSrc {
    List(Rc<List>),
    Map(Rc<Map>),
    Str(Vec<char>),
}

/// Аргументы встроенной функции с проверкой типов и понятными ошибками.
#[derive(Clone, Copy)]
pub struct Args<'a> {
    pub values: &'a [Value],
    pub name: &'a str,
    /// 1 для методов: аргумент 0 — получатель.
    pub offset: usize,
}

thread_local! {
    /// Общий `nil` для отсутствующих аргументов (одна утечка на поток).
    static NIL: &'static Value = Box::leak(Box::new(Value::Nil));
}

impl<'a> Args<'a> {
    pub fn new(values: &'a [Value], name: &'a str) -> Self {
        Self { values, name, offset: 0 }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Аргумент `i` или `nil`.
    pub fn get(&self, i: usize) -> &'a Value {
        match self.values.get(i) {
            Some(v) => v,
            None => NIL.with(|n| *n),
        }
    }

    pub fn has(&self, i: usize) -> bool {
        !matches!(self.get(i), Value::Nil)
    }

    pub fn bad(&self, i: usize, expected: &str) -> Throw {
        let got = self.get(i);
        let n = i + 1 - self.offset.min(i + 1);
        if i < self.offset {
            Throw::Msg(format!("`{}` must be called on {expected}, not on {}", self.name, got.type_name()))
        } else {
            Throw::Msg(format!(
                "bad argument #{n} to `{}`: expected {expected}, got {}",
                self.name,
                got.describe()
            ))
        }
    }

    pub fn num(&self, i: usize) -> Result<f64, Throw> {
        match self.get(i) {
            Value::Num(n) => Ok(*n),
            _ => Err(self.bad(i, "a number")),
        }
    }

    pub fn opt_num(&self, i: usize, default: f64) -> Result<f64, Throw> {
        match self.get(i) {
            Value::Nil => Ok(default),
            _ => self.num(i),
        }
    }

    /// Целое (дробная часть отбрасывается).
    pub fn int(&self, i: usize) -> Result<i64, Throw> {
        let n = self.num(i)?;
        if !n.is_finite() {
            return Err(self.bad(i, "a finite number"));
        }
        Ok(n.trunc() as i64)
    }

    pub fn str(&self, i: usize) -> Result<&'a Rc<str>, Throw> {
        match self.get(i) {
            Value::Str(s) => Ok(s),
            _ => Err(self.bad(i, "a string")),
        }
    }

    pub fn opt_str(&self, i: usize) -> Result<Option<&'a Rc<str>>, Throw> {
        match self.get(i) {
            Value::Nil => Ok(None),
            _ => self.str(i).map(Some),
        }
    }

    pub fn boolean(&self, i: usize) -> Result<bool, Throw> {
        match self.get(i) {
            Value::Bool(b) => Ok(*b),
            _ => Err(self.bad(i, "a boolean")),
        }
    }

    pub fn list(&self, i: usize) -> Result<&'a Rc<List>, Throw> {
        match self.get(i) {
            Value::List(l) => Ok(l),
            _ => Err(self.bad(i, "a list")),
        }
    }

    pub fn map(&self, i: usize) -> Result<&'a Rc<Map>, Throw> {
        match self.get(i) {
            Value::Map(m) => Ok(m),
            _ => Err(self.bad(i, "a map")),
        }
    }

    pub fn func(&self, i: usize) -> Result<&'a Value, Throw> {
        match self.get(i) {
            v @ (Value::Func(_) | Value::Native(_)) => Ok(v),
            _ => Err(self.bad(i, "a function")),
        }
    }

    pub fn handle(&self, i: usize, class: u32, what: &str) -> Result<u64, Throw> {
        match self.get(i) {
            Value::Handle(h) if h.class == class => Ok(h.id),
            _ => Err(self.bad(i, what)),
        }
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.into())
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(List::new(items))
    }

    /// Словарь из пар «строковый ключ — значение».
    pub fn map_from<'k>(pairs: impl IntoIterator<Item = (&'k str, Value)>) -> Value {
        let mut m = OrderedMap::default();
        for (k, v) in pairs {
            m.insert(Key::Str(k.into()), v);
        }
        Value::Map(Map::new(m))
    }

    pub fn color(r: f32, g: f32, b: f32, a: f32) -> Value {
        Value::Color(Rc::new([r, g, b, a]))
    }

    /// Имя типа, как его возвращает `type(x)` / `typeof x`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil | Value::Uninit => "nil",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Func(_) | Value::Native(_) => "function",
            Value::Vec2(..) => "vec2",
            Value::Vec3(..) => "vec3",
            Value::Color(_) => "color",
            Value::Handle(h) if h.class == TASK_CLASS => "task",
            Value::Handle(_) => "object",
            Value::Cell(_) | Value::Iter(_) => "internal",
        }
    }

    /// Описание для сообщений: `nil`, `the number 5`, `a string "abc"`.
    pub fn describe(&self) -> String {
        match self {
            Value::Nil | Value::Uninit => "nil".into(),
            Value::Bool(b) => format!("the boolean {b}"),
            Value::Num(n) => format!("the number {}", fmt_num(*n)),
            Value::Str(s) => {
                let short: String = s.chars().take(24).collect();
                let more = if s.chars().count() > 24 { "…" } else { "" };
                format!("the string \"{short}{more}\"")
            }
            other => format!("a {}", other.type_name()),
        }
    }

    /// Ложны `nil`, `false`, `0`, `NaN` и пустая строка (как в JavaScript).
    #[inline]
    pub fn truthy(&self) -> bool {
        match self {
            Value::Nil | Value::Uninit => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil | Value::Uninit)
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Равенство без приведения типов; списки и словари — по ссылке.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Nil | Value::Uninit, Value::Nil | Value::Uninit) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => Rc::ptr_eq(a, b),
            (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(a, b),
            (Value::Func(a), Value::Func(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => Rc::ptr_eq(a, b),
            (Value::Vec2(a, b), Value::Vec2(c, d)) => a == c && b == d,
            (Value::Vec3(a, b, c), Value::Vec3(d, e, f)) => a == d && b == e && c == f,
            (Value::Color(a), Value::Color(b)) => a == b,
            (Value::Handle(a), Value::Handle(b)) => a == b,
            _ => false,
        }
    }

    /// Текст значения, как его печатает `print` и `tostring`.
    pub fn to_text(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            other => {
                let mut out = String::new();
                other.write_repr(&mut out, 0, false);
                out
            }
        }
    }

    fn write_repr(&self, out: &mut String, depth: usize, quoted: bool) {
        match self {
            Value::Nil | Value::Uninit => out.push_str("nil"),
            Value::Bool(b) => {
                let _ = write!(out, "{b}");
            }
            Value::Num(n) => out.push_str(&fmt_num(*n)),
            Value::Str(s) => {
                if quoted {
                    let _ = write!(out, "{:?}", &**s);
                } else {
                    out.push_str(s);
                }
            }
            Value::List(l) => {
                if depth > 6 {
                    out.push_str("[…]");
                    return;
                }
                out.push('[');
                for (i, v) in l.borrow().iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if i >= 100 {
                        out.push('…');
                        break;
                    }
                    v.write_repr(out, depth + 1, true);
                }
                out.push(']');
            }
            Value::Map(m) => {
                if depth > 6 {
                    out.push_str("{…}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in m.borrow().iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if i >= 100 {
                        out.push('…');
                        break;
                    }
                    match k {
                        Key::Str(s) if is_identifier(s) => out.push_str(s),
                        other => {
                            out.push('[');
                            other.to_value().write_repr(out, depth + 1, true);
                            out.push(']');
                        }
                    }
                    out.push_str(": ");
                    v.write_repr(out, depth + 1, true);
                }
                out.push('}');
            }
            Value::Func(c) => {
                let _ = write!(out, "<function {}>", c.proto.name);
            }
            Value::Native(n) => {
                let _ = write!(out, "<function {}>", n.name);
            }
            Value::Vec2(x, y) => {
                let _ = write!(out, "vec2({}, {})", fmt_num(clean(*x)), fmt_num(clean(*y)));
            }
            Value::Vec3(x, y, z) => {
                let _ = write!(out, "vec3({}, {}, {})", fmt_num(clean(*x)), fmt_num(clean(*y)), fmt_num(clean(*z)));
            }
            Value::Color(c) => {
                let _ = write!(
                    out,
                    "color({}, {}, {}, {})",
                    fmt_num(clean(c[0])),
                    fmt_num(clean(c[1])),
                    fmt_num(clean(c[2])),
                    fmt_num(clean(c[3]))
                );
            }
            Value::Handle(h) if h.class == TASK_CLASS => {
                let _ = write!(out, "<task {}>", { h.id });
            }
            Value::Handle(h) => {
                let _ = write!(out, "<object {}:{}>", { h.class }, { h.id });
            }
            Value::Cell(c) => c.borrow().write_repr(out, depth, quoted),
            Value::Iter(_) => out.push_str("<iterator>"),
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        self.write_repr(&mut out, 0, true);
        f.write_str(&out)
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Num(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Num(n as f64)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.into())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s.into())
    }
}

pub(crate) fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
        && !crate::lexer::KEYWORDS.contains(&s)
}

/// f32 → f64 без «хвоста» двоичного округления: 0.1f32 читается как 0.1.
pub fn clean(x: f32) -> f64 {
    let d = x as f64;
    if !d.is_finite() {
        return d;
    }
    let r = (d * 1e6).round() / 1e6;
    if r as f32 == x {
        r
    } else {
        d
    }
}

/// Число в тексте: целые без `.0`.
pub fn fmt_num(n: f64) -> String {
    if n.is_nan() {
        "nan".into()
    } else if n.is_infinite() {
        if n > 0.0 { "inf".into() } else { "-inf".into() }
    } else if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
