//! Поле-ссылка на объект в инспекторе — настоящим вводом egui, без GPU: перетаскивание
//! строки из Hierarchy, список выбора с фильтром по компоненту, щелчок (подсветка),
//! двойной щелчок (выделение), Delete, Undo, мультивыделение, переименование цели и
//! сохранение сцены.
#![cfg(feature = "editor")]

use std::path::PathBuf;

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig};
use engine::prelude::*;

component! {
    pub struct Hp {
        pub value: i32 = 3,
    }
}
impl Component for Hp {}

component! {
    /// Компонент со ссылками: любой объект и объект с `Hp`.
    pub struct Linker {
        pub target: ObjectRef = ObjectRef::NONE,
        pub guard: ComponentRef<Hp> = ComponentRef::NONE,
    }
}
impl Component for Linker {}

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const NONE: egui::Modifiers = egui::Modifiers::NONE;

const SCENE: &str = r#"(
    name: "Refs",
    objects: [
        (name: "Lever", id: 11),
        (name: "Lamp", id: 12, components: [(kind: "Hp")]),
        (name: "Door", id: 13, components: [(kind: "Linker")]),
        (name: "Gate", id: 14, components: [(kind: "Linker", fields: { "target": Object(12) })]),
    ],
)"#;

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_editor_refs_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(dir.join("scenes/refs.scene"), SCENE).unwrap();
    dir
}

fn editor(dir: &PathBuf) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    app.register::<Hp>().register::<Linker>();
    engine::mount_assets(&mut app, dir).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/refs.scene")).unwrap()
}

fn find(editor: &Editor, name: &str) -> GameObjectId {
    editor.app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("`{name}` exists"))
}

fn linker(editor: &Editor, name: &str) -> Linker {
    editor.app.world.scene.get_component::<Linker>(find(editor, name)).unwrap().clone()
}

fn uid(editor: &Editor, name: &str) -> u64 {
    editor.app.world.scene.get(find(editor, name)).unwrap().uid()
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
        let _ = self.ctx.run(input, |ctx| editor.ui(ctx));
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: NONE }
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::button(pos, true)]);
        self.frame(editor, vec![Self::button(pos, false)]);
        self.frame(editor, vec![]);
    }

    fn double_click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::button(pos, true), Self::button(pos, false)]);
        self.frame(editor, vec![Self::button(pos, true), Self::button(pos, false)]);
        self.frame(editor, vec![]);
    }

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![Self::button(from, true)]);
        for step in 1..=10 {
            let pos = from + (to - from) * (step as f32 / 10.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        self.frame(editor, vec![Self::button(to, false)]);
        self.frame(editor, vec![]);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: NONE };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.frame(editor, vec![]);
    }

    fn row(&mut self, editor: &mut Editor, name: &str) -> egui::Pos2 {
        self.frame(editor, vec![]);
        self.ctx.read_response(Editor::hierarchy_row_id(find(editor, name))).expect("the row is drawn").rect.center()
    }

    fn field(&mut self, editor: &mut Editor, field: &str) -> egui::Rect {
        self.frame(editor, vec![]);
        editor
            .inspector_field_rects()
            .into_iter()
            .find(|((kind, name), _)| *kind == "Linker" && *name == field)
            .unwrap_or_else(|| panic!("field `{field}` is drawn"))
            .1
    }

    fn rect(&self, id: egui::Id) -> Option<egui::Rect> {
        self.ctx.read_response(id).map(|r| r.rect)
    }
}

fn select(editor: &mut Editor, hands: &mut Hands, name: &str) {
    let id = find(editor, name);
    editor.apply(Action::Select(Some(id)));
    hands.frame(editor, vec![]);
}

