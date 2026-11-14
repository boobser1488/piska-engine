//! Линии поверх видов Scene: контуры выделения, рамки камер, коллайдеры, круг кисти
//! рельефа, оси гизмо. Одна реализация на весь редактор, чтобы линии не «выстреливали»:
//!
//! * точка позади камеры не проецируется — отрезок сначала обрезается плоскостью у
//!   камеры (в пространстве отсечения, `w ≥ NEAR`), иначе её проекция улетает за экран
//!   на другую сторону;
//! * контуры рисуются по рёбрам (`line_segment`), а не одной ломаной `closed_line`: у
//!   ломаной на острых углах и рёбрах нулевой длины (сплющенный объект, камера под углом
//!   в 2D) «митра» стыка уходит в бесконечность;
//! * точки с NaN / ∞ пропускаются, а слишком далёкие отрезки подрезаются прямоугольником
//!   вокруг вида — у egui не набирается гигантских координат.

use egui::{Painter, Pos2, Rect, Stroke};
use engine_core::prelude::{Vec3, Vec4};

/// Плоскость отсечения у камеры: `w` в пространстве отсечения (для перспективы — глубина).
pub(crate) const NEAR: f32 = 1e-2;
/// Насколько за край вида (в его размерах) ещё рисуются концы отрезков.
const MARGIN: f32 = 4.0;

pub(crate) fn finite(p: Pos2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Обрезать отрезок пространства отсечения плоскостью `w = NEAR`. `None` — весь позади.
pub(crate) fn clip_near(a: Vec4, b: Vec4) -> Option<(Vec4, Vec4)> {
    if !(a.is_finite() && b.is_finite()) {
        return None;
    }
    match (a.w >= NEAR, b.w >= NEAR) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, b.lerp(a, (NEAR - b.w) / (a.w - b.w)))),
        (false, true) => Some((a.lerp(b, (NEAR - a.w) / (b.w - a.w)), b)),
    }
}

