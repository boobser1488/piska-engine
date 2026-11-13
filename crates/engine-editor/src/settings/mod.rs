//! Окна настроек: Edit → Preferences… (личные настройки редактора) и Edit → Project
//! Settings… (настройки проекта, файл `project.settings` в корне ассетов). Одно окно,
//! два набора страниц: слева список разделов и поиск по всем настройкам, справа страница
//! из строк (подпись, контрол, описание, стрелка «по умолчанию»), внизу Reset page /
//! Reset all, сверху Import / Export файла настроек.
//!
//! Страницы регистрируются в одном месте — таблице [`PAGES`]. Новая страница:
//!
//! 1. функция `fn my_page(s: &mut SettingsUi, editor: &mut Editor)`, которая описывает
//!    строки через `s.group(…)`, `s.toggle(…)`, `s.number(…)`, `s.combo(…)`, `s.row(…)`,
//!    `s.block(…)` (см. [`rows`]) и меняет значения *только* через их результат;
//! 2. запись `PageDef { scope, id, title, icon, about, draw: my_page }` в [`PAGES`].
//!
//! Настройки проекта правятся в черновике [`SettingsState::project`]: любое изменение сразу
//! применяется к открытому миру и записывается в файл, когда пользователь отпустил мышь
//! (или через полсекунды после последней буквы). Ошибки записи уходят в консоль.

mod build;
mod input;
mod pages_prefs;
mod pages_project;
mod physics;
pub(crate) mod rows;
mod shortcuts_page;
mod tags;
mod tools;

/// Идентификатор страницы Build окна Project Settings.
const BUILD_PAGE: &str = "build";

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Rect, Sense};
use engine_core::settings::{ProjectSettings, PROJECT_SETTINGS_FILE};

use crate::icons::{self, Icon};
use crate::prefs::Preferences;
use crate::state::Editor;
use crate::theme;
use crate::EditorError;
pub(crate) use rows::{Pass, SettingsUi};
pub(crate) use tags::{layer_combo, tag_combo};
pub use build::{
    read_build_options, validate_android_package, write_build_options, AndroidOrientation, BuildOptions,
    ANDROID_MAX_VERSION_CODE, ANDROID_MIN_SDK,
};
pub use input::InputBindingList;

/// Какое из двух окон настроек.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsScope {
    /// Edit → Preferences…: личные настройки редактора (файл настроек редактора).
    Preferences,
    /// Edit → Project Settings…: настройки проекта (`project.settings`).
    Project,
}

impl SettingsScope {
    fn index(self) -> usize {
        match self {
            SettingsScope::Preferences => 0,
            SettingsScope::Project => 1,
        }
    }

    fn title(self) -> &'static str {
        match self {
            SettingsScope::Preferences => "Preferences",
            SettingsScope::Project => "Project Settings",
        }
    }
}

/// Страница окна настроек.
pub(crate) struct PageDef {
    pub scope: SettingsScope,
    /// Постоянное имя страницы (для `Editor::show_settings_page`): `appearance`, `physics2d`.
    pub id: &'static str,
    pub title: &'static str,
    pub icon: Icon,
    /// Одна строка под заголовком страницы.
    pub about: &'static str,
    pub draw: fn(&mut SettingsUi<'_>, &mut Editor),
}

/// Все страницы обоих окон, в порядке списка слева. Новые страницы добавляются сюда.
pub(crate) const PAGES: &[PageDef] = &[
    // --- Preferences ---
    PageDef {
        scope: SettingsScope::Preferences,
        id: "general",
        title: "General",
        icon: Icon::Gear,
        about: "Undo, auto-save and the code editor. Kept for this user.",
        draw: pages_prefs::general,
    },
    PageDef {
        scope: SettingsScope::Preferences,
        id: "appearance",
        title: "Appearance",
        icon: Icon::Eye,
        about: "How the editor looks. Changes apply at once and are kept for this user.",
        draw: pages_prefs::appearance,
    },
    PageDef {
        scope: SettingsScope::Preferences,
        id: "scene-view",
        title: "Scene View",
        icon: Icon::Scene,
        about: "Grid, background, snapping steps and the camera of the Scene view.",
        draw: pages_prefs::scene_view,
    },
    PageDef {
        scope: SettingsScope::Preferences,
        id: "game-view",
        title: "Game View",
        icon: Icon::Camera,
        about: "Frame rate of the editor window in Edit and Play mode.",
        draw: pages_prefs::game_view,
    },
    PageDef {
        scope: SettingsScope::Preferences,
        id: "console",
        title: "Console & Diagnostics",
        icon: Icon::Console,
        about: "What the Console shows when the editor starts, and the Profiler history.",
        draw: pages_prefs::console,
    },
    PageDef {
        scope: SettingsScope::Preferences,
        id: "build-tools",
        title: "Build & Tools",
        icon: Icon::Build,
        about: "Where cargo, the Android SDK, Java, zig and the C++ tools are, and where exports go.",
        draw: tools::build_tools,
    },
    PageDef {
        scope: SettingsScope::Preferences,
        id: "shortcuts",
        title: "Shortcuts",
        icon: Icon::Text,
        about: "Keyboard shortcuts of every editor command. Click a shortcut, then press the new keys.",
        draw: shortcuts_page::shortcuts,
    },
    // --- Project Settings ---
    PageDef {
        scope: SettingsScope::Project,
        id: "player",
        title: "Player",
        icon: Icon::Play,
        about: "The game window, its title and icon, and the first scene. Exported with the game.",
        draw: pages_project::player,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "build",
        title: "Build",
        icon: Icon::Build,
        about: "Export options of the game crate: [package.metadata.piska] in its Cargo.toml, read by piska-export.",
        draw: build::build,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "physics2d",
        title: "Physics 2D",
        icon: Icon::Physics,
        about: "Gravity, solver and which layers collide for 2D bodies.",
        draw: physics::physics2d,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "physics3d",
        title: "Physics 3D",
        icon: Icon::Cube,
        about: "Gravity, solver and which layers collide for 3D bodies.",
        draw: physics::physics3d,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "tags-and-layers",
        title: "Tags and Layers",
        icon: Icon::List,
        about: "Tags and layer names offered by the Inspector. Scenes keep the tag text and the layer number.",
        draw: tags::tags_and_layers,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "input",
        title: "Input",
        icon: Icon::Target,
        about: "Input actions games read with input.button(…) and input.axis(…): keys, mouse and gamepad bindings.",
        draw: input::input,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "time",
        title: "Time",
        icon: Icon::Animation,
        about: "Fixed step of physics and FixedUpdate, the longest frame and the start time scale.",
        draw: pages_project::time,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "graphics",
        title: "Graphics",
        icon: Icon::Environment,
        about: "Quality level and what the renderer draws. Games can switch the level at run time.",
        draw: pages_project::graphics,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "audio",
        title: "Audio",
        icon: Icon::Audio,
        about: "Start volumes of the mixer buses.",
        draw: pages_project::audio,
    },
    PageDef {
        scope: SettingsScope::Project,
        id: "script-order",
        title: "Script Execution Order",
        icon: Icon::Script,
        about: "Which components run first. Changes wait for Apply, then work right away, also in Play.",
        draw: crate::script_order::page,
    },
];

/// Страницы окна `scope` по порядку.
pub(crate) fn pages(scope: SettingsScope) -> impl Iterator<Item = &'static PageDef> {
    PAGES.iter().filter(move |p| p.scope == scope)
}

