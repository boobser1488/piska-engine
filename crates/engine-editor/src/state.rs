//! Состояние редактора и действия над ним.
//!
//! Панели ничего не меняют «по-крупному» сами: они кладут [`Action`] в очередь,
//! а [`Editor::apply`] выполняет его. Поэтому всё, что можно сделать мышью,
//! можно сделать и из теста: `editor.apply(Action::Play)`.

use crate::i18n::{tr_s, trf};
use std::path::PathBuf;

use engine_assets::{AssetDatabase, AssetHandle, Guid};
use engine_core::prelude::*;
use engine_render::{
    Camera, CameraView, Light, LightKind, MeshRenderer, MeshShape, Projection, RenderStats, Renderer,
    SpriteRenderer,
};

use crate::catalog;
use crate::code::{BuildCommand, BuildStatus, CodeState};
use crate::console::ConsoleState;
use crate::hierarchy::HierarchyState;
use crate::inspector::InspectorState;
use crate::profiler::{FrameTimings, ProfilerState};
use crate::project::ProjectState;
use crate::view3d::Orbit3D;
use crate::viewport::ViewportState;
use crate::prefs::Preferences;
use crate::EditorError;

/// Настройки запуска редактора.
#[derive(Clone, Debug)]
pub struct EditorConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    /// Сцена, которую открыть при старте: путь относительно каталога ассетов.
    pub scene: Option<String>,
    /// Закрыть окно после стольких кадров (для смоук-тестов).
    pub max_frames: Option<u64>,
    /// Каталоги с исходниками игры (Rust, C++): их файлы видны в окне Project и
    /// открываются во вкладке Code.
    pub code_dirs: Vec<PathBuf>,
    /// Что запускает кнопка Build (Ctrl+B), по порядку.
    pub build_commands: Vec<BuildCommand>,
    /// Куда редактор кладёт новые скрипты на C++ (Add Component → New C++ Script).
    pub script_dir: Option<PathBuf>,
    /// Масштаб интерфейса: 1.0 — как есть, 1.25 — на четверть крупнее. `None` —
    /// взять запомненный с прошлого запуска (меню View → UI scale).
    pub ui_scale: Option<f32>,
    /// Крейт игры в `games/<name>/`, который экспортирует окно File → Build….
    pub game_crate: Option<String>,
    /// Корень рабочего пространства движка (по умолчанию — текущий каталог).
    pub workspace: Option<PathBuf>,
    /// Каталог скриптов на Rust (New Rust Script): в нём редактор создаёт файлы и
    /// пишет `mod.rs` с `register(app)`.
    pub rust_script_dir: Option<PathBuf>,
    /// Как пересобрать программу редактора и перезапуститься с новым кодом (Build).
    pub relaunch: Option<crate::rust_scripts::Relaunch>,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            title: tr_s("Piska Editor").to_string(),
            width: 1440,
            height: 860,
            scene: None,
            max_frames: None,
            code_dirs: Vec::new(),
            build_commands: Vec::new(),
            script_dir: None,
            ui_scale: None,
            game_crate: None,
            workspace: None,
            rust_script_dir: None,
            relaunch: None,
        }
    }
}

impl EditorConfig {
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Открыть сцену при старте: `EditorConfig::default().open_scene("scenes/level1.scene")`.
    pub fn open_scene(mut self, relative_path: impl Into<String>) -> Self {
        self.scene = Some(relative_path.into());
        self
    }

    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Показать в редакторе каталог с исходниками игры.
    pub fn code_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.code_dirs.push(dir.into());
        self
    }

    pub fn ui_scale(mut self, scale: f32) -> Self {
        self.ui_scale = Some(scale);
        self
    }

    /// Каталог скриптов C++-плагина: в нём редактор создаёт новые скрипты.
    pub fn cpp_scripts(mut self, dir: impl Into<PathBuf>) -> Self {
        self.script_dir = Some(dir.into());
        self
    }

    /// Игра для экспорта (File → Build…): крейт `games/<name>/` рабочего пространства.
    pub fn game_crate(mut self, name: impl Into<String>) -> Self {
        self.game_crate = Some(name.into());
        self
    }

    /// Корень рабочего пространства движка, если редактор запущен не из него.
    pub fn workspace(mut self, dir: impl Into<PathBuf>) -> Self {
        self.workspace = Some(dir.into());
        self
    }

    /// Добавить команду сборки:
    /// `.build_command(BuildCommand::new("C++ plugin", "cmake").args(["--build", "build"]))`.
    pub fn build_command(mut self, command: BuildCommand) -> Self {
        self.build_commands.push(command);
        self
    }

    /// Каталог скриптов на Rust: New Rust Script создаёт в нём `<snake_case>.rs` и
    /// переписывает `mod.rs` (его `register(app)` игра вызывает один раз). Каталог
    /// стоит показать и во вкладке Code — добавьте его или родителя в `code_dir`.
    pub fn rust_scripts(mut self, dir: impl Into<PathBuf>) -> Self {
        self.rust_script_dir = Some(dir.into());
        self
    }

    /// Пересборка с перезапуском: Build (Ctrl+B) запускает команды сборки, затем
    /// `relaunch.build` (cargo), и при успехе редактор перезапускается новой версией
    /// программы, вернув сцену, выделение, открытые файлы и камеру.
    pub fn relaunch(mut self, relaunch: crate::rust_scripts::Relaunch) -> Self {
        self.relaunch = Some(relaunch);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Сцена редактируется, игра не идёт.
    Edit,
    Playing,
    Paused,
}

/// Инструмент вьюпорта. Клавиши как в Unity: W / E / R.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Move,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewTab {
    /// Свободная камера редактора, гизмо и сетка.
    Scene,
    /// То, что видит камера сцены — как в собранной игре.
    Game,
    /// Редактор кода.
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomTab {
    Project,
    Console,
    Profiler,
}

/// Всё, что редактор умеет делать со сценой и с собой.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Select(Option<GameObjectId>),
    CreateEmpty { parent: Option<GameObjectId> },
    CreateSprite { parent: Option<GameObjectId> },
    CreateCamera,
    /// Создать объект с 3D-мешем встроенной формы.
    CreateMesh(MeshShape),
    CreateLight(LightKind),
    Delete(GameObjectId),
    Duplicate(GameObjectId),
    /// Сменить родителя, сохранив мировую позу. `None` — в корень сцены.
    Reparent { child: GameObjectId, parent: Option<GameObjectId> },
    /// Сдвинуть среди «братьев»: −1 — выше, +1 — ниже.
    MoveSibling { id: GameObjectId, delta: i32 },
    Rename { id: GameObjectId, name: String },
    AddComponent { object: GameObjectId, kind: String },
    /// `index` — порядковый номер компонента в инспекторе.
    RemoveComponent { object: GameObjectId, index: usize },
    /// Переставить компонент: с места `from` на место `to` (номера как в инспекторе).
    MoveComponent { object: GameObjectId, from: usize, to: usize },
    /// Один раз отсортировать детей объекта (`None` — корни сцены) по имени, без учёта
    /// регистра и с числами по значению. Отменяется как обычная правка.
    SortChildren { parent: Option<GameObjectId> },
    /// Создать экземпляр префаба (ссылка — путь относительно каталога ассетов или GUID).
    Instantiate { reference: String, position: Option<Vec3> },
    /// Создать объект со спрайтом из текстуры.
    CreateSpriteFromTexture { guid: Guid, position: Option<Vec3> },
    SavePrefab(GameObjectId),
    /// Сохранить объект префабом в каталог ассетов `folder` (строку иерархии бросили в окно
    /// Project); объект становится экземпляром.
    SavePrefabIn { object: GameObjectId, folder: String },
    UnpackPrefab(GameObjectId),
    Play,
    /// Пауза / продолжить.
    Pause,
    /// Один кадр длиной в фиксированный шаг; игра остаётся на паузе.
    Step,
    Stop,
    NewScene,
    /// Новая 3D-сцена: камера с перспективой, направленный свет, земля; вид Scene — 3D.
    NewScene3D,
    /// Путь относительно каталога ассетов.
    OpenScene(String),
    Save,
    SaveAs(String),
    Undo,
    Redo,
    /// Навести камеру редактора на выделенный объект (или на всю сцену).
    FrameSelected,
    RefreshAssets,
    Quit,
    /// Открыть файл во вкладке Code.
    OpenCode(PathBuf),
    /// Сохранить активный документ вкладки Code.
    SaveCode,
    /// Запустить команды сборки из [`EditorConfig::build_commands`].
    Build,
    /// Создать скрипт на C++ из шаблона, открыть его во вкладке Code и собрать плагин.
    /// Если задан `attach_to`, скрипт повесится на объект, как только плагин загрузится.
    NewScript { name: String, attach_to: Option<GameObjectId> },
    /// Создать объект по заготовке из меню GameObject: `"UI/Button"`, `"Audio/Audio Source"`,
    /// `"Effects/Particle System"`… Заготовка доступна, только если её компоненты
    /// зарегистрированы. Объекты интерфейса попадают под холст (`Canvas`).
    Create { preset: String, parent: Option<GameObjectId> },
    /// Включить или выключить объект (флажок в иерархии и инспекторе).
    SetActive { id: GameObjectId, active: bool },
    /// Сменить родителя и встать на место `index` среди его детей (перетаскивание
    /// между строками иерархии). Мировая поза сохраняется.
    ReparentAt { child: GameObjectId, parent: Option<GameObjectId>, index: usize },
    /// Вернуть полям компонента значения по умолчанию.
    ResetComponent { object: GameObjectId, index: usize },
    /// Создать каталог (путь относительно каталога ассетов).
    CreateFolder(String),
    /// Переименовать или переместить ассет или каталог вместе с `.meta`
    /// (пути относительно каталога ассетов). GUID сохраняется.
    RenameAsset { from: String, to: String },
    /// Удалить ассет (вместе с `.meta`) или каталог с диска — без корзины.
    DeleteAsset(String),
    /// Показать файл в проводнике системы.
    RevealAsset(String),
    /// Открыть модель `.pmodel` (путь относительно ассетов) в Piska Modeler.
    OpenModel(String),
    /// Создать пустую модель `.pmodel` в каталоге ассетов и открыть её в Piska Modeler.
    CreateModel { folder: String },
    /// Открыть или закрыть окно File → Build….
    ShowBuildDialog(bool),
    /// Экспортировать игру ([`EditorConfig::game_crate`]) — в фоне, вывод в Console.
    Export(crate::export::ExportSettings),
    /// Показать каталог последнего экспорта в файловом менеджере.
    RevealExport,
    /// Открыть модель (`.pmodel`) в Piska Modeler.
    EditInModeler(std::path::PathBuf),
    /// Чужой меш (`.obj`, `.glb`…): новая `.pmodel` рядом с ним как опорой — в Piska Modeler.
    EditCopyInModeler(std::path::PathBuf),
    /// Открыть последнюю веб-сборку в браузере через локальный сервер.
    RunInBrowser,
    /// Окна: открыть, закрыть, пристыковать, развернуть, раскладки (меню Window).
    Layout(crate::dock::LayoutAction),
    /// Создать скрипт на Rust из шаблона в [`EditorConfig::rust_script_dir`], обновить
    /// `mod.rs`, открыть файл во вкладке Code и пересобрать игру с перезапуском
    /// редактора. `attach_to` — повесить скрипт на объект после перезапуска.
    NewRustScript { name: String, attach_to: Option<GameObjectId> },
    /// Создать сценарий `<folder>/<name>.scenario` (по умолчанию `scenarios/`) из шаблона,
    /// открыть во вкладке Code; `attach_to` — сразу повесить на объект `ScenarioRunner`.
    NewScenario { name: String, folder: String, attach_to: Option<GameObjectId> },
    /// Переименовать файл скрипта (`to` — новое имя файла в том же каталоге).
    RenameScript { from: PathBuf, to: String },
    /// Удалить файл скрипта с диска.
    DeleteScript(PathBuf),
    /// Создать `New Material.material` в каталоге (путь относительно каталога ассетов)
    /// и открыть его в инспекторе.
    CreateMaterial { folder: String },
    /// Показать материал в инспекторе (`None` — снова выделенный объект).
    InspectMaterial(Option<Guid>),
    /// Назначить материал слоту (части меша) `MeshRenderer` объекта.
    AssignMaterial { object: GameObjectId, material: Guid, slot: usize },
    /// Создать `.material` из материалов файла модели объекта и назначить их частям.
    ExtractMaterials(GameObjectId),
    /// File → Project Hub…: показать Piska Hub и закрыть редактор (сначала вопрос про
    /// несохранённую сцену).
    ProjectHub,
    /// File → Open Recent: попросить Piska Hub открыть проект (каталог крейта игры) и
    /// закрыть этот редактор.
    OpenProject(PathBuf),
    /// Выделить несколько объектов; `active` (или последний) — активный.
    SelectMany { ids: Vec<GameObjectId>, active: Option<GameObjectId> },
    /// Ctrl+щелчок: добавить к выделению или убрать из него.
    ToggleSelect(GameObjectId),
    /// Ctrl+A: все объекты сцены.
    SelectAll,
    /// Удалить все выделенные объекты (одним шагом Undo).
    DeleteSelection,
    /// Ctrl+D для всего выделения.
    DuplicateSelection,
    /// Ctrl+C: скопировать выделенные объекты (с потомками) в буфер редактора.
    CopySelection,
    /// Ctrl+X: скопировать и удалить.
    CutSelection,
    /// Ctrl+V — рядом с активным объектом; `as_child` (Ctrl+Shift+V) — внутрь него.
    Paste { as_child: bool },
    /// Ctrl+G: новый пустой родитель в середине выделения, выделенное — внутрь.
    GroupSelection,
    /// H / Shift+H: выключить или включить все выделенные объекты.
    SetSelectionActive(bool),
}

