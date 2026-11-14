//! Снимки редактора для ручной проверки (волна 13): иерархия большой трассы marble с
//! поиском, окно Open Scene…, New Scene…, Play с вводом у редактора, плашка «скрипты не
//! собраны» у «Звездопада», русский интерфейс — при масштабе 100 % и 150 %.
//!
//! Нужен GPU, поэтому тест не идёт по умолчанию:
//! `PASS3_SHOTS=<каталог> cargo test -p engine --test editor_pass3_shots -- --ignored --test-threads 1`.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Panel};
use engine::editor_project::{project_app, ProjectDir};

const GAMES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games");

fn out_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("PASS3_SHOTS")?);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn open(game: &str, scene: Option<&str>, scale: f32) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let project = ProjectDir::read(&Path::new(GAMES).join(game)).unwrap();
    let (app, start) = project_app(&project).unwrap();
    let scene = scene.map(str::to_string).or(start).unwrap();
    let mut editor = Editor::new(app, EditorConfig::titled(format!("Piska Editor: {game}")).open_scene(scene)).unwrap();
    let mut prefs = editor.preferences().clone();
    prefs.ui_scale = scale;
    editor.set_preferences(prefs);
    editor
}

fn key(key: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn save(editor: &mut Editor, dir: &Path, name: &str, input: impl FnMut(u32, &mut Editor) -> Vec<egui::Event>) {
    let image = engine::editor::capture_with_input(editor, 1600, 900, 8, input).unwrap();
    image.save(dir.join(name)).unwrap();
}

/// Иерархия, Open Scene…, New Scene… при масштабе `scale` (четыре снимка — четыре
/// устройства GPU в процессе; больше за раз этот драйвер не любит).
fn hierarchy_and_scene_shots(scale: f32) {
    let Some(dir) = out_dir() else { return };
    {
        let tag = if scale > 1.2 { "150" } else { "100" };
        // Трасса marble: 329 объектов, поиск «rl1» — совпадения в дереве с подсветкой.
        let mut editor = open("marble", Some("scenes/courses/course_3.scene"), scale);
        editor.set_hierarchy_filter("raill");
        save(&mut editor, &dir, &format!("hierarchy_search_{tag}.png"), |_, _| Vec::new());
        editor.set_hierarchy_filter("");
        editor.collapse_hierarchy();
        save(&mut editor, &dir, &format!("hierarchy_collapsed_{tag}.png"), |_, _| Vec::new());
        // Ctrl+O — окно выбора сцены.
        let ctrl = egui::Modifiers::COMMAND;
        save(&mut editor, &dir, &format!("open_scene_{tag}.png"), |frame, _| match frame {
            3 => vec![key(egui::Key::O, ctrl, true)],
            4 => vec![key(egui::Key::O, ctrl, false)],
            6 => vec![egui::Event::Text("cour".into())],
            _ => Vec::new(),
        });
        editor.show_scene_picker(false);
        editor.show_new_scene_dialog(true);
        save(&mut editor, &dir, &format!("new_scene_{tag}.png"), |_, _| Vec::new());
    }
}

#[test]
#[ignore = "GPU screenshots for manual review; set PASS3_SHOTS"]
fn pass3_scale_100() {
    hierarchy_and_scene_shots(1.0);
}

#[test]
#[ignore = "GPU screenshots for manual review; set PASS3_SHOTS"]
fn pass3_scale_150() {
    hierarchy_and_scene_shots(1.5);
}

#[test]
#[ignore = "GPU screenshots for manual review; set PASS3_SHOTS"]
fn pass3_play_and_russian() {
    let Some(dir) = out_dir() else { return };
    // «Звездопад» в общем редакторе: Play, фокус — на Scene; плашка о несобранных скриптах.
    let mut editor = open("template", None, 1.0);
    editor.set_layout(engine::editor::LayoutPreset::TwoByThree);
    editor.apply(Action::Play);
    save(&mut editor, &dir, "starfall_play_editor_input.png", |frame, editor| {
        if frame == 4 {
            editor.focus_panel(Panel::Scene);
        }
        Vec::new()
    });
    // Русский интерфейс: подсказки мыши вида Scene с разделителями, панель Project.
    engine::editor::kit::i18n::set(engine::editor::kit::i18n::Language::Russian);
    let mut editor = open("marble", Some("scenes/courses/course_3.scene"), 1.0);
    editor.apply(Action::Stop);
    save(&mut editor, &dir, "russian_main.png", |_, _| Vec::new());
    engine::editor::kit::i18n::set(engine::editor::kit::i18n::Language::English);
    // Инспектор FpsController (группы полей) и окно Undo History.
    let mut editor = open("template", None, 1.0);
    editor.apply(Action::CreateEmpty { parent: None });
    let player = editor.selection().unwrap();
    editor.apply(Action::Rename { id: player, name: "Player".into() });
    editor.apply(Action::AddComponent { object: player, kind: "FpsController".into() });
    editor.show_undo_history(true);
    save(&mut editor, &dir, "fps_inspector_undo_history.png", |_, _| Vec::new());
}
