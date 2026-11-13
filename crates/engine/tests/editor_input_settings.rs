//! Project Settings → Input: карта действий ввода в окне настроек. Интерфейс гоняется без
//! окна и без GPU через `egui::Context` с синтетическим вводом.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine::editor::{egui, Editor, EditorConfig, InputBindingList, SettingsScope};
use engine::prelude::*;
use engine_core::settings::{ActionKind, InputSettings};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const PROJECT: SettingsScope = SettingsScope::Project;

// ----- «руки» теста -----

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

    fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn click_with(&mut self, editor: &mut Editor, pos: egui::Pos2, button: egui::PointerButton) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::pointer(pos, button, true)]);
        self.frame(editor, vec![Self::pointer(pos, button, false)]);
        self.frames(editor, 2);
    }

    /// Прокрутить страницу настроек, пока виджет `id` не окажется в видимой части окна.
    fn scroll_to(&mut self, editor: &mut Editor, id: egui::Id) {
        let over_page = egui::pos2(SCREEN.x * 0.6, SCREEN.y * 0.5);
        for _ in 0..40 {
            let y = self.center(id).y;
            let delta = if y > SCREEN.y * 0.72 {
                -60.0
            } else if y < SCREEN.y * 0.25 {
                60.0
            } else {
                // Прокрутка плавная: ждём, пока виджет остановится.
                let mut last = self.center(id);
                for _ in 0..120 {
                    self.frames(editor, 1);
                    let now = self.center(id);
                    if now == last {
                        return;
                    }
                    last = now;
                }
                return;
            };
            self.frame(editor, vec![egui::Event::PointerMoved(over_page)]);
            self.frame(
                editor,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta),
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            self.frames(editor, 2);
        }
    }

    fn click_id(&mut self, editor: &mut Editor, id: egui::Id) {
        self.scroll_to(editor, id);
        let pos = self.center(id);
        self.click_with(editor, pos, egui::PointerButton::Primary);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.frames(editor, 2);
    }

    fn type_text(&mut self, editor: &mut Editor, text: &str) {
        self.frame(editor, vec![egui::Event::Text(text.to_string())]);
        self.frames(editor, 2);
    }

    fn center(&self, id: egui::Id) -> egui::Pos2 {
        self.ctx.read_response(id).unwrap_or_else(|| panic!("the widget {id:?} is drawn in the last frame")).rect.center()
    }

    fn drawn(&self, id: egui::Id) -> bool {
        self.ctx.read_response(id).is_some()
    }
}

