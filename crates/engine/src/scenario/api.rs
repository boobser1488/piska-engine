//! Функции движка, доступные сценариям: объекты, сообщения, время, ввод, звук,
//! интерфейс, сцены и сохранения. Хост встроенных функций — `World`.

use std::collections::HashMap;

use engine_assets::{AssetDatabase, Guid};
use engine_audio::{Audio, ClipRef, PlayParams};
use engine_core::prelude::{Color, Ease, GameObjectId, Key, MouseButton, Quat, SaveData, Tween, TweenId, Vec2, Vec3, World};
use engine_core::reflect::Value as EValue;
use engine_scenario::{clean, Args, Handle, Key as MapKey, Map, NativeCtx, NativeResult, OrderedMap, Suspend, Throw, Value, Vm};
use engine_ui::{Slider, Text, UiContext};

use super::ScenarioMessage;

/// Ожидания, которые проверяет движок (`move_to`, `wait_click`).
#[derive(Default)]
pub(crate) struct HostWaits {
    next: u64,
    tweens: HashMap<u64, TweenId>,
}

pub(crate) fn world<'a>(ctx: &'a mut NativeCtx) -> Result<&'a mut World, Throw> {
    ctx.host::<World>().ok_or_else(|| Throw::msg("this function needs the game world (it only works inside a running scenario)"))
}

pub(crate) fn handle(class: u32, id: GameObjectId) -> Value {
    Value::Handle(Handle { class, id: id.to_bits() })
}

/// Объект из аргумента: ручка живого объекта или имя объекта сцены.
fn object_arg(world: &World, class: u32, args: &Args, i: usize) -> Result<GameObjectId, Throw> {
    match args.get(i) {
        Value::Handle(h) if h.class == class => {
            let id = GameObjectId::from_bits(h.id);
            if world.scene.contains(id) {
                Ok(id)
            } else {
                Err(Throw::msg(format!("`{}`: this object was destroyed", args.name)))
            }
        }
        Value::Str(name) => world
            .scene
            .find_by_name(name)
            .ok_or_else(|| Throw::msg(format!("`{}`: there is no object named `{name}` in the scene", args.name))),
        _ => Err(args.bad(i, "an object (from `find`) or an object name")),
    }
}

fn live(world: &World, id: u64) -> Result<GameObjectId, Throw> {
    let id = GameObjectId::from_bits(id);
    if world.scene.contains(id) {
        Ok(id)
    } else {
        Err(Throw::msg("this object was destroyed"))
    }
}

/// Значение движка → значение скрипта.
pub fn to_script(v: &EValue) -> Value {
    match v {
        EValue::Nil => Value::Nil,
        EValue::Bool(b) => Value::Bool(*b),
        EValue::Int(i) => Value::Num(*i as f64),
        EValue::Float(f) => Value::Num(clean(*f)),
        EValue::Str(s) | EValue::Enum(s) | EValue::Asset(s) => Value::str(s),
        EValue::Vec2(x, y) => Value::Vec2(*x, *y),
        EValue::Vec3(x, y, z) => Value::Vec3(*x, *y, *z),
        EValue::Quat(x, y, z, w) => Value::list(vec![Value::Num(clean(*x)), Value::Num(clean(*y)), Value::Num(clean(*z)), Value::Num(clean(*w))]),
        EValue::Color(r, g, b, a) => Value::color(*r, *g, *b, *a),
        EValue::List(items) => Value::list(items.iter().map(to_script).collect()),
        // Ссылку на объект без сцены не разрешить: ручкой её делает `get_path`.
        EValue::Object(_) => Value::Nil,
    }
}

/// Значение движка → значение скрипта; поля-ссылки (`ObjectRef`) — ручки объектов
/// класса `class` (nil, если не назначено или объект уничтожен).
fn to_script_in(world: &World, class: u32, v: &EValue) -> Value {
    match v {
        EValue::Object(uid) => world.scene.find_by_uid(*uid).map_or(Value::Nil, |id| handle(class, id)),
        EValue::List(items) => Value::list(items.iter().map(|item| to_script_in(world, class, item)).collect()),
        other => to_script(other),
    }
}

