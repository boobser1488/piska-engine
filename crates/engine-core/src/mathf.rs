//! Помощники в духе Unity `Mathf`, `Rect` и расширения `Color`.
//! Всё это реэкспортируется из [`crate::math`] и из `prelude`.

use std::f32::consts::{PI, TAU};

use crate::math::{Color, Vec2, Vec3};
use crate::reflect::{FieldValue, Value};

// ----------------------------------------------------------------------
// Цвет: дополнительные конструкторы и операции
// ----------------------------------------------------------------------

impl Color {
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    pub const CYAN: Color = Color::rgb(0.0, 1.0, 1.0);
    pub const MAGENTA: Color = Color::rgb(1.0, 0.0, 1.0);

    /// Цвет из sRGB-каналов 0..=1 (как их видит дизайнер); хранится в линейном виде.
    pub fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgb(srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b))
    }

    /// Цвет из `0xRRGGBBAA` (sRGB, альфа линейна).
    pub fn hex_rgba(rgba: u32) -> Self {
        Self::hex(rgba >> 8).with_alpha((rgba & 0xff) as f32 / 255.0)
    }

    /// Разбор строки `"#ff7a59"`, `"ff7a59"`, `"#ff7a59cc"` (с альфой) или `"#f80"`.
    /// `None`, если строка не похожа на цвет.
    pub fn parse_hex(text: &str) -> Option<Self> {
        let digits = text.trim().trim_start_matches('#');
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        match digits.len() {
            3 => {
                let v = u32::from_str_radix(digits, 16).ok()?;
                let expand = |n: u32| n * 17;
                Some(Self::hex(
                    expand((v >> 8) & 0xf) << 16 | expand((v >> 4) & 0xf) << 8 | expand(v & 0xf),
                ))
            }
            6 => Some(Self::hex(u32::from_str_radix(digits, 16).ok()?)),
            8 => Some(Self::hex_rgba(u32::from_str_radix(digits, 16).ok()?)),
            _ => None,
        }
    }

    /// Цвет из HSV: `h` — оттенок 0..1 (циклический), `s` и `v` — 0..1.
    /// Считается в sRGB, как в графическом редакторе, затем переводится в линейное
    /// пространство — так же, как [`Color::hex`].
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(1.0) * 6.0;
        let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        let channel = |n: f32| {
            let k = (n + h) % 6.0;
            v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
        };
        Self::srgb(channel(5.0), channel(3.0), channel(1.0))
    }

    /// Обратно в `(h, s, v)`, всё в 0..1 (sRGB, см. [`Color::from_hsv`]).
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let (r, g, b) = (linear_to_srgb(self.r), linear_to_srgb(self.g), linear_to_srgb(self.b));
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d <= 1e-6 {
            0.0
        } else if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
        (h, if max <= 1e-6 { 0.0 } else { d / max }, max)
    }

    /// Линейная интерполяция всех четырёх каналов (в линейном пространстве).
    pub fn lerp(self, other: Color, t: f32) -> Color {
        Color::rgba(
            lerp(self.r, other.r, t),
            lerp(self.g, other.g, t),
            lerp(self.b, other.b, t),
            lerp(self.a, other.a, t),
        )
    }

    /// Умножить RGB на `factor` (альфа не меняется): `> 1` — ярче (HDR), `< 1` — темнее.
    pub fn scaled(self, factor: f32) -> Color {
        Color::rgba(self.r * factor, self.g * factor, self.b * factor, self.a)
    }

    /// Покомпонентное произведение (тонирование).
    pub fn multiply(self, other: Color) -> Color {
        Color::rgba(self.r * other.r, self.g * other.g, self.b * other.b, self.a * other.a)
    }

    /// Яркость (Rec. 709, линейные каналы).
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// `#rrggbb` (sRGB), без альфы.
    pub fn to_hex_string(self) -> String {
        let byte = |c: f32| (linear_to_srgb(c).clamp(0.0, 1.0) * 255.0).round() as u32;
        format!("#{:02x}{:02x}{:02x}", byte(self.r), byte(self.g), byte(self.b))
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// ----------------------------------------------------------------------
// Скалярные помощники (Unity: Mathf)
// ----------------------------------------------------------------------

/// Unity: `Mathf.LerpUnclamped` — `t` не ограничивается.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Unity: `Mathf.InverseLerp` — где `value` между `a` и `b` (без ограничения; 0, если `a == b`).
pub fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if (b - a).abs() <= f32::EPSILON {
        0.0
    } else {
        (value - a) / (b - a)
    }
}

