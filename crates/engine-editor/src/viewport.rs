//! Вьюпорт: вид Scene (камера редактора, сетка, гизмо) и вид Game (камера сцены).
//!
//! Сама сцена приходит готовой текстурой от рендерера движка; здесь поверх неё
//! рисуются сетка, рамки и гизмо, и обрабатывается мышь.

use egui::{pos2, vec2, Color32, PointerButton, Pos2, Rect, Sense, Shape, Stroke};
use engine_core::prelude::*;
use engine_render::{Camera, Projection, SpriteRenderer};

use crate::hierarchy::object_icon;
use crate::icons::{self, Icon};
use crate::project::DragAsset;
use crate::state::{sprite_quad, Action, Editor, Mode, Tool, ViewTab};
use crate::theme;

const AXIS_LENGTH: f32 = 72.0;
const RING_RADIUS: f32 = 62.0;
const HANDLE_REACH: f32 = 8.0;

pub(crate) struct ViewportState {
    /// Текстура со сценой, которую нарисовал рендерер в этом кадре.
    pub texture: Option<egui::TextureId>,
    /// Где вьюпорт находится в окне (точки egui).
    pub rect: Rect,
    /// Его размер в пикселях — под него рендерер создаёт текстуру.
    pub pixels: [u32; 2],
    pub drag: Option<GizmoDrag>,
    pub drag_3d: Option<crate::view3d::Drag3D>,
    /// Вид 3D вращают за значок ориентации (нажали на нём и тянут).
    pub compass_drag: bool,
    /// Куда плавно доворачивается камера 3D после щелчка по оси значка: (yaw, pitch).
    pub orbit_goal: Option<(f32, f32)>,
    /// Границы мешей из файлов — для выбора щелчком и каркаса выделения.
    pub mesh_bounds: std::collections::HashMap<engine_assets::Guid, (Vec3, Vec3)>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            texture: None,
            rect: Rect::from_min_size(Pos2::ZERO, vec2(960.0, 540.0)),
            pixels: [960, 540],
            drag: None,
            drag_3d: None,
            compass_drag: false,
            orbit_goal: None,
            mesh_bounds: std::collections::HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handle {
    X,
    Y,
    /// Центральный квадрат: свободное перемещение / равномерный масштаб.
    Free,
    /// Кольцо поворота.
    Ring,
}

/// Перетаскивание гизмо: всё считается от состояния на момент нажатия,
/// поэтому ошибки не накапливаются от кадра к кадру.
pub(crate) struct GizmoDrag {
    object: GameObjectId,
    handle: Handle,
    pointer_start: Vec2,
    /// Мировая позиция объекта в начале.
    origin: Vec3,
    /// Локальный Transform в начале.
    start: Transform,
    parent_inverse: Mat4,
    /// Мировые направления локальных осей объекта (для масштаба).
    axis_x: Vec2,
    axis_y: Vec2,
}

/// Соответствие «мир ↔ окно» для вида Scene.
#[derive(Clone, Copy, Debug)]
pub struct ViewTransform {
    pub rect: Rect,
    pub center: Vec2,
    pub half_height: f32,
}

impl ViewTransform {
    /// Точек окна на мировую единицу.
    pub fn scale(&self) -> f32 {
        self.rect.height() / (2.0 * self.half_height.max(1e-6))
    }

    pub fn to_screen(&self, world: Vec2) -> Pos2 {
        let offset = (world - self.center) * self.scale();
        pos2(self.rect.center().x + offset.x, self.rect.center().y - offset.y)
    }

