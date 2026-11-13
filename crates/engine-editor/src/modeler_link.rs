//! Связь с Piska Modeler (`tools/piska-modeler`): двойной щелчок по `.pmodel` в окне
//! Project открывает модель в окне редактора моделей, Assets → New Model создаёт пустую.
//!
//! Редактор моделей — отдельная программа: сохранённая в нём модель подхватывается, когда
//! окно редактора снова получает фокус (ассеты перечитываются, меш на GPU обновляется).

use std::path::{Path, PathBuf};

use crate::EditorError;

/// Пустая модель: заготовка, которую Piska Modeler откроет и дополнит при сохранении.
const EMPTY_MODEL: &str = "(\n    format: \"piska-model\",\n    version: 1,\n    model: (\n        name: \"NAME\",\n        units_per_metre: 16.0,\n        texel_density: 1.0,\n        textures: [],\n        nodes: [],\n        next_id: 1,\n    ),\n)\n";

/// Программа Piska Modeler: `PISKA_MODELER`, иначе рядом с программой редактора (или
/// уровнем выше — для примеров из `target/<профиль>/examples`).
pub(crate) fn modeler_executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PISKA_MODELER").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let name = format!("piska-modeler{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
}

/// Открыть модель в Piska Modeler.
pub(crate) fn open_model(file: &Path) -> Result<(), EditorError> {
    let exe = modeler_executable().ok_or_else(|| {
        EditorError::Other(
            "Piska Modeler is not built next to the editor: run `cargo build -p piska-modeler` \
             (or set PISKA_MODELER to its path)"
                .to_string(),
        )
    })?;
    std::process::Command::new(&exe)
        .arg(file)
        .spawn()
        .map_err(|e| EditorError::Other(format!("cannot start `{}`: {e}", exe.display())))?;
    log::info!("opened {} in Piska Modeler", file.display());
    Ok(())
}

/// Создать пустую модель `model.pmodel` (`model2.pmodel`…) в каталоге; путь относительно
/// корня ассетов.
pub(crate) fn create_model(root: &Path, folder: &str) -> Result<String, EditorError> {
    let folder = folder.trim_matches('/');
    let dir = if folder.is_empty() { root.to_path_buf() } else { root.join(folder) };
    let (name, path) = (1..)
        .map(|i| if i == 1 { "model".to_string() } else { format!("model{i}") })
        .map(|name| {
            let path = dir.join(format!("{name}.pmodel"));
            (name, path)
        })
        .find(|(_, path)| !path.exists())
        .expect("a free name exists");
    std::fs::write(&path, EMPTY_MODEL.replace("NAME", &name))
        .map_err(|e| EditorError::Other(format!("cannot create `{}`: {e}", path.display())))?;
    Ok(if folder.is_empty() { format!("{name}.pmodel") } else { format!("{folder}/{name}.pmodel") })
}
