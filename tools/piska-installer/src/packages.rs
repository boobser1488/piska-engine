//! `install/packages.toml` — тот же файл данных, что читают скрипты и `doctor`: компоненты
//! (зависимости, размеры), адрес репозитория, расширение файла проекта.

use crate::scripts::PACKAGES_TOML;

/// Компонент установки.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    /// `core`, `cpp`, `web`, `android`, `linux-cross`, `engine`.
    pub id: String,
    /// Состав (английский технический текст из packages.toml; в интерфейсе — свои описания).
    pub contents: String,
    /// Место на диске, байт (для этой ОС).
    pub disk: u64,
    /// Скачивание, байт (примерно; для этой ОС).
    pub download: u64,
    /// Подпись размера из файла как есть («6 GB»).
    pub disk_text: String,
    pub download_text: String,
    /// Компоненты, без которых этот не ставится.
    pub needs: Vec<String>,
}

/// Данные установщиков.
#[derive(Clone, Debug)]
pub struct Packages {
    /// Компоненты в порядке скриптов.
    pub components: Vec<Component>,
    pub repo: String,
    pub branch: String,
    pub project_extension: String,
    pub project_file_description: String,
    /// Имя каталога состояния (`%LOCALAPPDATA%/<имя>/install`, `~/.local/state/<имя>/install`).
    pub state_dir_name: String,
    /// Имя каталога установки по умолчанию.
    pub install_dir_name: String,
}

/// «6 GB» → байты (GB / MB / KB, десятичная точка); непонятное — 0.
pub fn parse_size(text: &str) -> u64 {
    let mut parts = text.split_whitespace();
    let (Some(number), Some(unit)) = (parts.next(), parts.next()) else {
        return 0;
    };
    let Ok(value) = number.parse::<f64>() else {
        return 0;
    };
    let scale = match unit.to_ascii_uppercase().as_str() {
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "MB" => 1024.0 * 1024.0,
        "KB" => 1024.0,
        _ => return 0,
    };
    (value * scale) as u64
}

impl Packages {
    /// Вшитый файл. Он проверяется тестами, поэтому ошибка разбора — ошибка сборки, а не пользователя.
    pub fn embedded() -> Packages {
        Self::parse(PACKAGES_TOML, cfg!(windows)).expect("install/packages.toml (embedded) is valid")
    }

    pub fn parse(text: &str, windows: bool) -> Result<Packages, String> {
        let doc: toml::Value = text.parse().map_err(|e| format!("packages.toml: {e}"))?;
        let get = |section: &str, key: &str| -> Result<String, String> {
            doc.get(section)
                .and_then(|s| s.get(key))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("packages.toml: no `{section}.{key}`"))
        };
        let table = doc.get("component").and_then(|c| c.as_table()).ok_or("packages.toml: no [component.*] sections")?;
        // Порядок скриптов (`$AllComponents`): toml-таблица его не хранит.
        let order = ["core", "cpp", "web", "android", "linux-cross", "engine"];
        let mut components = Vec::new();
        for id in order {
            let Some(c) = table.get(id) else {
                return Err(format!("packages.toml: no [component.{id}]"));
            };
            let field = |key: &str| c.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let (disk_text, download_text) = if windows {
                (field("size_windows"), field("download_windows"))
            } else {
                (field("size_unix"), field("download_unix"))
            };
            components.push(Component {
                id: id.to_string(),
                contents: field("title"),
                disk: parse_size(&disk_text),
                download: parse_size(&download_text),
                disk_text,
                download_text,
                needs: c
                    .get("needs")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
            });
        }
        if table.len() != order.len() {
            return Err("packages.toml: a component the installer does not know".into());
        }
        Ok(Packages {
            components,
            repo: get("project", "repo")?,
            branch: get("project", "branch")?,
            project_extension: get("project", "project_extension")?,
            project_file_description: get("project", "project_file_description")?,
            state_dir_name: get("project", "state_dir_name")?,
            install_dir_name: get("project", "install_dir_name")?,
        })
    }

    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    /// Выбор вместе с зависимостями, в порядке скриптов (как `Resolve-Components`).
    pub fn resolve(&self, wanted: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        fn add(p: &Packages, id: &str, out: &mut Vec<String>) {
            if out.iter().any(|c| c == id) {
                return;
            }
            if let Some(c) = p.component(id) {
                for need in &c.needs {
                    add(p, need, out);
                }
            }
            out.push(id.to_string());
        }
        for c in &self.components {
            if wanted.contains(&c.id) {
                add(self, &c.id, &mut out);
            }
        }
        self.components.iter().filter(|c| out.contains(&c.id)).map(|c| c.id.clone()).collect()
    }

    /// Кто из выбранных требует `id` (для пояснения «отмечено, потому что …»).
    pub fn required_by(&self, id: &str, selected: &[String]) -> Vec<String> {
        self.components
            .iter()
            .filter(|c| c.id != id && selected.contains(&c.id) && c.needs.iter().any(|n| n == id))
            .map(|c| c.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_file_has_every_component_and_the_brand_extension() {
        for windows in [true, false] {
            let p = Packages::parse(PACKAGES_TOML, windows).unwrap();
            assert_eq!(p.components.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["core", "cpp", "web", "android", "linux-cross", "engine"]);
            for c in &p.components {
                assert!(c.disk > 0 && c.download > 0, "{} has sizes", c.id);
                assert!(c.download <= c.disk, "{}: download larger than on disk", c.id);
            }
        }
        let p = Packages::embedded();
        assert_eq!(p.project_extension, engine_editor::kit::i18n::brand::PROJECT_EXTENSION, "packages.toml and brand agree");
        assert_eq!(p.project_file_description, engine_editor::kit::i18n::brand::PROJECT_FILE_DESCRIPTION);
    }

    #[test]
    fn dependencies_resolve_like_the_scripts() {
        let p = Packages::embedded();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(p.resolve(&s(&["web"])), s(&["core", "web"]));
        assert_eq!(p.resolve(&s(&["engine", "android"])), s(&["core", "android", "engine"]));
        assert_eq!(p.required_by("core", &s(&["core", "web", "cpp"])), s(&["cpp", "web"]));
        assert_eq!(parse_size("1.3 GB"), (1.3 * 1024.0 * 1024.0 * 1024.0) as u64);
        assert_eq!(parse_size("200 MB"), 200 * 1024 * 1024);
        assert_eq!(parse_size("lots"), 0);
    }
}
