//! Твины: плавное изменение Transform или любого поля компонента по пути.
//!
//! ```
//! use engine_core::prelude::*;
//!
//! component! {
//!     pub struct Popup { pub shown: bool = false }
//! }
//! impl Component for Popup {
//!     fn on_start(&mut self, ctx: &mut Ctx) {
//!         let me = ctx.id;
//!         // Выехать вправо за полсекунды с замедлением в конце и по завершении
//!         // сообщить себе «Arrived».
//!         ctx.start_tween(
//!             Tween::position(me, Vec3::new(5.0, 0.0, 0.0))
//!                 .duration(0.5)
//!                 .ease(Ease::OutCubic)
//!                 .on_complete(me, "Arrived", Value::Nil),
//!         );
//!     }
//! }
//! ```
//!
//! Путь поля — `"Компонент.поле"` или `"Компонент.поле.канал"` (канал: `x y z w` либо
//! `r g b a`): `"SpriteRenderer.color"`, `"Health.hp"`, `"Transform.position.y"`.
//! У `Transform` особые поля: `position`, `scale` (Vec3), `rotation` (Quat, сферическая
//! интерполяция), `euler` (Vec3, градусы), `rotation_z` (радианы, для 2D).
//! Анимироваться могут `Float`, `Int` (округляется), `Vec2`, `Vec3`, `Quat`, `Color`.
//!
//! Правила:
//! * твин знает объект и умирает вместе с ним (уничтожение, замена сцены) — молча;
//! * «от» запоминается в момент старта анимации (после `delay`), если не задано `.from(..)`;
//! * время идёт по `time.delta`: пауза и замедление действуют, `.unscaled()` — реальное время;
//! * пока объект неактивен в иерархии, его твины стоят.

use std::f32::consts::{FRAC_PI_2, PI};

use crate::math::{EulerRot, Quat, Vec3};
use crate::object::GameObjectId;
use crate::reflect::{FieldValue, Value};
use crate::scene::Scene;
use crate::world::World;
use crate::reflect_enum;

reflect_enum! {
    /// Кривая скорости твина. `In*` — разгон, `Out*` — торможение, `InOut*` — оба.
    pub enum Ease {
        Linear,
        InQuad, OutQuad, InOutQuad,
        InCubic, OutCubic, InOutCubic,
        InQuart, OutQuart, InOutQuart,
        InSine, OutSine, InOutSine,
        InExpo, OutExpo, InOutExpo,
        InCirc, OutCirc, InOutCirc,
        InBack, OutBack, InOutBack,
        InElastic, OutElastic, InOutElastic,
        InBounce, OutBounce, InOutBounce,
        SmoothStep,
    }
}

#[allow(clippy::derivable_impls)] // `reflect_enum!` не умеет `#[default]`
impl Default for Ease {
    fn default() -> Self {
        Ease::Linear
    }
}

impl Ease {
    /// Все кривые — для выпадающих списков и перебора в тестах.
    pub const ALL: [Ease; 29] = [
        Ease::Linear,
        Ease::InQuad, Ease::OutQuad, Ease::InOutQuad,
        Ease::InCubic, Ease::OutCubic, Ease::InOutCubic,
        Ease::InQuart, Ease::OutQuart, Ease::InOutQuart,
        Ease::InSine, Ease::OutSine, Ease::InOutSine,
        Ease::InExpo, Ease::OutExpo, Ease::InOutExpo,
        Ease::InCirc, Ease::OutCirc, Ease::InOutCirc,
        Ease::InBack, Ease::OutBack, Ease::InOutBack,
        Ease::InElastic, Ease::OutElastic, Ease::InOutElastic,
        Ease::InBounce, Ease::OutBounce, Ease::InOutBounce,
        Ease::SmoothStep,
    ];

