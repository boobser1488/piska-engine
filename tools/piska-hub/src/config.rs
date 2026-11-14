//! Файл хаба `hub.ron`: добавленные проекты, закреплённые, время последнего открытия,
//! настройки. Переживает перезапуск; испорченный файл не мешает запуску.

use engine_editor::kit::i18n::{tr_s, trf, trn};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

/// Всё, что хаб помнит между запусками.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubConfig {
    /// Проекты, о которых хаб что-то знает: добавленные вручную и открывавшиеся.
    /// Игры рабочего пространства (`games/*`) находятся сами и сюда попадают, только
    /// когда их открывали, закрепили или убрали из списка.
    pub projects: Vec<ProjectEntry>,
    pub settings: Settings,
}

/// Запись о проекте.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectEntry {
    /// Каталог крейта игры.
    pub path: PathBuf,
    /// Закреплён вверху списка.
    pub favorite: bool,
    /// Когда открывали последний раз (секунды Unix).
    pub last_opened: Option<u64>,
    /// Убран из списка (для игр рабочего пространства, которые иначе нашлись бы снова).
    pub hidden: bool,
    /// Сколько единиц компиляции было в последней сборке редактора — знаменатель
    /// полосы хода сборки, пока cargo не сообщил свой.
    pub build_units: Option<u32>,
    /// Программа редактора, собранная последней удачной сборкой (Open без сборки).
    pub editor_exe: Option<PathBuf>,
    /// Программа игры из последней удачной сборки (Play без сборки).
    pub game_exe: Option<PathBuf>,
}

/// Что делать с хабом, когда проект открылся в редакторе.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AfterOpen {
    /// Спрятаться и вернуться, когда редактор закроют.
    #[default]
    Hide,
    /// Закрыть окно (процесс дождётся редактора без окна и завершится).
    Close,
}

/// Вид списка проектов.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    #[default]
    List,
    Grid,
}

/// Порядок списка (закреплённые всегда сверху).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    #[default]
    LastOpened,
    Name,
}

/// Настройки хаба (страница Settings). Масштаб интерфейса и акцент общие с
/// редактором и хранятся в его файле настроек.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Где создавать новые проекты; `None` — `games/` рабочего пространства движка.
    pub default_location: Option<PathBuf>,
    pub after_open: AfterOpen,
    pub view: ViewMode,
    pub sort: SortOrder,
    /// Каталоги, в которых хаб создавал проекты: из них можно удалять проекты с диска.
    pub project_roots: Vec<PathBuf>,
    /// Где мастер создал проект в прошлый раз (с него он начинает).
    pub last_location: Option<PathBuf>,
    /// Шаблон прошлого раза (`Empty 2D`, `Empty 3D`, `Starfall sample`).
    pub last_template: Option<String>,
    /// Куда импортировали проекты в прошлый раз.
    pub last_import_location: Option<PathBuf>,
    /// Куда сохраняли файл проекта в прошлый раз.
    pub last_pack_dir: Option<PathBuf>,
}

impl HubConfig {
    /// Прочитать файл. Нет файла — пустой список; испорченный — пустой список и
    /// объяснение (файл не перезаписывается, пока что-нибудь не поменяется).
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match ron::from_str::<HubConfig>(&text) {
                Ok(config) => (config, None),
                Err(err) => (
                    Self::default(),
                    Some(trf!(
                        "`{p0}` could not be read ({err}); starting with an empty list. \
                         Fix or delete the file to restore it.",
                        p0 = path.display()
                    , err = err)),
                ),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Self::default(), None),
            Err(err) => (Self::default(), Some(trf!("`{p0}` could not be read: {err}", p0 = path.display(), err = err))),
        }
    }

    /// Текст файла.
    pub fn to_text(&self) -> String {
        let pretty = ron::ser::PrettyConfig::new().depth_limit(4).indentor("    ".to_string());
        let body = ron::ser::to_string_pretty(self, pretty).unwrap_or_default();
        format!("// Piska Hub: project list and settings. Written by the hub; safe to edit while it is closed.\n{body}\n")
    }

    /// Записать (через временный файл: оборванная запись не портит список).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("ron.tmp");
        std::fs::write(&temp, self.to_text())?;
        std::fs::rename(&temp, path)
    }

    pub fn entry(&self, path: &Path) -> Option<&ProjectEntry> {
        self.projects.iter().find(|entry| paths::same(&entry.path, path))
    }

    /// Запись о проекте; нет — создаётся.
    pub fn entry_mut(&mut self, path: &Path) -> &mut ProjectEntry {
        let index = match self.projects.iter().position(|entry| paths::same(&entry.path, path)) {
            Some(index) => index,
            None => {
                self.projects.push(ProjectEntry {
                    path: paths::clean(path),
                    ..ProjectEntry::default()
                });
                self.projects.len() - 1
            }
        };
        &mut self.projects[index]
    }

    /// Убрать запись совсем.
    pub fn forget(&mut self, path: &Path) {
        self.projects.retain(|entry| !paths::same(&entry.path, path));
    }

    /// Отметить открытие проекта сейчас.
    pub fn touch(&mut self, path: &Path, now: u64) {
        let entry = self.entry_mut(path);
        entry.last_opened = Some(now);
        entry.hidden = false;
    }
}

