//! Preferences редактора: реестр команд и горячих клавиш (страница Shortcuts) и страницы
//! General, Appearance, Scene View, Console & Diagnostics, Build & Tools — каждая
//! настройка проверяется по тому, что она меняет в работе редактора. Без окна и без GPU:
//! интерфейс гоняется через `egui::Context` с синтетическим вводом (кроме галереи
//! снимков — она помечена `#[ignore]`).
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine::editor::{
    egui, Action, Editor, EditorConfig, ExportPlatform, ExportSettings, KeyBinding, Mode, Orbit3D, Panel, Preferences,
    SettingsScope, Tool, COMMANDS,
};
use engine::prelude::*;

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const PREFS: SettingsScope = SettingsScope::Preferences;

// ----- «руки» теста -----

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
        editor.forward_game_input(&self.ctx, &input);
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn frames(&mut self, editor: &mut Editor, count: usize) {
        for _ in 0..count {
            self.frame(editor, vec![]);
        }
    }

    fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::pointer(pos, egui::PointerButton::Primary, true)]);
        self.frame(editor, vec![Self::pointer(pos, egui::PointerButton::Primary, false)]);
        self.frames(editor, 2);
    }

    fn click_id(&mut self, editor: &mut Editor, id: egui::Id) {
        let pos = self.ctx.read_response(id).unwrap_or_else(|| panic!("the widget {id:?} is drawn")).rect.center();
        self.click(editor, pos);
    }

    fn drag(&mut self, editor: &mut Editor, button: egui::PointerButton, from: egui::Pos2, to: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![Self::pointer(from, button, true)]);
        for step in 1..=8 {
            let pos = from + (to - from) * (step as f32 / 8.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        self.frame(editor, vec![Self::pointer(to, button, false)]);
        self.frame(editor, vec![]);
    }

    /// Нажать и отпустить сочетание.
    fn press(&mut self, editor: &mut Editor, binding: KeyBinding) {
        for pressed in [true, false] {
            let event = egui::Event::Key {
                key: binding.key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: binding.modifiers(),
            };
            self.frame(editor, vec![event]);
        }
        self.frame(editor, vec![]);
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

    /// Есть ли в последнем кадре прямоугольник с заливкой `fill`.
    fn has_fill(&self, fill: egui::Color32) -> bool {
        fn search(shape: &egui::Shape, fill: egui::Color32) -> bool {
            match shape {
                egui::Shape::Rect(r) => r.fill == fill,
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| search(s, fill)),
                _ => false,
            }
        }
        self.shapes.iter().any(|c| search(&c.shape, fill))
    }
}

// ----- временный каталог ассетов -----

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "piska_prefs_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/scenes")).unwrap();
        Self { root }
    }

    fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn editor_over(assets: &Path, config: EditorConfig) -> Editor {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    app.world.scene.spawn("Hero");
    Editor::new(app, config).expect("the editor starts")
}

fn hero(editor: &Editor) -> GameObjectId {
    editor.app.world.scene.find_by_name("Hero").expect("Hero is in the scene")
}

fn key(key: egui::Key) -> KeyBinding {
    KeyBinding::key(key)
}

// ===========================================================================
// Shortcuts
// ===========================================================================

#[test]
fn every_default_shortcut_runs_its_command_through_the_registry() {
    let sandbox = Sandbox::new("defaults");
    let mut checked = 0;
    for def in COMMANDS {
        let Some(binding) = def.default else { continue };
        // Команды вкладки Code работают, только пока фокус во вкладке Code (и часть — из
        // двух нажатий): их нажатия проверяют тесты вкладки Code
        // (`crates/engine-editor/src/code_tests.rs`).
        if def.category == "Code Editor" || def.prefix.is_some() {
            continue;
        }
        let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
        let hero = hero(&editor);
        editor.apply(Action::Select(Some(hero)));
        let mut hands = Hands::new(&mut editor);
        let objects = editor.app.world.scene.len();
        hands.press(&mut editor, binding);
        assert_eq!(editor.last_shortcut_command(), Some(def.id), "{} ({binding}) runs `{}`", def.title, def.id);
        // Что именно сделала команда — для тех, у кого это видно снаружи.
        match def.id {
            "tool-move" => assert_eq!(editor.tool(), Tool::Move),
            "tool-rotate" => assert_eq!(editor.tool(), Tool::Rotate),
            "tool-scale" => assert_eq!(editor.tool(), Tool::Scale),
            "preferences" => assert!(editor.is_preferences_open()),
            "create-empty" | "create-empty-child" | "duplicate" => assert_eq!(editor.app.world.scene.len(), objects + 1),
            "delete" => assert!(editor.app.world.scene.find_by_name("Hero").is_none()),
            "select-none" => assert_eq!(editor.selection(), None),
            "play" => assert_eq!(editor.mode(), Mode::Playing),
            "quit" => assert!(editor.should_quit()),
            "save-as" => assert!(editor.has_dialog(), "Save As asks for a path"),
            "window-console" => assert!(editor.is_panel_open(Panel::Console)),
            _ => {}
        }
        checked += 1;
    }
    assert!(checked >= 30, "all default shortcuts were pressed ({checked})");
}