    pub fn to_world(&self, screen: Pos2) -> Vec2 {
        let offset = screen - self.rect.center();
        self.center + Vec2::new(offset.x, -offset.y) / self.scale()
    }
}

/// Окно Scene: камера редактора, сетка, гизмо.
pub(crate) fn show_scene(editor: &mut Editor, ui: &mut egui::Ui) {
    // Полоса вида Scene: что рисовать поверх сцены, камера, сводка справа.
    let objects = editor.app.world.scene.len();
    let draw_calls = editor.profiler.stats.draw_calls;
    theme::toolbar_strip(ui, |ui| {
        theme::dropdown_button(ui, "Gizmos", "What the Scene view draws over the scene", |ui| {
            if theme::item("Object Icons and Colliders").icon(Icon::Gizmo).checked(editor.show_gizmos).show(ui) {
                editor.show_gizmos = !editor.show_gizmos;
            }
            if theme::item("Grid").icon(Icon::Grid).checked(editor.show_grid).show(ui) {
                editor.show_grid = !editor.show_grid;
            }
        });
        theme::dropdown_button(ui, "Camera", "The Scene view camera", |ui| {
            if theme::item("3D View").icon(Icon::Cube).shortcut(&editor.shortcut_text("view-3d")).checked(editor.scene_3d).show(ui) {
                let enabled = !editor.scene_3d;
                editor.set_scene_3d(enabled);
            }
            theme::menu_separator(ui);
            if theme::item("Frame Selected")
                .icon(Icon::Search)
                .shortcut(&editor.shortcut_text("frame-selected"))
                .enabled_if(editor.selection.is_some(), "Select an object first")
                .show(ui)
            {
                editor.actions.push(Action::FrameSelected);
            }
            if theme::item("Frame Whole Scene").show(ui) {
                editor.frame_all();
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{objects} objects  ·  {draw_calls} draw calls"))
                    .size(theme::FONT_CAPTION)
                    .color(theme::TEXT_MUTED),
            );
        });
    });
    show_view(editor, ui, ViewTab::Scene);
}

/// Окно Game: то, что видит камера игры, со своей полосой инструментов.
pub(crate) fn show_game(editor: &mut Editor, ui: &mut egui::Ui) {
    let pixels = editor.dock.game.pixels;
    let draw_calls = editor.profiler.stats.draw_calls;
    let mut maximize = editor.dock.maximize_on_play;
    theme::toolbar_strip(ui, |ui| {
        ui.label(
            egui::RichText::new(format!("Display  {} × {}", pixels[0], pixels[1]))
                .size(theme::FONT_CAPTION)
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::toggle_button(
                ui,
                &mut maximize,
                Icon::Collapse,
                "Maximize On Play",
                "Enlarge the Game view to the whole dock area when Play starts",
            );
            ui.label(
                egui::RichText::new(format!("{draw_calls} draw calls"))
                    .size(theme::FONT_CAPTION)
                    .color(theme::TEXT_MUTED),
            );
        });
    });
    editor.dock.maximize_on_play = maximize;
    show_view(editor, ui, ViewTab::Game);
}

/// Область вида Scene или Game: текстура сцены, поверх — гизмо или рамка игры.
fn show_view(editor: &mut Editor, ui: &mut egui::Ui, which: ViewTab) {
    let rect = ui.available_rect_before_wrap();
    let id = if which == ViewTab::Game { Editor::game_view_id() } else { Editor::viewport_id() };
    let response = ui.interact(rect, id, Sense::click_and_drag());
    ui.allocate_rect(rect, Sense::hover());
    let pixels_per_point = ui.ctx().pixels_per_point();
    let pixels = [
        ((rect.width() * pixels_per_point).round() as u32).clamp(1, 8192),
        ((rect.height() * pixels_per_point).round() as u32).clamp(1, 8192),
    ];
    let texture = if which == ViewTab::Game {
        editor.dock.game.rect = rect;
        editor.dock.game.pixels = pixels;
        editor.dock.game.texture
    } else {
        editor.viewport.rect = rect;
        editor.viewport.pixels = pixels;
        editor.viewport.texture
    };

    let painter = ui.painter().with_clip_rect(rect);
    match texture {
        Some(texture) => {
            painter.image(
                texture,
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(rect, 0.0, theme::BG_APP);
        }
    }

    match which {
        ViewTab::Scene if editor.scene_3d => {
            crate::view3d::show(editor, ui, &painter, &response);
            hint(&painter, rect, "Right drag  orbit     Drag  pan     Wheel  zoom     F  frame selection");
        }
        ViewTab::Scene => scene_view(editor, ui, &painter, &response),
        ViewTab::Game | ViewTab::Code => game_view(editor, &painter, rect),
    }

    // Перетаскивание ассета из окна Project.
    if response.dnd_hover_payload::<DragAsset>().is_some() {
        painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0_f32, theme::accent()));
    }
    if let Some(asset) = response.dnd_release_payload::<DragAsset>() {
        let position = match which {
            // В 3D объект ложится на землю под курсором.
            ViewTab::Scene if editor.scene_3d => ui.ctx().pointer_latest_pos().and_then(|pointer| {
                let (origin, direction) =
                    crate::view3d::Projector::new(&editor.orbit, rect).ray(pointer);
                (direction.y < -1e-4).then(|| origin + direction * (-origin.y / direction.y))
            }),
            ViewTab::Scene => ui
                .ctx()
                .pointer_latest_pos()
                .map(|pointer| editor.view_transform().to_world(pointer).extend(0.0)),
            ViewTab::Game | ViewTab::Code => None,
        };
        match asset.importer.as_str() {
            "Prefab" => editor.actions.push(Action::Instantiate {
                reference: asset.path.clone(),
                position,
            }),
            "Texture" => editor.actions.push(Action::CreateSpriteFromTexture {
                guid: asset.guid,
                position,
            }),
            "Scene" => editor.actions.push(Action::OpenScene(asset.path.clone())),
            // Материал — объекту под курсором (первому слоту его MeshRenderer).
            "Material" => {
                let target = ui.ctx().pointer_latest_pos().and_then(|pointer| match which {
                    ViewTab::Scene if editor.scene_3d => {
                        let (origin, direction) = crate::view3d::Projector::new(&editor.orbit, rect).ray(pointer);
                        editor.pick_3d(origin, direction)
                    }
                    ViewTab::Scene => {
                        let point = editor.view_transform().to_world(pointer);
                        editor.pick(point, 0.1)
                    }
                    ViewTab::Game | ViewTab::Code => None,
                });
                match target {
                    Some(object) => editor.actions.push(Action::AssignMaterial { object, material: asset.guid, slot: 0 }),
                    None => log::warn!("drop `{}` onto an object with a MeshRenderer", asset.path),
                }
            }
            other => log::warn!("`{}` ({other}) cannot be dropped into the scene", asset.path),
        }
    }
}

