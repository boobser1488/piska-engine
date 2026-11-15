//! 3D-вид сцены: орбитальная камера, выбор объекта лучом, гизмо по трём осям.

use crate::i18n::{tr_s, trf};
use std::collections::HashMap;

use egui::{pos2, vec2, Color32, PointerButton, Pos2, Rect, Shape, Stroke};
use engine_assets::{AssetDatabase, Guid};
use engine_core::prelude::*;
use engine_render::{CameraView, Light, LightKind, MeshRenderer, SpriteRenderer};

use crate::hierarchy::object_icon;
use crate::selection::GizmoOp;
use crate::icons;
use crate::state::{Action, Editor, Tool};
use crate::theme;

const FOV_DEG: f32 = 50.0;
const HANDLE_REACH: f32 = 8.0;

/// Камера 3D-вида: смотрит на `target` с расстояния `distance`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit3D {
    pub target: Vec3,
    /// Поворот вокруг вертикали, радианы.
    pub yaw: f32,
    /// Наклон: 0 — вид сбоку, π/2 — сверху.
    pub pitch: f32,
    pub distance: f32,
}

impl Default for Orbit3D {
    fn default() -> Self {
        Self {
            target: Vec3::new(0.0, 0.8, 0.0),
            yaw: 0.6,
            pitch: 0.42,
            distance: 12.0,
        }
    }
}

impl Orbit3D {
    pub fn eye(&self) -> Vec3 {
        let direction = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        self.target + direction * self.distance
    }

    pub fn view(&self, aspect: f32, clear: Color) -> CameraView {
        CameraView::look_at(self.eye(), self.target, FOV_DEG, aspect, clear, true)
    }

    /// Встать так же, как стоит камера с мировой матрицей `matrix`.
    pub fn from_camera(matrix: Mat4) -> Self {
        let (_, rotation, position) = matrix.to_scale_rotation_translation();
        let forward = rotation * Vec3::NEG_Z;
        // Точка, на которую смотрит камера: пересечение с землёй, если оно недалеко.
        let distance = if forward.y < -0.05 {
            (position.y / -forward.y).clamp(2.0, 40.0)
        } else {
            10.0
        };
        let back = -forward;
        Self {
            target: position + forward * distance,
            yaw: back.x.atan2(back.z),
            pitch: back.y.clamp(-1.0, 1.0).asin(),
            distance,
        }
    }
}

/// Проекция «мир ↔ окно» для текущего кадра.
#[derive(Clone, Copy)]
pub(crate) struct Projector {
    view_proj: Mat4,
    inverse: Mat4,
    rect: Rect,
    pub eye: Vec3,
}

impl Projector {
    pub fn new(orbit: &Orbit3D, rect: Rect) -> Self {
        let aspect = rect.width() / rect.height().max(1.0);
        let view = orbit.view(aspect, Color::BLACK);
        Self {
            view_proj: view.view_proj,
            inverse: view.view_proj.inverse(),
            rect,
            eye: view.position,
        }
    }

    fn clip_to_screen(&self, clip: Vec4) -> Pos2 {
        let ndc = clip.truncate() / clip.w;
        pos2(
            self.rect.center().x + ndc.x * self.rect.width() * 0.5,
            self.rect.center().y - ndc.y * self.rect.height() * 0.5,
        )
    }

    /// Точка мира на экране; `None`, если она позади камеры (или у самой камеры) или
    /// координаты не числа.
    pub fn project(&self, world: Vec3) -> Option<Pos2> {
        let clip = self.view_proj * world.extend(1.0);
        (clip.is_finite() && clip.w >= crate::lines::NEAR)
            .then(|| self.clip_to_screen(clip))
            .filter(|p| crate::lines::finite(*p))
    }

    /// Отрезок мира на экране, обрезанный плоскостью у камеры (lines.rs).
    pub fn segment(&self, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
        crate::lines::project_segment(&self.view_proj, self.rect, a, b)
    }

    /// Ломаная мира (замкнутая, если `closed`) — по рёбрам, каждое обрезано у камеры.
    pub fn polyline(&self, painter: &egui::Painter, points: &[Vec3], closed: bool, stroke: Stroke) {
        let n = points.len();
        let edges = if closed { n } else { n.saturating_sub(1) };
        for i in 0..edges {
            if let Some([a, b]) = self.segment(points[i], points[(i + 1) % n]) {
                crate::lines::segment(painter, a, b, stroke);
            }
        }
    }

    /// Луч взгляда через точку окна: начало и единичное направление.
    pub fn ray(&self, screen: Pos2) -> (Vec3, Vec3) {
        let x = (screen.x - self.rect.center().x) / (self.rect.width() * 0.5);
        let y = -(screen.y - self.rect.center().y) / (self.rect.height() * 0.5);
        let near = self.inverse * Vec4::new(x, y, 0.0, 1.0);
        let far = self.inverse * Vec4::new(x, y, 1.0, 1.0);
        let (a, b) = (near.truncate() / near.w, far.truncate() / far.w);
        // Глубина в рендере обратная (z = 1 у камеры): начало луча — тот конец, что ближе
        // к глазу, так луч верен при любом соглашении о глубине.
        let (near, far) = if a.distance_squared(self.eye) <= b.distance_squared(self.eye) { (a, b) } else { (b, a) };
        (near, (far - near).normalize_or_zero())
    }
}

/// Локальные границы меша объекта (границы файлов кэшируются по GUID).
pub(crate) fn mesh_bounds(
    mesh: &MeshRenderer,
    assets: Option<&AssetDatabase>,
    cache: &mut HashMap<Guid, (Vec3, Vec3)>,
) -> (Vec3, Vec3) {
    if mesh.mesh.is_none() {
        return mesh.shape.bounds();
    }
    let guid = mesh.mesh.guid();
    *cache.entry(guid).or_insert_with(|| {
        assets
            .and_then(|assets| assets.path_of(guid))
            .and_then(|path| engine_assets::load_mesh(path).ok())
            .map_or((Vec3::splat(-0.5), Vec3::splat(0.5)), |data| data.bounds())
    })
}