#[test]
fn a_rebound_shortcut_runs_shows_in_the_menu_and_survives_a_restart() {
    let sandbox = Sandbox::new("rebind");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    editor.show_settings_page(PREFS, "shortcuts");
    // Таблица длинная: поиск оставляет нужную строку на виду.
    editor.set_settings_search(PREFS, "play / stop");
    let mut hands = Hands::new(&mut editor);
    assert!(hands.find_text(&mut editor, "Play / Stop").is_some(), "the table lists the command");

    // Щелчок по сочетанию — запись; следующее нажатие становится сочетанием.
    hands.click_id(&mut editor, Editor::shortcut_button_id("play"));
    assert_eq!(editor.listening_shortcut(), Some("play"));
    assert!(hands.find_text(&mut editor, "Press keys…").is_some());
    hands.press(&mut editor, key(egui::Key::F5));
    assert_eq!(editor.listening_shortcut(), None);
    assert_eq!(editor.shortcut("play"), Some(key(egui::Key::F5)));
    assert_eq!(editor.mode(), Mode::Edit, "the key that was recorded does not also run the command");
    assert!(editor.settings_status(PREFS).unwrap().contains("F5"));
    editor.show_preferences(false);

    // Старое сочетание свободно, новое запускает игру и останавливает её.
    hands.press(&mut editor, KeyBinding::ctrl(egui::Key::P));
    assert_eq!(editor.mode(), Mode::Edit);
    hands.press(&mut editor, key(egui::Key::F5));
    assert_eq!(editor.mode(), Mode::Playing);
    hands.press(&mut editor, key(egui::Key::F5));
    assert_eq!(editor.mode(), Mode::Edit);

    // Меню Edit показывает текущее сочетание.
    let edit = hands.find_text(&mut editor, "Edit").expect("the Edit menu");
    hands.click(&mut editor, edit);
    assert!(hands.find_text(&mut editor, "F5").is_some(), "the menu shows the new shortcut");
    assert!(hands.find_text(&mut editor, "Ctrl+P").is_none(), "and not the old one");
    assert!(hands.find_text(&mut editor, "Ctrl+Z").is_some(), "other items keep theirs");

    // Перезапуск: настройки записываются текстом и читаются заново.
    let saved = editor.preferences().to_text();
    assert!(saved.contains("shortcut.play: F5"), "{saved}");
    drop(editor);
    let mut next = editor_over(&sandbox.assets(), EditorConfig::default());
    next.set_preferences(Preferences::parse(&saved));
    let mut hands = Hands::new(&mut next);
    assert_eq!(next.shortcut_text("play"), "F5");
    hands.press(&mut next, key(egui::Key::F5));
    assert_eq!(next.mode(), Mode::Playing);
}

