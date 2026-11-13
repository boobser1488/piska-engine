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
