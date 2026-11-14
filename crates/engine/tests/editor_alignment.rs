//! «Линейка» выравнивания редактора без GPU: прямоугольники виджетов из кадра egui.
//! Группа Play / Pause / Step ровно по центру панели, одинаковые полосы вкладок, одна
//! колонка значений в инспекторе, и инспектор никогда не шире своего окна — ни для
//! одного зарегистрированного компонента, ни при ширине окна от минимальной до 1200 px,
//! ни при масштабе интерфейса 80–200 %.
#![cfg(feature = "editor")]

use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard};

use engine::editor::egui;
use engine::editor::kit::i18n::{self, Language};
use engine::editor::{Action, Editor, EditorConfig, Panel, Preferences};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Язык процесса общий: каждая проверка идёт под замком и на своём языке (английский и
/// русский — русские подписи длиннее, и ничто не должно вылезать из окон).
static LANGUAGE: Mutex<()> = Mutex::new(());

struct Session {
    editor: Editor,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for Session {
    type Target = Editor;
    fn deref(&self) -> &Editor {
        &self.editor
    }
}

impl DerefMut for Session {
    fn deref_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        i18n::set(Language::English);
    }
}

fn editor_in(language: Language) -> Session {
    let guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    i18n::set(language);
    Session { editor: editor_inner(), _guard: guard }
}

fn editor_inner() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap()
}

/// Прогнать `n` кадров на экране `size` (точки при масштабе 100 %).
fn frames(editor: &mut Editor, ctx: &egui::Context, size: egui::Vec2, n: usize) {
    let zoom = editor.preferences().ui_scale;
    for _ in 0..n {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size / zoom)),
            ..egui::RawInput::default()
        };
        let _ = ctx.run(input, |ctx| editor.ui(ctx));
    }
}

fn the_play_group_is_centred_in_the_toolbar_at_every_width(language: Language) {
    for width in [960.0, 1280.0, 1440.0, 1920.0, 2560.0] {
        let mut editor = editor_in(language);
        let ctx = egui::Context::default();
        frames(&mut editor, &ctx, egui::vec2(width, 860.0), 4);
        let chrome = editor.chrome_rects();
        let offset = chrome.transport.center().x - chrome.toolbar.center().x;
        assert!(offset.abs() <= 1.0, "at {width} px the Play group is {offset:+.1} px off centre");
        assert!(chrome.transport.height() <= chrome.toolbar.height());
    }
}

fn neighbouring_tab_strips_have_one_height(language: Language) {
    let mut editor = editor_in(language);
    let ctx = egui::Context::default();
    frames(&mut editor, &ctx, egui::vec2(1440.0, 860.0), 4);
    let heights: Vec<(Panel, f32)> = Panel::ALL
        .into_iter()
        .filter_map(|p| Some((p, editor.panel_tab_rect(p)?.height())))
        .collect();
    assert!(heights.len() >= 4);
    let first = heights[0].1;
    for (panel, height) in &heights {
        assert!((height - first).abs() <= 0.5, "{panel:?} tab strip is {height}, not {first}");
    }
}

fn the_inspector_has_one_value_column_and_caps_its_width(language: Language) {
    let mut editor = editor_in(language);
    let player = editor.app.world.scene.find_by_name("Player").unwrap();
    editor.apply(Action::Select(Some(player)));
    let ctx = egui::Context::default();
    for (width, label) in [(300.0, "narrow"), (1200.0, "wide")] {
        editor.float_panel(Panel::Inspector, Some(egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(width, 760.0))));
        frames(&mut editor, &ctx, egui::vec2(1600.0, 900.0), 4);
        let rects = editor.inspector_field_rects();
        assert!(rects.len() > 5, "{label}: fields are drawn");
        let left = rects.iter().map(|(_, r)| r.left()).fold(f32::MAX, f32::min);
        for ((component, field), rect) in &rects {
            assert!((rect.left() - left).abs() <= 1.0, "{label}: {component}.{field} starts at {}, column at {left}", rect.left());
            assert!(
                rect.width() <= engine::editor::kit::theme::PROP_VALUE_MAX + 1.0,
                "{label}: {component}.{field} is {} wide",
                rect.width()
            );
        }
        assert_eq!(editor.inspector_overflow(), 0.0, "{label}: the inspector content is wider than its window");
    }
}

fn no_component_widens_the_inspector_at_any_width_or_scale(language: Language) {
    let mut editor = editor_in(language);
    let names: Vec<&'static str> = editor.app.world.registry.names().collect();
    // Каждый компонент отдельно — чтобы провал называл виновника.
    let ctx = egui::Context::default();
    let mut failures = Vec::new();
    for kind in &names {
        editor.apply(Action::CreateEmpty { parent: None });
        let id = editor.selection().unwrap();
        editor.apply(Action::AddComponent { object: id, kind: kind.to_string() });
        // 150 — самое узкое окно дока при масштабе 150 % (область не уже 120 точек).
        for width in [150.0, 220.0, 480.0] {
            editor.float_panel(Panel::Inspector, Some(egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(width, 700.0))));
            frames(&mut editor, &ctx, egui::vec2(1600.0, 900.0), 3);
            let overflow = editor.inspector_overflow();
            if overflow > 0.5 {
                failures.push(format!("{kind} at {width}: +{overflow:.0} px"));
            }
        }
        editor.apply(Action::Delete(id));
    }
    assert!(failures.is_empty(), "components that widen the inspector: {failures:?}");

    // Один объект со всеми компонентами и очень длинным именем, масштаб 80–200 %.
    editor.apply(Action::CreateEmpty { parent: None });
    let id = editor.selection().unwrap();
    for kind in &names {
        editor.apply(Action::AddComponent { object: id, kind: kind.to_string() });
    }
    if let Some(object) = editor.app.world.scene.get_mut(id) {
        object.name = "An object with a very very long name that would never fit into a narrow inspector".to_string();
    }
    for scale in [0.8, 1.0, 1.5, 2.0] {
        editor.set_preferences(Preferences { ui_scale: scale, ..Preferences::default() });
        let ctx = egui::Context::default();
        for width in [220.0, 320.0, 480.0, 800.0, 1200.0] {
            editor.float_panel(Panel::Inspector, Some(egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(width / scale, 700.0))));
            frames(&mut editor, &ctx, egui::vec2(1600.0, 900.0), 3);
            let overflow = editor.inspector_overflow();
            assert!(overflow <= 0.5, "scale {scale}, width {width}: the inspector is {overflow} px wider than its window");
        }
    }
}

/// Каждая проверка — на английском и на русском.
macro_rules! both_languages {
    ($($name:ident),* $(,)?) => {
        $(
            mod $name {
                use super::*;
                #[test]
                fn english() {
                    super::$name(Language::English);
                }
                #[test]
                fn russian() {
                    super::$name(Language::Russian);
                }
            }
        )*
    };
}

both_languages!(the_play_group_is_centred_in_the_toolbar_at_every_width, neighbouring_tab_strips_have_one_height, the_inspector_has_one_value_column_and_caps_its_width, no_component_widens_the_inspector_at_any_width_or_scale);
