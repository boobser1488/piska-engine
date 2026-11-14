//! Функции, которые движок отдаёт плагину (`EngineApi`, ABI v3).
//!
//! Все функции — обычные `extern "C" fn`: аргументы-указатели приходят из C++ по контракту
//! ABI (null либо валидный указатель на время вызова), и это единственное допущение,
//! которое здесь делается — оно собрано в крошечных помощниках [`cs`], [`put`] и [`array`].
//! Дальше идёт безопасный Rust над `&mut World`.
//!
//! Поведение при ошибках одинаково для всех функций: вне хука, с нулевым или устаревшим
//! объектом, с неизвестным именем — запись в лог (не больше 10 раз на функцию) и
//! нейтральный результат (0, «нет объекта», пустая строка).

use std::collections::{HashMap, HashSet};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use engine_core::prelude::*;

use crate::abi::*;
use crate::scope::*;
use crate::services::CppServices;

// ── помощники границы ───────────────────────────────────────────────────────

/// Строка из C++: null или валидная C-строка на время вызова (контракт ABI).
fn cs<'a>(ptr: *const c_char) -> Option<&'a str> {
    // SAFETY: контракт ABI — указатель либо null, либо валидная C-строка плагина,
    // живая, пока выполняется функция API.
    unsafe { text(ptr) }
}

/// Записать значение по указателю плагина. `false` — указатель нулевой.
fn put<T>(out: *mut T, value: T) -> bool {
    if out.is_null() {
        return false;
    }
    // SAFETY: контракт ABI — ненулевой `out` указывает на `T`, доступный для записи.
    unsafe { out.write(value) };
    true
}

/// Прочитать значение по указателю плагина.
fn read_value(ptr: *const EngineValue) -> Option<Value> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: контракт ABI — ненулевой указатель ведёт на валидный `EngineValue`,
    // строка внутри — валидная C-строка (или null) на время вызова.
    unsafe { from_engine_value(&*ptr) }
}

/// Записать до `cap` объектов в массив плагина.
fn put_objects(out: *mut EngineObject, cap: u32, ids: &[GameObjectId]) {
    if out.is_null() {
        return;
    }
    for (i, id) in ids.iter().take(cap as usize).enumerate() {
        // SAFETY: контракт ABI — `out` указывает на `cap` элементов, `i < cap`.
        unsafe { out.add(i).write(to_object(*id)) };
    }
}

/// Записать строку в буфер плагина; вернуть полную длину.
fn put_text(value: &str, buf: *mut c_char, cap: u32) -> u32 {
    // SAFETY: контракт ABI — `buf` null либо `cap` байт, доступных для записи.
    unsafe { write_text(value, buf, cap) }
}

// ── лог, объект, Transform ──────────────────────────────────────────────────

extern "C" fn api_log(level: u32, message: *const c_char) {
    let Some(message) = cs(message) else {
        return;
    };
    match level {
        0 => log::error!(target: "plugin", "{message}"),
        1 => log::warn!(target: "plugin", "{message}"),
        _ => log::info!(target: "plugin", "{message}"),
    }
}

extern "C" fn api_is_alive(object: EngineObject) -> i32 {
    if object.generation == 0 {
        return 0;
    }
    with_world_quiet(|world| world.scene.contains(to_id(object)) as i32).unwrap_or(0)
}

