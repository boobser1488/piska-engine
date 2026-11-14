//! «Звездопад», открытый так, как его открывает Piska Hub, пока программа редактора
//! проекта не собрана: общий редактор (`piska-editor --project`) без кода проекта. Игра
//! в нём «не работает» — корзина и менеджер игры — скрипты на Rust, которых в общем
//! редакторе нет. Редактор должен сказать это прямо и предложить сборку в один щелчок.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Mode};
use engine::editor_project::{project_app, GameProject, ProjectDir};

const TEMPLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/template");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Проект из шаблона во временном каталоге (как «Starfall sample» хаба: тот же шаблон).
fn starfall_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-starfall-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&Path::new(TEMPLATE).join("assets"), &dir.join("assets"));
    std::fs::copy(Path::new(TEMPLATE).join("Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    dir
}

fn texts(ctx: &egui::Context, editor: &mut Editor, frames: usize) -> Vec<String> {
    let mut found = Vec::new();
    for frame in 0..frames {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        editor.forward_game_input(ctx, &input);
        editor.advance(1.0 / 60.0);
        let shapes = ctx.run(input, |ctx| editor.ui(ctx)).shapes;
        found.clear();
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for clipped in &shapes {
            walk(&clipped.shape, &mut found);
        }
    }
    found
}

#[test]
fn the_generic_editor_says_why_starfall_does_not_play_and_offers_the_build() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let dir = starfall_project("generic");
    let project = ProjectDir::read(&dir).unwrap();
    assert_eq!(project.editor_bin.as_deref(), Some("template-editor"), "the project has its own editor program");

    // Общий редактор без возможности собрать (так открывается проект без исходников).
    let (app, scene) = project_app(&project).unwrap();
    let mut editor = Editor::new(app, EditorConfig::titled("Piska Editor: Starfall").open_scene(scene.unwrap())).unwrap();
    assert_eq!(editor.missing_script_types(), ["Basket", "GameManager"]);
    let ctx = egui::Context::default();
    editor.apply(Action::Play);
    assert_eq!(editor.mode(), Mode::Playing);
    let shown = texts(&ctx, &mut editor, 4);
    assert!(
        shown.iter().any(|t| t.contains("does not contain the project's scripts (Basket, GameManager)") && t.contains("Piska Hub")),
        "the Game view explains why the game does nothing: {shown:?}"
    );
    editor.apply(Action::Stop);

    // Тот же проект с настройкой сборки (как его открывает хаб: `--project … --build`):
    // кнопка «Build Scripts» — сборка в один щелчок.
    let (app, scene) = project_app(&project).unwrap();
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let config = GameProject {
        package: "template",
        bin: "template-editor",
        manifest_dir: leak(dir.display().to_string()),
        target_dir: None,
    }
    .editor_config()
    .open_scene(scene.unwrap());
    let mut editor = Editor::new(app, config).unwrap();
    editor.set_view_tab(engine::editor::ViewTab::Game);
    let shown = texts(&ctx, &mut editor, 4);
    assert!(shown.iter().any(|t| t == "Build Scripts"), "one click builds the scripts: {shown:?}");
    assert!(shown.iter().any(|t| t.contains("Build them once")));
    let _ = std::fs::remove_dir_all(&dir);
}
