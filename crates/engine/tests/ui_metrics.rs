//! Замер интерфейса редактора по всем экранам, которые достижимы без окна (`kit::metrics`,
//! правила плотности [`Rules::compact`]) — в масштабе 100 / 125 / 150 %, на английском и
//! русском: главная раскладка (Hierarchy, Scene, Inspector, Project), Inspector каждого
//! объекта сцены, Project списком и сеткой, Console, Profiler, вкладки Game и Code (и Code с
//! ошибками), Error List, Find Results, окна
//! Build, Undo History, Getting Started, Shortcuts, Open Scene, New Scene, Export Project File
//! и каждая страница Preferences (в том числе Build Tools) и Project Settings.
//!
//! Падает, если: у контрола с рамкой высота не [`CONTROL_H`]; соседи строки расходятся по
//! верху или низу; промежуток между ними не из шкалы; рамка рисуется за контролом; контрол
//! выходит за свой контейнер; текст контрола ближе 4 точек к краю; чужой текст заходит под
//! контрол. При 100 % и 150 % каждому контролу главного экрана и окон прогоняются наведение,
//! нажатие и фокус клавиатуры. Сводка — в файл из `UI_METRICS_OUT`, если он задан.
//!
//! [`CONTROL_H`]: engine::editor::kit::theme::CONTROL_H
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::kit::i18n::{self, Language};
use engine::editor::kit::metrics::{self, Report, Rules};
use engine::editor::{Action, BottomTab, Editor, EditorConfig, LayoutAction, Panel, SettingsScope, ViewTab};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const PREFS: [&str; 8] = ["general", "appearance", "scene-view", "game-view", "console", "code-editor", "build-tools", "shortcuts"];
const PROJECT: [&str; 10] = ["player", "build", "physics2d", "physics3d", "tags-and-layers", "input", "time", "graphics", "audio", "script-order"];

fn editor(scale: f32, language: Language) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let mut editor = Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap();
    let mut prefs = editor.preferences().clone();
    prefs.ui_scale = scale;
    prefs.language = Some(language);
    editor.set_preferences(prefs);
    editor
}

fn frame(ctx: &egui::Context, editor: &mut Editor, time: &mut f64, events: Vec<egui::Event>) -> Vec<egui::epaint::ClippedShape> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
        time: Some(*time),
        predicted_dt: 1.0 / 60.0,
        events,
        ..egui::RawInput::default()
    };
    *time += 1.0 / 60.0;
    ctx.run(input, |ctx| editor.ui(ctx)).shapes
}

struct Run {
    total: Report,
    screens: usize,
    states: usize,
}

/// Щелчок правой кнопкой в `at`: открывает контекстное меню.
fn right_click(ctx: &egui::Context, editor: &mut Editor, time: &mut f64, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
    frame(ctx, editor, time, vec![egui::Event::PointerMoved(at)]);
    frame(ctx, editor, time, vec![button(true)]);
    frame(ctx, editor, time, vec![button(false)]);
}

fn escape(ctx: &egui::Context, editor: &mut Editor, time: &mut f64) {
    let key = |pressed| egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
    frame(ctx, editor, time, vec![key(true), key(false), egui::Event::PointerGone]);
    frame(ctx, editor, time, Vec::new());
}

impl Run {
    /// Замерить экран: несколько кадров, сводка последнего; `states` — ещё и состояния.
    fn measure(&mut self, ctx: &egui::Context, editor: &mut Editor, time: &mut f64, screen: &str, states: bool) {
        let mut shapes = Vec::new();
        for _ in 0..4 {
            shapes = frame(ctx, editor, time, Vec::new());
        }
        self.total.merge(metrics::analyze_shapes(screen, &metrics::widgets(ctx), &shapes, Rules::compact()));
        self.screens += 1;
        if states {
            let (report, n) = metrics::analyze_states(screen, ctx, Rules::compact(), |events| frame(ctx, editor, time, events));
            self.total.merge(report);
            self.states += n;
        }
    }
}