/// Перенести `value` из диапазона `[from_min, from_max]` в `[to_min, to_max]` (без ограничения).
pub fn remap(value: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    lerp(to_min, to_max, inverse_lerp(from_min, from_max, value))
}

/// Кубический сглаженный переход 0..1 (`t` ограничивается).
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Квинтический (более мягкие концы, чем у [`smoothstep`]).
pub fn smootherstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Unity: `Mathf.MoveTowards` — шаг к цели не длиннее `max_delta`, перелёта нет.
pub fn move_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    if (target - current).abs() <= max_delta {
        target
    } else {
        current + (target - current).signum() * max_delta
    }
}

pub fn move_towards_vec2(current: Vec2, target: Vec2, max_delta: f32) -> Vec2 {
    let diff = target - current;
    let dist = diff.length();
    if dist <= max_delta || dist <= f32::EPSILON {
        target
    } else {
        current + diff / dist * max_delta
    }
}

pub fn move_towards_vec3(current: Vec3, target: Vec3, max_delta: f32) -> Vec3 {
    let diff = target - current;
    let dist = diff.length();
    if dist <= max_delta || dist <= f32::EPSILON {
        target
    } else {
        current + diff / dist * max_delta
    }
}

/// Сглаживание, не зависящее от частоты кадров: `x = exp_decay(x, target, 8.0, dt)`.
/// `decay` — «жёсткость»: чем больше, тем быстрее приближение (около `1/decay` секунды).
pub fn exp_decay(current: f32, target: f32, decay: f32, dt: f32) -> f32 {
    lerp(target, current, (-decay * dt).exp())
}

/// Unity: `Mathf.SmoothDamp` — пружинное сглаживание без перелёта. `velocity` хранится
/// у вызывающего между кадрами; `smooth_time` — примерное время достижения цели.
pub fn smooth_damp(current: f32, target: f32, velocity: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let smooth_time = smooth_time.max(0.0001);
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * exp;
    let mut out = target + (change + temp) * exp;
    // Не перелетать цель.
    if (target - current > 0.0) == (out > target) {
        out = target;
        *velocity = 0.0;
    }
    out
}

pub fn smooth_damp_vec2(current: Vec2, target: Vec2, velocity: &mut Vec2, smooth_time: f32, dt: f32) -> Vec2 {
    Vec2::new(
        smooth_damp(current.x, target.x, &mut velocity.x, smooth_time, dt),
        smooth_damp(current.y, target.y, &mut velocity.y, smooth_time, dt),
    )
}

pub fn smooth_damp_vec3(current: Vec3, target: Vec3, velocity: &mut Vec3, smooth_time: f32, dt: f32) -> Vec3 {
    Vec3::new(
        smooth_damp(current.x, target.x, &mut velocity.x, smooth_time, dt),
        smooth_damp(current.y, target.y, &mut velocity.y, smooth_time, dt),
        smooth_damp(current.z, target.z, &mut velocity.z, smooth_time, dt),
    )
}

/// Unity: `Mathf.Repeat` — `t` по модулю `length` (результат в `0..length`).
pub fn repeat(t: f32, length: f32) -> f32 {
    if length <= 0.0 {
        0.0
    } else {
        t.rem_euclid(length)
    }
}

/// Unity: `Mathf.PingPong` — «туда-обратно» между 0 и `length`.
pub fn ping_pong(t: f32, length: f32) -> f32 {
    let t = repeat(t, length * 2.0);
    length - (t - length).abs()
}

/// Равны ли числа с относительным допуском `1e-5`.
pub fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-5 * a.abs().max(b.abs()).max(1.0)
}

// ----------------------------------------------------------------------
// Углы
// ----------------------------------------------------------------------

/// Привести угол (радианы) к диапазону `-π..=π`.
pub fn wrap_angle(angle: f32) -> f32 {
    let a = angle.rem_euclid(TAU);
    if a > PI {
        a - TAU
    } else {
        a
    }
}

