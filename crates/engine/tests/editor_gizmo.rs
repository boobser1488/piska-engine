//! Гизмо вида Scene настоящим вводом egui, без GPU: кольца поворота (ось и знак идут за
//! мышью), Global / Local, Pivot / Center, одно и несколько выделенных, повёрнутый родитель,
//! неравномерный масштаб, соответствие полям Rotation X / Y / Z инспектора; стрелки
//! перемещения и ручки масштаба; поворот в 2D.
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Orbit3D, Tool};
use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const NONE: egui::Modifiers = egui::Modifiers::NONE;
/// Размеры гизмо 3D, как в view3d.rs: стрелка — 0.16 расстояния до камеры, кольцо — 0.9 её.
const ARROW: f32 = 0.16;
const RING: f32 = 0.9;

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap()
}

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0 };
        for _ in 0..3 {
            hands.frame(editor, vec![]);
        }
        hands
    }

    fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        editor.forward_game_input(&self.ctx, &input);
        let _ = self.ctx.run(input, |ctx| editor.ui(ctx));
    }

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        let b = egui::PointerButton::Primary;
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: b, pressed, modifiers: NONE };
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![button(from, true)]);
        for step in 1..=8 {
            self.frame(editor, vec![egui::Event::PointerMoved(from + (to - from) * (step as f32 / 8.0))]);
        }
        self.frame(editor, vec![button(to, false)]);
        self.frame(editor, vec![]);
    }
}

/// Объект с несимметричным поворотом в корне сцены, камера смотрит на него.
fn setup(editor: &mut Editor, view: (f32, f32)) -> GameObjectId {
    let scene = &mut editor.app.world.scene;
    let id = scene.spawn("Gizmo Probe");
    let object = scene.get_mut(id).unwrap();
    object.transform.position = Vec3::new(40.0, 30.0, 0.0);
    object.transform.set_euler_deg(Vec3::new(20.0, 35.0, 10.0));
    look_at(editor, Vec3::new(40.0, 30.0, 0.0), view);
    editor.apply(Action::Select(Some(id)));
    id
}

fn look_at(editor: &mut Editor, target: Vec3, (yaw, pitch): (f32, f32)) {
    editor.set_scene_3d(true);
    editor.set_orbit(Orbit3D { target, yaw, pitch, distance: 9.0 });
}

/// Вернуть объектам их позы (вместо Undo: Undo пересобирает сцену, и id объектов меняются).
fn restore(editor: &mut Editor, poses: &[(GameObjectId, Transform)]) {
    for (id, pose) in poses {
        editor.app.world.scene.get_mut(*id).unwrap().transform = *pose;
    }
}

fn poses(editor: &Editor, ids: &[GameObjectId]) -> Vec<(GameObjectId, Transform)> {
    ids.iter().map(|id| (*id, editor.app.world.scene.get(*id).unwrap().transform)).collect()
}

fn world_rotation(editor: &Editor, id: GameObjectId) -> Quat {
    editor.app.world.scene.world_matrix(id).to_scale_rotation_translation().1
}

/// Ось и угол поворота `after · before⁻¹` (угол со знаком вдоль `axis`).
fn turn_about(before: Quat, after: Quat, axis: Vec3) -> (Vec3, f32) {
    let mut delta = after * before.inverse();
    if delta.w < 0.0 {
        delta = -delta;
    }
    let (turn_axis, angle) = delta.to_axis_angle();
    if turn_axis.dot(axis) < 0.0 {
        (-turn_axis, -angle)
    } else {
        (turn_axis, angle)
    }
}