/// Где луч входит в параллелепипед `min..max` (в тех же координатах, что луч).
fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut near, mut far) = (0.0_f32, f32::INFINITY);
    for axis in 0..3 {
        let (o, d) = (origin[axis], direction[axis]);
        // Плоскую фигуру (плоскость) делаем чуть толще, чтобы в неё можно было попасть.
        let (lo, hi) = (min[axis].min(max[axis] - 0.02), max[axis].max(min[axis] + 0.02));
        if d.abs() < 1e-8 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (t1, t2) = ((lo - o) / d, (hi - o) / d);
        near = near.max(t1.min(t2));
        far = far.min(t1.max(t2));
        if near > far {
            return None;
        }
    }
    Some(near)
}

impl Editor {
    /// Объект под лучом: ближайший меш или спрайт, а для «пустых» — точка у их позиции.
    pub(crate) fn pick_3d(&mut self, origin: Vec3, direction: Vec3) -> Option<GameObjectId> {
        let world = &self.app.world;
        let assets = world.resources.get::<AssetDatabase>();
        let scene = &world.scene;
        let mut best: Option<(f32, GameObjectId)> = None;
        let locked = |id: GameObjectId| !self.hierarchy.locked.is_empty() && {
            crate::state::path_of(scene, id).is_some_and(|path| (1..=path.len()).any(|n| self.hierarchy.locked.contains(&path[..n])))
        };
        for id in scene.ids() {
            if !scene.is_active(id) || locked(id) {
                continue;
            }
            let Some(object) = scene.get(id) else { continue };
            let matrix = scene.world_matrix(id);
            let hit = if let Some(mesh) = object.get::<MeshRenderer>() {
                if matrix.determinant().abs() < 1e-12 {
                    continue;
                }
                // Луч в локальных координатах: параметр `t` при этом не меняется.
                let inverse = matrix.inverse();
                let (min, max) = mesh_bounds(mesh, assets, &mut self.viewport.mesh_bounds);
                ray_box(inverse.transform_point3(origin), inverse.transform_vector3(direction), min, max)
            } else if let Some(terrain) = object.get::<engine_render::Terrain>() {
                let corner = matrix.w_axis.truncate();
                terrain.sampler(assets).raycast_local(origin - corner, direction, 20_000.0)
            } else if let Some(sprite) = object.get::<SpriteRenderer>() {
                let half = sprite.size.extend(0.0) * 0.5;
                if matrix.determinant().abs() < 1e-12 {
                    continue;
                }
                let inverse = matrix.inverse();
                ray_box(inverse.transform_point3(origin), inverse.transform_vector3(direction), -half, half)
            } else {
                // Маркер «пустого» объекта: шар, который на экране выглядит одинаково.
                let center = matrix.w_axis.truncate();
                let radius = center.distance(origin) * 0.02;
                let along = (center - origin).dot(direction);
                (along > 0.0 && (origin + direction * along).distance(center) <= radius).then_some(along - 1e3)
            };
            if let Some(t) = hit {
                if best.is_none_or(|(nearest, _)| t < nearest) {
                    best = Some((t, id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    /// Центр и радиус объекта в мире — для наведения камеры.
    pub(crate) fn bounds_3d(&mut self, id: GameObjectId) -> Option<(Vec3, f32)> {
        let world = &self.app.world;
        let scene = &world.scene;
        let object = scene.get(id)?;
        let matrix = scene.world_matrix(id);
        let local = match (object.get::<MeshRenderer>(), object.get::<SpriteRenderer>()) {
            (Some(mesh), _) => mesh_bounds(
                mesh,
                world.resources.get::<AssetDatabase>(),
                &mut self.viewport.mesh_bounds,
            ),
            (None, Some(sprite)) => (-sprite.size.extend(0.0) * 0.5, sprite.size.extend(0.0) * 0.5),
            _ => (Vec3::splat(-0.5), Vec3::splat(0.5)),
        };
        let (min, max) = engine_render::transform_bounds(matrix, local);
        Some(((min + max) * 0.5, ((max - min).length() * 0.5).max(0.25)))
    }
}

/// Перетаскивание ручки гизмо в 3D.
pub(crate) struct Drag3D {
    /// Активный объект в момент нажатия.
    object: GameObjectId,
    /// Все двигаемые объекты выделения.
    moved: Vec<crate::selection::Moved>,
    axis: usize,
    /// Мировая ось ручки.
    direction: Vec3,
    /// Параметр вдоль оси под курсором в момент нажатия.
    grab: f32,
    pointer_start: Pos2,
    /// Точка гизмо в начале.
    origin: Vec3,
    /// Поворот: касательная к кольцу в точке захвата на экране — пикселей на радиан.
    /// Сдвиг мыши вдоль неё поворачивает так, что точка кольца идёт за курсором.
    tangent: egui::Vec2,
    /// Поворот: центр гизмо на экране и знак оси к камере — запасной способ, когда
    /// касательная смотрит в камеру (кольцо видно ребром в точке захвата).
    screen_center: Pos2,
    facing: f32,
    /// Поворот: точка кольца, за которую взялись (мир).
    grab_point: Option<Vec3>,
}

/// Угол поворота вокруг `axis` через `origin`, при котором точка `grab` встаёт под луч
/// курсора: пересечение луча с плоскостью кольца. `None` — кольцо видно почти ребром
/// (тогда точнее касательная, [`ring_angle`]).
fn plane_angle(origin: Vec3, axis: Vec3, grab: Vec3, ray_origin: Vec3, ray_direction: Vec3) -> Option<f32> {
    let facing = ray_direction.dot(axis);
    if facing.abs() < 0.25 {
        return None;
    }
    let t = (origin - ray_origin).dot(axis) / facing;
    if t <= 0.0 {
        return None;
    }
    let hit = ray_origin + ray_direction * t;
    let a = (grab - origin) - axis * axis.dot(grab - origin);
    let b = (hit - origin) - axis * axis.dot(hit - origin);
    if a.length_squared() < 1e-10 || b.length_squared() < 1e-10 {
        return None;
    }
    Some(axis.dot(a.cross(b)).atan2(a.dot(b)))
}

/// Отрезков в кольце поворота.
const RING_SEGMENTS: usize = 72;

/// Точки кольца поворота вокруг `axis` (единичная) радиуса `radius`, по кругу.
fn ring_points(origin: Vec3, axis: Vec3, radius: f32) -> Vec<Vec3> {
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    (0..RING_SEGMENTS)
        .map(|i| {
            let t = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            origin + (u * t.cos() + v * t.sin()) * radius
        })
        .collect()
}

/// Кольцо под курсором: (ось, точка кольца в мире). Передняя половина кольца (к камере)
/// важнее задней — где кольца пересекаются, берётся видимое.
fn hit_ring(projector: &Projector, origin: Vec3, axes: &[Vec3; 3], radius: f32, pointer: Pos2) -> Option<(usize, Vec3)> {
    let toward_eye = (projector.eye - origin).normalize_or_zero();
    let mut best: Option<(f32, usize, Vec3)> = None;
    for (axis, direction) in axes.iter().enumerate() {
        let points = ring_points(origin, *direction, radius);
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let Some([sa, sb]) = projector.segment(a, b) else {
                continue;
            };
            let behind = ((a + b) * 0.5 - origin).dot(toward_eye) < 0.0;
            let distance = distance_to_segment(pointer, sa, sb) + if behind { 3.0 } else { 0.0 };
            if distance <= HANDLE_REACH && best.map_or(true, |(d, _, _)| distance < d) {
                // Точка отрезка, ближайшая к курсору, — по доле на экране.
                let along = sb - sa;
                let t = if along.length_sq() > 1e-6 { ((pointer - sa).dot(along) / along.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                best = Some((distance, axis, a.lerp(b, t)));
            }
        }
    }
    best.map(|(_, axis, point)| (axis, point))
}

/// Угол поворота (радианы) по сдвигу мыши: вдоль касательной к кольцу в точке захвата,
/// так что эта точка следует за курсором; если касательная на экране вырождена — по углу
/// вокруг центра гизмо с учётом того, смотрит ли ось на камеру.
fn ring_angle(tangent: egui::Vec2, screen_center: Pos2, facing: f32, start: Pos2, now: Pos2) -> f32 {
    if tangent.length_sq() >= 4.0 {
        return (now - start).dot(tangent) / tangent.length_sq();
    }
    let (from, to) = (start - screen_center, now - screen_center);
    if from.length_sq() < 1e-6 || to.length_sq() < 1e-6 {
        return 0.0;
    }
    // Экран: y вниз, угол atan2 растёт по часовой; ось к камере — поворот «+» против часовой.
    let screen = (from.x * to.y - from.y * to.x).atan2(from.dot(to));
    if facing > 0.0 { -screen } else { screen }
}

/// Параметр точки на прямой `point + s · axis`, ближайшей к лучу.
fn closest_on_axis(point: Vec3, axis: Vec3, ray_origin: Vec3, ray_direction: Vec3) -> Option<f32> {
    let w = point - ray_origin;
    let b = axis.dot(ray_direction);
    let denominator = 1.0 - b * b;
    (denominator.abs() > 1e-5).then(|| (b * ray_direction.dot(w) - axis.dot(w)) / denominator)
}

fn distance_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((point - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (point - (a + ab * t)).length()
}

pub(crate) fn show(editor: &mut Editor, ui: &egui::Ui, painter: &egui::Painter, response: &egui::Response) {
    let rect = editor.viewport.rect;
    let pointer = ui.ctx().pointer_latest_pos();
    // Кисть рельефа забирает левую кнопку себе (terrain_tools.rs).
    let terrain_brush = crate::terrain_tools::brush_active(editor);

    // --- Навигация, как в Unity:
    // ПКМ — обзор мышью и полёт W/A/S/D (Q / E — вниз / вверх, Shift — быстрее, колесо —
    // скорость полёта); Alt+ЛКМ — вращение вокруг точки взгляда; Alt+ПКМ — ближе / дальше;
    // СКМ или Space+ЛКМ — сдвиг; колесо — ближе / дальше; ЛКМ по пустому месту — рамка.
    let (alt, space) = ui.input(|i| (i.modifiers.alt, i.key_down(egui::Key::Space)));
    let rmb = ui.input(|i| i.pointer.secondary_down());
    let flying = rmb && !alt && (response.hovered() || editor.viewport.flying);
    editor.viewport.flying = flying;
    // Сырое движение мыши с прошлого кадра (копится, только пока идёт обзор).
    let raw_motion = std::mem::take(&mut editor.viewport.raw_motion);
    if flying && !editor.viewport.looking {
        editor.viewport.raw_seen = false;
    }
    editor.viewport.looking = flying;
    if flying {
        // Пока кнопка зажата, кадры идут в такт монитору, а не по событиям мыши: мышь
        // шлёт их со своей частотой, и поворот, нарисованный «когда пришло», идёт неровно.
        ui.ctx().request_repaint();
    }
    if response.hovered() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        if flying {
            if scroll.abs() > 0.0 {
                editor.prefs.fly_speed = (editor.prefs.fly_speed * (scroll * 0.003).exp()).clamp(0.1, 200.0);
                editor.viewport.fly_speed_shown_until = ui.input(|i| i.time) + 1.2;
            }
        } else {
            let factor = (-scroll * 0.0025 * editor.prefs.zoom_speed).exp() / pinch;
            editor.orbit.distance = (editor.orbit.distance * factor).clamp(0.3, 2000.0);
        }
    }
    let delta = response.drag_delta();
    // Значок ориентации: потянуть за него — вращать вид, щёлкнуть по оси — встать вдоль неё.
    if response.drag_started_by(PointerButton::Primary) {
        let origin = ui.input(|i| i.pointer.press_origin());
        editor.viewport.compass_drag = origin.is_some_and(|p| over_compass(rect, p));
    } else if !response.dragged_by(PointerButton::Primary) {
        editor.viewport.compass_drag = false;
    }
    // Скорость и направление — Preferences → Scene View.
    let speed = 0.008 * editor.prefs.orbit_speed;
    let vertical = if editor.prefs.invert_orbit_y { -speed } else { speed };
    let orbiting = editor.viewport.compass_drag
        || (response.dragged_by(PointerButton::Primary) && alt && editor.viewport.drag_3d.is_none());
    if orbiting {
        editor.orbit.yaw -= delta.x * speed;
        editor.orbit.pitch = (editor.orbit.pitch + delta.y * vertical).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        editor.viewport.orbit_goal = None;
    } else if response.dragged_by(PointerButton::Secondary) && !alt {
        // Обзор мышью: глаз на месте, поворачивается направление взгляда.
        //
        // Поворот берётся из сырого движения мыши (счёты датчика), а не из сдвига курсора:
        // курсор ходит целыми пикселями, с ускорением системы и упирается в край экрана.
        // Чувствительность — как у вида Scene в Unity (0.003 рад на счёт): при прежних
        // 0.008 рад на пиксель курсора один шаг сдвигал картинку пикселей на восемь, и
        // любая дрожь руки выглядела тряской камеры.
        if raw_motion != Vec2::ZERO {
            editor.viewport.raw_seen = true;
        }
        let used = if editor.viewport.raw_seen { egui::vec2(raw_motion.x, raw_motion.y) } else { delta };
        let look = LOOK_SPEED * editor.prefs.orbit_speed;
        let look_vertical = if editor.prefs.invert_orbit_y { -look } else { look };
        let eye = editor.orbit.eye();
        editor.orbit.yaw -= used.x * look;
        editor.orbit.pitch = (editor.orbit.pitch + used.y * look_vertical).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        if crate::look_trace::enabled() {
            let (time, raw_dt, moves) = ui.input(|i| {
                (i.time, i.unstable_dt, i.events.iter().filter(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_))).count())
            });
            let raw = egui::vec2(raw_motion.x, raw_motion.y);
            crate::look_trace::record(time, raw_dt, camera_dt(editor, ui), moves, delta, raw, used, editor.orbit.yaw, editor.orbit.pitch);
        }
        let moved = editor.orbit.eye();
        editor.orbit.target += eye - moved;
        editor.viewport.orbit_goal = None;
    } else if response.dragged_by(PointerButton::Secondary) && alt {
        editor.orbit.distance = (editor.orbit.distance * (delta.y * 0.01).exp()).clamp(0.3, 2000.0);
    }
    fly_3d(editor, ui, flying);
    // Плавный доворот к виду, выбранному щелчком по значку.
    if let Some((yaw, pitch)) = editor.viewport.orbit_goal {
        let dt = camera_dt(editor, ui);
        let k = 1.0 - (-dt * 16.0).exp();
        let turn = wrap_angle(yaw - editor.orbit.yaw);
        let tilt = pitch - editor.orbit.pitch;
        if turn.abs() < 0.003 && tilt.abs() < 0.003 {
            editor.orbit.yaw = wrap_angle(yaw);
            editor.orbit.pitch = pitch;
            editor.viewport.orbit_goal = None;
        } else {
            editor.orbit.yaw += turn * k;
            editor.orbit.pitch += tilt * k;
            // В режиме Edit кадры рисуются по требованию: просим следующий.
            ui.ctx().request_repaint();
        }
    }
    let panning = response.dragged_by(PointerButton::Middle)
        || (response.dragged_by(PointerButton::Primary)
            && space
            && !editor.viewport.compass_drag
            && !terrain_brush
            && editor.viewport.drag_3d.is_none());
    if panning {
        // Сдвиг в плоскости экрана: точка под курсором остаётся под курсором.
        let orbit = editor.orbit;
        let forward = (orbit.target - orbit.eye()).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(forward);
        let per_point = 2.0 * orbit.distance * (FOV_DEG.to_radians() * 0.5).tan() / rect.height().max(1.0);
        editor.orbit.target += (-right * delta.x + up * delta.y) * per_point;
    }

    let projector = Projector::new(&editor.orbit, rect);
    let line = |painter: &egui::Painter, a: Vec3, b: Vec3, stroke: Stroke| {
        if let Some([a, b]) = projector.segment(a, b) {
            crate::lines::segment(painter, a, b, stroke);
        }
    };

    // Сетка на земле рисуется самим рендером внутри 3D-прохода (`CameraView::grid`),
    // чтобы модели её закрывали; здесь её нет.

    // --- Маркеры объектов без меша, направление солнца ---
    let selection = editor
        .selection
        .filter(|id| editor.app.world.scene.contains(*id));
    {
        let scene = &editor.app.world.scene;
        for id in scene.ids() {
            let Some(object) = scene.get(id) else { continue };
            if !editor.show_gizmos
                || !scene.is_active(id)
                || object.has::<MeshRenderer>()
                || object.has::<SpriteRenderer>()
            {
                continue;
            }
            let matrix = scene.world_matrix(id);
            let position = matrix.w_axis.truncate();
            let Some(screen) = projector.project(position).filter(|p| rect.expand(12.0).contains(*p)) else {
                continue;
            };
            let selected = editor.is_selected(id);
            let color = if selected { theme::accent() } else { Color32::from_white_alpha(170) };
            let (icon, _) = object_icon(object);
            icons::paint(painter, Rect::from_center_size(screen, vec2(14.0, 14.0)), icon, color);
            if let Some(light) = object.get::<Light>() {
                if light.kind == LightKind::Directional {
                    let (_, rotation, _) = matrix.to_scale_rotation_translation();
                    let length = position.distance(projector.eye) * 0.12;
                    line(
                        painter,
                        position,
                        position + rotation * Vec3::NEG_Z * length,
                        Stroke::new(1.5_f32, theme::WARN),
                    );
                }
            }
        }
    }

    // --- Выделение: каркас границ каждого выделенного объекта ---
    for id in editor.selected() {
        let outline = if selection == Some(id) { theme::accent() } else { theme::mix(theme::accent(), Color32::WHITE, 0.35) };
        let world = &editor.app.world;
        let scene = &world.scene;
        let matrix = scene.world_matrix(id);
        let local = scene.get(id).and_then(|object| {
            if let Some(mesh) = object.get::<MeshRenderer>() {
                Some(mesh_bounds(
                    mesh,
                    world.resources.get::<AssetDatabase>(),
                    &mut editor.viewport.mesh_bounds,
                ))
            } else {
                object
                    .get::<SpriteRenderer>()
                    .map(|sprite| (-sprite.size.extend(0.0) * 0.5, sprite.size.extend(0.0) * 0.5))
            }
        });
        if let Some((min, max)) = local {
            let corner = |index: usize| {
                matrix.transform_point3(Vec3::new(
                    if index & 1 == 0 { min.x } else { max.x },
                    if index & 2 == 0 { min.y } else { max.y },
                    if index & 4 == 0 { min.z } else { max.z },
                ))
            };
            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                line(painter, corner(a), corner(b), Stroke::new(1.5_f32, outline));
            }
        }
    }

    // --- Коллайдер выделенного объекта: зелёный каркас. Физику редактор не подключает,
    // форма читается через reflection по именам полей `Collider` / `CharacterController`. ---
    if let Some(id) = selection.filter(|_| editor.show_gizmos) {
        let scene = &editor.app.world.scene;
        let matrix = scene.world_matrix(id);
        let stroke = Stroke::new(1.0_f32, theme::OK);
        if let Some(object) = scene.get(id) {
            let field = |component: &dyn Component, name: &str| component.get_field(name);
            let boxed = |painter: &egui::Painter, center: Vec3, half: Vec3| {
                let corner = |index: usize| {
                    matrix.transform_point3(
                        center
                            + Vec3::new(
                                if index & 1 == 0 { -half.x } else { half.x },
                                if index & 2 == 0 { -half.y } else { half.y },
                                if index & 4 == 0 { -half.z } else { half.z },
                            ),
                    )
                };
                for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    line(painter, corner(a), corner(b), stroke);
                }
            };
            for component in object.components() {
                match component.type_name() {
                    "Collider" => {
                        let offset = match field(component, "offset") {
                            Some(Value::Vec3(x, y, z)) => Vec3::new(x, y, z),
                            _ => Vec3::ZERO,
                        };
                        let half = match (field(component, "shape"), field(component, "size"), field(component, "radius")) {
                            (Some(Value::Enum(shape)), _, Some(Value::Float(radius))) if shape == "Sphere" => {
                                Vec3::splat(radius)
                            }
                            (_, Some(Value::Vec3(x, y, z)), _) => Vec3::new(x, y, z) * 0.5,
                            _ => Vec3::splat(0.5),
                        };
                        boxed(painter, offset, half);
                    }
                    "CharacterController" => {
                        let (height, radius) = match (field(component, "height"), field(component, "radius")) {
                            (Some(Value::Float(height)), Some(Value::Float(radius))) => (height, radius),
                            _ => (1.8, 0.35),
                        };
                        // Позиция персонажа — у ног.
                        boxed(painter, Vec3::Y * height * 0.5, Vec3::new(radius, height * 0.5, radius));
                    }
                    _ => {}
                }
            }
        }
    }

    crate::terrain_tools::scene_view(editor, ui, painter, response, &projector);
    let missed = if terrain_brush {
        false
    } else {
        gizmo(editor, ui, painter, response, &projector, selection, pointer)
    };
    // ЛКМ мимо ручек и значка ориентации, без Alt / Space — рамка выделения.
    if missed && !alt && !space && !editor.viewport.compass_drag {
        editor.viewport.box_select = ui.input(|i| i.pointer.press_origin());
    }
    let pick_box = |editor: &Editor, area: Rect| -> Vec<GameObjectId> {
        let projector = Projector::new(&editor.orbit, editor.viewport.rect);
        let scene = &editor.app.world.scene;
        crate::selection::tree_order(scene)
            .into_iter()
            .filter(|id| scene.is_active(*id) && !editor.pick_locked(*id))
            .filter(|id| projector.project(scene.world_position(*id)).is_some_and(|p| area.contains(p)))
            .collect()
    };
    crate::viewport::box_select(editor, ui, painter, response, pick_box);
    if ui.input(|i| i.time) < editor.viewport.fly_speed_shown_until {
        let text = trf!("Fly speed  {} m/s", format!("{:.1}", editor.prefs.fly_speed));
        let galley_width = text.len() as f32 * 6.5;
        theme::chip(painter, pos2(rect.center().x - galley_width * 0.5, rect.bottom() - 40.0), &text, theme::TEXT);
        ui.ctx().request_repaint();
    }
    let handles = compass_handles(&editor.orbit, rect);
    let hovered = pointer
        .filter(|_| response.hovered() && !editor.viewport.compass_drag)
        .and_then(|p| compass_hit(&handles, p));
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let active = pointer.is_some_and(|p| response.hovered() && over_compass(rect, p)) || editor.viewport.compass_drag;
    orientation(painter, &handles, hovered, active, rect);

    // --- Клик без перетаскивания: по значку ориентации — вид вдоль оси, иначе выбор объекта ---
    if response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if let Some(direction) = compass_hit(&handles, pointer) {
                editor.viewport.orbit_goal = Some(view_along(direction, &editor.orbit));
                ui.ctx().request_repaint();
            } else if !over_compass(rect, pointer) && !terrain_brush {
                let (origin, direction) = projector.ray(pointer);
                let picked = editor.pick_3d(origin, direction);
                let modifiers = ui.input(|i| i.modifiers);
                editor.pick_with_modifiers(picked.into_iter().collect(), modifiers);
            }
        }
    }
}

/// Гизмо 3D: действует на всё выделение вокруг точки гизмо (Pivot / Center).
/// `true` — левую кнопку нажали мимо ручек.
fn gizmo(
    editor: &mut Editor,
    ui: &egui::Ui,
    painter: &egui::Painter,
    response: &egui::Response,
    projector: &Projector,
    selection: Option<GameObjectId>,
    pointer: Option<Pos2>,
) -> bool {
    let pressed = response.drag_started_by(PointerButton::Primary);
    let Some(id) = selection else {
        editor.viewport.drag_3d = None;
        return pressed;
    };
    let scene = &editor.app.world.scene;
    let (_, world_rotation, active_position) = scene.world_matrix(id).to_scale_rotation_translation();
    let origin = editor.gizmo_pivot().unwrap_or(active_position);
    // Ручки одинакового размера на экране, как бы далеко ни был объект.
    let length = origin.distance(projector.eye) * 0.16;
    // Перемещение и поворот — по осям мира (Global) или объекта (Local); масштаб — всегда
    // по осям объекта.
    let local = [world_rotation * Vec3::X, world_rotation * Vec3::Y, world_rotation * Vec3::Z];
    let axes: [Vec3; 3] = match editor.tool {
        Tool::Scale => local,
        _ if editor.gizmo_local => local,
        _ => [Vec3::X, Vec3::Y, Vec3::Z],
    };
    // Кольца поворота — чуть больше стрелок, чтобы не путались с ними.
    let ring_radius = length * 0.9;
    let colors = [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z];
    let tool = editor.tool;
    let hit = |point: Pos2| {
        if tool == Tool::Rotate {
            return hit_ring(projector, origin, &axes, ring_radius, point).map(|(axis, _)| axis);
        }
        (0..3)
            .filter_map(|axis| {
                let [a, b] = projector.segment(origin, origin + axes[axis] * length)?;
                let distance = distance_to_segment(point, a, b);
                (distance <= HANDLE_REACH).then_some((distance, axis))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, axis)| axis)
    };

    // --- Начало перетаскивания ---
    let mut on_handle = false;
    if pressed {
        let press = ui.input(|i| i.pointer.press_origin());
        if let Some((press, axis)) = press.and_then(|press| Some((press, hit(press)?))) {
            on_handle = true;
            // Alt + перетаскивание перемещения — сначала дублировать выделение.
            if editor.tool == Tool::Move && ui.input(|i| i.modifiers.alt) {
                editor.apply(Action::DuplicateSelection);
            }
            let (ray_origin, ray_direction) = projector.ray(press);
            // Поворот: касательная к кольцу в точке захвата, пикселей на радиан.
            let grab_point = hit_ring(projector, origin, &axes, ring_radius, press).map(|(_, p)| p);
            let tangent = grab_point
                .and_then(|grab| {
                    const STEP: f32 = 0.01;
                    let a = projector.project(grab)?;
                    let b = projector.project(origin + Quat::from_axis_angle(axes[axis], STEP) * (grab - origin))?;
                    Some((b - a) / STEP)
                })
                .unwrap_or(egui::Vec2::ZERO);
            editor.viewport.drag_3d = Some(Drag3D {
                object: editor.selection.unwrap_or(id),
                moved: editor.capture_moved(),
                axis,
                direction: axes[axis],
                grab: closest_on_axis(origin, axes[axis], ray_origin, ray_direction).unwrap_or(0.0),
                pointer_start: press,
                origin,
                tangent,
                screen_center: projector.project(origin).unwrap_or(press),
                facing: axes[axis].dot(projector.eye - origin).signum(),
                grab_point,
            });
        }
    }
    let active_now = editor.selection;
    if response.drag_stopped() || editor.viewport.drag_3d.as_ref().is_some_and(|d| Some(d.object) != active_now) {
        editor.viewport.drag_3d = None;
    }

    // --- Само перетаскивание: всё считается от состояния на момент нажатия ---
    let snap = ui.input(|i| i.modifiers.command) != editor.snap;
    let (snap_move, snap_turn, snap_scale) = editor.snap_steps();
    let mut op = None;
    if let (Some(drag), Some(pointer)) = (editor.viewport.drag_3d.as_ref(), pointer) {
        let (ray_origin, ray_direction) = projector.ray(pointer);
        let along = closest_on_axis(drag.origin, drag.direction, ray_origin, ray_direction);
        match (editor.tool, along) {
            (Tool::Move, Some(along)) => {
                let mut shift = along - drag.grab;
                if snap {
                    shift = (shift / snap_move).round() * snap_move;
                }
                op = Some(GizmoOp::Move(drag.direction * shift));
            }
            (Tool::Scale, Some(along)) if drag.grab.abs() > 1e-4 => {
                let mut factor = (along / drag.grab).max(0.01);
                if snap {
                    factor = (factor / snap_scale).round() * snap_scale;
                }
                let mut factors = Vec3::ONE;
                factors[drag.axis] = factor;
                op = Some(GizmoOp::Scale { factor: factors, axes, pivot: drag.origin });
            }
            (Tool::Rotate, _) => {
                // Поворот вокруг оси кольца: точка кольца под курсором идёт за мышью
                // (раньше — только горизонталь мыши, без учёта того, как ось видна на экране).
                // Кольцо видно не ребром — точка кольца ровно под курсором (пересечение луча с
                // плоскостью кольца); почти ребром — по касательной.
                let mut angle = drag
                    .grab_point
                    .and_then(|grab| plane_angle(drag.origin, drag.direction, grab, ray_origin, ray_direction))
                    .unwrap_or_else(|| ring_angle(drag.tangent, drag.screen_center, drag.facing, drag.pointer_start, pointer));
                if snap {
                    angle = (angle / snap_turn).round() * snap_turn;
                }
                op = Some(GizmoOp::Rotate { rotation: Quat::from_axis_angle(drag.direction, angle), pivot: drag.origin });
            }
            _ => {}
        }
    }
    if let (Some(op), Some(drag)) = (op, editor.viewport.drag_3d.take()) {
        editor.apply_gizmo_op(&drag.moved, op);
        editor.viewport.drag_3d = Some(drag);
    }

    // --- Отрисовка ---
    let active = editor.viewport.drag_3d.as_ref().map(|drag| drag.axis);
    let hovered = active.or_else(|| pointer.filter(|_| response.hovered()).and_then(hit));
    if editor.tool == Tool::Rotate {
        // Три кольца в плоскостях, перпендикулярных осям; задняя половина бледнее.
        let toward_eye = (projector.eye - origin).normalize_or_zero();
        for axis in 0..3 {
            let highlighted = hovered == Some(axis);
            let color = if highlighted { Color32::WHITE } else { colors[axis] };
            let width = if highlighted { 3.0_f32 } else { 2.0_f32 };
            let points = ring_points(origin, axes[axis], ring_radius);
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                let Some([sa, sb]) = projector.segment(a, b) else {
                    continue;
                };
                let behind = ((a + b) * 0.5 - origin).dot(toward_eye) < 0.0;
                let stroke = if behind { Stroke::new(1.0_f32, color.gamma_multiply(0.35)) } else { Stroke::new(width, color) };
                crate::lines::segment(painter, sa, sb, stroke);
            }
        }
        if let Some(center) = projector.project(origin) {
            painter.circle_filled(center, 3.5, Color32::WHITE);
        }
        return pressed && !on_handle;
    }
    for axis in 0..3 {
        let Some([a, b]) = projector.segment(origin, origin + axes[axis] * length) else {
            continue;
        };
        let highlighted = hovered == Some(axis);
        let color = if highlighted { Color32::WHITE } else { colors[axis] };
        crate::lines::segment(painter, a, b, Stroke::new(if highlighted { 3.0_f32 } else { 2.0_f32 }, color));
        let direction = (b - a).normalized();
        let side = vec2(-direction.y, direction.x) * 5.0;
        match editor.tool {
            Tool::Move => {
                painter.add(Shape::convex_polygon(vec![b + direction * 12.0, b + side, b - side], color, Stroke::NONE));
            }
            Tool::Scale => {
                painter.rect_filled(Rect::from_center_size(b, vec2(10.0, 10.0)), 1.0, color);
            }
            Tool::Rotate => {
                painter.circle_stroke(b, 6.0, Stroke::new(2.0_f32, color));
            }
        }
    }
    if let Some(center) = projector.project(origin) {
        painter.circle_filled(center, 3.5, Color32::WHITE);
    }
    pressed && !on_handle
}