    /// Значение кривой при `t` из 0..=1 (вне диапазона `t` ограничивается). `apply(0) == 0`
    /// и `apply(1) == 1` для всех кривых; `Back` и `Elastic` в середине выходят за 0..1.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        const C1: f32 = 1.70158;
        const C2: f32 = C1 * 1.525;
        const C3: f32 = C1 + 1.0;
        const C4: f32 = 2.0 * PI / 3.0;
        const C5: f32 = 2.0 * PI / 4.5;
        match self {
            Ease::Linear => t,
            Ease::InQuad => t * t,
            Ease::OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Ease::InOutQuad => in_out(t, |x| x * x * 2.0),
            Ease::InCubic => t * t * t,
            Ease::OutCubic => 1.0 - (1.0 - t).powi(3),
            Ease::InOutCubic => in_out(t, |x| x * x * x * 4.0),
            Ease::InQuart => t.powi(4),
            Ease::OutQuart => 1.0 - (1.0 - t).powi(4),
            Ease::InOutQuart => in_out(t, |x| x.powi(4) * 8.0),
            Ease::InSine => 1.0 - (t * FRAC_PI_2).cos(),
            Ease::OutSine => (t * FRAC_PI_2).sin(),
            Ease::InOutSine => -((PI * t).cos() - 1.0) / 2.0,
            Ease::InExpo => 2f32.powf(10.0 * t - 10.0),
            Ease::OutExpo => 1.0 - 2f32.powf(-10.0 * t),
            Ease::InOutExpo => {
                if t < 0.5 {
                    2f32.powf(20.0 * t - 10.0) / 2.0
                } else {
                    (2.0 - 2f32.powf(-20.0 * t + 10.0)) / 2.0
                }
            }
            Ease::InCirc => 1.0 - (1.0 - t * t).sqrt(),
            Ease::OutCirc => (1.0 - (t - 1.0).powi(2)).sqrt(),
            Ease::InOutCirc => {
                if t < 0.5 {
                    (1.0 - (1.0 - (2.0 * t).powi(2)).sqrt()) / 2.0
                } else {
                    ((1.0 - (-2.0 * t + 2.0).powi(2)).sqrt() + 1.0) / 2.0
                }
            }
            Ease::InBack => C3 * t * t * t - C1 * t * t,
            Ease::OutBack => 1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2),
            Ease::InOutBack => {
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
                }
            }
            Ease::InElastic => -(2f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * C4).sin(),
            Ease::OutElastic => 2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * C4).sin() + 1.0,
            Ease::InOutElastic => {
                if t < 0.5 {
                    -(2f32.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * C5).sin()) / 2.0
                } else {
                    2f32.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * C5).sin() / 2.0 + 1.0
                }
            }
            Ease::InBounce => 1.0 - bounce_out(1.0 - t),
            Ease::OutBounce => bounce_out(t),
            Ease::InOutBounce => {
                if t < 0.5 {
                    (1.0 - bounce_out(1.0 - 2.0 * t)) / 2.0
                } else {
                    (1.0 + bounce_out(2.0 * t - 1.0)) / 2.0
                }
            }
            Ease::SmoothStep => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// «InOut» из функции первой половины: `f` считает кривую для `t < 0.5`.
fn in_out(t: f32, first_half: impl Fn(f32) -> f32) -> f32 {
    if t < 0.5 {
        first_half(t)
    } else {
        1.0 - first_half(1.0 - t)
    }
}

fn bounce_out(t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

// ----------------------------------------------------------------------
// Путь к полю
// ----------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum TransformField {
    Position,
    Rotation,
    Scale,
    Euler,
    RotationZ,
}

#[derive(Clone, Debug, PartialEq)]
enum Path {
    Transform(TransformField),
    Component { component: String, field: String },
}

#[derive(Clone, Debug, PartialEq)]
struct FieldPath {
    path: Path,
    /// Номер канала (x/r = 0, y/g = 1, z/b = 2, w/a = 3), если анимируется одна компонента.
    channel: Option<usize>,
    text: String,
}

fn parse_path(text: &str) -> Result<FieldPath, String> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(format!(
            "bad tween path `{text}`: expected `Component.field` or `Component.field.channel`, e.g. `SpriteRenderer.color` or `Transform.position.y`"
        ));
    }
    let channel = match parts.get(2) {
        None => None,
        Some(c) => Some(match *c {
            "x" | "r" => 0,
            "y" | "g" => 1,
            "z" | "b" => 2,
            "w" | "a" => 3,
            other => {
                return Err(format!(
                    "bad tween path `{text}`: unknown channel `{other}` (use x, y, z, w or r, g, b, a)"
                ))
            }
        }),
    };
    let path = if parts[0] == "Transform" {
        Path::Transform(match parts[1] {
            "position" => TransformField::Position,
            "rotation" => TransformField::Rotation,
            "scale" => TransformField::Scale,
            "euler" => TransformField::Euler,
            "rotation_z" => TransformField::RotationZ,
            other => {
                return Err(format!(
                    "bad tween path `{text}`: Transform has no field `{other}` (position, rotation, scale, euler, rotation_z)"
                ))
            }
        })
    } else {
        Path::Component {
            component: parts[0].to_string(),
            field: parts[1].to_string(),
        }
    };
    Ok(FieldPath {
        path,
        channel,
        text: text.to_string(),
    })
}