/// Точка кольца `axis` для захвата: на передней половине, подальше от двух других колец
/// и с самой длинной касательной на экране. Возвращает точку на экране, мировую точку и
/// касательную (пикселей на радиан поворота «+» вокруг оси).
fn ring_grab(editor: &Editor, origin: Vec3, axes: [Vec3; 3], axis: usize) -> (egui::Pos2, Vec3, egui::Vec2) {
    let eye = editor.orbit().eye();
    let radius = origin.distance(eye) * ARROW * RING;
    let ring = |a: Vec3| {
        let u = a.any_orthonormal_vector();
        let v = a.cross(u);
        (0..360).map(move |i| {
            let t = (i as f32).to_radians();
            origin + (u * t.cos() + v * t.sin()) * radius
        })
    };
    let others: Vec<egui::Pos2> =
        (0..3).filter(|i| *i != axis).flat_map(|i| ring(axes[i])).filter_map(|p| editor.world_to_screen_3d(p)).collect();
    let toward_eye = (eye - origin).normalize();
    ring(axes[axis])
        .filter(|p| (*p - origin).dot(toward_eye) > 0.05 * radius)
        .filter_map(|p| {
            let screen = editor.world_to_screen_3d(p)?;
            let ahead = editor.world_to_screen_3d(origin + Quat::from_axis_angle(axes[axis], 0.01) * (p - origin))?;
            let clear = others.iter().map(|o| o.distance(screen)).fold(f32::INFINITY, f32::min);
            // Подальше от других колец (дальше досягаемости ручки 8 px) и с длинной касательной.
            (clear > 10.0).then_some((screen, p, (ahead - screen) / 0.01, clear.min(24.0)))
        })
        .max_by(|a, b| (a.3 * a.2.length()).total_cmp(&(b.3 * b.2.length())))
        .map(|(screen, p, tangent, _)| (screen, p, tangent))
        .unwrap_or_else(|| panic!("ring {axis} has a clear visible point"))
}

/// Перетащить кольцо `axis` на `pixels` вдоль касательной и проверить: повернулось вокруг
/// этой оси, со знаком движения мыши, и точка кольца пришла под курсор.
fn drag_ring(hands: &mut Hands, editor: &mut Editor, ids: &[GameObjectId], axes: [Vec3; 3], axis: usize, pixels: f32) -> f32 {
    hands.frame(editor, vec![]);
    let origin = editor.gizmo_pivot_world().expect("a selection");
    let (press, grab, tangent) = ring_grab(editor, origin, axes, axis);
    let before: Vec<Quat> = ids.iter().map(|id| world_rotation(editor, *id)).collect();
    let start = poses(editor, ids);
    let to = press + tangent.normalized() * pixels;
    hands.drag(editor, press, to);
    let expected = pixels / tangent.length();
    let mut angle = 0.0;
    for (id, before) in ids.iter().zip(before) {
        let (turn_axis, turned) = turn_about(before, world_rotation(editor, *id), axes[axis]);
        assert!(
            turn_axis.dot(axes[axis]) > 0.995,
            "ring {axis}: the object turned about {turn_axis}, not the ring axis {}",
            axes[axis]
        );
        assert!(
            turned * pixels.signum() > 0.0 && (turned.abs() - expected.abs()).abs() < expected.abs() * 0.3,
            "ring {axis}: dragged {pixels} px along the ring, turned {turned} rad (expected about {expected})"
        );
        angle = turned;
    }
    // Захваченная точка кольца — под курсором (поворот точкой, а не горизонталью мыши).
    // Кольцо, видное почти ребром, крутится по касательной — там «точно под курсором» не
    // определено, проверяются только ось и знак.
    // Точка остаётся на кольце, поэтому «под курсором» — по направлению от центра: центр,
    // повёрнутая точка и курсор на одной прямой экрана.
    let facing = axes[axis].dot((editor.orbit().eye() - origin).normalize()).abs();
    if facing >= 0.3 {
        let followed = Quat::from_axis_angle(axes[axis], angle) * (grab - origin) + origin;
        let landed = editor.world_to_screen_3d(followed).unwrap();
        let center = editor.world_to_screen_3d(origin).unwrap();
        let (u, v) = (landed - center, to - center);
        let off = (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y).to_degrees().abs();
        assert!(off < 2.5, "ring {axis}: the grabbed point is at {landed:?}, {off:.1}° away from the mouse at {to:?}");
    }
    restore(editor, &start);
    angle
}

#[test]
fn every_rotate_ring_turns_about_its_own_axis_with_the_sign_of_the_mouse() {
    for view in [(0.6, 0.42), (2.6, -0.35), (-1.9, 0.8)] {
        let mut editor = editor();
        let id = setup(&mut editor, view);
        let mut hands = Hands::new(&mut editor);
        editor.set_tool(Tool::Rotate);
        // Global: кольца — оси мира.
        editor.set_gizmo_local(false);
        let world = [Vec3::X, Vec3::Y, Vec3::Z];
        for axis in 0..3 {
            drag_ring(&mut hands, &mut editor, &[id], world, axis, 36.0);
            drag_ring(&mut hands, &mut editor, &[id], world, axis, -36.0);
        }
        // Local: кольца — оси объекта.
        editor.set_gizmo_local(true);
        let rotation = world_rotation(&editor, id);
        let local = [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z];
        for axis in 0..3 {
            drag_ring(&mut hands, &mut editor, &[id], local, axis, 36.0);
        }
    }
}