/// Полёт: пока зажата правая кнопка, W / A / S / D — вперёд / влево / назад / вправо,
/// Q / E — вниз / вверх, Shift — втрое быстрее; разгон и торможение плавные. Когда вид
/// Scene в фокусе — стрелки двигают камеру и без правой кнопки.
fn fly_3d(editor: &mut Editor, ui: &egui::Ui, flying: bool) {
    let focused = editor.focused_panel() == Some(crate::dock::Panel::Scene) && !ui.ctx().wants_keyboard_input();
    let (input, fast, dt) = ui.input(|i| {
        let key = |k: egui::Key| if i.key_down(k) { 1.0 } else { 0.0 };
        let mut v = Vec3::ZERO; // (вправо, вверх, вперёд)
        if flying {
            v += Vec3::new(
                key(egui::Key::D) - key(egui::Key::A),
                key(egui::Key::E) - key(egui::Key::Q),
                key(egui::Key::W) - key(egui::Key::S),
            );
        }
        if focused && !flying && i.modifiers.is_none() {
            v += Vec3::new(
                key(egui::Key::ArrowRight) - key(egui::Key::ArrowLeft),
                0.0,
                key(egui::Key::ArrowUp) - key(egui::Key::ArrowDown),
            );
        }
        (v, i.modifiers.shift, i.unstable_dt.clamp(MOTION_DT.0, MOTION_DT.1))
    });
    // Время кадра — выровненное окном под период монитора (если оно известно).
    let dt = if editor.viewport.frame_dt > 0.0 { editor.viewport.frame_dt.clamp(MOTION_DT.0, MOTION_DT.1) } else { dt };
    let orbit = editor.orbit;
    let forward = (orbit.target - orbit.eye()).normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let speed = editor.prefs.fly_speed * if fast { 3.0 } else { 1.0 };
    let desired = (right * input.x + Vec3::Y * input.y + forward * input.z).normalize_or_zero() * speed;
    let k = 1.0 - (-dt * 12.0).exp();
    let velocity = editor.viewport.fly_velocity + (desired - editor.viewport.fly_velocity) * k;
    editor.viewport.fly_velocity = if desired == Vec3::ZERO && velocity.length() < 0.01 { Vec3::ZERO } else { velocity };
    if editor.viewport.fly_velocity != Vec3::ZERO {
        editor.orbit.target += editor.viewport.fly_velocity * dt;
        ui.ctx().request_repaint();
    }
}

