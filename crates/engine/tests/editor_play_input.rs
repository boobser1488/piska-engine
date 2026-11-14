//! Ввод в режиме Play, настоящими событиями egui, без GPU: игра получает клавиатуру,
//! мышь и геймпады, только пока фокус у окна Game (как в Unity). Щелчок по Scene,
//! Shift+Esc, Pause и Stop отдают ввод редактору, ничего не «залипает».
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, LayoutPreset, Mode, Panel, Tool};
use engine::prelude::*;
use engine_core::input::{GamepadButton, GamepadId, Key, MouseButton};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const NONE: egui::Modifiers = egui::Modifiers::NONE;

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap()
}

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0 };
        for _ in 0..3 {
            hands.frame(editor, vec![], NONE);
        }
        hands
    }

    /// Кадр как в окне: ввод игре → кадр игры → интерфейс.
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
        // Кадр игры не идёт здесь: тест смотрит состояние ввода, которое увидит игра.
        let _ = self.ctx.run(input, |ctx| editor.ui(ctx));
    }

    fn key_event(key: egui::Key, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    /// Нажать и держать.
    fn hold(&mut self, editor: &mut Editor, key: egui::Key) {
        self.frame(editor, vec![Self::key_event(key, true, NONE)], NONE);
    }

    fn release(&mut self, editor: &mut Editor, key: egui::Key) {
        self.frame(editor, vec![Self::key_event(key, false, NONE)], NONE);
    }

    fn press(&mut self, editor: &mut Editor, key: egui::Key, modifiers: egui::Modifiers) {
        self.frame(editor, vec![Self::key_event(key, true, modifiers)], modifiers);
        self.frame(editor, vec![Self::key_event(key, false, modifiers)], modifiers);
        self.frame(editor, vec![], NONE);
    }

    /// Нажать левую кнопку в точке (и не отпускать): что увидит игра в этом кадре.
    fn mouse_down(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)], NONE);
        let button = egui::PointerButton::Primary;
        self.frame(editor, vec![egui::Event::PointerButton { pos, button, pressed: true, modifiers: NONE }], NONE);
    }

    fn mouse_up(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        let button = egui::PointerButton::Primary;
        self.frame(editor, vec![egui::Event::PointerButton { pos, button, pressed: false, modifiers: NONE }], NONE);
        self.frame(editor, vec![], NONE);
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.mouse_down(editor, pos);
        self.mouse_up(editor, pos);
    }
}

fn tab(editor: &Editor, panel: Panel) -> egui::Pos2 {
    editor.panel_tab_rect(panel).unwrap_or_else(|| panic!("{panel:?} has a tab")).center()
}

#[test]
fn keys_reach_the_game_only_while_the_game_view_has_focus() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    editor.apply(Action::Play);
    hands.frame(&mut editor, vec![], NONE);
    assert_eq!(editor.focused_panel(), Some(Panel::Game), "Play gives the Game view the focus");
    assert!(editor.game_has_input_focus());

    hands.hold(&mut editor, egui::Key::D);
    assert!(editor.app.world.input.key(Key::D), "the game sees D while the Game view has focus");

    // Пользователь щёлкает по вкладке Scene (Play идёт): D отпускается сам, новые клавиши
    // игре не достаются, а буквы снова — инструменты редактора.
    let scene_tab = tab(&editor, Panel::Scene);
    hands.click(&mut editor, scene_tab);
    assert_eq!(editor.focused_panel(), Some(Panel::Scene));
    assert!(!editor.game_has_input_focus());
    assert!(!editor.app.world.input.key(Key::D), "a key held when the focus left is released, not stuck");
    hands.hold(&mut editor, egui::Key::W);
    assert!(!editor.app.world.input.key(Key::W), "keys typed in the Scene view do not reach the game");
    hands.release(&mut editor, egui::Key::W);
    hands.press(&mut editor, egui::Key::E, NONE);
    assert_eq!(editor.tool(), Tool::Rotate, "E is the editor's Rotate tool while the Scene view has focus");

    // Вкладка Game снова: ввод опять у игры.
    let game_tab = tab(&editor, Panel::Game);
    hands.click(&mut editor, game_tab);
    assert!(editor.game_has_input_focus());
    hands.hold(&mut editor, egui::Key::A);
    assert!(editor.app.world.input.key(Key::A));
    hands.press(&mut editor, egui::Key::R, NONE);
    assert_eq!(editor.tool(), Tool::Rotate, "with the game focused, R belongs to the game, not the Scale tool");

    // Shift+Esc отдаёт ввод редактору, щелчок по виду Game возвращает.
    let shift = egui::Modifiers::SHIFT;
    hands.press(&mut editor, egui::Key::Escape, shift);
    assert!(!editor.game_has_input_focus(), "Shift+Esc releases the game input");
    assert!(!editor.app.world.input.key(Key::A));
    assert_eq!(editor.mode(), Mode::Playing, "the game keeps running");
    let inside = editor.game_view_rect().center();
    hands.mouse_down(&mut editor, inside);
    assert!(editor.game_has_input_focus(), "a click on the Game view gives the input back");
    assert!(
        editor.app.world.input.mouse_button(MouseButton::Left),
        "and that click is already the game's"
    );
    hands.mouse_up(&mut editor, inside);

    // Pause и Stop тоже отпускают всё.
    hands.hold(&mut editor, egui::Key::D);
    assert!(editor.app.world.input.key(Key::D));
    editor.apply(Action::Pause);
    hands.frame(&mut editor, vec![], NONE);
    assert!(!editor.game_has_input_focus());
    assert!(!editor.app.world.input.key(Key::D), "Pause releases held keys");
    editor.apply(Action::Pause);
    hands.frame(&mut editor, vec![], NONE);
    assert!(editor.game_has_input_focus(), "resuming gives the input back to the focused Game view");
    editor.apply(Action::Stop);
    hands.frame(&mut editor, vec![], NONE);
    assert!(!editor.game_has_input_focus());
}