/// Кратчайшая разность углов `to - from` в радианах, `-π..=π` (Unity: `Mathf.DeltaAngle`).
pub fn delta_angle(from: f32, to: f32) -> f32 {
    wrap_angle(to - from)
}

/// Интерполяция угла по кратчайшей дуге (радианы).
pub fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    from + delta_angle(from, to) * t
}

/// Шаг угла к цели по кратчайшей дуге не длиннее `max_delta` (радианы).
pub fn move_towards_angle(current: f32, target: f32, max_delta: f32) -> f32 {
    let delta = delta_angle(current, target);
    if delta.abs() <= max_delta {
        current + delta
    } else {
        current + delta.signum() * max_delta
    }
}

/// То же, что [`delta_angle`], но в градусах (`-180..=180`).
pub fn delta_angle_deg(from: f32, to: f32) -> f32 {
    delta_angle(from.to_radians(), to.to_radians()).to_degrees()
}

/// То же, что [`lerp_angle`], но в градусах.
pub fn lerp_angle_deg(from: f32, to: f32, t: f32) -> f32 {
    from + delta_angle_deg(from, to) * t
}

/// Угол вектора в радианах (`atan2(y, x)`): 0 — вправо, против часовой стрелки.
pub fn angle_of(v: Vec2) -> f32 {
    v.y.atan2(v.x)
}

/// Единичный вектор по углу в радианах.
pub fn from_angle(angle: f32) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

// ----------------------------------------------------------------------
// Rect
// ----------------------------------------------------------------------

/// Прямоугольник со сторонами вдоль осей: левый нижний угол + размер (ось Y вверх,
/// как в мире 2D-игры).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self::new(min.x, min.y, max.x - min.x, max.y - min.y)
    }

    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        Self::new(center.x - size.x * 0.5, center.y - size.y * 0.5, size.x, size.y)
    }

    pub fn min(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn max(&self) -> Vec2 {
        Vec2::new(self.x + self.width, self.y + self.height)
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    /// Точка внутри (границы включены).
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    /// Пересекаются ли прямоугольники (касание сторонами не считается).
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }

    /// Общая часть; `None`, если прямоугольники не пересекаются.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let min = self.min().max(other.min());
        let max = self.max().min(other.max());
        (max.x > min.x && max.y > min.y).then(|| Rect::from_min_max(min, max))
    }

    /// Наименьший прямоугольник, содержащий оба.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect::from_min_max(self.min().min(other.min()), self.max().max(other.max()))
    }

    /// Раздвинуть все стороны на `amount` (отрицательное — сжать).
    pub fn expand(&self, amount: f32) -> Rect {
        Rect::new(
            self.x - amount,
            self.y - amount,
            self.width + amount * 2.0,
            self.height + amount * 2.0,
        )
    }

    pub fn translate(&self, delta: Vec2) -> Rect {
        Rect::new(self.x + delta.x, self.y + delta.y, self.width, self.height)
    }

    /// Ближайшая к `p` точка внутри прямоугольника.
    pub fn clamp_point(&self, p: Vec2) -> Vec2 {
        p.clamp(self.min(), self.max())
    }

    /// Точка прямоугольника по нормализованным координатам (0,0 — min, 1,1 — max).
    pub fn point_at(&self, normalized: Vec2) -> Vec2 {
        self.min() + self.size() * normalized
    }

    /// Нормализованные координаты точки (Unity: `Rect.PointToNormalized`).
    pub fn normalized_point(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            inverse_lerp(self.x, self.x + self.width, p.x),
            inverse_lerp(self.y, self.y + self.height, p.y),
        )
    }
}

/// `Rect` можно держать в поле компонента: в сцене это `List([x, y, width, height])`.
impl FieldValue for Rect {
    const EXPECTED: &'static str = "List(x, y, width, height)";
    fn to_value(&self) -> Value {
        Value::List(
            [self.x, self.y, self.width, self.height]
                .into_iter()
                .map(Value::Float)
                .collect(),
        )
    }
    fn from_value(value: &Value) -> Option<Self> {
        let Value::List(items) = value else {
            return None;
        };
        let nums: Option<Vec<f32>> = items.iter().map(f32::from_value).collect();
        match nums?.as_slice() {
            [x, y, w, h] => Some(Rect::new(*x, *y, *w, *h)),
            _ => None,
        }
    }
}