#[test]
fn rings_follow_a_rotated_parent_and_non_uniform_scale_in_local_mode() {
    let mut editor = editor();
    let child = setup(&mut editor, (0.6, 0.42));
    let scene = &mut editor.app.world.scene;
    let parent = scene.spawn("Gizmo Parent");
    {
        let p = scene.get_mut(parent).unwrap();
        p.transform.position = Vec3::new(40.0, 30.0, 0.0);
        p.transform.set_euler_deg(Vec3::new(0.0, 90.0, 30.0));
    }
    scene.set_parent(child, Some(parent)).unwrap();
    {
        let c = scene.get_mut(child).unwrap();
        c.transform.position = Vec3::ZERO;
        c.transform.scale = Vec3::new(2.0, 0.5, 1.0);
    }
    editor.apply(Action::Select(Some(child)));
    let mut hands = Hands::new(&mut editor);
    editor.set_tool(Tool::Rotate);
    editor.set_gizmo_local(true);
    let rotation = world_rotation(&editor, child);
    let local = [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z];
    for axis in 0..3 {
        drag_ring(&mut hands, &mut editor, &[child], local, axis, 30.0);
    }
    // Масштаб объекта поворот не трогает.
    assert_eq!(editor.app.world.scene.get(child).unwrap().transform.scale, Vec3::new(2.0, 0.5, 1.0));
}

#[test]
fn rings_turn_the_whole_selection_about_the_pivot_or_the_center() {
    for center in [false, true] {
        let mut editor = editor();
        let a = setup(&mut editor, (0.6, 0.42));
        let scene = &mut editor.app.world.scene;
        let b = scene.spawn("Gizmo Probe B");
        scene.get_mut(b).unwrap().transform.position = Vec3::new(42.0, 30.5, -1.0);
        editor.select_many(&[a, b], Some(a));
        editor.set_pivot_center(center);
        let mut hands = Hands::new(&mut editor);
        editor.set_tool(Tool::Rotate);
        editor.set_gizmo_local(false);
        hands.frame(&mut editor, vec![]);
        let pivot = editor.gizmo_pivot_world().unwrap();
        let start = [a, b].map(|id| editor.app.world.scene.world_position(id));
        hands.frame(&mut editor, vec![]);
        let (press, _, tangent) = ring_grab(&editor, pivot, [Vec3::X, Vec3::Y, Vec3::Z], 1);
        hands.drag(&mut editor, press, press + tangent.normalized() * 40.0);
        let turn = Quat::from_rotation_y(40.0 / tangent.length());
        for (id, start) in [a, b].into_iter().zip(start) {
            let now = editor.app.world.scene.world_position(id);
            let expected = pivot + turn * (start - pivot);
            assert!(
                now.distance(expected) < 0.3 * (start - pivot).length().max(0.05),
                "center={center}: {now} should orbit the pivot to about {expected}"
            );
            assert!((now.distance(pivot) - start.distance(pivot)).abs() < 1e-3, "the distance to the pivot is kept");
        }
        if !center {
            assert!(editor.app.world.scene.world_position(a).distance(start[0]) < 1e-4, "Pivot: the active object stays");
        }
    }
}

#[test]
fn inspector_rotation_fields_match_the_ring_that_was_dragged() {
    let mut editor = editor();
    let id = setup(&mut editor, (0.6, 0.42));
    editor.app.world.scene.get_mut(id).unwrap().transform.rotation = Quat::IDENTITY;
    let mut hands = Hands::new(&mut editor);
    editor.set_tool(Tool::Rotate);
    editor.set_gizmo_local(false);
    for axis in 0..3 {
        let angle = drag_ring(&mut hands, &mut editor, &[id], [Vec3::X, Vec3::Y, Vec3::Z], axis, 30.0);
        // drag_ring возвращает позу; повторить и прочитать поля инспектора.
        let start = poses(&editor, &[id]);
        hands.frame(&mut editor, vec![]);
        let origin = editor.gizmo_pivot_world().unwrap();
        let (press, _, tangent) = ring_grab(&editor, origin, [Vec3::X, Vec3::Y, Vec3::Z], axis);
        hands.drag(&mut editor, press, press + tangent.normalized() * 30.0);
        let euler = editor.app.world.scene.get(id).unwrap().transform.euler_deg();
        for other in 0..3 {
            if other == axis {
                assert!((euler[other] - angle.to_degrees()).abs() < 1.0, "ring {axis}: Rotation {euler} has {} in its own field", angle.to_degrees());
            } else {
                assert!(euler[other].abs() < 0.5, "ring {axis}: Rotation {euler} changed another field");
            }
        }
        restore(&mut editor, &start);
    }
}

