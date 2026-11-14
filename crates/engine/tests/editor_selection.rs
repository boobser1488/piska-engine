//! Выделение нескольких объектов и сочетания клавиш — настоящим вводом egui, без GPU:
//! Ctrl / Shift в Hierarchy и в виде Scene, рамка, перемещение группы одним шагом Undo,
//! копирование / вставка, группировка, Ctrl+A, полёт по виду Scene на правой кнопке.
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Tool};
use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap()
}

fn find(editor: &Editor, name: &str) -> GameObjectId {
    editor.app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("`{name}` exists"))
}

const CTRL: egui::Modifiers = egui::Modifiers::COMMAND;
const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;
const NONE: egui::Modifiers = egui::Modifiers::NONE;

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0 };
        for _ in 0..3 {
            hands.frame(editor, vec![], NONE);
        }
        hands
    }

    fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: DT,
            modifiers,
            events,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        editor.forward_game_input(&self.ctx, &input);
        let _ = self.ctx.run(input, |ctx| editor.ui(ctx));
    }

    fn rect(&self, id: egui::Id) -> egui::Rect {
        self.ctx.read_response(id).expect("the widget is drawn").rect
    }

    fn button(pos: egui::Pos2, button: egui::PointerButton, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers }
    }

    fn click_with(&mut self, editor: &mut Editor, pos: egui::Pos2, modifiers: egui::Modifiers) {
        let b = egui::PointerButton::Primary;
        self.frame(editor, vec![egui::Event::PointerMoved(pos)], modifiers);
        self.frame(editor, vec![Self::button(pos, b, true, modifiers)], modifiers);
        self.frame(editor, vec![Self::button(pos, b, false, modifiers)], modifiers);
        self.frame(editor, vec![], NONE);
    }

    fn drag_with(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2, button: egui::PointerButton, modifiers: egui::Modifiers) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)], modifiers);
        self.frame(editor, vec![Self::button(from, button, true, modifiers)], modifiers);
        for step in 1..=8 {
            let pos = from + (to - from) * (step as f32 / 8.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)], modifiers);
        }
        self.frame(editor, vec![Self::button(to, button, false, modifiers)], modifiers);
        self.frame(editor, vec![], NONE);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        self.frame(editor, vec![event(true)], modifiers);
        self.frame(editor, vec![event(false)], modifiers);
        self.frame(editor, vec![], NONE);
    }

    fn row(&mut self, editor: &mut Editor, name: &str) -> egui::Pos2 {
        self.frame(editor, vec![], NONE);
        self.rect(Editor::hierarchy_row_id(find(editor, name))).center()
    }
}

fn names(editor: &Editor) -> Vec<String> {
    let mut names: Vec<String> = editor
        .selected()
        .into_iter()
        .filter_map(|id| editor.app.world.scene.get(id).map(|o| o.name.clone()))
        .collect();
    names.sort();
    names
}

#[test]
fn ctrl_click_toggles_and_shift_click_selects_a_range_in_the_hierarchy() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let a = hands.row(&mut editor, "Platform A");
    hands.click_with(&mut editor, a, NONE);
    let c = hands.row(&mut editor, "Platform C");
    hands.click_with(&mut editor, c, CTRL);
    assert_eq!(names(&editor), ["Platform A", "Platform C"]);
    assert_eq!(editor.selection(), Some(find(&editor, "Platform C")), "the last clicked is active");
    // Ctrl+щелчок по выделенному — убрать.
    hands.click_with(&mut editor, a, CTRL);
    assert_eq!(names(&editor), ["Platform C"]);

    // Shift — диапазон от активного.
    hands.click_with(&mut editor, a, NONE);
    let c = hands.row(&mut editor, "Platform C");
    hands.click_with(&mut editor, c, SHIFT);
    assert_eq!(names(&editor), ["Platform A", "Platform B", "Platform C"]);

    // Простой щелчок — снова один объект; Esc — ничего.
    let b = hands.row(&mut editor, "Platform B");
    hands.click_with(&mut editor, b, NONE);
    assert_eq!(names(&editor), ["Platform B"]);
    hands.key(&mut editor, egui::Key::Escape, NONE);
    assert!(editor.selected().is_empty());

    // Ctrl+A — все объекты сцены.
    hands.key(&mut editor, egui::Key::A, CTRL);
    assert_eq!(editor.selected().len(), editor.app.world.scene.len());
}

