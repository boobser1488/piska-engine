//! Общие типы 2D-отрисовки: вершина спрайта, «заход» (run) в батче и генерация
//! вершин для обычного спрайта. Частицы, тайлмапы и следы пишут вершины в тот же
//! буфер — рендерер сортирует заходы и склеивает соседние с одной текстурой.

use engine_assets::Guid;
use engine_core::prelude::*;

use crate::components::SpriteRenderer;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SpriteVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// Кадр листа (min u, min v, max u, max v): шейдер не даёт выборке уменьшенной копии
    /// (мипа) выйти за него — иначе вдали между плитками проступают соседние кадры.
    pub rect: [f32; 4],
}

/// Чем закрашивать: ассет или одна из встроенных текстур.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TexRef {
    White,
    /// Мягкое круглое пятно (гауссов спад) — частицы по умолчанию.
    Soft,
    /// Круг с чётким краем.
    Disc,
    Asset(Guid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Blend {
    Alpha,
    Additive,
    /// Свой шейдер спрайта (`SpriteShader`): номер пачки «шейдер + параметры»
    /// (renderer/custom.rs), смешивание — альфа.
    Custom(u32),
}

/// Непрерывный кусок вершин в сыром буфере с общим ключом сортировки.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawRun {
    pub sort_key: (i32, i32),
    pub depth: f32,
    pub texture: TexRef,
    pub blend: Blend,
    pub start: u32,
    pub len: u32,
}

/// Направления «вправо» и «вверх» на экране камеры в мировых координатах.
/// Из них строятся billboard-частицы; для 2D-камеры это просто X и Y.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraBasis {
    pub right: Vec3,
    pub up: Vec3,
}

impl CameraBasis {
    pub fn from_view_proj(view_proj: Mat4) -> Self {
        // Строки матрицы проекции×вид — градиенты координат clip-пространства;
        // для симметричной проекции это оси камеры, умноженные на положительное число.
        let rows = view_proj.transpose();
        let norm = |v: Vec3, fallback: Vec3| {
            let n = v.normalize_or_zero();
            if n == Vec3::ZERO {
                fallback
            } else {
                n
            }
        };
        Self {
            right: norm(rows.x_axis.truncate(), Vec3::X),
            up: norm(rows.y_axis.truncate(), Vec3::Y),
        }
    }
}

/// Поправка UV на полтексела внутрь: без неё билинейная фильтрация подмешивает
/// соседний кадр листа. Для пиксель-арта (Nearest) поправка нулевая.
pub(crate) type UvInset = Vec2;

/// Прямоугольник UV: (u0, v0) — верхний левый угол изображения, (u1, v1) — нижний правый.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct UvRect {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

impl UvRect {
    pub const FULL: UvRect = UvRect {
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
    };

    /// Ячейка `index` сетки `columns × rows` внутри прямоугольника `self`
    /// (строки идут сверху вниз, как в картинке). Индекс берётся по модулю.
    pub fn cell(self, columns: u32, rows: u32, index: u32, inset: UvInset) -> UvRect {
        let columns = columns.max(1);
        let rows = rows.max(1);
        let index = index % (columns * rows);
        let (column, row) = (index % columns, index / columns);
        let width = (self.u1 - self.u0) / columns as f32;
        let height = (self.v1 - self.v0) / rows as f32;
        let u0 = self.u0 + width * column as f32;
        let v0 = self.v0 + height * row as f32;
        UvRect {
            u0: u0 + inset.x,
            v0: v0 + inset.y,
            u1: u0 + width - inset.x,
            v1: v0 + height - inset.y,
        }
    }
}

/// UV спрайта с учётом листа, прямоугольника-источника и отражений.
pub(crate) fn sprite_uv(sprite: &SpriteRenderer, inset: UvInset) -> UvRect {
    let base = UvRect {
        u0: sprite.uv_offset.x,
        v0: sprite.uv_offset.y,
        u1: sprite.uv_offset.x + sprite.uv_size.x,
        v1: sprite.uv_offset.y + sprite.uv_size.y,
    };
    let sub_image = sprite.sheet_columns > 1
        || sprite.sheet_rows > 1
        || sprite.uv_offset != Vec2::ZERO
        || sprite.uv_size != Vec2::ONE;
    let mut uv = if sprite.sheet_columns > 1 || sprite.sheet_rows > 1 {
        base.cell(sprite.sheet_columns, sprite.sheet_rows, sprite.frame, inset)
    } else if sub_image {
        UvRect {
            u0: base.u0 + inset.x,
            v0: base.v0 + inset.y,
            u1: base.u1 - inset.x,
            v1: base.v1 - inset.y,
        }
    } else {
        base
    };
    if sprite.flip_x {
        std::mem::swap(&mut uv.u0, &mut uv.u1);
    }
    if sprite.flip_y {
        std::mem::swap(&mut uv.v0, &mut uv.v1);
    }
    uv
}

/// Четыре вершины четырёхугольника (углы по порядку: левый нижний, правый нижний,
/// правый верхний, левый верхний). Треугольники (0 1 2) и (0 2 3) задаёт общий индексный
/// буфер квадов рендерера — вершин на треть меньше, чем при шести на квад.
pub(crate) const QUAD_VERTICES: usize = 4;

pub(crate) fn push_quad(out: &mut Vec<SpriteVertex>, corners: [Vec3; 4], uv: UvRect, color: [f32; 4]) {
    let uvs = [[uv.u0, uv.v1], [uv.u1, uv.v1], [uv.u1, uv.v0], [uv.u0, uv.v0]];
    let rect = [uv.u0.min(uv.u1), uv.v0.min(uv.v1), uv.u0.max(uv.u1), uv.v0.max(uv.v1)];
    let vertex = |i: usize| SpriteVertex {
        position: corners[i].to_array(),
        uv: uvs[i],
        color,
        rect,
    };
    out.extend_from_slice(&[vertex(0), vertex(1), vertex(2), vertex(3)]);
}

/// Углы прямоугольника спрайта в мире с учётом точки опоры.
pub(crate) fn sprite_corners(sprite: &SpriteRenderer, matrix: Mat4) -> [Vec3; 4] {
    let size = sprite.size;
    let left = -sprite.pivot.x * size.x;
    let right = (1.0 - sprite.pivot.x) * size.x;
    let bottom = -sprite.pivot.y * size.y;
    let top = (1.0 - sprite.pivot.y) * size.y;
    [(left, bottom), (right, bottom), (right, top), (left, top)]
        .map(|(x, y)| matrix.transform_point3(Vec3::new(x, y, 0.0)))
}