#[test]
fn drag_pick_ping_clear_and_undo() {
    let dir = project("single");
    let mut editor = editor(&dir);
    let mut hands = Hands::new(&mut editor);
    select(&mut editor, &mut hands, "Door");

    // Строку Lever из Hierarchy — на поле `target`.
    let from = hands.row(&mut editor, "Lever");
    let to = hands.field(&mut editor, "target").center();
    hands.drag(&mut editor, from, to);
    assert_eq!(linker(&editor, "Door").target.uid(), uid(&editor, "Lever"));
    assert_eq!(editor.selection(), Some(find(&editor, "Door")), "dropping does not change the selection");

    // Щелчок — подсветка в Hierarchy, двойной щелчок — выделение цели.
    let field = hands.field(&mut editor, "target").left_center() + egui::vec2(30.0, 0.0);
    hands.click(&mut editor, field);
    assert_eq!(editor.pinged_object(&hands.ctx), Some(find(&editor, "Lever")));
    hands.double_click(&mut editor, field);
    assert_eq!(editor.selection(), Some(find(&editor, "Lever")));
    select(&mut editor, &mut hands, "Door");

    // Щелчок по полю, Delete — очистить поле (объект Door не удаляется); Undo — вернуть.
    let field = hands.field(&mut editor, "target").left_center() + egui::vec2(30.0, 0.0);
    hands.click(&mut editor, field);
    hands.key(&mut editor, egui::Key::Delete);
    assert!(linker(&editor, "Door").target.is_none());
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(egui::pos2(5.0, 5.0))]);
    hands.click(&mut editor, egui::pos2(SCREEN.x - 5.0, SCREEN.y - 5.0));
    editor.apply(Action::Undo);
    hands.frame(&mut editor, vec![]);
    assert_eq!(linker(&editor, "Door").target.uid(), uid(&editor, "Lever"));
    editor.apply(Action::Redo);
    assert!(linker(&editor, "Door").target.is_none());

    // Список выбора поля `guard: ComponentRef<Hp>` — только объекты с Hp.
    select(&mut editor, &mut hands, "Door");
    let picker = hands.rect(Editor::object_field_picker_id("Linker", 0, "guard")).expect("the picker button is drawn");
    hands.click(&mut editor, picker.center());
    let lamp_option = Editor::object_field_option_id("Linker", 0, "guard", uid(&editor, "Lamp"));
    let lever_option = Editor::object_field_option_id("Linker", 0, "guard", uid(&editor, "Lever"));
    let lamp = hands.rect(lamp_option).expect("Lamp has Hp: it is offered");
    assert!(hands.rect(lever_option).is_none(), "Lever has no Hp: it is not offered");
    hands.click(&mut editor, lamp.center());
    assert_eq!(linker(&editor, "Door").guard.object.uid(), uid(&editor, "Lamp"));

    // Перетаскивание объекта без Hp на типизированное поле ничего не меняет.
    let from = hands.row(&mut editor, "Lever");
    let to = hands.field(&mut editor, "guard").center();
    hands.drag(&mut editor, from, to);
    assert_eq!(linker(&editor, "Door").guard.object.uid(), uid(&editor, "Lamp"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_selection_shows_mixed_and_assigns_all() {
    let dir = project("multi");
    let mut editor = editor(&dir);
    let mut hands = Hands::new(&mut editor);
    let door = find(&editor, "Door");
    let gate = find(&editor, "Gate");
    editor.select_many(&[door, gate], Some(gate));
    hands.frame(&mut editor, vec![]);
    // Поле у Door пустое, у Gate — Lamp: значения разные; бросаем Lever — обоим.
    let field = hands
        .rect(egui::Id::new(("object_field", ("multi", "Linker", "target"))))
        .expect("the mixed reference field is drawn");
    let from = hands.row(&mut editor, "Lever");
    hands.drag(&mut editor, from, field.center());
    let lever = uid(&editor, "Lever");
    assert_eq!(linker(&editor, "Door").target.uid(), lever);
    assert_eq!(linker(&editor, "Gate").target.uid(), lever);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn renaming_the_target_and_saving_keeps_the_reference() {
    let dir = project("rename");
    let mut editor = editor(&dir);
    let mut hands = Hands::new(&mut editor);
    let lamp = find(&editor, "Lamp");
    editor.apply(Action::Rename { id: lamp, name: "Ceiling Lamp".into() });
    // Переставить объекты: Gate — первым.
    let gate = find(&editor, "Gate");
    editor.app.world.scene.set_sibling_index(gate, 0);
    hands.frame(&mut editor, vec![]);
    editor.apply(Action::Save);
    let text = std::fs::read_to_string(dir.join("scenes/refs.scene")).unwrap();
    assert!(text.contains("Ceiling Lamp"), "{text}");

    let mut app = engine::app();
    app.register::<Hp>().register::<Linker>();
    app.world.load_scene_str(&text, "refs.scene").unwrap();
    let gate = app.world.scene.find_by_name("Gate").unwrap();
    let target = app.world.scene.get_component::<Linker>(gate).unwrap().target;
    let found = app.world.scene.resolve(target).unwrap();
    assert_eq!(app.world.scene.get(found).unwrap().name, "Ceiling Lamp");
    let _ = std::fs::remove_dir_all(&dir);
}
