//! Общий проигрыватель Piska: `piska-player --project <каталог проекта>`.
//!
//! Запускает игру проекта без компиляции его кода: компоненты движка, Scenario, ассеты и
//! стартовая сцена. Для проектов со своим кодом на Rust / C++ нужна их собственная
//! программа (`cargo run -p <пакет>`).

// Подсистема «окно» (без консоли и в отладочной сборке) — build.rs: engine_build::windows_app().

use std::path::PathBuf;

fn main() -> Result<(), engine::EngineError> {
    // Своей консоли нет: подключиться к терминалу или писать журнал в файл.
    engine::gui_startup();
    let mut project: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project = args.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!("piska-player --project <dir>\n\nRuns a Piska project without compiling its code.");
                return Ok(());
            }
            other if project.is_none() && !other.starts_with('-') => project = Some(PathBuf::from(other)),
            _ => {}
        }
    }
    let project = project.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    engine::editor_project::play_project(&project)
}