fn game_view(editor: &Editor, painter: &egui::Painter, rect: Rect) {
    match editor.mode {
        Mode::Playing | Mode::Paused => {
            let color = if editor.mode == Mode::Playing { theme::accent() } else { theme::WARN };
            painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0_f32, color));
            if editor.mode == Mode::Paused {
                theme::chip(painter, rect.left_top() + vec2(12.0, 12.0), "Paused — Step advances one frame", theme::WARN);
            }
        }
        Mode::Edit => {
            let text = format!(
                "This is what the game camera sees. {} to run the game",
                editor.prefs.shortcuts.tip("Press Play", "play")
            );
            let galley = painter.layout_no_wrap(text, theme::caption(), theme::TEXT_MUTED);
            let size = galley.size() + vec2(16.0, 8.0);
            let plate = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 24.0), size);
            painter.rect_filled(plate, theme::R_S, Color32::from_rgba_unmultiplied(16, 16, 16, 200));
            painter.galley(plate.center() - galley.size() * 0.5, galley, theme::TEXT_MUTED);
        }
    }
}

/// Подсказка по управлению в нижнем левом углу вида.
fn hint(painter: &egui::Painter, rect: Rect, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), theme::caption(), theme::TEXT_MUTED);
    let plate = Rect::from_min_size(
        rect.left_bottom() + vec2(8.0, -galley.size().y - 14.0),
        galley.size() + vec2(12.0, 6.0),
    );
    painter.rect_filled(plate, theme::R_S, Color32::from_rgba_unmultiplied(16, 16, 16, 160));
    painter.galley(plate.min + vec2(6.0, 3.0), galley, theme::TEXT_MUTED);
}