#[test]
fn the_type_filter_finds_objects_by_component() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    // В поиске Hierarchy `t:Camera` оставляет объекты с камерой.
    editor.apply(Action::Select(None));
    let _ = &mut hands;
    let scene = &editor.app.world.scene;
    let cameras = scene.ids().into_iter().filter(|id| scene.get_component::<Camera>(*id).is_some()).count();
    assert!(cameras >= 1);
    editor.set_hierarchy_filter("t:Camera");
    hands.frame(&mut editor, vec![], NONE);
    hands.frame(&mut editor, vec![], NONE);
    let camera = find(&editor, "Main Camera");
    assert!(hands.ctx.read_response(Editor::hierarchy_row_id(camera)).is_some(), "the camera row is shown");
    let platform = find(&editor, "Platform A");
    assert!(hands.ctx.read_response(Editor::hierarchy_row_id(platform)).is_none(), "a platform has no camera");
    // Слово и тип вместе: и имя, и компонент.
    editor.set_hierarchy_filter("platform t:SpriteRenderer");
    hands.frame(&mut editor, vec![], NONE);
    hands.frame(&mut editor, vec![], NONE);
    assert!(hands.ctx.read_response(Editor::hierarchy_row_id(platform)).is_some());
    assert!(hands.ctx.read_response(Editor::hierarchy_row_id(camera)).is_none());
}

#[test]
fn scene_view_ctrl_click_toggles_and_a_box_selects() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = find(&editor, "Player");
    let coin = find(&editor, "Platform A");
    let at = |editor: &Editor, id| editor.world_to_screen(editor.app.world.scene.world_position(id).truncate());

    let p = at(&editor, player);
    hands.click_with(&mut editor, p, NONE);
    assert_eq!(editor.selection(), Some(player));
    let c = at(&editor, coin);
    hands.click_with(&mut editor, c, CTRL);
    assert!(editor.is_selected(player) && editor.is_selected(coin), "{:?}", names(&editor));
    let c = at(&editor, coin);
    hands.click_with(&mut editor, c, CTRL);
    assert!(!editor.is_selected(coin));

    // Рамка вокруг всего вида Scene выделяет много объектов.
    editor.apply(Action::Select(None));
    let view = editor.viewport_rect();
    hands.drag_with(&mut editor, view.min + egui::vec2(4.0, 4.0), view.max - egui::vec2(4.0, 30.0), egui::PointerButton::Primary, NONE);
    assert!(editor.selected().len() >= 3, "box select picked {:?}", names(&editor));
}

#[test]
fn the_move_gizmo_moves_the_whole_selection_with_one_undo() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let a = find(&editor, "Platform A");
    let b = find(&editor, "Platform B");
    editor.select_many(&[a, b], Some(a));
    editor.set_tool(Tool::Move);
    hands.frame(&mut editor, vec![], NONE);
    let before_a = editor.app.world.scene.world_position(a);
    let before_b = editor.app.world.scene.world_position(b);
    // Центральный квадрат гизмо — в точке активного объекта (режим Pivot).
    let handle = editor.world_to_screen(before_a.truncate());
    hands.drag_with(&mut editor, handle, handle + egui::vec2(80.0, 0.0), egui::PointerButton::Primary, NONE);
    let moved_a = editor.app.world.scene.world_position(a) - before_a;
    let moved_b = editor.app.world.scene.world_position(b) - before_b;
    assert!(moved_a.x > 0.5, "the active object moved: {moved_a}");
    assert!((moved_a - moved_b).length() < 1e-3, "both moved by the same offset: {moved_a} vs {moved_b}");

    editor.apply(Action::Undo);
    let a = find(&editor, "Platform A");
    let b = find(&editor, "Platform B");
    assert!((editor.app.world.scene.world_position(a) - before_a).length() < 1e-4);
    assert!((editor.app.world.scene.world_position(b) - before_b).length() < 1e-4);
    assert_eq!(names(&editor), ["Platform A", "Platform B"], "undo keeps the selection");
}