#[test]
fn move_arrows_and_scale_handles_follow_their_axes() {
    let mut editor = editor();
    let id = setup(&mut editor, (0.6, 0.42));
    let mut hands = Hands::new(&mut editor);
    for local in [false, true] {
        editor.set_gizmo_local(local);
        let rotation = world_rotation(&editor, id);
        let axes = if local { [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z] } else { [Vec3::X, Vec3::Y, Vec3::Z] };
        editor.set_tool(Tool::Move);
        for axis in 0..3 {
            hands.frame(&mut editor, vec![]);
            let origin = editor.app.world.scene.world_position(id);
            let length = origin.distance(editor.orbit().eye()) * ARROW;
            let base = editor.world_to_screen_3d(origin).unwrap();
            let at = editor.world_to_screen_3d(origin + axes[axis] * length * 0.7).unwrap();
            let direction = (at - base).normalized();
            let start = poses(&editor, &[id]);
            hands.drag(&mut editor, at, at + direction * 50.0);
            let moved = editor.app.world.scene.world_position(id) - origin;
            assert!(moved.dot(axes[axis]) > 0.05, "local={local} arrow {axis}: moved {moved} not along {}", axes[axis]);
            assert!((moved - axes[axis] * moved.dot(axes[axis])).length() < 1e-3, "local={local} arrow {axis}: moved off the axis: {moved}");
            restore(&mut editor, &start);
        }
    }
    // Масштаб — по осям объекта: тянуть ручку X наружу — растёт только scale.x.
    editor.set_tool(Tool::Scale);
    let rotation = world_rotation(&editor, id);
    for axis in 0..3 {
        hands.frame(&mut editor, vec![]);
        let origin = editor.app.world.scene.world_position(id);
        let length = origin.distance(editor.orbit().eye()) * ARROW;
        let direction = rotation * [Vec3::X, Vec3::Y, Vec3::Z][axis];
        let base = editor.world_to_screen_3d(origin).unwrap();
        let end = editor.world_to_screen_3d(origin + direction * length).unwrap();
        let before = editor.app.world.scene.get(id).unwrap().transform.scale;
        let start = poses(&editor, &[id]);
        hands.drag(&mut editor, end, end + (end - base).normalized() * 40.0);
        let after = editor.app.world.scene.get(id).unwrap().transform.scale;
        assert!(after[axis] > before[axis] * 1.05, "scale handle {axis}: {before} → {after}");
        for other in (0..3).filter(|o| *o != axis) {
            assert!((after[other] - before[other]).abs() < 1e-4, "scale handle {axis} changed axis {other}: {before} → {after}");
        }
        restore(&mut editor, &start);
    }
}

#[test]
fn the_2d_rotate_ring_turns_with_the_mouse() {
    let mut editor = editor();
    let id = editor.app.world.scene.find_by_name("Platform A").unwrap();
    editor.apply(Action::Select(Some(id)));
    let mut hands = Hands::new(&mut editor);
    editor.set_tool(Tool::Rotate);
    hands.frame(&mut editor, vec![]);
    let center = editor.world_to_screen(editor.app.world.scene.world_position(id).truncate());
    let before = editor.app.world.scene.get(id).unwrap().transform.angle_z();
    let start = poses(&editor, &[id]);
    // Справа на кольце → вверх по экрану: против часовой, «+» вокруг Z (ось Y мира — вверх).
    hands.drag(&mut editor, center + egui::vec2(62.0, 0.0), center + egui::vec2(0.0, -62.0));
    let turned = editor.app.world.scene.get(id).unwrap().transform.angle_z() - before;
    assert!((turned.to_degrees() - 90.0).abs() < 3.0, "counter-clockwise on screen is +90°: {}", turned.to_degrees());
    restore(&mut editor, &start);
    hands.frame(&mut editor, vec![]);
    hands.drag(&mut editor, center + egui::vec2(62.0, 0.0), center + egui::vec2(0.0, 62.0));
    let turned = editor.app.world.scene.get(id).unwrap().transform.angle_z() - before;
    assert!((turned.to_degrees() + 90.0).abs() < 3.0, "clockwise is −90°: {}", turned.to_degrees());
}