fn read_full(scene: &Scene, id: GameObjectId, path: &FieldPath) -> Result<Value, String> {
    let obj = scene.get(id).ok_or_else(|| "object is gone".to_string())?;
    match &path.path {
        Path::Transform(field) => {
            let t = &obj.transform;
            Ok(match field {
                TransformField::Position => Value::Vec3(t.position.x, t.position.y, t.position.z),
                TransformField::Scale => Value::Vec3(t.scale.x, t.scale.y, t.scale.z),
                TransformField::Rotation => Value::Quat(t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w),
                TransformField::Euler => {
                    let e = t.euler_deg();
                    Value::Vec3(e.x, e.y, e.z)
                }
                TransformField::RotationZ => Value::Float(t.rotation.to_euler(EulerRot::ZYX).0),
            })
        }
        Path::Component { component, field } => {
            let c = obj
                .components()
                .find(|c| c.type_name() == component)
                .ok_or_else(|| format!("object `{}` has no component `{component}`", obj.name))?;
            c.get_field(field).ok_or_else(|| {
                format!(
                    "component `{component}` has no field `{field}` (available: {})",
                    c.field_names().join(", ")
                )
            })
        }
    }
}

fn write_full(scene: &mut Scene, id: GameObjectId, path: &FieldPath, value: &Value) -> Result<(), String> {
    let obj = scene.get_mut(id).ok_or_else(|| "object is gone".to_string())?;
    match &path.path {
        Path::Transform(field) => {
            let t = &mut obj.transform;
            match (field, value) {
                (TransformField::Position, Value::Vec3(x, y, z)) => t.position = Vec3::new(*x, *y, *z),
                (TransformField::Scale, Value::Vec3(x, y, z)) => t.scale = Vec3::new(*x, *y, *z),
                (TransformField::Rotation, Value::Quat(x, y, z, w)) => {
                    t.rotation = Quat::from_xyzw(*x, *y, *z, *w).normalize()
                }
                (TransformField::Euler, Value::Vec3(x, y, z)) => t.set_euler_deg(Vec3::new(*x, *y, *z)),
                (TransformField::RotationZ, Value::Float(a)) => t.rotation = Quat::from_rotation_z(*a),
                _ => return Err(format!("`{}` cannot take a {} value", path.text, value.kind_name())),
            }
            Ok(())
        }
        Path::Component { component, field } => {
            let name = obj.name.clone();
            let c = obj
                .components_mut()
                .find(|c| c.type_name() == component)
                .ok_or_else(|| format!("object `{name}` has no component `{component}`"))?;
            c.set_field(field, value).map_err(|e| e.to_string())
        }
    }
}

fn channel_get(value: &Value, channel: usize) -> Option<f32> {
    let list: Vec<f32> = match value {
        Value::Vec2(x, y) => vec![*x, *y],
        Value::Vec3(x, y, z) => vec![*x, *y, *z],
        Value::Quat(x, y, z, w) | Value::Color(x, y, z, w) => vec![*x, *y, *z, *w],
        _ => return None,
    };
    list.get(channel).copied()
}

fn channel_set(value: &mut Value, channel: usize, v: f32) -> bool {
    match (value, channel) {
        (Value::Vec2(x, _), 0) | (Value::Vec3(x, ..), 0) | (Value::Quat(x, ..), 0) | (Value::Color(x, ..), 0) => *x = v,
        (Value::Vec2(_, y), 1) | (Value::Vec3(_, y, _), 1) | (Value::Quat(_, y, ..), 1) | (Value::Color(_, y, ..), 1) => *y = v,
        (Value::Vec3(_, _, z), 2) | (Value::Quat(_, _, z, _), 2) | (Value::Color(_, _, z, _), 2) => *z = v,
        (Value::Quat(.., w), 3) | (Value::Color(.., w), 3) => *w = v,
        _ => return false,
    }
    true
}