extern "C" fn api_get_position(object: EngineObject) -> EngineVec3 {
    with_object("get_position", object, |world, id| {
        world.scene.get(id).map_or(ZERO, |o| vec3(o.transform.position))
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_set_position(object: EngineObject, position: EngineVec3) {
    with_object("set_position", object, |world, id| {
        if let Some(o) = world.scene.get_mut(id) {
            o.transform.position = glam3(position);
        }
    });
}

extern "C" fn api_get_rotation(object: EngineObject) -> EngineVec3 {
    with_object("get_rotation", object, |world, id| {
        world.scene.get(id).map_or(ZERO, |o| vec3(o.transform.euler_deg()))
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_set_rotation(object: EngineObject, euler_deg: EngineVec3) {
    with_object("set_rotation", object, |world, id| {
        if let Some(o) = world.scene.get_mut(id) {
            o.transform.set_euler_deg(glam3(euler_deg));
        }
    });
}

extern "C" fn api_get_scale(object: EngineObject) -> EngineVec3 {
    with_object("get_scale", object, |world, id| {
        world.scene.get(id).map_or(ZERO, |o| vec3(o.transform.scale))
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_set_scale(object: EngineObject, scale: EngineVec3) {
    with_object("set_scale", object, |world, id| {
        if let Some(o) = world.scene.get_mut(id) {
            o.transform.scale = glam3(scale);
        }
    });
}

extern "C" fn api_get_world_position(object: EngineObject) -> EngineVec3 {
    with_object("get_world_position", object, |world, id| vec3(world.scene.world_position(id)))
        .unwrap_or(ZERO)
}

/// Мировая матрица родителя (единичная у корня).
fn parent_matrix(scene: &Scene, id: GameObjectId) -> Mat4 {
    scene
        .get(id)
        .and_then(|o| o.parent())
        .map_or(Mat4::IDENTITY, |parent| scene.world_matrix(parent))
}

extern "C" fn api_set_world_position(object: EngineObject, position: EngineVec3) {
    with_object("set_world_position", object, |world, id| {
        let local = parent_matrix(&world.scene, id).inverse().transform_point3(glam3(position));
        if let Some(o) = world.scene.get_mut(id) {
            o.transform.position = local;
        }
    });
}

fn euler_of(rotation: Quat) -> Vec3 {
    let mut t = Transform::IDENTITY;
    t.rotation = rotation;
    t.euler_deg()
}

fn quat_of(euler_deg: Vec3) -> Quat {
    let mut t = Transform::IDENTITY;
    t.set_euler_deg(euler_deg);
    t.rotation
}

extern "C" fn api_get_world_rotation(object: EngineObject) -> EngineVec3 {
    with_object("get_world_rotation", object, |world, id| {
        vec3(euler_of(world.scene.world_rotation(id)))
    })
    .unwrap_or(ZERO)
}

fn set_world_rotation(world: &mut World, id: GameObjectId, rotation: Quat) {
    let parent = world.scene.get(id).and_then(|o| o.parent());
    let parent_rotation = parent.map_or(Quat::IDENTITY, |p| world.scene.world_rotation(p));
    if let Some(o) = world.scene.get_mut(id) {
        o.transform.rotation = parent_rotation.inverse() * rotation;
    }
}

extern "C" fn api_set_world_rotation(object: EngineObject, euler_deg: EngineVec3) {
    with_object("set_world_rotation", object, |world, id| {
        set_world_rotation(world, id, quat_of(glam3(euler_deg)));
    });
}

extern "C" fn api_get_direction(object: EngineObject, axis: u32) -> EngineVec3 {
    with_object("get_direction", object, |world, id| {
        let rotation = world.scene.world_rotation(id);
        vec3(match axis {
            0 => rotation * Vec3::X,
            1 => rotation * Vec3::Y,
            _ => rotation * Vec3::NEG_Z,
        })
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_look_at(object: EngineObject, target: EngineVec3, up: EngineVec3) {
    with_object("look_at", object, |world, id| {
        let position = world.scene.world_position(id);
        let mut t = Transform::from_xyz(position.x, position.y, position.z);
        t.look_at(glam3(target), glam3(up));
        set_world_rotation(world, id, t.rotation);
    });
}

// ── ввод ────────────────────────────────────────────────────────────────────

fn name_map<T: Copy + std::fmt::Debug>(all: &[T]) -> HashMap<String, T> {
    all.iter().map(|v| (format!("{v:?}").to_lowercase(), *v)).collect()
}

fn lookup<T: Copy>(map: &HashMap<String, T>, name: &str) -> Option<T> {
    map.get(&name.to_lowercase()).copied()
}

fn key_by_name(name: &str) -> Option<Key> {
    static MAP: OnceLock<HashMap<String, Key>> = OnceLock::new();
    let map = MAP.get_or_init(|| name_map(&Key::ALL));
    // Цифры можно писать и просто «1».
    if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) {
        return lookup(map, &format!("digit{name}"));
    }
    lookup(map, name)
}

fn pad_button_by_name(name: &str) -> Option<GamepadButton> {
    static MAP: OnceLock<HashMap<String, GamepadButton>> = OnceLock::new();
    lookup(MAP.get_or_init(|| name_map(&GamepadButton::ALL)), name)
}

fn pad_axis_by_name(name: &str) -> Option<GamepadAxis> {
    static MAP: OnceLock<HashMap<String, GamepadAxis>> = OnceLock::new();
    lookup(MAP.get_or_init(|| name_map(&GamepadAxis::ALL)), name)
}

/// Режим опроса: 0 — удерживается, 1 — нажата, 2 — отпущена.
fn by_mode(mode: u32, held: impl FnOnce() -> bool, down: impl FnOnce() -> bool, up: impl FnOnce() -> bool) -> bool {
    match mode {
        0 => held(),
        1 => down(),
        _ => up(),
    }
}

extern "C" fn api_input_axis(name: *const c_char) -> f32 {
    let Some(name) = cs(name) else {
        return 0.0;
    };
    with_world("input_axis", |world| world.input.axis(name)).unwrap_or(0.0)
}

extern "C" fn api_input_button(name: *const c_char) -> i32 {
    let Some(name) = cs(name) else {
        return 0;
    };
    with_world("input_button", |world| world.input.button(name) as i32).unwrap_or(0)
}

extern "C" fn api_input_button_down(name: *const c_char) -> i32 {
    let Some(name) = cs(name) else {
        return 0;
    };
    with_world("input_button_down", |world| world.input.button_down(name) as i32).unwrap_or(0)
}

extern "C" fn api_input_button_up(name: *const c_char) -> i32 {
    let Some(name) = cs(name) else {
        return 0;
    };
    with_world("input_button_up", |world| world.input.button_up(name) as i32).unwrap_or(0)
}

extern "C" fn api_input_button_value(name: *const c_char) -> f32 {
    let Some(name) = cs(name) else {
        return 0.0;
    };
    with_world("input_button_value", |world| world.input.button_value(name)).unwrap_or(0.0)
}

extern "C" fn api_input_key(name: *const c_char, mode: u32) -> i32 {
    let Some(name) = cs(name) else {
        return 0;
    };
    with_world("input_key", |world| {
        let Some(key) = key_by_name(name) else {
            complain("input_key", || {
                format!("input_key: unknown key `{name}`; use a name like `Space`, `A`, `Digit1`, `LShift`, `Left`, `F5`")
            });
            return 0;
        };
        let input = &world.input;
        by_mode(mode, || input.key(key), || input.key_down(key), || input.key_up(key)) as i32
    })
    .unwrap_or(0)
}

fn mouse_by_index(button: u32) -> MouseButton {
    match button {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        _ => MouseButton::Middle,
    }
}

extern "C" fn api_input_mouse_button(button: u32, mode: u32) -> i32 {
    with_world("input_mouse_button", |world| {
        let (input, b) = (&world.input, mouse_by_index(button));
        by_mode(mode, || input.mouse_button(b), || input.mouse_button_down(b), || input.mouse_button_up(b)) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_input_vector(kind: u32) -> EngineVec3 {
    with_world("input_vector", |world| {
        let v = match kind {
            0 => world.input.mouse_position(),
            1 => world.input.mouse_delta(),
            2 => world.input.scroll_delta(),
            _ => world.input.screen_size(),
        };
        EngineVec3 { x: v.x, y: v.y, z: 0.0 }
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_input_gamepad_count() -> u32 {
    with_world("input_gamepad_count", |world| world.input.gamepad_count() as u32).unwrap_or(0)
}

/// Геймпады, к которым относится запрос: один по номеру или все (`pad < 0`).
fn pads(input: &Input, pad: i32) -> Vec<GamepadId> {
    let all = input.gamepads();
    if pad < 0 {
        all
    } else {
        all.get(pad as usize).copied().into_iter().collect()
    }
}

extern "C" fn api_input_gamepad_button(pad: i32, name: *const c_char, mode: u32) -> i32 {
    let Some(name) = cs(name) else {
        return 0;
    };
    with_world("input_gamepad_button", |world| {
        let Some(button) = pad_button_by_name(name) else {
            complain("input_gamepad_button", || {
                format!("input_gamepad_button: unknown button `{name}`; use `South`, `East`, `LeftBumper`, `DPadUp`, `Start`…")
            });
            return 0;
        };
        let input = &world.input;
        pads(input, pad).into_iter().any(|id| {
            by_mode(
                mode,
                || input.gamepad_button(id, button),
                || input.gamepad_button_down(id, button),
                || input.gamepad_button_up(id, button),
            )
        }) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_input_gamepad_axis(pad: i32, name: *const c_char) -> f32 {
    let Some(name) = cs(name) else {
        return 0.0;
    };
    with_world("input_gamepad_axis", |world| {
        let Some(axis) = pad_axis_by_name(name) else {
            complain("input_gamepad_axis", || {
                format!("input_gamepad_axis: unknown axis `{name}`; use `LeftStickX`, `LeftStickY`, `RightStickX`, `RightStickY`, `LeftTrigger`, `RightTrigger`")
            });
            return 0.0;
        };
        pads(&world.input, pad)
            .into_iter()
            .map(|id| world.input.gamepad_axis(id, axis))
            .fold(0.0f32, |best, v| if v.abs() > best.abs() { v } else { best })
    })
    .unwrap_or(0.0)
}

extern "C" fn api_input_gamepad_stick(pad: i32, which: u32, dead_zone: f32) -> EngineVec3 {
    with_world("input_gamepad_stick", |world| {
        let input = &world.input;
        let best = pads(input, pad)
            .into_iter()
            .map(|id| {
                if which == 0 {
                    input.gamepad_left_stick(id, dead_zone)
                } else {
                    input.gamepad_right_stick(id, dead_zone)
                }
            })
            .fold(Vec2::ZERO, |best, v| if v.length() > best.length() { v } else { best });
        EngineVec3 { x: best.x, y: best.y, z: 0.0 }
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_input_touch_count() -> u32 {
    with_world("input_touch_count", |world| world.input.touches().len() as u32).unwrap_or(0)
}

extern "C" fn api_input_touch(index: u32, out: *mut EngineTouch) -> i32 {
    with_world("input_touch", |world| {
        let Some(t) = world.input.touches().get(index as usize) else {
            return 0;
        };
        let phase = match t.phase {
            TouchPhase::Began => 0,
            TouchPhase::Moved => 1,
            TouchPhase::Ended => 2,
        };
        put(
            out,
            EngineTouch {
                id: t.id,
                x: t.position.x,
                y: t.position.y,
                dx: t.delta.x,
                dy: t.delta.y,
                phase,
            },
        ) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_input_text(buf: *mut c_char, cap: u32) -> u32 {
    with_world("input_text", |world| put_text(world.input.text_input(), buf, cap)).unwrap_or(0)
}

// ── время ───────────────────────────────────────────────────────────────────

extern "C" fn api_time_seconds() -> f32 {
    with_world("time_seconds", |world| world.time.elapsed as f32).unwrap_or(0.0)
}

extern "C" fn api_time_info(out: *mut EngineTime) {
    let info = with_world("time_info", |world| {
        let t = &world.time;
        EngineTime {
            delta: t.delta,
            unscaled_delta: t.unscaled_delta,
            fixed_delta: t.fixed_delta,
            time_scale: t.time_scale,
            elapsed: t.elapsed,
            unscaled_elapsed: t.unscaled_elapsed,
            frame: t.frame,
        }
    })
    .unwrap_or_default();
    put(out, info);
}

extern "C" fn api_time_set_scale(scale: f32) {
    with_world("time_set_scale", |world| {
        // Как `ctx.set_time_scale`: отрицательное — ноль; нечисло — единица.
        world.time.time_scale = if scale.is_finite() { scale.max(0.0) } else { 1.0 };
    });
}

// ── объекты и иерархия ──────────────────────────────────────────────────────

extern "C" fn api_spawn(name: *const c_char) -> EngineObject {
    let name = cs(name).unwrap_or("GameObject");
    with_world("spawn", |world| to_object(world.scene.spawn(name))).unwrap_or(NO_OBJECT)
}

extern "C" fn api_find_by_tag(tag: *const c_char) -> EngineObject {
    let Some(tag) = cs(tag) else {
        return NO_OBJECT;
    };
    with_world("find_by_tag", |world| world.scene.find_by_tag(tag).map_or(NO_OBJECT, to_object))
        .unwrap_or(NO_OBJECT)
}

extern "C" fn api_find_by_name(name: *const c_char) -> EngineObject {
    let Some(name) = cs(name) else {
        return NO_OBJECT;
    };
    with_world("find_by_name", |world| world.scene.find_by_name(name).map_or(NO_OBJECT, to_object))
        .unwrap_or(NO_OBJECT)
}

extern "C" fn api_find_all_by_tag(tag: *const c_char, out: *mut EngineObject, cap: u32) -> u32 {
    let Some(tag) = cs(tag) else {
        return 0;
    };
    with_world("find_all_by_tag", |world| {
        let found = world.scene.find_all_by_tag(tag);
        put_objects(out, cap, &found);
        found.len() as u32
    })
    .unwrap_or(0)
}

extern "C" fn api_find_with_component(component: *const c_char, out: *mut EngineObject, cap: u32) -> u32 {
    let Some(component) = cs(component) else {
        return 0;
    };
    with_world("find_with_component", |world| {
        let found: Vec<GameObjectId> = world
            .scene
            .ids()
            .into_iter()
            .filter(|id| {
                world
                    .scene
                    .get(*id)
                    .is_some_and(|o| o.components().any(|c| c.type_name() == component))
            })
            .collect();
        put_objects(out, cap, &found);
        found.len() as u32
    })
    .unwrap_or(0)
}

extern "C" fn api_find_child(parent: EngineObject, name: *const c_char) -> EngineObject {
    let Some(name) = cs(name) else {
        return NO_OBJECT;
    };
    with_object("find_child", parent, |world, id| {
        world.scene.find_child(id, name).map_or(NO_OBJECT, to_object)
    })
    .unwrap_or(NO_OBJECT)
}

extern "C" fn api_destroy(object: EngineObject) {
    with_object("destroy", object, |world, id| world.scene.destroy(id));
}

extern "C" fn api_destroy_after(object: EngineObject, delay: f32) -> u64 {
    with_object("destroy_after", object, |world, id| {
        let timer = world.scheduler().destroy_after(id, delay);
        remember_timer(world, timer)
    })
    .unwrap_or(0)
}

extern "C" fn api_instantiate(prefab: *const c_char) -> EngineObject {
    let Some(prefab) = cs(prefab) else {
        return NO_OBJECT;
    };
    with_world("instantiate", |world| match world.instantiate(prefab) {
        Ok(id) => to_object(id),
        Err(err) => {
            complain("instantiate", || format!("instantiate(`{prefab}`): {err}"));
            NO_OBJECT
        }
    })
    .unwrap_or(NO_OBJECT)
}

extern "C" fn api_instantiate_at(
    prefab: *const c_char,
    position: EngineVec3,
    euler_deg: EngineVec3,
    parent: EngineObject,
) -> EngineObject {
    let Some(prefab) = cs(prefab) else {
        return NO_OBJECT;
    };
    with_world("instantiate_at", |world| {
        let parent = (parent.generation != 0).then(|| to_id(parent));
        match world.instantiate_at(prefab, glam3(position), quat_of(glam3(euler_deg)), parent) {
            Ok(id) => to_object(id),
            Err(err) => {
                complain("instantiate_at", || format!("instantiate_at(`{prefab}`): {err}"));
                NO_OBJECT
            }
        }
    })
    .unwrap_or(NO_OBJECT)
}

extern "C" fn api_duplicate(object: EngineObject) -> EngineObject {
    with_object("duplicate", object, |world, id| match world.duplicate(id) {
        Ok(copy) => to_object(copy),
        Err(err) => {
            complain("duplicate", || format!("duplicate: {err}"));
            NO_OBJECT
        }
    })
    .unwrap_or(NO_OBJECT)
}

extern "C" fn api_get_parent(object: EngineObject) -> EngineObject {
    with_object("get_parent", object, |world, id| {
        world.scene.parent_of(id).map_or(NO_OBJECT, to_object)
    })
    .unwrap_or(NO_OBJECT)
}

extern "C" fn api_set_parent(object: EngineObject, parent: EngineObject, keep_world: i32) -> i32 {
    with_object("set_parent", object, |world, id| {
        let parent = (parent.generation != 0).then(|| to_id(parent));
        let result = if keep_world != 0 {
            world.scene.set_parent_keep_world(id, parent)
        } else {
            world.scene.set_parent(id, parent)
        };
        match result {
            Ok(()) => 1,
            Err(err) => {
                complain("set_parent", || format!("set_parent: {err}"));
                0
            }
        }
    })
    .unwrap_or(0)
}

extern "C" fn api_child_count(object: EngineObject) -> u32 {
    with_object("child_count", object, |world, id| {
        world.scene.get(id).map_or(0, |o| o.children().len() as u32)
    })
    .unwrap_or(0)
}

extern "C" fn api_get_child(object: EngineObject, index: u32) -> EngineObject {
    with_object("get_child", object, |world, id| {
        world
            .scene
            .get(id)
            .and_then(|o| o.children().get(index as usize).copied())
            .map_or(NO_OBJECT, to_object)
    })
    .unwrap_or(NO_OBJECT)
}

extern "C" fn api_set_active(object: EngineObject, active: i32) {
    with_object("set_active", object, |world, id| {
        world.scene.set_active(id, active != 0);
    });
}

extern "C" fn api_get_active(object: EngineObject, in_hierarchy: i32) -> i32 {
    with_object("get_active", object, |world, id| {
        if in_hierarchy != 0 {
            world.scene.is_active(id) as i32
        } else {
            world.scene.get(id).is_some_and(|o| o.active) as i32
        }
    })
    .unwrap_or(0)
}

extern "C" fn api_get_name(object: EngineObject, buf: *mut c_char, cap: u32) -> u32 {
    with_object("get_name", object, |world, id| {
        put_text(world.scene.get(id).map_or("", |o| o.name.as_str()), buf, cap)
    })
    .unwrap_or(0)
}

extern "C" fn api_set_name(object: EngineObject, name: *const c_char) {
    let Some(name) = cs(name) else {
        return;
    };
    with_object("set_name", object, |world, id| {
        if let Some(o) = world.scene.get_mut(id) {
            o.name = name.to_string();
        }
    });
}

extern "C" fn api_get_tag(object: EngineObject, buf: *mut c_char, cap: u32) -> u32 {
    with_object("get_tag", object, |world, id| {
        put_text(world.scene.get(id).map_or("", |o| o.tag.as_str()), buf, cap)
    })
    .unwrap_or(0)
}

extern "C" fn api_set_tag(object: EngineObject, tag: *const c_char) {
    let Some(tag) = cs(tag) else {
        return;
    };
    with_object("set_tag", object, |world, id| {
        if let Some(o) = world.scene.get_mut(id) {
            o.tag = tag.to_string();
        }
    });
}

extern "C" fn api_get_layer(object: EngineObject) -> u32 {
    with_object("get_layer", object, |world, id| world.scene.get(id).map_or(0, |o| o.layer)).unwrap_or(0)
}

extern "C" fn api_set_layer(object: EngineObject, layer: u32) {
    with_object("set_layer", object, |world, id| {
        if let Some(o) = world.scene.get_mut(id) {
            o.layer = layer.min(31);
        }
    });
}

extern "C" fn api_dont_destroy_on_load(object: EngineObject) {
    with_object("dont_destroy_on_load", object, |world, id| {
        world.scene.set_persistent(id, true);
    });
}

// ── компоненты и поля ───────────────────────────────────────────────────────

extern "C" fn api_add_component(object: EngineObject, component: *const c_char) -> i32 {
    let Some(kind) = cs(component) else {
        return 0;
    };
    with_object("add_component", object, |world, id| match world.registry.create(kind) {
        Ok(created) => {
            if let Some(o) = world.scene.get_mut(id) {
                o.add_boxed(created);
            }
            1
        }
        Err(err) => {
            complain("add_component", || format!("add_component(`{kind}`): {err}"));
            0
        }
    })
    .unwrap_or(0)
}

extern "C" fn api_remove_component(object: EngineObject, component: *const c_char) -> i32 {
    let Some(kind) = cs(component) else {
        return 0;
    };
    with_object("remove_component", object, |world, id| {
        let Some(o) = world.scene.get_mut(id) else {
            return 0;
        };
        let position = o.components().position(|c| c.type_name() == kind);
        match position {
            Some(index) => o.remove_at(index) as i32,
            None => 0,
        }
    })
    .unwrap_or(0)
}

extern "C" fn api_has_component(object: EngineObject, component: *const c_char) -> i32 {
    let Some(kind) = cs(component) else {
        return 0;
    };
    with_object("has_component", object, |world, id| {
        world.scene.get(id).is_some_and(|o| o.components().any(|c| c.type_name() == kind)) as i32
    })
    .unwrap_or(0)
}

/// Типы компонентов объекта через запятую — для подсказки в ошибке.
fn component_list(world: &World, id: GameObjectId) -> String {
    world
        .scene
        .get(id)
        .map(|o| o.components().map(|c| c.type_name()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

extern "C" fn api_get_field(
    object: EngineObject,
    component: *const c_char,
    field: *const c_char,
    out: *mut EngineValue,
) -> i32 {
    let (Some(component), Some(field)) = (cs(component), cs(field)) else {
        return 0;
    };
    with_object("get_field", object, |world, id| {
        let found = world
            .scene
            .get(id)
            .and_then(|o| o.components().find(|c| c.type_name() == component));
        let Some(found) = found else {
            complain("get_field", || {
                format!(
                    "get_field: the object has no component `{component}` (it has: {}). \
                     Note that a component's own fields are read directly, not through get_field",
                    component_list(world, id)
                )
            });
            return 0;
        };
        let Some(value) = found.get_field(field) else {
            complain("get_field", || {
                format!(
                    "get_field: component `{component}` has no field `{field}` (available fields: {})",
                    found.field_names().join(", ")
                )
            });
            return 0;
        };
        // Поле-ссылка на объект: C++ получает сам объект (0 — не назначено или уничтожен).
        if let Value::Object(uid) = value {
            let mut object = nil_value();
            object.kind = ENGINE_VALUE_OBJECT;
            object.i = world.scene.find_by_uid(uid).map_or(0, |target| target.to_bits() as i64);
            return put(out, object) as i32;
        }
        put(out, to_engine_value(&value)) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_set_field(
    object: EngineObject,
    component: *const c_char,
    field: *const c_char,
    value: *const EngineValue,
) -> i32 {
    let (Some(component), Some(field), Some(new)) = (cs(component), cs(field), read_value(value)) else {
        return 0;
    };
    with_object("set_field", object, |world, id| {
        // Объект из C++ приходит числом (`ENGINE_VALUE_OBJECT` → `Int`): для поля-ссылки
        // это стабильный номер объекта (0 — очистить).
        let as_reference = match new {
            Value::Int(bits) => Some(Value::Object(
                world.scene.get(GameObjectId::from_bits(bits as u64)).map_or(0, |o| o.uid()),
            )),
            Value::Nil => Some(Value::Object(0)),
            _ => None,
        };
        let Some(o) = world.scene.get_mut(id) else {
            return 0;
        };
        let Some(target) = o.components_mut().find(|c| c.type_name() == component) else {
            complain("set_field", || {
                format!(
                    "set_field: the object has no component `{component}`. Add it first with add_component"
                )
            });
            return 0;
        };
        let new = match target.get_field(field) {
            Some(Value::Object(_)) => as_reference.unwrap_or(new),
            Some(current) => coerce_like(&current, new),
            None => new,
        };
        match target.set_field(field, &new) {
            Ok(()) => 1,
            Err(err) => {
                complain("set_field", || format!("set_field: {err}"));
                0
            }
        }
    })
    .unwrap_or(0)
}

// ── сообщения, таймеры, твины ───────────────────────────────────────────────

extern "C" fn api_send_message(object: EngineObject, name: *const c_char, value: f32) {
    let Some(name) = cs(name) else {
        return;
    };
    with_object("send_message", object, |world, id| {
        world.send_message(id, name, Value::Float(value));
    });
}

extern "C" fn api_send_message_value(object: EngineObject, name: *const c_char, value: *const EngineValue) {
    let Some(name) = cs(name) else {
        return;
    };
    let value = read_value(value).unwrap_or(Value::Nil);
    with_object("send_message_value", object, |world, id| {
        world.send_message(id, name, value);
    });
}

extern "C" fn api_send_after(
    object: EngineObject,
    delay: f32,
    name: *const c_char,
    value: *const EngineValue,
) -> u64 {
    let Some(name) = cs(name) else {
        return 0;
    };
    let value = read_value(value).unwrap_or(Value::Nil);
    with_object("send_after", object, |world, id| {
        let timer = world.scheduler().send_after(id, delay, name, value);
        remember_timer(world, timer)
    })
    .unwrap_or(0)
}

extern "C" fn api_send_every(
    object: EngineObject,
    interval: f32,
    name: *const c_char,
    value: *const EngineValue,
) -> u64 {
    let Some(name) = cs(name) else {
        return 0;
    };
    let value = read_value(value).unwrap_or(Value::Nil);
    with_object("send_every", object, |world, id| {
        let timer = world.scheduler().send_every(id, interval, name, value);
        remember_timer(world, timer)
    })
    .unwrap_or(0)
}

extern "C" fn api_cancel_timer(timer: u64) -> i32 {
    with_world("cancel_timer", |world| {
        find_timer(timer).is_some_and(|t| world.scheduler().cancel(t)) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_cancel_timers_of(object: EngineObject) -> u32 {
    with_object("cancel_timers_of", object, |world, id| world.scheduler().cancel_owned_by(id) as u32)
        .unwrap_or(0)
}

extern "C" fn api_tween_field(
    object: EngineObject,
    path: *const c_char,
    to: *const EngineValue,
    params: *const EngineTweenParams,
) -> u64 {
    let (Some(path), Some(to)) = (cs(path), read_value(to)) else {
        return 0;
    };
    // SAFETY: контракт ABI — `params` null либо указывает на валидный `EngineTweenParams`.
    let params = (!params.is_null()).then(|| unsafe { *params });
    with_object("tween_field", object, |world, id| {
        let mut tween = Tween::field(id, path, RawValue(to));
        if let Some(p) = params {
            tween = tween.duration(p.duration).delay(p.delay);
            if let Some(name) = cs(p.ease) {
                match Ease::from_value(&Value::Enum(name.to_string())) {
                    Some(ease) => tween = tween.ease(ease),
                    None => complain("tween_field", || {
                        format!("tween_field: unknown ease `{name}`; use `Linear`, `OutQuad`, `InOutBack`, `OutBounce`… (see docs/api/gameplay.md)")
                    }),
                }
            }
            tween = if p.loops == 0 { tween.forever() } else { tween.loops(p.loops) };
            if p.yoyo != 0 {
                tween = tween.yoyo();
            }
            if p.unscaled != 0 {
                tween = tween.unscaled();
            }
            if let Some(name) = cs(p.on_complete) {
                let target = to_id(p.on_complete_target);
                if p.on_complete_target.generation != 0 && world.scene.contains(target) {
                    tween = tween.on_complete(target, name, Value::Nil);
                } else {
                    complain("tween_field", || {
                        "tween_field: `on_complete` needs a live target object; the message will not be sent".to_string()
                    });
                }
            }
        }
        let started = world.start_tween(tween);
        remember_tween(world, started)
    })
    .unwrap_or(0)
}

extern "C" fn api_tween_cancel(tween: u64) -> i32 {
    with_world("tween_cancel", |world| {
        find_tween(tween).is_some_and(|t| world.tweens().cancel(t)) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_tween_cancel_of(object: EngineObject) -> u32 {
    with_object("tween_cancel_of", object, |world, id| world.tweens().cancel_all_for(id) as u32)
        .unwrap_or(0)
}

// ── сцены и выход ───────────────────────────────────────────────────────────

extern "C" fn api_load_scene(name: *const c_char) {
    let Some(name) = cs(name) else {
        return;
    };
    with_world("load_scene", |world| world.scenes().load(name));
}

extern "C" fn api_load_scene_additive(name: *const c_char) {
    let Some(name) = cs(name) else {
        return;
    };
    with_world("load_scene_additive", |world| world.scenes().load_additive(name));
}

extern "C" fn api_unload_scene(name: *const c_char) {
    let Some(name) = cs(name) else {
        return;
    };
    with_world("unload_scene", |world| world.scenes().unload(name));
}

extern "C" fn api_reload_scene() {
    with_world("reload_scene", |world| world.scenes().reload());
}

extern "C" fn api_scene_name(buf: *mut c_char, cap: u32) -> u32 {
    with_world("scene_name", |world| put_text(&world.scene.name, buf, cap)).unwrap_or(0)
}

extern "C" fn api_quit() {
    with_world("quit", |world| world.request_quit());
}

// ── сохранения ──────────────────────────────────────────────────────────────

fn save_data(world: &mut World) -> &mut SaveData {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !world.resources.contains::<SaveData>() && !WARNED.swap(true, Ordering::Relaxed) {
        log::warn!(
            target: "plugin",
            "SaveData is not installed, using an in-memory one that is lost on exit; \
             insert `SaveData::open(\"YourGame\")` into `world.resources` at startup"
        );
    }
    world.resources.get_or_insert_with(SaveData::in_memory)
}

extern "C" fn api_save_get(key: *const c_char, out: *mut EngineValue) -> i32 {
    let Some(key) = cs(key) else {
        return 0;
    };
    with_world("save_get", |world| match save_data(world).get_value(key) {
        Some(value) => put(out, to_engine_value(value)) as i32,
        None => 0,
    })
    .unwrap_or(0)
}

extern "C" fn api_save_set(key: *const c_char, value: *const EngineValue) -> i32 {
    let (Some(key), Some(value)) = (cs(key), read_value(value)) else {
        return 0;
    };
    with_world("save_set", |world| {
        save_data(world).set(key, &RawValue(value));
        1
    })
    .unwrap_or(0)
}

extern "C" fn api_save_has(key: *const c_char) -> i32 {
    let Some(key) = cs(key) else {
        return 0;
    };
    with_world("save_has", |world| save_data(world).has_key(key) as i32).unwrap_or(0)
}

extern "C" fn api_save_delete(key: *const c_char) -> i32 {
    let Some(key) = cs(key) else {
        return 0;
    };
    with_world("save_delete", |world| save_data(world).delete_key(key) as i32).unwrap_or(0)
}

extern "C" fn api_save_flush() -> i32 {
    with_world("save_flush", |world| match save_data(world).save() {
        Ok(()) => 1,
        Err(err) => {
            complain("save_flush", || format!("save_flush: {err}"));
            0
        }
    })
    .unwrap_or(0)
}

// ── случайные числа ─────────────────────────────────────────────────────────

extern "C" fn api_random_f32() -> f32 {
    with_world("random_f32", |world| world.random().f32()).unwrap_or(0.0)
}

extern "C" fn api_random_range(min: f32, max: f32) -> f32 {
    with_world("random_range", |world| world.random().range(min, max)).unwrap_or(min)
}

extern "C" fn api_random_int(min: i32, max: i32) -> i32 {
    with_world("random_int", |world| world.random().int_range_inclusive(min, max)).unwrap_or(min)
}

extern "C" fn api_random_chance(probability: f32) -> i32 {
    with_world("random_chance", |world| world.random().chance(probability) as i32).unwrap_or(0)
}

extern "C" fn api_random_seed(seed: u64) {
    with_world("random_seed", |world| world.random().seed(seed));
}

extern "C" fn api_random_in_unit_circle() -> EngineVec3 {
    with_world("random_in_unit_circle", |world| {
        let v = world.random().in_unit_circle();
        EngineVec3 { x: v.x, y: v.y, z: 0.0 }
    })
    .unwrap_or(ZERO)
}

extern "C" fn api_random_on_unit_sphere() -> EngineVec3 {
    with_world("random_on_unit_sphere", |world| vec3(world.random().on_unit_sphere())).unwrap_or(ZERO)
}

// ── сервисы ─────────────────────────────────────────────────────────────────

thread_local! {
    static SERVICE_WARNED: std::cell::RefCell<(HashSet<String>, HashMap<String, u32>)> = Default::default();
}

extern "C" fn api_service_available(name: *const c_char) -> i32 {
    let Some(name) = cs(name) else {
        return 0;
    };
    with_world_quiet(|world| {
        world.resources.get::<CppServices>().is_some_and(|s| s.contains(name)) as i32
    })
    .unwrap_or(0)
}

extern "C" fn api_service_call(
    name: *const c_char,
    args: *const EngineValue,
    arg_count: u32,
    out: *mut EngineValue,
    out_cap: u32,
    out_count: *mut u32,
) -> i32 {
    put(out_count, 0);
    let Some(name) = cs(name) else {
        return ENGINE_SERVICE_BAD_ARGS;
    };
    if !in_hook() {
        complain("service_call", || {
            format!("service `{name}` was called outside of a component hook; call services from on_start/on_update/…")
        });
        return ENGINE_SERVICE_NO_WORLD;
    }
    let args: Vec<Value> = (0..arg_count as usize)
        .map(|i| {
            if args.is_null() {
                return Value::Nil;
            }
            // SAFETY: контракт ABI — `args` указывает на `arg_count` валидных значений.
            read_value(unsafe { args.add(i) }).unwrap_or(Value::Nil)
        })
        .collect();

    with_world_quiet(|world| {
        let handler = world.resources.get::<CppServices>().and_then(|s| s.get(name));
        let Some(handler) = handler else {
            let first = SERVICE_WARNED.with(|w| w.borrow_mut().0.insert(name.to_string()));
            if first {
                log::error!(
                    target: "plugin",
                    "C++ called service `{name}`, but the engine has no such service, so the call \
                     returns a neutral value. Services are installed by `engine::app()` (module \
                     `engine::cpp_services`); an App built from `engine_core::App::new()` has none"
                );
            }
            return ENGINE_SERVICE_MISSING;
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(world, &args)));
        match result {
            Ok(Ok(results)) => {
                let written = results.len().min(out_cap as usize);
                if !out.is_null() {
                    for (i, value) in results.iter().take(written).enumerate() {
                        // SAFETY: контракт ABI — `out` указывает на `out_cap` значений, `i < out_cap`.
                        unsafe { out.add(i).write(to_engine_value(value)) };
                    }
                }
                put(out_count, written as u32);
                ENGINE_SERVICE_OK
            }
            Ok(Err(message)) => {
                let shown = SERVICE_WARNED.with(|w| {
                    let mut w = w.borrow_mut();
                    let n = w.1.entry(name.to_string()).or_insert(0);
                    *n += 1;
                    *n
                });
                if shown <= 5 {
                    log::error!(target: "plugin", "service `{name}`: {message}");
                }
                ENGINE_SERVICE_BAD_ARGS
            }
            Err(_) => {
                log::error!(target: "plugin", "service `{name}` panicked; the call is ignored");
                ENGINE_SERVICE_BAD_ARGS
            }
        }
    })
    .unwrap_or(ENGINE_SERVICE_NO_WORLD)
}

/// Таблица сервисов, которую движок отдаёт каждому плагину.
pub(crate) static HOST_API: EngineApi = EngineApi {
    abi_version: ENGINE_ABI_VERSION,
    log: api_log,
    is_alive: api_is_alive,
    get_position: api_get_position,
    set_position: api_set_position,
    get_rotation: api_get_rotation,
    set_rotation: api_set_rotation,
    get_scale: api_get_scale,
    set_scale: api_set_scale,
    get_world_position: api_get_world_position,
    set_world_position: api_set_world_position,
    get_world_rotation: api_get_world_rotation,
    set_world_rotation: api_set_world_rotation,
    get_direction: api_get_direction,
    look_at: api_look_at,
    input_axis: api_input_axis,
    input_button: api_input_button,
    input_button_down: api_input_button_down,
    input_button_up: api_input_button_up,
    input_button_value: api_input_button_value,
    input_key: api_input_key,
    input_mouse_button: api_input_mouse_button,
    input_vector: api_input_vector,
    input_gamepad_count: api_input_gamepad_count,
    input_gamepad_button: api_input_gamepad_button,
    input_gamepad_axis: api_input_gamepad_axis,
    input_gamepad_stick: api_input_gamepad_stick,
    input_touch_count: api_input_touch_count,
    input_touch: api_input_touch,
    input_text: api_input_text,
    time_seconds: api_time_seconds,
    time_info: api_time_info,
    time_set_scale: api_time_set_scale,
    spawn: api_spawn,
    find_by_tag: api_find_by_tag,
    find_by_name: api_find_by_name,
    find_all_by_tag: api_find_all_by_tag,
    find_with_component: api_find_with_component,
    find_child: api_find_child,
    destroy: api_destroy,
    destroy_after: api_destroy_after,
    instantiate: api_instantiate,
    instantiate_at: api_instantiate_at,
    duplicate: api_duplicate,
    get_parent: api_get_parent,
    set_parent: api_set_parent,
    child_count: api_child_count,
    get_child: api_get_child,
    set_active: api_set_active,
    get_active: api_get_active,
    get_name: api_get_name,
    set_name: api_set_name,
    get_tag: api_get_tag,
    set_tag: api_set_tag,
    get_layer: api_get_layer,
    set_layer: api_set_layer,
    dont_destroy_on_load: api_dont_destroy_on_load,
    add_component: api_add_component,
    remove_component: api_remove_component,
    has_component: api_has_component,
    get_field: api_get_field,
    set_field: api_set_field,
    send_message: api_send_message,
    send_message_value: api_send_message_value,
    send_after: api_send_after,
    send_every: api_send_every,
    cancel_timer: api_cancel_timer,
    cancel_timers_of: api_cancel_timers_of,
    tween_field: api_tween_field,
    tween_cancel: api_tween_cancel,
    tween_cancel_of: api_tween_cancel_of,
    load_scene: api_load_scene,
    load_scene_additive: api_load_scene_additive,
    unload_scene: api_unload_scene,
    reload_scene: api_reload_scene,
    scene_name: api_scene_name,
    quit: api_quit,
    save_get: api_save_get,
    save_set: api_save_set,
    save_has: api_save_has,
    save_delete: api_save_delete,
    save_flush: api_save_flush,
    random_f32: api_random_f32,
    random_range: api_random_range,
    random_int: api_random_int,
    random_chance: api_random_chance,
    random_seed: api_random_seed,
    random_in_unit_circle: api_random_in_unit_circle,
    random_on_unit_sphere: api_random_on_unit_sphere,
    service_available: api_service_available,
    service_call: api_service_call,
};
