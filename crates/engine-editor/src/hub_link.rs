//! Связь с Piska Hub (`tools/piska-hub`): File → Project Hub… и File → Open Recent.
//!
//! Хаб и редактор договариваются файлами в каталоге настроек хаба (`%APPDATA%/piska`,
//! `$XDG_CONFIG_HOME/piska` или `~/.config/piska`; `PISKA_HUB_CONFIG_DIR` заменяет):
//! `hub.alive` — хаб работает, `hub.request` — просьба (`show` / `open <путь>`),
//! `recent.txt` — недавние проекты. Формат описан в `docs/api/hub.md`, §8.

use crate::i18n::{tr_s, trf};
use std::path::{Path, PathBuf};
use std::process::Child;

use crate::state::Editor;
use crate::EditorError;

/// Каталог настроек хаба — тот же, что у самого хаба.
fn hub_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PISKA_HUB_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(appdata).join("piska"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(xdg).join("piska"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("piska"))
}

/// Недавние проекты хаба: (имя, каталог), новые сверху. Пусто — хаб ещё не писал список.
pub(crate) fn recent_projects() -> Vec<(String, PathBuf)> {
    let Some(text) = hub_dir().and_then(|dir| std::fs::read_to_string(dir.join("recent.txt")).ok()) else {
        return Vec::new();
    };
    parse_recent(&text)
}

fn parse_recent(text: &str) -> Vec<(String, PathBuf)> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let _time = parts.next()?;
            let name = parts.next()?.trim().to_string();
            let path = PathBuf::from(parts.next()?.trim());
            (!name.is_empty() && path.is_dir()).then_some((name, path))
        })
        .collect()
}

/// Хаб работает: его отметка моложе 5 секунд.
fn hub_alive(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("hub.alive")) else {
        return false;
    };
    let time: u64 = text.lines().nth(1).and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    now.saturating_sub(time) < 5
}

/// Запустить хаб: программа `piska-hub` рядом с программой редактора (тот же каталог
/// сборки), иначе `cargo run -p piska-hub` в рабочем пространстве движка.
fn launch_hub(workspace: Option<&Path>, open: Option<&Path>) -> Result<Child, String> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(format!("piska-hub{}", std::env::consts::EXE_SUFFIX))))
        .filter(|hub| hub.is_file());
    let mut command = match (exe, workspace) {
        (Some(hub), _) => engine_build::host::command(hub),
        (None, Some(workspace)) => {
            let mut cargo = engine_build::host::command(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
            cargo.args(["run", "-p", "piska-hub", "--"]).current_dir(workspace);
            cargo
        }
        (None, None) => {
            return Err(
                tr_s("Piska Hub was not found next to the editor and the engine workspace is unknown; \
                 start it with `cargo run -p piska-hub` in the engine folder")
                    .to_string(),
            )
        }
    };
    if let Some(open) = open {
        command.arg("--open").arg(open);
    }
    command.spawn().map_err(|e| trf!("cannot start Piska Hub: {e}", e = e))
}

impl Editor {
    /// File → Project Hub… (`open: None`) и File → Open Recent (`open: Some(каталог)`):
    /// показать хаб (или попросить его открыть проект) и закрыть редактор. Хаб, которого
    /// пришлось запустить, этот процесс дожидается после закрытия окна: `cargo run`
    /// редактора на Windows иначе закрыл бы хаб вместе с собой.
    pub(crate) fn go_to_hub(&mut self, open: Option<PathBuf>) -> Result<(), EditorError> {
        let dir = hub_dir();
        let alive = dir.as_deref().is_some_and(hub_alive);
        if let (true, Some(dir)) = (alive, dir.as_deref()) {
            let request = match &open {
                Some(path) => format!("open {}\n", path.display()),
                None => "show\n".to_string(),
            };
            std::fs::write(dir.join("hub.request"), request)
                .map_err(|e| EditorError::Other(trf!("cannot reach Piska Hub through `{p0}`: {e}", p0 = dir.display(), e = e)))?;
        } else {
            let child = launch_hub(self.config.workspace.as_deref(), open.as_deref()).map_err(EditorError::Other)?;
            self.set_successor(child);
        }
        log::info!("handing over to Piska Hub");
        self.quit = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recent_list_keeps_existing_folders_in_order() {
        let here = std::env::current_dir().unwrap();
        let text = format!(
            "# header\n20\tNewer\t{}\n10\tGone\t{}\nbroken line\n",
            here.display(),
            here.join("no-such-folder").display()
        );
        assert_eq!(parse_recent(&text), vec![("Newer".to_string(), here)]);
    }
}