/// Имя действия для окна Undo History: вариант `Action` словами (`CreateEmpty` →
/// «Create Empty»).
fn action_label(action: &Action) -> String {
    let debug = format!("{action:?}");
    let variant = debug.split(|c: char| !c.is_alphanumeric()).next().unwrap_or_default();
    crate::theme::pretty_type(variant)
}

impl Editor {
    /// Шаги окна Undo History: имена от первого к последнему и сколько из них сейчас
    /// применено (позиция «вы здесь»; шаги дальше неё — то, что вернёт Redo).
    pub fn undo_history(&self) -> (Vec<String>, usize) {
        let mut steps = self.history.undo_labels.clone();
        steps.extend(self.history.redo_labels.iter().rev().cloned());
        (steps, self.history.undo_labels.len())
    }

    /// Перейти к позиции `position` истории (0 — до первого шага): Undo / Redo нужное число раз.
    pub fn jump_in_history(&mut self, position: usize) {
        self.settle();
        while self.history.undo.len() > position {
            let before = self.history.undo.len();
            self.apply(Action::Undo);
            if self.history.undo.len() == before {
                break;
            }
        }
        while self.history.undo.len() < position && !self.history.redo.is_empty() {
            self.apply(Action::Redo);
        }
    }
}

/// Строка окна Project.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AssetEntry {
    pub guid: Guid,
    /// Путь относительно каталога ассетов, с `/`.
    pub path: String,
    pub importer: String,
}

pub(crate) enum Dialog {
    SaveAs { path: String, then: Option<Box<Action>> },
    Unsaved { then: Box<Action> },
    NewScript { name: String, object: Option<GameObjectId> },
    /// «New Scenario Script»: имя файла, каталог в ассетах, объект для `ScenarioRunner`.
    NewScenario { name: String, folder: String, object: Option<GameObjectId> },
    NewFolder { parent: String, name: String },
}

/// Undo на снимках: `baseline` — сцена на момент последнего «спокойного» кадра.
struct History {
    baseline: SceneSnapshot,
    /// Идёт правка (тянут ползунок или гизмо, печатают имя) — это один шаг Undo.
    editing: bool,
    undo: Vec<SceneSnapshot>,
    redo: Vec<SceneSnapshot>,
    /// Имена шагов (окно Undo History): `undo_labels[i]` — шаг из `undo[i]` в следующее
    /// состояние, `redo_labels[i]` — шаг в `redo[i]`.
    undo_labels: Vec<String>,
    redo_labels: Vec<String>,
    /// Имя для следующего шага (действие, которое сейчас выполняется).
    next_label: Option<String>,
}

/// Редактор: приложение-игра плюс всё состояние окон.
pub struct Editor {
    /// Игра, которую редактируем. В режиме Edit её кадры не идут.
    pub app: App,
    pub(crate) config: EditorConfig,
    pub(crate) mode: Mode,
    pub(crate) tool: Tool,
    pub(crate) view_tab: ViewTab,
    pub(crate) bottom_tab: BottomTab,
    pub(crate) selection: Option<GameObjectId>,
    /// Всё выделение (с активным `selection`), см. `selection.rs`.
    pub(crate) selected: Vec<GameObjectId>,
    /// Объекты, скопированные Ctrl+C / Ctrl+X.
    pub(crate) object_clipboard: Vec<engine_core::serialize::NodeData>,
    /// Гизмо крутит выделение вокруг его середины, а не активного объекта.
    pub(crate) pivot_center: bool,
    /// Окно Help → Keyboard Shortcuts.
    pub(crate) shortcuts_help_open: bool,
    /// Окна Help: Getting Started, «Piska Modeler не найден».
    pub(crate) help: crate::help::HelpState,
    /// Заставка оконного запуска (splash.rs).
    pub(crate) splash: crate::splash::Splash,
    /// Сколько кадров нарисовано уже с темой и шрифтами.
    pub(crate) themed_frames: u64,
    /// Где в последнем кадре стояли части рамки окна (для проверок выравнивания).
    pub(crate) chrome: ChromeRects,
    pub(crate) scene_path: Option<String>,
    pub(crate) dirty: bool,
    history: History,
    play_snapshot: Option<SceneSnapshot>,
    /// Камера вида Scene: центр и половина высоты в мировых единицах.
    pub(crate) cam_center: Vec2,
    pub(crate) cam_half_height: f32,
    /// Вид Scene показывает сцену в 3D (орбитальная камера), а не в плоскости XY.
    pub(crate) scene_3d: bool,
    pub(crate) orbit: Orbit3D,
    pub(crate) viewport: ViewportState,
    pub(crate) hierarchy: HierarchyState,
    pub(crate) inspector: InspectorState,
    pub(crate) project: ProjectState,
    /// Кэш и очередь предпросмотра моделей (см. `preview.rs`).
    pub(crate) previews: crate::preview::Previews,
    /// Редактор ассетов `.material` и слоты материалов (см. `material_editor.rs`).
    pub(crate) material_editor: crate::material_editor::MaterialEditor,
    pub(crate) console: ConsoleState,
    pub(crate) profiler: ProfilerState,
    pub(crate) code: CodeState,
    pub(crate) assets: Vec<AssetEntry>,
    pub(crate) actions: Vec<Action>,
    pub(crate) dialog: Option<Dialog>,
    /// Файлы ассетов перечитаны: рендереру пора забыть загруженные текстуры и меши.
    pub(crate) gpu_assets_stale: bool,
    /// Скрипты, которые ждут загрузки плагина, чтобы повеситься на объект.
    pub(crate) pending_scripts: Vec<(String, GameObjectId)>,
    pub(crate) quit: bool,
    /// Окно New Script открывается на Rust (иначе — C++): последний выбранный язык.
    pub(crate) script_rust: bool,
    /// Сборка на Rust с перезапуском и передача сеанса новому процессу.
    pub(crate) rust: crate::rust_scripts::RustState,
    /// Настройки из окна Preferences.
    pub(crate) prefs: Preferences,
    pub(crate) preferences_open: bool,
    /// Окно Edit → Project Settings…: порядок выполнения скриптов.
    pub(crate) script_order: crate::script_order::ScriptOrderState,
    /// Окна настроек (Preferences, Project Settings): страницы, поиск, черновик проекта.
    pub(crate) settings: crate::settings::SettingsState,
    /// Окно File → Build… и последний экспорт.
    pub(crate) export: crate::export::ExportState,
    /// Сетка во вьюпорте.
    pub(crate) show_grid: bool,
    /// Значки объектов, каркасы коллайдеров и направления света во вьюпорте.
    pub(crate) show_gizmos: bool,
    /// Привязка гизмо к шагу всегда (Ctrl — наоборот, временно выключает).
    pub(crate) snap: bool,
    /// Гизмо перемещения — по осям объекта, а не мира.
    pub(crate) gizmo_local: bool,
    /// Каталоги внутри каталога ассетов (с `/`), включая пустые.
    pub(crate) folders: Vec<String>,
    /// Окна-вкладки (док), плавающие окна, развёрнутое окно, вид Game.
    pub(crate) dock: crate::dock::DockLayout,
    /// Кисти рельефа и черновик его параметров (terrain_tools.rs).
    pub(crate) terrain_tools: crate::terrain_tools::TerrainTools,
    /// Окна Open Scene… и New Scene… (scene_picker.rs).
    pub(crate) scene_picker: crate::scene_picker::ScenePicker,
    /// Окно Edit → Undo History (undo_history.rs).
    pub(crate) undo_history_open: bool,
}

impl Editor {
    /// Создать редактор для приложения. Компоненты игры должны быть уже
    /// зарегистрированы (`app.register::<T>()`), а каталог ассетов — подключён.
    pub fn new(app: App, config: EditorConfig) -> Result<Self, EditorError> {
        engine_core::logging::init();
        // Сцена с компонентом, которого нет в этой сборке, открывается: он хранится
        // как «потерянный скрипт» (см. `engine_core::missing`).
        engine_core::missing::keep_missing_scripts(true);
        let baseline = app.world.snapshot();
        let mut editor = Self {
            app,
            mode: Mode::Edit,
            tool: Tool::Move,
            view_tab: ViewTab::Scene,
            bottom_tab: BottomTab::Project,
            selection: None,
            selected: Vec::new(),
            object_clipboard: Vec::new(),
            pivot_center: false,
            shortcuts_help_open: false,
            help: crate::help::HelpState::default(),
            splash: crate::splash::Splash::default(),
            themed_frames: 0,
            chrome: ChromeRects::default(),
            scene_path: None,
            dirty: false,
            history: History {
                baseline,
                editing: false,
                undo: Vec::new(),
                redo: Vec::new(),
                undo_labels: Vec::new(),
                redo_labels: Vec::new(),
                next_label: None,
            },
            play_snapshot: None,
            cam_center: Vec2::ZERO,
            cam_half_height: 6.0,
            scene_3d: false,
            orbit: Orbit3D::default(),
            viewport: ViewportState::default(),
            hierarchy: HierarchyState::default(),
            inspector: InspectorState::default(),
            project: ProjectState::default(),
            previews: crate::preview::Previews::default(),
            material_editor: crate::material_editor::MaterialEditor::default(),
            console: ConsoleState::default(),
            profiler: ProfilerState::default(),
            code: CodeState::default(),
            assets: Vec::new(),
            actions: Vec::new(),
            dialog: None,
            gpu_assets_stale: false,
            pending_scripts: Vec::new(),
            quit: false,
            script_rust: config.rust_script_dir.is_some(),
            rust: crate::rust_scripts::RustState::default(),
            prefs: Preferences {
                ui_scale: config.ui_scale.unwrap_or(1.0),
                ..Preferences::default()
            },
            preferences_open: false,
            script_order: crate::script_order::ScriptOrderState::default(),
            settings: crate::settings::SettingsState::default(),
            export: crate::export::ExportState::default(),
            show_grid: true,
            show_gizmos: true,
            snap: false,
            gizmo_local: false,
            folders: Vec::new(),
            dock: crate::dock::DockLayout::default(),
            terrain_tools: Default::default(),
            scene_picker: Default::default(),
            undo_history_open: false,
            config,
        };
        editor.reindex_assets();
        editor.code.scan(&editor.config.code_dirs);
        if let Some(scene) = editor.config.scene.clone() {
            editor.load_scene(&scene)?;
        }
        editor.frame_initial();
        Ok(editor)
    }

