//! Всплывающие окна редактора двигаются за заголовок настоящей мышью: сдвиг на (+120, +80),
//! место остаётся после закрытия и нового запуска (Build, Undo History — через Preferences),
//! за край окна программы заголовок не уходит, двойной щелчок по заголовку — снова по
//! центру, Esc закрывает верхнее окно.
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::egui::Id;
use engine::editor::kit::window::last_layout;
use engine::editor::{Action, Editor, EditorConfig};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

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
        let _ = self.ctx.run(input, |ctx| editor.ui(ctx));
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![Self::button(from, true)]);
        for step in 1..=8 {
            self.frame(editor, vec![egui::Event::PointerMoved(from + (to - from) * (step as f32 / 8.0))]);
        }
        self.frame(editor, vec![Self::button(to, false)]);
        self.frame(editor, vec![]);
    }

    fn double_click(&mut self, editor: &mut Editor, at: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(at)]);
        for _ in 0..2 {
            self.frame(editor, vec![Self::button(at, true)]);
            self.frame(editor, vec![Self::button(at, false)]);
        }
        self.frame(editor, vec![]);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        self.frame(editor, vec![event(true)]);
        self.frame(editor, vec![event(false)]);
        self.frame(editor, vec![]);
    }

    /// Рамка окна и середина его заголовка (место, за которое тянут).
    fn window(&self, id: Id) -> (egui::Rect, egui::Pos2) {
        // Раскладка окна этого кадра (ответ виджета заголовка может остаться от прохода
        // примерки размера, когда окно стояло иначе).
        let layout = last_layout(&self.ctx, id).unwrap_or_else(|| panic!("window {id:?} is drawn"));
        let title = layout.title;
        // Тянуть левее середины: справа в заголовке кнопка «×».
        (layout.frame, egui::pos2(title.left() + title.width() * 0.35, title.center().y))
    }
}

/// Id окон в kit (как их строит `tool_window(Id::new(…))`).
fn build_id() -> Id {
    Id::new(Id::new("editor_export"))
}

fn undo_id() -> Id {
    Id::new(Id::new("undo_history_window"))
}

fn open(editor: &mut Editor, id: Id, show: bool) {
    if id == build_id() {
        editor.apply(Action::ShowBuildDialog(show));
    } else {
        editor.show_undo_history(show);
    }
}

#[test]
fn windows_move_by_their_title_and_remember_the_place() {
    for (name, id) in [("build", build_id()), ("undo_history", undo_id())] {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        open(&mut editor, id, true);
        hands.frame(&mut editor, vec![]);
        hands.frame(&mut editor, vec![]);
        let (before, grab) = hands.window(id);
        hands.drag(&mut editor, grab, grab + egui::vec2(120.0, 80.0));
        let (after, _) = hands.window(id);
        assert!((after.min - before.min - egui::vec2(120.0, 80.0)).length() < 4.0, "{name}: {before:?} → {after:?}");

        // Закрыть и открыть — то же место; в Preferences записано.
        open(&mut editor, id, false);
        hands.frame(&mut editor, vec![]);
        open(&mut editor, id, true);
        hands.frame(&mut editor, vec![]);
        hands.frame(&mut editor, vec![]);
        let (again, _) = hands.window(id);
        assert!((again.min - after.min).length() < 2.0, "{name}: reopened at {again:?}, left at {after:?}");
        let stored = editor.preferences().window_offsets.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert!(stored.is_some_and(|[x, y]| (x - 120.0).abs() < 2.0 && (y - 80.0).abs() < 2.0), "{name}: stored {stored:?}");

        // Новый запуск с теми же настройками — то же место.
        let prefs = editor.preferences().clone();
        let mut next = self::editor();
        next.set_preferences(prefs);
        let mut fresh = Hands::new(&mut next);
        open(&mut next, id, true);
        fresh.frame(&mut next, vec![]);
        fresh.frame(&mut next, vec![]);
        let (restored, grab) = fresh.window(id);
        assert!((restored.min - after.min).length() < 2.0, "{name}: next start at {restored:?}, left at {after:?}");

        // Далеко за край — заголовок остаётся на экране.
        fresh.drag(&mut next, grab, grab + egui::vec2(3000.0, 2000.0));
        let (clamped, _) = fresh.window(id);
        let title = last_layout(&fresh.ctx, id).unwrap().title;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN);
        assert!(screen.intersect(title).width() >= 40.0 && title.bottom() <= screen.bottom(), "{name}: the title bar {title:?} stays on screen");
        assert!(clamped.left() < screen.right(), "{name}: {clamped:?}");

        // Двойной щелчок по заголовку — снова по центру.
        let (_, grab) = fresh.window(id);
        fresh.double_click(&mut next, grab);
        let (centred, _) = fresh.window(id);
        assert!((centred.center().x - screen.center().x).abs() < 4.0, "{name}: double-click centres the window: {centred:?}");

        // Esc закрывает верхнее окно.
        fresh.key(&mut next, egui::Key::Escape);
        assert!(!window_open(&next, id), "{name}: Esc closes the window");
    }
}

fn window_open(editor: &Editor, id: Id) -> bool {
    if id == build_id() {
        editor.is_build_dialog_open()
    } else {
        editor.is_undo_history_open()
    }
}