#[test]
fn with_scene_and_game_side_by_side_the_scene_view_keeps_mouse_keys_and_gamepads() {
    let mut editor = editor();
    editor.set_layout(LayoutPreset::TwoByThree);
    let mut hands = Hands::new(&mut editor);
    editor.apply(Action::Play);
    hands.frame(&mut editor, vec![], NONE);
    assert!(editor.is_panel_visible(Panel::Scene) && editor.is_panel_visible(Panel::Game));
    assert!(editor.game_has_input_focus());

    // Геймпад нажат, пока ввод у игры.
    let pad = GamepadId(0);
    editor.app.world.input.set_gamepad_button(pad, GamepadButton::South, true);
    assert!(editor.app.world.input.gamepad_button(pad, GamepadButton::South));

    // Щелчок по виду Scene: игре не достаётся ни сам щелчок, ни клавиши, ни геймпад.
    let scene = editor.panel_rect(Panel::Scene).expect("the Scene view is drawn").center();
    hands.mouse_down(&mut editor, scene);
    assert!(!editor.app.world.input.mouse_button(MouseButton::Left), "a click in the Scene view is not the game's");
    hands.mouse_up(&mut editor, scene);
    assert_eq!(editor.focused_panel(), Some(Panel::Scene));
    assert!(!editor.game_has_input_focus());
    assert!(!editor.app.world.input.gamepad_button(pad, GamepadButton::South), "the held pad button is released");
    hands.hold(&mut editor, egui::Key::A);
    assert!(!editor.app.world.input.key(Key::A));
    hands.release(&mut editor, egui::Key::A);
    editor.app.world.input.set_gamepad_button(pad, GamepadButton::East, true);
    assert!(
        !editor.app.world.input.gamepad_button(pad, GamepadButton::East),
        "gamepads go to the game only while the Game view has focus"
    );
    editor.app.world.input.set_gamepad_button(pad, GamepadButton::East, false);

    // Щелчок по виду Game — всё снова у игры.
    let game = editor.game_view_rect().center();
    hands.click(&mut editor, game);
    assert!(editor.game_has_input_focus());
    editor.app.world.input.set_gamepad_button(pad, GamepadButton::East, true);
    assert!(editor.app.world.input.gamepad_button(pad, GamepadButton::East));
    editor.apply(Action::Stop);
}

#[test]
fn focus_game_view_on_play_can_be_turned_off() {
    let mut editor = editor();
    editor.set_layout(LayoutPreset::TwoByThree);
    let mut prefs = editor.preferences().clone();
    assert!(prefs.focus_game_on_play, "on by default, as in Unity");
    prefs.focus_game_on_play = false;
    editor.set_preferences(prefs.clone());
    editor.set_layout(LayoutPreset::TwoByThree);
    let mut hands = Hands::new(&mut editor);
    let scene = editor.panel_rect(Panel::Scene).expect("the Scene view is drawn").center();
    hands.click(&mut editor, scene);
    assert_eq!(editor.focused_panel(), Some(Panel::Scene));

    editor.apply(Action::Play);
    hands.frame(&mut editor, vec![], NONE);
    assert_eq!(editor.focused_panel(), Some(Panel::Scene), "Play leaves the focus where it was");
    assert!(!editor.game_has_input_focus());
    hands.hold(&mut editor, egui::Key::D);
    assert!(!editor.app.world.input.key(Key::D));
    editor.apply(Action::Stop);

    // Настройка переживает запись и чтение файла.
    let text = prefs.to_text();
    assert!(!engine::editor::Preferences::parse(&text).focus_game_on_play);
}