    // ------------------------------------------------------------------
    // Чтение состояния
    // ------------------------------------------------------------------

    pub fn config(&self) -> &EditorConfig {
        &self.config
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn view_tab(&self) -> ViewTab {
        self.view_tab
    }

    /// Переключить вид: Scene (камера редактора) или Game (камера сцены).
    pub fn set_view_tab(&mut self, tab: ViewTab) {
        self.view_tab = tab;
        self.viewport.drag = None;
    }

    /// Какое окно показано внизу: Project, Console или Profiler.
    pub fn set_bottom_tab(&mut self, tab: BottomTab) {
        self.bottom_tab = tab;
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub fn selection(&self) -> Option<GameObjectId> {
        self.selection
    }

    /// Есть несохранённые изменения сцены.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Файл открытой сцены относительно каталога ассетов.
    pub fn scene_path(&self) -> Option<&str> {
        self.scene_path.as_deref()
    }

    pub fn can_undo(&self) -> bool {
        self.material_undo_available() || (self.mode == Mode::Edit && (!self.history.undo.is_empty() || self.history.editing))
    }

    pub fn can_redo(&self) -> bool {
        self.material_redo_available() || (self.mode == Mode::Edit && !self.history.redo.is_empty())
    }

    /// Среднее время кадра по частям за последние секунды — то, что показывает Profiler.
    pub fn frame_timings(&self) -> FrameTimings {
        self.profiler.summary()
    }

    /// Открыт модальный диалог (несохранённые изменения, «Сохранить как»).
    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    /// Пользователь попросил закрыть редактор (и разобрался с несохранённым).
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Настройки редактора (окно Preferences).
    pub fn preferences(&self) -> &Preferences {
        &self.prefs
    }

    /// Заменить настройки (масштаб интерфейса применится в следующем кадре).
    pub fn set_preferences(&mut self, prefs: Preferences) {
        self.prefs = prefs;
        // Сцена, открытая при запуске (до чтения настроек), — тоже в недавние.
        if let Some(path) = self.scene_path.clone() {
            self.remember_scene(&path);
        }
        self.apply_panel_prefs();
        // Раскладку окон из настроек (испорченную — Default с предупреждением).
        self.restore_layout_from_prefs();
    }

    /// Показать ассет в окне Project: открыть его каталог и выделить
    /// (путь относительно каталога ассетов).
    pub fn select_asset(&mut self, relative_path: &str) {
        self.project.reveal(relative_path);
        self.bottom_tab = BottomTab::Project;
    }

    /// Какое окно показано внизу.
    pub fn bottom_tab(&self) -> BottomTab {
        self.bottom_tab
    }

    /// Открыть или закрыть окно Preferences.
    pub fn show_preferences(&mut self, open: bool) {
        self.preferences_open = open;
    }

    /// Открыто ли окно Preferences.
    pub fn is_preferences_open(&self) -> bool {
        self.preferences_open
    }

    /// Привязка гизмо к шагу (шаги — Preferences → Scene View) включена постоянно.
    pub fn is_snapping(&self) -> bool {
        self.snap
    }

    pub fn set_snapping(&mut self, snap: bool) {
        self.snap = snap;
    }

    /// Показывать сетку во вьюпорте.
    pub fn set_grid_visible(&mut self, visible: bool) {
        self.show_grid = visible;
    }

    /// Поставить камеру 3D-вида (так же её двигают мышью); для тестов и снимков.
    pub fn set_orbit(&mut self, orbit: crate::view3d::Orbit3D) {
        self.orbit = orbit;
        self.viewport.orbit_goal = None;
    }

    /// Камера 3D-вида Scene.
    pub fn orbit(&self) -> crate::view3d::Orbit3D {
        self.orbit
    }

    /// Где на экране ручка значка ориентации для мирового направления (`Vec3::X`,
    /// `Vec3::NEG_Z`, …): щелчок по ней ставит камеру с этой стороны.
    pub fn scene_compass_handle(&self, direction: Vec3) -> Option<egui::Pos2> {
        crate::view3d::compass_handles(&self.orbit, self.viewport.rect)
            .into_iter()
            .find(|handle| handle.direction.distance(direction) < 1e-3)
            .map(|handle| handle.center)
    }

    /// Показывать значки объектов и каркасы коллайдеров во вьюпорте.
    pub fn set_gizmos_visible(&mut self, visible: bool) {
        self.show_gizmos = visible;
    }

    /// Гизмо перемещения — по локальным осям объекта (`true`) или по осям мира.
    pub fn set_gizmo_local(&mut self, local: bool) {
        self.gizmo_local = local;
    }

    /// Нужно ли рисовать кадры непрерывно: идёт игра, сборка, ждут скрипты или
    /// перерисовка по требованию выключена в настройках.
    pub fn wants_continuous_redraw(&self) -> bool {
        self.mode == Mode::Playing
            || !self.prefs.redraw_on_demand
            // Открытый Profiler показывает живой график: без этого в режиме Edit он
            // обновлялся бы только при движении мыши и выглядел зависшим.
            || self.is_panel_visible(crate::dock::Panel::Profiler)
            || self.code.build_status() == BuildStatus::Running
            || self.rust_busy()
    }

    /// Путь ассета для показа: относительно каталога ассетов и с `/`. Абсолютные пути
    /// внутри каталога ассетов или каталогов кода тоже укорачиваются.
    pub fn display_path(&self, path: &std::path::Path) -> String {
        if let Some(root) = self.assets_root() {
            if let Ok(relative) = path.strip_prefix(&root) {
                return format!("Assets/{}", relative.to_string_lossy().replace('\\', "/"));
            }
        }
        for dir in &self.config.code_dirs {
            if let Ok(relative) = path.strip_prefix(dir) {
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                return format!("{name}/{}", relative.to_string_lossy().replace('\\', "/"));
            }
        }
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    /// Заголовок окна: «Piska Editor — Level 1*».
    pub fn window_title(&self) -> String {
        format!(
            "{} — {}{}",
            self.config.title,
            self.app.world.scene.name,
            if self.dirty { "*" } else { "" }
        )
    }

    pub(crate) fn assets_root(&self) -> Option<PathBuf> {
        self.app
            .world
            .resources
            .get::<AssetDatabase>()
            .map(|db| db.root().to_path_buf())
    }

    pub(crate) fn require_assets_root(&self, what: &str) -> Result<PathBuf, EditorError> {
        self.assets_root().ok_or_else(|| {
            EditorError::Other(format!(
                "cannot {what}: no assets directory is mounted. Call \
                 `engine::mount_assets(&mut app, \"assets\")` before starting the editor"
            ))
        })
    }

    fn scene_file(&self, relative: &str) -> PathBuf {
        match self.assets_root() {
            Some(root) => root.join(relative),
            None => PathBuf::from(relative),
        }
    }

    // ------------------------------------------------------------------
    // Кадр
    // ------------------------------------------------------------------

    /// Продвинуть игру на кадр, если она идёт. Вызывается раз за кадр редактора.
    pub fn advance(&mut self, dt: f32) {
        if self.mode == Mode::Playing {
            self.app.tick(dt);
            self.drop_dead_selection();
            // Игра попросила выйти (кнопка Quit в её меню): в редакторе это Stop, а не
            // закрытие окна.
            if self.app.world.quit_requested() {
                log::info!("the game asked to quit: leaving Play mode");
                if let Err(err) = self.stop() {
                    log::error!("{err}");
                }
            }
        } else {
            // Игра стоит, но подсистемы могут, например, подхватить пересобранный плагин.
            self.app.idle();
        }
        self.attach_pending_scripts();
        self.poll_rust();
    }

    /// Повесить новые скрипты на объекты, как только их тип появился в реестре
    /// (то есть плагин пересобран и загружен).
    fn attach_pending_scripts(&mut self) {
        if self.pending_scripts.is_empty() {
            return;
        }
        let ready: Vec<(String, GameObjectId)> = self
            .pending_scripts
            .iter()
            .filter(|(kind, _)| self.app.world.registry.contains(kind))
            .cloned()
            .collect();
        self.pending_scripts
            .retain(|(kind, _)| !self.app.world.registry.contains(kind));
        for (kind, object) in ready {
            if self.app.world.scene.contains(object) {
                log::info!("script `{kind}` is built and attached");
                self.apply(Action::AddComponent { object, kind });
            }
        }
    }

    fn new_script(&mut self, name: &str, attach_to: Option<GameObjectId>) -> Result<(), EditorError> {
        let dir = self.config.script_dir.clone().ok_or_else(|| {
            EditorError::Other(
                "cannot create a script: the editor does not know where C++ scripts live. \
                 Pass the folder when starting it: `EditorConfig::default().cpp_scripts(\"plugins/game/scripts\")`"
                    .to_string(),
            )
        })?;
        let name = name.trim();
        let valid = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(EditorError::Other(trf!(
                "`{name}` cannot be a script name: use a C++ class name such as `EnemyPatrol` \
                 (Latin letters, digits and `_`, starting with a capital letter)"
            , name = name)));
        }
        let path = dir.join(format!("{name}.cpp", name = name));
        if self.app.world.registry.contains(name) || path.exists() {
            return Err(EditorError::Other(trf!(
                "a component named `{name}` already exists; pick another name"
            , name = name)));
        }
        let io = |source: std::io::Error| SceneError::Io {
            path: path.display().to_string(),
            source,
        };
        std::fs::create_dir_all(&dir).map_err(io)?;
        std::fs::write(&path, SCRIPT_TEMPLATE.replace("{Name}", name)).map_err(io)?;
        log::info!("created `{}`", path.display());

        self.code.scan(&self.config.code_dirs);
        self.code.open(&path).map_err(EditorError::Other)?;
        self.view_tab = ViewTab::Code;
        if let Some(object) = attach_to {
            self.pending_scripts.push((name.to_string(), object));
        }
        if self.config.build_commands.is_empty() && self.config.relaunch.is_some() {
            // C++, вкомпилированный в игру (`engine-cpp-build`): нужна пересборка с перезапуском.
            self.start_rust_build()?;
        } else if self.config.build_commands.is_empty() {
            log::warn!("no build commands are configured: build the plugin yourself to start using `{name}`");
        } else {
            self.code.start_build(self.config.build_commands.iter().cloned().map(|c| self.with_tools(c)).collect());
        }
        Ok(())
    }

    /// Скрипты, ожидающие сборки плагина (имя типа, объект).
    pub fn pending_scripts(&self) -> &[(String, GameObjectId)] {
        &self.pending_scripts
    }

    /// Активный документ вкладки Code: путь, текст, есть ли несохранённые правки.
    pub fn code_document(&self) -> Option<(&std::path::Path, &str, bool)> {
        self.code.active()
    }

    /// Заменить текст активного документа (то же, что набрать его в редакторе).
    pub fn set_code_text(&mut self, text: &str) {
        self.code.set_active_text(text);
    }

    /// Чем закончилась последняя сборка.
    pub fn build_status(&self) -> BuildStatus {
        self.code.build_status()
    }

    /// Каким видом рисовать вьюпорт в этом кадре (`size` — размер текстуры в пикселях).
    pub(crate) fn camera_view(&self, renderer: &mut Renderer, size: [u32; 2]) -> CameraView {
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        match self.view_tab {
            ViewTab::Scene if self.scene_3d => {
                let [r, g, b] = self.scene_background();
                let mut view = self.orbit.view(aspect, Color::rgb(r, g, b));
                // Сетка — внутри 3D-прохода: модели закрывают её, а не рисуется поверх.
                view.grid = self.scene_grid();
                // Туман сцены в виде Scene — по выбору (Preferences → Scene View, меню Gizmos).
                view.fog = self.prefs.scene_fog;
                view
            }
            ViewTab::Scene => {
                let [r, g, b] = self.scene_background();
                let mut view = CameraView::orthographic_2d(
                    self.cam_center,
                    self.cam_half_height,
                    aspect,
                    Color::rgb(r, g, b),
                );
                // Сетка 2D-вида — за спрайтами, внутри прохода рендера.
                view.grid = self.scene_grid();
                view
            }
            ViewTab::Game | ViewTab::Code => renderer.scene_view(&self.app.world, aspect),
        }
    }

    /// Размер текстуры вьюпорта в пикселях по последней раскладке интерфейса.
    pub(crate) fn viewport_pixels(&self) -> [u32; 2] {
        self.viewport.pixels
    }

    pub(crate) fn set_viewport_texture(&mut self, texture: egui::TextureId) {
        self.viewport.texture = Some(texture);
    }

    /// Рендер сообщает редактору итоги кадра.
    pub(crate) fn frame_rendered(&mut self, stats: RenderStats, timings: FrameTimings) {
        self.profiler.push(timings, stats, self.app.world.scene.len());
    }

    // ------------------------------------------------------------------
    // Undo
    // ------------------------------------------------------------------

    /// Сцену только что изменили (или вот-вот изменят). Первый вызов за правку
    /// открывает шаг Undo; правка длится, пока тянут мышь или печатают.
    pub(crate) fn touch(&mut self) {
        if self.mode != Mode::Edit {
            return; // правки в Play живут до Stop и в историю не попадают
        }
        self.dirty = true;
        if !self.history.editing {
            self.history.editing = true;
            self.history.undo.push(self.history.baseline.clone());
            let label = self.history.next_label.take().unwrap_or_else(|| "Edit".to_string());
            self.history.undo_labels.push(label);
            if self.history.undo.len() > self.prefs.undo_limit.max(1) {
                self.history.undo.remove(0);
                self.history.undo_labels.remove(0);
            }
            self.history.redo.clear();
            self.history.redo_labels.clear();
        }
    }

    /// Правка закончилась: запомнить новое состояние как точку отсчёта.
    pub(crate) fn settle(&mut self) {
        if !self.history.editing {
            return;
        }
        self.history.editing = false;
        let now = self.app.world.snapshot();
        if now == self.history.baseline {
            self.history.undo.pop(); // «правка», которая ничего не изменила
            self.history.undo_labels.pop();
        } else {
            self.history.baseline = now;
        }
    }

    pub(crate) fn reset_history(&mut self) {
        self.history = History {
            baseline: self.app.world.snapshot(),
            editing: false,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_labels: Vec::new(),
            redo_labels: Vec::new(),
            next_label: None,
        };
        self.dirty = false;
    }

    fn restore_baseline(&mut self) -> Result<(), EditorError> {
        let paths = self.selection_paths();
        self.app.world.restore(&self.history.baseline)?;
        self.restore_selection_paths(paths);
        self.dirty = true;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Действия
    // ------------------------------------------------------------------

    /// Выполнить действие. Ошибка не прерывает работу: она попадает в консоль.
    pub fn apply(&mut self, action: Action) {
        if let Err(err) = self.try_apply(action) {
            log::error!("{err}");
        }
        // Действие без правки (выделение) не должно назвать следующую правку мышью.
        self.history.next_label = None;
        self.normalize_selection();
    }

    /// Выполнить всё, что панели накопили за кадр.
    pub(crate) fn apply_queued(&mut self) {
        for action in std::mem::take(&mut self.actions) {
            self.apply(action);
        }
    }

    pub(crate) fn try_apply(&mut self, action: Action) -> Result<(), EditorError> {
        // Имя шага Undo — по действию («Rename», «Create Empty»); правки мышью — «Edit».
        if !self.history.editing && !matches!(action, Action::Undo | Action::Redo) {
            self.history.next_label = Some(action_label(&action));
        }
        // Действия, которые выбрасывают текущую сцену, сначала спрашивают про несохранённое.
        let discards_scene = matches!(
            action,
            Action::NewScene
                | Action::NewScene3D
                | Action::OpenScene(_)
                | Action::Quit
                | Action::ProjectHub
                | Action::OpenProject(_)
        );
        if discards_scene {
            if self.mode != Mode::Edit {
                self.stop()?;
            }
            if self.dirty {
                self.dialog = Some(Dialog::Unsaved {
                    then: Box::new(action),
                });
                return Ok(());
            }
        }

        match action {
            Action::SelectMany { .. }
            | Action::ToggleSelect(_)
            | Action::SelectAll
            | Action::DeleteSelection
            | Action::DuplicateSelection
            | Action::CopySelection
            | Action::CutSelection
            | Action::Paste { .. }
            | Action::GroupSelection
            | Action::SetSelectionActive(_) => self.apply_selection_action(action)?,
            Action::Select(id) => {
                self.selection = id.filter(|id| self.app.world.scene.contains(*id));
                self.selected = self.selection.into_iter().collect();
                // Выделили объект — инспектор снова показывает его, а не материал.
                if self.selection.is_some() {
                    self.open_material(None);
                }
            }
            Action::CreateEmpty { parent } => {
                self.touch();
                let id = self.create_object("GameObject", parent)?;
                self.selection = Some(id);
            }
            Action::CreateSprite { parent } => {
                self.touch();
                let id = self.create_object(tr_s("Sprite"), parent)?;
                if let Some(obj) = self.app.world.scene.get_mut(id) {
                    obj.add(SpriteRenderer::default());
                }
                self.selection = Some(id);
            }
            Action::CreateMesh(shape) => {
                self.touch();
                let id = self.create_object(&format!("{shape:?}"), None)?;
                if let Some(obj) = self.app.world.scene.get_mut(id) {
                    // Плоскость ложится на землю, остальные формы — стоят на ней.
                    if shape != MeshShape::Plane {
                        obj.transform.position.y += 0.5;
                    }
                    obj.add(MeshRenderer::shape(shape));
                }
                self.selection = Some(id);
            }
            Action::CreateLight(kind) => {
                self.touch();
                let name = match kind {
                    LightKind::Directional => tr_s("Directional Light"),
                    LightKind::Point => tr_s("Point Light"),
                };
                let id = self.create_object(name, None)?;
                if let Some(obj) = self.app.world.scene.get_mut(id) {
                    match kind {
                        LightKind::Directional => {
                            obj.transform.set_euler_deg(Vec3::new(-50.0, 30.0, 0.0));
                            obj.add(Light::default());
                        }
                        LightKind::Point => {
                            obj.transform.position.y += 2.0;
                            obj.add(Light::point(Color::WHITE, 8.0, 10.0));
                        }
                    }
                }
                self.selection = Some(id);
            }
            Action::CreateCamera => {
                self.touch();
                let id = self.create_object(tr_s("Camera"), None)?;
                if let Some(obj) = self.app.world.scene.get_mut(id) {
                    obj.add(Camera::default());
                }
                self.selection = Some(id);
            }
            Action::Delete(id) => {
                if self.app.world.scene.contains(id) {
                    self.touch();
                    self.app.world.scene.destroy(id);
                    self.app.world.flush_destroyed();
                    self.drop_dead_selection();
                }
            }
            Action::Duplicate(id) => {
                self.touch();
                let copy = self.app.world.duplicate(id)?;
                let scene = &mut self.app.world.scene;
                let parent = scene.get(copy).and_then(|o| o.parent());
                let base = scene.get(id).map(|o| o.name.clone()).unwrap_or_default();
                let name = unique_name(scene, parent, &base, Some(copy));
                if let Some(obj) = scene.get_mut(copy) {
                    obj.name = name;
                }
                self.selection = Some(copy);
            }
            Action::Reparent { child, parent } => {
                if Some(child) != parent {
                    self.touch();
                    self.app.world.scene.set_parent_keep_world(child, parent)?;
                }
            }
            Action::MoveSibling { id, delta } => {
                let scene = &mut self.app.world.scene;
                let Some(obj) = scene.get(id) else {
                    return Ok(());
                };
                let siblings = match obj.parent() {
                    Some(parent) => scene.get(parent).map(|p| p.children().to_vec()),
                    None => Some(scene.roots().to_vec()),
                }
                .unwrap_or_default();
                if let Some(index) = siblings.iter().position(|s| *s == id) {
                    let target = (index as i64 + delta as i64).clamp(0, siblings.len() as i64 - 1);
                    if target as usize != index {
                        self.touch();
                        self.app.world.scene.set_sibling_index(id, target as usize);
                    }
                }
            }
            Action::MoveComponent { object, from, to } => {
                let moves = self
                    .app
                    .world
                    .scene
                    .get(object)
                    .is_some_and(|o| from != to && from < o.components().count() && to < o.components().count());
                if moves {
                    self.touch();
                    if let Some(obj) = self.app.world.scene.get_mut(object) {
                        obj.move_component(from, to);
                    }
                }
            }
            Action::SortChildren { parent } => {
                let scene = &self.app.world.scene;
                let siblings = match parent {
                    Some(parent) => scene.get(parent).map(|p| p.children().to_vec()),
                    None => Some(scene.roots().to_vec()),
                }
                .unwrap_or_default();
                let mut sorted = siblings.clone();
                sorted.sort_by(|a, b| {
                    let name = |id: &GameObjectId| scene.get(*id).map(|o| o.name.clone()).unwrap_or_default();
                    crate::sorting::natural_cmp(&name(a), &name(b))
                });
                if sorted != siblings {
                    self.touch();
                    for (index, id) in sorted.into_iter().enumerate() {
                        self.app.world.scene.set_sibling_index(id, index);
                    }
                }
            }
            Action::Rename { id, name } => {
                let name = name.trim();
                if !name.is_empty() && self.app.world.scene.get(id).is_some_and(|o| o.name != name) {
                    self.touch();
                    if let Some(obj) = self.app.world.scene.get_mut(id) {
                        obj.name = name.to_string();
                    }
                }
            }
            Action::AddComponent { object, kind } => {
                let component = self.app.world.registry.create(&kind)?;
                self.touch();
                let obj = self
                    .app
                    .world
                    .scene
                    .get_mut(object)
                    .ok_or(SceneError::DeadObject(object))?;
                obj.add_boxed(component);
            }
            Action::RemoveComponent { object, index } => {
                self.touch();
                if let Some(obj) = self.app.world.scene.get_mut(object) {
                    obj.remove_at(index);
                }
                self.app.world.flush_destroyed(); // сжать список компонентов
            }
            Action::Instantiate { reference, position } => {
                self.touch();
                let id = self.app.world.instantiate(&reference)?;
                let position = position.unwrap_or_else(|| self.spawn_point());
                let scene = &mut self.app.world.scene;
                let base = scene.get(id).map(|o| capitalize(&o.name)).unwrap_or_default();
                let name = unique_name(scene, None, &base, Some(id));
                if let Some(obj) = scene.get_mut(id) {
                    obj.name = name;
                    obj.transform.position = position;
                }
                self.selection = Some(id);
            }
            Action::CreateSpriteFromTexture { guid, position } => {
                self.touch();
                let name = self
                    .assets
                    .iter()
                    .find(|asset| asset.guid == guid)
                    .map(|asset| capitalize(file_stem(&asset.path)))
                    .unwrap_or_else(|| tr_s("Sprite").to_string());
                let id = self.create_object(&name, None)?;
                let position = position.unwrap_or_else(|| self.spawn_point());
                if let Some(obj) = self.app.world.scene.get_mut(id) {
                    obj.transform.position = position;
                    obj.add(SpriteRenderer::textured(AssetHandle::from_guid(guid)));
                }
                self.selection = Some(id);
            }
            Action::SavePrefab(id) => self.save_prefab(id)?,
            Action::SavePrefabIn { object, folder } => self.save_prefab_in(object, &folder)?,
            Action::UnpackPrefab(id) => {
                if self.app.world.scene.get(id).is_some_and(|o| o.prefab().is_some()) {
                    self.touch();
                    if let Some(obj) = self.app.world.scene.get_mut(id) {
                        obj.set_prefab(None);
                    }
                }
            }
            Action::Play => match self.mode {
                Mode::Edit => {
                    self.before_play();
                    self.play()
                }
                Mode::Paused => self.mode = Mode::Playing,
                Mode::Playing => {}
            },
            Action::Pause => match self.mode {
                Mode::Playing => {
                    self.mode = Mode::Paused;
                    self.app.world.input.release_all();
                }
                Mode::Paused => self.mode = Mode::Playing,
                Mode::Edit => {}
            },
            Action::Step => {
                if self.mode == Mode::Edit {
                    self.play();
                }
                self.mode = Mode::Paused;
                let dt = self.app.world.time.fixed_delta;
                self.app.tick(dt);
                self.drop_dead_selection();
            }
            Action::Stop => self.stop()?,
            Action::NewScene | Action::NewScene3D => {
                let three_d = action == Action::NewScene3D;
                self.app.world.restore(&SceneSnapshot {
                    name: tr_s("Untitled").to_string(),
                    objects: Vec::new(),
                })?;
                // Как в Unity: новая сцена сразу с камерой (в 3D — ещё свет и земля).
                // Имена объектов попадают в файл сцены и от языка интерфейса не зависят.
                let scene = &mut self.app.world.scene;
                if three_d {
                    let camera = scene.build("Main Camera").tag("MainCamera").with(Camera::perspective(60.0)).id(); // i18n-skip
                    if let Some(object) = scene.get_mut(camera) {
                        object.transform.position = Vec3::new(0.0, 2.0, 8.0);
                        object.transform.set_euler_deg(Vec3::new(-12.0, 0.0, 0.0));
                    }
                    let light = scene.build("Directional Light").with(Light::default()).id(); // i18n-skip
                    if let Some(object) = scene.get_mut(light) {
                        object.transform.set_euler_deg(Vec3::new(-50.0, 30.0, 0.0));
                    }
                    let ground = scene.build("Ground").with(MeshRenderer::shape(MeshShape::Plane)).id(); // i18n-skip
                    if let Some(object) = scene.get_mut(ground) {
                        object.transform.scale = Vec3::new(10.0, 1.0, 10.0);
                    }
                } else {
                    scene.build("Main Camera").tag("MainCamera").with(Camera::default()); // i18n-skip
                }
                self.scene_path = None;
                self.selection = None;
                self.reset_history();
                self.set_scene_3d(three_d);
                self.frame_initial();
            }
            Action::OpenScene(path) => {
                self.load_scene(&path)?;
                self.frame_initial();
            }
            Action::Save => self.save()?,
            Action::SaveAs(path) => self.save_as(&path)?,
            // Инспектор показывает материал — Ctrl+Z отменяет правку материала.
            Action::Undo if self.material_undo_available() => {
                self.undo_material();
            }
            Action::Redo if self.material_redo_available() => {
                self.redo_material();
            }
            Action::Undo => {
                if self.mode == Mode::Edit {
                    self.settle();
                    if let Some(previous) = self.history.undo.pop() {
                        let current = std::mem::replace(&mut self.history.baseline, previous);
                        self.history.redo.push(current);
                        let label = self.history.undo_labels.pop().unwrap_or_default();
                        self.history.redo_labels.push(label);
                        self.restore_baseline()?;
                    }
                }
            }
            Action::Redo => {
                if self.mode == Mode::Edit {
                    self.settle();
                    if let Some(next) = self.history.redo.pop() {
                        let current = std::mem::replace(&mut self.history.baseline, next);
                        self.history.undo.push(current);
                        let label = self.history.redo_labels.pop().unwrap_or_default();
                        self.history.undo_labels.push(label);
                        self.restore_baseline()?;
                    }
                }
            }
            Action::FrameSelected if self.scene_3d => {
                // Всё выделение: сфера, в которую входят сферы всех объектов.
                let spheres: Vec<(Vec3, f32)> = self.selected().into_iter().filter_map(|id| self.bounds_3d(id)).collect();
                if !spheres.is_empty() {
                    let center = spheres.iter().map(|(c, _)| *c).sum::<Vec3>() / spheres.len() as f32;
                    let radius = spheres.iter().map(|(c, r)| c.distance(center) + r).fold(0.0_f32, f32::max);
                    self.orbit.target = center;
                    self.orbit.distance = (radius * 3.2).max(1.5);
                }
            }
            Action::FrameSelected => {
                let bounds = self
                    .selected()
                    .into_iter()
                    .filter_map(|id| self.bounds_of(id))
                    .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)));
                match bounds {
                    Some((min, max)) => self.frame_rect(min, max),
                    None => self.frame_all(),
                }
            }
            Action::RefreshAssets => self.refresh_assets()?,
            Action::Quit => self.quit = true,
            Action::ProjectHub => self.go_to_hub(None)?,
            Action::OpenProject(dir) => self.go_to_hub(Some(dir))?,
            Action::OpenCode(path) => {
                self.code.open(&path).map_err(EditorError::Other)?;
                self.view_tab = ViewTab::Code;
            }
            Action::SaveCode => {
                if let Some(path) = self.code.save_active().map_err(EditorError::Other)? {
                    log::info!("saved `{}`", path.display());
                    self.hint_rust_saved(&path);
                }
            }
            Action::NewScript { name, attach_to } => self.new_script(&name, attach_to)?,
            Action::NewRustScript { name, attach_to } => self.new_rust_script(&name, attach_to)?,
            Action::NewScenario { name, folder, attach_to } => self.new_scenario(&name, &folder, attach_to)?,
            Action::RenameScript { from, to } => self.rename_script(&from, &to)?,
            Action::DeleteScript(path) => self.delete_script(&path)?,
            Action::CreateMaterial { folder } => {
                let path = self.create_material(&folder)?;
                self.refresh_assets()?;
                self.select_asset(&path);
                self.inspect_material(&path);
            }
            Action::InspectMaterial(guid) => self.open_material(guid),
            Action::AssignMaterial { object, material, slot } => {
                self.assign_material(object, material, slot);
            }
            Action::ExtractMaterials(object) => {
                self.extract_materials(object)?;
                self.refresh_assets()?;
            }
            Action::Build => {
                // Собирается то, что на диске: сначала сохраняем открытые файлы.
                if self.code.has_unsaved() {
                    self.code.save_all().map_err(EditorError::Other)?;
                }
                if self.config.relaunch.is_some() {
                    self.start_rust_build()?;
                } else {
                    self.code.start_build(self.config.build_commands.iter().cloned().map(|c| self.with_tools(c)).collect());
                }
            }
            Action::Create { preset, parent } => self.create_preset(&preset, parent)?,
            Action::SetActive { id, active } => {
                if self.app.world.scene.get(id).is_some_and(|o| o.active != active) {
                    self.touch();
                    if let Some(object) = self.app.world.scene.get_mut(id) {
                        object.active = active;
                    }
                }
            }
            Action::ReparentAt { child, parent, index } => {
                if Some(child) != parent && self.app.world.scene.contains(child) {
                    self.touch();
                    let scene = &mut self.app.world.scene;
                    if scene.get(child).and_then(|o| o.parent()) != parent {
                        scene.set_parent_keep_world(child, parent)?;
                    }
                    scene.set_sibling_index(child, index);
                }
            }
            Action::ResetComponent { object, index } => {
                let kind = self
                    .app
                    .world
                    .scene
                    .get(object)
                    .and_then(|o| o.components().nth(index))
                    .map(|c| c.type_name())
                    .ok_or(SceneError::DeadObject(object))?;
                let defaults = self.app.world.registry.create(kind)?;
                self.touch();
                if let Some(component) = self
                    .app
                    .world
                    .scene
                    .get_mut(object)
                    .and_then(|o| o.components_mut().nth(index))
                {
                    for field in component.field_names() {
                        if let Some(value) = defaults.get_field(field) {
                            let _ = component.set_field(field, &value);
                        }
                    }
                }
            }
            Action::CreateFolder(relative) => {
                let root = self.require_assets_root("create a folder")?;
                let dir = root.join(relative.trim_matches('/'));
                std::fs::create_dir_all(&dir).map_err(|source| SceneError::Io {
                    path: relative.clone(),
                    source,
                })?;
                self.refresh_assets()?;
            }
            Action::RenameAsset { from, to } => self.rename_asset(&from, &to)?,
            Action::DeleteAsset(relative) => self.delete_asset(&relative)?,
            Action::RevealAsset(relative) => {
                let root = self.require_assets_root("reveal a file")?;
                reveal(&root.join(relative.trim_matches('/')));
            }
            Action::OpenModel(relative) => {
                let root = self.require_assets_root("open a model")?;
                self.launch_modeler(Some(&root.join(relative.trim_matches('/'))));
            }
            Action::CreateModel { folder } => {
                let root = self.require_assets_root("create a model")?;
                let relative = crate::modeler_link::create_model(&root, &folder)?;
                self.refresh_assets()?;
                self.select_asset(&relative);
                self.launch_modeler(Some(&root.join(&relative)));
            }
            Action::ShowBuildDialog(open) => self.export.open = open,
            Action::Export(settings) => self.start_export(settings)?,
            Action::RevealExport => self.reveal_export()?,
            Action::EditInModeler(path) => self.launch_modeler(Some(&path)),
            Action::EditCopyInModeler(path) => self.edit_copy_in_modeler(&path)?,
            Action::RunInBrowser => self.run_in_browser()?,
            Action::Layout(action) => self.apply_layout_action(action)?,
        }
        Ok(())
    }

