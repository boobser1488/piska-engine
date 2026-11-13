//! Сервисы для C++: физика, звук и интерфейс, доступные из `engine_component.hpp`.
//!
//! `engine-cpp-bridge` не зависит от этих крейтов, поэтому фасад кладёт в мир таблицу
//! именованных сервисов ([`engine_cpp_bridge::CppServices`]), а C++ зовёт их по имени
//! (`engine::physics::raycast`, `engine::audio::play`, `engine::ui::clicked` — обёртки над
//! `engine::call_service`). Своё можно добавить так же:
//!
//! ```ignore
//! engine::cpp_services::register(&mut app.world, "game.score", |world, args| Ok(vec![Value::Int(7)]));
//! ```
//!
//! Объект в аргументах и результатах — `Value::Int(GameObjectId::to_bits())`
//! ([`engine_cpp_bridge::object_arg`] / [`object_value`]).

// При выключенных возможностях часть разборщиков аргументов не нужна.
#![allow(dead_code)]

#[cfg(feature = "audio")]
use engine_audio::{Audio, Bus, ClipRef, PlayParams, VoiceId};
use engine_core::prelude::*;
use engine_cpp_bridge::{object_arg, object_value, services_mut, CppServices};
#[cfg(feature = "physics2d")]
use engine_physics::{Collider2D, Physics2D, Rigidbody2D, Shape2D};
#[cfg(feature = "physics3d")]
use engine_physics::Physics3D;
#[cfg(feature = "ui")]
use engine_ui::UiContext;

type Args<'a> = &'a [Value];
type Out = Result<Vec<Value>, String>;

/// Зарегистрировать сервис в мире приложения (замена прежнего с тем же именем).
pub fn register(
    world: &mut World,
    name: &str,
    handler: impl Fn(&mut World, &[Value]) -> Result<Vec<Value>, String> + 'static,
) {
    services_mut(world).register(name, handler);
}

/// Установить сервисы физики, звука и интерфейса. Зовёт `engine::app()`.
pub fn install(app: &mut App) {
    #[allow(unused_variables)]
    let services = services_mut(&mut app.world);
    #[cfg(any(feature = "physics2d", feature = "physics3d"))]
    install_physics(services);
    #[cfg(feature = "audio")]
    install_audio(services);
    #[cfg(feature = "ui")]
    install_ui(services);
}

// ── разбор аргументов ───────────────────────────────────────────────────────

fn float(args: Args, i: usize) -> f32 {
    match args.get(i) {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(n)) => *n as f32,
        _ => 0.0,
    }
}

fn int(args: Args, i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Int(n)) => *n,
        Some(Value::Float(f)) => *f as i64,
        _ => 0,
    }
}

fn boolean(args: Args, i: usize) -> bool {
    match args.get(i) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Int(n)) => *n != 0,
        _ => false,
    }
}

fn string<'a>(args: Args<'a>, i: usize) -> &'a str {
    match args.get(i) {
        Some(Value::Str(s) | Value::Enum(s) | Value::Asset(s)) => s,
        _ => "",
    }
}

fn vec2(args: Args, i: usize) -> Vec2 {
    match args.get(i) {
        Some(Value::Vec2(x, y)) | Some(Value::Vec3(x, y, _)) => Vec2::new(*x, *y),
        _ => Vec2::ZERO,
    }
}

fn vec3(args: Args, i: usize) -> Vec3 {
    match args.get(i) {
        Some(Value::Vec3(x, y, z)) => Vec3::new(*x, *y, *z),
        Some(Value::Vec2(x, y)) => Vec3::new(*x, *y, 0.0),
        _ => Vec3::ZERO,
    }
}

fn object(args: Args, i: usize) -> Option<GameObjectId> {
    args.get(i).and_then(object_arg)
}

fn live_object(world: &World, args: Args, i: usize) -> Result<GameObjectId, String> {
    let id = object(args, i).ok_or("the object argument is missing (a null handle)")?;
    if world.scene.contains(id) {
        Ok(id)
    } else {
        Err(format!("the object {id:?} was destroyed (stale handle)"))
    }
}