/// Границы шага времени для движения камеры (с).
pub(crate) const MOTION_DT: (f32, f32) = (1.0 / 500.0, 0.05);

/// Шаг времени для движения камеры — настоящее время с прошлого кадра. `stable_dt` egui
/// для этого не годится: это сглаженная оценка (а после простоя — просто 1/60), и когда
/// кадр длится дольше или короче, камера за него проходит не то расстояние — движение
/// выходит рывками.
pub(crate) fn motion_dt(ui: &egui::Ui) -> f32 {
    ui.input(|i| i.unstable_dt).clamp(MOTION_DT.0, MOTION_DT.1)
}

/// Шаг времени для камеры вида: время кадра, которое окно выровняло под период монитора
/// (`engine_render::frame_time`), а если окна нет (тесты) — [`motion_dt`].
pub(crate) fn camera_dt(editor: &Editor, ui: &egui::Ui) -> f32 {
    if editor.viewport.frame_dt > 0.0 {
        editor.viewport.frame_dt.clamp(MOTION_DT.0, MOTION_DT.1)
    } else {
        motion_dt(ui)
    }
}

/// Обзор правой кнопкой: радиан на один счёт мыши (как у вида Scene в Unity).
pub(crate) const LOOK_SPEED: f32 = 0.003;

/// Наклон камеры ограничен: ровно сверху `look_at` с осью «вверх» по Y вырождается.
const PITCH_LIMIT: f32 = 1.5;
const COMPASS_RADIUS: f32 = 34.0;
const COMPASS_ARM: f32 = 24.0;