    /// Объект по заготовке каталога (`"UI/Button"`).
    fn create_preset(&mut self, id: &str, parent: Option<GameObjectId>) -> Result<(), EditorError> {
        let preset = catalog::preset(id).ok_or_else(|| {
            EditorError::Other(trf!(
                "there is no `{id}` in the GameObject menu; use one of: {p0}",
                p0 = catalog::PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")
            , id = id))
        })?;
        let registry = &self.app.world.registry;
        if !preset.available(registry) {
            let missing: Vec<&str> = preset
                .components
                .iter()
                .chain(preset.children.iter().flat_map(|(_, kinds, _)| kinds.iter()))
                .copied()
                .filter(|kind| !registry.contains(kind))
                .collect();
            return Err(EditorError::Other(format!(
                "cannot create `{}`: components {} are not registered. Start the editor from \
                 `engine::app()`, which installs audio, UI and effects",
                preset.label,
                missing.join(", ")
            )));
        }
        self.touch();
        let parent = match parent {
            Some(parent) => Some(parent),
            None if preset.is_ui() => self.canvas_for_new_ui()?,
            None => None,
        };
        let id = self.create_object(preset.name, parent)?;
        self.add_components(id, preset.components)?;
        for (name, kinds, text) in preset.children {
            let child = self.create_object(name, Some(id))?;
            self.add_components(child, kinds)?;
            if let Some(object) = self.app.world.scene.get_mut(child) {
                for component in object.components_mut() {
                    if component.field_names().contains(&"text") {
                        let _ = component.set_field("text", &Value::Str(text.to_string()));
                    }
                }
            }
        }
        self.selection = Some(id);
        Ok(())
    }

