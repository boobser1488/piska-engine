//! Мелкие функции редактора волны 12: сцена помечается изменённой при перетаскивании
//! гизмо (и Save пишет правку), `Editor::set_project_folder`, звук игры в редакторе,
//! окно Tools → Check Build Tools… Без GPU и без звука.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Tool};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

/// Временный проект: только сцена уровня (ассеты по GUID просто не найдутся — для
/// гизмо и сохранения они не нужны).
fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_editor_tools_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::copy(Path::new(ASSETS).join("scenes/level1.scene"), dir.join("scenes/level1.scene")).unwrap();
    // Префабы, на которые ссылается сцена.
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    for entry in std::fs::read_dir(Path::new(ASSETS).join("prefabs")).unwrap().flatten() {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), dir.join("prefabs").join(entry.file_name())).unwrap();
        }
    }
    dir
}

fn editor(dir: &Path) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, dir).unwrap();
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

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![button(from, true)]);
        for step in 1..=8 {
            self.frame(editor, vec![egui::Event::PointerMoved(from + (to - from) * (step as f32 / 8.0))]);
        }
        self.frame(editor, vec![button(to, false)]);
        self.frame(editor, vec![]);
    }
}

/// Отчёт swarm: «объект, сдвинутый прямо в сцене, не помечает её изменённой». Это не
/// ошибка редактора: прямое `scene.get_mut(..)` в тесте обходит редактор. Перетаскивание
/// гизмо (как и правка в инспекторе) помечает сцену, а Save пишет правку в файл.
#[test]
fn dragging_the_gizmo_marks_the_scene_modified_and_save_writes_it() {
    let dir = project("dirty");
    let mut editor = editor(&dir);
    let mut hands = Hands::new(&mut editor);
    assert!(!editor.is_dirty(), "a freshly opened scene is clean");
    let id = editor.app.world.scene.find_by_name("Platform A").expect("level1 has Platform A");
    editor.apply(Action::Select(Some(id)));
    editor.set_tool(Tool::Move);
    hands.frame(&mut editor, vec![]);
    let before = editor.app.world.scene.world_position(id);
    let handle = editor.world_to_screen(before.truncate());
    hands.drag(&mut editor, handle, handle + egui::vec2(120.0, 0.0));
    let after = editor.app.world.scene.world_position(id);
    assert!(after.x - before.x > 0.5, "the gizmo moved the object: {before} → {after}");
    assert!(editor.is_dirty(), "the gizmo drag marks the scene modified");
    editor.apply(Action::Save);
    assert!(!editor.is_dirty());
    let text = std::fs::read_to_string(dir.join("scenes/level1.scene")).unwrap();
    let mut game = engine::app();
    scripts::register(&mut game);
    engine::mount_assets(&mut game, &dir).unwrap();
    game.world.load_scene_str(&text, "level1.scene").unwrap();
    let saved = game.world.scene.find_by_name("Platform A").unwrap();
    assert!((game.world.scene.world_position(saved).x - after.x).abs() < 1e-3, "the saved file has the new position");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_folder_audio_and_build_tools() {
    let dir = project("misc");
    let mut editor = editor(&dir);
    let mut hands = Hands::new(&mut editor);

    // --- Editor::set_project_folder ---
    editor.set_project_folder("Assets/scenes").unwrap();
    assert_eq!(editor.project_folder(), "scenes");
    editor.set_project_folder("data/").unwrap();
    assert_eq!(editor.project_folder(), "data");
    editor.set_project_folder("").unwrap();
    assert_eq!(editor.project_folder(), "");
    let err = editor.set_project_folder("no/such/folder").unwrap_err().to_string();
    assert!(err.contains("no/such/folder"), "{err}");
    assert_eq!(editor.project_folder(), "", "a bad path changes nothing");

    // --- Звук игры в редакторе: настройка доходит до Audio каждый кадр ---
    editor.set_editor_audio(true, 0.2);
    hands.frame(&mut editor, vec![]);
    assert_eq!(editor.editor_audio(), (true, 0.2));
    if let Some(audio) = editor.app.world.resources.get::<engine::audio::Audio>() {
        assert!(audio.is_editor_muted());
        assert!((audio.editor_volume() - 0.2).abs() < 1e-4);
    }
    editor.set_editor_audio(false, 0.5);
    hands.frame(&mut editor, vec![]);
    if let Some(audio) = editor.app.world.resources.get::<engine::audio::Audio>() {
        assert!(!audio.is_editor_muted());
    }
    let defaults = engine::editor::Preferences::default();
    assert!(!defaults.editor_audio_muted && (defaults.editor_volume - 0.5).abs() < 1e-6, "defaults: sound on, 50 %");

    // --- Tools → Check Build Tools…: piska-export не собран — окно объясняет, как собрать ---
    let built = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|t| t.join("debug").join(format!("piska-export{}", std::env::consts::EXE_SUFFIX)))
        .filter(|p| p.is_file());
    std::env::set_var("PISKA_EXPORT", dir.join("no-such-piska-export.exe"));
    editor.check_build_tools(&hands.ctx);
    hands.frame(&mut editor, vec![]);
    assert!(editor.build_tools_not_built(&hands.ctx));
    assert!(editor.build_tools_report(&hands.ctx).is_none());

    // Собранный инструмент — настоящий `doctor --json` (только читает).
    if let Some(exe) = built {
        std::env::set_var("PISKA_EXPORT", &exe);
        editor.check_build_tools(&hands.ctx);
        let mut report = None;
        for _ in 0..600 {
            hands.frame(&mut editor, vec![]);
            report = editor.build_tools_report(&hands.ctx);
            if report.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let report = report.expect("doctor answered within a minute");
        assert!(!report.checks.is_empty(), "the doctor lists the tools");
        hands.frame(&mut editor, vec![]);
        for check in report.checks.iter().filter(|c| c.failed() && c.fix.is_some()) {
            assert!(
                hands.ctx.read_response(Editor::build_tools_copy_fix_id(&check.id)).is_some(),
                "`{}` is failing: its row has a Copy fix command button",
                check.id
            );
        }
    }
    std::env::remove_var("PISKA_EXPORT");
    let _ = std::fs::remove_dir_all(&dir);
}
