//! Доступ к миру из колбэков API, переводы значений через границу и жалобы в лог.
//!
//! У функций [`EngineApi`](crate::abi::EngineApi) нет параметра «мир»: на время хука
//! указатель на `World` кладётся в ячейку потока. Это единственное место движка, где
//! `&mut World` временно превращается в сырой указатель.
//!
//! Правило повторного входа: хук C++ вызывается только из [`CppComponent`](crate::CppComponent)
//! (и из `CppPlugins::fixed_update`), а функции API хуков не вызывают — они лишь ставят
//! в очередь сообщения, уничтожение и смену сцены. Поэтому двух `&mut World` одновременно
//! не бывает, а «уничтожить себя в `on_update`» безопасно: объект живёт до конца кадра.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use engine_core::prelude::*;

use crate::abi::*;

thread_local! {
    /// Мир, в котором сейчас выполняется хук C++-компонента (или null вне хука).
    static CURRENT_WORLD: Cell<*mut World> = const { Cell::new(std::ptr::null_mut()) };
    static COMPLAINTS: RefCell<HashMap<&'static str, u32>> = RefCell::new(HashMap::new());
    static RING: RefCell<Ring> = const { RefCell::new(Ring { slots: Vec::new(), next: 0 }) };
}

/// Пока жив — функции API работают с этим миром.
pub(crate) struct WorldScope {
    previous: *mut World,
}

impl WorldScope {
    pub(crate) fn enter(world: &mut World) -> Self {
        Self {
            previous: CURRENT_WORLD.replace(world),
        }
    }
}

impl Drop for WorldScope {
    fn drop(&mut self) {
        CURRENT_WORLD.set(self.previous);
    }
}

/// Идёт ли сейчас хук (есть ли мир для API).
pub(crate) fn in_hook() -> bool {
    !CURRENT_WORLD.get().is_null()
}

/// Выполнить `f` над текущим миром. Вне хука — `None` и ошибка в логе (`api` — имя функции).
pub(crate) fn with_world<R>(api: &'static str, f: impl FnOnce(&mut World) -> R) -> Option<R> {
    let world = CURRENT_WORLD.get();
    if world.is_null() {
        complain(api, || {
            format!(
                "`{api}` was called outside of a component hook (constructor, static initializer or \
                 on_load?); the engine world is only available inside on_start/on_update/…. \
                 Move the call into a hook"
            )
        });
        return None;
    }
    // SAFETY: указатель положил `WorldScope::enter` из живого `&mut World`, и он
    // действителен, пока область жива — то есть на всё время вызова хука C++.
    // Исходная ссылка в это время не используется (поток стоит внутри FFI-вызова),
    // а функции API не вызывают хуков, так что двух `&mut World` одновременно нет.
    Some(f(unsafe { &mut *world }))
}

/// Как [`with_world`], но молча: для запросов, которым вне хука отвечать «нет» нормально.
pub(crate) fn with_world_quiet<R>(f: impl FnOnce(&mut World) -> R) -> Option<R> {
    let world = CURRENT_WORLD.get();
    if world.is_null() {
        return None;
    }
    // SAFETY: см. `with_world`.
    Some(f(unsafe { &mut *world }))
}

/// Выполнить `f` над живым объектом. Нулевой или устаревший хэндл — ошибка в логе и `None`.
pub(crate) fn with_object<R>(
    api: &'static str,
    object: EngineObject,
    f: impl FnOnce(&mut World, GameObjectId) -> R,
) -> Option<R> {
    with_world(api, |world| {
        let id = to_id(object);
        if object.generation == 0 {
            complain(api, || {
                format!(
                    "`{api}` got a null object handle: the lookup that produced it found nothing \
                     (check `if (obj)` before using it)"
                )
            });
            return None;
        }
        if !world.scene.contains(id) {
            complain(api, || {
                format!(
                    "`{api}` got a stale object handle {id:?}: the object was destroyed. \
                     Check `obj.alive()` before using a stored handle"
                )
            });
            return None;
        }
        Some(f(world, id))
    })
    .flatten()
}

/// Сообщить об ошибке плагина: не больше 10 раз на функцию API, чтобы цикл в `on_update`
/// не завалил консоль.
pub(crate) fn complain(api: &'static str, message: impl FnOnce() -> String) {
    const LIMIT: u32 = 10;
    let count = COMPLAINTS.with(|map| {
        let mut map = map.borrow_mut();
        let count = map.entry(api).or_insert(0);
        *count += 1;
        *count
    });
    match count {
        1..=LIMIT => log::error!(target: "plugin", "{}", message()),
        n if n == LIMIT + 1 => log::error!(
            target: "plugin",
            "further errors from `{api}` are not logged (the first {LIMIT} were shown above)"
        ),
        _ => {}
    }
}

/// Сбросить счётчики жалоб (для тестов, которые проверяют текст в логе).
#[doc(hidden)]
pub fn reset_complaints() {
    COMPLAINTS.with(|map| map.borrow_mut().clear());
}