#[test]
fn copy_paste_duplicate_delete_and_group_work_on_the_whole_selection() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let count = editor.app.world.scene.len();
    let a = find(&editor, "Platform A");
    let b = find(&editor, "Platform B");
    editor.select_many(&[a, b], Some(b));

    hands.key(&mut editor, egui::Key::C, CTRL);
    assert_eq!(editor.object_clipboard_len(), 2);
    hands.key(&mut editor, egui::Key::V, CTRL);
    assert_eq!(editor.app.world.scene.len(), count + 2);
    assert_eq!(editor.selected().len(), 2, "the pasted copies are selected");

    hands.key(&mut editor, egui::Key::D, CTRL);
    assert_eq!(editor.app.world.scene.len(), count + 4);
    hands.key(&mut editor, egui::Key::Delete, NONE);
    assert_eq!(editor.app.world.scene.len(), count + 2);

    // Ctrl+G: новый родитель, оба внутри, мировые позиции на месте.
    let a = find(&editor, "Platform A");
    let b = find(&editor, "Platform B");
    let (pa, pb) = (editor.app.world.scene.world_position(a), editor.app.world.scene.world_position(b));
    editor.select_many(&[a, b], Some(a));
    hands.key(&mut editor, egui::Key::G, CTRL);
    let group = editor.selection().expect("the group is selected");
    let scene = &editor.app.world.scene;
    assert_eq!(scene.parent_of(a), Some(group));
    assert_eq!(scene.parent_of(b), Some(group));
    assert!((scene.world_position(a) - pa).length() < 1e-4 && (scene.world_position(b) - pb).length() < 1e-4);

    // H — выключить выделение, Shift+H — включить.
    editor.select_many(&[a, b], Some(a));
    hands.key(&mut editor, egui::Key::H, NONE);
    assert!(!editor.app.world.scene.get(a).unwrap().active && !editor.app.world.scene.get(b).unwrap().active);
    hands.key(&mut editor, egui::Key::H, SHIFT);
    assert!(editor.app.world.scene.get(a).unwrap().active);

    // F — кадрирует всё выделение (камера 2D смотрит на середину).
    hands.key(&mut editor, egui::Key::F, NONE);
}

#[test]
fn right_mouse_and_w_flies_forward_while_w_alone_picks_the_move_tool() {
    let mut editor = editor();
    editor.set_scene_3d(true);
    let mut hands = Hands::new(&mut editor);
    editor.set_tool(Tool::Rotate);
    let view = editor.viewport_rect();
    let center = view.center();
    let start = editor.orbit();
    let forward = (start.target - start.eye()).normalize();

    // ПКМ зажата над видом, W держат 30 кадров.
    let rmb = egui::PointerButton::Secondary;
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(center)], NONE);
    hands.frame(&mut editor, vec![Hands::button(center, rmb, true, NONE)], NONE);
    let w = |pressed| egui::Event::Key { key: egui::Key::W, physical_key: None, pressed, repeat: false, modifiers: NONE };
    hands.frame(&mut editor, vec![w(true)], NONE);
    for _ in 0..30 {
        hands.frame(&mut editor, vec![], NONE);
    }
    hands.frame(&mut editor, vec![w(false)], NONE);
    hands.frame(&mut editor, vec![Hands::button(center, rmb, false, NONE)], NONE);
    let moved = editor.orbit().target - start.target;
    assert!(moved.dot(forward) > 0.5, "the camera flew forward: {moved}");
    assert_eq!(editor.tool(), Tool::Rotate, "W while flying is not the Move tool");

    // Без правой кнопки W — инструмент Move.
    hands.key(&mut editor, egui::Key::W, NONE);
    assert_eq!(editor.tool(), Tool::Move);
}
