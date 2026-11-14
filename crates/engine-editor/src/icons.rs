//! Иконки, нарисованные фигурами: не зависят от шрифта, одинаково чёткие при любом
//! масштабе интерфейса. Все вписаны в квадрат `rect` и рисуются одним цветом.

use std::f32::consts::{PI, TAU};

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Step,
    Stop,
    Move,
    Rotate,
    Scale,
    Object,
    Sprite,
    Camera,
    Prefab,
    Folder,
    FolderOpen,
    File,
    Scene,
    Image,
    Plus,
    Close,
    Info,
    Warn,
    Error,
    Search,
    Gear,
    Eye,
    EyeOff,
    Grid,
    Magnet,
    Globe,
    Local,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    More,
    Refresh,
    List,
    Tiles,
    Code,
    Console,
    Chart,
    Hierarchy,
    Inspector,
    Script,
    Undo,
    Redo,
    Save,
    Copy,
    Trash,
    Check,
    Sun,
    Bulb,
    Cube,
    Sphere,
    Cylinder,
    Plane,
    Audio,
    Ui,
    Particles,
    Tilemap,
    Animation,
    Tween,
    Physics,
    Collider,
    Transform,
    Build,
    Collapse,
    ArrowDown,
    Text,
    External,
    Environment,
    Gizmo,
    Reset,
    /// Кнопка выбора у поля ссылки (кружок с точкой).
    Target,
    Lock,
    Unlock,
    Filter,
    ArrowUp,
    Star,
    Clock,
}