#[test]
fn a_taken_shortcut_asks_before_it_is_moved() {
    let sandbox = Sandbox::new("conflict");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    editor.show_settings_page(PREFS, "shortcuts");
    editor.set_settings_search(PREFS, "quit");
    let mut hands = Hands::new(&mut editor);
    let ctrl_s = KeyBinding::ctrl(egui::Key::S);

    // Cancel — ничего не меняется.
    hands.click_id(&mut editor, Editor::shortcut_button_id("quit"));
    hands.press(&mut editor, ctrl_s);
    assert!(hands.find_text(&mut editor, "Ctrl+S is already used").is_some(), "the conflict is shown");
    hands.click_id(&mut editor, Editor::settings_cancel_id());
    assert_eq!(editor.shortcut("save"), Some(ctrl_s));
    assert_eq!(editor.shortcut("quit"), Some(KeyBinding::ctrl(egui::Key::Q)));

    // Replace — сочетание переходит, у прежней команды его больше нет.
    hands.click_id(&mut editor, Editor::shortcut_button_id("quit"));
    hands.press(&mut editor, ctrl_s);
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    assert_eq!(editor.shortcut("quit"), Some(ctrl_s));
    assert_eq!(editor.shortcut("save"), None);
    assert_eq!(editor.shortcut_text("save"), "");

    editor.show_preferences(false);
    hands.press(&mut editor, ctrl_s);
    assert_eq!(editor.last_shortcut_command(), Some("quit"));
    assert!(editor.should_quit());
}

#[test]
fn escape_cancels_recording_and_reset_brings_the_defaults_back() {
    let sandbox = Sandbox::new("reset");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    editor.show_settings_page(PREFS, "shortcuts");
    editor.set_settings_search(PREFS, "save");
    let mut hands = Hands::new(&mut editor);

    hands.click_id(&mut editor, Editor::shortcut_button_id("save"));
    hands.press(&mut editor, key(egui::Key::Escape));
    assert_eq!(editor.listening_shortcut(), None);
    assert_eq!(editor.shortcut("save"), Some(KeyBinding::ctrl(egui::Key::S)), "Esc keeps the old shortcut");

    // Крестик снимает сочетание; стрелка строки возвращает его.
    editor.set_settings_search(PREFS, "undo");
    hands.frames(&mut editor, 2);
    hands.click_id(&mut editor, Editor::shortcut_clear_id("undo"));
    assert_eq!(editor.shortcut("undo"), None);
    hands.click_id(&mut editor, Editor::settings_reset_id(PREFS, "Undo"));
    assert_eq!(editor.shortcut("undo"), Some(KeyBinding::ctrl(egui::Key::Z)));

    // Поиск находит команду по сочетанию.
    editor.set_settings_search(PREFS, "ctrl+shift+s");
    hands.frames(&mut editor, 2);
    // «ctrl+shift+s» — начало и `Ctrl+Shift+Space` (Parameter Info вкладки Code).
    assert_eq!(editor.settings_search_hits(PREFS), [("shortcuts", 2)]);
    assert_eq!(editor.settings_visible_rows(PREFS), ["Save As…", "Parameter Info"]);
    editor.set_settings_search(PREFS, "");

    // Reset page — все сочетания по умолчанию.
    editor.set_shortcut("play", Some(key(egui::Key::F5)));
    editor.set_shortcut("tool-move", None);
    hands.click_id(&mut editor, Editor::settings_reset_page_id(PREFS));
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    for def in COMMANDS {
        assert_eq!(editor.shortcut(def.id), def.default, "{}", def.id);
    }
}

// ===========================================================================
// General
// ===========================================================================

#[test]
fn undo_remembers_as_many_steps_as_the_preference_says() {
    let sandbox = Sandbox::new("undo");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    let mut prefs = editor.preferences().clone();
    prefs.undo_limit = 3;
    editor.set_preferences(prefs);
    let mut hands = Hands::new(&mut editor);
    for _ in 0..6 {
        editor.apply(Action::CreateEmpty { parent: None });
        hands.frames(&mut editor, 2);
    }
    let mut undone = 0;
    while editor.can_undo() && undone < 10 {
        editor.apply(Action::Undo);
        hands.frames(&mut editor, 1);
        undone += 1;
    }
    assert_eq!(undone, 3);
}