// ----------------------------------------------------------------------
// Объекты и строки
// ----------------------------------------------------------------------

pub(crate) const NO_OBJECT: EngineObject = EngineObject {
    index: 0,
    generation: 0,
};
pub(crate) const ZERO: EngineVec3 = EngineVec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

pub(crate) fn to_id(object: EngineObject) -> GameObjectId {
    GameObjectId::from_bits((object.generation as u64) << 32 | object.index as u64)
}

pub(crate) fn to_object(id: GameObjectId) -> EngineObject {
    let bits = id.to_bits();
    EngineObject {
        index: bits as u32,
        generation: (bits >> 32) as u32,
    }
}

pub(crate) fn vec3(v: Vec3) -> EngineVec3 {
    EngineVec3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

pub(crate) fn glam3(v: EngineVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Строка из C++.
///
/// # Safety
/// `text` — null или валидная C-строка, живая на время вызова (контракт ABI).
pub(crate) unsafe fn text<'a>(text: *const c_char) -> Option<&'a str> {
    if text.is_null() {
        return None;
    }
    // SAFETY: см. предусловие функции.
    unsafe { CStr::from_ptr(text) }.to_str().ok()
}

/// Записать строку в буфер плагина (с нулём, обрезая по границе символа) и вернуть
/// полную длину без нуля.
///
/// # Safety
/// `buf` — null или указатель на `cap` байт, доступных для записи (контракт ABI).
pub(crate) unsafe fn write_text(value: &str, buf: *mut c_char, cap: u32) -> u32 {
    if !buf.is_null() && cap > 0 {
        let mut n = value.len().min(cap as usize - 1);
        while !value.is_char_boundary(n) {
            n -= 1;
        }
        // SAFETY: `n < cap`, буфер по контракту вмещает `cap` байт; области не пересекаются.
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), buf.cast::<u8>(), n);
            buf.add(n).cast::<u8>().write(0);
        }
    }
    value.len() as u32
}

// ----------------------------------------------------------------------
// Значения
// ----------------------------------------------------------------------

/// Кольцо строк, которые движок показывает плагину по указателю (`EngineValue.s`).
struct Ring {
    slots: Vec<CString>,
    next: usize,
}

const RING_CAPACITY: usize = 256;

/// Строка для C++: живёт, пока не сделано ещё 256 таких строк.
pub(crate) fn intern(value: &str) -> *const c_char {
    let c = CString::new(value.replace('\0', " ")).unwrap_or_default();
    RING.with(|ring| {
        let mut ring = ring.borrow_mut();
        let at = ring.next;
        ring.next = (at + 1) % RING_CAPACITY;
        if ring.slots.len() < RING_CAPACITY {
            ring.slots.push(c);
            ring.slots.last().map_or(std::ptr::null(), |s| s.as_ptr())
        } else {
            ring.slots[at] = c;
            ring.slots[at].as_ptr()
        }
    })
}

pub(crate) fn nil_value() -> EngineValue {
    EngineValue {
        kind: ENGINE_VALUE_NIL,
        i: 0,
        f: [0.0; 4],
        s: std::ptr::null(),
    }
}

/// `Value` движка → значение ABI. Строки идут через кольцо [`intern`].
/// `Value::List` через границу не ходит: он превращается в `Nil`.
pub(crate) fn to_engine_value(value: &Value) -> EngineValue {
    let mut out = nil_value();
    match value {
        Value::Nil | Value::List(_) => {}
        Value::Bool(b) => {
            out.kind = ENGINE_VALUE_BOOL;
            out.i = *b as i64;
        }
        Value::Int(i) => {
            out.kind = ENGINE_VALUE_INT;
            out.i = *i;
        }
        Value::Float(f) => {
            out.kind = ENGINE_VALUE_FLOAT;
            out.f[0] = *f;
        }
        Value::Str(s) => {
            out.kind = ENGINE_VALUE_STR;
            out.s = intern(s);
        }
        Value::Enum(s) => {
            out.kind = ENGINE_VALUE_ENUM;
            out.s = intern(s);
        }
        Value::Asset(s) => {
            out.kind = ENGINE_VALUE_ASSET;
            out.s = intern(s);
        }
        Value::Vec2(x, y) => {
            out.kind = ENGINE_VALUE_VEC2;
            out.f = [*x, *y, 0.0, 0.0];
        }
        Value::Vec3(x, y, z) => {
            out.kind = ENGINE_VALUE_VEC3;
            out.f = [*x, *y, *z, 0.0];
        }
        Value::Quat(x, y, z, w) => {
            out.kind = ENGINE_VALUE_QUAT;
            out.f = [*x, *y, *z, *w];
        }
        Value::Color(r, g, b, a) => {
            out.kind = ENGINE_VALUE_COLOR;
            out.f = [*r, *g, *b, *a];
        }
    }
    out
}