fn v3(v: Vec3) -> Value {
    Value::Vec3(v.x, v.y, v.z)
}

fn v2(v: Vec2) -> Value {
    Value::Vec2(v.x, v.y)
}

fn objects(ids: impl IntoIterator<Item = GameObjectId>, limit: usize) -> Vec<Value> {
    ids.into_iter().take(limit).map(object_value).collect()
}

// ── физика ──────────────────────────────────────────────────────────────────

#[cfg(feature = "physics3d")]
fn physics3d(world: &mut World) -> Result<&mut Physics3D, String> {
    world
        .resources
        .get_mut::<Physics3D>()
        .ok_or_else(|| "3D physics is not installed (add `engine_physics::Physics3DSystem` to the app)".to_string())
}

#[cfg(any(feature = "physics2d", feature = "physics3d"))]
fn install_physics(s: &mut CppServices) {
    #[cfg(feature = "physics3d")]
    s.register("physics3d.raycast", |world, a| -> Out {
        let ignore = object(a, 3);
        let physics = physics3d(world)?;
        Ok(match physics.raycast(vec3(a, 0), vec3(a, 1), float(a, 2), ignore) {
            Some(hit) => vec![object_value(hit.object), v3(hit.point), v3(hit.normal), Value::Float(hit.distance)],
            None => vec![],
        })
    });
    #[cfg(feature = "physics2d")]
    s.register("physics2d.raycast", |world, a| -> Out {
        let ignore = object(a, 3);
        let physics = world
            .resources
            .get::<Physics2D>()
            .ok_or("2D physics is not installed (add `engine_physics::Physics2DSystem` to the app)")?;
        Ok(match physics.raycast(vec2(a, 0), vec2(a, 1), float(a, 2), ignore) {
            Some(hit) => vec![object_value(hit.object), v2(hit.point), v2(hit.normal), Value::Float(hit.distance)],
            None => vec![],
        })
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.overlap_sphere", |world, a| -> Out {
        // Уничтоженный в этом кадре объект ещё числится в физике до её шага.
        let mut found = physics3d(world)?.overlap_sphere(vec3(a, 0), float(a, 1));
        found.retain(|&id| world.scene.contains(id));
        Ok(objects(found, int(a, 2).max(0) as usize))
    });
    #[cfg(feature = "physics2d")]
    s.register("physics2d.overlap_circle", |world, a| -> Out {
        let found = overlap_circle_2d(world, vec2(a, 0), float(a, 1));
        Ok(objects(found, int(a, 2).max(0) as usize))
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.apply_impulse", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        physics3d(world)?.apply_impulse(id, vec3(a, 1));
        Ok(vec![])
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.apply_impulse_at", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        physics3d(world)?.apply_impulse_at(id, vec3(a, 1), vec3(a, 2));
        Ok(vec![])
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.apply_torque_impulse", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        physics3d(world)?.apply_torque_impulse(id, vec3(a, 1));
        Ok(vec![])
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.add_force", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        physics3d(world)?.add_force(id, vec3(a, 1));
        Ok(vec![])
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.radial_impulse", |world, a| -> Out {
        let hit = physics3d(world)?.radial_impulse(vec3(a, 0), float(a, 1), float(a, 2));
        Ok(objects(hit, 64))
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.mass", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        Ok(vec![Value::Float(physics3d(world)?.mass(id))])
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.is_sleeping", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        Ok(vec![Value::Bool(physics3d(world)?.is_sleeping(id))])
    });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.gravity", |world, _| -> Out { Ok(vec![v3(physics3d(world)?.gravity)]) });
    #[cfg(feature = "physics3d")]
    s.register("physics3d.set_gravity", |world, a| -> Out {
        physics3d(world)?.gravity = vec3(a, 0);
        Ok(vec![])
    });
    #[cfg(feature = "physics2d")]
    s.register("physics2d.gravity", |world, _| -> Out {
        let physics = world.resources.get::<Physics2D>().ok_or("2D physics is not installed")?;
        Ok(vec![v2(physics.gravity)])
    });
    #[cfg(feature = "physics2d")]
    s.register("physics2d.set_gravity", |world, a| -> Out {
        let physics = world.resources.get_mut::<Physics2D>().ok_or("2D physics is not installed")?;
        physics.gravity = vec2(a, 0);
        Ok(vec![])
    });
    #[cfg(feature = "physics2d")]
    s.register("physics2d.velocity_change", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        let delta = vec2(a, 1);
        let body = world
            .scene
            .get_component_mut::<Rigidbody2D>(id)
            .ok_or("the object has no Rigidbody2D")?;
        body.velocity += delta;
        Ok(vec![])
    });
}