#[test]
fn the_scene_is_saved_before_play_and_after_the_auto_save_delay() {
    let sandbox = Sandbox::new("autosave");
    let file = sandbox.assets().join("scenes/auto.scene");
    let read = || std::fs::read_to_string(&file).unwrap();

    // Выключено — Play ничего не пишет.
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    editor.apply(Action::SaveAs("scenes/auto.scene".to_string()));
    let mut hands = Hands::new(&mut editor);
    let create = |editor: &mut Editor, hands: &mut Hands, name: &str| {
        editor.apply(Action::CreateEmpty { parent: None });
        let id = editor.selection().unwrap();
        editor.app.world.scene.get_mut(id).unwrap().name = name.to_string();
        hands.frames(editor, 2);
    };
    create(&mut editor, &mut hands, "BeforePlay");
    editor.apply(Action::Play);
    assert!(!read().contains("BeforePlay"));
    editor.apply(Action::Stop);

    // Save before Play.
    let mut prefs = editor.preferences().clone();
    prefs.autosave_on_play = true;
    editor.set_preferences(prefs);
    assert!(editor.is_dirty());
    editor.apply(Action::Play);
    assert!(read().contains("BeforePlay"), "the scene is written before the game starts");
    editor.apply(Action::Stop);

    // Автосохранение по времени: 0.01 мин = 0.6 с = 36 кадров.
    let mut prefs = editor.preferences().clone();
    prefs.autosave_on_play = false;
    prefs.autosave_minutes = 0.01;
    editor.set_preferences(prefs);
    create(&mut editor, &mut hands, "Autosaved");
    hands.frames(&mut editor, 18);
    assert!(editor.is_dirty(), "not yet");
    assert!(!read().contains("Autosaved"));
    hands.frames(&mut editor, 30);
    assert!(!editor.is_dirty(), "saved after the delay");
    assert!(read().contains("Autosaved"));
}

#[test]
fn the_external_code_editor_command_is_started_and_errors_are_reported() {
    let sandbox = Sandbox::new("external");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    let file = sandbox.assets().join("scenes");
    assert!(editor.open_in_external_editor(&file, 1).is_err(), "nothing is set: the Code tab is used");

    let mut prefs = editor.preferences().clone();
    prefs.external_editor = "cargo --version".to_string();
    editor.set_preferences(prefs);
    editor.open_in_external_editor(&file, 3).expect("an existing program starts");

    // Кнопка Test на странице General пишет итог в строку состояния.
    let mut prefs = editor.preferences().clone();
    prefs.external_editor = "piska-no-such-editor {file}".to_string();
    editor.set_preferences(prefs);
    editor.show_settings_page(PREFS, "general");
    let mut hands = Hands::new(&mut editor);
    hands.click_id(&mut editor, Editor::external_editor_test_id());
    let status = editor.settings_status(PREFS).unwrap();
    assert!(status.contains("cannot start the external editor `piska-no-such-editor`"), "{status}");
}

// ===========================================================================
// Appearance, Scene View
// ===========================================================================

#[test]
fn play_mode_tint_strength_reaches_the_toolbar() {
    use engine::editor::kit::theme;
    let sandbox = Sandbox::new("tint");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    let mut hands = Hands::new(&mut editor);
    editor.apply(Action::Play);
    hands.frames(&mut editor, 2);
    assert!(hands.has_fill(theme::play_tint(1.0)));
    let mut prefs = editor.preferences().clone();
    prefs.play_tint = 0.3;
    editor.set_preferences(prefs);
    hands.frames(&mut editor, 2);
    assert!(hands.has_fill(theme::play_tint(0.3)));
    assert!(!hands.has_fill(theme::play_tint(1.0)));
}

#[test]
fn grid_and_background_colours_reach_the_scene_view() {
    let sandbox = Sandbox::new("grid");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    let wall = editor.scene_grid().expect("the grid is on");
    let mut prefs = editor.preferences().clone();
    prefs.grid_color = egui::Color32::from_rgb(255, 0, 0);
    prefs.grid_opacity = 0.5;
    prefs.scene_background = egui::Color32::from_rgb(0, 0, 255);
    editor.set_preferences(prefs);
    let grid = editor.scene_grid().unwrap();
    assert_eq!(grid.color, [1.0, 0.0, 0.0]);
    assert!((grid.opacity - wall.opacity * 0.5).abs() < 1e-6);
    assert_eq!(editor.scene_background(), [0.0, 0.0, 1.0]);
    editor.set_scene_3d(true);
    assert_eq!(editor.scene_grid().unwrap().color, [1.0, 0.0, 0.0], "the 3D ground grid too");
    editor.set_grid_visible(false);
    assert!(editor.scene_grid().is_none());
}

