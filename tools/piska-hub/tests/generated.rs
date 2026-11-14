//! Проекты из мастера компилируются и проходят свой тест без окна.
//!
//! Долго: отдельный проект впервые проверяет (или собирает) движок целиком, поэтому
//! тесты помечены `#[ignore]`:
//!
//! ```text
//! cargo test -p piska-hub --test generated -- --ignored
//! ```
//!
//! Проекты создаются во временном каталоге ОС и удаляются после проверки.
//! `CARGO_TARGET_DIR` наследуется (общий кэш сборки), иначе — каталог рядом с проектами.

use std::path::{Path, PathBuf};
use std::process::Command;

use piska_hub::paths;
use piska_hub::templates::{self, NewProject, Template};

fn create(dir: &Path, name: &str, template: Template, cpp: bool) -> PathBuf {
    let root = paths::find_engine_root().unwrap();
    let plan = templates::plan(
        &NewProject {
            name: name.into(),
            location: dir.to_path_buf(),
            template,
            cpp,
        },
        &root,
    )
    .unwrap();
    templates::create(&plan, &root).unwrap();
    plan.dir
}

fn cargo(project: &Path, args: &[&str], scratch: &Path) -> (bool, String) {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command.args(args).arg("--offline").current_dir(project).env("ENGINE_AUDIO", "off");
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        command.env("CARGO_TARGET_DIR", scratch.join("target"));
    }
    let output = command.output().expect("cargo runs");
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-gen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Все три шаблона (один — с C++) проходят `cargo check --all-targets`: библиотека,
/// программа игры, программа редактора (`--features editor`) и тесты.
#[test]
#[ignore = "compiles the engine for a standalone project; run with --ignored"]
fn every_template_type_checks() {
    let dir = scratch("check");
    for (name, template, cpp) in [
        ("gen_empty2d", Template::Empty2D, false),
        ("gen_empty3d", Template::Empty3D, true),
        ("gen_starfall", Template::Starfall, false),
    ] {
        let project = create(&dir, name, template, cpp);
        let (ok, output) = cargo(&project, &["check", "--all-targets", "--features", "editor", "--message-format=short"], &dir);
        assert!(ok, "`cargo check` of the {} project failed:\n{output}", template.title());
        assert!(!output.contains("warning:"), "the generated {} project has warnings:\n{output}", template.title());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Пустой 2D-проект собирается и проходит свой тест без окна (`tests/smoke.rs`).
#[test]
#[ignore = "builds the engine for a standalone project; run with --ignored"]
fn an_empty_project_passes_its_smoke_test() {
    let dir = scratch("smoke");
    let project = create(&dir, "gen_smoke", Template::Empty2D, false);
    let (ok, output) = cargo(&project, &["test", "--test", "smoke"], &dir);
    assert!(ok, "the smoke test of a new project failed:\n{output}");
    assert!(output.contains("test result: ok. 1 passed"), "{output}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Новый проект Empty 3D открывается общим редактором без компиляции: время от записи
/// файлов до первого нарисованного кадра (без окна: `ENGINE_SCREENSHOT`). Нужен собранный
/// `piska-editor` (`cargo build -p piska-editor`).
#[test]
#[ignore = "needs a built piska-editor and the GPU; run with --ignored"]
fn a_new_project_opens_in_the_generic_editor_without_compiling() {
    let engine = paths::find_engine_root().unwrap();
    let editor = piska_hub::builds::tool_exe("piska-editor", None, Some(&engine)).expect("piska-editor is built");
    let dir = scratch("generic");
    let started = std::time::Instant::now();
    let project = create(&dir, "gen_generic", Template::Empty3D, false);
    let written = started.elapsed();
    let shot = dir.join("first-frame.png");
    let output = Command::new(&editor)
        .arg("--project")
        .arg(&project)
        .env("ENGINE_SCREENSHOT", &shot)
        .env("ENGINE_FRAMES", "3")
        .env("ENGINE_AUDIO", "off")
        .output()
        .expect("piska-editor runs");
    let total = started.elapsed();
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    assert!(shot.is_file());
    assert!(!log.contains("is missing"), "a new project has no missing scripts:\n{log}");
    eprintln!(
        "files written in {:.0} ms; first editor frame after {:.2} s in total",
        written.as_secs_f64() * 1000.0,
        total.as_secs_f64()
    );
    if let Some(keep) = std::env::var_os("PISKA_HUB_SHOTS") {
        let _ = std::fs::copy(&shot, std::path::Path::new(&keep).join("21_generic_editor_new_project.png"));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Демо нового проекта работает без компиляции: общий проигрыватель запускает сценарий
/// Scenario, и куб поворачивается (кадр 1 и кадр 60 отличаются). Без окна.
#[test]
#[ignore = "needs a built piska-player and the GPU; run with --ignored"]
fn the_scenario_demo_runs_in_the_generic_player() {
    let engine = paths::find_engine_root().unwrap();
    let player = piska_hub::builds::tool_exe("piska-player", None, Some(&engine)).expect("piska-player is built");
    let dir = scratch("player");
    let project = create(&dir, "gen_player", Template::Empty3D, false);
    let frame = |frames: &str, name: &str| {
        let shot = dir.join(name);
        let output = Command::new(&player)
            .arg("--project")
            .arg(&project)
            .env("ENGINE_SCREENSHOT", &shot)
            .env("ENGINE_FRAMES", frames)
            .env("ENGINE_AUDIO", "off")
            .output()
            .expect("piska-player runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let log = String::from_utf8_lossy(&output.stderr);
        for line in log.lines().filter(|l| l.contains("scenario") || l.contains("warn") || l.contains("error")) {
            eprintln!("player: {line}");
        }
        image::open(&shot).unwrap().to_rgba8()
    };
    let first = frame("1", "first.png");
    let later = frame("60", "later.png");
    let changed = first.pixels().zip(later.pixels()).filter(|(a, b)| a != b).count();
    eprintln!("{changed} pixels changed between frame 1 and frame 60");
    assert!(changed > 500, "the Scenario script turns the cube");
    std::fs::remove_dir_all(&dir).unwrap();
}