fn page_def(scope: SettingsScope, id: &str) -> Option<&'static PageDef> {
    pages(scope).find(|p| p.id == id)
}

/// Что ждёт подтверждения.
#[derive(Clone, Debug, PartialEq)]
enum Confirm {
    ResetPage(SettingsScope, &'static str),
    ResetAll(SettingsScope),
    /// Удалить тег, который носят объекты открытой сцены (сколько).
    RemoveTag(String, usize),
    /// Сочетание `binding` для команды `id` уже занято командой `other`.
    ShortcutConflict { id: &'static str, binding: crate::shortcuts::KeyBinding, other: &'static str },
}

/// Окно пути для Import / Export.
struct FileDialog {
    scope: SettingsScope,
    import: bool,
    path: String,
}

/// Состояние окон настроек.
#[derive(Default)]
pub(crate) struct SettingsState {
    /// Открытая страница каждого окна.
    page: [Option<&'static str>; 2],
    search: [String; 2],
    /// Совпадения поиска по страницам в последнем кадре: (страница, строк).
    hits: [Vec<(&'static str, usize)>; 2],
    /// Нарисованные строки последнего кадра: подпись → область.
    rows: [Vec<(String, Rect)>; 2],
    /// Черновик настроек проекта (`None` — ещё не брали из мира).
    pub(crate) project: Option<ProjectSettings>,
    /// Что последним применено к миру.
    applied: Option<ProjectSettings>,
    /// Что последним записано в файл.
    written: Option<ProjectSettings>,
    /// Когда черновик последний раз изменился (время egui) — для отложенной записи.
    changed_at: Option<f64>,
    confirm: Option<Confirm>,
    file: Option<FileDialog>,
    /// Строка состояния внизу окна: текст и «это ошибка».
    status: [Option<(String, bool)>; 2],
    /// Страница Tags and Layers: поле нового тега и правка имени (номер, текст).
    pub(crate) new_tag: String,
    pub(crate) tag_edit: Option<(usize, String)>,
    /// Страница Shortcuts: запись сочетания, последняя команда с клавиатуры.
    pub(crate) keys: crate::shortcuts::ShortcutState,
    /// Время egui первой несохранённой правки (автосохранение, Preferences → General).
    pub(crate) autosave_since: Option<f64>,
    /// Проверка инструментов на странице Build & Tools: (поле, итог, это ошибка).
    pub(crate) tool_checks: Vec<(&'static str, String, bool)>,
    /// Страница Input: выбранное действие, ожидание нажатия, поля имён.
    pub(crate) input: input::InputPageState,
    /// Страница Build: прочитанный Cargo.toml игры, неверные поля, отложенная запись.
    pub(crate) build: build::BuildPageState,
}

// ---------------------------------------------------------------------------
// Идентификаторы (для тестов и автоматизации)
// ---------------------------------------------------------------------------

impl Editor {
    /// Строка настройки `label` в окне `scope`.
    pub fn settings_row_id(scope: SettingsScope, label: &str) -> egui::Id {
        rows::row_id(scope, label)
    }

    /// Стрелка «вернуть по умолчанию» строки `label` (видна, только если значение изменено).
    pub fn settings_reset_id(scope: SettingsScope, label: &str) -> egui::Id {
        rows::reset_id(scope, label)
    }

    /// Раздел `page` в списке слева.
    pub fn settings_page_id(scope: SettingsScope, page: &str) -> egui::Id {
        egui::Id::new(("settings_page", scope, page.to_string()))
    }

    /// Поле поиска окна.
    pub fn settings_search_id(scope: SettingsScope) -> egui::Id {
        egui::Id::new(("settings_search", scope))
    }

    pub fn settings_reset_page_id(scope: SettingsScope) -> egui::Id {
        egui::Id::new(("settings_reset_page", scope))
    }

    pub fn settings_reset_all_id(scope: SettingsScope) -> egui::Id {
        egui::Id::new(("settings_reset_all", scope))
    }

    pub fn settings_import_id(scope: SettingsScope) -> egui::Id {
        egui::Id::new(("settings_import", scope))
    }

    pub fn settings_export_id(scope: SettingsScope) -> egui::Id {
        egui::Id::new(("settings_export", scope))
    }

    /// Кнопка закрытия окна.
    pub fn settings_close_id(scope: SettingsScope) -> egui::Id {
        egui::Id::new(("settings_close", scope))
    }

    /// Кнопка подтверждения (Reset, Remove, Import, Export) в открытом диалоге окна настроек.
    pub fn settings_confirm_id() -> egui::Id {
        egui::Id::new("settings_confirm")
    }

    /// Кнопка Cancel в диалоге окна настроек.
    pub fn settings_cancel_id() -> egui::Id {
        egui::Id::new("settings_cancel")
    }
}

// ---------------------------------------------------------------------------
// Открыть, выбрать страницу, искать
// ---------------------------------------------------------------------------

impl Editor {
    fn settings_open(&self, scope: SettingsScope) -> bool {
        match scope {
            SettingsScope::Preferences => self.preferences_open,
            SettingsScope::Project => self.script_order.open,
        }
    }

    fn set_settings_open(&mut self, scope: SettingsScope, open: bool) {
        match scope {
            SettingsScope::Preferences => self.preferences_open = open,
            SettingsScope::Project => self.script_order.open = open,
        }
    }

    /// Открыть окно настроек на странице `page` (`appearance`, `player`, `tags-and-layers`…).
    /// Незнакомая страница — окно открывается на текущей.
    pub fn show_settings_page(&mut self, scope: SettingsScope, page: &str) {
        if let Some(def) = page_def(scope, page) {
            self.settings.page[scope.index()] = Some(def.id);
        }
        self.set_settings_open(scope, true);
    }

    /// Открыть Project Settings на странице `page` (как [`Editor::show_settings_page`]).
    pub fn show_project_settings_page(&mut self, page: &str) {
        self.show_settings_page(SettingsScope::Project, page);
    }

    /// Открытая страница окна `scope`.
    pub fn settings_page(&self, scope: SettingsScope) -> &'static str {
        self.settings.page[scope.index()].unwrap_or_else(|| pages(scope).next().map_or("", |p| p.id))
    }

    /// Страницы окна `scope`: (имя, заголовок).
    pub fn settings_pages(scope: SettingsScope) -> Vec<(&'static str, &'static str)> {
        pages(scope).map(|p| (p.id, p.title)).collect()
    }

    /// Текст поиска окна (как ввод в поле поиска).
    pub fn set_settings_search(&mut self, scope: SettingsScope, text: &str) {
        self.settings.search[scope.index()] = text.to_string();
    }

    /// Страницы, на которых поиск нашёл строки в последнем кадре: (имя, сколько строк).
    pub fn settings_search_hits(&self, scope: SettingsScope) -> Vec<(&'static str, usize)> {
        self.settings.hits[scope.index()].clone()
    }

    /// Где нарисована строка `label` в последнем кадре.
    pub fn settings_row_rect(&self, scope: SettingsScope, label: &str) -> Option<Rect> {
        self.settings.rows[scope.index()].iter().find(|(l, _)| l == label).map(|(_, r)| *r)
    }

    /// Подписи строк, нарисованных в последнем кадре.
    pub fn settings_visible_rows(&self, scope: SettingsScope) -> Vec<String> {
        self.settings.rows[scope.index()].iter().map(|(l, _)| l.clone()).collect()
    }

    /// Текст строки состояния окна (последнее сохранение или ошибка).
    pub fn settings_status(&self, scope: SettingsScope) -> Option<String> {
        self.settings.status[scope.index()].as_ref().map(|(text, _)| text.clone())
    }

    /// Строка состояния окна Preferences (итог кнопок Test / Validate).
    pub(crate) fn set_preferences_status(&mut self, text: String, error: bool) {
        self.settings.status[SettingsScope::Preferences.index()] = Some((text, error));
    }

    /// Открыт диалог окна настроек (подтверждение или путь файла): горячие клавиши ждут.
    pub(crate) fn settings_modal_open(&self) -> bool {
        self.settings.confirm.is_some() || self.settings.file.is_some()
    }

    /// Назначить команде `id` сочетание, как страница Shortcuts после нажатия: если оно
    /// занято другой командой, открывается вопрос «заменить?» (Replace / Cancel).
    /// `true` — назначено сразу.
    pub fn assign_shortcut(&mut self, id: &str, binding: crate::shortcuts::KeyBinding) -> bool {
        let Some(def) = crate::shortcuts::command(id) else { return false };
        if self.prefs.shortcuts.binding(def.id) == Some(binding) {
            return true;
        }
        if let Some(other) = self.prefs.shortcuts.conflict(def.id, binding) {
            self.settings.confirm = Some(Confirm::ShortcutConflict { id: def.id, binding, other: other.id });
            return false;
        }
        self.prefs.shortcuts.set(def.id, Some(binding));
        self.settings.status[SettingsScope::Preferences.index()] = Some((format!("{} now runs “{}”", binding.text(), def.title), false));
        true
    }
}

// ---------------------------------------------------------------------------
// Черновик настроек проекта: применить и записать
// ---------------------------------------------------------------------------

impl Editor {
    /// Взять черновик из мира, если его ещё нет.
    pub(crate) fn ensure_project_draft(&mut self) {
        if self.settings.project.is_none() {
            let current = self.app.world.project_settings();
            self.settings.applied = Some(current.clone());
            self.settings.written = Some(current.clone());
            self.settings.project = Some(current);
        }
    }

    /// Черновик настроек проекта (для правки страницами).
    pub(crate) fn project_draft_mut(&mut self) -> &mut ProjectSettings {
        self.ensure_project_draft();
        self.settings.project.get_or_insert_with(ProjectSettings::default)
    }

    /// Настройки проекта, как их показывает окно Project Settings (то же, что действует в
    /// мире; порядок скриптов — без неприменённых правок страницы Script Execution Order).
    pub fn project_settings_draft(&self) -> ProjectSettings {
        self.settings.project.clone().unwrap_or_else(|| self.app.world.project_settings())
    }

    /// Изменить настройки проекта, как это делает окно: сразу применить к миру и записать
    /// `project.settings`.
    pub fn edit_project_settings(&mut self, edit: impl FnOnce(&mut ProjectSettings)) -> Result<(), EditorError> {
        edit(self.project_draft_mut());
        self.commit_project_settings(true)
    }

    /// Применить черновик к миру, если он изменился, и (при `write`) записать файл.
    pub(crate) fn commit_project_settings(&mut self, write: bool) -> Result<(), EditorError> {
        let Some(draft) = self.settings.project.clone() else {
            return Ok(());
        };
        if self.settings.applied.as_ref() != Some(&draft) {
            self.apply_project_settings_live(draft.clone());
            self.settings.applied = Some(draft.clone());
        }
        if !write || self.settings.written.as_ref() == Some(&draft) {
            return Ok(());
        }
        self.settings.changed_at = None;
        // Пробовали — больше не пишем, пока черновик не изменится снова.
        self.settings.written = Some(draft.clone());
        let result = self
            .assets_root()
            .ok_or_else(|| {
                EditorError::Other(
                    "cannot save project settings: no assets directory is mounted. \
                     Call `engine::mount_assets(&mut app, \"assets\")` before starting the editor"
                        .to_string(),
                )
            })
            .and_then(|root| draft.save(root.join(PROJECT_SETTINGS_FILE)).map_err(|e| EditorError::Other(e.to_string())));
        let status = &mut self.settings.status[SettingsScope::Project.index()];
        match &result {
            Ok(()) => *status = Some((format!("Saved to {PROJECT_SETTINGS_FILE}"), false)),
            Err(error) => {
                log::error!("{error}");
                *status = Some((error.to_string(), true));
            }
        }
        result
    }

    /// Единственное место, где редактор применяет настройки проекта к открытому миру.
    ///
    /// `World::apply_project_settings` применяет то, что принадлежит ядру (порядок скриптов,
    /// время, ввод, ресурсы `ProjectSettings` и `GraphicsSettings`), и вызывает хуки
    /// подсистем (`World::add_settings_hook`: физика 2D / 3D, звук), которые
    /// `engine::app()` регистрирует для собранных в игру частей движка. Редактору не нужно
    /// знать о подсистемах: новая подсистема регистрирует свой хук, а не правит этот код.
    fn apply_project_settings_live(&mut self, settings: ProjectSettings) {
        self.app.world.apply_project_settings(settings);
    }
}

// ---------------------------------------------------------------------------
// Сброс, импорт, экспорт
// ---------------------------------------------------------------------------

impl Editor {
    /// Вернуть к умолчанию страницу `page` окна `scope` (или все страницы при `None`) — без
    /// вопроса (окно спрашивает подтверждение само). Настройки проекта сразу записываются.
    pub fn reset_settings(&mut self, scope: SettingsScope, page: Option<&str>) -> Result<(), EditorError> {
        let ctx = egui::Context::default();
        let mut result = Ok(());
        // Проход Reset не рисует, но строкам нужен `Ui`: пустой кадр без экрана.
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                result = self.reset_pages(ui, scope, page);
            });
        });
        result
    }

