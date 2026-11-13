//! Общая часть тестов окна Project как файлового менеджера: «руки» (события egui), временный
//! каталог ассетов и вспомогательные функции. Настоящий `assets/` тесты не меняют: они
//! работают на временных каталогах и копиях.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine::editor::{egui, Editor, EditorConfig, Panel};

pub const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
pub const DT: f32 = 1.0 / 60.0;
pub const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

// ----- «руки» теста -----

pub struct Hands {
    pub ctx: egui::Context,
    pub time: f64,
    pub shapes: Vec<egui::epaint::ClippedShape>,
    pub modifiers: egui::Modifiers,
    pub screen: egui::Vec2,
    pub pointer: Option<egui::Pos2>,
}

impl Hands {
    pub fn new(editor: &mut Editor) -> Self {
        let mut hands = Self {
            ctx: egui::Context::default(),
            time: 0.0,
            shapes: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            screen: SCREEN,
            pointer: None,
        };
        hands.frames(editor, 3);
        hands
    }

    pub fn frame_with(&mut self, editor: &mut Editor, events: Vec<egui::Event>, dropped: Vec<egui::DroppedFile>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.screen)),
            time: Some(self.time),
            predicted_dt: DT,
            modifiers: self.modifiers,
            events,
            dropped_files: dropped,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    pub fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>) {
        self.frame_with(editor, events, Vec::new());
    }

    pub fn frames(&mut self, editor: &mut Editor, count: usize) {
        for _ in 0..count {
            self.frame(editor, vec![]);
        }
    }

    /// Прошло `seconds` секунд без ввода.
    pub fn wait(&mut self, editor: &mut Editor, seconds: f64) {
        self.time += seconds;
        self.frames(editor, 2);
    }

    pub fn button(&self, pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: self.modifiers }
    }

    pub fn move_to(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.pointer = Some(pos);
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
    }

    pub fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.move_to(editor, pos);
        let (down, up) = (self.button(pos, true), self.button(pos, false));
        self.frame(editor, vec![down]);
        self.frame(editor, vec![up]);
        self.frames(editor, 2);
    }

    pub fn click_with(&mut self, editor: &mut Editor, pos: egui::Pos2, modifiers: egui::Modifiers) {
        // Не сливаться с предыдущим щелчком в двойной или тройной.
        self.wait(editor, 0.4);
        self.modifiers = modifiers;
        self.click(editor, pos);
        self.modifiers = egui::Modifiers::NONE;
    }

    pub fn double_click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        // Пауза: щелчок, сделанный перед этим, не должен слиться с первым щелчком двойного.
        self.wait(editor, 0.5);
        self.move_to(editor, pos);
        for _ in 0..2 {
            let (down, up) = (self.button(pos, true), self.button(pos, false));
            self.frame(editor, vec![down]);
            self.frame(editor, vec![up]);
        }
        self.frames(editor, 2);
    }

    pub fn right_click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.move_to(editor, pos);
        let event = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.frames(editor, 2);
    }

    pub fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        self.drag_with(editor, from, to, egui::Modifiers::NONE);
    }

    pub fn drag_with(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2, modifiers: egui::Modifiers) {
        self.move_to(editor, from);
        let down = self.button(from, true);
        self.frame(editor, vec![down]);
        self.modifiers = modifiers;
        for step in 1..=8 {
            let pos = from + (to - from) * (step as f32 / 8.0);
            self.move_to(editor, pos);
        }
        self.frames(editor, 1);
        let up = self.button(to, false);
        self.frame(editor, vec![up]);
        self.modifiers = egui::Modifiers::NONE;
        self.frames(editor, 3);
    }

    /// Нажать клавишу с модификаторами.
    pub fn key(&mut self, editor: &mut Editor, key: egui::Key, modifiers: egui::Modifiers) {
        self.modifiers = modifiers;
        let event = |pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.modifiers = egui::Modifiers::NONE;
        self.frames(editor, 2);
    }

    pub fn press(&mut self, editor: &mut Editor, key: egui::Key) {
        self.key(editor, key, egui::Modifiers::NONE);
    }

    pub fn ctrl(&mut self, editor: &mut Editor, key: egui::Key) {
        self.key(editor, key, egui::Modifiers::COMMAND);
    }

    pub fn type_text(&mut self, editor: &mut Editor, text: &str) {
        self.frame(editor, vec![egui::Event::Text(text.to_string())]);
        self.frames(editor, 2);
    }

    pub fn center(&self, id: egui::Id) -> egui::Pos2 {
        self.ctx.read_response(id).expect("the widget is drawn in the last frame").rect.center()
    }

    pub fn has(&self, id: egui::Id) -> bool {
        self.ctx.read_response(id).is_some()
    }

    pub fn asset(&self, editor: &Editor, path: &str) -> egui::Pos2 {
        self.center(editor.project_row_id(path).unwrap_or_else(|| panic!("asset `{path}` is known")))
    }

    pub fn folder(&self, path: &str) -> egui::Pos2 {
        self.center(Editor::project_folder_tile_id(path))
    }

    pub fn tree(&self, path: &str) -> egui::Pos2 {
        self.center(Editor::project_tree_row_id(path))
    }

    /// Центр надписи `text` в последнем кадре (самая верхняя, если их несколько).
    pub fn find_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
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
}

// ----- временный каталог ассетов -----

pub struct Sandbox {
    pub root: PathBuf,
}

impl Sandbox {
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!("piska_files_{tag}_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// Копия настоящего каталога ассетов: тесты меняют только её.
    pub fn copy_of_assets(tag: &str) -> Self {
        let sandbox = Self::new(tag);
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap().flatten() {
                let target = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &target);
                } else {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        copy(Path::new(ASSETS), &sandbox.root);
        sandbox
    }

    pub fn file(&self, rel: &str, content: &str) -> &Self {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        self
    }

    pub fn dir(&self, rel: &str) -> &Self {
        std::fs::create_dir_all(self.root.join(rel)).unwrap();
        self
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).exists()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn editor_over(assets: &Path) -> Editor {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    let mut editor = Editor::new(app, EditorConfig::default()).expect("the editor starts");
    // Окно Project на весь редактор: в стандартной раскладке оно невысокое.
    editor.maximize_panel(Some(Panel::Project));
    editor
}

pub fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

pub fn selected(editor: &Editor) -> Vec<String> {
    editor.project_selection_all()
}

pub fn alt() -> egui::Modifiers {
    egui::Modifiers::ALT
}

/// GUID из `<файл>.meta`.
pub fn meta_guid(path: &Path) -> String {
    let mut name = path.as_os_str().to_owned();
    name.push(".meta");
    let text = std::fs::read_to_string(PathBuf::from(name)).expect("the asset has a .meta");
    let at = text.find("guid:").expect("the .meta has a guid");
    text[at..].split('"').nth(1).expect("the guid is quoted").to_string()
}

/// Нарисована ли плитка (строка) ассета в последнем кадре.
pub fn painted(hands: &Hands, editor: &Editor, path: &str) -> bool {
    editor.project_row_id(path).is_some_and(|id| hands.has(id))
}