/// Значение скрипта → значение движка того же вида, что `like` (тип поля).
pub fn from_script(v: &Value, like: Option<&EValue>) -> Result<EValue, String> {
    let num = |v: &Value| v.as_num().ok_or_else(|| format!("expected a number, got {}", v.describe()));
    Ok(match (like, v) {
        (Some(EValue::Int(_)), _) => EValue::Int(num(v)?.round() as i64),
        (Some(EValue::Float(_)), _) => EValue::Float(num(v)? as f32),
        (Some(EValue::Bool(_)), _) => EValue::Bool(v.truthy()),
        (Some(EValue::Enum(_)), Value::Str(s)) => EValue::Enum(s.to_string()),
        (Some(EValue::Asset(_)), Value::Str(s)) => EValue::Asset(s.to_string()),
        (Some(EValue::Asset(_)), Value::Nil) => EValue::Asset(String::new()),
        (Some(EValue::Str(_)), other) => EValue::Str(other.to_text()),
        (Some(EValue::Vec3(..)), Value::Vec2(x, y)) => EValue::Vec3(*x, *y, 0.0),
        (Some(EValue::Vec2(..)), Value::Vec3(x, y, _)) => EValue::Vec2(*x, *y),
        (Some(EValue::Color(..)), Value::Str(s)) => {
            let c = parse_hex(s).ok_or_else(|| format!("`{s}` is not a color — write \"#ff8800\""))?;
            EValue::Color(c[0], c[1], c[2], c[3])
        }
        (Some(EValue::List(items)), Value::List(l)) => {
            let first = items.first();
            EValue::List(l.to_vec().iter().map(|x| from_script(x, first)).collect::<Result<_, _>>()?)
        }
        (_, Value::Nil) => EValue::Nil,
        (_, Value::Bool(b)) => EValue::Bool(*b),
        (_, Value::Num(n)) => EValue::Float(*n as f32),
        (_, Value::Str(s)) => EValue::Str(s.to_string()),
        (_, Value::Vec2(x, y)) => EValue::Vec2(*x, *y),
        (_, Value::Vec3(x, y, z)) => EValue::Vec3(*x, *y, *z),
        (_, Value::Color(c)) => EValue::Color(c[0], c[1], c[2], c[3]),
        (_, Value::List(l)) => EValue::List(l.to_vec().iter().map(|x| from_script(x, None)).collect::<Result<_, _>>()?),
        (_, other) => return Err(format!("{} can't be stored in a component field", other.describe())),
    })
}

fn parse_hex(s: &str) -> Option<[f32; 4]> {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|b| b as f32 / 255.0);
    match h.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 1.0]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

fn vec3_of(v: &Value) -> Option<Vec3> {
    match v {
        Value::Vec3(x, y, z) => Some(Vec3::new(*x, *y, *z)),
        Value::Vec2(x, y) => Some(Vec3::new(*x, *y, 0.0)),
        _ => None,
    }
}

/// Имя клавиши: `"space"`, `"A"`, `"left"`, `"enter"`, `"1"`, `"f5"`.
pub fn key_by_name(name: &str) -> Option<Key> {
    let n: String = name.trim().to_ascii_lowercase().chars().filter(|c| *c != '_' && *c != ' ').collect();
    let n = match n.as_str() {
        "arrowleft" => "left".to_string(),
        "arrowright" => "right".to_string(),
        "arrowup" => "up".to_string(),
        "arrowdown" => "down".to_string(),
        "return" => "enter".to_string(),
        "esc" => "escape".to_string(),
        "shift" => "lshift".to_string(),
        "ctrl" | "control" => "lctrl".to_string(),
        "alt" => "lalt".to_string(),
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => format!("digit{d}"),
        other => other.to_string(),
    };
    Key::ALL.into_iter().find(|k| format!("{k:?}").to_ascii_lowercase() == n)
}

/// Имя сглаживания: `"out_quad"`, `"OutQuad"`, `"inOutSine"`, `"linear"`.
pub fn ease_by_name(name: &str) -> Option<Ease> {
    let n: String = name.chars().filter(|c| *c != '_' && *c != '-' && *c != ' ').collect::<String>().to_ascii_lowercase();
    Ease::ALL.into_iter().find(|e| format!("{e:?}").to_ascii_lowercase() == n)
}

fn ease_arg(args: &Args, i: usize) -> Result<Ease, Throw> {
    match args.get(i) {
        Value::Nil => Ok(Ease::InOutSine),
        Value::Str(s) => ease_by_name(s).ok_or_else(|| {
            Throw::msg(format!("unknown ease `{s}` — use linear, in_quad, out_quad, in_out_sine, out_back, out_bounce, …"))
        }),
        _ => Err(args.bad(i, "an ease name like \"out_quad\"")),
    }
}

