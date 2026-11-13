//! Действия операционной системы: показать каталог в файловом менеджере, открыть
//! файл программой по умолчанию.

use std::path::Path;
use std::process::Command;

/// Показать каталог (или файл в его каталоге) в файловом менеджере.
pub fn reveal(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("`{}` does not exist", path.display()));
    }
    #[cfg(windows)]
    let result = if path.is_dir() {
        Command::new("explorer").arg(path).spawn()
    } else {
        Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    };
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open")
        .arg(if path.is_dir() { path } else { path.parent().unwrap_or(path) })
        .spawn();
    result.map(|_| ()).map_err(|e| format!("cannot open the file manager: {e}"))
}

/// Открыть файл программой, назначенной в системе (`.md` — редактор или браузер).
pub fn open_with_default(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("`{}` does not exist", path.display()));
    }
    #[cfg(windows)]
    let result = Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(path).spawn();
    result.map(|_| ()).map_err(|e| format!("cannot open `{}`: {e}", path.display()))
}
