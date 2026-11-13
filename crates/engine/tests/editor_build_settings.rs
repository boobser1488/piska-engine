//! Project Settings → Build: настройки экспорта в `[package.metadata.piska]` Cargo.toml
//! игры. Интерфейс гоняется без окна и без GPU через `egui::Context`.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine::editor::{egui, AndroidOrientation, BuildOptions, Editor, EditorConfig, SettingsScope};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const PROJECT: SettingsScope = SettingsScope::Project;

const MANIFEST: &str = r#"# Игра для теста страницы Build.
[package]
name = "lantern"    # имя пакета
version = "0.3.0"
edition = "2021"

# Настройки экспорта (их правит редактор).
[package.metadata.piska]
title = "Lantern"   # заголовок

[dependencies]
# Движок без редактора.
engine = { path = "../../crates/engine", default-features = false }

[features]
default = []
"#;

struct Hands {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
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
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn frames(&mut self, editor: &mut Editor, count: usize) {
        for _ in 0..count {
            self.frame(editor, vec![]);
        }
    }

    fn click_id(&mut self, editor: &mut Editor, id: egui::Id) {
        let pos = self.ctx.read_response(id).unwrap_or_else(|| panic!("{id:?} is drawn")).rect.center();
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![button(true)]);
        self.frame(editor, vec![button(false)]);
        self.frames(editor, 2);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.frames(editor, 2);
    }

    fn type_text(&mut self, editor: &mut Editor, text: &str) {
        self.frame(editor, vec![egui::Event::Text(text.to_string())]);
        self.frames(editor, 2);
    }

    /// Есть ли в последнем кадре надпись, содержащая `part`.
    fn shows(&mut self, editor: &mut Editor, part: &str) -> bool {
        self.frame(editor, vec![]);
        fn search(shape: &egui::Shape, part: &str) -> bool {
            match shape {
                egui::Shape::Text(t) => t.galley.text().contains(part),
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| search(s, part)),
                _ => false,
            }
        }
        self.shapes.iter().any(|c| search(&c.shape, part))
    }
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str, manifest: Option<&str>) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "piska_build_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/sprites")).unwrap();
        std::fs::write(root.join("assets/sprites/logo.png"), b"\x89PNG").unwrap();
        if let Some(text) = manifest {
            std::fs::write(root.join("Cargo.toml"), text).unwrap();
        }
        Self { root }
    }

    fn manifest(&self) -> String {
        std::fs::read_to_string(self.root.join("Cargo.toml")).unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn editor_over(assets: &Path) -> Editor {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    Editor::new(app, EditorConfig::default()).expect("the editor starts")
}

fn open_build_page(sandbox: &Sandbox) -> (Editor, Hands) {
    let mut editor = editor_over(&sandbox.root.join("assets"));
    editor.show_project_settings_page("build");
    let hands = Hands::new(&mut editor);
    (editor, hands)
}

#[test]
fn without_a_game_crate_the_page_explains_instead_of_showing_controls() {
    let sandbox = Sandbox::new("nocrate", None);
    let (mut editor, mut hands) = open_build_page(&sandbox);
    assert_eq!(editor.settings_page(PROJECT), "build");
    assert!(editor.game_manifest_path().is_none());
    assert!(hands.shows(&mut editor, "This project has no game crate"));
    assert!(editor.settings_visible_rows(PROJECT).is_empty(), "no controls: {:?}", editor.settings_visible_rows(PROJECT));
    assert!(editor.build_options().unwrap_err().to_string().contains("no game crate found"));
    // Не крейт игры (не зависит от engine) — то же самое.
    std::fs::write(sandbox.root.join("Cargo.toml"), "[package]\nname = \"tool\"\n").unwrap();
    assert!(editor.game_manifest_path().is_none());
}

#[test]
fn page_edits_reach_the_cargo_toml_and_keep_everything_else() {
    let sandbox = Sandbox::new("edit", Some(MANIFEST));
    let (mut editor, mut hands) = open_build_page(&sandbox);
    assert_eq!(editor.game_manifest_path().as_deref(), Some(sandbox.root.join("Cargo.toml").as_path()));
    let rows = editor.settings_visible_rows(PROJECT);
    for row in ["Manifest", "Title", "Icon", "Package id", "Version code", "Version name", "Orientation", "Minimum SDK"] {
        assert!(rows.iter().any(|r| r == row), "row {row} in {rows:?}");
    }
    assert_eq!(editor.build_options().unwrap().title.as_deref(), Some("Lantern"));

    // Package id: печатаем и подтверждаем Enter — значение уходит в файл.
    hands.click_id(&mut editor, Editor::build_field_id("android.package"));
    hands.type_text(&mut editor, "com.example.lantern");
    hands.key(&mut editor, egui::Key::Enter);
    hands.frames(&mut editor, 2);
    let text = sandbox.manifest();
    assert!(text.contains("[package.metadata.piska.android]\npackage = \"com.example.lantern\""), "{text}");
    for kept in [
        "# Игра для теста страницы Build.",
        "name = \"lantern\"    # имя пакета",
        "# Настройки экспорта (их правит редактор).",
        "title = \"Lantern\"   # заголовок",
        "# Движок без редактора.",
        "[features]\ndefault = []",
    ] {
        assert!(text.contains(kept), "`{kept}` is kept:\n{text}");
    }
    assert_eq!(editor.build_options().unwrap().android_package.as_deref(), Some("com.example.lantern"));
    assert!(editor.settings_status(PROJECT).unwrap_or_default().contains("Saved export options"));

    // Неверный id: поле показывает ошибку, файл не меняется.
    hands.click_id(&mut editor, Editor::build_field_id("android.package"));
    for _ in 0.."com.example.lantern".len() {
        hands.key(&mut editor, egui::Key::Backspace);
    }
    hands.type_text(&mut editor, "1lantern");
    hands.key(&mut editor, egui::Key::Enter);
    hands.frames(&mut editor, 2);
    let error = editor.build_field_error("android.package").expect("the field shows an error");
    assert!(error.contains("not a valid Android package id") && error.contains("com.example.my_game"), "{error}");
    assert_eq!(sandbox.manifest(), text, "an invalid value is not written");

    // Стрелка сброса убирает ключ.
    hands.click_id(&mut editor, Editor::settings_reset_id(PROJECT, "Package id"));
    hands.frames(&mut editor, 2);
    assert!(editor.build_field_error("android.package").is_none());
    assert!(!sandbox.manifest().contains("com.example.lantern"));
    assert!(!sandbox.manifest().contains("[package.metadata.piska.android]"), "the emptied table goes away");
}

#[test]
fn options_set_through_the_editor_round_trip_and_invalid_ones_are_refused() {
    let sandbox = Sandbox::new("api", Some(MANIFEST));
    let (mut editor, mut hands) = open_build_page(&sandbox);
    let options = BuildOptions {
        title: Some("Lantern Deluxe".into()),
        icon: Some("sprites/logo.png".into()),
        android_package: Some("org.studio.lantern".into()),
        android_version_code: Some(7),
        android_version_name: Some("1.2.0".into()),
        android_orientation: Some(AndroidOrientation::Portrait),
        android_min_sdk: Some(29),
    };
    editor.set_build_options(&options).unwrap();
    assert_eq!(editor.build_options().unwrap(), options);
    let text = sandbox.manifest();
    assert!(text.contains("title = \"Lantern Deluxe\"   # заголовок"), "the comment stays:\n{text}");
    assert!(text.contains("orientation = \"portrait\"") && text.contains("min_sdk = 29") && text.contains("version_code = 7"), "{text}");
    // Страница показывает записанное.
    hands.frames(&mut editor, 2);
    assert!(hands.shows(&mut editor, "Portrait"));
    assert!(hands.shows(&mut editor, "sprites/logo.png"));

    let bad = BuildOptions { android_min_sdk: Some(19), ..options.clone() };
    let error = editor.set_build_options(&bad).unwrap_err().to_string();
    assert!(error.contains("at least 26"), "{error}");
    assert_eq!(sandbox.manifest(), text, "nothing is written");
    assert_eq!(editor.settings_status(PROJECT).as_deref(), Some(error.as_str()));

    // Reset page убирает все ключи экспорта, остальной файл остаётся.
    hands.click_id(&mut editor, Editor::settings_reset_page_id(PROJECT));
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    assert_eq!(editor.build_options().unwrap(), BuildOptions::default());
    let text = sandbox.manifest();
    assert!(text.contains("# Движок без редактора.") && text.contains("name = \"lantern\"    # имя пакета"), "{text}");
}