    fn add_components(&mut self, id: GameObjectId, kinds: &[&str]) -> Result<(), EditorError> {
        for kind in kinds {
            let component = self.app.world.registry.create(kind)?;
            if let Some(object) = self.app.world.scene.get_mut(id) {
                object.add_boxed(component);
            }
        }
        Ok(())
    }

    /// Холст для нового элемента интерфейса: выделенный (или его предок), любой в
    /// сцене, а если холстов нет — новый.
    fn canvas_for_new_ui(&mut self) -> Result<Option<GameObjectId>, EditorError> {
        let scene = &self.app.world.scene;
        let has_canvas = |id: GameObjectId| {
            scene
                .get(id)
                .is_some_and(|o| o.components().any(|c| c.type_name() == "Canvas"))
        };
        let mut current = self.selection;
        while let Some(id) = current {
            if has_canvas(id) {
                return Ok(Some(id));
            }
            current = scene.get(id).and_then(|o| o.parent());
        }
        if let Some(id) = scene.ids().into_iter().find(|id| has_canvas(*id)) {
            return Ok(Some(id));
        }
        if !self.app.world.registry.contains("Canvas") {
            return Ok(None);
        }
        let canvas = self.create_object(tr_s("Canvas"), None)?;
        self.add_components(canvas, &[tr_s("Canvas")])?;
        Ok(Some(canvas))
    }

