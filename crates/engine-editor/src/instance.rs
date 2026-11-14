//! Один редактор на проект: файл-метка во временном каталоге с номером процесса, который
//! держит проект открытым. Редактор пишет её при открытии окна и убирает при выходе;
//! Piska Hub перед «Open» проверяет её и не запускает второй редактор того же проекта
//! (даже если первый запущен не из хаба или хаб перезапускали).
//!
//! Перезапуск после Build переписывает метку номером нового процесса; старый при
//! выходе удаляет её, только если она всё ещё его.

use std::path::{Path, PathBuf};

/// Каталог проекта по каталогу ассетов: проект — каталог с `assets/` внутри.
pub fn project_dir_of_assets(assets: &Path) -> PathBuf {
    match assets.file_name() {
        Some(name) if name == "assets" => assets.parent().map_or_else(|| assets.to_path_buf(), Path::to_path_buf),
        _ => assets.to_path_buf(),
    }
}

/// Ключ проекта: путь без `\\?\`, с `/`, в нижнем регистре (Windows не различает регистр).
fn key(project: &Path) -> String {
    let path = project.canonicalize().unwrap_or_else(|_| project.to_path_buf());
    let text = path.display().to_string();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Файл-метка проекта.
pub fn lock_path(project: &Path) -> PathBuf {
    // FNV-1a: короткое стабильное имя без зависимостей.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key(project).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    std::env::temp_dir().join(format!("piska-editor-{hash:016x}.lock"))
}

/// Записать метку «проект открыт процессом `pid`».
pub fn acquire(project: &Path, pid: u32) -> std::io::Result<()> {
    std::fs::write(lock_path(project), format!("{pid}\n{}\n", key(project)))
}

/// Убрать метку, если она принадлежит процессу `pid`.
pub fn release(project: &Path, pid: u32) {
    let path = lock_path(project);
    if read_pid(&path) == Some(pid) {
        let _ = std::fs::remove_file(path);
    }
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.lines().next()?.trim().parse().ok()
}

/// Попросить редактор, у которого открыт проект, показать своё окно (поднять и дать ему
/// фокус): файл-просьба рядом с меткой; редактор проверяет её между кадрами.
pub fn request_focus(project: &Path) -> std::io::Result<()> {
    std::fs::write(lock_path(project).with_extension("focus"), std::process::id().to_string())
}

/// Есть просьба показать окно (и она снимается). Зовёт окно редактора между кадрами.
pub fn take_focus_request(project: &Path) -> bool {
    std::fs::remove_file(lock_path(project).with_extension("focus")).is_ok()
}

/// Номер живого процесса редактора, у которого открыт проект; `None` — не открыт (или
/// метка осталась от процесса, которого уже нет).
pub fn running_editor(project: &Path) -> Option<u32> {
    let pid = read_pid(&lock_path(project))?;
    (pid != std::process::id() && process_alive(pid)).then_some(pid)
}

/// Жив ли процесс (без небезопасного кода: `tasklist` на Windows, `/proc` на Linux).
pub fn process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        engine_build::host::command("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
    }
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        // Проверить нечем: считаем, что метка верна, пока её не убрали.
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_names_one_live_editor_per_project() {
        let project = std::env::temp_dir().join(format!("piska-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(project.join("assets")).unwrap();
        assert_eq!(project_dir_of_assets(&project.join("assets")), project);
        // Тот же проект под другим написанием пути — та же метка.
        let spelled = PathBuf::from(project.display().to_string().to_uppercase());
        if cfg!(windows) {
            assert_eq!(lock_path(&project), lock_path(&spelled));
        }
        assert_eq!(running_editor(&project), None);

        // Живой «чужой» процесс держит проект.
        let mut child = if cfg!(windows) {
            std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn()
        } else {
            std::process::Command::new("sleep").arg("30").spawn()
        }
        .unwrap();
        acquire(&project, child.id()).unwrap();
        assert_eq!(running_editor(&project), Some(child.id()), "the hub sees the open editor");
        // Просьба хаба «покажи окно» доходит один раз.
        request_focus(&project).unwrap();
        assert!(take_focus_request(&project));
        assert!(!take_focus_request(&project));
        // Чужую метку не убирают.
        release(&project, std::process::id());
        assert_eq!(running_editor(&project), Some(child.id()));
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(running_editor(&project), None, "a lock left by a dead process does not count");
        release(&project, child.id());
        assert!(!lock_path(&project).exists());
        let _ = std::fs::remove_dir_all(&project);
    }
}
