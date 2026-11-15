//! `engine-core` — ядро движка в стиле Unity.
//!
//! Модель: [`Scene`](scene::Scene) владеет [`GameObject`](object::GameObject)'ами,
//! у каждого есть обязательный [`Transform`](transform::Transform) и список
//! компонентов ([`Component`](component::Component)). Скрипт — это обычный компонент
//! с переопределёнными хуками `on_start` / `on_update` / `on_fixed_update` / `on_destroy`.
//!
//! Крейт не знает ни про окно, ни про GPU, ни про физику: всё это подключается
//! снаружи как [`Subsystem`](app::Subsystem). Поэтому игру можно гонять в тестах
//! без окна: `app.run_headless(120, 1.0 / 60.0)`.
//!
//! Геймплей-API (всё в `prelude`): время и `time_scale`, таймеры ([`schedule`], [`timer`]),
//! твины ([`tween`]), смена сцен ([`scenes`]), сохранения ([`save`]), случайность
//! ([`random`]), математика ([`mathf`]), действия ввода и геймпады ([`input`]),
//! конечный автомат ([`state`]). Справочник: `docs/api/gameplay.md`.
//!
//! В крейте нет ни одной строки `unsafe`.
#![forbid(unsafe_code)]

pub mod app;
pub mod component;
pub mod error;
mod index;
pub mod input;
pub mod logging;
pub mod math;
pub mod mathf;
pub mod missing;
pub mod object;
pub mod objref;
pub mod profile;
pub mod random;
pub mod reflect;
pub mod save;
pub mod scene;
pub mod scenes;
pub mod schedule;
pub mod script;
pub mod serialize;
pub mod settings;
pub mod settings_sections;
pub mod state;
pub mod timer;
pub mod transform;
pub mod tween;
pub mod world;

/// Всё, что нужно в 95% игрового кода: `use engine_core::prelude::*;`
pub mod prelude {
    pub use crate::app::{App, Subsystem};
    pub use crate::component::{Component, Ctx, Hooks, Message};
    pub use crate::error::{ReflectError, SceneError};
    pub use crate::input::{
        apply_dead_zone, AnalogBinding, AxisBindings, Binding, GamepadAxis, GamepadButton, GamepadId, Input, Key,
        MouseButton, Touch, TouchPhase,
    };
    pub use crate::math::{Color, EulerRot, Mat4, Quat, Vec2, Vec3, Vec4};
    pub use crate::mathf::{
        angle_of, approx_eq, delta_angle, delta_angle_deg, exp_decay, from_angle, inverse_lerp, lerp,
        lerp_angle, lerp_angle_deg, move_towards, move_towards_angle, move_towards_vec2,
        move_towards_vec3, ping_pong, remap, repeat, smooth_damp, smooth_damp_vec2, smooth_damp_vec3,
        smootherstep, smoothstep, wrap_angle, Rect,
    };
    pub use crate::random::Random;
    pub use crate::save::{FileBackend, MemoryBackend, SaveBackend, SaveData, SaveError};
    pub use crate::scenes::{
        DirScenes, MemoryScenes, SceneLoadFailed, SceneLoaded, SceneManager, SceneSource,
    };
    pub use crate::schedule::{Scheduler, TimerId};
    pub use crate::script::{AsRotation, AsVec3, ScriptErrors};
    pub use crate::state::StateMachine;
    pub use crate::timer::{Cooldown, Timer};
    pub use crate::tween::{Ease, Sequence, Tween, TweenFinished, TweenId, Tweens};
    pub use crate::object::{GameObject, GameObjectId};
    pub use crate::objref::{ComponentRef, ObjectRef};
    pub use crate::reflect::{FieldMeta, FieldValue, Reflect, TypeName, TypeRegistry, Value};
    pub use crate::scene::{ObjectBuilder, Scene};
    pub use crate::settings::ProjectSettings;
    pub use crate::serialize::{
        NodeData, Override, Prefab, PrefabSource, SceneData, SceneSnapshot,
    };
    pub use crate::transform::Transform;
    pub use crate::world::{Events, Resources, Time, World};
    pub use crate::{component, reflect_enum};
}