/// Обрезать отрезок прямоугольником (Лян — Барски). `None` — отрезок снаружи.
pub(crate) fn clip_rect(a: Pos2, b: Pos2, rect: Rect) -> Option<(Pos2, Pos2)> {
    if !(finite(a) && finite(b)) {
        return None;
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (p, q) in [
        (-d.x, a.x - rect.min.x),
        (d.x, rect.max.x - a.x),
        (-d.y, a.y - rect.min.y),
        (d.y, rect.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((a + d * t0, a + d * t1))
}

/// Область, в которой ещё рисуются отрезки: вид с запасом в `MARGIN` его размеров.
fn bounds(painter: &Painter) -> Rect {
    let clip = painter.clip_rect();
    clip.expand2(clip.size() * MARGIN)
}

/// Отрезок экрана: без NaN, подрезанный вокруг вида.
pub(crate) fn segment(painter: &Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    if let Some((a, b)) = clip_rect(a, b, bounds(painter)) {
        painter.line_segment([a, b], stroke);
    }
}

/// Ломаная (замкнутая, если `closed`) по рёбрам: без стыков-«митр», рёбра нулевой длины
/// и точки с NaN пропускаются.
pub(crate) fn polyline(painter: &Painter, points: &[Pos2], closed: bool, stroke: Stroke) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let edges = if closed { n } else { n - 1 };
    for i in 0..edges {
        let (a, b) = (points[i], points[(i + 1) % n]);
        if a.distance_sq(b) > 1e-6 {
            segment(painter, a, b, stroke);
        }
    }
}

/// Отрезок мира через проекцию `view_proj` в прямоугольник `rect`: обрезка у камеры, потом
/// перевод в точки окна. `None` — отрезок целиком позади камеры.
pub(crate) fn project_segment(view_proj: &engine_core::prelude::Mat4, rect: Rect, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
    let (ca, cb) = clip_near(*view_proj * a.extend(1.0), *view_proj * b.extend(1.0))?;
    let to_screen = |clip: Vec4| {
        let ndc = clip.truncate() / clip.w;
        egui::pos2(rect.center().x + ndc.x * rect.width() * 0.5, rect.center().y - ndc.y * rect.height() * 0.5)
    };
    let (pa, pb) = (to_screen(ca), to_screen(cb));
    (finite(pa) && finite(pb)).then_some([pa, pb])
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;
    use engine_core::prelude::Mat4;

    #[test]
    fn segments_behind_the_camera_are_cut_at_the_near_plane() {
        // Камера в начале координат смотрит в −Z; точка позади (z = +5) не проецируется.
        let proj = engine_render::perspective(60f32.to_radians(), 1.0, 0.1, 100.0);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 400.0));
        let [a, b] = project_segment(&proj, rect, Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 5.0)).unwrap();
        assert!(finite(a) && finite(b));
        // Отрезок по оси взгляда — точка в центре, обрезанный конец тоже.
        assert!((a - rect.center()).length() < 1e-3 && (b - rect.center()).length() < 1e-3, "{a:?} {b:?}");
        // Отрезок вбок через плоскость камеры: обрезанный конец уходит далеко вбок, но в ту
        // же сторону, где лежит видимая часть, а не «выстреливает» в противоположную.
        let [a, b] = project_segment(&proj, rect, Vec3::new(1.0, 0.0, -2.0), Vec3::new(1.0, 0.0, 2.0)).unwrap();
        assert!(a.x > rect.center().x && b.x > rect.center().x, "{a:?} {b:?}");
        assert!(project_segment(&proj, rect, Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 2.0)).is_none());
        assert!(project_segment(&proj, rect, Vec3::NAN, Vec3::ZERO).is_none());
    }

    /// Белые вершины, которые нарисовал кадр (обводка — белая, фон панели — серый).
    fn stroke_vertices(draw: impl FnOnce(&Painter)) -> Vec<Pos2> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut draw = Some(draw);
        let full = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(draw) = draw.take() {
                    draw(ui.painter());
                }
            });
        });
        let mut out = Vec::new();
        for primitive in ctx.tessellate(full.shapes, 1.0) {
            if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                out.extend(mesh.vertices.iter().filter(|v| v.color == egui::Color32::WHITE).map(|v| v.pos));
            }
        }
        out
    }

    #[test]
    fn acute_and_degenerate_outlines_do_not_shoot_out() {
        // Почти сплющенный треугольник (острый угол ~0.1°) и четырёхугольник с ребром нулевой
        // длины — как контур спрайта под родителем с неравным масштабом или рамка камеры,
        // повёрнутой в 3D, в виде 2D.
        let sliver = [pos2(100.0, 100.0), pos2(400.0, 100.5), pos2(100.0, 101.0)];
        let flat = [pos2(500.0, 300.0), pos2(600.0, 300.0), pos2(600.0, 300.0), pos2(500.0, 300.0)];
        let stroke = Stroke::new(2.0_f32, egui::Color32::WHITE);
        for points in [&sliver[..], &flat[..]] {
            let bbox = Rect::from_points(points).expand(stroke.width + 1.0);
            let vertices = stroke_vertices(|painter| polyline(painter, points, true, stroke));
            assert!(!vertices.is_empty());
            for v in &vertices {
                assert!(finite(*v) && bbox.contains(*v), "{v:?} sticks out of {bbox:?}");
            }
        }
        // Для сравнения: одна ломаная `closed_line` на том же острие даёт «митру»-шип.
        let spike = stroke_vertices(|painter| {
            painter.add(egui::Shape::closed_line(sliver.to_vec(), stroke));
        });
        let bbox = Rect::from_points(&sliver).expand(stroke.width + 1.0);
        assert!(spike.iter().any(|v| !bbox.contains(*v)), "closed_line spikes on acute corners — the bug this module avoids");
    }

    #[test]
    fn rect_clipping_keeps_the_inner_part_and_drops_nan() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0));
        let (a, b) = clip_rect(pos2(-10.0, 5.0), pos2(20.0, 5.0), rect).unwrap();
        assert_eq!((a, b), (pos2(0.0, 5.0), pos2(10.0, 5.0)));
        assert!(clip_rect(pos2(-10.0, -5.0), pos2(-1.0, -1.0), rect).is_none());
        assert!(clip_rect(pos2(f32::NAN, 0.0), pos2(1.0, 1.0), rect).is_none());
        let (a, b) = clip_rect(pos2(2.0, 2.0), pos2(3.0, 3.0), rect).unwrap();
        assert_eq!((a, b), (pos2(2.0, 2.0), pos2(3.0, 3.0)));
    }
}
