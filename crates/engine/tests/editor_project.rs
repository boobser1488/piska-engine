//! Окно Project работает мышью: каталоги открываются щелчком в дереве и двойным щелчком
//! по плитке, ассеты выделяются. Окно и GPU не нужны.
#![cfg(feature = "editor")]

use std::path::Path;

use engine::editor::{egui, Editor, EditorConfig, Panel};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0 };
        hands.frames(editor, 3);
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
        self.frames(editor, 2);
    }

    fn double_click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        for _ in 0..2 {
            self.frame(editor, vec![Self::button(pos, true)]);
            self.frame(editor, vec![Self::button(pos, false)]);
        }
        self.frames(editor, 2);
    }

    /// Центр виджета с таким идентификатором, если он нарисован в последнем кадре.
    fn center(&self, id: egui::Id) -> Option<egui::Pos2> {
        self.ctx.read_response(id).map(|response| response.rect.center())
    }

    fn asset(&self, editor: &Editor, path: &str) -> Option<egui::Pos2> {
        self.center(editor.project_row_id(path).unwrap_or_else(|| panic!("asset `{path}` is known")))
    }
}

fn editor() -> Editor {
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, Path::new(ASSETS)).expect("assets directory opens");
    let mut editor =
        Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).expect("level1.scene loads");
    // Окно Project на весь редактор: в стандартной раскладке оно невысокое, и плитки
    // второго ряда уходят за край экрана — щёлкать по ним было бы нечем.
    editor.maximize_panel(Some(Panel::Project));
    editor
}

#[test]
fn a_folder_opens_by_a_click_in_the_tree() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    assert_eq!(editor.project_folder(), "", "the Project window starts in the assets root");
    assert!(hands.asset(&editor, "models/crystal.obj").is_none(), "files of subfolders are not shown in the root");

    let row = hands.center(Editor::project_tree_row_id("models")).expect("the tree has a `models` row");
    hands.click(&mut editor, row);
    assert_eq!(editor.project_folder(), "models", "a click on the tree row opens the folder");
    assert!(hands.asset(&editor, "models/crystal.obj").is_some(), "its files are listed");

    let root = hands.center(Editor::project_tree_row_id("")).expect("the tree has the `Assets` row");
    hands.click(&mut editor, root);
    assert_eq!(editor.project_folder(), "");
}

#[test]
fn a_folder_tile_opens_by_a_double_click_and_a_single_click_selects() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);

    let tile = hands.center(Editor::project_folder_tile_id("scenes")).expect("the root shows a `scenes` tile");
    hands.click(&mut editor, tile);
    assert_eq!(editor.project_selection(), Some("scenes"), "one click selects the folder tile");
    assert_eq!(editor.project_folder(), "", "and does not open it");

    hands.double_click(&mut editor, tile);
    assert_eq!(editor.project_folder(), "scenes", "a double click opens the folder");

    let file = hands.asset(&editor, "scenes/level1.scene").expect("the scene file is listed");
    hands.click(&mut editor, file);
    assert_eq!(editor.project_selection(), Some("scenes/level1.scene"), "a click selects an asset");

    // Щелчок по пустому месту снимает выделение, но не мешает щелчкам по плиткам.
    // Пустое место — правее самой правой плитки этой папки.
    let rightmost = std::fs::read_dir(Path::new(ASSETS).join("scenes"))
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.ends_with(".meta"))
        .filter_map(|name| editor.project_row_id(&format!("scenes/{name}")))
        .filter_map(|id| hands.center(id))
        .map(|center| center.x)
        .fold(file.x, f32::max);
    let empty = egui::pos2(rightmost + 170.0, file.y);
    hands.click(&mut editor, empty);
    assert_eq!(editor.project_selection(), None);
}

#[test]
fn the_breadcrumb_goes_back_up() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let tile = hands.center(Editor::project_folder_tile_id("sprites")).expect("the root shows a `sprites` tile");
    hands.double_click(&mut editor, tile);
    assert_eq!(editor.project_folder(), "sprites");

    let up = hands.center(Editor::project_tree_row_id("")).expect("the tree has the `Assets` row");
    hands.click(&mut editor, up);
    assert_eq!(editor.project_folder(), "");
    assert!(hands.center(Editor::project_folder_tile_id("sprites")).is_some(), "the root tiles are back");
}