/// Значение для твина: число, вектор или цвет.
fn tween_to(target: GameObjectId, path: &str, v: &Value) -> Result<Tween, Throw> {
    Ok(match v {
        Value::Num(n) => Tween::field(target, path, *n as f32),
        Value::Vec2(x, y) => Tween::field(target, path, Vec2::new(*x, *y)),
        Value::Vec3(x, y, z) => Tween::field(target, path, Vec3::new(*x, *y, *z)),
        Value::Color(c) => Tween::field(target, path, Color { r: c[0], g: c[1], b: c[2], a: c[3] }),
        other => return Err(Throw::msg(format!("can't tween to {} — use a number, vector or color", other.describe()))),
    })
}

fn split_path(path: &str) -> Result<(&str, &str), Throw> {
    path.split_once('.')
        .filter(|(c, f)| !c.is_empty() && !f.is_empty())
        .ok_or_else(|| Throw::msg(format!("expected \"Component.field\", got \"{path}\" (for example \"Light.color\")")))
}

fn get_path(world: &World, class: u32, id: GameObjectId, path: &str) -> Result<Value, Throw> {
    let (component, field) = split_path(path)?;
    let obj = world.scene.get(id).ok_or_else(|| Throw::msg("this object was destroyed"))?;
    let c = obj.components().find(|c| c.type_name() == component).ok_or_else(|| {
        let names: Vec<&str> = obj.components().map(|c| c.type_name()).collect();
        Throw::msg(format!("`{}` has no component `{component}` (it has: {})", obj.name, names.join(", ")))
    })?;
    let v = c.get_field(field).ok_or_else(|| {
        Throw::msg(format!("component `{component}` has no field `{field}` (fields: {})", c.field_names().join(", ")))
    })?;
    Ok(to_script_in(world, class, &v))
}

fn set_path(world: &mut World, class: u32, id: GameObjectId, path: &str, v: &Value) -> Result<(), Throw> {
    let (component, field) = split_path(path)?;
    // Ручка объекта (или nil) для поля-ссылки — стабильный номер объекта.
    let reference = match v {
        Value::Handle(h) if h.class == class => Some(EValue::Object(world.scene.get(live(world, h.id)?).map_or(0, |o| o.uid()))),
        Value::Nil => Some(EValue::Object(0)),
        _ => None,
    };
    let obj = world.scene.get_mut(id).ok_or_else(|| Throw::msg("this object was destroyed"))?;
    let name = obj.name.clone();
    let c = obj
        .components_mut()
        .find(|c| c.type_name() == component)
        .ok_or_else(|| Throw::msg(format!("`{name}` has no component `{component}`")))?;
    let like = c.get_field(field).ok_or_else(|| {
        Throw::msg(format!("component `{component}` has no field `{field}` (fields: {})", c.field_names().join(", ")))
    })?;
    let value = match (&like, reference) {
        (EValue::Object(_), Some(reference)) => reference,
        (EValue::Object(_), None) => return Err(Throw::msg(format!("`{path}`: expected an object (from `find`) or nil"))),
        _ => from_script(v, Some(&like)).map_err(|e| Throw::msg(format!("`{path}`: {e}")))?,
    };
    c.set_field(field, &value).map_err(|e| Throw::msg(format!("`{path}`: {e}")))
}

/// Начать твин и вернуть токен ожидания его конца.
fn tween_wait(world: &mut World, tween: Tween) -> Value {
    let id = world.start_tween(tween);
    let waits = world.resources.get_or_insert_with(HostWaits::default);
    waits.next += 1;
    let n = waits.next;
    waits.tweens.insert(n, id);
    Value::list(vec![Value::str("tween"), Value::Num(n as f64)])
}

