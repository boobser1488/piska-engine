//! Файлы данных игры (`.ron`, `.json`): таблицы уровней, баланса, волн. Читаются через
//! базу ассетов — значит, одинаково из каталога проекта и из пака экспортированной игры.
//!
//! ```no_run
//! # use engine_assets::AssetDatabase;
//! #[derive(serde::Deserialize)]
//! struct Level { name: String, need: u32 }
//!
//! # fn demo(db: &AssetDatabase) -> Result<(), engine_assets::AssetError> {
//! let levels: Vec<Level> = db.load_data("data/levels.ron")?;
//! # Ok(()) }
//! ```

use serde::de::DeserializeOwned;

use crate::{AssetDatabase, AssetError};

/// Формат файла данных — по расширению.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFormat {
    Ron,
    Json,
}

impl DataFormat {
    /// Формат по пути (`.ron`, `.json`); другое расширение — `None`.
    pub fn of(path: &str) -> Option<Self> {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".ron") {
            Some(Self::Ron)
        } else if lower.ends_with(".json") {
            Some(Self::Json)
        } else {
            None
        }
    }
}

/// Разобрать текст файла данных `path` (формат — по расширению) в `T`.
pub fn parse_data<T: DeserializeOwned>(path: &str, text: &str) -> Result<T, AssetError> {
    let import = |reason: String| AssetError::Import { path: path.to_string(), reason };
    match DataFormat::of(path) {
        Some(DataFormat::Ron) => ron::from_str(text).map_err(|e| {
            import(format!(
                "line {}, column {}: {}. Fix the RON file (the expected shape is the Rust type it is read into)",
                e.position.line, e.position.col, e.code
            ))
        }),
        Some(DataFormat::Json) => serde_json::from_str(text).map_err(|e| {
            import(format!("line {}, column {}: {e}. Fix the JSON file", e.line(), e.column()))
        }),
        None => Err(import("data files must end in `.ron` or `.json`".to_string())),
    }
}

impl AssetDatabase {
    /// Прочитать файл данных (`.ron` или `.json`, путь относительно корня ассетов) в тип
    /// `T` (`#[derive(serde::Deserialize)]`). Ошибка называет файл, строку и столбец.
    /// Каждый вызов читает файл заново: правка файла видна при следующей загрузке.
    pub fn load_data<T: DeserializeOwned>(&self, relative_path: &str) -> Result<T, AssetError> {
        let bytes = self.read_path(relative_path)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| AssetError::Import {
            path: relative_path.to_string(),
            reason: "the file is not UTF-8 text".to_string(),
        })?;
        parse_data(relative_path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Level {
        name: String,
        need: u32,
    }

    fn project(tag: &str, files: &[(&str, &str)]) -> (std::path::PathBuf, AssetDatabase) {
        let dir = std::env::temp_dir().join(format!("piska_data_assets_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        for (path, text) in files {
            std::fs::write(dir.join(path), text).unwrap();
        }
        let db = AssetDatabase::open(&dir).unwrap();
        (dir, db)
    }

    #[test]
    fn ron_and_json_files_load_into_types() {
        let (dir, db) = project("load", &[
            ("data/levels.ron", r#"[(name: "Cave", need: 5), (name: "Lake", need: 7)]"#),
            ("data/levels.json", r#"[{"name": "Cave", "need": 5}]"#),
        ]);
        let ron: Vec<Level> = db.load_data("data/levels.ron").unwrap();
        assert_eq!(ron, vec![Level { name: "Cave".into(), need: 5 }, Level { name: "Lake".into(), need: 7 }]);
        let json: Vec<Level> = db.load_data("data/levels.json").unwrap();
        assert_eq!(json[0].need, 5);
        // Файл перечитывается: правка видна при следующей загрузке.
        std::fs::write(dir.join("data/levels.json"), r#"[{"name": "Cave", "need": 9}]"#).unwrap();
        let json: Vec<Level> = db.load_data("data/levels.json").unwrap();
        assert_eq!(json[0].need, 9);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_name_the_file_and_the_line() {
        let (dir, db) = project("errors", &[("data/bad.ron", "[\n(name: \"Cave\", need: \"five\")]"), ("data/x.txt", "1")]);
        let err = db.load_data::<Vec<Level>>("data/bad.ron").unwrap_err().to_string();
        assert!(err.contains("data/bad.ron") && err.contains("line 2"), "{err}");
        let err = db.load_data::<u32>("data/x.txt").unwrap_err().to_string();
        assert!(err.contains(".ron") && err.contains(".json"), "{err}");
        let err = db.load_data::<u32>("data/missing.ron").unwrap_err().to_string();
        assert!(err.contains("missing.ron"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