    fn rename_asset(&mut self, from: &str, to: &str) -> Result<(), EditorError> {
        let root = self.require_assets_root("rename an asset")?;
        let (from, to) = (from.trim_matches('/'), to.trim().trim_matches('/').replace('\\', "/"));
        if to.is_empty() || to.split('/').any(|part| part.is_empty() || part == "..") {
            return Err(EditorError::Other(trf!("`{to}` is not a valid name for an asset", to = to)));
        }
        let (source, target) = (root.join(from), root.join(&to));
        if target.exists() {
            return Err(EditorError::Other(trf!("`{to}` already exists; pick another name", to = to)));
        }
        let io = |source: std::io::Error| SceneError::Io {
            path: from.to_string(),
            source,
        };
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::rename(&source, &target).map_err(io)?;
        let meta = |path: &std::path::Path| {
            let mut name = path.as_os_str().to_owned();
            name.push(".meta");
            PathBuf::from(name)
        };
        if meta(&source).is_file() {
            std::fs::rename(meta(&source), meta(&target)).map_err(io)?;
        }
        // Открытая сцена переехала вместе с файлом.
        if let Some(scene) = self.scene_path.as_mut() {
            if scene == from {
                *scene = to.clone();
            } else if let Some(rest) = scene.strip_prefix(&format!("{from}/")) {
                *scene = format!("{to}/{rest}");
            }
        }
        log::info!("renamed `{from}` to `{to}`");
        self.refresh_assets()
    }