#[cfg(feature = "physics2d")]
/// Объекты, чьи `Collider2D` задевают круг. Считается по сцене (в Rapier нет такого запроса
/// в используемой обёртке): коробка — повёрнутый прямоугольник, круг, капсула — отрезок с радиусом.
fn overlap_circle_2d(world: &World, center: Vec2, radius: f32) -> Vec<GameObjectId> {
    let mut found = Vec::new();
    for (id, collider) in world.scene.iter_active_with::<Collider2D>() {
        let matrix = world.scene.world_matrix(id);
        let (scale, _, _) = matrix.to_scale_rotation_translation();
        let k = scale.x.abs().max(scale.y.abs()).max(1e-6);
        let local = matrix
            .inverse()
            .transform_point3(Vec3::new(center.x, center.y, 0.0))
            .truncate()
            - collider.offset;
        let reach = radius / k;
        let distance = match collider.shape {
            Shape2D::Circle => (local.length() - collider.radius).max(0.0),
            Shape2D::Box => {
                let half = collider.size * 0.5;
                (local.abs() - half).max(Vec2::ZERO).length()
            }
            Shape2D::Capsule => {
                let r = collider.size.x * 0.5;
                let half = (collider.size.y * 0.5 - r).max(0.0);
                let nearest = Vec2::new(0.0, local.y.clamp(-half, half));
                ((local - nearest).length() - r).max(0.0)
            }
        };
        if distance <= reach {
            found.push(id);
        }
    }
    found
}

// ── звук ────────────────────────────────────────────────────────────────────

#[cfg(feature = "audio")]
fn audio(world: &mut World) -> Result<&mut Audio, String> {
    world.resources.get_mut::<Audio>().ok_or_else(|| "the Audio resource is not installed".to_string())
}

#[cfg(feature = "audio")]
fn bus(name: &str) -> Result<Bus, String> {
    Bus::from_value(&Value::Enum(name.to_string()))
        .ok_or_else(|| format!("unknown audio bus `{name}`; use Master, Music, Sfx or Ui"))
}

#[cfg(feature = "audio")]
/// Имя зарегистрированного клипа или GUID ассета.
fn clip_ref(audio: &Audio, name: &str) -> ClipRef {
    if !audio.has_clip(name) {
        if let Some(guid) = engine_assets::Guid::parse(name) {
            return ClipRef::Asset(guid);
        }
    }
    ClipRef::Name(name.to_string())
}

#[cfg(feature = "audio")]
fn voice(args: Args, i: usize) -> VoiceId {
    VoiceId(int(args, i) as u64)
}

