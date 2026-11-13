//! Рельеф в редакторе: создание из меню, мазок кистью в 3D-виде Scene поднимает землю
//! только под кистью и отменяется одним Undo; снимок редактора с кистью над
//! вылепленным холмом (если задан `TERRAIN_SHOTS`).
#![cfg(feature = "editor")]

use engine::editor::{egui, Action, Editor, EditorConfig, Orbit3D};
use engine::prelude::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);
const DT: f32 = 1.0 / 60.0;

fn frame(editor: &mut Editor, ctx: &egui::Context, time: &mut f64, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        time: Some(*time),
        predicted_dt: DT,
        events,
        ..egui::RawInput::default()
    };
    *time += DT as f64;
    let _ = ctx.run(input, |ctx| editor.ui(ctx));
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

fn editor_with_terrain() -> (Editor, GameObjectId) {
    let app = engine::app();
    let mut editor = Editor::new(app, EditorConfig::titled("Terrain test")).unwrap();
    let id = editor.create_terrain();
    editor.set_scene_3d(true);
    let centre = Vec3::new(0.0, terrain_height(&editor.app.world, 0.0, 0.0).unwrap(), 0.0);
    editor.set_orbit(Orbit3D { target: centre, yaw: 0.6, pitch: 0.7, distance: 120.0 });
    (editor, id)
}

#[test]
fn a_brush_stroke_raises_the_ground_and_undo_restores_it() {
    let (mut editor, id) = editor_with_terrain();
    let (ctx, mut time) = (egui::Context::default(), 0.0);
    for _ in 0..3 {
        frame(&mut editor, &ctx, &mut time, vec![]);
    }
    editor.set_terrain_tool(Some(BrushTool::Raise));
    editor.set_terrain_brush(TerrainBrush { radius: 10.0, strength: 1.0, falloff: 0.5, ..TerrainBrush::default() });
    let target = editor.orbit().target;
    let pos = editor.world_to_screen_3d(target).expect("the centre is on screen");
    let before = terrain_height(&editor.app.world, target.x, target.z).unwrap();
    let far_before = terrain_height(&editor.app.world, target.x + 60.0, target.z).unwrap();

    frame(&mut editor, &ctx, &mut time, vec![egui::Event::PointerMoved(pos)]);
    let hover = editor.terrain_brush_point().expect("the brush is over the terrain");
    assert!(hover.distance(target) < 3.0, "brush point {hover} vs {target}");
    frame(&mut editor, &ctx, &mut time, vec![button(pos, true)]);
    for _ in 0..30 {
        frame(&mut editor, &ctx, &mut time, vec![]);
    }
    frame(&mut editor, &ctx, &mut time, vec![button(pos, false)]);
    frame(&mut editor, &ctx, &mut time, vec![]);

    let after = terrain_height(&editor.app.world, target.x, target.z).unwrap();
    assert!(after - before > 2.0, "the stroke raised the ground: {before} → {after}");
    let far_after = terrain_height(&editor.app.world, target.x + 60.0, target.z).unwrap();
    assert_eq!(far_before.to_bits(), far_after.to_bits(), "nothing changes outside the brush");
    assert_eq!(editor.selection(), Some(id), "the brush does not change the selection");

    editor.apply(Action::Undo);
    let undone = terrain_height(&editor.app.world, target.x, target.z).unwrap();
    assert!((undone - before).abs() < 1e-3, "one Undo removes the whole stroke: {undone} vs {before}");
    let terrain = editor.app.world.scene.find_by_name("Terrain").unwrap();
    assert!(editor.app.world.scene.get_component::<Terrain>(terrain).unwrap().edits.is_empty());
}

/// Снимок: вылепленный холм и круг кисти. Нужен GPU; файл — только по `TERRAIN_SHOTS`.
#[test]
fn editor_screenshot_with_brush_and_sculpted_hill() {
    let Ok(dir) = std::env::var("TERRAIN_SHOTS") else { return };
    let (mut editor, id) = editor_with_terrain();
    let target = editor.orbit().target;
    if let Some(terrain) = editor.app.world.scene.get_component_mut::<Terrain>(id) {
        let brush = TerrainBrush { tool: BrushTool::Raise, radius: 18.0, strength: 1.0, falloff: 0.8, ..TerrainBrush::default() };
        for _ in 0..40 {
            terrain.apply_brush(&brush, 256.0 - 20.0, 256.0, 0.05, None);
        }
        let paint = TerrainBrush { tool: BrushTool::Paint, radius: 10.0, layer: Some(2), ..brush };
        terrain.apply_brush(&paint, 256.0 + 25.0, 256.0 + 10.0, 1.0, None);
    }
    editor.set_terrain_tool(Some(BrushTool::Raise));
    editor.set_terrain_brush(TerrainBrush { radius: 14.0, ..TerrainBrush::default() });
    let image = engine::editor::capture_with_input(&mut editor, 1280, 800, 6, |frame, editor| {
        if frame < 2 {
            return vec![];
        }
        let p = editor.world_to_screen_3d(target + Vec3::new(18.0, 0.0, 12.0)).unwrap_or(egui::pos2(640.0, 400.0));
        vec![egui::Event::PointerMoved(p)]
    })
    .unwrap();
    image.save(format!("{dir}/editor_brush.png")).unwrap();
}
