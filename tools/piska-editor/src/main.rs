//! Общий редактор Piska: `piska-editor --project <каталог проекта> [--build]`.
//!
//! Открывает любой проект без компиляции его кода: компоненты движка, Scenario, ассеты
//! и стартовая сцена проекта. Компоненты проекта на Rust / C++ видны как «missing script»
//! и сохраняются как были. `--build` — у проекта есть свой код, а его программа
//! редактора ещё не собрана: сборка начинается сразу, и редактор перезапускается в
//! программу проекта, когда она готова. Подробно — `docs/api/hub.md`.

// Подсистема «окно» (без консоли и в отладочной сборке) — build.rs: engine_build::windows_app().

use std::path::PathBuf;

fn main() -> Result<(), engine::EngineError> {
    // Своей консоли нет: подключиться к терминалу или писать журнал в файл.
    engine::gui_startup();
    let mut project: Option<PathBuf> = None;
    let mut build = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project = args.next().map(PathBuf::from),
            "--build" => build = true,
            "--help" | "-h" => {
                println!("piska-editor --project <dir> [--build]\n\nOpens a Piska project without compiling its code.");
                return Ok(());
            }
            // `--editor-session <файл>` и прочее читает сам редактор.
            "--editor-session" => {
                args.next();
            }
            other if project.is_none() && !other.starts_with('-') => project = Some(PathBuf::from(other)),
            _ => {}
        }
    }
    let project = project.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    engine::editor_project::open_project(&project, option_env!("CARGO_TARGET_DIR"), build)
}