fn number(value: &Value) -> Option<f32> {
    match value {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f32),
        _ => None,
    }
}

/// Интерполяция двух значений одного вида. Результат имеет вид `from`.
fn interpolate(from: &Value, to: &Value, t: f32) -> Option<Value> {
    let l = |a: f32, b: f32| a + (b - a) * t;
    Some(match (from, to) {
        (Value::Int(_), _) => Value::Int(l(number(from)?, number(to)?).round() as i64),
        (Value::Float(_), _) => Value::Float(l(number(from)?, number(to)?)),
        (Value::Vec2(a, b), Value::Vec2(c, d)) => Value::Vec2(l(*a, *c), l(*b, *d)),
        (Value::Vec3(a, b, c), Value::Vec3(d, e, f)) => Value::Vec3(l(*a, *d), l(*b, *e), l(*c, *f)),
        (Value::Color(a, b, c, d), Value::Color(e, f, g, h)) => Value::Color(l(*a, *e), l(*b, *f), l(*c, *g), l(*d, *h)),
        (Value::Quat(a, b, c, d), Value::Quat(e, f, g, h)) => {
            let q = Quat::from_xyzw(*a, *b, *c, *d)
                .normalize()
                .slerp(Quat::from_xyzw(*e, *f, *g, *h).normalize(), t);
            Value::Quat(q.x, q.y, q.z, q.w)
        }
        _ => return None,
    })
}

// ----------------------------------------------------------------------
// Описание твина
// ----------------------------------------------------------------------

/// Идентификатор твина или последовательности — для [`Tweens::cancel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TweenId(u64);

/// Событие «твин или последовательность доработали до конца» (`ctx.world.events.read::<TweenFinished>()`
/// в следующем кадре). Для бесконечных и отменённых твинов не отправляется.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TweenFinished {
    pub id: TweenId,
    /// Объект твина; у последовательности — адресат `on_complete` (если задан).
    pub target: Option<GameObjectId>,
}

/// Описание анимации; запускается `ctx.start_tween(..)` или `world.start_tween(..)`.
#[derive(Clone, Debug)]
pub struct Tween {
    target: GameObjectId,
    path: String,
    to: Value,
    from: Option<Value>,
    duration: f32,
    delay: f32,
    ease: Ease,
    /// 0 — бесконечно.
    loops: u32,
    yoyo: bool,
    unscaled: bool,
    on_complete: Option<(GameObjectId, String, Value)>,
}

impl Tween {
    /// Анимировать поле по пути (см. описание модуля): `Tween::field(id, "SpriteRenderer.color", Color::RED)`.
    pub fn field(target: GameObjectId, path: &str, to: impl FieldValue) -> Self {
        Self {
            target,
            path: path.to_string(),
            to: to.to_value(),
            from: None,
            duration: 1.0,
            delay: 0.0,
            ease: Ease::Linear,
            loops: 1,
            yoyo: false,
            unscaled: false,
            on_complete: None,
        }
    }

    /// Локальная позиция `Transform` (Unity: `DOLocalMove`).
    pub fn position(target: GameObjectId, to: Vec3) -> Self {
        Self::field(target, "Transform.position", to)
    }

    /// Локальный масштаб.
    pub fn scale(target: GameObjectId, to: Vec3) -> Self {
        Self::field(target, "Transform.scale", to)
    }

    /// Поворот как кватернион (кратчайшая дуга).
    pub fn rotation(target: GameObjectId, to: Quat) -> Self {
        Self::field(target, "Transform.rotation", to)
    }

    /// Углы Эйлера в градусах (X, Y, Z): можно «накрутить» больше 180°, например `euler.y = 360`.
    /// Интерполяция покомпонентная, а не по кратчайшей дуге.
    pub fn euler(target: GameObjectId, to_deg: Vec3) -> Self {
        Self::field(target, "Transform.euler", to_deg)
    }

    /// Поворот вокруг Z в радианах — для 2D. Покомпонентно: от 0 до `TAU` — полный оборот.
    pub fn rotation_z(target: GameObjectId, to_radians: f32) -> Self {
        Self::field(target, "Transform.rotation_z", to_radians)
    }

    pub fn from(mut self, value: impl FieldValue) -> Self {
        self.from = Some(value.to_value());
        self
    }

