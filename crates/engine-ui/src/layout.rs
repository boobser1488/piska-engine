//! Математика раскладки: масштаб холста, якоря `RectTransform`, раскладка детей.
//! Всё — чистые функции над пикселями экрана, без сцены.

use engine_core::math::Vec2;

use crate::components::{Canvas, LayoutDirection, LayoutGroup, RectTransform, ScaleMode};

/// Прямоугольник в пикселях экрана: `min` — левый верх, `max` — правый низ.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            min: Vec2::new(x, y),
            max: Vec2::new(x + width, y + height),
        }
    }

    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self { min, max: min + size }
    }

    pub fn size(&self) -> Vec2 {
        (self.max - self.min).max(Vec2::ZERO)
    }

    pub fn width(&self) -> f32 {
        self.size().x
    }

    pub fn height(&self) -> f32 {
        self.size().y
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.min.x && point.x < self.max.x && point.y >= self.min.y && point.y < self.max.y
    }

    /// Сжать на `by` со всех сторон (отрицательное — расширить).
    pub fn shrink(&self, by: f32) -> Self {
        Self {
            min: self.min + Vec2::splat(by),
            max: self.max - Vec2::splat(by),
        }
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let min = self.min.max(other.min);
        Rect {
            min,
            max: self.max.min(other.max).max(min),
        }
    }

    /// Края по целым пикселям — рамки и текст остаются резкими.
    pub fn snapped(&self) -> Rect {
        Rect {
            min: self.min.round(),
            max: self.max.round(),
        }
    }

    pub fn translate(&self, by: Vec2) -> Rect {
        Rect {
            min: self.min + by,
            max: self.max + by,
        }
    }
}