fn scene_view(editor: &mut Editor, ui: &egui::Ui, painter: &egui::Painter, response: &egui::Response) {
    let rect = editor.viewport.rect;
    let pointer = ui.ctx().pointer_latest_pos();

    // --- Навигация: колесо — масштаб вокруг курсора, ПКМ/СКМ — сдвиг ---
    if response.hovered() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (-scroll * 0.0025 * editor.prefs.zoom_speed).exp() / pinch;
        if (factor - 1.0).abs() > 1e-4 {
            let anchor = pointer.filter(|p| rect.contains(*p)).unwrap_or(rect.center());
            let before = editor.view_transform().to_world(anchor);
            editor.cam_half_height = (editor.cam_half_height * factor).clamp(0.05, 5000.0);
            let after = editor.view_transform().to_world(anchor);
            editor.cam_center += before - after;
        }
    }
    let panning = response.dragged_by(PointerButton::Secondary)
        || response.dragged_by(PointerButton::Middle)
        || (response.dragged_by(PointerButton::Primary)
            && !response.drag_started_by(PointerButton::Primary)
            && editor.viewport.drag.is_none());
    if panning {
        let delta = response.drag_delta() / editor.view_transform().scale();
        editor.cam_center += Vec2::new(-delta.x, delta.y);
    }

    // Сетка 2D-вида рисуется рендером за спрайтами (`CameraView::grid`).
    let view = editor.view_transform();

    // --- Рамки камер и маркеры объектов без спрайта ---
    let game_aspect = rect.width() / rect.height().max(1.0);
    let scene = &editor.app.world.scene;
    for id in scene.ids() {
        let Some(object) = scene.get(id) else { continue };
        if !scene.is_active(id) || !editor.show_gizmos {
            continue;
        }
        let selected = editor.selection == Some(id);
        if let Some(camera) = object.get::<Camera>() {
            if camera.projection == Projection::Orthographic {
                let (_, rotation, position) = scene.world_matrix(id).to_scale_rotation_translation();
                let half = Vec2::new(camera.ortho_size * game_aspect, camera.ortho_size);
                let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| {
                    let local = Vec3::new(half.x * x, half.y * y, 0.0);
                    view.to_screen((position + rotation * local).truncate())
                });
                let alpha = if selected { 220 } else { 90 };
                painter.add(Shape::closed_line(
                    corners.to_vec(),
                    Stroke::new(1.0_f32, Color32::from_white_alpha(alpha)),
                ));
            }
        }
        if !object.has::<SpriteRenderer>() {
            let center = view.to_screen(scene.world_position(id).truncate());
            if rect.expand(12.0).contains(center) {
                let (icon, _) = object_icon(object);
                let color = if selected { theme::accent() } else { Color32::from_white_alpha(150) };
                icons::paint(painter, Rect::from_center_size(center, vec2(14.0, 14.0)), icon, color);
            }
        }
    }

    // --- Выделение: контур спрайта и коллайдера, затем гизмо ---
    let selection = editor.selection.filter(|id| scene.contains(*id));
    if let Some(id) = selection {
        if let Some(quad) = sprite_quad(scene, id) {
            painter.add(Shape::closed_line(
                quad.map(|corner| view.to_screen(corner)).to_vec(),
                Stroke::new(1.5_f32, theme::accent()),
            ));
        }
        if editor.show_gizmos {
            draw_collider(painter, &view, scene, id);
        }
    }

    gizmo(editor, ui, painter, response, selection, pointer);

    // --- Клик без перетаскивания — выбор объекта ---
    if response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let view = editor.view_transform();
            let picked = editor.pick(view.to_world(pointer), 10.0 / view.scale());
            editor.actions.push(Action::Select(picked));
        }
    }

    hint(painter, rect, "Drag  pan     Wheel  zoom     F  frame selection     Ctrl  snap");
}

/// Контур 2D-коллайдера. Физику редактор не подключает: форма читается через
/// reflection по именам полей компонента `Collider2D`.
fn draw_collider(painter: &egui::Painter, view: &ViewTransform, scene: &Scene, id: GameObjectId) {
    let Some(object) = scene.get(id) else { return };
    let Some(collider) = object.components().find(|c| c.type_name() == "Collider2D") else {
        return;
    };
    let vec2_field = |name: &str| match collider.get_field(name) {
        Some(Value::Vec2(x, y)) => Vec2::new(x, y),
        _ => Vec2::ZERO,
    };
    let shape = match collider.get_field("shape") {
        Some(Value::Enum(name)) => name,
        _ => "Box".to_string(),
    };
    let offset = vec2_field("offset");
    let matrix = scene.world_matrix(id);
    let stroke = Stroke::new(1.0_f32, theme::OK);
    let to_screen = |local: Vec2| view.to_screen(matrix.transform_point3(local.extend(0.0)).truncate());

    if shape == "Circle" {
        let radius = match collider.get_field("radius") {
            Some(Value::Float(radius)) => radius,
            _ => 0.5,
        };
        let (scale, _, _) = matrix.to_scale_rotation_translation();
        let world_radius = radius * scale.x.abs().max(scale.y.abs());
        painter.circle_stroke(to_screen(offset), world_radius * view.scale(), stroke);
    } else {
        let half = vec2_field("size") * 0.5;
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| to_screen(offset + Vec2::new(half.x * x, half.y * y)));
        painter.add(Shape::closed_line(corners.to_vec(), stroke));
    }
}