/// Нарисовать иконку, вписанную в `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5; // «радиус» иконки
    let at = |x: f32, y: f32| pos2(c.x + x * s, c.y + y * s);
    let w = (s * 0.17).clamp(1.15, 1.7);
    let line = Stroke::new(w, color);
    let thin = Stroke::new((w * 0.75).max(1.0), color);
    let fill = |points: Vec<Pos2>| Shape::convex_polygon(points, color, Stroke::NONE);
    let outline = |points: Vec<Pos2>| Shape::closed_line(points, line);
    let polyline = |points: Vec<Pos2>| Shape::line(points, line);
    let seg = |a: (f32, f32), b: (f32, f32)| painter.line_segment([at(a.0, a.1), at(b.0, b.1)], line);
    let arc = |radius: f32, from: f32, to: f32, steps: usize| -> Vec<Pos2> {
        (0..=steps)
            .map(|i| {
                let angle = from + (to - from) * i as f32 / steps as f32;
                at(angle.cos() * radius, angle.sin() * radius)
            })
            .collect()
    };
    let rect_at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    // Куб в изометрии: внешний шестиугольник и три ребра к центру.
    let cube = |filled: bool| {
        let hex: Vec<Pos2> = (0..6)
            .map(|i| {
                let angle = (i as f32 / 6.0 - 0.25) * TAU;
                at(angle.cos() * 0.88, angle.sin() * 0.88)
            })
            .collect();
        if filled {
            painter.add(Shape::convex_polygon(hex.clone(), color.gamma_multiply(0.35), Stroke::NONE));
        }
        painter.add(outline(hex.clone()));
        let center = at(0.0, 0.05);
        for corner in [hex[1], hex[3], hex[5]] {
            painter.line_segment([center, corner], thin);
        }
    };

    match icon {
        Icon::Play => {
            painter.add(fill(vec![at(-0.55, -0.78), at(0.8, 0.0), at(-0.55, 0.78)]));
        }
        Icon::Pause => {
            painter.rect_filled(rect_at(-0.62, -0.72, -0.16, 0.72), 1.0, color);
            painter.rect_filled(rect_at(0.16, -0.72, 0.62, 0.72), 1.0, color);
        }
        Icon::Step => {
            painter.add(fill(vec![at(-0.78, -0.72), at(0.32, 0.0), at(-0.78, 0.72)]));
            painter.rect_filled(rect_at(0.42, -0.72, 0.78, 0.72), 1.0, color);
        }
        Icon::Stop => {
            painter.rect_filled(rect_at(-0.6, -0.6, 0.6, 0.6), 1.0, color);
        }
        Icon::Move => {
            seg((-0.9, 0.0), (0.9, 0.0));
            seg((0.0, -0.9), (0.0, 0.9));
            for (tip, a, b) in [
                ((1.0, 0.0), (0.58, -0.32), (0.58, 0.32)),
                ((-1.0, 0.0), (-0.58, -0.32), (-0.58, 0.32)),
                ((0.0, -1.0), (-0.32, -0.58), (0.32, -0.58)),
                ((0.0, 1.0), (-0.32, 0.58), (0.32, 0.58)),
            ] {
                painter.add(fill(vec![at(tip.0, tip.1), at(a.0, a.1), at(b.0, b.1)]));
            }
        }
        Icon::Rotate | Icon::Refresh | Icon::Reset => {
            let (from, to) = if icon == Icon::Reset { (-0.1 * PI, 1.35 * PI) } else { (-0.5 * PI, 1.0 * PI) };
            painter.add(polyline(arc(0.72, from, to, 24)));
            let end = at((to).cos() * 0.72, (to).sin() * 0.72);
            let tangent = vec2(-(to).sin(), (to).cos());
            let normal = vec2(tangent.y, -tangent.x);
            painter.add(fill(vec![
                end + tangent * s * 0.45,
                end + normal * s * 0.36,
                end - normal * s * 0.36,
            ]));
        }
        Icon::Scale => {
            painter.add(outline(vec![at(-0.85, -0.05), at(0.05, -0.05), at(0.05, 0.85), at(-0.85, 0.85)]));
            seg((0.05, -0.05), (0.72, -0.72));
            painter.add(fill(vec![at(0.92, -0.92), at(0.3, -0.85), at(0.85, -0.3)]));
        }
        Icon::Object | Icon::Cube => cube(false),
        Icon::Prefab => cube(true),
        Icon::Sprite => {
            painter.add(outline(vec![at(-0.8, -0.8), at(0.8, -0.8), at(0.8, 0.8), at(-0.8, 0.8)]));
            painter.add(fill(vec![at(-0.55, 0.55), at(-0.1, -0.05), at(0.2, 0.3), at(0.2, 0.55)]));
            painter.circle_filled(at(0.4, -0.35), s * 0.17, color);
        }
        Icon::Image => {
            painter.add(outline(vec![at(-0.85, -0.7), at(0.85, -0.7), at(0.85, 0.7), at(-0.85, 0.7)]));
            painter.add(fill(vec![at(-0.6, 0.48), at(-0.1, -0.1), at(0.25, 0.25), at(0.25, 0.48)]));
            painter.circle_filled(at(0.42, -0.3), s * 0.16, color);
        }
        Icon::Camera => {
            painter.rect_stroke(rect_at(-0.9, -0.5, 0.3, 0.55), 2.0, line);
            painter.add(fill(vec![at(0.3, 0.02), at(0.92, -0.42), at(0.92, 0.48)]));
            painter.circle_stroke(at(-0.55, -0.75), s * 0.22, thin);
            painter.circle_stroke(at(-0.05, -0.75), s * 0.22, thin);
        }
        Icon::Folder | Icon::FolderOpen => {
            painter.add(fill(vec![at(-0.9, -0.68), at(-0.2, -0.68), at(0.02, -0.45), at(-0.9, -0.45)]));
            if icon == Icon::Folder {
                painter.add(outline(vec![at(-0.9, -0.45), at(0.9, -0.45), at(0.9, 0.7), at(-0.9, 0.7)]));
            } else {
                painter.add(outline(vec![at(-0.9, -0.45), at(0.75, -0.45), at(0.75, -0.15), at(-0.9, -0.15)]));
                painter.add(fill(vec![at(-0.72, -0.1), at(1.0, -0.1), at(0.8, 0.7), at(-0.9, 0.7)]));
            }
        }
        Icon::File | Icon::Script | Icon::Text => {
            painter.add(outline(vec![at(-0.62, -0.88), at(0.22, -0.88), at(0.65, -0.45), at(0.65, 0.88), at(-0.62, 0.88)]));
            painter.add(polyline(vec![at(0.2, -0.88), at(0.2, -0.45), at(0.65, -0.45)]));
            match icon {
                Icon::Script => {
                    painter.add(Shape::line(vec![at(-0.12, -0.05), at(-0.36, 0.25), at(-0.12, 0.55)], thin));
                    painter.add(Shape::line(vec![at(0.12, -0.05), at(0.36, 0.25), at(0.12, 0.55)], thin));
                }
                Icon::Text => {
                    for y in [-0.1, 0.2, 0.5] {
                        painter.line_segment([at(-0.35, y), at(0.38, y)], thin);
                    }
                }
                _ => {}
            }
        }
        Icon::Scene => {
            painter.add(outline(vec![at(-0.88, -0.65), at(0.88, -0.65), at(0.88, 0.65), at(-0.88, 0.65)]));
            painter.add(fill(vec![at(-0.62, 0.42), at(-0.18, -0.2), at(0.12, 0.12), at(0.35, -0.08), at(0.65, 0.42)]));
        }
        Icon::Plus => {
            seg((-0.72, 0.0), (0.72, 0.0));
            seg((0.0, -0.72), (0.0, 0.72));
        }
        Icon::Close => {
            seg((-0.58, -0.58), (0.58, 0.58));
            seg((-0.58, 0.58), (0.58, -0.58));
        }
        Icon::Check => {
            painter.add(polyline(vec![at(-0.7, 0.02), at(-0.22, 0.5), at(0.72, -0.5)]));
        }
        Icon::Info => {
            painter.circle_filled(c, s * 0.82, color);
            let ink = Color32::from_rgb(0x16, 0x17, 0x1a);
            painter.rect_filled(rect_at(-0.11, -0.12, 0.11, 0.5), 1.0, ink);
            painter.circle_filled(at(0.0, -0.4), s * 0.13, ink);
        }
        Icon::Warn => {
            painter.add(fill(vec![at(0.0, -0.88), at(0.92, 0.75), at(-0.92, 0.75)]));
            let ink = Color32::from_rgb(0x16, 0x17, 0x1a);
            painter.rect_filled(rect_at(-0.09, -0.35, 0.09, 0.25), 1.0, ink);
            painter.circle_filled(at(0.0, 0.47), s * 0.11, ink);
        }
        Icon::Error => {
            let points: Vec<Pos2> = (0..8)
                .map(|i| {
                    let angle = (i as f32 / 8.0 + 1.0 / 16.0) * TAU;
                    at(angle.cos() * 0.88, angle.sin() * 0.88)
                })
                .collect();
            painter.add(fill(points));
            let ink = Stroke::new(w, Color32::from_rgb(0x16, 0x17, 0x1a));
            painter.line_segment([at(-0.32, -0.32), at(0.32, 0.32)], ink);
            painter.line_segment([at(-0.32, 0.32), at(0.32, -0.32)], ink);
        }
        Icon::Search => {
            painter.circle_stroke(at(-0.15, -0.15), s * 0.55, line);
            seg((0.27, 0.27), (0.85, 0.85));
        }
        Icon::Gear => {
            let teeth: Vec<Pos2> = (0..32)
                .map(|i| {
                    let angle = i as f32 / 32.0 * TAU;
                    let radius = if (i / 2) % 2 == 0 { 0.92 } else { 0.68 };
                    at(angle.cos() * radius, angle.sin() * radius)
                })
                .collect();
            painter.add(Shape::closed_line(teeth, thin));
            painter.circle_stroke(c, s * 0.3, line);
        }
        Icon::Eye | Icon::EyeOff => {
            let mut lid = arc(1.0, 0.18 * PI, 0.82 * PI, 12)
                .into_iter()
                .map(|p| pos2(p.x, c.y + (p.y - c.y) * 0.9 - s * 0.45))
                .collect::<Vec<_>>();
            let lower: Vec<Pos2> = arc(1.0, 1.18 * PI, 1.82 * PI, 12)
                .into_iter()
                .map(|p| pos2(p.x, c.y + (p.y - c.y) * 0.9 + s * 0.45))
                .collect();
            lid.extend(lower);
            painter.add(Shape::closed_line(lid, thin));
            painter.circle_filled(c, s * 0.27, color);
            if icon == Icon::EyeOff {
                seg((-0.85, 0.85), (0.85, -0.85));
            }
        }
        Icon::Grid | Icon::Tiles | Icon::Tilemap => {
            if icon == Icon::Tiles {
                for (x, y) in [(-0.85, -0.85), (0.1, -0.85), (-0.85, 0.1), (0.1, 0.1)] {
                    painter.rect_stroke(rect_at(x, y, x + 0.75, y + 0.75), 1.0, thin);
                }
            } else {
                painter.add(outline(vec![at(-0.85, -0.85), at(0.85, -0.85), at(0.85, 0.85), at(-0.85, 0.85)]));
                for t in [-0.28, 0.28] {
                    painter.line_segment([at(t, -0.85), at(t, 0.85)], thin);
                    painter.line_segment([at(-0.85, t), at(0.85, t)], thin);
                }
                if icon == Icon::Tilemap {
                    painter.rect_filled(rect_at(-0.28, -0.28, 0.28, 0.28), 0.0, color);
                }
            }
        }
        Icon::Magnet => {
            painter.add(polyline(
                [vec![at(-0.62, -0.8), at(-0.62, 0.1)], arc(0.62, PI, 0.0, 14), vec![at(0.62, -0.8)]]
                    .concat()
                    .into_iter()
                    .map(|p| if p.y > c.y + 0.1 * s { pos2(p.x, p.y.max(c.y + 0.1 * s)) } else { p })
                    .collect(),
            ));
            painter.rect_filled(rect_at(-0.82, -0.9, -0.42, -0.55), 0.0, color);
            painter.rect_filled(rect_at(0.42, -0.9, 0.82, -0.55), 0.0, color);
        }
        Icon::Globe | Icon::Environment => {
            painter.circle_stroke(c, s * 0.85, line);
            painter.line_segment([at(-0.85, 0.0), at(0.85, 0.0)], thin);
            let meridian: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let angle = -0.5 * PI + PI * i as f32 / 16.0;
                    at(angle.cos() * 0.38, angle.sin() * 0.85)
                })
                .collect();
            painter.add(Shape::line(meridian.clone(), thin));
            painter.add(Shape::line(meridian.into_iter().map(|p| pos2(2.0 * c.x - p.x, p.y)).collect(), thin));
        }
        Icon::Local | Icon::Transform | Icon::Gizmo => {
            seg((-0.7, 0.7), (0.8, 0.7));
            seg((-0.7, 0.7), (-0.7, -0.8));
            seg((-0.7, 0.7), (0.2, -0.1));
            if icon == Icon::Gizmo {
                painter.circle_filled(at(-0.7, 0.7), s * 0.2, color);
            }
        }
        Icon::ChevronRight => {
            painter.add(polyline(vec![at(-0.25, -0.6), at(0.35, 0.0), at(-0.25, 0.6)]));
        }
        Icon::ChevronDown => {
            painter.add(polyline(vec![at(-0.6, -0.25), at(0.0, 0.35), at(0.6, -0.25)]));
        }
        Icon::ChevronUp => {
            painter.add(polyline(vec![at(-0.6, 0.25), at(0.0, -0.35), at(0.6, 0.25)]));
        }
        Icon::More => {
            for y in [-0.6, 0.0, 0.6] {
                painter.circle_filled(at(0.0, y), s * 0.15, color);
            }
        }
        Icon::List => {
            for y in [-0.6, 0.0, 0.6] {
                painter.circle_filled(at(-0.72, y), s * 0.12, color);
                painter.line_segment([at(-0.4, y), at(0.85, y)], line);
            }
        }
        Icon::Code => {
            painter.add(polyline(vec![at(-0.35, -0.6), at(-0.9, 0.0), at(-0.35, 0.6)]));
            painter.add(polyline(vec![at(0.35, -0.6), at(0.9, 0.0), at(0.35, 0.6)]));
            painter.line_segment([at(0.15, -0.8), at(-0.15, 0.8)], thin);
        }
        Icon::Console => {
            painter.rect_stroke(rect_at(-0.9, -0.75, 0.9, 0.75), 2.0, thin);
            painter.add(polyline(vec![at(-0.55, -0.3), at(-0.2, 0.0), at(-0.55, 0.3)]));
            painter.line_segment([at(0.0, 0.35), at(0.5, 0.35)], line);
        }
        Icon::Chart => {
            for (x, top) in [(-0.6, 0.1), (0.0, -0.6), (0.6, -0.2)] {
                painter.rect_filled(rect_at(x - 0.2, top, x + 0.2, 0.8), 1.0, color);
            }
        }
        Icon::Hierarchy => {
            painter.rect_stroke(rect_at(-0.9, -0.85, -0.3, -0.4), 1.0, thin);
            painter.rect_stroke(rect_at(-0.1, -0.2, 0.9, 0.2), 1.0, thin);
            painter.rect_stroke(rect_at(-0.1, 0.5, 0.9, 0.9), 1.0, thin);
            painter.add(Shape::line(vec![at(-0.6, -0.4), at(-0.6, 0.7), at(-0.1, 0.7)], thin));
            painter.line_segment([at(-0.6, 0.0), at(-0.1, 0.0)], thin);
        }
        Icon::Inspector => {
            for (y, knob) in [(-0.55, 0.3), (0.0, -0.35), (0.55, 0.1)] {
                painter.line_segment([at(-0.85, y), at(0.85, y)], thin);
                painter.circle_filled(at(knob, y), s * 0.2, color);
            }
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let points: Vec<Pos2> = arc(0.62, -0.5 * PI, 0.5 * PI, 14)
                .into_iter()
                .map(|p| pos2(c.x + (p.x - c.x + 0.05 * s) * flip, p.y + 0.15 * s))
                .collect();
            let start = points[0];
            let mut path = vec![pos2(c.x - 0.6 * s * flip, start.y)];
            path.extend(points);
            path.push(pos2(c.x - 0.6 * s * flip, c.y + 0.77 * s));
            painter.add(Shape::line(path, line));
            let tip = pos2(c.x - 0.85 * s * flip, start.y);
            painter.add(fill(vec![
                tip,
                pos2(c.x - 0.45 * s * flip, start.y - 0.35 * s),
                pos2(c.x - 0.45 * s * flip, start.y + 0.35 * s),
            ]));
        }
        Icon::Save => {
            painter.add(outline(vec![at(-0.8, -0.8), at(0.5, -0.8), at(0.8, -0.5), at(0.8, 0.8), at(-0.8, 0.8)]));
            painter.rect_stroke(rect_at(-0.4, -0.8, 0.3, -0.3), 0.0, thin);
            painter.rect_stroke(rect_at(-0.45, 0.15, 0.45, 0.8), 0.0, thin);
        }
        Icon::Copy => {
            painter.rect_stroke(rect_at(-0.85, -0.85, 0.35, 0.35), 2.0, thin);
            painter.rect_stroke(rect_at(-0.35, -0.35, 0.85, 0.85), 1.5, line);
        }
        Icon::Trash => {
            seg((-0.85, -0.55), (0.85, -0.55));
            painter.add(polyline(vec![at(-0.3, -0.55), at(-0.25, -0.85), at(0.25, -0.85), at(0.3, -0.55)]));
            painter.add(outline(vec![at(-0.62, -0.4), at(0.62, -0.4), at(0.5, 0.85), at(-0.5, 0.85)]));
            painter.line_segment([at(-0.15, -0.15), at(-0.12, 0.6)], thin);
            painter.line_segment([at(0.15, -0.15), at(0.12, 0.6)], thin);
        }
        Icon::Sun => {
            painter.circle_filled(c, s * 0.38, color);
            for i in 0..8 {
                let angle = i as f32 / 8.0 * TAU;
                let (x, y) = (angle.cos(), angle.sin());
                painter.line_segment([at(x * 0.6, y * 0.6), at(x * 0.92, y * 0.92)], thin);
            }
        }
        Icon::Bulb => {
            painter.add(polyline(
                [vec![at(-0.3, 0.45)], arc(0.6, 0.8 * PI, 2.2 * PI, 20), vec![at(0.3, 0.45)]].concat(),
            ));
            painter.line_segment([at(-0.3, 0.45), at(0.3, 0.45)], line);
            painter.line_segment([at(-0.25, 0.68), at(0.25, 0.68)], line);
            painter.line_segment([at(-0.15, 0.9), at(0.15, 0.9)], line);
        }
        Icon::Sphere => {
            painter.circle_stroke(c, s * 0.85, line);
            let equator: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let angle = PI * i as f32 / 20.0;
                    at(-angle.cos() * 0.85, angle.sin() * 0.3)
                })
                .collect();
            painter.add(Shape::line(equator, thin));
        }
        Icon::Cylinder => {
            let ellipse = |y: f32| -> Vec<Pos2> {
                (0..=24)
                    .map(|i| {
                        let angle = TAU * i as f32 / 24.0;
                        at(angle.cos() * 0.7, y + angle.sin() * 0.25)
                    })
                    .collect()
            };
            painter.add(Shape::closed_line(ellipse(-0.6), line));
            let bottom: Vec<Pos2> = ellipse(0.6).into_iter().filter(|p| p.y >= c.y + 0.6 * s - 0.01).collect();
            painter.add(Shape::line(bottom, line));
            seg((-0.7, -0.6), (-0.7, 0.6));
            seg((0.7, -0.6), (0.7, 0.6));
        }
        Icon::Plane => {
            painter.add(outline(vec![at(-0.45, -0.45), at(0.95, -0.45), at(0.45, 0.45), at(-0.95, 0.45)]));
        }
        Icon::Audio => {
            painter.add(fill(vec![at(-0.85, -0.3), at(-0.45, -0.3), at(-0.45, 0.3), at(-0.85, 0.3)]));
            painter.add(fill(vec![at(-0.45, -0.3), at(0.05, -0.75), at(0.05, 0.75), at(-0.45, 0.3)]));
            painter.add(Shape::line(arc(0.45, -0.3 * PI, 0.3 * PI, 10).into_iter().map(|p| p + vec2(0.1 * s, 0.0)).collect(), thin));
            painter.add(Shape::line(arc(0.8, -0.28 * PI, 0.28 * PI, 12).into_iter().map(|p| p + vec2(0.1 * s, 0.0)).collect(), thin));
        }
        Icon::Ui => {
            painter.rect_stroke(rect_at(-0.9, -0.7, 0.9, 0.7), 2.0, thin);
            painter.rect_filled(rect_at(-0.55, -0.3, 0.55, 0.1), 2.0, color);
            painter.line_segment([at(-0.55, 0.38), at(0.2, 0.38)], thin);
        }
        Icon::Particles => {
            let star = |x: f32, y: f32, r: f32| {
                painter.add(fill(vec![at(x, y - r), at(x + r * 0.28, y), at(x, y + r), at(x - r * 0.28, y)]));
                painter.add(fill(vec![at(x - r, y), at(x, y - r * 0.28), at(x + r, y), at(x, y + r * 0.28)]));
            };
            star(-0.25, 0.15, 0.7);
            star(0.55, -0.5, 0.38);
            star(0.6, 0.6, 0.25);
        }
        Icon::Animation => {
            painter.rect_stroke(rect_at(-0.9, -0.65, 0.9, 0.65), 2.0, thin);
            for x in [-0.6, -0.2, 0.2, 0.6] {
                painter.rect_filled(rect_at(x - 0.08, -0.62, x + 0.08, -0.42), 0.0, color);
                painter.rect_filled(rect_at(x - 0.08, 0.42, x + 0.08, 0.62), 0.0, color);
            }
            painter.add(fill(vec![at(-0.2, -0.25), at(0.3, 0.0), at(-0.2, 0.25)]));
        }
        Icon::Tween => {
            let curve: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    let eased = t * t * (3.0 - 2.0 * t);
                    at(-0.85 + 1.7 * t, 0.75 - 1.5 * eased)
                })
                .collect();
            painter.add(Shape::line(curve, line));
            painter.circle_filled(at(-0.85, 0.75), s * 0.15, color);
            painter.circle_filled(at(0.85, -0.75), s * 0.15, color);
        }
        Icon::Physics => {
            painter.circle_stroke(at(0.2, 0.15), s * 0.55, line);
            for y in [-0.25, 0.15, 0.55] {
                painter.line_segment([at(-0.95, y), at(-0.5, y)], thin);
            }
        }
        Icon::Collider => {
            let dashes = [(-0.85, -0.85, 0.85, -0.85), (0.85, -0.85, 0.85, 0.85), (0.85, 0.85, -0.85, 0.85), (-0.85, 0.85, -0.85, -0.85)];
            for (x0, y0, x1, y1) in dashes {
                for k in 0..3 {
                    let t0 = k as f32 / 3.0 + 0.04;
                    let t1 = t0 + 0.22;
                    painter.line_segment(
                        [at(x0 + (x1 - x0) * t0, y0 + (y1 - y0) * t0), at(x0 + (x1 - x0) * t1, y0 + (y1 - y0) * t1)],
                        thin,
                    );
                }
            }
            painter.circle_filled(c, s * 0.22, color);
        }
        Icon::Build => {
            // Молоток: рукоять идёт по диагонали, боёк стоит поперёк неё (а не горизонтально —
            // иначе головка выглядит свёрнутой набок). Ось рукояти — (0.6, −0.8), ось бойка —
            // перпендикуляр (0.8, 0.6); углы бойка: центр ± 0.52 вдоль бойка ± 0.24 вдоль рукояти.
            painter.add(fill(vec![at(-0.27, -0.52), at(0.018, -0.904), at(0.85, -0.28), at(0.562, 0.104)]));
            painter.line_segment([at(0.22, -0.30), at(-0.62, 0.82)], Stroke::new(w * 1.6, color));
        }
        Icon::Collapse => {
            painter.rect_stroke(rect_at(-0.85, -0.85, 0.45, 0.2), 2.0, thin);
            painter.rect_stroke(rect_at(-0.45, -0.25, 0.85, 0.85), 1.5, line);
        }
        Icon::ArrowDown => {
            seg((0.0, -0.85), (0.0, 0.45));
            painter.add(polyline(vec![at(-0.5, 0.0), at(0.0, 0.5), at(0.5, 0.0)]));
            seg((-0.7, 0.85), (0.7, 0.85));
        }
        Icon::Target => {
            painter.circle_stroke(c, s * 0.72, line);
            painter.circle_filled(c, s * 0.22, color);
        }
        Icon::Lock | Icon::Unlock => {
            painter.add(outline(vec![at(-0.7, -0.1), at(0.7, -0.1), at(0.7, 0.85), at(-0.7, 0.85)]));
            let (from, to) = if icon == Icon::Lock { (PI, 2.0 * PI) } else { (1.1 * PI, 1.9 * PI) };
            let shackle: Vec<Pos2> = arc(0.42, from, to, 12).into_iter().map(|p| p + vec2(0.0, -0.3 * s)).collect();
            let mut path = vec![at(-0.42, -0.1)];
            path.extend(shackle);
            if icon == Icon::Lock {
                path.push(at(0.42, -0.1));
            }
            painter.add(polyline(path));
            painter.line_segment([at(0.0, 0.25), at(0.0, 0.5)], line);
        }
        Icon::Filter => {
            painter.add(outline(vec![at(-0.85, -0.75), at(0.85, -0.75), at(0.15, 0.05), at(0.15, 0.8), at(-0.15, 0.6), at(-0.15, 0.05)]));
        }
        Icon::ArrowUp => {
            seg((0.0, 0.85), (0.0, -0.75));
            painter.add(polyline(vec![at(-0.55, -0.2), at(0.0, -0.8), at(0.55, -0.2)]));
        }
        Icon::Star => {
            let points: Vec<Pos2> = (0..10)
                .map(|i| {
                    let angle = (i as f32 / 10.0 - 0.25) * TAU;
                    let radius = if i % 2 == 0 { 0.92 } else { 0.4 };
                    at(angle.cos() * radius, angle.sin() * radius + 0.06)
                })
                .collect();
            painter.add(Shape::closed_line(points, line));
        }
        Icon::Clock => {
            painter.circle_stroke(c, s * 0.82, line);
            painter.add(polyline(vec![at(0.0, -0.5), at(0.0, 0.0), at(0.38, 0.22)]));
        }
        Icon::External => {
            painter.add(polyline(vec![at(0.1, -0.75), at(-0.75, -0.75), at(-0.75, 0.75), at(0.75, 0.75), at(0.75, -0.1)]));
            seg((-0.05, 0.05), (0.85, -0.85));
            painter.add(polyline(vec![at(0.3, -0.85), at(0.85, -0.85), at(0.85, -0.3)]));
        }
    }
}