/// Проверка ожиданий движка (вызывает группа задач каждый кадр).
pub(crate) fn poll(world: &mut World, token: &Value) -> Option<Value> {
    let Value::List(l) = token else { return Some(Value::Nil) };
    let kind = l.get(0).unwrap_or_default();
    match kind.as_str() {
        Some("tween") => {
            let n = l.get(1).and_then(|v| v.as_num()).unwrap_or(0.0) as u64;
            let tid = world.resources.get::<HostWaits>().and_then(|w| w.tweens.get(&n).copied());
            match tid {
                Some(tid) if world.tweens().is_running(tid) => None,
                _ => {
                    if let Some(w) = world.resources.get_mut::<HostWaits>() {
                        w.tweens.remove(&n);
                    }
                    Some(Value::Nil)
                }
            }
        }
        Some("click") => {
            let name = l.get(1).unwrap_or_default();
            world.ui().clicked(name.as_str().unwrap_or("")).then_some(Value::Bool(true))
        }
        _ => Some(Value::Nil),
    }
}

fn audio(world: &mut World) -> Result<&mut Audio, Throw> {
    world.resources.get_mut::<Audio>().ok_or_else(|| Throw::msg("sound is not available (the Audio resource is not installed)"))
}

/// Клип по имени (`audio.add_clip`), пути ассета (`sounds/hit.wav`) или GUID.
fn clip(world: &mut World, name: &str) -> Result<ClipRef, Throw> {
    if audio(world)?.has_clip(name) {
        return Ok(ClipRef::Name(name.to_string()));
    }
    if let Some(db) = world.resources.get::<AssetDatabase>() {
        for candidate in [name.to_string(), format!("sounds/{name}"), format!("audio/{name}")] {
            if let Some(guid) = db.guid_of(&candidate) {
                return Ok(ClipRef::Asset(guid));
            }
        }
    }
    if let Some(guid) = Guid::parse(name) {
        return Ok(ClipRef::Asset(guid));
    }
    Err(Throw::msg(format!(
        "no sound `{name}` — use a clip added with `audio.add_clip` or an asset path like \"sounds/hit.wav\""
    )))
}

fn ui_object(world: &World, args: &Args) -> Result<GameObjectId, Throw> {
    let name = args.str(0)?;
    world.scene.find_by_name(name).ok_or_else(|| Throw::msg(format!("`{}`: there is no UI object named `{name}`", args.name)))
}

fn self_id(ctx: &NativeCtx) -> GameObjectId {
    GameObjectId::from_bits(ctx.user())
}

fn nsmap(entries: Vec<(&str, Value)>) -> Value {
    let mut m = OrderedMap::default();
    for (k, v) in entries {
        m.insert(MapKey::Str(k.into()), v);
    }
    Value::Map(Map::new(m))
}

fn native(name: &str, f: impl Fn(&mut NativeCtx, Args) -> NativeResult + 'static) -> Value {
    engine_scenario::function(name, f)
}