/// Текущее время в секундах Unix.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Текст `recent.txt` для редактора: недавно открытые проекты, новые сверху.
/// `items` — (время, имя, путь).
pub fn recent_text(items: &[(u64, String, PathBuf)]) -> String {
    let mut sorted: Vec<_> = items.iter().collect();
    sorted.sort_by_key(|item| std::cmp::Reverse(item.0));
    let mut text = String::from("# Piska Hub: recently opened projects (time\tname\tpath), newest first\n");
    for (time, name, path) in sorted.into_iter().take(12) {
        let name = name.replace(['\t', '\n'], " ");
        text.push_str(&format!("{time}\t{name}\t{}\n", path.display()));
    }
    text
}

/// «5 minutes ago», «Yesterday», «2026-11-02».
pub fn ago(then: Option<u64>, now: u64) -> String {
    let Some(then) = then else {
        return tr_s("Never").to_string();
    };
    let seconds = now.saturating_sub(then);
    match seconds {
        0..=59 => tr_s("Just now").to_string(),
        60..=3599 => trn!("{n} minute ago", "{n} minutes ago", seconds / 60),
        3600..=86_399 => trn!("{n} hour ago", "{n} hours ago", seconds / 3600),
        86_400..=172_799 => tr_s("Yesterday").to_string(),
        172_800..=604_799 => trn!("{n} day ago", "{n} days ago", seconds / 86_400),
        _ => date(then),
    }
}

/// Дата `ГГГГ-ММ-ДД` (UTC) из секунд Unix.
pub fn date(unix: u64) -> String {
    // Алгоритм «civil from days» (Howard Hinnant).
    let days = (unix / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_the_file() {
        let dir = std::env::temp_dir().join(format!("piska-hub-config-{}", std::process::id()));
        let file = dir.join("hub.ron");
        let mut config = HubConfig::default();
        config.entry_mut(&dir.join("a")).favorite = true;
        config.touch(&dir.join("b"), 1_700_000_000);
        config.entry_mut(&dir.join("c")).hidden = true;
        config.entry_mut(&dir.join("b")).build_units = Some(412);
        config.settings = Settings {
            default_location: Some(dir.join("projects")),
            after_open: AfterOpen::Close,
            view: ViewMode::Grid,
            sort: SortOrder::Name,
            project_roots: vec![dir.join("projects")],
            last_location: Some(dir.join("projects")),
            last_template: Some("Empty 3D".into()),
            last_import_location: Some(dir.join("imported")),
            last_pack_dir: Some(dir.join("saved")),
        };
        config.save(&file).unwrap();
        let (loaded, problem) = HubConfig::load(&file);
        assert_eq!(problem, None);
        assert_eq!(loaded, config);
        assert_eq!(config.projects.len(), 3, "the same path is one entry");
        assert!(loaded.entry(&dir.join("b").join("..").join("b")).is_some(), "paths are compared normalized");

        // Испорченный файл: пустой список и понятное объяснение.
        std::fs::write(&file, "(projects: [ oops").unwrap();
        let (broken, problem) = HubConfig::load(&file);
        assert_eq!(broken, HubConfig::default());
        assert!(problem.unwrap().contains("could not be read"));
        // Нет файла — тихо пустой.
        let (empty, problem) = HubConfig::load(&dir.join("missing.ron"));
        assert_eq!((empty, problem), (HubConfig::default(), None));
        // Старый файл без новых полей читается.
        std::fs::write(&file, "(projects: [(path: \"x\")])").unwrap();
        let (old, problem) = HubConfig::load(&file);
        assert!(problem.is_none());
        assert_eq!(old.projects[0].path, PathBuf::from("x"));
        assert_eq!(old.settings.after_open, AfterOpen::Hide);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn times_read_like_people_say_them() {
        let now = 1_763_000_000;
        assert_eq!(ago(None, now), "Never");
        assert_eq!(ago(Some(now - 5), now), "Just now");
        assert_eq!(ago(Some(now - 60), now), "1 minute ago");
        assert_eq!(ago(Some(now - 3 * 3600), now), "3 hours ago");
        assert_eq!(ago(Some(now - 90_000), now), "Yesterday");
        assert_eq!(ago(Some(now - 4 * 86_400), now), "4 days ago");
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_763_000_000), "2025-11-13");
        assert_eq!(date(951_782_400), "2000-02-29");
    }

    #[test]
    fn the_recent_list_is_newest_first() {
        let text = recent_text(&[
            (10, "old".into(), PathBuf::from("a")),
            (30, "new".into(), PathBuf::from("b")),
        ]);
        let lines: Vec<&str> = text.lines().skip(1).collect();
        assert_eq!(lines, ["30\tnew\tb", "10\told\ta"]);
    }
}
