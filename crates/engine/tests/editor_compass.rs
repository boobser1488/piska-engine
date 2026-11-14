//! Значок ориентации в 3D-виде Scene нажимается: щелчок по концу оси ставит камеру с этой
//! стороны, за сам значок вид можно вращать, а объекты под ним при этом не выбираются.
//! Окно и GPU не нужны: кадры интерфейса строятся в контексте egui без отрисовки.
#![cfg(feature = "editor")]

use std::f32::consts::{FRAC_PI_2, PI};

use engine::editor::{egui, Editor, EditorConfig};
use engine::prelude::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);
const DT: f32 = 1.0 / 60.0;

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0 };
        hands.frames(editor, 2);
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
        let _ = self.ctx.run(input, |ctx| editor.ui(ctx));
    }

    fn frames(&mut self, editor: &mut Editor, count: usize) {
        for _ in 0..count {
            self.frame(editor, vec![]);
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::button(pos, true)]);
        self.frame(editor, vec![Self::button(pos, false)]);
        // Плавный доворот камеры занимает меньше секунды.
        self.frames(editor, 90);
    }

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![Self::button(from, true)]);
        for step in 1..=8 {
            let pos = from + (to - from) * (step as f32 / 8.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        self.frame(editor, vec![Self::button(to, false)]);
        self.frame(editor, vec![]);
    }
}

/// Сцена, заставленная кубами: под значком ориентации всегда есть что выбрать.
fn editor() -> Editor {
    let mut app = engine::app();
    app.spawn("Camera").at3(5.0, 4.0, 8.0).with(Camera::perspective(50.0));
    for (index, (x, z)) in [(0.0, 0.0), (6.0, -6.0), (-6.0, -6.0), (6.0, 6.0), (-6.0, 6.0)].into_iter().enumerate() {
        app.spawn(format!("Cube {index}"))
            .at3(x, 3.0, z)
            .scale(8.0, 8.0, 8.0)
            .with(MeshRenderer::cube());
    }
    Editor::new(app, EditorConfig::default()).expect("the editor starts")
}

fn handle(editor: &Editor, direction: Vec3) -> egui::Pos2 {
    editor
        .scene_compass_handle(direction)
        .expect("the compass has a handle for every axis direction")
}

/// Разница углов с учётом оборота.
fn angle_between(a: f32, b: f32) -> f32 {
    let diff = (a - b).rem_euclid(2.0 * PI);
    diff.min(2.0 * PI - diff)
}

#[test]
fn clicking_a_compass_axis_turns_the_scene_camera_to_that_side() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    assert!(editor.is_scene_3d());
    let start = editor.orbit();

    // +X: камера встаёт справа от цели и смотрит вдоль −X, без наклона.
    let at = handle(&editor, Vec3::X);
    hands.click(&mut editor, at);
    let orbit = editor.orbit();
    assert!(angle_between(orbit.yaw, FRAC_PI_2) < 0.01, "yaw {}", orbit.yaw);
    assert!(orbit.pitch.abs() < 0.01, "pitch {}", orbit.pitch);
    let side = (orbit.eye() - orbit.target).normalize();
    assert!(side.distance(Vec3::X) < 0.02, "the eye is on the +X side: {side:?}");
    assert_eq!(orbit.target, start.target, "the look-at point stays");
    assert_eq!(orbit.distance, start.distance, "and so does the distance");

    // −Z: с обратной стороны.
    let at = handle(&editor, Vec3::NEG_Z);
    hands.click(&mut editor, at);
    let orbit = editor.orbit();
    assert!(angle_between(orbit.yaw, PI) < 0.01, "yaw {}", orbit.yaw);
    assert!(orbit.pitch.abs() < 0.01);

    // +Y: вид сверху, поворот вокруг вертикали сохраняется.
    let at = handle(&editor, Vec3::Y);
    hands.click(&mut editor, at);
    let orbit = editor.orbit();
    assert!(orbit.pitch > 1.49, "top view: pitch {}", orbit.pitch);
    assert!(angle_between(orbit.yaw, PI) < 0.01, "yaw is kept: {}", orbit.yaw);

    // −Y: снизу. При виде ровно сверху нижний конец оси спрятан за верхним, поэтому
    // сначала — вид сбоку.
    let at = handle(&editor, Vec3::X);
    hands.click(&mut editor, at);
    let at = handle(&editor, Vec3::NEG_Y);
    hands.click(&mut editor, at);
    assert!(editor.orbit().pitch < -1.49, "bottom view: pitch {}", editor.orbit().pitch);
}

#[test]
fn the_compass_swallows_clicks_and_drags_instead_of_the_scene() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);

    // Щелчок по оси значка не выбирает куб, который под ним виден.
    let at = handle(&editor, Vec3::Z);
    hands.click(&mut editor, at);
    assert_eq!(editor.selection(), None, "a compass click is not an object pick");

    // Тот же щелчок рядом со значком, но мимо него, куб выбирает.
    let beside = handle(&editor, Vec3::Y) + egui::vec2(-140.0, 120.0);
    hands.click(&mut editor, beside);
    assert!(editor.selection().is_some(), "a click on the scene still picks");

    // Потянуть за значок — вращать вид: точка взгляда на месте, углы меняются.
    let before = editor.orbit();
    let grab = handle(&editor, Vec3::Y) + egui::vec2(0.0, 14.0);
    hands.drag(&mut editor, grab, grab + egui::vec2(-60.0, 30.0));
    let after = editor.orbit();
    assert_eq!(after.target, before.target, "dragging the compass orbits, it does not pan");
    assert!(angle_between(after.yaw, before.yaw) > 0.2, "yaw {} -> {}", before.yaw, after.yaw);
    assert!((after.pitch - before.pitch).abs() > 0.1, "pitch {} -> {}", before.pitch, after.pitch);

    // А средняя кнопка по самой сцене сдвигает точку взгляда (левая там — рамка выделения).
    let scene_point = beside + egui::vec2(-200.0, 60.0);
    let middle = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(scene_point)]);
    hands.frame(&mut editor, vec![middle(scene_point, true)]);
    for step in 1..=8 {
        let pos = scene_point + egui::vec2(10.0 * step as f32, 0.0);
        hands.frame(&mut editor, vec![egui::Event::PointerMoved(pos)]);
    }
    hands.frame(&mut editor, vec![middle(scene_point + egui::vec2(80.0, 0.0), false)]);
    assert_ne!(editor.orbit().target, after.target, "dragging the scene with the middle button pans");
}