/// Иконка в строке текста: занимает место и ничего не ловит.
pub fn inline(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    paint(ui.painter(), rect.shrink(1.0), icon, color);
}

/// Маленькая иконка 12 px в строке текста (строка состояния, поля).
pub fn inline_small(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(theme::ICON_S, theme::ICON_S), Sense::hover());
    paint(ui.painter(), rect, icon, color);
}

/// Кнопка панели инструментов: квадрат с иконкой; `selected` — включена.
pub fn button(ui: &mut Ui, icon: Icon, selected: bool, tint: Option<Color32>, tooltip: &str) -> Response {
    // Квадрат со стороной в высоту контрола: в одной строке с полями и кнопками.
    let size = vec2(theme::CONTROL_H, theme::CONTROL_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let hovered = response.hovered() && enabled;
    let background = if selected {
        Some(theme::accent_soft())
    } else if hovered {
        Some(theme::BG_HOVER)
    } else {
        None
    };
    if let Some(background) = background {
        ui.painter().rect_filled(rect, theme::R_S, background);
    }
    let color = if !enabled {
        theme::TEXT_DISABLED
    } else if selected {
        theme::TEXT_STRONG
    } else {
        tint.unwrap_or(if hovered { theme::TEXT_STRONG } else { theme::TEXT })
    };
    paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(15.0, 15.0)), icon, color);
    response.on_hover_text(tooltip)
}

/// Маленькая кнопка-иконка без фона (закрыть, добавить, меню «⋮»).
pub fn small_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
    }
    let color = if response.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
    paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(theme::ICON_S, theme::ICON_S)), icon, color);
    response.on_hover_text(tooltip)
}
