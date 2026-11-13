//! Сценарии на языке Scenario в редакторе: «New Scenario Script…» (меню Assets,
//! контекстное меню папки в Project, Add Component в инспекторе) создаёт
//! `assets/scenarios/<name>.scenario` из короткого шаблона, открывает его во вкладке Code
//! и, если выбран объект, вешает на него `ScenarioRunner`. Сборка не нужна: сценарий
//! перезапускается сам, когда файл сохранён.

use engine_core::error::SceneError;
use engine_core::object::GameObjectId;
use engine_core::reflect::Value;

use crate::state::{Dialog, Editor, ViewTab};
use crate::EditorError;

/// Шаблон нового сценария. `{name}` заменяется именем файла.
pub(crate) const SCENARIO_TEMPLATE: &str = r#"-- {name}.scenario — сценарий на языке Scenario (docs/api/scenario.md).
-- Сохраните файл (Ctrl+S): в Play сценарий сразу перезапустится, без пересборки.

log("{name}: started on " .. self.name)

-- Ждать секунду, потом три раза по полсекунды.
wait(1)
for i = 1, 3 do
    log("tick " .. i)
    wait(0.5)
end
"#;

/// Каталог сценариев по умолчанию (относительно ассетов).
pub(crate) const SCENARIO_DIR: &str = "scenarios";

impl Editor {
    /// Открыть диалог «New Scenario Script». `folder` — каталог в ассетах
    /// (пусто — `scenarios`), `object` — на какой объект повесить `ScenarioRunner`.
    pub(crate) fn open_scenario_dialog(&mut self, folder: &str, object: Option<GameObjectId>) {
        self.dialog = Some(Dialog::NewScenario { name: String::new(), folder: folder.to_string(), object });
    }

    /// Создать сценарий: файл из шаблона, обновить базу ассетов, открыть во вкладке
    /// Code, повесить `ScenarioRunner` на `attach_to`.
    pub(crate) fn new_scenario(&mut self, name: &str, folder: &str, attach_to: Option<GameObjectId>) -> Result<(), EditorError> {
        let root = self.require_assets_root("create a scenario")?;
        let name = name.trim().trim_end_matches(".scenario").trim();
        let valid = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
        if !valid {
            return Err(EditorError::Other(format!(
                "`{name}` cannot be a scenario name: use letters, digits, `_` or `-`, for example `intro` or `wave_1`"
            )));
        }
        let folder = folder.trim().trim_matches('/');
        let folder = if folder.is_empty() { SCENARIO_DIR } else { folder };
        let relative = format!("{folder}/{name}.scenario");
        let path = root.join(&relative);
        if path.exists() {
            return Err(EditorError::Other(format!("`{relative}` already exists; pick another name")));
        }
        let io = |source: std::io::Error| SceneError::Io { path: path.display().to_string(), source };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(&path, SCENARIO_TEMPLATE.replace("{name}", name)).map_err(io)?;
        log::info!("created `{relative}`");
        self.apply(crate::state::Action::RefreshAssets);
        self.code.open(&path).map_err(EditorError::Other)?;
        self.view_tab = ViewTab::Code;
        if let Some(object) = attach_to {
            self.attach_scenario(object, &relative)?;
        }
        Ok(())
    }

    /// Повесить `ScenarioRunner` с этим сценарием на объект.
    fn attach_scenario(&mut self, object: GameObjectId, relative: &str) -> Result<(), EditorError> {
        if !self.app.world.registry.contains("ScenarioRunner") {
            return Err(EditorError::Other(
                "`ScenarioRunner` is not registered: start the editor from `engine::app()` to use scenarios".to_string(),
            ));
        }
        let guid = self
            .app
            .world
            .resources
            .get::<engine_assets::AssetDatabase>()
            .and_then(|db| db.guid_of(relative))
            .ok_or_else(|| EditorError::Other(format!("`{relative}` is not in the asset database")))?;
        let mut runner = self.app.world.registry.create("ScenarioRunner")?;
        runner
            .set_field("script", &Value::Asset(guid.to_string()))
            .map_err(|e| EditorError::Other(e.to_string()))?;
        if !self.app.world.scene.contains(object) {
            return Ok(());
        }
        self.touch();
        let obj = self.app.world.scene.get_mut(object).ok_or(SceneError::DeadObject(object))?;
        obj.add_boxed(runner);
        log::info!("`{relative}` attached to `{}` (ScenarioRunner)", obj.name);
        Ok(())
    }
}