    /// Длительность одного прохода, секунды (по умолчанию 1). `0` — применится за один кадр.
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = seconds.max(0.0);
        self
    }

    /// Пауза перед стартом, секунды.
    pub fn delay(mut self, seconds: f32) -> Self {
        self.delay = seconds.max(0.0);
        self
    }

    pub fn ease(mut self, ease: Ease) -> Self {
        self.ease = ease;
        self
    }

    /// Сколько раз проиграть (по умолчанию 1).
    pub fn loops(mut self, count: u32) -> Self {
        self.loops = count.max(1);
        self
    }

    /// Играть бесконечно (до `cancel` или смерти объекта). Завершения не будет.
    pub fn forever(mut self) -> Self {
        self.loops = 0;
        self
    }

    /// Каждый второй проход играется в обратную сторону (туда-обратно).
    pub fn yoyo(mut self) -> Self {
        self.yoyo = true;
        self
    }

    /// Идти по реальному времени (интерфейс на паузе игры).
    pub fn unscaled(mut self) -> Self {
        self.unscaled = true;
        self
    }

    /// По завершении отправить `target` сообщение `name` (хук `on_message`).
    pub fn on_complete(mut self, target: GameObjectId, name: &str, value: Value) -> Self {
        self.on_complete = Some((target, name.to_string(), value));
        self
    }

    /// Полная длительность с задержкой и повторами; `INFINITY` для бесконечного.
    pub fn total_duration(&self) -> f32 {
        if self.loops == 0 {
            f32::INFINITY
        } else {
            self.delay + self.duration * self.loops as f32
        }
    }
}

/// Последовательность твинов: `then` — после всего, что было, `join` — одновременно с
/// предыдущим шагом, `wait` — пауза.
///
/// ```
/// use engine_core::prelude::*;
///
/// let mut scene = Scene::new("demo");
/// let hero = scene.spawn("Hero");
/// let _seq = Sequence::new()
///     .then(Tween::position(hero, Vec3::Y).duration(0.3))
///     .join(Tween::scale(hero, Vec3::splat(1.5)).duration(0.3))
///     .wait(0.2)
///     .then(Tween::position(hero, Vec3::ZERO).duration(0.3));
/// ```
///
/// Бесконечные твины внутри последовательности не поддерживаются (она бы не кончилась).
#[derive(Clone, Debug, Default)]
pub struct Sequence {
    steps: Vec<(f32, Tween)>,
    end: f32,
    last_start: f32,
    unscaled: bool,
    on_complete: Option<(GameObjectId, String, Value)>,
}

impl Sequence {
    pub fn new() -> Self {
        Self::default()
    }

    fn finite(tween: &Tween) -> f32 {
        let total = tween.total_duration();
        if total.is_finite() {
            total
        } else {
            log::warn!("a `forever` tween inside a Sequence is played once instead: the sequence would never end");
            tween.delay + tween.duration
        }
    }

    /// Шаг, который стартует, когда закончилось всё добавленное ранее.
    pub fn then(mut self, mut tween: Tween) -> Self {
        tween.loops = tween.loops.max(1);
        let start = self.end;
        self.end = start + Self::finite(&tween);
        self.last_start = start;
        self.steps.push((start, tween));
        self
    }

    /// Шаг, который стартует одновременно с предыдущим `then`.
    pub fn join(mut self, mut tween: Tween) -> Self {
        tween.loops = tween.loops.max(1);
        let start = self.last_start;
        self.end = self.end.max(start + Self::finite(&tween));
        self.steps.push((start, tween));
        self
    }

    /// Пауза перед следующим `then`.
    pub fn wait(mut self, seconds: f32) -> Self {
        self.end += seconds.max(0.0);
        self.last_start = self.end;
        self
    }

    pub fn unscaled(mut self) -> Self {
        self.unscaled = true;
        self
    }

    /// Сообщение, когда закончится вся последовательность.
    pub fn on_complete(mut self, target: GameObjectId, name: &str, value: Value) -> Self {
        self.on_complete = Some((target, name.to_string(), value));
        self
    }

    /// Общая длительность.
    pub fn total_duration(&self) -> f32 {
        self.end
    }
}

// ----------------------------------------------------------------------
// Исполнение
// ----------------------------------------------------------------------

