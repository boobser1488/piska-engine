//! Перетаскивание между окнами настоящей мышью egui, без GPU: строка Hierarchy в окно
//! Project — префаб в открытый каталог; сцена из окна Project на пустое место Hierarchy —
//! сцена открывается.
#![cfg(feature = "editor")]

use std::path::PathBuf;

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Panel};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const NONE: egui::Modifiers = egui::Modifiers::NONE;

/// Временный каталог ассетов с двумя сценами: `scenes/one.scene` и `scenes/two.scene`
/// (в ней объект Crate); открыта вторая.
fn editor(name: &str) -> (Editor, PathBuf) {
    std::env::set_var("ENGINE_AUDIO", "off");
    let root = std::env::temp_dir().join(format!("piska-dnd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    let mut app = engine::app();
    engine::mount_assets(&mut app, &root).unwrap();
    let mut editor = Editor::new(app, EditorConfig::default()).unwrap();
    editor.apply(Action::NewScene);
    editor.apply(Action::SaveAs("scenes/one.scene".into()));
    editor.apply(Action::NewScene);
    editor.apply(Action::CreateEmpty { parent: None });
    let crate_id = editor.selection().unwrap();
    editor.apply(Action::Rename { id: crate_id, name: "Crate".into() });
    editor.apply(Action::SaveAs("scenes/two.scene".into()));
    assert_eq!(editor.scene_path(), Some("scenes/two.scene"));
    // Окно Project — в каталоге scenes.
    editor.select_asset("scenes/one.scene");
    (editor, root)
}

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0 };
        for _ in 0..4 {
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
        let button = egui::PointerButton::Primary;
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![egui::Event::PointerButton { pos: from, button, pressed: true, modifiers: NONE }]);
        for step in 1..=12 {
            let pos = from + (to - from) * (step as f32 / 12.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        self.frame(editor, vec![egui::Event::PointerMoved(to)]);
        self.frame(editor, vec![egui::Event::PointerButton { pos: to, button, pressed: false, modifiers: NONE }]);
        for _ in 0..3 {
            self.frame(editor, vec![]);
        }
    }
}

#[test]
fn a_hierarchy_row_dropped_on_the_project_window_becomes_a_prefab_in_the_open_folder() {
    let (mut editor, root) = editor("prefab");
    let mut hands = Hands::new(&mut editor);
    let crate_id = editor.app.world.scene.find_by_name("Crate").unwrap();
    let row = hands.ctx.read_response(Editor::hierarchy_row_id(crate_id)).expect("the row is drawn").rect.center();
    let tile = editor.project_entry_rect("scenes/one.scene").expect("the scene tile is drawn");
    // Пустое место под плитками каталога.
    let target = egui::pos2(tile.left() + tile.width() * 3.5, tile.center().y);
    assert!(editor.panel_rect(Panel::Project).unwrap().contains(target));
    hands.drag(&mut editor, row, target);
    assert!(root.join("scenes/crate.prefab").is_file(), "the prefab is saved into the open folder");
    let object = editor.app.world.scene.get(crate_id).unwrap();
    assert_eq!(object.prefab(), Some("scenes/crate.prefab"), "the object becomes its instance");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_scene_dropped_on_the_empty_hierarchy_opens() {
    let (mut editor, root) = editor("scene");
    let mut hands = Hands::new(&mut editor);
    let tile = editor.project_entry_rect("scenes/one.scene").expect("the scene tile is drawn").center();
    let hierarchy = editor.panel_rect(Panel::Hierarchy).unwrap();
    let target = egui::pos2(hierarchy.center().x, hierarchy.bottom() - 40.0);
    hands.drag(&mut editor, tile, target);
    assert_eq!(editor.scene_path(), Some("scenes/one.scene"), "the dropped scene is opened");
    let _ = std::fs::remove_dir_all(root);
}