#[cfg(feature = "audio")]
fn install_audio(s: &mut CppServices) {
    s.register("audio.play", |world, a| -> Out {
        let bus = bus(string(a, 5))?;
        let audio = audio(world)?;
        let mut params = PlayParams::new()
            .volume(float(a, 1))
            .pitch(float(a, 2))
            .pan(float(a, 3))
            .looping(boolean(a, 4))
            .bus(bus)
            .fade_in(float(a, 10))
            .range(float(a, 8), float(a, 9));
        if boolean(a, 6) {
            params = params.at(vec3(a, 7));
        }
        let clip = clip_ref(audio, string(a, 0));
        Ok(vec![Value::Int(audio.play_with(clip, params).0 as i64)])
    });
    s.register("audio.stop", |world, a| -> Out {
        audio(world)?.stop(voice(a, 0));
        Ok(vec![])
    });
    s.register("audio.is_playing", |world, a| -> Out { Ok(vec![Value::Bool(audio(world)?.is_playing(voice(a, 0)))]) });
    s.register("audio.set_volume", |world, a| -> Out {
        audio(world)?.set_volume(voice(a, 0), float(a, 1));
        Ok(vec![])
    });
    s.register("audio.set_pitch", |world, a| -> Out {
        audio(world)?.set_pitch(voice(a, 0), float(a, 1));
        Ok(vec![])
    });
    s.register("audio.fade_out", |world, a| -> Out {
        audio(world)?.fade_out(voice(a, 0), float(a, 1));
        Ok(vec![])
    });
    s.register("audio.play_music", |world, a| -> Out {
        let audio = audio(world)?;
        let clip = clip_ref(audio, string(a, 0));
        Ok(vec![Value::Int(audio.play_music(clip, float(a, 1)).0 as i64)])
    });
    s.register("audio.stop_music", |world, a| -> Out {
        audio(world)?.stop_music(float(a, 0));
        Ok(vec![])
    });
    s.register("audio.is_music_playing", |world, _| -> Out { Ok(vec![Value::Bool(audio(world)?.is_music_playing())]) });
    s.register("audio.voice_count", |world, _| -> Out { Ok(vec![Value::Int(audio(world)?.voice_count() as i64)]) });
    s.register("audio.stop_all", |world, _| -> Out {
        audio(world)?.stop_all();
        Ok(vec![])
    });
    s.register("audio.pause_all", |world, _| -> Out {
        audio(world)?.pause_all();
        Ok(vec![])
    });
    s.register("audio.resume_all", |world, _| -> Out {
        audio(world)?.resume_all();
        Ok(vec![])
    });
    s.register("audio.set_bus_volume", |world, a| -> Out {
        let bus = bus(string(a, 0))?;
        audio(world)?.set_bus_volume(bus, float(a, 1));
        Ok(vec![])
    });
    s.register("audio.bus_volume", |world, a| -> Out {
        let bus = bus(string(a, 0))?;
        Ok(vec![Value::Float(audio(world)?.bus_volume(bus))])
    });
    s.register("audio.set_bus_muted", |world, a| -> Out {
        let bus = bus(string(a, 0))?;
        audio(world)?.set_bus_muted(bus, boolean(a, 1));
        Ok(vec![])
    });
    s.register("audio.play_source", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        audio(world)?.play_source(id);
        Ok(vec![])
    });
    s.register("audio.stop_source", |world, a| -> Out {
        let id = live_object(world, a, 0)?;
        audio(world)?.stop_source(id);
        Ok(vec![])
    });
}

// ── интерфейс ───────────────────────────────────────────────────────────────

#[cfg(feature = "ui")]
fn install_ui(s: &mut CppServices) {
    s.register("ui.clicked", |world, a| -> Out { Ok(vec![Value::Bool(world.ui().clicked(string(a, 0)))]) });
    s.register("ui.toggled", |world, a| -> Out {
        Ok(world.ui().toggled(string(a, 0)).map(Value::Bool).into_iter().collect())
    });
    s.register("ui.value_changed", |world, a| -> Out {
        Ok(world.ui().value_changed(string(a, 0)).map(Value::Float).into_iter().collect())
    });
    s.register("ui.text_changed", |world, a| -> Out {
        Ok(world.ui().text_changed(string(a, 0)).map(|t| Value::Str(t.to_string())).into_iter().collect())
    });
    s.register("ui.submitted", |world, a| -> Out {
        Ok(world.ui().submitted(string(a, 0)).map(|t| Value::Str(t.to_string())).into_iter().collect())
    });
    s.register("ui.pointer_over_ui", |world, _| -> Out { Ok(vec![Value::Bool(world.ui().pointer_over_ui())]) });
    s.register("ui.wants_keyboard", |world, _| -> Out { Ok(vec![Value::Bool(world.ui().wants_keyboard())]) });
}