    fn delete_asset(&mut self, relative: &str) -> Result<(), EditorError> {
        let root = self.require_assets_root("delete an asset")?;
        let relative = relative.trim_matches('/');
        if relative.is_empty() || relative.split('/').any(|part| part == "..") {
            return Err(EditorError::Other("refusing to delete the assets directory itself".to_string()));
        }
        let path = root.join(relative);
        let io = |source: std::io::Error| SceneError::Io {
            path: relative.to_string(),
            source,
        };
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(io)?;
        } else {
            std::fs::remove_file(&path).map_err(io)?;
            let mut meta = path.into_os_string();
            meta.push(".meta");
            let meta = PathBuf::from(meta);
            if meta.is_file() {
                std::fs::remove_file(meta).map_err(io)?;
            }
        }
        log::info!("deleted `{relative}`");
        self.refresh_assets()
    }

    fn drop_dead_selection(&mut self) {
        if self
            .selection
            .is_some_and(|id| !self.app.world.scene.contains(id))
        {
            self.selection = None;
        }
    }

    /// Где появляются новые объекты: центр вида Scene, округлённый до полуметра
    /// (в 3D — точка на земле под центром вида).
    fn spawn_point(&self) -> Vec3 {
        if self.scene_3d {
            let target = (self.orbit.target * 2.0).round() / 2.0;
            Vec3::new(target.x, 0.0, target.z)
        } else {
            ((self.cam_center * 2.0).round() / 2.0).extend(0.0)
        }
    }

    fn create_object(
        &mut self,
        name: &str,
        parent: Option<GameObjectId>,
    ) -> Result<GameObjectId, EditorError> {
        let position = self.spawn_point();
        let scene = &mut self.app.world.scene;
        let name = unique_name(scene, parent, name, None);
        let id = scene.spawn(name);
        match parent {
            // Ребёнок появляется в начале координат родителя.
            Some(parent) => {
                if let Err(err) = scene.set_parent(id, Some(parent)) {
                    scene.destroy(id);
                    self.app.world.flush_destroyed();
                    return Err(err.into());
                }
                let path = path_of(scene, parent);
                self.hierarchy.expand(path);
            }
            None => {
                if let Some(obj) = scene.get_mut(id) {
                    obj.transform.position = position;
                }
            }
        }
        Ok(id)
    }

    // ------------------------------------------------------------------
    // Play / Stop
    // ------------------------------------------------------------------

    fn play(&mut self) {
        self.settle();
        self.play_snapshot = Some(self.app.world.snapshot());
        let world = &mut self.app.world;
        // Таймеры, твины и time_scale прошлых запусков не должны дожить до нового.
        world.reset_gameplay_state();
        world.input.release_all();
        world.time.elapsed = 0.0;
        world.time.frame = 0;
        world.time.delta = 0.0;
        self.mode = Mode::Playing;
        // Ввод у игры, только пока фокус у окна Game (ui.rs, `forward_game_input`):
        // Play сам отдаёт ему фокус, если это не выключено в Preferences.
        self.dock.game.released = false;
        if self.prefs.focus_game_on_play {
            self.view_tab = ViewTab::Game;
            self.focus_panel(crate::dock::Panel::Game);
        }
        log::info!("play: `{}`", self.app.world.scene.name);
        let missing = self.missing_script_types();
        if !missing.is_empty() {
            log::warn!(
                "Play runs without {}: these scripts are not built into this editor, so the game cannot work as \
                 intended. Build the project's scripts (the banner in the Game view) or open the project from Piska Hub",
                missing.join(", ")
            );
        }
    }

    fn stop(&mut self) -> Result<(), EditorError> {
        if self.mode == Mode::Edit {
            return Ok(());
        }
        self.mode = Mode::Edit;
        self.view_tab = ViewTab::Scene;
        self.viewport.drag = None;
        self.dock.game.input_focus = false;
        self.dock.game.released = false;
        // Stop снимает всё: и нажатое, и захват курсора с просьбой игры.
        if self.app.world.input.is_cursor_captured() {
            self.dock.game.cursor_request = Some(false);
        }
        self.app.world.input.release_all();
        self.app.world.input.set_cursor_capture_state(false);
        self.app.world.reset_gameplay_state();
        if let Some(snapshot) = self.play_snapshot.take() {
            let paths = self.selection_paths();
            self.app.world.restore(&snapshot)?;
            self.restore_selection_paths(paths);
        }
        log::info!("stop: scene restored to its state before play");
        Ok(())
    }

    // ------------------------------------------------------------------
    // Сцены и ассеты
    // ------------------------------------------------------------------

    fn load_scene(&mut self, relative: &str) -> Result<(), EditorError> {
        let relative = relative.replace('\\', "/");
        self.app.world.load_scene(self.scene_file(&relative))?;
        self.remember_scene(&relative);
        self.scene_path = Some(relative);
        self.selection = None;
        self.reset_history();
        self.report_missing_scripts();
        Ok(())
    }

    fn save(&mut self) -> Result<(), EditorError> {
        if self.mode != Mode::Edit {
            return Err(EditorError::Other(
                tr_s("the game is running: press Stop before saving, otherwise the file would get \
                 the mid-game state of the scene")
                    .to_string(),
            ));
        }
        match self.scene_path.clone() {
            Some(path) => self.save_as(&path),
            None => {
                self.dialog = Some(Dialog::SaveAs {
                    path: self.suggested_scene_path(),
                    then: None,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn suggested_scene_path(&self) -> String {
        let name: String = self
            .app
            .world
            .scene
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        format!("scenes/{name}.scene", name = name)
    }

    fn save_as(&mut self, relative: &str) -> Result<(), EditorError> {
        if self.mode != Mode::Edit {
            return Err(EditorError::Other(
                tr_s("the game is running: press Stop before saving").to_string(),
            ));
        }
        let mut relative = relative.trim().replace('\\', "/");
        if relative.is_empty() {
            return Err(EditorError::Other("the scene file name is empty".to_string()));
        }
        if !relative.ends_with(".scene") {
            relative.push_str(".scene");
        }
        self.settle();
        let file = self.scene_file(&relative);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|source| SceneError::Io {
                path: dir.display().to_string(),
                source,
            })?;
        }
        let is_new_file = !file.exists();
        if self.scene_path.as_deref() != Some(relative.as_str()) {
            self.app.world.scene.name = capitalize(file_stem(&relative)).replace('_', " ");
            self.history.baseline = self.app.world.snapshot();
        }
        self.app.world.save_scene(&file)?;
        self.remember_scene(&relative);
        self.scene_path = Some(relative.clone());
        self.dirty = false;
        log::info!("saved `{relative}`");
        if is_new_file && self.assets_root().is_some() {
            self.refresh_assets()?;
        }
        Ok(())
    }

    fn save_prefab(&mut self, id: GameObjectId) -> Result<(), EditorError> {
        self.save_prefab_in(id, "prefabs")
    }

    fn save_prefab_in(&mut self, id: GameObjectId, folder: &str) -> Result<(), EditorError> {
        let folder = folder.trim_matches('/');
        let folder = if folder.is_empty() { "prefabs" } else { folder };
        let root = self.require_assets_root("save a prefab")?;
        let scene = &self.app.world.scene;
        let prefab = Prefab::from_object(scene, id).ok_or(SceneError::DeadObject(id))?;
        let stem: String = prefab
            .root
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '_' })
            .collect();
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).map_err(|source| SceneError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let mut relative = format!("{folder}/{stem}.prefab");
        let mut suffix = 1;
        while root.join(&relative).exists() {
            relative = format!("{folder}/{stem}_{suffix}.prefab");
            suffix += 1;
        }
        prefab.save(root.join(&relative))?;
        self.refresh_assets()?;
        self.touch();
        if let Some(obj) = self.app.world.scene.get_mut(id) {
            obj.set_prefab(Some(relative.clone()));
        }
        log::info!("prefab saved to `{relative}`; the object is now its instance");
        Ok(())
    }

    /// Перечитать каталог ассетов с диска (новые файлы получают `.meta`).
    pub(crate) fn refresh_assets(&mut self) -> Result<(), EditorError> {
        let root = self.require_assets_root("refresh assets")?;
        let database = AssetDatabase::open(root)?;
        self.app.world.prefabs = Box::new(database.clone());
        self.app.world.resources.insert(database);
        self.previews.assets_changed(self.app.world.resources.get::<AssetDatabase>());
        self.material_editor.assets_changed();
        self.reindex_assets();
        self.code.scan(&self.config.code_dirs);
        self.viewport.mesh_bounds.clear();
        self.gpu_assets_stale = true;
        Ok(())
    }

    fn reindex_assets(&mut self) {
        let mut assets: Vec<AssetEntry> = match self.app.world.resources.get::<AssetDatabase>() {
            Some(db) => db
                .assets()
                .map(|(guid, path)| AssetEntry {
                    guid,
                    path: path.to_string(),
                    importer: db.meta(guid).map(|m| m.importer.clone()).unwrap_or_default(),
                })
                .collect(),
            None => Vec::new(),
        };
        assets.sort_by(|a, b| a.path.cmp(&b.path));
        self.assets = assets;
        self.project.invalidate();

        // Каталоги — с диска: пустой каталог тоже виден в окне Project.
        fn walk(root: &std::path::Path, dir: &std::path::Path, depth: usize, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() && !name.starts_with('.') {
                    if let Ok(relative) = path.strip_prefix(root) {
                        out.push(relative.to_string_lossy().replace('\\', "/"));
                    }
                    if depth < 8 {
                        walk(root, &path, depth + 1, out);
                    }
                }
            }
        }
        let mut folders = Vec::new();
        if let Some(root) = self.assets_root() {
            walk(&root, &root, 0, &mut folders);
        }
        for asset in &self.assets {
            let mut parts: Vec<&str> = asset.path.split('/').collect();
            parts.pop();
            for end in 1..=parts.len() {
                folders.push(parts[..end].join("/"));
            }
        }
        folders.sort();
        folders.dedup();
        self.folders = folders;
    }

    // ------------------------------------------------------------------
    // Геометрия: границы, наведение камеры, выбор объекта под курсором
    // ------------------------------------------------------------------

    /// Мировые границы объекта: спрайт, если он есть, иначе точка.
    pub(crate) fn bounds_of(&self, id: GameObjectId) -> Option<(Vec2, Vec2)> {
        let scene = &self.app.world.scene;
        scene.get(id)?;
        match sprite_quad(scene, id) {
            Some(quad) => {
                let min = quad.iter().copied().reduce(Vec2::min)?;
                let max = quad.iter().copied().reduce(Vec2::max)?;
                Some((min, max))
            }
            None => {
                let position = scene.world_position(id).truncate();
                Some((position - Vec2::splat(0.5), position + Vec2::splat(0.5)))
            }
        }
    }

    fn frame_rect(&mut self, min: Vec2, max: Vec2) {
        let [w, h] = self.viewport.pixels;
        let aspect = if w > 1 && h > 1 { w as f32 / h as f32 } else { 16.0 / 9.0 };
        let size = (max - min).max(Vec2::splat(0.5));
        self.cam_center = (min + max) * 0.5;
        self.cam_half_height = (size.y * 0.5).max(size.x * 0.5 / aspect) * 1.25;
    }

    /// Вид при открытии сцены: то, что видит камера игры, с запасом по краям.
    /// Без камеры — вся сцена.
    fn frame_initial(&mut self) {
        let scene = &self.app.world.scene;
        let camera = scene
            .ids()
            .into_iter()
            .filter(|id| scene.is_active(*id))
            .filter_map(|id| Some((id, scene.get_component::<Camera>(id)?)))
            .max_by_key(|(_, camera)| camera.priority);
        match camera {
            // Сцена с перспективной камерой открывается в 3D-виде с той же точки.
            Some((id, camera)) if camera.projection == Projection::Perspective => {
                self.scene_3d = true;
                self.orbit = Orbit3D::from_camera(scene.world_matrix(id));
                // Чуть сбоку, выше и дальше: значок игровой камеры виден и не
                // заслоняет то, на что она смотрит.
                self.orbit.yaw += 0.5;
                self.orbit.pitch = (self.orbit.pitch + 0.12).min(1.4);
                self.orbit.distance *= 1.35;
            }
            Some((id, camera)) => {
                self.scene_3d = false;
                self.cam_center = scene.world_position(id).truncate();
                self.cam_half_height = camera.ortho_size.max(0.5) * 1.5;
            }
            None => self.frame_all(),
        }
    }

    /// Показывает ли вид Scene сцену в 3D.
    pub fn is_scene_3d(&self) -> bool {
        self.scene_3d
    }

    /// Переключить вид Scene между плоскостью XY и 3D.
    pub fn set_scene_3d(&mut self, enabled: bool) {
        self.scene_3d = enabled;
        self.viewport.drag = None;
        self.viewport.drag_3d = None;
    }

    /// Мировая точка → точка окна в 3D-виде (`None`, если она позади камеры).
    pub fn world_to_screen_3d(&self, world: Vec3) -> Option<egui::Pos2> {
        crate::view3d::Projector::new(&self.orbit, self.viewport.rect).project(world)
    }

    /// Мировая точка гизмо (Pivot — активный объект, Center — середина выделения).
    pub fn gizmo_pivot_world(&self) -> Option<Vec3> {
        self.gizmo_pivot()
    }

    /// Навести камеру редактора на всю сцену.
    pub(crate) fn frame_all(&mut self) {
        let bounds = self
            .app
            .world
            .scene
            .ids()
            .into_iter()
            .filter_map(|id| self.bounds_of(id))
            .reduce(|(min_a, max_a), (min_b, max_b)| (min_a.min(min_b), max_a.max(max_b)));
        match bounds {
            Some((min, max)) => {
                // Огромный фон не должен «уменьшать» всё остальное до точки.
                let size = (max - min).min(Vec2::new(48.0, 27.0));
                let center = (min + max) * 0.5;
                self.frame_rect(center - size * 0.5, center + size * 0.5);
            }
            None => {
                self.cam_center = Vec2::ZERO;
                self.cam_half_height = 6.0;
            }
        }
    }

    /// Объект под мировой точкой: верхний по порядку отрисовки спрайт, а если
    /// спрайта нет — ближайший «пустой» объект в радиусе `radius`.
    pub(crate) fn pick(&self, point: Vec2, radius: f32) -> Option<GameObjectId> {
        let scene = &self.app.world.scene;
        let mut best_sprite: Option<((i32, i32, f32, usize), GameObjectId)> = None;
        let mut best_marker: Option<(f32, GameObjectId)> = None;
        for (index, id) in scene.ids().into_iter().enumerate() {
            // Запертые замком иерархии (с детьми) щелчком не выбираются.
            if !scene.is_active(id) || self.pick_locked(id) {
                continue;
            }
            match scene.get_component::<SpriteRenderer>(id) {
                Some(sprite) => {
                    let matrix = scene.world_matrix(id);
                    if matrix.determinant().abs() < 1e-12 {
                        continue;
                    }
                    let local = matrix.inverse().transform_point3(point.extend(0.0));
                    let half = sprite.size * 0.5;
                    if local.x.abs() <= half.x && local.y.abs() <= half.y {
                        let key = (sprite.sorting_layer, sprite.order, matrix.w_axis.z, index);
                        if best_sprite.as_ref().is_none_or(|(best, _)| key >= *best) {
                            best_sprite = Some((key, id));
                        }
                    }
                }
                None => {
                    let distance = scene.world_position(id).truncate().distance(point);
                    if distance <= radius && best_marker.is_none_or(|(best, _)| distance < best) {
                        best_marker = Some((distance, id));
                    }
                }
            }
        }
        // Маркер «пустого» объекта маленький, поэтому точное попадание в него важнее спрайта под ним.
        best_marker.map(|(_, id)| id).or(best_sprite.map(|(_, id)| id))
    }

    /// Мировая точка → точка окна (вид Scene). Нужна вьюпорту и тестам.
    pub fn world_to_screen(&self, world: Vec2) -> egui::Pos2 {
        self.view_transform().to_screen(world)
    }

    pub(crate) fn view_transform(&self) -> crate::viewport::ViewTransform {
        crate::viewport::ViewTransform {
            rect: self.viewport.rect,
            center: self.cam_center,
            half_height: self.cam_half_height,
        }
    }

    /// Прямоугольник вьюпорта в координатах окна (в точках egui).
    pub fn viewport_rect(&self) -> egui::Rect {
        // Scene и Game — разные окна; «вьюпорт» — тот вид, что выбран последним.
        if self.view_tab == ViewTab::Game {
            self.dock.game.rect
        } else {
            self.viewport.rect
        }
    }

    /// Идентификатор строки объекта в окне Hierarchy — для автоматизации UI.
    pub fn hierarchy_row_id(id: GameObjectId) -> egui::Id {
        egui::Id::new(("hierarchy_row", id))
    }

    /// Окно Project показывает ассеты списком (`true`) или плитками с миниатюрами.
    pub fn set_project_list_view(&mut self, list: bool) {
        self.prefs.project_ui.grid = !list;
    }

    /// Размер плитки окна Project в точках (бегунок рядом с поиском: 40–160).
    pub fn set_project_tile_size(&mut self, size: f32) {
        self.prefs.project_ui.tile = size.clamp(40.0, 160.0);
    }

    /// Всё, что выделено в окне Project (пути каталогов и ассетов, по алфавиту).
    pub fn project_selection_all(&self) -> Vec<String> {
        self.project.selection.iter().cloned().collect()
    }

    /// Выделить в окне Project несколько элементов (пути относительно каталога ассетов).
    pub fn select_project_items(&mut self, paths: &[&str]) {
        self.project.select_many(paths.iter().map(|p| p.to_string()));
    }

    /// Сообщения (тосты) окна Project, которые сейчас показаны.
    pub fn project_toasts(&self) -> Vec<String> {
        self.project.toasts.iter().map(|t| t.text.clone()).collect()
    }

    /// Идёт ли копирование файлов, брошенных на окно Project.
    pub fn project_import_active(&self) -> bool {
        self.project.import.is_some()
    }

    /// Можно ли в окне Project вернуться назад / вперёд по истории.
    pub fn project_history(&self) -> (bool, bool) {
        (self.project.history.can_back(), self.project.history.can_forward())
    }

    /// Идёт ли переименование на месте; текст в поле.
    pub fn project_rename_text(&self) -> Option<&str> {
        self.project.rename.as_ref().map(|r| r.text.as_str())
    }

    /// Ждёт ли окно Project подтверждения удаления насовсем.
    pub fn project_confirm_pending(&self) -> bool {
        self.project.confirm.is_some()
    }

    /// Открыт ли встроенный браузер файлов «Import New Asset…».
    pub fn project_browser_open(&self) -> bool {
        self.project.browser.is_some()
    }

    /// Открыть браузер файлов «Import New Asset…» (в каталог окна Project).
    pub fn show_import_browser(&mut self) {
        let dest = self.project.folder.clone();
        self.project.browser = Some(crate::project_chrome::Browser::new(dest));
    }

    /// Что лежит во внутреннем буфере обмена окна Project: пути и «вырезано» ли.
    pub fn project_clipboard(&self) -> Option<(Vec<String>, bool)> {
        self.project.clipboard.as_ref().map(|c| (c.paths.clone(), c.cut))
    }

    /// Искать во всех ассетах (`true`) или только в открытом каталоге (`false`).
    pub fn set_project_search_scope(&mut self, all: bool) {
        self.prefs.project_ui.scope = if all { "all" } else { "folder" }.to_string();
    }

    /// То же, что кнопка-звезда у поиска: запомнить текущий запрос в «Favorites».
    pub fn save_project_search(&mut self) {
        let query = self.project.filter.trim().to_string();
        let known = crate::project_nav::DEFAULT_SEARCHES.iter().any(|(_, q)| *q == query)
            || self.prefs.project_ui.searches.iter().any(|(_, q)| *q == query);
        if !query.is_empty() && !known {
            self.prefs.project_ui.searches.push((query.clone(), query));
        }
    }

    /// Идентификатор границы столбца в заголовке списка (`type`, `modified`, `size`).
    pub fn project_column_grip_id(key: &str) -> egui::Id {
        egui::Id::new(("project_column_grip", key.to_string()))
    }

    /// Идентификатор фона строки пути: щелчок по нему открывает поле пути.
    pub fn project_path_bar_id() -> egui::Id {
        egui::Id::new("project_path_background")
    }

    /// Идентификатор строки-заголовка «Favorites» в дереве.
    pub fn project_favorites_header_id() -> egui::Id {
        egui::Id::new(("project_tree_header", "\u{3}favorites"))
    }

    /// Сколько раз база ассетов была перечитана с диска: одна операция над несколькими
    /// файлами перечитывает её один раз.
    pub fn project_scan_count(&self) -> u64 {
        self.project.rev
    }

    /// Сколько элементов окно Project нарисовало в последнем кадре (остальные не рисуются).
    pub fn project_painted_count(&self) -> usize {
        self.project.entry_rects.len()
    }

    /// Текущий запрос поиска окна Project.
    pub fn project_search(&self) -> &str {
        &self.project.filter
    }

    /// Левая и правая части строки состояния окна Project.
    pub fn project_status(&self) -> (String, String) {
        crate::project_chrome::status_text(self)
    }

    /// Положение прямоугольника, который окно Project заняло в последнем кадре.
    pub fn project_panel_rect(&self) -> Option<egui::Rect> {
        self.project.panel_rect
    }

    /// Идентификатор строки пути (хлебных крошек) окна Project.
    pub fn project_crumb_id(path: &str) -> egui::Id {
        egui::Id::new(("project_crumb", path.to_string()))
    }

    /// Идентификатор поля переименования на месте.
    pub fn project_rename_field_id(path: &str) -> egui::Id {
        egui::Id::new(("project_rename_field", path.to_string()))
    }

    /// Идентификатор поля пути.
    pub fn project_path_field_id() -> egui::Id {
        egui::Id::new("project_path_field")
    }

    /// Идентификатор строки в «Favorites»: закреплённый каталог.
    pub fn project_favorite_id(path: &str) -> egui::Id {
        egui::Id::new(("project_fav", path.to_string()))
    }

    /// Идентификатор строки сохранённого поиска в «Favorites».
    pub fn project_saved_search_id(query: &str) -> egui::Id {
        egui::Id::new(("project_search", query.to_string()))
    }

    /// Счётчики предпросмотра моделей: сколько миниатюр нарисовано, сколько не удалось.
    pub fn preview_stats(&self) -> crate::preview::PreviewStats {
        self.previews.stats
    }

    /// Где в окне Project нарисована плитка (строка) ассета — в точках интерфейса, из
    /// последнего кадра. `None` — ассет сейчас не виден.
    pub fn project_entry_rect(&self, relative_path: &str) -> Option<egui::Rect> {
        self.project.entry_rects.get(relative_path).copied()
    }

    /// Где нарисован большой предпросмотр модели в панели сведений окна Project.
    pub fn project_preview_rect(&self) -> Option<egui::Rect> {
        self.project.preview_rect
    }

    /// Каталог, открытый в окне Project (путь относительно каталога ассетов; корень — пусто).
    pub fn project_folder(&self) -> &str {
        &self.project.folder
    }

    /// Что выделено в окне Project: путь каталога или ассета.
    pub fn project_selection(&self) -> Option<&str> {
        self.project.selected_path.as_deref()
    }

    /// Идентификатор строки каталога в дереве окна Project (корень — пустая строка).
    pub fn project_tree_row_id(folder: &str) -> egui::Id {
        crate::project::tree_row_id(folder)
    }

    /// Идентификатор плитки (строки) каталога в содержимом окна Project.
    pub fn project_folder_tile_id(folder: &str) -> egui::Id {
        crate::project::folder_entry_id(folder)
    }

    /// Идентификатор строки ассета в окне Project (путь относительно каталога ассетов).
    pub fn project_row_id(&self, relative_path: &str) -> Option<egui::Id> {
        self.assets
            .iter()
            .find(|asset| asset.path == relative_path)
            .map(|asset| crate::project::asset_row_id(asset.guid))
    }

    /// Где в окне Inspector сейчас виджет поля `component.field` выделенного объекта.
    pub fn inspector_field_rect(&self, component: &str, field: &str) -> Option<egui::Rect> {
        self.inspector
            .field_rects
            .iter()
            .find(|((kind, name), _)| *kind == component && *name == field)
            .map(|(_, rect)| *rect)
    }

    /// Идентификатор области вьюпорта.
    pub fn viewport_id() -> egui::Id {
        egui::Id::new("editor_viewport")
    }
}

