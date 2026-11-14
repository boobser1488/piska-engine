//! Вкладка Samples: игры-примеры движка (с картинками) и документация `docs/api/*.md`.

use engine_editor::kit::i18n::{n_};
use std::path::{Path, PathBuf};

/// Игра-пример из `games/` движка.
pub struct Sample {
    /// Каталог в `games/`.
    pub folder: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    /// Кадр из игры (JPEG 512×288).
    pub picture: &'static [u8],
}

pub const SAMPLES: [Sample; 3] = [
    Sample {
        folder: "swarm",
        title: n_("Swarm"),
        summary: n_("Arena survival shooter: waves, bosses, upgrades, a sprite atlas and a thousand bullets on screen."),
        picture: include_bytes!("../assets/samples/swarm.jpg"),
    },
    Sample {
        folder: "lantern",
        title: n_("Lantern Keeper"),
        summary: n_("2D platformer with tilemaps, light, enemies, a boss, menus and saved progress across levels."),
        picture: include_bytes!("../assets/samples/lantern.jpg"),
    },
    Sample {
        folder: "marble",
        title: n_("Marble Factory"),
        summary: n_("3D physics course with PBR materials, fog and bloom; level mechanisms are written in C++."),
        picture: include_bytes!("../assets/samples/marble.jpg"),
    },
];

/// Пример по каталогу проекта (`games/swarm` → Swarm).
pub fn sample_for(dir: &Path, engine_root: Option<&Path>) -> Option<&'static Sample> {
    let root = engine_root?;
    SAMPLES
        .iter()
        .find(|sample| crate::paths::same(dir, &root.join("games").join(sample.folder)))
}

/// Страница документации.
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub path: PathBuf,
    /// Заголовок первой строки `# …`.
    pub title: String,
    /// `editor.md`.
    pub file: String,
}

/// Руководства `docs/api/*.md` движка, по имени файла.
pub fn docs(engine_root: &Path) -> Vec<Doc> {
    let mut docs: Vec<Doc> = std::fs::read_dir(engine_root.join("docs").join("api"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e == "md"))
                .map(|path| {
                    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let title = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|text| {
                            text.lines()
                                .find_map(|line| line.strip_prefix("# ").map(|t| t.trim().to_string()))
                        })
                        .unwrap_or_else(|| file.trim_end_matches(".md").to_string());
                    Doc { path, title, file }
                })
                .collect()
        })
        .unwrap_or_default();
    docs.sort_by(|a, b| a.file.cmp(&b.file));
    docs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_pictures_decode_and_docs_have_titles() {
        for sample in &SAMPLES {
            let image = image::load_from_memory(sample.picture).unwrap();
            assert_eq!((image.width(), image.height()), (512, 288), "{}", sample.folder);
        }
        let root = crate::paths::find_engine_root().unwrap();
        let docs = docs(&root);
        let editor = docs.iter().find(|d| d.file == "editor.md").expect("docs/api/editor.md");
        assert!(editor.title.contains("Редактор"));
        assert!(sample_for(&root.join("games").join("swarm"), Some(&root)).is_some_and(|s| s.title == "Swarm"));
        assert!(sample_for(&root.join("games").join("template"), Some(&root)).is_none());
    }
}