/// Угол в диапазоне (-π, π].
fn wrap_angle(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(std::f32::consts::TAU);
    if wrapped > std::f32::consts::PI {
        wrapped - std::f32::consts::TAU
    } else {
        wrapped
    }
}

fn compass_center(rect: Rect) -> Pos2 {
    pos2(rect.right() - 46.0, rect.top() + 46.0)
}

fn over_compass(rect: Rect, point: Pos2) -> bool {
    point.distance(compass_center(rect)) <= COMPASS_RADIUS + 2.0
}

/// Ручка значка ориентации: конец мировой оси (`+X`, `-X`, …) на экране.
pub(crate) struct CompassHandle {
    /// Мировое направление; по щелчку камера встаёт с этой стороны.
    pub direction: Vec3,
    pub center: Pos2,
    pub radius: f32,
    color: Color32,
    /// Буква оси; у отрицательного конца её нет.
    label: &'static str,
    /// Смотрит от камеры (рисуется тусклее).
    away: bool,
}

/// Шесть ручек значка, дальние первыми (так их и рисуют).
pub(crate) fn compass_handles(orbit: &Orbit3D, rect: Rect) -> Vec<CompassHandle> {
    let center = compass_center(rect);
    let forward = (orbit.target - orbit.eye()).normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward);
    let mut handles: Vec<(f32, CompassHandle)> = [
        (Vec3::X, theme::AXIS_X, tr_s("X")),
        (Vec3::Y, theme::AXIS_Y, tr_s("Y")),
        (Vec3::Z, theme::AXIS_Z, tr_s("Z")),
    ]
    .into_iter()
    .flat_map(|(axis, color, label)| [(axis, color, label, true), (-axis, color, label, false)])
    .map(|(direction, color, label, positive)| {
        let depth = direction.dot(forward);
        let handle = CompassHandle {
            direction,
            center: center + vec2(direction.dot(right), -direction.dot(up)) * COMPASS_ARM,
            radius: if positive { 8.0 } else { 5.5 },
            color,
            label: if positive { label } else { "" },
            away: depth > 0.3,
        };
        (depth, handle)
    })
    .collect();
    handles.sort_by(|a, b| b.0.total_cmp(&a.0));
    handles.into_iter().map(|(_, handle)| handle).collect()
}