/// Значение ABI → `Value` движка. `None` — неизвестный вид или битая строка.
///
/// # Safety
/// `value.s` для строковых видов — null или валидная C-строка (контракт ABI).
pub(crate) unsafe fn from_engine_value(value: &EngineValue) -> Option<Value> {
    // SAFETY: предусловие функции.
    let string = || unsafe { text(value.s) }.map(str::to_string);
    Some(match value.kind {
        ENGINE_VALUE_NIL => Value::Nil,
        ENGINE_VALUE_BOOL => Value::Bool(value.i != 0),
        ENGINE_VALUE_INT => Value::Int(value.i),
        ENGINE_VALUE_FLOAT => Value::Float(value.f[0]),
        ENGINE_VALUE_STR => Value::Str(string().unwrap_or_default()),
        ENGINE_VALUE_ENUM => Value::Enum(string()?),
        ENGINE_VALUE_ASSET => Value::Asset(string().unwrap_or_default()),
        ENGINE_VALUE_VEC2 => Value::Vec2(value.f[0], value.f[1]),
        ENGINE_VALUE_VEC3 => Value::Vec3(value.f[0], value.f[1], value.f[2]),
        ENGINE_VALUE_QUAT => Value::Quat(value.f[0], value.f[1], value.f[2], value.f[3]),
        ENGINE_VALUE_COLOR => Value::Color(value.f[0], value.f[1], value.f[2], value.f[3]),
        // Объект через границу идёт как число: `GameObjectId::to_bits()`.
        ENGINE_VALUE_OBJECT => Value::Int(value.i),
        _ => return None,
    })
}

/// Привести новое значение к виду, который сейчас лежит в поле: так C++ может записать
/// `1` в `float`-поле Rust-компонента, а строку — в перечисление.
pub(crate) fn coerce_like(current: &Value, new: Value) -> Value {
    match (current, new) {
        (Value::Int(_), Value::Float(f)) => Value::Int(f.round() as i64),
        (Value::Float(_), Value::Int(i)) => Value::Float(i as f32),
        (Value::Bool(_), Value::Int(i)) => Value::Bool(i != 0),
        (Value::Bool(_), Value::Float(f)) => Value::Bool(f != 0.0),
        (Value::Int(_), Value::Bool(b)) => Value::Int(b as i64),
        (Value::Float(_), Value::Bool(b)) => Value::Float(b as i32 as f32),
        (Value::Enum(_), Value::Str(s)) => Value::Enum(s),
        (Value::Str(_), Value::Enum(s)) => Value::Str(s),
        (Value::Asset(_), Value::Str(s)) => Value::Asset(s),
        (Value::Color(_, _, _, a), Value::Vec3(x, y, z)) => Value::Color(x, y, z, *a),
        (Value::Vec3(..), Value::Color(r, g, b, _)) => Value::Vec3(r, g, b),
        (Value::Vec2(..), Value::Vec3(x, y, _)) => Value::Vec2(x, y),
        (Value::Vec3(..), Value::Vec2(x, y)) => Value::Vec3(x, y, 0.0),
        (_, new) => new,
    }
}

/// `Value` как поле: чтобы твины и сохранения принимали динамическое значение.
pub(crate) struct RawValue(pub Value);

impl FieldValue for RawValue {
    const EXPECTED: &'static str = "any value";

    fn to_value(&self) -> Value {
        self.0.clone()
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(RawValue(value.clone()))
    }
}

// ----------------------------------------------------------------------
// Таблица хэндлов таймеров и твинов (в ABI они — обычные числа)
// ----------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Handles {
    next: u64,
    timers: HashMap<u64, TimerId>,
    tweens: HashMap<u64, TweenId>,
}

thread_local! {
    static HANDLES: RefCell<Handles> = RefCell::new(Handles::default());
}

const PRUNE_AT: usize = 4096;

pub(crate) fn remember_timer(world: &mut World, timer: TimerId) -> u64 {
    let stale = HANDLES.with(|h| h.borrow().timers.len() >= PRUNE_AT);
    if stale {
        let scheduler = world.scheduler();
        HANDLES.with(|h| h.borrow_mut().timers.retain(|_, t| scheduler.is_pending(*t)));
    }
    HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        h.next += 1;
        let handle = h.next;
        h.timers.insert(handle, timer);
        handle
    })
}

pub(crate) fn find_timer(handle: u64) -> Option<TimerId> {
    HANDLES.with(|h| h.borrow().timers.get(&handle).copied())
}

pub(crate) fn remember_tween(world: &mut World, tween: TweenId) -> u64 {
    let stale = HANDLES.with(|h| h.borrow().tweens.len() >= PRUNE_AT);
    if stale {
        let tweens = world.tweens();
        HANDLES.with(|h| h.borrow_mut().tweens.retain(|_, t| tweens.is_running(*t)));
    }
    HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        h.next += 1;
        let handle = h.next;
        h.tweens.insert(handle, tween);
        handle
    })
}

pub(crate) fn find_tween(handle: u64) -> Option<TweenId> {
    HANDLES.with(|h| h.borrow().tweens.get(&handle).copied())
}
