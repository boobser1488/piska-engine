//! Математика: реэкспорт `glam` и цвет.

pub use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

/// Скалярные помощники, углы и `Rect` живут в [`crate::mathf`] и доступны отсюда же.
pub use crate::mathf::*;

/// Цвет в линейном пространстве (то, с чем работает шейдер).
/// Для «дизайнерских» значений из палитры используйте [`Color::hex`] — он
/// переводит sRGB в линейное пространство.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `Color::hex(0xff7a59)` — цвет как в графическом редакторе (sRGB).
    pub fn hex(rgb: u32) -> Self {
        let channel = |shift: u32| srgb_to_linear(((rgb >> shift) & 0xff) as f32 / 255.0);
        Self::rgb(channel(16), channel(8), channel(0))
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