#[test]
fn the_move_step_of_snapping_comes_from_the_preferences() {
    let sandbox = Sandbox::new("snap");
    let mut moved = Vec::new();
    for step in [0.5_f32, 2.0] {
        let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
        let mut prefs = editor.preferences().clone();
        prefs.snap_move = step;
        editor.set_preferences(prefs);
        editor.set_snapping(true);
        let hero = hero(&editor);
        editor.apply(Action::Select(Some(hero)));
        let mut hands = Hands::new(&mut editor);
        let origin = editor.world_to_screen(Vec2::ZERO);
        let units = editor.world_to_screen(Vec2::new(2.7, 0.0)).x - origin.x;
        let grab = origin + egui::vec2(40.0, 0.0);
        hands.drag(&mut editor, egui::PointerButton::Primary, grab, grab + egui::vec2(units, 0.0));
        moved.push(editor.app.world.scene.get(hero).unwrap().transform.position.x);
    }
    assert!((moved[0] - 2.5).abs() < 1e-4, "0.5 m steps: {}", moved[0]);
    assert!((moved[1] - 2.0).abs() < 1e-4, "2 m steps: {}", moved[1]);
}

#[test]
fn orbit_speed_and_inversion_change_how_the_3d_camera_turns() {
    let sandbox = Sandbox::new("orbit");
    let mut turns = Vec::new();
    for (speed, invert) in [(1.0_f32, false), (2.0, true)] {
        let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
        let mut prefs = editor.preferences().clone();
        prefs.orbit_speed = speed;
        prefs.invert_orbit_y = invert;
        editor.set_preferences(prefs);
        editor.set_scene_3d(true);
        editor.set_orbit(Orbit3D::default());
        let mut hands = Hands::new(&mut editor);
        let start = editor.orbit();
        let center = editor.viewport_rect().center();
        hands.drag(&mut editor, egui::PointerButton::Secondary, center, center + egui::vec2(80.0, 40.0));
        let end = editor.orbit();
        turns.push((end.yaw - start.yaw, end.pitch - start.pitch));
    }
    let (yaw1, pitch1) = turns[0];
    let (yaw2, pitch2) = turns[1];
    assert!(yaw1 < 0.0 && pitch1 > 0.0, "a drag to the right and down turns the camera: {turns:?}");
    assert!((yaw2 / yaw1 - 2.0).abs() < 0.05, "twice the speed turns twice as far: {turns:?}");
    assert!((pitch2 / pitch1 + 2.0).abs() < 0.05, "inverted and twice as fast vertically: {turns:?}");
}

/// Обзор правой кнопкой крутится сырым движением мыши: один счёт датчика — 0.003 рад (как в
/// Unity). Сдвиг курсора при этом второй раз не прибавляется, глаз стоит на месте, а дрожь
/// руки в один счёт туда-сюда качает вид на те же 0.003 рад и возвращает его точно назад —
/// раньше шаг курсора в один пиксель давал 0.008 рад, около восьми пикселей картинки.
#[test]
fn right_button_look_follows_raw_mouse_counts_and_does_not_amplify_jitter() {
    let sandbox = Sandbox::new("look_raw");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    editor.set_scene_3d(true);
    editor.set_orbit(Orbit3D::default());
    let mut hands = Hands::new(&mut editor);
    let center = editor.viewport_rect().center();
    let start = editor.orbit();
    let eye = start.eye();

    hands.frame(&mut editor, vec![egui::Event::PointerMoved(center)]);
    hands.frame(&mut editor, vec![Hands::pointer(center, egui::PointerButton::Secondary, true)]);
    // Мышь едет вправо: 10 счётов за кадр; курсор системы при этом тоже едет (на 7 точек).
    let mut pos = center;
    for _ in 0..12 {
        pos += egui::vec2(7.0, 0.0);
        editor.add_raw_mouse_motion(10.0, 0.0);
        hands.frame(&mut editor, vec![egui::Event::PointerMoved(pos)]);
    }
    let turned = editor.orbit();
    let yaw = turned.yaw - start.yaw;
    // 12 кадров по 10 счётов = 0.36 рад; первые кадр-два, пока egui решает, что это
    // перетаскивание, могут не считаться. Со сдвигом курсора вдобавок вышло бы больше 0.6.
    assert!((-0.3601..=-0.2999).contains(&yaw), "raw counts turn the camera by 0.003 rad each: {yaw}");
    assert!((turned.pitch - start.pitch).abs() < 1e-5, "no vertical motion: {}", turned.pitch - start.pitch);
    assert!((turned.eye() - eye).length() < 1e-3, "the eye stays in place: {:?} -> {:?}", eye, turned.eye());

    // Дрожь: счёт вправо, счёт влево; курсор на месте.
    let (mut lo, mut hi) = (turned.yaw, turned.yaw);
    for i in 0..20 {
        editor.add_raw_mouse_motion(if i % 2 == 0 { 1.0 } else { -1.0 }, 0.0);
        hands.frame(&mut editor, vec![egui::Event::PointerMoved(pos)]);
        lo = lo.min(editor.orbit().yaw);
        hi = hi.max(editor.orbit().yaw);
    }
    assert!(hi - lo <= 0.0031, "one count of jitter is one small step, not a jump: {}", hi - lo);
    assert!((editor.orbit().yaw - turned.yaw).abs() < 1e-5, "and it comes back exactly: {} vs {}", editor.orbit().yaw, turned.yaw);
    assert!((editor.orbit().eye() - eye).length() < 1e-3, "the eye still has not moved");

    // Кнопку отпустили: сырое движение больше не крутит вид.
    hands.frame(&mut editor, vec![Hands::pointer(pos, egui::PointerButton::Secondary, false)]);
    hands.frame(&mut editor, vec![]);
    let released = editor.orbit().yaw;
    editor.add_raw_mouse_motion(50.0, 0.0);
    hands.frame(&mut editor, vec![]);
    hands.frame(&mut editor, vec![]);
    assert!((editor.orbit().yaw - released).abs() < 1e-6, "raw motion without the button does nothing");
}

