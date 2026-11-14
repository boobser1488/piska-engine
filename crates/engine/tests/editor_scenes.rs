//! Выбор сцены настоящим вводом egui, без GPU: File → Open Scene… (Ctrl+O) с поиском и
//! клавиатурой, вопрос о несохранённом, недавние сцены, New Scene… (Ctrl+N, 2D / 3D),
//! переключатель сцен в строке сцены окна Hierarchy, Play / Stop на открытой сцене.
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Mode, Preferences};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const NONE: egui::Modifiers = egui::Modifiers::NONE;
const CTRL: egui::Modifiers = egui::Modifiers::COMMAND;

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let mut editor = Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap();
    // Чистые настройки: недавние сцены этого теста не смешиваются с чужими.
    editor.set_preferences(Preferences::default());
    editor
}

struct Hands {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
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
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn press(&mut self, editor: &mut Editor, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        self.frame(editor, vec![event(true)], modifiers);
        self.frame(editor, vec![event(false)], modifiers);
        self.frame(editor, vec![], NONE);
    }

    fn type_text(&mut self, editor: &mut Editor, text: &str) {
        self.frame(editor, vec![egui::Event::Text(text.to_string())], NONE);
        self.frame(editor, vec![], NONE);
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        let button = egui::PointerButton::Primary;
        self.frame(editor, vec![egui::Event::PointerMoved(pos)], NONE);
        self.frame(editor, vec![egui::Event::PointerButton { pos, button, pressed: true, modifiers: NONE }], NONE);
        self.frame(editor, vec![egui::Event::PointerButton { pos, button, pressed: false, modifiers: NONE }], NONE);
        self.frame(editor, vec![], NONE);
    }

    /// Центр надписи `text` в последнем кадре (самая нижняя, если их несколько: всплывающее
    /// меню рисуется под строкой, на которой открыто).
    fn find_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
        self.all_text(editor, text).into_iter().max_by(|a, b| a.y.total_cmp(&b.y))
    }

    /// Самая верхняя надпись `text` (строка сцены Hierarchy выше строки состояния).
    fn find_top_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
        self.all_text(editor, text).into_iter().min_by(|a, b| a.y.total_cmp(&b.y))
    }

    fn all_text(&mut self, editor: &mut Editor, text: &str) -> Vec<egui::Pos2> {
        self.frame(editor, vec![], NONE);
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
        found
    }
}

#[test]
fn the_project_scene_list_has_every_scene_and_the_picker_opens_one_with_the_keyboard() {
    let mut editor = editor();
    let scenes: Vec<String> = editor.project_scenes().into_iter().map(|s| s.path).collect();
    for expected in ["scenes/level1.scene", "scenes/demo3d.scene", "scenes/showcase/valley.scene"] {
        assert!(scenes.iter().any(|s| s == expected), "{expected} is listed: {scenes:?}");
    }
    let mut hands = Hands::new(&mut editor);

    // Ctrl+O — окно со списком; поиск печатают сразу (фокус в поле), Enter открывает.
    hands.press(&mut editor, egui::Key::O, CTRL);
    assert!(editor.is_scene_picker_open(), "Ctrl+O opens File → Open Scene…");
    hands.type_text(&mut editor, "valy");
    let found: Vec<String> = editor.scene_picker_matches().into_iter().map(|s| s.path).collect();
    assert_eq!(found.first().map(String::as_str), Some("scenes/showcase/valley.scene"), "fuzzy search: {found:?}");
    hands.press(&mut editor, egui::Key::Enter, NONE);
    assert!(!editor.is_scene_picker_open());
    assert_eq!(editor.scene_path(), Some("scenes/showcase/valley.scene"));

    // Стрелки двигают выбор; Esc закрывает, ничего не открывая.
    hands.press(&mut editor, egui::Key::O, CTRL);
    hands.type_text(&mut editor, "level");
    hands.press(&mut editor, egui::Key::Escape, NONE);
    assert!(!editor.is_scene_picker_open());
    assert_eq!(editor.scene_path(), Some("scenes/showcase/valley.scene"));

    // Недавние: новые первыми, переживают запись настроек.
    let recent = editor.recent_scenes();
    assert_eq!(recent[..2], ["scenes/showcase/valley.scene".to_string(), "scenes/level1.scene".to_string()]);
    let text = editor.preferences().to_text();
    editor.set_preferences(Preferences::parse(&text));
    assert_eq!(editor.recent_scenes()[..2], recent[..2]);
}

