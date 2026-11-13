//! Кисть: превращает фигуры интерфейса в четырёхугольники экранного слоя.

use engine_core::math::{Color, Vec2};
use engine_render::{Overlay, OverlayTexture, OverlayVertex};

use crate::layout::Rect;
use crate::text::ATLAS_ID;

/// Цвет с домноженной прозрачностью ветки (CanvasGroup).
pub(crate) fn fade(color: Color, alpha: f32) -> Color {
    color.with_alpha(color.a * alpha)
}

pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

pub(crate) struct Painter<'a> {
    pub overlay: &'a mut Overlay,
    /// UV белого квадрата в атласе.
    pub white: [f32; 2],
    /// Сторона атласа в пикселях.
    pub atlas_size: f32,
    /// Текущая область отсечения `[x, y, w, h]`.
    pub clip: Option<[f32; 4]>,
}

/// Запас по краю четырёхугольника под сглаживание.
const AA: f32 = 1.0;

impl Painter<'_> {
    fn push(&mut self, texture: OverlayTexture, quad: [OverlayVertex; 4]) {
        self.overlay.push_quad(texture, self.clip, quad);
    }

    /// Скруглённый прямоугольник с рамкой (рамка рисуется внутрь).
    pub fn rect(&mut self, rect: Rect, fill: Color, radius: f32, border: f32, border_color: Color) {
        let has_border = border > 0.0 && border_color.a > 0.0;
        if fill.a <= 0.0 && !has_border {
            return;
        }
        let half = rect.size() * 0.5;
        if half.x <= 0.0 || half.y <= 0.0 {
            return;
        }
        let shape = [half.x, half.y, radius.max(0.0), if has_border { border } else { 0.0 }];
        self.shape(rect.center(), half + Vec2::splat(AA), shape, 0.0, fill, border_color, self.white_texture());
    }

    /// Мягкая тень прямоугольника: размытие `blur` пикселей.
    pub fn shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
        if color.a <= 0.0 || blur <= 0.0 {
            return;
        }
        let half = rect.size() * 0.5;
        if half.x <= 0.0 || half.y <= 0.0 {
            return;
        }
        let shape = [half.x, half.y, radius.max(0.0), 0.0];
        self.shape(
            rect.center(),
            half + Vec2::splat(blur + AA),
            shape,
            blur,
            color,
            Color::TRANSPARENT,
            self.white_texture(),
        );
    }

    /// Отрезок с круглыми концами (галочка).
    pub fn line(&mut self, from: Vec2, to: Vec2, thickness: f32, color: Color) {
        let delta = to - from;
        let length = delta.length();
        if length <= 0.0 || color.a <= 0.0 {
            return;
        }
        let dir = delta / length;
        let normal = Vec2::new(-dir.y, dir.x);
        let half = Vec2::new(length * 0.5 + thickness * 0.5, thickness * 0.5);
        let shape = [half.x, half.y, thickness * 0.5, 0.0];
        let center = (from + to) * 0.5;
        let grown = half + Vec2::splat(AA);
        let uv = self.white;
        let corners = [
            Vec2::new(-grown.x, -grown.y),
            Vec2::new(grown.x, -grown.y),
            Vec2::new(grown.x, grown.y),
            Vec2::new(-grown.x, grown.y),
        ];
        let quad = corners.map(|local| OverlayVertex {
            position: (center + dir * local.x + normal * local.y).to_array(),
            uv,
            color: color.to_array(),
            border_color: [0.0; 4],
            shape,
            local: [local.x, local.y, 0.0, 0.0],
        });
        self.push(self.white_texture(), quad);
    }

    /// Картинка из атласа (глиф): `min` и `size` — пиксели, `origin`/`extent` — пиксели атласа.
    pub fn glyph(&mut self, min: Vec2, size: Vec2, origin: [u32; 2], extent: [u32; 2], color: Color) {
        let inv = 1.0 / self.atlas_size;
        let u0 = origin[0] as f32 * inv;
        let v0 = origin[1] as f32 * inv;
        let u1 = (origin[0] + extent[0]) as f32 * inv;
        let v1 = (origin[1] + extent[1]) as f32 * inv;
        let max = min + size;
        let c = color.to_array();
        let vertex = |x: f32, y: f32, u: f32, v: f32| OverlayVertex {
            position: [x, y],
            uv: [u, v],
            color: c,
            ..OverlayVertex::default()
        };
        self.push(
            OverlayTexture::Image(ATLAS_ID),
            [
                vertex(min.x, min.y, u0, v0),
                vertex(max.x, min.y, u1, v0),
                vertex(max.x, max.y, u1, v1),
                vertex(min.x, max.y, u0, v1),
            ],
        );
    }

    /// Текстура ассета на прямоугольнике (`uv` — `[u0, v0, u1, v1]`) со скруглением.
    /// `bounds` — прямоугольник всей картинки (для скругления углов у 9-slice).
    pub fn textured(&mut self, texture: OverlayTexture, rect: Rect, uv: [f32; 4], color: Color, bounds: Rect, radius: f32) {
        if color.a <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let center = bounds.center();
        let half = bounds.size() * 0.5;
        let shape = if radius > 0.0 { [half.x, half.y, radius, 0.0] } else { [0.0; 4] };
        let c = color.to_array();
        let vertex = |p: Vec2, u: f32, v: f32| OverlayVertex {
            position: p.to_array(),
            uv: [u, v],
            color: c,
            border_color: [0.0; 4],
            shape,
            local: [p.x - center.x, p.y - center.y, 0.0, 0.0],
        };
        self.push(
            texture,
            [
                vertex(rect.min, uv[0], uv[1]),
                vertex(Vec2::new(rect.max.x, rect.min.y), uv[2], uv[1]),
                vertex(rect.max, uv[2], uv[3]),
                vertex(Vec2::new(rect.min.x, rect.max.y), uv[0], uv[3]),
            ],
        );
    }

    fn white_texture(&self) -> OverlayTexture {
        OverlayTexture::Image(ATLAS_ID)
    }

    /// Четырёхугольник `center ± extent` с фигурой `shape` (см. `OverlayVertex::shape`).
    #[allow(clippy::too_many_arguments)]
    fn shape(
        &mut self,
        center: Vec2,
        extent: Vec2,
        shape: [f32; 4],
        softness: f32,
        fill: Color,
        border: Color,
        texture: OverlayTexture,
    ) {
        let uv = self.white;
        let corners = [
            Vec2::new(-extent.x, -extent.y),
            Vec2::new(extent.x, -extent.y),
            Vec2::new(extent.x, extent.y),
            Vec2::new(-extent.x, extent.y),
        ];
        let quad = corners.map(|local| OverlayVertex {
            position: (center + local).to_array(),
            uv,
            color: fill.to_array(),
            border_color: border.to_array(),
            shape,
            local: [local.x, local.y, softness, 0.0],
        });
        self.push(texture, quad);
    }
}