/// Ручка под курсором: ближняя к камере выигрывает у дальней.
pub(crate) fn compass_hit(handles: &[CompassHandle], point: Pos2) -> Option<Vec3> {
    handles
        .iter()
        .rev()
        .find(|handle| handle.center.distance(point) <= handle.radius + 2.0)
        .map(|handle| handle.direction)
}

/// Углы камеры, с которыми она стоит со стороны `direction` и смотрит на цель.
pub(crate) fn view_along(direction: Vec3, orbit: &Orbit3D) -> (f32, f32) {
    if direction.y.abs() > 0.5 {
        // Сверху и снизу поворот вокруг вертикали сохраняется.
        (orbit.yaw, PITCH_LIMIT * direction.y.signum())
    } else {
        (direction.x.atan2(direction.z), 0.0)
    }
}

/// Значок ориентации в правом верхнем углу: куда смотрят оси мира. Его ручки нажимаются.
fn orientation(painter: &egui::Painter, handles: &[CompassHandle], hovered: Option<Vec3>, active: bool, rect: Rect) {
    let center = compass_center(rect);
    let backdrop = if active { 205 } else { 150 };
    painter.circle_filled(center, COMPASS_RADIUS, Color32::from_rgba_unmultiplied(12, 13, 15, backdrop));
    for handle in handles {
        let positive = !handle.label.is_empty();
        let lit = hovered == Some(handle.direction);
        let color = if handle.away && !lit { handle.color.gamma_multiply(0.5) } else { handle.color };
        if positive {
            painter.line_segment([center, handle.center], Stroke::new(2.0_f32, color));
            painter.circle_filled(handle.center, handle.radius, color);
            painter.text(
                handle.center,
                egui::Align2::CENTER_CENTER,
                handle.label,
                theme::strong(10.5),
                Color32::from_rgb(0x10, 0x11, 0x13),
            );
        } else {
            // Отрицательный конец оси: кружок без подписи, как в Unity.
            painter.circle_filled(handle.center, handle.radius, color.gamma_multiply(0.55));
            painter.circle_stroke(handle.center, handle.radius, Stroke::new(1.5_f32, color));
        }
        if lit {
            painter.circle_stroke(handle.center, handle.radius + 2.0, Stroke::new(1.5_f32, Color32::WHITE));
        }
    }
}