// ----- временный проект -----

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "piska_input_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        Self { root }
    }

    fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    fn saved(&self) -> ProjectSettings {
        ProjectSettings::load(self.assets().join("project.settings")).expect("project.settings parses")
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

fn open_input_page(editor: &mut Editor) -> Hands {
    editor.show_project_settings_page("input");
    Hands::new(editor)
}

fn bindings(editor: &Editor, action: &str) -> Vec<String> {
    editor.project_settings_draft().input.find(action).unwrap_or_else(|| panic!("action {action}")).bindings.clone()
}

/// Нажата ли кнопка `action` в мире редактора, когда держат только `key`.
fn fires(editor: &mut Editor, action: &str, key: Key) -> bool {
    let input = &mut editor.app.world.input;
    input.release_all();
    input.set_key(key, true);
    let down = input.button(action);
    input.set_key(key, false);
    input.release_all();
    down
}

// ----- тесты -----

#[test]
fn the_page_lists_the_built_in_actions_and_selects_the_first() {
    let sandbox = Sandbox::new("list");
    let mut editor = editor_over(&sandbox.assets());
    let mut hands = open_input_page(&mut editor);
    assert_eq!(editor.settings_page(PROJECT), "input");
    for name in ["Horizontal", "Vertical", "Jump", "Fire1", "Submit", "Cancel"] {
        assert!(hands.drawn(Editor::input_action_row_id(name)), "{name} is listed");
    }
    assert_eq!(editor.selected_input_action().as_deref(), Some("Horizontal"));
    // Ось показывает списки «минус» / «плюс» и аналоговые оси.
    let rows = editor.settings_visible_rows(PROJECT);
    for row in ["Action list", "Name", "Type", "Negative", "Positive", "Analog axes", "Dead zone", "Revert action"] {
        assert!(rows.iter().any(|r| r == row), "row {row} in {rows:?}");
    }
    hands.click_id(&mut editor, Editor::input_action_row_id("Jump"));
    assert_eq!(editor.selected_input_action().as_deref(), Some("Jump"));
    let rows = editor.settings_visible_rows(PROJECT);
    assert!(rows.iter().any(|r| r == "Bindings") && !rows.iter().any(|r| r == "Negative"), "{rows:?}");
}

#[test]
fn listen_replaces_a_binding_the_world_follows_and_the_file_reloads_equal() {
    let sandbox = Sandbox::new("listen");
    let mut editor = editor_over(&sandbox.assets());
    let mut hands = open_input_page(&mut editor);
    hands.click_id(&mut editor, Editor::input_action_row_id("Jump"));
    assert_eq!(bindings(&editor, "Jump"), ["Key:Space", "Pad:South"]);
    assert!(fires(&mut editor, "Jump", Key::Space));

    // Щелчок по привязке Space — ожидание; нажатие K заменяет её.
    hands.click_id(&mut editor, Editor::input_binding_id(InputBindingList::Bindings, 0));
    assert!(editor.is_listening_for_input());
    hands.key(&mut editor, egui::Key::K);
    assert!(!editor.is_listening_for_input());
    assert_eq!(bindings(&editor, "Jump"), ["Key:K", "Pad:South"]);

    // Сразу действует в мире: K нажимает Jump, Space — больше нет.
    assert!(fires(&mut editor, "Jump", Key::K));
    assert!(!fires(&mut editor, "Jump", Key::Space));
    // И записано в project.settings: файл читается в то же самое.
    hands.frames(&mut editor, 2);
    let saved = sandbox.saved();
    assert_eq!(saved.input, editor.project_settings_draft().input);
    assert_eq!(saved.input.find("Jump").unwrap().bindings, ["Key:K", "Pad:South"]);

    // Новый запуск: `mount_assets` применяет карту из файла.
    let mut fresh = editor_over(&sandbox.assets());
    assert!(fires(&mut fresh, "Jump", Key::K));
    assert!(!fires(&mut fresh, "Jump", Key::Space));
}

#[test]
fn listen_adds_a_mouse_button_and_escape_cancels() {
    let sandbox = Sandbox::new("mouse");
    let mut editor = editor_over(&sandbox.assets());
    let mut hands = open_input_page(&mut editor);
    hands.click_id(&mut editor, Editor::input_action_row_id("Fire1"));
    let before = bindings(&editor, "Fire1");

    // Esc отменяет ожидание, ничего не меняя.
    hands.click_id(&mut editor, Editor::input_listen_id(InputBindingList::Bindings));
    assert!(editor.is_listening_for_input());
    hands.key(&mut editor, egui::Key::Escape);
    assert!(!editor.is_listening_for_input());
    assert_eq!(bindings(&editor, "Fire1"), before);

    // Правый щелчок где угодно — новая привязка Mouse:Right; отпускание кнопки ничего
    // не нажимает (ожидание не начинается снова).
    hands.click_id(&mut editor, Editor::input_listen_id(InputBindingList::Bindings));
    let somewhere = hands.center(Editor::input_binding_id(InputBindingList::Bindings, 0));
    hands.click_with(&mut editor, somewhere, egui::PointerButton::Secondary);
    assert!(!editor.is_listening_for_input());
    let mut expected = before.clone();
    expected.push("Mouse:Right".to_string());
    assert_eq!(bindings(&editor, "Fire1"), expected);
    let input = &mut editor.app.world.input;
    input.set_mouse_button(MouseButton::Right, true);
    assert!(input.button("Fire1"), "the mouse button fires the action");
    input.release_all();

    // Удалить привязку крестиком.
    hands.click_id(&mut editor, Editor::input_remove_binding_id(InputBindingList::Bindings, 2));
    assert_eq!(bindings(&editor, "Fire1"), before);
}

#[test]
fn axis_lists_listen_on_their_side_and_revert_restores_the_default() {
    let sandbox = Sandbox::new("axis");
    let mut editor = editor_over(&sandbox.assets());
    let mut hands = open_input_page(&mut editor);
    assert_eq!(editor.selected_input_action().as_deref(), Some("Horizontal"));
    hands.click_id(&mut editor, Editor::input_binding_id(InputBindingList::Negative, 0));
    hands.key(&mut editor, egui::Key::J);
    let horizontal = editor.project_settings_draft().input.find("Horizontal").unwrap().clone();
    assert_eq!(horizontal.negative[0], "Key:J");
    let input = &mut editor.app.world.input;
    input.set_key(Key::J, true);
    assert_eq!(input.axis("Horizontal"), -1.0, "J pushes the axis to −1");
    input.release_all();
    input.set_key(Key::A, true);
    assert_eq!(input.axis("Horizontal"), 0.0, "A no longer does");
    input.release_all();

    hands.click_id(&mut editor, Editor::input_revert_action_id());
    assert_eq!(editor.project_settings_draft().input, InputSettings::default());
    let input = &mut editor.app.world.input;
    input.set_key(Key::A, true);
    assert_eq!(input.axis("Horizontal"), -1.0);
    input.release_all();
}

#[test]
fn actions_are_added_renamed_and_removed_with_unique_names() {
    let sandbox = Sandbox::new("actions");
    let mut editor = editor_over(&sandbox.assets());
    let mut hands = open_input_page(&mut editor);

    // Новое действие-кнопка Dash.
    hands.click_id(&mut editor, Editor::input_new_action_field_id());
    hands.type_text(&mut editor, "Dash");
    hands.click_id(&mut editor, Editor::input_add_action_id(ActionKind::Button));
    let map = editor.project_settings_draft().input;
    assert_eq!(map.find("Dash").map(|a| a.kind), Some(ActionKind::Button));
    assert_eq!(editor.selected_input_action().as_deref(), Some("Dash"));
    // Привязка Q через Listen — и мир знает действие.
    hands.click_id(&mut editor, Editor::input_listen_id(InputBindingList::Bindings));
    hands.key(&mut editor, egui::Key::Q);
    assert_eq!(bindings(&editor, "Dash"), ["Key:Q"]);
    assert!(fires(&mut editor, "Dash", Key::Q));

    // Второе Dash не добавляется: ошибка в строке состояния.
    hands.click_id(&mut editor, Editor::input_new_action_field_id());
    hands.type_text(&mut editor, "Dash");
    hands.click_id(&mut editor, Editor::input_add_action_id(ActionKind::Axis));
    assert_eq!(editor.project_settings_draft().input.actions.iter().filter(|a| a.name == "Dash").count(), 1);
    assert!(editor.settings_status(PROJECT).unwrap_or_default().contains("already exists"));

    // Переименование: Jump занято, Sprint — нет.
    hands.click_id(&mut editor, Editor::input_action_row_id("Dash"));
    let rename = Editor::input_rename_field_id();
    hands.click_id(&mut editor, rename);
    for _ in 0..4 {
        hands.key(&mut editor, egui::Key::Backspace);
    }
    hands.type_text(&mut editor, "Jump");
    hands.key(&mut editor, egui::Key::Enter);
    assert!(editor.project_settings_draft().input.find("Dash").is_some(), "a taken name is refused");
    assert!(editor.settings_status(PROJECT).unwrap_or_default().contains("“Jump” already exists"));
    hands.click_id(&mut editor, rename);
    for _ in 0..4 {
        hands.key(&mut editor, egui::Key::Backspace);
    }
    hands.type_text(&mut editor, "Sprint");
    hands.key(&mut editor, egui::Key::Enter);
    let map = editor.project_settings_draft().input;
    assert!(map.find("Dash").is_none());
    assert_eq!(map.find("Sprint").unwrap().bindings, ["Key:Q"], "renaming keeps the bindings");
    assert!(!editor.app.world.input.has_button("Dash"), "the old name leaves the world");
    assert!(fires(&mut editor, "Sprint", Key::Q));

    // Удаление: своё и встроенное.
    hands.click_id(&mut editor, Editor::input_remove_action_id("Sprint"));
    hands.click_id(&mut editor, Editor::input_remove_action_id("Fire1"));
    let map = editor.project_settings_draft().input;
    assert!(map.find("Sprint").is_none() && map.find("Fire1").is_none());
    assert!(!editor.app.world.input.has_button("Sprint") && !editor.app.world.input.has_button("Fire1"));
    hands.frames(&mut editor, 2);
    assert_eq!(sandbox.saved().input, map, "the file has the same map");
    // Новый запуск: встроенного Fire1 больше нет и в игре.
    let fresh = editor_over(&sandbox.assets());
    assert!(!fresh.app.world.input.has_button("Fire1"));
    assert!(fresh.app.world.input.has_button("Jump"));

    // Reset page возвращает встроенные действия.
    hands.click_id(&mut editor, Editor::settings_reset_page_id(PROJECT));
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    assert_eq!(editor.project_settings_draft().input, InputSettings::default());
    assert!(editor.app.world.input.has_button("Fire1"));
    assert_eq!(sandbox.saved().input, InputSettings::default());
}

#[test]
fn game_code_that_binds_after_start_up_wins_over_the_map() {
    let sandbox = Sandbox::new("codewins");
    let mut settings = ProjectSettings::default();
    let jump = settings.input.actions.iter_mut().find(|a| a.name == "Jump").unwrap();
    jump.bindings = vec!["Key:K".to_string()];
    settings.save(sandbox.assets().join("project.settings")).unwrap();

    let mut app = engine::app();
    engine::mount_assets(&mut app, sandbox.assets()).unwrap();
    assert_eq!(app.world.input.button_bindings("Jump"), [Binding::Key(Key::K)], "the map is applied at start-up");
    // Код игры (`build`) привязывает своё позже — побеждает он.
    app.world.input.bind_button("Jump", &[Key::L]);
    app.world.input.set_key(Key::L, true);
    assert!(app.world.input.button("Jump"));
}

// ----- снимки страниц (нужен GPU) -----

/// `ENGINE_SHOTS_DIR=out cargo test -p engine --features editor --test editor_input_settings -- --ignored shots`
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn input_and_build_page_shots() {
    use engine::editor::{capture, capture_with_input};
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let sandbox = Sandbox::new("shots");
    std::fs::write(
        sandbox.root.join("Cargo.toml"),
        "[package]\nname = \"lantern\"\nversion = \"0.3.0\"\n\n[package.metadata.piska]\ntitle = \"Lantern\"\n\n[package.metadata.piska.android]\npackage = \"com.example.lantern\"\norientation = \"portrait\"\n\n[dependencies]\nengine = { path = \"../../crates/engine\" }\n",
    )
    .unwrap();
    let mut editor = editor_over(&sandbox.assets());
    editor
        .edit_project_settings(|s| {
            s.input.actions.push(engine_core::settings::InputAction::button("Dash", &[Binding::Key(Key::Q), Binding::Mouse(MouseButton::Right)]));
        })
        .unwrap();
    editor.show_project_settings_page("input");
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("project_input_axis.png")).unwrap();
    editor.select_input_action("Jump");
    let hands = Hands::new(&mut editor);
    let chip = hands.center(Editor::input_binding_id(InputBindingList::Bindings, 0));
    let image = capture_with_input(&mut editor, 1440, 860, 16, |frame, _| match frame {
        6 => vec![egui::Event::PointerMoved(chip)],
        8 => vec![Hands::pointer(chip, egui::PointerButton::Primary, true)],
        9 => vec![Hands::pointer(chip, egui::PointerButton::Primary, false)],
        _ => vec![],
    })
    .unwrap();
    image.save(dir.join("project_input_listen.png")).unwrap();
    editor.show_project_settings_page("build");
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("project_build.png")).unwrap();
    let bare = Sandbox::new("shots_empty");
    let mut empty = editor_over(&bare.assets());
    empty.show_project_settings_page("build");
    capture(&mut empty, 1440, 860, 10).unwrap().save(dir.join("project_build_no_crate.png")).unwrap();
}