/// Куда и что отправить по завершении.
type Completion = Option<(GameObjectId, String, Value)>;

struct Running {
    id: TweenId,
    /// Участник последовательности: `cancel(group)` убивает и его.
    group: Option<TweenId>,
    /// Пустышка последовательности: в конце шлёт событие и сообщение.
    marker: bool,
    target: Option<GameObjectId>,
    path: Option<FieldPath>,
    to: Value,
    from: Option<Value>,
    duration: f32,
    delay: f32,
    ease: Ease,
    loops: u32,
    yoyo: bool,
    unscaled: bool,
    elapsed: f32,
    on_complete: Option<(GameObjectId, String, Value)>,
}

/// Все работающие твины. Ресурс мира, создаётся при первом использовании.
#[derive(Default)]
pub struct Tweens {
    running: Vec<Running>,
    next_id: u64,
}

impl Tweens {
    fn fresh_id(&mut self) -> TweenId {
        self.next_id += 1;
        TweenId(self.next_id)
    }

    /// Запустить твин. Неверный путь (`"Comp.fiel"`) не паникует: ошибка уходит в лог
    /// при первом кадре, твин снимается.
    pub fn start(&mut self, tween: Tween) -> TweenId {
        let id = self.fresh_id();
        self.add(id, None, tween, false);
        id
    }

    fn add(&mut self, id: TweenId, group: Option<TweenId>, tween: Tween, unscaled: bool) {
        let (path, to) = match parse_path(&tween.path) {
            Ok(parsed) => {
                // С каналом анимируется одно число.
                let to = if parsed.channel.is_some() {
                    match number(&tween.to) {
                        Some(n) => Value::Float(n),
                        None => tween.to.clone(),
                    }
                } else {
                    tween.to.clone()
                };
                (Some(parsed), to)
            }
            Err(message) => {
                log::error!("{message}");
                return;
            }
        };
        self.running.push(Running {
            id,
            group,
            marker: false,
            target: Some(tween.target),
            path,
            to,
            from: tween.from,
            duration: tween.duration,
            delay: tween.delay,
            ease: tween.ease,
            loops: tween.loops,
            yoyo: tween.yoyo,
            unscaled: tween.unscaled || unscaled,
            elapsed: 0.0,
            on_complete: tween.on_complete,
        });
    }

    /// Запустить последовательность; её `TweenId` отменяет всю цепочку разом.
    pub fn start_sequence(&mut self, sequence: Sequence) -> TweenId {
        let id = self.fresh_id();
        let unscaled = sequence.unscaled;
        for (start, mut tween) in sequence.steps {
            tween.delay += start;
            let member = self.fresh_id();
            self.add(member, Some(id), tween, unscaled);
        }
        self.running.push(Running {
            id,
            group: None,
            marker: true,
            target: sequence.on_complete.as_ref().map(|c| c.0),
            path: None,
            to: Value::Nil,
            from: None,
            duration: 0.0,
            delay: sequence.end,
            ease: Ease::Linear,
            loops: 1,
            yoyo: false,
            unscaled,
            elapsed: 0.0,
            on_complete: sequence.on_complete,
        });
        id
    }

    /// Снять твин (или всю последовательность) на том значении, где он остановился.
    /// Сообщение о завершении не отправляется. `false`, если такого уже нет.
    pub fn cancel(&mut self, id: TweenId) -> bool {
        let before = self.running.len();
        self.running.retain(|r| r.id != id && r.group != Some(id));
        self.running.len() != before
    }

    /// Снять все твины объекта.
    pub fn cancel_all_for(&mut self, target: GameObjectId) -> usize {
        let before = self.running.len();
        self.running.retain(|r| r.target != Some(target));
        before - self.running.len()
    }

    /// Снять вообще все.
    pub fn cancel_all(&mut self) {
        self.running.clear();
    }

    pub fn is_running(&self, id: TweenId) -> bool {
        self.running.iter().any(|r| r.id == id || r.group == Some(id))
    }

