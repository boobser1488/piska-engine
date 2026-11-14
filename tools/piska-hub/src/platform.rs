//! Действия операционной системы: показать каталог в файловом менеджере, открыть
//! файл программой по умолчанию.

use engine_editor::kit::i18n::{trf};
use std::path::Path;

/// Показать каталог (или файл в его каталоге) в файловом менеджере.
pub fn reveal(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(trf!("`{}` does not exist", path.display()));
    }
    // Одна реализация на все программы: путь с пробелами, кириллицей, `\\?\` и `/`
    // выделяется в своём каталоге (раньше проводник открывал рабочий стол).
    engine_build::host::reveal_in_file_manager(path).map_err(|e| trf!("cannot open the file manager: {e}", e = e))
}

/// Открыть файл программой, назначенной в системе (`.md` — редактор или браузер).
pub fn open_with_default(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(trf!("`{}` does not exist", path.display()));
    }
    #[cfg(windows)]
    let result = engine_build::host::command("explorer").arg(engine_build::host::native_path(path)).spawn();
    #[cfg(target_os = "macos")]
    let result = engine_build::host::command("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = engine_build::host::command("xdg-open").arg(path).spawn();
    result.map(|_| ()).map_err(|e| trf!("cannot open `{p0}`: {e}", p0 = path.display(), e = e))
}