component! {
    /// Взгляд мышью, как у контроллера от первого лица: просит захват курсора при старте
    /// и поворачивается сырым движением мыши, только пока курсор захвачен.
    pub struct Looker {
        pub yaw: f32 = 0.0,
    }
}

impl Component for Looker {
    fn on_start(&mut self, ctx: &mut Ctx) {
        ctx.world.input.set_cursor_captured(true);
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if ctx.input().is_cursor_captured() {
            self.yaw += ctx.input().mouse_delta().x;
        }
    }
}

fn yaw(editor: &Editor) -> f32 {
    let scene = &editor.app.world.scene;
    let id = scene.find_by_name("Eye").expect("the looker exists");
    scene.get_component::<Looker>(id).expect("it has a Looker").yaw
}

#[test]
fn cursor_capture_is_granted_only_to_the_focused_game_view_and_shift_esc_gives_the_mouse_back() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    app.register::<Looker>();
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let mut editor = Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap();
    editor.app.world.scene.build("Eye").with(Looker::default());
    // Scene и Game рядом; Play не трогает фокус — он остаётся на Scene.
    let mut prefs = editor.preferences().clone();
    prefs.focus_game_on_play = false;
    editor.set_preferences(prefs);
    editor.set_layout(LayoutPreset::TwoByThree);
    let mut hands = Hands::new(&mut editor);
    let scene = editor.panel_rect(Panel::Scene).unwrap().center();
    hands.click(&mut editor, scene);
    editor.apply(Action::Play);
    let tick = |hands: &mut Hands, editor: &mut Editor| {
        hands.frame(editor, vec![], NONE);
        editor.advance(DT);
    };
    for _ in 0..3 {
        tick(&mut hands, &mut editor);
    }
    assert!(editor.app.world.input.cursor_capture_requested(), "the game asks for the cursor");
    assert!(!editor.game_cursor_captured(), "not granted while the Scene view has focus");
    editor.add_raw_mouse_motion(50.0, 0.0);
    tick(&mut hands, &mut editor);
    assert_eq!(yaw(&editor), 0.0, "mouse motion does not reach the game");

    // Щелчок по виду Game — захват дан, сырое движение поворачивает взгляд.
    let game = editor.game_view_rect().center();
    hands.click(&mut editor, game);
    tick(&mut hands, &mut editor);
    assert!(editor.game_cursor_captured(), "granted after a click in the Game view");
    editor.add_raw_mouse_motion(40.0, 0.0);
    editor.advance(DT);
    assert_eq!(yaw(&editor), 40.0, "raw mouse deltas turn the view");

    // Shift+Esc — мышь редактору, взгляд больше не крутится; просьба игры остаётся.
    hands.press(&mut editor, egui::Key::Escape, egui::Modifiers::SHIFT);
    assert!(!editor.game_cursor_captured(), "Shift+Esc releases the cursor");
    assert!(editor.app.world.input.cursor_capture_requested(), "the game's request is kept");
    editor.add_raw_mouse_motion(40.0, 0.0);
    tick(&mut hands, &mut editor);
    assert_eq!(yaw(&editor), 40.0, "the view stops turning");

    // Снова щелчок — снова захват; открытое меню поверх вида снимает его.
    hands.click(&mut editor, game);
    tick(&mut hands, &mut editor);
    assert!(editor.game_cursor_captured(), "a click re-grants the still requested capture");
    editor.show_scene_picker(true);
    tick(&mut hands, &mut editor);
    tick(&mut hands, &mut editor);
    assert!(!editor.game_cursor_captured(), "a modal window over the Game view releases the cursor");
    editor.show_scene_picker(false);

    // Stop — ничего не остаётся: ни захвата, ни просьбы.
    editor.apply(Action::Stop);
    tick(&mut hands, &mut editor);
    assert!(!editor.game_cursor_captured());
    assert!(!editor.app.world.input.cursor_capture_requested());
    assert!(!editor.app.world.input.is_cursor_captured());
}
