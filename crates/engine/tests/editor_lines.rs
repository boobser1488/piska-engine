//! Линии вида Scene не «выстреливают»: камера внутри выделенного объекта и позади него,
//! сплющенный и скошенный контур спрайта в 2D. Проверяется то, что egui действительно
//! нарисует: все вершины кадра конечны и лежат рядом с окном.
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Orbit3D};
use engine::prelude::*;
use engine::render::MeshShape;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let app = engine::app();
    Editor::new(app, EditorConfig::default()).unwrap()
}

/// Кадр интерфейса → вершины, которые получит видеокарта.
fn vertices(editor: &mut Editor, ctx: &egui::Context, frames: usize) -> Vec<(egui::Pos2, egui::Color32)> {
    let mut out = Vec::new();
    for frame in 0..frames {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| editor.ui(ctx));
        out.clear();
        for primitive in ctx.tessellate(full.shapes, full.pixels_per_point) {
            if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                out.extend(mesh.vertices.iter().map(|v| (v.pos, v.color)));
            }
        }
    }
    out
}

fn assert_sane(vertices: &[(egui::Pos2, egui::Color32)], what: &str) {
    let limit = egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN).expand(SCREEN.x * 10.0);
    for (v, _) in vertices {
        assert!(v.x.is_finite() && v.y.is_finite(), "{what}: a NaN vertex");
        assert!(limit.contains(*v), "{what}: a vertex shoots out to {v:?}");
    }
}

#[test]
fn outlines_stay_on_screen_with_the_camera_inside_or_behind_the_selection() {
    let mut editor = editor();
    editor.apply(Action::NewScene3D);
    editor.apply(Action::CreateMesh(MeshShape::Cube));
    let cube = editor.selection().expect("the cube is selected");
    if let Some(object) = editor.app.world.scene.get_mut(cube) {
        object.transform.position = Vec3::ZERO;
        object.transform.scale = Vec3::splat(4.0);
    }
    let ctx = egui::Context::default();
    // Камера внутри куба, смотрит в разные стороны; затем вплотную позади грани.
    for (yaw, pitch, distance) in [(0.3, 0.2, 0.5), (2.0, -0.6, 1.0), (4.0, 1.2, 0.05), (0.0, 0.0, 2.01)] {
        editor.set_orbit(Orbit3D { target: Vec3::new(0.2, 0.1, -0.3), yaw, pitch, distance });
        let vertices = vertices(&mut editor, &ctx, 3);
        assert!(!vertices.is_empty());
        assert_sane(&vertices, &format!("camera at yaw {yaw}, pitch {pitch}, distance {distance}"));
    }
}

#[test]
fn a_skewed_or_flattened_sprite_outline_has_no_miter_spikes() {
    let mut editor = editor();
    editor.apply(Action::NewScene);
    // Родитель с сильно неравным масштабом и повёрнутый ребёнок — контур-параллелограмм с
    // очень острыми углами; второй спрайт сплющен в линию.
    let scene = &mut editor.app.world.scene;
    let parent = scene.build("Squashed").scale(8.0, 0.05, 1.0).id();
    let child = scene
        .build("Skewed")
        .rotation(Quat::from_rotation_z(0.7))
        .with(engine::render::SpriteRenderer::default())
        .child_of(parent)
        .id();
    let flat = scene.build("Flat").at(0.0, -3.0).scale(2.0, 0.0, 1.0).with(engine::render::SpriteRenderer::default()).id();
    editor.apply(Action::SelectMany { ids: vec![child, flat], active: Some(flat) });
    let ctx = egui::Context::default();
    let drawn = vertices(&mut editor, &ctx, 3);
    assert_sane(&drawn, "2D outlines");
    // Контур (неактивного) скошенного спрайта не выходит за его углы.
    let matrix = editor.app.world.scene.world_matrix(child);
    let corners: Vec<egui::Pos2> = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
        .iter()
        .map(|(x, y)| editor.world_to_screen(matrix.transform_point3(Vec3::new(*x, *y, 0.0)).truncate()))
        .collect();
    let bbox = egui::Rect::from_points(&corners).expand(4.0);
    let theme = |c| engine::editor::kit::theme::mix(engine::editor::kit::theme::accent(), egui::Color32::WHITE, c);
    let outline = theme(0.35);
    let outline_vertices: Vec<egui::Pos2> = drawn.iter().filter(|(_, c)| *c == outline).map(|(p, _)| *p).collect();
    assert!(!outline_vertices.is_empty(), "the outline is drawn");
    for v in outline_vertices {
        assert!(bbox.contains(v), "the outline sticks out to {v:?}, the sprite is {bbox:?}");
    }
}