    fn reset_pages(&mut self, ui: &mut egui::Ui, scope: SettingsScope, page: Option<&str>) -> Result<(), EditorError> {
        if scope == SettingsScope::Project {
            self.ensure_project_draft();
        }
        // Страница Build правит `Cargo.toml` игры, а не файл настроек: «Reset all» её не
        // трогает, она сбрасывается только своей кнопкой «Reset page» (с отдельным
        // предупреждением).
        let skipped = |id: &str| page.is_none() && scope == SettingsScope::Project && id == BUILD_PAGE;
        for def in pages(scope).filter(|p| page.is_none_or(|id| id == p.id) && !skipped(p.id)) {
            let mut s = SettingsUi::new(ui, Pass::Reset, scope, "", def.title);
            (def.draw)(&mut s, self);
        }
        let what = page.and_then(|id| page_def(scope, id)).map_or("all settings".to_string(), |p| format!("“{}”", p.title));
        self.settings.status[scope.index()] = Some((format!("Reset {what} to defaults"), false));
        if scope == SettingsScope::Project {
            self.commit_project_settings(true)?;
        }
        Ok(())
    }

    /// Каталог проекта: родитель каталога ассетов (или текущий каталог).
    fn settings_base_dir(&self) -> PathBuf {
        self.assets_root()
            .and_then(|root| root.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default()
    }

    fn resolve_settings_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path.trim());
        if path.is_relative() {
            self.settings_base_dir().join(path)
        } else {
            path.to_path_buf()
        }
    }

    /// Записать настройки окна `scope` в файл (`path` относительно каталога проекта).
    pub fn export_settings_file(&mut self, scope: SettingsScope, path: &str) -> Result<PathBuf, EditorError> {
        let file = self.resolve_settings_path(path);
        let text = match scope {
            SettingsScope::Preferences => self.prefs.to_text(),
            SettingsScope::Project => self.project_settings_draft().to_ron(),
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&file, text)
            .map_err(|e| EditorError::Other(format!("cannot write `{}`: {e}. Choose another folder", file.display())))?;
        self.settings.status[scope.index()] = Some((format!("Exported to {}", file.display()), false));
        Ok(file)
    }

    /// Прочитать настройки окна `scope` из файла и сразу применить (проект — и записать
    /// в `project.settings`). Раскладка окон редактора при импорте Preferences сохраняется.
    pub fn import_settings_file(&mut self, scope: SettingsScope, path: &str) -> Result<(), EditorError> {
        let file = self.resolve_settings_path(path);
        let text = std::fs::read_to_string(&file)
            .map_err(|e| EditorError::Other(format!("cannot read `{}`: {e}. Check the path", file.display())))?;
        match scope {
            SettingsScope::Preferences => {
                if !text.contains(':') {
                    return Err(EditorError::Other(format!(
                        "`{}` is not an editor preferences file: export one with Preferences → Export…",
                        file.display()
                    )));
                }
                let mut prefs = Preferences::parse(&text);
                prefs.layout = self.prefs.layout.clone();
                prefs.saved_layouts = self.prefs.saved_layouts.clone();
                self.prefs = prefs;
            }
            SettingsScope::Project => {
                let settings = ProjectSettings::from_ron(&text, &file.display().to_string())
                    .map_err(|e| EditorError::Other(e.to_string()))?;
                *self.project_draft_mut() = settings.clone();
                self.script_order.draft = settings;
                self.commit_project_settings(true)?;
            }
        }
        self.settings.status[scope.index()] = Some((format!("Imported {}", file.display()), false));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Окно
// ---------------------------------------------------------------------------

impl Editor {
    /// Окно Preferences (вызывается из `Editor::ui`).
    pub(crate) fn preferences_window(&mut self, ctx: &egui::Context) {
        self.settings_window(ctx, SettingsScope::Preferences);
    }

    /// Окно Project Settings (вызывается из `Editor::ui`).
    pub(crate) fn project_settings_window(&mut self, ctx: &egui::Context) {
        self.settings_window(ctx, SettingsScope::Project);
        // Отложенная запись: мышь отпущена и ничего не печатают — или полсекунды тишины.
        if self.settings.project.is_some() {
            let now = ctx.input(|i| i.time);
            let draft_changed = self.settings.written != self.settings.project;
            if draft_changed && self.settings.changed_at.is_none() {
                self.settings.changed_at = Some(now);
            }
            let quiet = !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input();
            let waited = self.settings.changed_at.is_some_and(|t| now - t > 0.5);
            let _ = self.commit_project_settings(quiet || waited);
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context, scope: SettingsScope) {
        if !self.settings_open(scope) {
            return;
        }
        if scope == SettingsScope::Project {
            self.ensure_project_draft();
            self.ensure_script_order_loaded();
        }
        let id = match scope {
            SettingsScope::Preferences => "editor_preferences",
            SettingsScope::Project => "editor_project_settings",
        };
        let screen = ctx.screen_rect();
        let size = vec2(900.0_f32.min(screen.width() - 40.0), 620.0_f32.min(screen.height() - 60.0));
        egui::Window::new(scope.title())
            .id(egui::Id::new(id))
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_size(size)
            .min_size(vec2(620.0, 380.0))
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .frame(theme::window_frame(&ctx.style()).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| self.settings_body(ui, scope));
        self.settings_dialogs(ctx, scope);
    }

    fn settings_body(&mut self, ui: &mut egui::Ui, scope: SettingsScope) {
        let ix = scope.index();
        let defs: Vec<&'static PageDef> = pages(scope).collect();
        let query = self.settings.search[ix].trim().to_lowercase();

        // Поиск по всем страницам: проход Probe ничего не рисует.
        let mut hits: Vec<(&'static str, usize)> = Vec::new();
        if !query.is_empty() {
            for def in &defs {
                let mut probe = SettingsUi::new(ui, Pass::Probe, scope, &query, def.title);
                (def.draw)(&mut probe, self);
                if probe.hits > 0 {
                    hits.push((def.id, probe.hits));
                }
            }
        }
        let mut current = self.settings_page(scope);
        if !query.is_empty() && !hits.iter().any(|(id, _)| *id == current) {
            if let Some((first, _)) = hits.first() {
                current = first;
            }
        }
        self.settings.page[ix] = Some(current);
        self.settings.hits[ix] = hits.clone();

        let body = ui.available_rect_before_wrap();
        let body = Rect::from_min_size(body.min, ui.available_size());
        ui.allocate_rect(body, Sense::hover());

        // Полоса сверху: поиск, импорт, экспорт.
        let top = Rect::from_min_size(body.min, vec2(body.width(), theme::STRIP + theme::SP_S * 2.0));
        let bottom = Rect::from_min_max(pos2(body.left(), body.bottom() - (theme::STRIP + theme::SP_M)), body.max);
        let side = Rect::from_min_max(pos2(body.left(), top.bottom()), pos2(body.left() + 196.0, bottom.top()));
        let page_rect = Rect::from_min_max(pos2(side.right(), top.bottom()), pos2(body.right(), bottom.top()));
        let painter = ui.painter().clone();
        painter.rect_filled(top, egui::Rounding { nw: theme::R_L, ne: theme::R_L, sw: 0.0, se: 0.0 }, theme::BG_HEADER);
        painter.rect_filled(side, 0.0, theme::BG_HEADER);
        painter.rect_filled(bottom, egui::Rounding { nw: 0.0, ne: 0.0, sw: theme::R_L, se: theme::R_L }, theme::BG_HEADER);
        let line = egui::Stroke::new(1.0_f32, theme::LINE);
        painter.hline(top.x_range(), top.bottom() - 0.5, line);
        painter.hline(bottom.x_range(), bottom.top() + 0.5, line);
        painter.vline(side.right() - 0.5, side.y_range(), line);

        // --- Поиск, Import, Export ---
        let mut top_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(top.shrink2(vec2(theme::SP_M + 2.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        top_ui.spacing_mut().item_spacing.x = theme::SP_S;
        // Заголовок окна слева (иконка и имя), как у диалогов.
        let (icon_rect, _) = top_ui.allocate_exact_size(vec2(theme::ICON, theme::ICON), Sense::hover());
        icons::paint(top_ui.painter(), icon_rect, Icon::Gear, theme::TEXT_MUTED);
        top_ui.add_space(theme::SP_XS);
        top_ui.label(egui::RichText::new(scope.title()).font(theme::strong(theme::FONT_HEADING)).color(theme::TEXT_STRONG));
        let title_end = top_ui.min_rect().right();
        top_ui.add_space((side.right() + theme::SP_XL - theme::SP_M - title_end).max(theme::SP_L));
        let hint = match scope {
            SettingsScope::Preferences => "Search preferences",
            SettingsScope::Project => "Search project settings",
        };
        let search = theme::search_field(&mut top_ui, &mut self.settings.search[ix], hint, 300.0);
        let _ = top_ui.interact(search.rect, Editor::settings_search_id(scope), Sense::hover());
        top_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui.allocate_exact_size(vec2(theme::CONTROL_H, theme::CONTROL_H), Sense::hover()).0;
            let close = ui.interact(close, Editor::settings_close_id(scope), Sense::click());
            if close.hovered() {
                ui.painter().rect_filled(close.rect, theme::R_S, theme::BG_HOVER);
            }
            icons::paint(
                ui.painter(),
                Rect::from_center_size(close.rect.center(), vec2(theme::ICON_S, theme::ICON_S)),
                Icon::Close,
                if close.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED },
            );
            if close.on_hover_text("Close").clicked() {
                self.set_settings_open(scope, false);
            }
            theme::toolbar_separator(ui);
            let export = theme::icon_text_button(ui, Icon::Save, "Export…");
            let export = ui
                .interact(export.rect, Editor::settings_export_id(scope), Sense::click())
                .on_hover_text("Save these settings to a file");
            if export.clicked() {
                self.settings.file = Some(FileDialog { scope, import: false, path: default_file(scope).to_string() });
            }
            let import = theme::icon_text_button(ui, Icon::FolderOpen, "Import…");
            let import = ui
                .interact(import.rect, Editor::settings_import_id(scope), Sense::click())
                .on_hover_text("Replace these settings with the ones from a file");
            if import.clicked() {
                self.settings.file = Some(FileDialog { scope, import: true, path: default_file(scope).to_string() });
            }
        });

        // --- Разделы слева ---
        let mut side_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(side.shrink2(vec2(0.0, theme::SP_S)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        side_ui.spacing_mut().item_spacing.y = 0.0;
        for def in &defs {
            let count = hits.iter().find(|(id, _)| *id == def.id).map(|(_, n)| *n);
            if !query.is_empty() && count.is_none() {
                continue;
            }
            let (rect, _) = side_ui.allocate_exact_size(vec2(side.width() - 1.0, theme::ROW + 2.0), Sense::hover());
            let response = side_ui.interact(rect, Editor::settings_page_id(scope, def.id), Sense::click());
            let selected = def.id == current;
            theme::list_row(side_ui.painter(), rect, selected, true, response.hovered());
            let icon_rect = Rect::from_center_size(pos2(rect.left() + theme::SP_L + theme::ICON * 0.5, rect.center().y), vec2(theme::ICON, theme::ICON));
            icons::paint(side_ui.painter(), icon_rect, def.icon, if selected { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
            let text_color = if selected { theme::TEXT_STRONG } else { theme::TEXT };
            let right = rect.right() - theme::SP_M - if count.is_some() { 26.0 } else { 0.0 };
            let galley = side_ui.painter().layout(
                def.title.to_string(),
                theme::body(),
                text_color,
                f32::INFINITY,
            );
            side_ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(right, rect.bottom()))).galley(
                pos2(icon_rect.right() + theme::SP_M, rect.center().y - galley.size().y * 0.5),
                galley,
                text_color,
            );
            if let Some(count) = count {
                let badge = side_ui.painter().layout_no_wrap(count.to_string(), theme::caption(), theme::TEXT_STRONG);
                let pill = Rect::from_center_size(pos2(rect.right() - theme::SP_M - 10.0, rect.center().y), vec2((badge.size().x + 8.0).max(18.0), 15.0));
                side_ui.painter().rect_filled(pill, 7.5, if selected { theme::BG_ACTIVE } else { theme::accent_soft() });
                side_ui.painter().galley(pill.center() - badge.size() * 0.5, badge, theme::TEXT_STRONG);
            }
            if response.clicked() {
                self.settings.page[ix] = Some(def.id);
                current = def.id;
            }
        }

        // --- Страница ---
        let mut page_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(page_rect.shrink2(vec2(theme::SP_XL, theme::SP_L)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let mut drawn = Vec::new();
        if !query.is_empty() && hits.is_empty() {
            page_ui.add_space(theme::SP_XXL * 2.0);
            theme::empty_state(&mut page_ui, Icon::Search, &format!("No settings match “{}”", self.settings.search[ix].trim()), "Try another word, or clear the search.");
        } else if let Some(def) = page_def(scope, current) {
            page_ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(theme::ICON, theme::ICON), Sense::hover());
                icons::paint(ui.painter(), rect, def.icon, theme::TEXT_MUTED);
                ui.add_space(theme::SP_XS);
                ui.label(egui::RichText::new(def.title).font(theme::strong(theme::FONT_HEADING)).color(theme::TEXT_STRONG));
            });
            page_ui.add_space(theme::SP_XS);
            page_ui.label(egui::RichText::new(def.about).font(theme::caption()).color(theme::TEXT_MUTED));
            page_ui.add_space(theme::SP_M);
            egui::ScrollArea::vertical()
                .id_salt(("settings_scroll", scope, def.id))
                .auto_shrink([false, false])
                .show(&mut page_ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.set_width(ui.available_width() - theme::SP_M);
                    let mut s = SettingsUi::new(ui, Pass::Draw, scope, &query, def.title);
                    (def.draw)(&mut s, self);
                    drawn = std::mem::take(&mut s.rows);
                    ui.add_space(theme::SP_L);
                });
        }
        self.settings.rows[ix] = drawn;

        // --- Сброс и состояние ---
        let mut bottom_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bottom.shrink2(vec2(theme::SP_L, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        bottom_ui.spacing_mut().item_spacing.x = theme::SP_S;
        let reset_page = theme::secondary_button(&mut bottom_ui, "Reset page");
        let reset_page = bottom_ui
            .interact(reset_page.rect, Editor::settings_reset_page_id(scope), Sense::click())
            .on_hover_text("Set every setting on this page back to its default");
        if reset_page.clicked() {
            self.settings.confirm = Some(Confirm::ResetPage(scope, current));
        }
        let reset_all = theme::secondary_button(&mut bottom_ui, "Reset all");
        let reset_all = bottom_ui
            .interact(reset_all.rect, Editor::settings_reset_all_id(scope), Sense::click())
            .on_hover_text("Set every setting on every page back to its default");
        if reset_all.clicked() {
            self.settings.confirm = Some(Confirm::ResetAll(scope));
        }
        let (status, error) = match &self.settings.status[ix] {
            Some((text, error)) => (text.clone(), *error),
            None => (
                match scope {
                    SettingsScope::Preferences => "Changes apply at once and are kept for this user".to_string(),
                    SettingsScope::Project => format!("Changes apply at once and are saved to Assets/{PROJECT_SETTINGS_FILE}"),
                },
                false,
            ),
        };
        bottom_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(status).font(theme::caption()).color(if error { theme::ERROR } else { theme::TEXT_MUTED }))
                    .truncate(),
            );
        });
    }

    /// Подтверждения и окно пути.
    fn settings_dialogs(&mut self, ctx: &egui::Context, scope: SettingsScope) {
        let confirm = self.settings.confirm.clone().filter(|c| match c {
            Confirm::ResetPage(s, _) | Confirm::ResetAll(s) => *s == scope,
            Confirm::RemoveTag(..) => scope == SettingsScope::Project,
            Confirm::ShortcutConflict { .. } => scope == SettingsScope::Preferences,
        });
        if let Some(confirm) = confirm {
            let (title, text, button) = match &confirm {
                Confirm::ResetPage(_, page) => (
                    format!("Reset {}?", page_def(scope, page).map_or("this page", |p| p.title)),
                    if scope == SettingsScope::Project && *page == BUILD_PAGE {
                        "Every export option on this page is removed from [package.metadata.piska] in the game's Cargo.toml, so the export uses its defaults again.".to_string()
                    } else {
                        "Every setting on this page goes back to its default value.".to_string()
                    },
                    "Reset",
                ),
                Confirm::ResetAll(_) => (
                    format!("Reset all {}?", match scope {
                        SettingsScope::Preferences => "preferences",
                        SettingsScope::Project => "project settings",
                    }),
                    match scope {
                        SettingsScope::Preferences => "Every preference on every page goes back to its default value. Window layouts are kept.".to_string(),
                        SettingsScope::Project => format!("Every project setting goes back to its default value and Assets/{PROJECT_SETTINGS_FILE} is rewritten. The export options on the Build page (the game's Cargo.toml) are kept."),
                    },
                    "Reset All",
                ),
                Confirm::RemoveTag(tag, used) => (
                    format!("Remove tag “{tag}”?"),
                    format!(
                        "{used} object(s) in the open scene use it. They keep the tag text; the Inspector shows it as missing until the tag is added again."
                    ),
                    "Remove",
                ),
                Confirm::ShortcutConflict { id, binding, other } => {
                    let title_of = |id: &'static str| crate::shortcuts::command(id).map_or(id, |c| c.title);
                    (
                        format!("{} is already used", binding.text()),
                        format!(
                            "It runs “{}”. Replace: “{}” gets {} and “{}” is left without a shortcut.",
                            title_of(other),
                            title_of(id),
                            binding.text(),
                            title_of(other)
                        ),
                        "Replace",
                    )
                }
            };
            let mut outcome = None;
            let modal = egui::Modal::new(egui::Id::new("settings_confirm_modal")).frame(theme::dialog_frame(&ctx.style())).show(ctx, |ui| {
                ui.set_width(380.0);
                theme::dialog_title(ui, Icon::Warn, theme::WARN, &title);
                ui.label(egui::RichText::new(text).color(theme::TEXT_MUTED));
                ui.add_space(theme::SP_L);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let yes = theme::danger_button(ui, button);
                        if ui.interact(yes.rect, Editor::settings_confirm_id(), Sense::click()).clicked() {
                            outcome = Some(true);
                        }
                        let no = theme::secondary_button(ui, "Cancel");
                        if ui.interact(no.rect, Editor::settings_cancel_id(), Sense::click()).clicked() {
                            outcome = Some(false);
                        }
                    });
                });
            });
            if modal.should_close() && outcome.is_none() {
                outcome = Some(false);
            }
            if let Some(yes) = outcome {
                self.settings.confirm = None;
                if yes {
                    let result = match confirm {
                        Confirm::ResetPage(scope, page) => self.reset_settings(scope, Some(page)),
                        Confirm::ResetAll(scope) => self.reset_settings(scope, None),
                        Confirm::RemoveTag(tag, _) => {
                            tags::remove_tag(self, &tag);
                            Ok(())
                        }
                        Confirm::ShortcutConflict { id, binding, other } => {
                            self.prefs.shortcuts.set(other, None);
                            self.prefs.shortcuts.set(id, Some(binding));
                            self.settings.status[scope.index()] =
                                Some((format!("{} now runs “{}”", binding.text(), crate::shortcuts::command(id).map_or(id, |c| c.title)), false));
                            Ok(())
                        }
                    };
                    if let Err(error) = result {
                        self.settings.status[scope.index()] = Some((error.to_string(), true));
                    }
                }
            }
        }

        let base = self.settings_base_dir();
        let Some(dialog) = self.settings.file.as_mut().filter(|d| d.scope == scope) else {
            return;
        };
        let import = dialog.import;
        let what = match scope {
            SettingsScope::Preferences => "Preferences",
            SettingsScope::Project => "Project Settings",
        };
        let mut outcome = None;
        let modal = egui::Modal::new(egui::Id::new("settings_file_modal")).frame(theme::dialog_frame(&ctx.style())).show(ctx, |ui| {
            ui.set_width(460.0);
            theme::dialog_title(ui, if import { Icon::FolderOpen } else { Icon::Save }, theme::accent(), &format!("{} {what}", if import { "Import" } else { "Export" }));
            ui.label(
                egui::RichText::new(if import {
                    "Read the settings from this file and apply them right away."
                } else {
                    "Write the current settings to this file."
                })
                .color(theme::TEXT_MUTED),
            );
            ui.add_space(theme::SP_M);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut dialog.path)
                    .font(theme::mono())
                    .desired_width(f32::INFINITY)
                    .vertical_align(egui::Align::Center)
                    .min_size(vec2(0.0, theme::FIELD_H + 4.0)),
            );
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(theme::SP_XS);
            ui.label(
                egui::RichText::new(format!("Relative paths start in {}", base.display()))
                    .font(theme::caption())
                    .color(theme::TEXT_DISABLED),
            );
            ui.add_space(theme::SP_L);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let yes = theme::primary_button(ui, if import { "Import" } else { "Export" });
                if ui.interact(yes.rect, Editor::settings_confirm_id(), Sense::click()).clicked() || enter {
                    outcome = Some(true);
                }
                let no = theme::secondary_button(ui, "Cancel");
                if ui.interact(no.rect, Editor::settings_cancel_id(), Sense::click()).clicked() {
                    outcome = Some(false);
                }
            });
        });
        if modal.should_close() && outcome.is_none() {
            outcome = Some(false);
        }
        if let Some(yes) = outcome {
            let path = self.settings.file.take().map(|d| d.path).unwrap_or_default();
            if yes && !path.trim().is_empty() {
                let result = if import { self.import_settings_file(scope, &path) } else { self.export_settings_file(scope, &path).map(|_| ()) };
                if let Err(error) = result {
                    log::error!("{error}");
                    self.settings.status[scope.index()] = Some((error.to_string(), true));
                }
            }
        }
    }
}

/// Имя файла по умолчанию для Import / Export.
fn default_file(scope: SettingsScope) -> &'static str {
    match scope {
        SettingsScope::Preferences => "editor-preferences.ron",
        SettingsScope::Project => "project-settings.ron",
    }
}