/// Показать файл в файловом менеджере системы.
pub(crate) fn reveal(path: &std::path::Path) {
    // Общая реализация (engine-build): `explorer /select,"C:\…"` одним аргументом — путь с
    // пробелами и `/` больше не уводит проводник на рабочий стол.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    let result = engine_build::host::reveal_in_file_manager(path);
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    let result: std::io::Result<()> = Err(std::io::Error::other("this platform has no file manager"));
    if let Err(err) = result {
        log::warn!("cannot show `{}` in the file manager: {err}", path.display());
    }
}

/// Шаблон нового скрипта на C++.
const SCRIPT_TEMPLATE: &str = r#"// {Name} — игровой скрипт. Он вешается на объект так же, как Light или MeshRenderer:
// в инспекторе — Add Component → {Name}.
//
// Хуки (переопределяйте только нужные): on_start, on_update, on_fixed_update,
// on_late_update, on_message, on_collision_enter, on_trigger_enter, on_destroy.
// Весь API (ввод, объекты, таймеры, физика, звук, интерфейс) — docs/api/cpp.md.

#include "engine_component.hpp"

struct {Name} : engine::Component {
    // Поля из ENGINE_FIELDS видны в инспекторе и сохраняются в сцену.
    float speed = 1.0f;

    void on_start(engine::Object self) override {
        (void)self;
    }

    void on_update(engine::Object self, float dt) override {
        // Пример: медленно вращать объект вокруг вертикальной оси.
        auto rotation = self.rotation();
        rotation.y += speed * 30.0f * dt;
        self.set_rotation(rotation);
    }
};
ENGINE_FIELDS({Name}, ENGINE_FIELD({Name}, speed, ENGINE_FIELD_F32));
ENGINE_COMPONENT({Name});
"#;

/// Мировые углы спрайта объекта (против часовой стрелки, от левого нижнего).
pub(crate) fn sprite_quad(scene: &Scene, id: GameObjectId) -> Option<[Vec2; 4]> {
    let sprite = scene.get_component::<SpriteRenderer>(id)?;
    let matrix = scene.world_matrix(id);
    let half = sprite.size * 0.5;
    Some(
        [
            (-half.x, -half.y),
            (half.x, -half.y),
            (half.x, half.y),
            (-half.x, half.y),
        ]
        .map(|(x, y)| matrix.transform_point3(Vec3::new(x, y, 0.0)).truncate()),
    )
}

/// Путь до объекта индексами среди «братьев» — переживает пересоздание сцены.
pub(crate) fn path_of(scene: &Scene, id: GameObjectId) -> Option<Vec<usize>> {
    let mut path = Vec::new();
    let mut current = id;
    loop {
        let obj = scene.get(current)?;
        match obj.parent() {
            Some(parent) => {
                let index = scene
                    .get(parent)?
                    .children()
                    .iter()
                    .position(|c| *c == current)?;
                path.push(index);
                current = parent;
            }
            None => {
                path.push(scene.roots().iter().position(|r| *r == current)?);
                break;
            }
        }
    }
    path.reverse();
    Some(path)
}

pub(crate) fn id_at(scene: &Scene, path: &[usize]) -> Option<GameObjectId> {
    let (first, rest) = path.split_first()?;
    let mut id = *scene.roots().get(*first)?;
    for index in rest {
        id = *scene.get(id)?.children().get(*index)?;
    }
    Some(id)
}

/// Имя, которого ещё нет среди детей `parent`: `Coin`, `Coin (1)`, `Coin (2)`…
pub(crate) fn unique_name(
    scene: &Scene,
    parent: Option<GameObjectId>,
    base: &str,
    ignore: Option<GameObjectId>,
) -> String {
    let siblings: Vec<GameObjectId> = match parent {
        Some(parent) => scene.get(parent).map(|p| p.children().to_vec()).unwrap_or_default(),
        None => scene.roots().to_vec(),
    };
    let taken = |name: &str| {
        siblings
            .iter()
            .filter(|id| Some(**id) != ignore)
            .any(|id| scene.get(*id).is_some_and(|o| o.name == name))
    };
    if !taken(base) {
        return base.to_string();
    }
    // `Coin (1)` → основа `Coin`.
    let stem = match base.rfind(" (") {
        Some(index) if base.ends_with(')') && base[index + 2..base.len() - 1].parse::<u32>().is_ok() => {
            &base[..index]
        }
        _ => base,
    };
    (1..)
        .map(|n| format!("{stem} ({n})"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| base.to_string())
}

pub(crate) fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name)
}

pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Области рамки окна в последнем кадре: главная панель инструментов и группа
/// Play / Pause / Step (проверки выравнивания, `Editor::chrome_rects`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeRects {
    pub toolbar: egui::Rect,
    pub transport: egui::Rect,
}

impl Default for ChromeRects {
    fn default() -> Self {
        Self { toolbar: egui::Rect::NOTHING, transport: egui::Rect::NOTHING }
    }
}

impl Editor {
    /// Где в последнем кадре стояли главная панель и группа Play / Pause / Step.
    pub fn chrome_rects(&self) -> ChromeRects {
        self.chrome
    }

    /// Насколько содержимое инспектора в последнем кадре вышло за ширину окна (0 — нет).
    pub fn inspector_overflow(&self) -> f32 {
        self.inspector.overflow
    }

    /// Где в последнем кадре нарисованы виджеты значений инспектора: (компонент, поле).
    pub fn inspector_field_rects(&self) -> Vec<((&'static str, &'static str), egui::Rect)> {
        self.inspector.field_rects.iter().map(|(k, v)| (*k, *v)).collect()
    }
}
