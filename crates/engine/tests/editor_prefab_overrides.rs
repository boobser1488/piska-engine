//! Переопределения экземпляра префаба в инспекторе: отметка у переопределённого поля
//! ребёнка экземпляра, меню «Revert» по правому щелчку, сохранение переопределений
//! детей (активность, поле, добавленный компонент) — настоящим вводом egui, без GPU.
#![cfg(feature = "editor")]

use std::path::PathBuf;

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig};
use engine::prelude::*;

component! {
    pub struct Glow {
        pub power: f32 = 1.0,
    }
}
impl Component for Glow {}

component! {
    pub struct Tag2 {
        pub note: String = String::new(),
    }
}
impl Component for Tag2 {}

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

const PREFAB: &str = r#"(
    name: "Lamp",
    children: [
        (name: "Bulb", components: [(kind: "Glow", fields: { "power": Float(2.0) })]),
        (name: "Base"),
    ],
)"#;

const SCENE: &str = r#"(name: "S", objects: [(name: "Lamp A", id: 7, prefab: Some("prefabs/lamp.prefab"))])"#;

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_prefab_overrides_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(dir.join("prefabs/lamp.prefab"), PREFAB).unwrap();
    std::fs::write(dir.join("scenes/s.scene"), SCENE).unwrap();
    dir
}

fn editor(dir: &PathBuf) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    app.register::<Glow>().register::<Tag2>();
    engine::mount_assets(&mut app, dir).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/s.scene")).unwrap()
}

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
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

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2, button: egui::PointerButton) {
        let event = |pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE };
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.frame(editor, vec![]);
    }
}

fn bulb(editor: &Editor) -> GameObjectId {
    let lamp = editor.app.world.scene.find_by_name("Lamp A").unwrap();
    let children = editor.app.world.scene.get(lamp).unwrap().children().to_vec();
    children.into_iter().find(|c| editor.app.world.scene.get(*c).is_some_and(|o| o.name.starts_with("Bulb"))).unwrap()
}

#[test]
fn overridden_child_fields_are_marked_and_revert_from_the_context_menu() {
    let dir = project("revert");
    let mut editor = editor(&dir);
    let mut hands = Hands { ctx: egui::Context::default(), time: 0.0 };
    let id = bulb(&editor);
    editor.apply(Action::Select(Some(id)));
    for _ in 0..3 {
        hands.frame(&mut editor, vec![]);
    }
    assert!(!editor.is_field_overridden("Glow", "power"));
    assert!(hands.ctx.read_response(Editor::prefab_override_row_id("Glow", "power")).is_none(), "not marked yet");

    editor.app.world.scene.get_component_mut::<Glow>(id).unwrap().power = 5.0;
    // Действие редактора, чтобы правка попала в историю Undo (прямое изменение сцены — нет).
    editor.apply(Action::Rename { id, name: "Bulb Light".into() });
    // Кеш узла префаба живёт секунду — пропустим её.
    for _ in 0..70 {
        hands.frame(&mut editor, vec![]);
    }
    assert!(editor.is_field_overridden("Glow", "power"));
    let row = hands.ctx.read_response(Editor::prefab_override_row_id("Glow", "power")).expect("the overridden row is marked").rect;

    // Правый щелчок по строке — меню; первый пункт — Revert.
    hands.click(&mut editor, row.center(), egui::PointerButton::Secondary);
    let revert = egui::pos2(row.center().x + 30.0, row.center().y + 14.0);
    hands.click(&mut editor, revert, egui::PointerButton::Primary);
    assert_eq!(editor.app.world.scene.get_component::<Glow>(id).unwrap().power, 2.0, "reverted to the prefab value");
    assert!(!editor.is_field_overridden("Glow", "power"));
    // Revert — обычная правка: Undo возвращает переопределение.
    editor.apply(Action::Undo);
    assert_eq!(editor.app.world.scene.get_component::<Glow>(bulb(&editor)).unwrap().power, 5.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn child_overrides_made_in_the_editor_are_saved_and_reloaded() {
    let dir = project("save");
    let mut editor = editor(&dir);
    let id = bulb(&editor);
    let lamp = editor.app.world.scene.find_by_name("Lamp A").unwrap();
    let base = editor.app.world.scene.find_child(lamp, "Base").unwrap();
    editor.apply(Action::Select(Some(base)));
    editor.apply(Action::SetSelectionActive(false));
    editor.app.world.scene.get_component_mut::<Glow>(id).unwrap().power = 9.0;
    editor.app.world.scene.get_mut(base).unwrap().add(Tag2 { note: "added".into() });
    editor.apply(Action::Save);
    let text = std::fs::read_to_string(dir.join("scenes/s.scene")).unwrap();
    assert!(text.contains("child_components"), "{text}");

    let mut app = engine::app();
    app.register::<Glow>().register::<Tag2>();
    engine::mount_assets(&mut app, &dir).unwrap();
    app.world.load_scene_str(&text, "s.scene").unwrap();
    let scene = &app.world.scene;
    let lamp = scene.find_by_name("Lamp A").unwrap();
    let bulb = scene.find_child(lamp, "Bulb").unwrap();
    let base = scene.find_child(lamp, "Base").unwrap();
    assert_eq!(scene.get_component::<Glow>(bulb).unwrap().power, 9.0);
    assert!(!scene.get(base).unwrap().active);
    assert_eq!(scene.get_component::<Tag2>(base).unwrap().note, "added");
    let _ = std::fs::remove_dir_all(&dir);
}
