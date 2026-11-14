//! Связь хаба с редактором через файлы в каталоге настроек:
//! `hub.alive` — хаб работает (pid и время отметки), `hub.request` — просьба
//! редактора (`show` или `open <путь>`), `recent.txt` — недавние проекты для
//! File → Open Recent.

use std::path::{Path, PathBuf};

use crate::paths;

/// Просьба к хабу.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Показать окно.
    Show,
    /// Открыть проект в редакторе.
    Open(PathBuf),
    /// Открыть страницу импорта с этим каталогом или файлом проекта (двойной щелчок по файлу).
    Import(PathBuf),
}

/// Разобрать текст `hub.request`.
pub fn parse_request(text: &str) -> Option<Request> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line == "show" {
        return Some(Request::Show);
    }
    if let Some(path) = line.strip_prefix("import ") {
        let path = path.trim();
        return (!path.is_empty()).then(|| Request::Import(PathBuf::from(path)));
    }
    let path = line.strip_prefix("open ")?.trim();
    (!path.is_empty()).then(|| Request::Open(PathBuf::from(path)))
}

/// Забрать просьбу (файл удаляется).
pub fn take_request(dir: &Path) -> Option<Request> {
    let file = dir.join(paths::REQUEST_FILE);
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    parse_request(&text)
}

/// Отметить, что хаб жив.
pub fn write_alive(dir: &Path, now: u64) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(paths::ALIVE_FILE), format!("{}\n{now}\n", std::process::id()));
}

/// Хаб закрылся.
pub fn remove_alive(dir: &Path) {
    let file = dir.join(paths::ALIVE_FILE);
    // Только свою отметку: второй хаб мог уже записать свою.
    let ours = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| text.lines().next().map(str::to_string))
        .is_some_and(|pid| pid.trim() == std::process::id().to_string());
    if ours {
        let _ = std::fs::remove_file(file);
    }
}

/// Другой хаб работает (отметка моложе 5 секунд и не наша).
pub fn other_hub_alive(dir: &Path, now: u64) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(paths::ALIVE_FILE)) else {
        return false;
    };
    let mut lines = text.lines();
    let pid = lines.next().unwrap_or_default().trim().to_string();
    let time: u64 = lines.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    pid != std::process::id().to_string() && now.saturating_sub(time) < 5
}

/// Передать просьбу работающему хабу.
pub fn send_request(dir: &Path, request: &Request) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = match request {
        Request::Show => "show\n".to_string(),
        Request::Open(path) => format!("open {}\n", path.display()),
        Request::Import(path) => format!("import {}\n", path.display()),
    };
    std::fs::write(dir.join(paths::REQUEST_FILE), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("piska-hub-link-{}", std::process::id()));
        assert_eq!(take_request(&dir), None);
        send_request(&dir, &Request::Open(PathBuf::from("C:/games/my game"))).unwrap();
        assert_eq!(take_request(&dir), Some(Request::Open(PathBuf::from("C:/games/my game"))));
        assert_eq!(take_request(&dir), None, "a request is taken once");
        send_request(&dir, &Request::Import(PathBuf::from("C:/x/my game.pks"))).unwrap();
        assert_eq!(take_request(&dir), Some(Request::Import(PathBuf::from("C:/x/my game.pks"))));
        assert_eq!(parse_request("\nshow\n"), Some(Request::Show));
        assert_eq!(parse_request("open "), None);
        assert_eq!(parse_request("dance"), None);

        write_alive(&dir, 100);
        assert!(!other_hub_alive(&dir, 101), "our own mark is not another hub");
        std::fs::write(dir.join(paths::ALIVE_FILE), "1\n100\n").unwrap();
        assert!(other_hub_alive(&dir, 103));
        assert!(!other_hub_alive(&dir, 110), "a stale mark means the hub is gone");
        remove_alive(&dir);
        assert!(dir.join(paths::ALIVE_FILE).is_file(), "someone else's mark is kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
