//! Публичные команды редактора для автоматизации и тестов (скриншоты, сценарии): то же,
//! что пользователь делает мышью.

use crate::state::Editor;
use crate::EditorError;

impl Editor {
    /// Открыть каталог в окне Project (как щелчок по нему в дереве): путь относительно
    /// каталога ассетов, `""` — корень; принимается и `Assets/…`, и `\`. Ошибка — каталога
    /// нет (или у проекта нет каталога ассетов).
    pub fn set_project_folder(&mut self, folder: &str) -> Result<(), EditorError> {
        let mut folder = folder.trim().replace('\\', "/");
        if folder == "Assets" {
            folder.clear();
        } else if let Some(rest) = folder.strip_prefix("Assets/") {
            folder = rest.to_string();
        }
        let folder = folder.trim_matches('/').to_string();
        let root = self
            .assets_root()
            .ok_or_else(|| EditorError::Other("this project has no assets folder".to_string()))?;
        if !root.join(&folder).is_dir() {
            return Err(EditorError::Other(format!(
                "there is no folder `{folder}` in the project assets (paths are relative to the assets folder)"
            )));
        }
        self.project.go(&folder);
        Ok(())
    }
}