// ===========================================================================
// Console & Diagnostics
// ===========================================================================

#[test]
fn console_defaults_limits_and_clear_on_play_follow_the_preferences() {
    let sandbox = Sandbox::new("console");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    assert_eq!(editor.console_toggles(), [true, true, true, false, false]);
    let mut prefs = editor.preferences().clone();
    prefs.console_warn = false;
    prefs.console_collapse = true;
    prefs.console_error_pause = true;
    prefs.console_max = 50;
    editor.set_preferences(prefs.clone());
    assert_eq!(editor.console_toggles(), [true, false, true, true, true]);

    let mut hands = Hands::new(&mut editor);
    for i in 0..120 {
        log::info!("console limit test line {i}");
    }
    hands.frames(&mut editor, 2);
    assert_eq!(editor.console_message_count(), 50, "only the newest 50 are kept");

    prefs.console_clear_on_play = true;
    editor.set_preferences(prefs);
    editor.apply(Action::Play);
    assert!(editor.console_message_count() < 10, "Play starts with an empty Console");

    // Страница: снять флажок — Console меняется сразу.
    editor.apply(Action::Stop);
    editor.show_settings_page(PREFS, "console");
    hands.frames(&mut editor, 2);
    hands.click_id(&mut editor, Editor::settings_reset_id(PREFS, "Show warnings"));
    assert!(editor.preferences().console_warn);
    assert!(editor.console_toggles()[1]);
}

// ===========================================================================
// Build & Tools
// ===========================================================================

#[test]
fn tool_paths_and_the_export_folder_reach_the_export_command() {
    let sandbox = Sandbox::new("tools");
    let config = EditorConfig::default().game_crate("demo").workspace(sandbox.root.clone());
    let mut editor = editor_over(&sandbox.assets(), config);
    let web = ExportSettings { platform: ExportPlatform::Web, ..ExportSettings::default() };
    assert_eq!(editor.export_out_dir(&web).unwrap(), sandbox.root.join("dist/demo/web"));
    let plain = editor.export_command(&web).unwrap();
    assert_eq!(plain.program, "cargo");
    assert!(plain.env.is_empty());

    let mut prefs = editor.preferences().clone();
    prefs.export_folder = "builds".to_string();
    prefs.tool_cargo = "my-cargo".to_string();
    prefs.tool_android_sdk = "X:/android/sdk".to_string();
    prefs.tool_java = "X:/jdk".to_string();
    prefs.tool_zig = "X:/zig/zig.exe".to_string();
    editor.set_preferences(prefs);
    assert_eq!(editor.export_out_dir(&web).unwrap(), sandbox.root.join("builds/demo/web"));
    let command = editor.export_command(&web).unwrap();
    assert_eq!(command.program, "my-cargo");
    let env = |key: &str| command.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    assert_eq!(env("ANDROID_HOME"), Some("X:/android/sdk"));
    assert_eq!(env("ANDROID_SDK_ROOT"), Some("X:/android/sdk"));
    assert_eq!(env("JAVA_HOME"), Some("X:/jdk"));
    assert_eq!(env("PISKA_ZIG"), Some("X:/zig/zig.exe"));
    assert_eq!(env("ANDROID_NDK_HOME"), None, "empty fields add nothing");
    let out = editor.export_out_dir(&web).unwrap().display().to_string();
    assert!(command.args.contains(&out), "the export goes into the folder: {:?}", command.args);
}