/// Пикселей экрана на единицу холста.
pub fn canvas_scale(canvas: &Canvas, screen: Vec2) -> f32 {
    let scale = match canvas.scale_mode {
        ScaleMode::ConstantPixelSize => canvas.scale_factor,
        ScaleMode::ScaleWithScreenSize => {
            let reference = canvas.reference_resolution.max(Vec2::ONE);
            // Как в Unity: среднее в логарифмах, чтобы 2× по одной оси и 0.5× по другой
            // давали 1×, а не 1.25×.
            let t = canvas.match_width_or_height.clamp(0.0, 1.0);
            let log_w = (screen.x.max(1.0) / reference.x).log2();
            let log_h = (screen.y.max(1.0) / reference.y).log2();
            (log_w + (log_h - log_w) * t).exp2()
        }
    };
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Прямоугольник элемента внутри `parent` (пиксели). `size_override` — размер в
/// пикселях вместо `rt.size * scale` по осям, где якоря не разведены (подгонка под
/// содержимое).
pub fn resolve_rect(rt: &RectTransform, parent: Rect, scale: f32, size_override: Option<Vec2>) -> Rect {
    let parent_size = parent.size();
    let anchor_min = parent.min + parent_size * rt.anchor_min;
    let anchor_max = parent.min + parent_size * rt.anchor_max;
    let span = anchor_max - anchor_min;
    let mut size = span + rt.size * scale;
    if let Some(fit) = size_override {
        if (rt.anchor_max.x - rt.anchor_min.x).abs() < f32::EPSILON {
            size.x = fit.x;
        }
        if (rt.anchor_max.y - rt.anchor_min.y).abs() < f32::EPSILON {
            size.y = fit.y;
        }
    }
    let size = size.max(Vec2::ZERO);
    let reference = anchor_min + span * rt.pivot;
    let pivot_point = reference + rt.position * scale;
    let min = pivot_point - size * rt.pivot;
    Rect::from_min_size(min, size)
}

/// Отступы группы в пикселях: (лево, верх, право, низ).
fn padding(group: &LayoutGroup, scale: f32) -> (f32, f32, f32, f32) {
    (
        group.padding_left * scale,
        group.padding_top * scale,
        group.padding_right * scale,
        group.padding_bottom * scale,
    )
}

/// Оси раскладки: (вдоль, поперёк) для вектора.
fn split(direction: LayoutDirection, v: Vec2) -> (f32, f32) {
    match direction {
        LayoutDirection::Vertical => (v.y, v.x),
        LayoutDirection::Horizontal => (v.x, v.y),
    }
}

fn join(direction: LayoutDirection, main: f32, cross: f32) -> Vec2 {
    match direction {
        LayoutDirection::Vertical => Vec2::new(cross, main),
        LayoutDirection::Horizontal => Vec2::new(main, cross),
    }
}

/// Сколько места нужно группе под детей с размерами `children` (пиксели), с отступами.
/// Поперёк — наибольший ребёнок; `own` — собственный размер, если дети растягиваются.
pub fn content_size(group: &LayoutGroup, scale: f32, children: &[Vec2], own: Vec2) -> Vec2 {
    let (left, top, right, bottom) = padding(group, scale);
    let mut main = 0.0;
    let mut cross: f32 = 0.0;
    for child in children {
        let (m, c) = split(group.direction, *child);
        main += m;
        cross = cross.max(c);
    }
    main += group.spacing * scale * children.len().saturating_sub(1) as f32;
    let (pad_main, pad_cross) = split(group.direction, Vec2::new(left + right, top + bottom));
    let (_, own_cross) = split(group.direction, own);
    let cross = if group.stretch_children {
        own_cross
    } else {
        cross + pad_cross
    };
    join(group.direction, main + pad_main, cross)
}

/// Разложить детей с желаемыми размерами `children` (пиксели) внутри `rect`.
/// Результат дописывается в `out` в том же порядке.
pub fn layout_children(group: &LayoutGroup, rect: Rect, scale: f32, children: &[Vec2], out: &mut Vec<Rect>) {
    let (left, top, right, bottom) = padding(group, scale);
    let inner = Rect {
        min: rect.min + Vec2::new(left, top),
        max: (rect.max - Vec2::new(right, bottom)).max(rect.min + Vec2::new(left, top)),
    };
    let direction = group.direction;
    let (avail_main, avail_cross) = split(direction, inner.size());
    let spacing = group.spacing * scale;
    let count = children.len();
    if count == 0 {
        return;
    }
    let total: f32 = children.iter().map(|c| split(direction, *c).0).sum::<f32>()
        + spacing * (count - 1) as f32;
    let mut free = avail_main - total;
    // `expand_children`: свободное место (или нехватку) делим поровну — дети
    // заполняют группу точно, не вылезая за край.
    let extra = if group.expand_children {
        let each = free / count as f32;
        free = 0.0;
        each
    } else {
        0.0
    };
    let (align_main, align_cross) = split(direction, group.child_alignment.fractions());
    let (inner_main, inner_cross) = split(direction, inner.min);
    let mut cursor = inner_main + free.max(0.0) * align_main;
    for child in children {
        let (main, cross) = split(direction, *child);
        let main = (main + extra).max(0.0);
        let (cross, offset) = if group.stretch_children {
            (avail_cross, 0.0)
        } else {
            (cross, (avail_cross - cross) * align_cross)
        };
        let min = join(direction, cursor, inner_cross + offset);
        out.push(Rect::from_min_size(min, join(direction, main, cross)));
        cursor += main + spacing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Alignment, Anchor};

    fn approx(a: Vec2, b: Vec2) {
        assert!((a - b).length() < 1e-3, "{a:?} != {b:?}");
    }

    #[test]
    fn canvas_scale_matches_unity_formula() {
        let canvas = Canvas::default(); // 1280×720, match 0.5
        assert!((canvas_scale(&canvas, Vec2::new(1280.0, 720.0)) - 1.0).abs() < 1e-5);
        assert!((canvas_scale(&canvas, Vec2::new(2560.0, 1440.0)) - 2.0).abs() < 1e-5);
        // Шире в 2 раза, той же высоты: среднее логарифмов → √2.
        assert!((canvas_scale(&canvas, Vec2::new(2560.0, 720.0)) - 2f32.sqrt()).abs() < 1e-4);
        let by_height = Canvas {
            match_width_or_height: 1.0,
            ..Canvas::default()
        };
        assert!((canvas_scale(&by_height, Vec2::new(2560.0, 360.0)) - 0.5).abs() < 1e-5);
        let constant = Canvas {
            scale_mode: ScaleMode::ConstantPixelSize,
            scale_factor: 1.5,
            ..Canvas::default()
        };
        assert_eq!(canvas_scale(&constant, Vec2::new(10.0, 10.0)), 1.5);
    }

    #[test]
    fn point_anchors_place_pivot_at_offset_from_anchor() {
        let parent = Rect::new(0.0, 0.0, 800.0, 600.0);
        let centered = RectTransform::anchored(Anchor::Center, Vec2::new(10.0, -20.0), Vec2::new(200.0, 100.0));
        let rect = resolve_rect(&centered, parent, 1.0, None);
        approx(rect.min, Vec2::new(310.0, 230.0));
        approx(rect.size(), Vec2::new(200.0, 100.0));

        let corner = RectTransform::anchored(Anchor::BottomRight, Vec2::new(-16.0, -16.0), Vec2::new(64.0, 32.0));
        let rect = resolve_rect(&corner, parent, 2.0, None);
        approx(rect.max, Vec2::new(800.0 - 32.0, 600.0 - 32.0));
        approx(rect.size(), Vec2::new(128.0, 64.0));
    }

    #[test]
    fn stretched_anchors_add_size_to_anchor_span() {
        let parent = Rect::new(100.0, 50.0, 400.0, 200.0);
        let full = resolve_rect(&RectTransform::stretch(), parent, 1.0, None);
        assert_eq!(full, parent);
        let inset = resolve_rect(&RectTransform::inset(10.0, 20.0, 30.0, 40.0), parent, 1.0, None);
        approx(inset.min, Vec2::new(110.0, 70.0));
        approx(inset.max, Vec2::new(470.0, 210.0));
        // Полоса вдоль верха высотой 48.
        let bar = RectTransform::anchored(Anchor::StretchTop, Vec2::ZERO, Vec2::new(0.0, 48.0));
        let rect = resolve_rect(&bar, parent, 1.0, None);
        approx(rect.min, parent.min);
        approx(rect.size(), Vec2::new(400.0, 48.0));
        // Подгонка под содержимое меняет только «точечные» оси.
        let rect = resolve_rect(&bar, parent, 1.0, Some(Vec2::new(5.0, 90.0)));
        approx(rect.size(), Vec2::new(400.0, 90.0));
    }

    #[test]
    fn vertical_group_stacks_with_spacing_padding_and_stretch() {
        let group = LayoutGroup::vertical(10.0).with_padding(20.0);
        let rect = Rect::new(0.0, 0.0, 300.0, 400.0);
        let mut out = Vec::new();
        layout_children(&group, rect, 1.0, &[Vec2::new(100.0, 50.0), Vec2::new(80.0, 30.0)], &mut out);
        assert_eq!(out[0], Rect::new(20.0, 20.0, 260.0, 50.0));
        assert_eq!(out[1], Rect::new(20.0, 80.0, 260.0, 30.0));

        // Без растяжения, по центру, с масштабом 2.
        let centered = LayoutGroup {
            stretch_children: false,
            child_alignment: Alignment::Center,
            ..LayoutGroup::vertical(10.0)
        };
        out.clear();
        layout_children(&centered, rect, 2.0, &[Vec2::new(100.0, 50.0), Vec2::new(80.0, 30.0)], &mut out);
        // Всего 50 + 20 + 30 = 100, свободно 300 → сверху 150.
        assert_eq!(out[0], Rect::new(100.0, 150.0, 100.0, 50.0));
        assert_eq!(out[1], Rect::new(110.0, 220.0, 80.0, 30.0));
    }

    #[test]
    fn horizontal_group_expands_children_into_free_space() {
        let group = LayoutGroup {
            expand_children: true,
            ..LayoutGroup::horizontal(10.0)
        };
        let mut out = Vec::new();
        layout_children(&group, Rect::new(0.0, 0.0, 330.0, 40.0), 1.0, &[Vec2::new(100.0, 10.0); 3], &mut out);
        assert_eq!(out.len(), 3);
        assert!((out[0].width() - 103.333).abs() < 0.01);
        assert!((out[2].max.x - 330.0).abs() < 0.01);
        assert_eq!(out[1].height(), 40.0);

        // Не хватает места — дети сжимаются поровну и не вылезают за край.
        out.clear();
        layout_children(&group, Rect::new(0.0, 0.0, 210.0, 40.0), 1.0, &[Vec2::new(100.0, 10.0); 3], &mut out);
        assert!((out[0].width() - 63.333).abs() < 0.01);
        assert!((out[2].max.x - 210.0).abs() < 0.01);
    }

    #[test]
    fn content_size_sums_main_axis_and_takes_max_cross() {
        let group = LayoutGroup {
            stretch_children: false,
            ..LayoutGroup::vertical(8.0).with_padding(12.0)
        };
        let size = content_size(&group, 1.0, &[Vec2::new(100.0, 40.0), Vec2::new(160.0, 40.0)], Vec2::ZERO);
        assert_eq!(size, Vec2::new(184.0, 112.0));
        let stretched = LayoutGroup::vertical(8.0).with_padding(12.0);
        let size = content_size(&stretched, 1.0, &[Vec2::new(100.0, 40.0)], Vec2::new(300.0, 0.0));
        assert_eq!(size, Vec2::new(300.0, 64.0));
    }
}