#[test]
fn opening_a_scene_over_unsaved_changes_asks_first() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = editor.app.world.scene.find_by_name("Player").unwrap();
    editor.apply(Action::Rename { id: player, name: "Hero".into() });
    hands.frame(&mut editor, vec![], NONE);
    assert!(editor.is_dirty());

    hands.press(&mut editor, egui::Key::O, CTRL);
    hands.type_text(&mut editor, "demo3d");
    hands.press(&mut editor, egui::Key::Enter, NONE);
    assert!(editor.has_dialog(), "Save / Don't Save / Cancel is asked before the scene is replaced");
    assert_eq!(editor.scene_path(), Some("scenes/level1.scene"));

    // Cancel — остаёмся в своей сцене с правками.
    let cancel = hands.find_text(&mut editor, "Cancel").expect("the dialog has Cancel");
    hands.click(&mut editor, cancel);
    assert!(!editor.has_dialog());
    assert_eq!(editor.scene_path(), Some("scenes/level1.scene"));
    assert!(editor.is_dirty());

    // Ещё раз — Don't Save: правки уходят, сцена открывается.
    hands.press(&mut editor, egui::Key::O, CTRL);
    hands.type_text(&mut editor, "demo3d");
    hands.press(&mut editor, egui::Key::Enter, NONE);
    let discard = hands.find_text(&mut editor, "Don't Save").expect("the dialog has Don't Save");
    hands.click(&mut editor, discard);
    assert_eq!(editor.scene_path(), Some("scenes/demo3d.scene"));
    assert!(!editor.is_dirty());
}

#[test]
fn the_hierarchy_scene_name_switches_scenes_in_two_clicks() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let name = editor.app.world.scene.name.clone();
    let header = hands.find_top_text(&mut editor, &name).expect("the Hierarchy shows the scene name");
    hands.click(&mut editor, header);
    let item = hands.find_text(&mut editor, "workshop").expect("the dropdown lists the project's scenes");
    hands.click(&mut editor, item);
    assert_eq!(editor.scene_path(), Some("scenes/showcase/workshop.scene"));
}

#[test]
fn new_scene_offers_2d_and_3d_starters_and_play_stop_keep_the_open_scene() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    hands.press(&mut editor, egui::Key::N, CTRL);
    assert!(editor.is_new_scene_dialog_open(), "Ctrl+N asks 2D or 3D");
    let three_d = hands.find_text(&mut editor, "3D").expect("the 3D choice");
    hands.click(&mut editor, three_d);
    hands.press(&mut editor, egui::Key::Enter, NONE);
    assert!(!editor.is_new_scene_dialog_open());
    let names: Vec<String> = editor.app.world.scene.ids().iter().filter_map(|id| editor.app.world.scene.get(*id)).map(|o| o.name.clone()).collect();
    assert_eq!(names, ["Main Camera", "Directional Light", "Ground"]);
    assert!(editor.is_scene_3d(), "the Scene view switches to 3D");
    assert_eq!(editor.scene_path(), None);

    // 2D — одна камера.
    hands.press(&mut editor, egui::Key::N, CTRL);
    let two_d = hands.find_text(&mut editor, "2D").expect("the 2D choice");
    hands.click(&mut editor, two_d);
    let create = hands.find_text(&mut editor, "Create").expect("Create");
    hands.click(&mut editor, create);
    assert_eq!(editor.app.world.scene.len(), 1);
    assert!(!editor.is_scene_3d());

    // Play начинается с открытой сцены, после Stop она та же.
    editor.apply(Action::OpenScene("scenes/level1.scene".into()));
    editor.apply(Action::Play);
    for _ in 0..30 {
        editor.advance(DT);
        hands.frame(&mut editor, vec![], NONE);
    }
    assert_eq!(editor.mode(), Mode::Playing);
    editor.apply(Action::Stop);
    assert_eq!(editor.scene_path(), Some("scenes/level1.scene"));
    assert!(!editor.is_dirty());
}

#[test]
fn undo_history_lists_named_steps_and_a_click_jumps_there() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = editor.app.world.scene.find_by_name("Player").unwrap();
    editor.apply(Action::Rename { id: player, name: "Hero".into() });
    hands.frame(&mut editor, vec![], NONE);
    editor.apply(Action::CreateEmpty { parent: None });
    hands.frame(&mut editor, vec![], NONE);
    let (steps, position) = editor.undo_history();
    assert_eq!(steps, ["Rename", "Create Empty"]);
    assert_eq!(position, 2);

    editor.show_undo_history(true);
    hands.frame(&mut editor, vec![], NONE);
    hands.frame(&mut editor, vec![], NONE);
    let row = |hands: &Hands, index: usize| {
        hands.ctx.read_response(Editor::undo_history_row_id(index)).expect("the step row is drawn").rect.center()
    };
    // Щелчок по «Rename» — отменить создание, имя остаётся новым.
    let rename = row(&hands, 1);
    hands.click(&mut editor, rename);
    assert_eq!(editor.undo_history().1, 1);
    assert!(editor.app.world.scene.find_by_name("GameObject").is_none(), "the created object is undone");
    assert!(editor.app.world.scene.find_by_name("Hero").is_some());
    // «Сцена открыта» — всё отменено; последняя строка — всё вернуть.
    let opened = row(&hands, 0);
    hands.click(&mut editor, opened);
    assert_eq!(editor.undo_history().1, 0);
    assert!(editor.app.world.scene.find_by_name("Player").is_some());
    let last = row(&hands, 2);
    hands.click(&mut editor, last);
    assert_eq!(editor.undo_history(), (vec!["Rename".to_string(), "Create Empty".to_string()], 2));
}