#[test]
fn detect_and_validate_buttons_check_the_tools() {
    let sandbox = Sandbox::new("validate");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    editor.show_settings_page(PREFS, "build-tools");
    let mut hands = Hands::new(&mut editor);

    // cargo есть на машине, где идут тесты: Detect находит, Validate говорит версию.
    hands.click_id(&mut editor, Editor::tool_detect_id("cargo"));
    let found = editor.preferences().tool_cargo.clone();
    assert!(Path::new(&found).is_file(), "Detect fills in the cargo path: {found}");
    hands.click_id(&mut editor, Editor::tool_validate_id("cargo"));
    let (text, error) = editor.tool_check("cargo").unwrap();
    assert!(!error && text.starts_with("cargo "), "{text}");

    // Папка без SDK — ошибка с подсказкой.
    let mut prefs = editor.preferences().clone();
    prefs.tool_android_sdk = sandbox.root.display().to_string();
    editor.set_preferences(prefs);
    // Курсор прочь: подсказка прошлой кнопки не должна закрывать следующую строку.
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(egui::pos2(4.0, 840.0))]);
    hands.frames(&mut editor, 3);
    hands.click_id(&mut editor, Editor::tool_validate_id("android-sdk"));
    let (text, error) = editor.tool_check("android-sdk").unwrap();
    assert!(error && text.contains("platform-tools"), "{text}");

    // Стрелка сброса очищает путь.
    hands.click_id(&mut editor, Editor::settings_reset_id(PREFS, "Android SDK"));
    assert!(editor.preferences().tool_android_sdk.is_empty());
}

// ===========================================================================
// Снимки страниц (нужен GPU)
// ===========================================================================

/// `ENGINE_SHOTS_DIR=out cargo test -p engine --features editor --test editor_prefs -- --ignored gallery`
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn preferences_screenshot_gallery() {
    use engine::editor::capture_with_input;
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let sandbox = Sandbox::new("gallery");
    let mut editor = editor_over(&sandbox.assets(), EditorConfig::default());
    let mut prefs = editor.preferences().clone();
    prefs.external_editor = "code -g {file}:{line}".to_string();
    prefs.autosave_minutes = 5.0;
    prefs.snap_move = 0.25;
    prefs.console_collapse = true;
    prefs.export_folder = "builds".to_string();
    editor.set_preferences(prefs);
    editor.set_shortcut("play", Some(KeyBinding::key(egui::Key::F5)));
    for (id, _) in Editor::settings_pages(PREFS) {
        editor.show_settings_page(PREFS, id);
        engine::editor::capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join(format!("prefs_{id}.png"))).unwrap();
    }
    // Запись сочетания и вопрос о занятом сочетании.
    editor.show_settings_page(PREFS, "shortcuts");
    editor.listen_shortcut(Some("quit"));
    engine::editor::capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("prefs_shortcuts_listen.png")).unwrap();
    editor.listen_shortcut(None);
    editor.listen_shortcut(Some("quit"));
    let ctrl_s = KeyBinding::ctrl(egui::Key::S);
    let image = capture_with_input(&mut editor, 1440, 860, 14, |frame, _| match frame {
        4 => vec![egui::Event::Key { key: ctrl_s.key, physical_key: None, pressed: true, repeat: false, modifiers: ctrl_s.modifiers() }],
        _ => vec![],
    })
    .unwrap();
    image.save(dir.join("prefs_shortcuts_conflict.png")).unwrap();
}