/// Ставит в ВМ все функции движка. Возвращает номер класса ручек объектов.
pub(crate) fn install(vm: &mut Vm) -> u32 {
    let obj = vm.register_class("object");
    let h = move |id: GameObjectId| handle(obj, id);

    // ---- вывод ----
    for (name, warn) in [("print", false), ("log", false), ("warn", true)] {
        vm.register(name, move |ctx, a| {
            let text = a.values.iter().map(Value::to_text).collect::<Vec<_>>().join(" ");
            let id = self_id(ctx);
            let object = ctx.host::<World>().and_then(|w| w.scene.get(id).map(|o| o.name.clone())).unwrap_or_default();
            let line = if object.is_empty() { text } else { format!("[{object}] {text}") };
            if warn {
                log::warn!(target: "scenario", "{line}");
            } else {
                log::info!(target: "scenario", "{line}");
            }
            Ok(Value::Nil)
        });
    }

    // ---- объекты ----
    vm.register("find", move |ctx, a| {
        let name = a.str(0)?.clone();
        let w = world(ctx)?;
        Ok(w.scene.find_by_name(&name).map(h).unwrap_or_default())
    });
    vm.register("find_tag", move |ctx, a| {
        let tag = a.str(0)?.clone();
        let w = world(ctx)?;
        Ok(w.scene.find_by_tag(&tag).map(h).unwrap_or_default())
    });
    vm.register("find_all_tag", move |ctx, a| {
        let tag = a.str(0)?.clone();
        let w = world(ctx)?;
        Ok(Value::list(w.scene.find_all_by_tag(&tag).into_iter().map(h).collect()))
    });
    vm.register("spawn", move |ctx, a| {
        let what = a.str(0)?.clone();
        let pos = match a.get(1) {
            v @ (Value::Vec2(..) | Value::Vec3(..)) => vec3_of(v).unwrap_or_default(),
            Value::Nil => Vec3::ZERO,
            _ => Vec3::new(a.num(1)? as f32, a.opt_num(2, 0.0)? as f32, a.opt_num(3, 0.0)? as f32),
        };
        let w = world(ctx)?;
        let id = spawn(w, &what, pos)?;
        Ok(h(id))
    });
    vm.register("destroy", move |ctx, a| {
        let w = world(ctx)?;
        let id = match a.get(0) {
            Value::Nil => return Ok(Value::Nil),
            _ => object_arg(w, obj, &a, 0)?,
        };
        let delay = a.opt_num(1, 0.0)?;
        if delay > 0.0 {
            w.scheduler().destroy_after(id, delay as f32);
        } else {
            w.scene.destroy(id);
        }
        Ok(Value::Nil)
    });

    // ---- свойства объекта ----
    vm.class_getter(obj, move |ctx, bits, name| {
        let w = world(ctx)?;
        let id = live(w, bits)?;
        let o = w.scene.get(id).ok_or_else(|| Throw::msg("this object was destroyed"))?;
        let t = &o.transform;
        Ok(Some(match name {
            "name" => Value::str(&o.name),
            "tag" => Value::str(&o.tag),
            "active" => Value::Bool(o.active),
            "position" => Value::Vec3(t.position.x, t.position.y, t.position.z),
            "world_position" => {
                let p = w.scene.world_position(id);
                Value::Vec3(p.x, p.y, p.z)
            }
            "rotation" => {
                let e = t.euler_deg();
                Value::Vec3(e.x, e.y, e.z)
            }
            "scale" => Value::Vec3(t.scale.x, t.scale.y, t.scale.z),
            "parent" => o.parent().map(h).unwrap_or_default(),
            "id" => Value::Num(bits as f64),
            "exists" => Value::Bool(true),
            _ => return Ok(None),
        }))
    });
    vm.class_setter(obj, move |ctx, bits, name, v| {
        let w = world(ctx)?;
        let id = live(w, bits)?;
        match name {
            "parent" => {
                let parent = match &v {
                    Value::Nil => None,
                    Value::Handle(p) if p.class == obj => Some(live(w, p.id)?),
                    other => return Err(Throw::msg(format!("`parent` must be an object or nil, got {}", other.describe()))),
                };
                w.scene.set_parent(id, parent).map_err(|e| Throw::msg(e.to_string()))?;
                return Ok(true);
            }
            "active" => {
                w.scene.set_active(id, v.truthy());
                return Ok(true);
            }
            _ => {}
        }
        let o = w.scene.get_mut(id).ok_or_else(|| Throw::msg("this object was destroyed"))?;
        let want_vec = |v: &Value, what: &str| vec3_of(v).ok_or_else(|| Throw::msg(format!("`{what}` must be a vec3 or vec2, got {}", v.describe())));
        match name {
            "name" => o.name = v.to_text(),
            "tag" => o.tag = v.to_text(),
            "position" => {
                let p = want_vec(&v, "position")?;
                let z = if matches!(v, Value::Vec2(..)) { o.transform.position.z } else { p.z };
                o.transform.position = Vec3::new(p.x, p.y, z);
            }
            "rotation" => match v {
                Value::Num(deg) => o.transform.rotation = Quat::from_rotation_z((deg as f32).to_radians()),
                other => o.transform.set_euler_deg(want_vec(&other, "rotation")?),
            },
            "scale" => match v {
                Value::Num(s) => o.transform.scale = Vec3::splat(s as f32),
                other => {
                    let s = want_vec(&other, "scale")?;
                    let z = if matches!(other, Value::Vec2(..)) { o.transform.scale.z } else { s.z };
                    o.transform.scale = Vec3::new(s.x, s.y, z);
                }
            },
            _ => return Ok(false),
        }
        Ok(true)
    });

    // ---- методы объекта ----
    vm.class_method(obj, "children", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let kids = w.scene.get(id).map(|o| o.children().to_vec()).unwrap_or_default();
        Ok(Value::list(kids.into_iter().map(h).collect()))
    });
    vm.class_method(obj, "find", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let name = a.str(1)?;
        Ok(w.scene.find_child(id, name).map(h).unwrap_or_default())
    });
    vm.class_method(obj, "has", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let comp = a.str(1)?;
        Ok(Value::Bool(w.scene.get(id).is_some_and(|o| o.components().any(|c| c.type_name() == &**comp))))
    });
    vm.class_method(obj, "get", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        get_path(w, obj, id, a.str(1)?)
    });
    vm.class_method(obj, "set", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        set_path(w, obj, id, a.str(1)?, a.get(2))?;
        Ok(Value::Nil)
    });
    vm.class_method(obj, "send", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let name = a.str(1)?;
        let value = from_script(a.get(2), None).map_err(Throw::Msg)?;
        w.send_message(id, name, value);
        Ok(Value::Nil)
    });
    vm.class_method(obj, "destroy", move |ctx, a| {
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let delay = a.opt_num(1, 0.0)?;
        if delay > 0.0 {
            w.scheduler().destroy_after(id, delay as f32);
        } else {
            w.scene.destroy(id);
        }
        Ok(Value::Nil)
    });
    vm.class_method(obj, "move_to", move |ctx, a| {
        // move_to(x, y[, z], seconds[, ease]) или move_to(vec, seconds[, ease])
        let (target, rest) = match a.get(1) {
            v @ (Value::Vec2(..) | Value::Vec3(..)) => (v.clone(), 2),
            _ => {
                let x = a.num(1)?;
                let y = a.num(2)?;
                // Третье число — z, если за ним есть ещё число (секунды).
                if matches!(a.get(4), Value::Num(_)) {
                    (Value::Vec3(x as f32, y as f32, a.num(3)? as f32), 4)
                } else {
                    (Value::Vec2(x as f32, y as f32), 3)
                }
            }
        };
        let seconds = a.num(rest)?.max(0.0) as f32;
        let ease = ease_arg(&a, rest + 1)?;
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let cur = w.scene.get(id).map(|o| o.transform.position).unwrap_or_default();
        let to = match target {
            Value::Vec2(x, y) => Vec3::new(x, y, cur.z),
            other => vec3_of(&other).unwrap_or(cur),
        };
        let token = tween_wait(w, Tween::position(id, to).duration(seconds).ease(ease));
        ctx.suspend(Suspend::Host(token));
        Ok(Value::Nil)
    });
    vm.class_method(obj, "tween", move |ctx, a| {
        let path = a.str(1)?.clone();
        let seconds = a.num(3)?.max(0.0) as f32;
        let ease = ease_arg(&a, 4)?;
        let w = world(ctx)?;
        let id = object_arg(w, obj, &a, 0)?;
        let tween = tween_to(id, &path, a.get(2))?.duration(seconds).ease(ease);
        w.start_tween(tween);
        Ok(Value::Nil)
    });

    // ---- сообщения ----
    vm.register("emit", move |ctx, a| {
        let name = a.str(0)?.clone();
        let value = from_script(a.get(1), None).map_err(Throw::Msg)?;
        let me = self_id(ctx);
        let w = world(ctx)?;
        let mut cur = Some(me);
        while let Some(id) = cur {
            if !w.scene.contains(id) {
                break;
            }
            w.send_message(id, &name, value.clone());
            cur = w.scene.get(id).and_then(|o| o.parent());
        }
        Ok(Value::Nil)
    });
    vm.register("broadcast", move |ctx, a| {
        let name = a.str(0)?.clone();
        let value = from_script(a.get(1), None).map_err(Throw::Msg)?;
        let w = world(ctx)?;
        for id in w.scene.find_with::<super::ScenarioRunner>() {
            w.send_message(id, &name, value.clone());
        }
        w.events.emit(ScenarioMessage { name: name.to_string(), value });
        Ok(Value::Nil)
    });

    // ---- время ----
    vm.register("time", |ctx, _| Ok(Value::Num(world(ctx)?.time.elapsed)));
    vm.register("dt", |ctx, _| Ok(Value::Num(world(ctx)?.time.delta as f64)));
    vm.register("time_scale", |ctx, a| {
        let w = world(ctx)?;
        if a.has(0) {
            w.time.time_scale = a.num(0)?.max(0.0) as f32;
        }
        Ok(Value::Num(w.time.time_scale as f64))
    });

    // ---- ввод ----
    let key_arg = |a: &Args| -> Result<Key, Throw> {
        let name = a.str(0)?;
        key_by_name(name).ok_or_else(|| Throw::msg(format!("unknown key `{name}` — use names like \"space\", \"a\", \"left\", \"enter\", \"1\", \"f5\"")))
    };
    vm.register("key", move |ctx, a| {
        let k = key_arg(&a)?;
        Ok(Value::Bool(world(ctx)?.input.key(k)))
    });
    vm.register("key_down", move |ctx, a| {
        let k = key_arg(&a)?;
        Ok(Value::Bool(world(ctx)?.input.key_down(k)))
    });
    vm.register("axis", |ctx, a| {
        let name = a.str(0)?.clone();
        Ok(Value::Num(world(ctx)?.input.axis(&name) as f64))
    });
    vm.register("action", |ctx, a| {
        let name = a.str(0)?.clone();
        Ok(Value::Bool(world(ctx)?.input.button(&name)))
    });
    vm.register("action_down", |ctx, a| {
        let name = a.str(0)?.clone();
        Ok(Value::Bool(world(ctx)?.input.button_down(&name)))
    });
    // Те же имена, что у коротких форм скриптов на Rust (`ctx.pressed` / `held` / `released`,
    // `ctx.axis2`, `ctx.rand_range`): один словарь для обоих языков.
    vm.register("pressed", |ctx, a| {
        let name = a.str(0)?.clone();
        Ok(Value::Bool(world(ctx)?.input.button_down(&name)))
    });
    vm.register("held", |ctx, a| {
        let name = a.str(0)?.clone();
        Ok(Value::Bool(world(ctx)?.input.button(&name)))
    });
    vm.register("released", |ctx, a| {
        let name = a.str(0)?.clone();
        Ok(Value::Bool(world(ctx)?.input.button_up(&name)))
    });
    vm.register("axis2", |ctx, a| {
        let (x, y) = (a.str(0)?.clone(), a.str(1)?.clone());
        let input = &world(ctx)?.input;
        let v = engine_core::math::Vec2::new(input.axis(&x), input.axis(&y));
        let v = if v.length_squared() > 1.0 { v.normalize() } else { v };
        Ok(Value::Vec2(v.x, v.y))
    });
    vm.register("mouse", |ctx, _| {
        let p = world(ctx)?.input.mouse_position();
        Ok(Value::Vec2(p.x, p.y))
    });
    vm.register("mouse_down", |ctx, a| {
        let button = match a.opt_str(0)?.map(|s| s.to_ascii_lowercase()) {
            None => MouseButton::Left,
            Some(s) if s == "left" => MouseButton::Left,
            Some(s) if s == "right" => MouseButton::Right,
            Some(s) if s == "middle" => MouseButton::Middle,
            Some(s) => return Err(Throw::msg(format!("unknown mouse button `{s}` — use \"left\", \"right\" or \"middle\""))),
        };
        Ok(Value::Bool(world(ctx)?.input.mouse_button_down(button)))
    });

    // ---- звук ----
    vm.register("play", |ctx, a| {
        let name = a.str(0)?.clone();
        let volume = a.opt_num(1, 1.0)? as f32;
        let w = world(ctx)?;
        let c = clip(w, &name)?;
        audio(w)?.play_with(c, PlayParams::new().volume(volume));
        Ok(Value::Nil)
    });
    vm.register("music", |ctx, a| {
        let name = a.str(0)?.clone();
        let fade = a.opt_num(1, 1.0)? as f32;
        let w = world(ctx)?;
        let c = clip(w, &name)?;
        audio(w)?.play_music(c, fade);
        Ok(Value::Nil)
    });
    vm.register("stop_music", |ctx, a| {
        let fade = a.opt_num(0, 1.0)? as f32;
        audio(world(ctx)?)?.stop_music(fade);
        Ok(Value::Nil)
    });

    // ---- интерфейс ----
    let ui = nsmap(vec![
        ("text", native("text", |ctx, a| {
            let w = world(ctx)?;
            let id = ui_object(w, &a)?;
            let text = w.scene.get_component_mut::<Text>(id).ok_or_else(|| Throw::msg(format!("`{}` has no Text component", a.get(0).to_text())))?;
            if a.len() > 1 {
                text.set(&a.get(1).to_text());
            }
            Ok(Value::str(&text.text))
        })),
        ("show", native("show", |ctx, a| {
            let w = world(ctx)?;
            let id = ui_object(w, &a)?;
            w.scene.set_active(id, true);
            Ok(Value::Nil)
        })),
        ("hide", native("hide", |ctx, a| {
            let w = world(ctx)?;
            let id = ui_object(w, &a)?;
            w.scene.set_active(id, false);
            Ok(Value::Nil)
        })),
        ("value", native("value", |ctx, a| {
            let w = world(ctx)?;
            let id = ui_object(w, &a)?;
            let slider = w.scene.get_component_mut::<Slider>(id).ok_or_else(|| Throw::msg(format!("`{}` has no Slider component", a.get(0).to_text())))?;
            if a.len() > 1 {
                slider.value = (a.num(1)? as f32).clamp(slider.min, slider.max);
            }
            Ok(Value::Num(clean(slider.value)))
        })),
        ("clicked", native("clicked", |ctx, a| {
            let name = a.str(0)?.clone();
            Ok(Value::Bool(world(ctx)?.ui().clicked(&name)))
        })),
    ]);
    vm.register_value("ui", ui);
    vm.register("wait_click", |ctx, a| {
        let name = a.str(0)?.clone();
        ctx.suspend(Suspend::Host(Value::list(vec![Value::str("click"), Value::Str(name)])));
        Ok(Value::Nil)
    });

    // ---- сцены и данные ----
    vm.register("load_scene", |ctx, a| {
        let name = a.str(0)?.clone();
        world(ctx)?.scenes().load(&name);
        Ok(Value::Nil)
    });
    vm.register("reload_scene", |ctx, _| {
        world(ctx)?.scenes().reload();
        Ok(Value::Nil)
    });
    vm.register("quit", |ctx, _| {
        world(ctx)?.request_quit();
        Ok(Value::Nil)
    });
    let save = nsmap(vec![
        ("get", native("get", |ctx, a| {
            let key = a.str(0)?.clone();
            let w = world(ctx)?;
            let data = w.resources.get_or_insert_with(SaveData::in_memory);
            Ok(data.get_value(&key).map(to_script).unwrap_or_else(|| a.get(1).clone()))
        })),
        ("set", native("set", |ctx, a| {
            let key = a.str(0)?.clone();
            let v = a.get(1).clone();
            let w = world(ctx)?;
            let data = w.resources.get_or_insert_with(SaveData::in_memory);
            match &v {
                Value::Bool(b) => data.set_bool(&key, *b),
                Value::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => data.set_int(&key, *n as i64),
                Value::Num(n) => data.set_float(&key, *n as f32),
                Value::Str(s) => data.set_string(&key, s),
                Value::Vec2(x, y) => data.set_vec2(&key, Vec2::new(*x, *y)),
                Value::Vec3(x, y, z) => data.set_vec3(&key, Vec3::new(*x, *y, *z)),
                Value::Nil => {
                    data.delete_key(&key);
                }
                other => {
                    return Err(Throw::msg(format!(
                        "`save.set` stores booleans, numbers, strings and vectors, not {}",
                        other.describe()
                    )))
                }
            }
            if let Err(e) = data.save() {
                log::warn!(target: "scenario", "save.set(\"{key}\"): {e}");
            }
            Ok(Value::Nil)
        })),
    ]);
    vm.register_value("save", save);
    obj
}

/// Создать объект: префаб по пути/GUID/имени (`prefabs/<name>.prefab`) или копия объекта сцены.
fn spawn(world: &mut World, what: &str, pos: Vec3) -> Result<GameObjectId, Throw> {
    let mut reference: Option<String> = None;
    if what.ends_with(".prefab") || Guid::parse(what).is_some() {
        reference = Some(what.to_string());
    } else if let Some(db) = world.resources.get::<AssetDatabase>() {
        for candidate in [format!("prefabs/{what}.prefab"), format!("{what}.prefab")] {
            if db.guid_of(&candidate).is_some() {
                reference = Some(candidate);
                break;
            }
        }
    }
    if let Some(r) = reference {
        return world.instantiate_at(&r, pos, Quat::IDENTITY, None).map_err(|e| Throw::msg(e.to_string()));
    }
    if let Some(original) = world.scene.find_by_name(what) {
        let id = world.duplicate(original).map_err(|e| Throw::msg(e.to_string()))?;
        if let Some(o) = world.scene.get_mut(id) {
            o.transform.position = pos;
        }
        return Ok(id);
    }
    Err(Throw::msg(format!("`spawn`: no prefab `{what}` (looked for prefabs/{what}.prefab) and no object named `{what}` to copy")))
}
