//! Сценарии Scenario в редакторе: «New Scenario Script…» из меню, файл из шаблона во
//! вкладке Code, `ScenarioRunner` на выбранном объекте, ошибка в сохранённом файле — в Console.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::egui;
use engine::editor::{Action, Editor, ViewTab};
use engine::prelude::*;

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

struct Hands {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
        hands.frame(editor, vec![]);
        hands.frame(editor, vec![]);
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
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn find_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
        self.frame(editor, vec![]);
        fn search(shape: &egui::Shape, text: &str, out: &mut Vec<egui::Pos2>) {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => out.push(t.pos + t.galley.rect.center().to_vec2()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| search(s, text, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &self.shapes {
            search(&clipped.shape, text, &mut found);
        }
        found.into_iter().min_by(|a, b| a.y.total_cmp(&b.y))
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::button(pos, true)]);
        self.frame(editor, vec![Self::button(pos, false)]);
        self.frame(editor, vec![]);
    }

    fn press(&mut self, editor: &mut Editor, key: egui::Key) {
        for pressed in [true, false] {
            self.frame(
                editor,
                vec![egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
        }
    }
}

fn temp_assets(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_scenario_editor_{}_{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    dir
}

fn editor_in(dir: &Path) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    app.spawn("Lamp");
    engine::mount_assets(&mut app, dir).unwrap();
    Editor::new(app, EditorConfig::default()).unwrap()
}

fn runner_script(editor: &Editor, object: GameObjectId) -> Option<String> {
    let runner = editor.app.world.scene.get_component::<ScenarioRunner>(object)?;
    let db = editor.app.world.resources.get::<AssetDatabase>()?;
    db.relative_path_of(runner.script.guid()).map(str::to_string)
}

#[test]
fn new_scenario_script_from_the_assets_menu() {
    let dir = temp_assets("menu");
    let mut editor = editor_in(&dir);
    let lamp = editor.app.world.scene.find_by_name("Lamp").unwrap();
    editor.apply(Action::Select(Some(lamp)));
    let mut hands = Hands::new(&mut editor);
    let assets = hands.find_text(&mut editor, "Assets").expect("the Assets menu");
    hands.click(&mut editor, assets);
    // Порядок пунктов: Rust, C++, Scenario.
    let rust = hands.find_text(&mut editor, "New Rust Script…").expect("Rust item");
    let cpp = hands.find_text(&mut editor, "New C++ Script…").expect("C++ item");
    let item = hands.find_text(&mut editor, "New Scenario Script…").expect("the Scenario item");
    assert!(rust.y < cpp.y && cpp.y < item.y, "Rust, C++, Scenario: {rust:?} {cpp:?} {item:?}");
    hands.click(&mut editor, item);
    assert!(editor.has_dialog());
    assert!(hands.find_text(&mut editor, "New Scenario Script").is_some(), "the dialog title");
    hands.frame(&mut editor, vec![egui::Event::Text("intro".to_string())]);
    hands.press(&mut editor, egui::Key::Enter);
    assert!(!editor.has_dialog());

    let file = dir.join("scenarios/intro.scenario");
    let text = std::fs::read_to_string(&file).expect("the dialog created the file");
    assert!(text.contains("log(\"intro: started on \" .. self.name)"), "{text}");
    engine::scenario::lang::Script::compile("intro.scenario", &text).expect("the template compiles");
    let (open, _, _) = editor.code_document().expect("opened in the Code tab");
    assert!(open.ends_with("intro.scenario"), "{}", open.display());
    assert_eq!(editor.view_tab(), ViewTab::Code);
    assert_eq!(runner_script(&editor, lamp).as_deref(), Some("scenarios/intro.scenario"), "attached to the selection");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn new_scenario_action_validates_and_uses_the_folder() {
    let dir = temp_assets("action");
    let mut editor = editor_in(&dir);
    editor.apply(Action::NewScenario { name: "wave 1!".to_string(), folder: String::new(), attach_to: None });
    assert!(!dir.join("scenarios").exists(), "a bad name creates nothing");
    editor.apply(Action::NewScenario { name: "wave_1".to_string(), folder: "levels".to_string(), attach_to: None });
    assert!(dir.join("levels/wave_1.scenario").is_file());
    // Тот же файл второй раз не перезаписывается.
    std::fs::write(dir.join("levels/wave_1.scenario"), "let mine = 1\n").unwrap();
    editor.apply(Action::NewScenario { name: "wave_1".to_string(), folder: "levels".to_string(), attach_to: None });
    assert_eq!(std::fs::read_to_string(dir.join("levels/wave_1.scenario")).unwrap(), "let mine = 1\n");
    let db = editor.app.world.resources.get::<AssetDatabase>().unwrap();
    let guid = db.guid_of("levels/wave_1.scenario").expect("in the asset database");
    assert_eq!(db.meta(guid).unwrap().importer, "Scenario");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_saved_syntax_error_appears_in_the_console() {
    let dir = temp_assets("console");
    let mut editor = editor_in(&dir);
    editor.apply(Action::NewScenario { name: "broken_door".to_string(), folder: String::new(), attach_to: None });
    let since = engine::core::logging::records().last().map(|r| r.seq).unwrap_or(0);
    editor.set_code_text("let open = false\nif open then\n  log(\"never\")\n");
    editor.apply(Action::SaveCode);
    for _ in 0..60 {
        editor.advance(DT);
    }
    let errors: Vec<String> = engine::core::logging::records_since(since)
        .into_iter()
        .filter(|r| r.level == log::Level::Error && r.message.contains("broken_door.scenario"))
        .map(|r| r.message.clone())
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("scenarios/broken_door.scenario:4:1: expected `end` or `else` or `elseif` to close the `if` opened at line 2"),
        "{}",
        errors[0]
    );
    let _ = std::fs::remove_dir_all(dir);
}