#[test]
fn editor_screens_measure_up() {
    let mut run = Run { total: Report::default(), screens: 0, states: 0 };
    let mut menus = 0;
    for language in [Language::English, Language::Russian] {
        for scale in [1.0_f32, 1.25, 1.5] {
            let states = scale != 1.25;
            let mut editor = editor(scale, language);
            let ctx = egui::Context::default();
            let mut time = 0.0;
            let tag = format!("{language:?} {scale}x");
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} main"), states);
            // Inspector каждого объекта сцены (все виды компонентов уровня).
            let ids = editor.app.world.scene.ids();
            for (i, id) in ids.iter().enumerate() {
                editor.apply(Action::Select(Some(*id)));
                run.measure(&ctx, &mut editor, &mut time, &format!("{tag} inspector/{i}"), states && i == 1);
            }
            editor.apply(Action::Select(None));
            // Контекстные меню: строка Hierarchy и сцена в окне Project.
            if let Some(first) = ids.first() {
                let row = ctx.read_response(Editor::hierarchy_row_id(*first)).map(|r| r.rect.center());
                if let Some(at) = row {
                    let layers = ctx.memory(|m| m.layer_ids().count());
                    right_click(&ctx, &mut editor, &mut time, at);
                    run.measure(&ctx, &mut editor, &mut time, &format!("{tag} hierarchy-context-menu"), false);
                    // Меню — новый слой поверх окна.
                    menus += usize::from(ctx.memory(|m| m.layer_ids().count()) > layers);
                    escape(&ctx, &mut editor, &mut time);
                }
            }
            editor.set_project_list_view(true);
            frame(&ctx, &mut editor, &mut time, Vec::new());
            if let Some(at) = editor.project_row_id("scenes").and_then(|id| ctx.read_response(id)).map(|r| r.rect.center()) {
                let layers = ctx.memory(|m| m.layer_ids().count());
                right_click(&ctx, &mut editor, &mut time, at);
                run.measure(&ctx, &mut editor, &mut time, &format!("{tag} project-context-menu"), false);
                menus += usize::from(ctx.memory(|m| m.layer_ids().count()) > layers);
                escape(&ctx, &mut editor, &mut time);
            }
            // Нижние вкладки и Project в двух видах.
            editor.set_project_list_view(true);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} project-list"), false);
            editor.set_project_list_view(false);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} project-grid"), false);
            editor.set_bottom_tab(BottomTab::Console);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} console"), states);
            editor.set_bottom_tab(BottomTab::Profiler);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} profiler"), false);
            editor.set_bottom_tab(BottomTab::Project);
            editor.set_view_tab(ViewTab::Game);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} game-view"), false);
            editor.set_view_tab(ViewTab::Code);
            editor.set_code_text("fn main() {\n    println!(\"hello\");\n}\n");
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} code"), false);
            // Вкладка Code с ошибками (волны, значки, полоса обзора, схема файла), Error List
            // и Find Results.
            let code_dir = std::env::temp_dir().join(format!("piska_metrics_code_{}", std::process::id()));
            std::fs::create_dir_all(&code_dir).unwrap();
            let broken = code_dir.join("broken.rs");
            std::fs::write(&broken, "fn main() {
    let x = 5
    let y = (1, 2];
}
").unwrap();
            editor.apply(Action::OpenCode(broken));
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} code-squiggles"), false);
            editor.apply(Action::Layout(LayoutAction::Focus(Panel::ErrorList)));
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} error-list"), states);
            editor.apply(Action::Layout(LayoutAction::Focus(Panel::FindResults)));
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} find-results"), states);
            editor.set_bottom_tab(BottomTab::Project);
            let _ = std::fs::remove_dir_all(&code_dir);
            editor.set_view_tab(ViewTab::Scene);
            // Окна.
            editor.apply(Action::ShowBuildDialog(true));
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} build"), states);
            editor.apply(Action::ShowBuildDialog(false));
            editor.show_undo_history(true);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} undo-history"), false);
            editor.show_undo_history(false);
            editor.show_getting_started(true);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} getting-started"), states);
            editor.show_getting_started(false);
            editor.show_shortcuts_help(true);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} shortcuts"), false);
            editor.show_shortcuts_help(false);
            editor.show_scene_picker(true);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} open-scene"), states);
            editor.show_scene_picker(false);
            editor.show_new_scene_dialog(true);
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} new-scene"), states);
            editor.show_new_scene_dialog(false);
            editor.show_project_file_export(&ctx, std::path::Path::new(ASSETS).join(".."));
            run.measure(&ctx, &mut editor, &mut time, &format!("{tag} export-project-file"), states);
            frame(&ctx, &mut editor, &mut time, vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
            for page in PREFS {
                editor.show_settings_page(SettingsScope::Preferences, page);
                run.measure(&ctx, &mut editor, &mut time, &format!("{tag} prefs/{page}"), states && page == "general");
            }
            editor.set_settings_open(SettingsScope::Preferences, false);
            for page in PROJECT {
                editor.show_settings_page(SettingsScope::Project, page);
                run.measure(&ctx, &mut editor, &mut time, &format!("{tag} project/{page}"), states && page == "player");
            }
            editor.set_settings_open(SettingsScope::Project, false);
        }
    }
    i18n::set(Language::English);
    let total = &run.total;
    let text = format!(
        "# Editor UI metrics\n\n{} screens, {} control states.\n\n## Control heights (pt)\n\n| height | count |\n|---|---|\n{}\n\n## Gaps between row neighbours (pt)\n\n| gap | count |\n|---|---|\n{}\n\n## Offenders ({})\n\n{}\n",
        run.screens,
        run.states,
        Report::table(&total.heights),
        Report::table(&total.gaps),
        total.offenders.len(),
        total.offenders.iter().map(|o| format!("* {o}")).collect::<Vec<_>>().join("\n")
    );
    if let Ok(path) = std::env::var("UI_METRICS_OUT") {
        std::fs::write(path, &text).unwrap();
    }
    assert!(run.screens >= 150, "screens measured: {}", run.screens);
    assert!(menus >= 6, "context menus opened and measured: {menus}");
    assert!(total.offenders.is_empty(), "{} offenders, first ones:\n{}", total.offenders.len(), total.offenders.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
}