/// Направление мировой оси на экране (ось Y экрана смотрит вниз).
fn screen_dir(world: Vec2) -> egui::Vec2 {
    vec2(world.x, -world.y)
}

fn distance_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((point - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (point - (a + ab * t)).length()
}

fn hit_handle(tool: Tool, origin: Pos2, axis_x: Vec2, axis_y: Vec2, pointer: Pos2) -> Option<Handle> {
    let offset = pointer - origin;
    match tool {
        Tool::Rotate => ((offset.length() - RING_RADIUS).abs() <= HANDLE_REACH).then_some(Handle::Ring),
        Tool::Move | Tool::Scale => {
            if offset.x.abs() <= 9.0 && offset.y.abs() <= 9.0 {
                return Some(Handle::Free);
            }
            let end_x = origin + screen_dir(axis_x) * AXIS_LENGTH;
            let end_y = origin + screen_dir(axis_y) * AXIS_LENGTH;
            if distance_to_segment(pointer, origin, end_x) <= HANDLE_REACH {
                Some(Handle::X)
            } else if distance_to_segment(pointer, origin, end_y) <= HANDLE_REACH {
                Some(Handle::Y)
            } else {
                None
            }
        }
    }
}

fn gizmo(
    editor: &mut Editor,
    ui: &egui::Ui,
    painter: &egui::Painter,
    response: &egui::Response,
    selection: Option<GameObjectId>,
    pointer: Option<Pos2>,
) {
    let Some(id) = selection else {
        editor.viewport.drag = None;
        return;
    };
    let view = editor.view_transform();
    let scene = &editor.app.world.scene;
    let world = scene.world_matrix(id);
    let (_, world_rotation, world_position) = world.to_scale_rotation_translation();
    let origin = view.to_screen(world_position.truncate());

    // Перемещение — по осям мира; масштаб — по локальным осям объекта.
    let local_x = (world_rotation * Vec3::X).truncate().try_normalize().unwrap_or(Vec2::X);
    let local_y = (world_rotation * Vec3::Y).truncate().try_normalize().unwrap_or(Vec2::Y);
    let (axis_x, axis_y) = match editor.tool {
        Tool::Scale => (local_x, local_y),
        _ => (Vec2::X, Vec2::Y),
    };

    // --- Начало перетаскивания ---
    if response.drag_started_by(PointerButton::Primary) {
        let press = ui.input(|i| i.pointer.press_origin());
        let handle = press.and_then(|press| hit_handle(editor.tool, origin, axis_x, axis_y, press));
        if let (Some(handle), Some(press), Some(object)) = (handle, press, scene.get(id)) {
            let parent_inverse = object
                .parent()
                .map_or(Mat4::IDENTITY, |parent| scene.world_matrix(parent).inverse());
            editor.viewport.drag = Some(GizmoDrag {
                object: id,
                handle,
                pointer_start: view.to_world(press),
                origin: world_position,
                start: object.transform,
                parent_inverse,
                axis_x,
                axis_y,
            });
        }
    }
    if response.drag_stopped() || editor.viewport.drag.as_ref().is_some_and(|d| d.object != id) {
        editor.viewport.drag = None;
    }

    // --- Само перетаскивание ---
    let snap = ui.input(|i| i.modifiers.command) != editor.snap;
    let (snap_move, snap_turn, snap_scale) = editor.snap_steps();
    let mut readout = None;
    if let (Some(drag), Some(pointer)) = (editor.viewport.drag.as_ref(), pointer) {
        let now = view.to_world(pointer);
        let delta = now - drag.pointer_start;
        let origin_2d = drag.origin.truncate();
        let mut transform = drag.start;
        match (editor.tool, drag.handle) {
            (Tool::Move, handle) => {
                let delta = match handle {
                    Handle::X => Vec2::new(delta.x, 0.0),
                    Handle::Y => Vec2::new(0.0, delta.y),
                    _ => delta,
                };
                let mut target = origin_2d + delta;
                if snap {
                    if handle != Handle::Y {
                        target.x = (target.x / snap_move).round() * snap_move;
                    }
                    if handle != Handle::X {
                        target.y = (target.y / snap_move).round() * snap_move;
                    }
                }
                transform.position = drag
                    .parent_inverse
                    .transform_point3(target.extend(drag.origin.z));
                readout = Some(format!("{:.2}, {:.2}", target.x, target.y));
            }
            (Tool::Rotate, _) => {
                let from = drag.pointer_start - origin_2d;
                let to = now - origin_2d;
                if from.length_squared() > 1e-8 && to.length_squared() > 1e-8 {
                    let mut angle = from.angle_to(to);
                    if snap {
                        let step = snap_turn;
                        angle = (angle / step).round() * step;
                    }
                    transform.rotation = Quat::from_rotation_z(angle) * drag.start.rotation;
                    readout = Some(format!("{:+.1}°", angle.to_degrees()));
                }
            }
            (Tool::Scale, handle) => {
                let along = |axis: Vec2| {
                    let start = (drag.pointer_start - origin_2d).dot(axis);
                    let current = (now - origin_2d).dot(axis);
                    if start.abs() > 1e-5 { current / start } else { 1.0 }
                };
                let mut factor = match handle {
                    Handle::X => Vec2::new(along(drag.axis_x), 1.0),
                    Handle::Y => Vec2::new(1.0, along(drag.axis_y)),
                    _ => Vec2::splat(1.0 + (delta.x + delta.y) * view.scale() / AXIS_LENGTH),
                };
                if snap {
                    factor = (factor / snap_scale).round() * snap_scale;
                }
                transform.scale = drag.start.scale * Vec3::new(factor.x, factor.y, 1.0);
                if handle == Handle::Free {
                    transform.scale.z = drag.start.scale.z * factor.x;
                }
                readout = Some(format!("× {:.2}, {:.2}", factor.x, factor.y));
            }
        }
        let changed = editor
            .app
            .world
            .scene
            .get(id)
            .is_some_and(|object| object.transform != transform);
        if changed {
            editor.touch();
            if let Some(object) = editor.app.world.scene.get_mut(id) {
                object.transform = transform;
            }
        }
    }

    // --- Отрисовка ---
    let active = editor.viewport.drag.as_ref().map(|drag| drag.handle);
    let hovered = match active {
        Some(handle) => Some(handle),
        None => pointer
            .filter(|_| response.hovered())
            .and_then(|pointer| hit_handle(editor.tool, origin, axis_x, axis_y, pointer)),
    };
    let tint = |handle: Handle, color: Color32| {
        if hovered == Some(handle) { Color32::WHITE } else { color }
    };
    let width = |handle: Handle| if hovered == Some(handle) { 3.0_f32 } else { 2.0_f32 };

    match editor.tool {
        Tool::Move | Tool::Scale => {
            for (handle, axis, color) in [
                (Handle::X, axis_x, theme::AXIS_X),
                (Handle::Y, axis_y, theme::AXIS_Y),
            ] {
                let direction = screen_dir(axis);
                let end = origin + direction * AXIS_LENGTH;
                let color = tint(handle, color);
                painter.line_segment([origin, end], Stroke::new(width(handle), color));
                if editor.tool == Tool::Move {
                    let side = vec2(-direction.y, direction.x) * 5.0;
                    painter.add(Shape::convex_polygon(
                        vec![end + direction * 12.0, end + side, end - side],
                        color,
                        Stroke::NONE,
                    ));
                } else {
                    painter.rect_filled(Rect::from_center_size(end, vec2(10.0, 10.0)), 1.0, color);
                }
            }
            let center = Rect::from_center_size(origin, vec2(14.0, 14.0));
            painter.rect_filled(center, 2.0, tint(Handle::Free, theme::accent()).gamma_multiply(0.9));
            painter.rect_stroke(center, 2.0, Stroke::new(1.0_f32, Color32::WHITE));
        }
        Tool::Rotate => {
            painter.circle_stroke(
                origin,
                RING_RADIUS,
                Stroke::new(width(Handle::Ring), tint(Handle::Ring, theme::AXIS_Z)),
            );
            painter.line_segment(
                [origin, origin + screen_dir(local_x) * RING_RADIUS],
                Stroke::new(1.0_f32, theme::AXIS_X),
            );
            painter.circle_filled(origin, 3.0, Color32::WHITE);
        }
    }
    if let Some(text) = readout {
        theme::chip(painter, origin + vec2(16.0, 16.0), &text, Color32::WHITE);
    }
}
