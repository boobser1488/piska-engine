//! Один установщик за раз: работающий отмечается файлом `installer.alive` (pid и время,
//! обновляется каждые несколько секунд) в каталоге настроек; второй запуск оставляет
//! просьбу `installer.focus` и выходит, первый выводит своё окно вперёд.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ALIVE: &str = "installer.alive";
const FOCUS: &str = "installer.focus";
/// Отметка старше — установщик считается закрытым (упал, выключили питание).
const STALE: u64 = 10;
/// Как часто обновлять отметку.
pub const HEARTBEAT: Duration = Duration::from_secs(3);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Работающий экземпляр (отметка снимается при `drop`).
#[derive(Debug)]
pub struct Instance {
    dir: PathBuf,
    pid: u32,
    last: u64,
}

/// Другой установщик уже открыт: ему отправлена просьба показаться.
#[derive(Debug, PartialEq, Eq)]
pub struct AlreadyRunning;

fn other_alive(dir: &Path, me: u32, at: u64) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(ALIVE)) else {
        return false;
    };
    let mut parts = text.split_whitespace();
    let pid: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let time: u64 = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    pid != me && pid != 0 && at.saturating_sub(time) <= STALE
}

impl Instance {
    /// Занять место; если другой установщик жив — попросить его показаться.
    pub fn claim(dir: &Path) -> Result<Instance, AlreadyRunning> {
        Self::claim_as(dir, std::process::id(), now())
    }

    pub fn claim_as(dir: &Path, pid: u32, at: u64) -> Result<Instance, AlreadyRunning> {
        if other_alive(dir, pid, at) {
            let _ = std::fs::write(dir.join(FOCUS), format!("{pid} {at}"));
            return Err(AlreadyRunning);
        }
        let mut me = Instance { dir: dir.to_path_buf(), pid, last: 0 };
        me.beat(at);
        Ok(me)
    }

    fn beat(&mut self, at: u64) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join(ALIVE), format!("{} {at}", self.pid));
        self.last = at;
    }

    /// Обновить отметку (дёшево; вызывается из `update`). `true` — второй запуск просил показаться.
    pub fn tick(&mut self) -> bool {
        let at = now();
        if at.saturating_sub(self.last) >= HEARTBEAT.as_secs() {
            self.beat(at);
        }
        let focus = self.dir.join(FOCUS);
        if focus.is_file() {
            let _ = std::fs::remove_file(focus);
            return true;
        }
        false
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // Только свою отметку: если её уже переписал другой — не трогать.
        let file = self.dir.join(ALIVE);
        if std::fs::read_to_string(&file).is_ok_and(|t| t.split_whitespace().next() == Some(&self.pid.to_string())) {
            let _ = std::fs::remove_file(file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_instance_asks_the_first_to_show_itself() {
        let dir = std::env::temp_dir().join(format!("piska-installer-instance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut first = Instance::claim_as(&dir, 100, now()).unwrap();
        assert_eq!(Instance::claim_as(&dir, 200, now()).unwrap_err(), AlreadyRunning);
        assert!(first.tick(), "the first one sees the request");
        assert!(!first.tick(), "once");
        drop(first);
        assert!(!dir.join(ALIVE).exists());
        // Отметка упавшего установщика (старая) не мешает.
        std::fs::write(dir.join(ALIVE), format!("300 {}", now() - 3600)).unwrap();
        assert!(Instance::claim_as(&dir, 400, now()).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
