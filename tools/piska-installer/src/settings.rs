//! Свои настройки установщика: размер окна, язык и последний выбор. Файл
//! `<каталог настроек бренда>/installer.ron` (`%APPDATA%/piska`, `~/.config/piska`);
//! `PISKA_INSTALLER_CONFIG_DIR` его заменяет (тесты, переносной запуск).

use std::path::{Path, PathBuf};

use engine_editor::kit::i18n::brand;
use serde::{Deserialize, Serialize};

pub const FILE: &str = "installer.ron";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Размер окна в логических точках.
    pub window: Option<(f32, f32)>,
    /// `en` / `ru`; `None` — язык системы.
    pub language: Option<String>,
    pub ui_scale: f32,
    pub components: Vec<String>,
    pub install_dir: Option<String>,
    /// `existing` / `download`.
    pub source: Option<String>,
    pub source_dir: Option<String>,
    pub start_menu: bool,
    pub desktop: bool,
    pub file_associations: bool,
    pub write_prefs: bool,
    pub cache: Option<String>,
    pub proxy: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            window: None,
            language: None,
            ui_scale: 1.0,
            components: Vec::new(),
            install_dir: None,
            source: None,
            source_dir: None,
            start_menu: true,
            desktop: false,
            file_associations: false,
            write_prefs: true,
            cache: None,
            proxy: None,
        }
    }
}

/// Каталог настроек бренда.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PISKA_INSTALLER_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join(brand::CONFIG_DIR_NAME))
}

impl Settings {
    /// Прочитать; нет файла или он испорчен — настройки по умолчанию.
    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join(FILE)).ok().and_then(|text| ron::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?;
        // Сначала во временный файл: оборванная запись не портит прежние настройки.
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, dir.join(FILE))
    }
}
