//! Связь с Piska Modeler (`modeler/`, отдельный репозиторий): двойной щелчок по `.pmodel` в окне
//! Project открывает модель в окне редактора моделей, Assets → New Model создаёт пустую.
//!
//! Редактор моделей — отдельная программа: сохранённая в нём модель подхватывается, когда
//! окно редактора снова получает фокус (ассеты перечитываются, меш на GPU обновляется).

use crate::i18n::{n_, trf};
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
    // Рядом с программой редактора (target/<профиль>, или уровнем выше для examples/deps),
    // затем в CARGO_TARGET_DIR: `cargo build --manifest-path modeler/Cargo.toml` кладёт его туда.
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|dir| dir.join(&name))
        .chain(target.into_iter().flat_map(|t| [t.join("debug").join(&name), t.join("release").join(&name)]))
        .find(|candidate| candidate.is_file())
}

// Запуск программы — `Editor::launch_modeler` (help.rs): без неё — окно с объяснением.

/// Чужие форматы мешей, которые Piska Modeler берёт опорным мешем: для них в меню
/// «Edit a Copy in Modeler» (новая `.pmodel` рядом, меш в ней — серая опора для обрисовки).
pub(crate) const COPY_FORMATS: &[&str] = &["obj", "glb", "gltf", "bbmodel"];

/// Подсказка неактивного пункта, когда программы Piska Modeler нет.
pub(crate) const MODELER_MISSING: &str =
    n_("Piska Modeler is not installed: build it with `cargo build --manifest-path modeler/Cargo.toml` or set PISKA_MODELER");

/// Пункт меню «… in Modeler» для файла.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelerEntry {
    /// `.pmodel` — открыть и править саму модель.
    Edit,
    /// Чужой меш — новая модель с ним как опорой.
    EditCopy,
}

impl ModelerEntry {
    /// Какой пункт нужен файлу (`None` — не модель).
    pub fn for_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if ext == "pmodel" {
            Some(Self::Edit)
        } else if COPY_FORMATS.contains(&ext.as_str()) {
            Some(Self::EditCopy)
        } else {
            None
        }
    }

    /// Подпись (английский ключ перевода).
    pub fn label(self) -> &'static str {
        match self {
            Self::Edit => n_("Edit in Modeler"),
            Self::EditCopy => n_("Edit a Copy in Modeler"),
        }
    }
}

/// Где появится копия чужого меша: `<имя>.pmodel` (`<имя>2.pmodel`…) рядом с исходником.
pub(crate) fn copy_target(source: &Path) -> PathBuf {
    let dir = source.parent().unwrap_or(Path::new(""));
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".to_string());
    (1..)
        .map(|i| if i == 1 { dir.join(format!("{stem}.pmodel")) } else { dir.join(format!("{stem}{i}.pmodel")) })
        .find(|path| !path.exists())
        .expect("a free name exists")
}

/// Аргументы Piska Modeler: открыть `model`, добавить опорный меш `source` и сохранить.
pub(crate) fn copy_args(model: &Path, source: &Path) -> Vec<std::ffi::OsString> {
    vec![model.as_os_str().to_owned(), "--reference".into(), source.as_os_str().to_owned()]
}

/// Пустая модель с именем `name` (для копии чужого меша).
pub(crate) fn empty_model(name: &str) -> String {
    EMPTY_MODEL.replace("NAME", &name.replace(['"', '\\'], "_"))
}

/// Создать пустую модель `model.pmodel` (`model2.pmodel`…) в каталоге; путь относительно
/// корня ассетов.
pub(crate) fn create_model(root: &Path, folder: &str) -> Result<String, EditorError> {
    let folder = folder.trim_matches('/');
    let dir = if folder.is_empty() { root.to_path_buf() } else { root.join(folder) };
    let (name, path) = (1..)
        .map(|i| if i == 1 { "model".to_string() } else { format!("model{i}", i = i) })
        .map(|name| {
            let path = dir.join(format!("{name}.pmodel", name = name));
            (name, path)
        })
        .find(|(_, path)| !path.exists())
        .expect("a free name exists");
    std::fs::write(&path, EMPTY_MODEL.replace("NAME", &name))
        .map_err(|e| EditorError::Other(trf!("cannot create `{p0}`: {e}", p0 = path.display(), e = e)))?;
    Ok(if folder.is_empty() { format!("{name}.pmodel", name = name) } else { format!("{folder}/{name}.pmodel", folder = folder, name = name) })
}