    /// Сколько твинов работает (участники последовательностей считаются по отдельности).
    pub fn len(&self) -> usize {
        self.running.iter().filter(|r| !r.marker).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

enum Outcome {
    Running,
    Finished,
    Dropped,
}

/// Где мы в проигрывании: прогресс 0..=1 для кривой и «кончилось ли».
fn sample(t: f32, duration: f32, loops: u32, yoyo: bool) -> (f32, bool) {
    if duration <= 0.0 {
        return (1.0, true);
    }
    let cycle = (t / duration).floor().max(0.0);
    if loops != 0 && cycle >= loops as f32 {
        let ends_at_start = yoyo && loops & 1 == 0;
        return (if ends_at_start { 0.0 } else { 1.0 }, true);
    }
    let within = ((t - cycle * duration) / duration).clamp(0.0, 1.0);
    let reversed = yoyo && (cycle as u64) % 2 == 1;
    (if reversed { 1.0 - within } else { within }, false)
}

impl Running {
    fn advance(&mut self, scene: &mut Scene, dt: f32) -> Outcome {
        self.elapsed += dt;
        if self.elapsed < self.delay {
            return Outcome::Running;
        }
        if self.marker {
            return Outcome::Finished;
        }
        let (Some(target), Some(path)) = (self.target, self.path.as_ref()) else {
            return Outcome::Dropped;
        };

        let log_drop = |scene: &Scene, message: String| {
            let name = scene.get(target).map(|o| o.name.as_str()).unwrap_or("?");
            log::warn!("tween `{}` on `{name}` dropped: {message}", path.text);
            Outcome::Dropped
        };

        if self.from.is_none() {
            // Начало анимации: запомнить, откуда идём.
            let current = match read_full(scene, target, path) {
                Ok(v) => v,
                Err(e) => return log_drop(scene, e),
            };
            let from = match path.channel {
                Some(channel) => match channel_get(&current, channel) {
                    Some(n) => Value::Float(n),
                    None => {
                        let message = format!("a {} has no channel {channel}", current.kind_name());
                        return log_drop(scene, message);
                    }
                },
                None => current,
            };
            self.from = Some(from);
        }
        let from = self.from.clone().unwrap_or(Value::Nil);

        let (progress, finished) = sample(self.elapsed - self.delay, self.duration, self.loops, self.yoyo);
        let Some(blended) = interpolate(&from, &self.to, self.ease.apply(progress)) else {
            let message = format!(
                "cannot animate from {} to {}",
                from.kind_name(),
                self.to.kind_name()
            );
            return log_drop(scene, message);
        };

        let result = match path.channel {
            Some(channel) => (|| {
                let mut full = read_full(scene, target, path)?;
                let n = number(&blended).unwrap_or(0.0);
                if !channel_set(&mut full, channel, n) {
                    return Err(format!("a {} has no channel {channel}", full.kind_name()));
                }
                write_full(scene, target, path, &full)
            })(),
            None => write_full(scene, target, path, &blended),
        };
        if let Err(e) = result {
            return log_drop(scene, e);
        }
        if finished {
            Outcome::Finished
        } else {
            Outcome::Running
        }
    }
}

/// Шаг твинов: вызывается из `App::tick` раз в кадр, перед `on_update`.
pub(crate) fn run(world: &mut World) {
    let (dt, unscaled_dt) = (world.time.delta, world.time.unscaled_delta);
    let Some(mut tweens) = world.resources.remove::<Tweens>() else {
        return;
    };
    if tweens.running.is_empty() {
        world.resources.insert(tweens);
        return;
    }

    type Finished = (TweenId, Option<GameObjectId>, Completion);
    let mut finished: Vec<Finished> = Vec::new();
    tweens.running.retain_mut(|r| {
        // Объект умер (уничтожен, сцена заменена) — твин уходит молча.
        if let Some(target) = r.target {
            if !world.scene.contains(target) {
                return false;
            }
            if !r.marker && !world.scene.is_active(target) {
                return true;
            }
        }
        let step = if r.unscaled { unscaled_dt } else { dt };
        match r.advance(&mut world.scene, step) {
            Outcome::Running => true,
            Outcome::Dropped => false,
            Outcome::Finished => {
                finished.push((r.id, r.target, r.on_complete.take()));
                false
            }
        }
    });
    world.resources.insert(tweens);

    for (id, target, on_complete) in finished {
        world.events.emit(TweenFinished { id, target });
        if let Some((to, name, value)) = on_complete {
            world.send_message(to, &name, value);
        }
    }
    // Колбэки о завершении получат сообщения в конце `update` (там вызывается `flush_messages`).
}
