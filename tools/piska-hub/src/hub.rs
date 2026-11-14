//! Состояние хаба и всё, что он делает. Интерфейс (`ui.rs`) только складывает
//! [`HubAction`]; здесь они выполняются — поэтому хаб проверяется без окна.

use engine_editor::kit::i18n::{tr_s, trf, trn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use engine_editor::kit::theme;
use engine_editor::Preferences;

use crate::config::{now_unix, AfterOpen, HubConfig, SortOrder, ViewMode};
use crate::jobs::{self, Job, JobKind, JobState, Processes};
use crate::link::{self, Request};
use crate::project::{self, Listed, ProjectError, ProjectInfo, Source};
use crate::samples::{self, Doc};
use crate::templates::{self, NewProject, Template};
use crate::{paths, platform};

/// Страница левой навигации.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Projects,
    NewProject,
    Samples,
    /// Программы движка: Piska Modeler, Scenario, экспорт, руководства.
    Tools,
    Settings,
    /// Импорт проектов: из каталога, из файла проекта, сразу несколько.
    Import,
}

/// Модальное окно.
#[derive(Clone, Debug)]
pub enum Dialog {
    /// Добавить существующий проект или найти потерянный (`locate` — его прежний путь).
    Add {
        locate: Option<PathBuf>,
        browser: FolderBrowser,
    },
    /// Удалить проект с диска; `verdict` — можно ли (проверено при открытии окна).
    Delete { path: PathBuf, title: String, verdict: Result<(), String> },
    /// Экспорт через piska-export.
    Export {
        path: PathBuf,
        title: String,
        platform: &'static str,
        release: bool,
    },
    /// Выбрать каталог для мастера или настроек.
    Browse { target: BrowseTarget, browser: FolderBrowser },
    /// Сохранить проект в файл.
    Pack(Box<crate::import_flow::PackDialog>),
}

/// Куда пойдёт выбранный каталог.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseTarget {
    WizardLocation,
    DefaultLocation,
    /// Каталог с проектом или с проектами для импорта.
    ImportFolder,
    /// Файл проекта для импорта.
    ImportFile,
    /// Куда импортировать.
    ImportLocation,
    /// Куда сохранить файл проекта.
    PackFolder,
}

/// Встроенный обзор каталогов: путь, подкаталоги, отметка проектов Piska.
#[derive(Clone, Debug)]
pub struct FolderBrowser {
    /// Текст поля пути (его можно править руками).
    pub path: String,
    /// Подкаталоги показанного каталога: (имя, путь, это проект Piska).
    pub entries: Vec<(String, PathBuf, bool)>,
    /// Файлы показанного каталога с нужным расширением (если `extension` задано).
    pub files: Vec<(String, PathBuf)>,
    /// Показывать файлы с этим расширением (без точки).
    pub extension: Option<&'static str>,
    /// Каталог, для которого собраны `entries`.
    pub listed: Option<PathBuf>,
    /// Последняя проверка пути как проекта: (путь, результат).
    checked: Option<(String, Result<ProjectInfo, ProjectError>)>,
}

impl FolderBrowser {
    /// Обзор, который показывает ещё и файлы с расширением `extension`.
    pub fn with_files(start: &Path, extension: &'static str) -> Self {
        let mut browser = Self::new(start);
        browser.extension = Some(extension);
        browser.listed = None;
        browser.refresh();
        browser
    }

    pub fn new(start: &Path) -> Self {
        let mut browser = Self {
            path: start.display().to_string(),
            entries: Vec::new(),
            files: Vec::new(),
            extension: None,
            listed: None,
            checked: None,
        };
        browser.refresh();
        browser
    }

    /// Перечитать подкаталоги, если путь в поле изменился.
    pub fn refresh(&mut self) {
        let dir = PathBuf::from(self.path.trim());
        if self.listed.as_ref() == Some(&dir) {
            return;
        }
        self.listed = Some(dir.clone());
        self.entries.clear();
        self.files.clear();
        if self.path.trim().is_empty() {
            self.entries = drives();
            return;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut found: Vec<(String, PathBuf, bool)> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                let project = path.join("Cargo.toml").is_file() && path.join("src").join("lib.rs").is_file();
                (name, path, project)
            })
            .filter(|(name, _, _)| !name.starts_with('.') && !name.starts_with('$'))
            .collect();
        found.sort_by_key(|(name, _, _)| name.to_lowercase());
        self.entries = found;
        if let (Some(extension), Ok(entries)) = (self.extension, std::fs::read_dir(&dir)) {
            let mut files: Vec<(String, PathBuf)> = entries
                .flatten()
                .filter(|e| e.path().is_file())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case(extension)))
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .collect();
            files.sort_by_key(|(name, _)| name.to_lowercase());
            self.files = files;
        }
    }

    /// Проект ли каталог из поля пути (проверка повторяется, только когда путь изменился).
    pub fn check(&mut self) -> Result<ProjectInfo, ProjectError> {
        let path = self.path.trim().to_string();
        if let Some((checked, result)) = &self.checked {
            if *checked == path {
                return result.clone();
            }
        }
        let result = project::inspect(Path::new(&path));
        self.checked = Some((path, result.clone()));
        result
    }

    pub fn enter(&mut self, dir: &Path) {
        self.path = dir.display().to_string();
        self.refresh();
    }

    /// На уровень выше (из корня диска — к списку дисков).
    pub fn up(&mut self) {
        let current = PathBuf::from(self.path.trim());
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => self.enter(parent),
            _ => {
                self.path.clear();
                self.refresh();
            }
        }
    }
}

/// Диски Windows (`C:\`, `D:\`) или корень `/`.
fn drives() -> Vec<(String, PathBuf, bool)> {
    if cfg!(windows) {
        (b'A'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\", letter as char)))
            .filter(|path| path.is_dir())
            .map(|path| (path.display().to_string(), path, false))
            .collect()
    } else {
        vec![("/".to_string(), PathBuf::from("/"), false)]
    }
}

/// Мастер нового проекта.
#[derive(Clone, Debug)]
pub struct Wizard {
    pub name: String,
    pub location: String,
    pub template: Template,
    pub cpp: bool,
    /// Только что созданный проект (экран «готово»).
    pub created: Option<PathBuf>,
    pub error: Option<String>,
}

/// Что получится из полей мастера: каталог, внутри ли `games/` движка, файлы — или
/// почему проект не создать.
pub type WizardPlan = Result<(PathBuf, bool, Vec<String>), String>;

/// Что хаб просит у окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowRequest {
    pub visible: bool,
    /// Вывести окно на передний план (один раз).
    pub focus: bool,
    pub quit: bool,
}

/// Действие из интерфейса (и из тестов).
#[derive(Clone, Debug, PartialEq)]
pub enum HubAction {
    Navigate(Page),
    Select(Option<PathBuf>),
    Open(PathBuf),
    Play(PathBuf),
    ShowExport(PathBuf),
    Export { path: PathBuf, platform: String, release: bool },
    Reveal(PathBuf),
    TogglePin(PathBuf),
    Remove(PathBuf),
    AskDelete(PathBuf),
    Delete(PathBuf),
    ShowAdd { locate: Option<PathBuf> },
    Add(PathBuf),
    Locate { old: PathBuf, new: PathBuf },
    Browse(BrowseTarget),
    SetBrowsed { target: BrowseTarget, path: PathBuf },
    CreateProject,
    CancelJob,
    CloseJob,
    RetryJob,
    OpenFile(PathBuf),
    SetView(ViewMode),
    SetSort(SortOrder),
    SetAfterOpen(AfterOpen),
    SetDefaultLocation(Option<PathBuf>),
    SetUiScale(f32),
    SetAccent(usize),
    /// Язык интерфейса (`None` — как в системе). Общая настройка с редактором.
    SetLanguage(Option<engine_editor::kit::i18n::Language>),
    RestoreHidden,
    Refresh,
    CloseDialog,
    /// Собрать заново и открыть, даже если программа редактора свежая.
    RebuildAndOpen(PathBuf),
    /// Собрать заново и запустить игру.
    RebuildAndPlay(PathBuf),
    /// Открыть программу редактора прошлой сборки, не собирая (идущая сборка отменяется).
    OpenLastBuild(PathBuf),
    /// Запустить программу игры прошлой сборки, не собирая.
    PlayLastBuild(PathBuf),
    /// Собрать редакторы всех проектов списка по очереди (без запуска).
    PrebuildAll,
    /// Остановить «Prebuild all».
    CancelPrebuild,
    /// Создать проект по мастеру и сразу открыть.
    CreateAndOpen,
    /// Показать / спрятать подробное окно хода сборки (сборка идёт в фоне в любом случае).
    ShowJob(bool),
    /// Убрать законченную сборку из полосы задач.
    DismissJob,
    /// Piska Modeler: пустой или с файлом `.pmodel`.
    OpenModeler(Option<PathBuf>),
    /// Открыть страницу импорта.
    ShowImport,
    /// Добавить в импорт каталог с проектом (или проектами) либо файл проекта.
    ImportAdd(PathBuf),
    /// Найти проекты в каталоге и добавить их.
    ImportScan(PathBuf),
    /// Убрать источник из списка импорта.
    ImportRemove(usize),
    /// Запустить импорт отмеченных проектов.
    ImportRun,
    /// Остановить идущий импорт.
    ImportCancel,
    /// Очистить страницу импорта (после результата).
    ImportReset,
    /// Окно «Save Project As File…».
    ShowPack(PathBuf),
    /// Начать сохранение по окну (путь и части берутся из него).
    StartPack(Box<crate::import_flow::PackDialog>),
    /// Остановить фоновую задачу (сохранение, импорт, тесты).
    CancelWork(u64),
    /// Убрать законченную фоновую задачу из полосы.
    DismissWork(u64),
}

impl ToolStart {
    /// Общий редактор с аргументами.
    pub fn editor(args: Vec<String>) -> Self {
        Self {
            package: "piska-editor".into(),
            bin: "piska-editor".into(),
            args,
        }
    }
}

/// Запуск программы движка после её сборки.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolStart {
    /// Пакет (`piska-editor`, `piska-modeler`).
    pub package: String,
    /// Программа (`piska-editor`, `piska-player`, `piska-modeler`).
    pub bin: String,
    pub args: Vec<String>,
}

/// Сборка, ждущая своей очереди.
#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    pub path: PathBuf,
    pub kind: JobKind,
    pub launch: bool,
    /// Собирается программа движка, а не проект.
    pub tool: Option<ToolStart>,
}

/// Как запускать: решить самому, всегда собирать или взять прошлую сборку.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartMode {
    /// Свежая программа — сразу; изменился проект или программы нет — собрать.
    Auto,
    Rebuild,
    LastBuild,
}

/// Что сейчас со сборкой — простыми словами для окна хода.
#[derive(Clone, Debug, PartialEq)]
pub enum Diagnosis {
    /// cargo ждёт, пока другая сборка освободит каталог сборки или кэш пакетов.
    WaitingForLock { what: String, seconds: u64 },
    /// Ошибка в коде движка (не в проекте): движок сейчас не собирается.
    EngineBroken { package: String, error: crate::cargo::Diagnostic },
    /// Ошибка в коде проекта.
    ProjectError { error: crate::cargo::Diagnostic },
}

/// «Prebuild all»: редакторы проектов по очереди.
#[derive(Clone, Debug, Default)]
pub struct Prebuild {
    /// Очередь: (каталог, имя).
    pub queue: Vec<(PathBuf, String)>,
    /// Сколько уже начато.
    pub index: usize,
    /// Результаты: (имя, удалось, пояснение).
    pub results: Vec<(String, bool, String)>,
    /// Свежих, которые пропущены.
    pub skipped: usize,
    pub finished: bool,
}

/// Результат фонового обхода: список проектов и документация.
type Scan = (Vec<Listed>, Vec<Doc>);

/// Картинки, декодированные в фоне: (ключ текстуры, картинка или `None`).
type DecodedImage = (String, Option<egui::ColorImage>);
pub(crate) type ImageChannel = (std::sync::mpsc::Sender<DecodedImage>, std::sync::mpsc::Receiver<DecodedImage>);

/// Как создать хаб.
#[derive(Clone, Debug, Default)]
pub struct HubOptions {
    /// Корень движка; `None` — найти ([`paths::find_engine_root`]).
    pub engine_root: Option<PathBuf>,
    /// Каталог настроек; `None` — [`paths::config_dir`].
    pub config_dir: Option<PathBuf>,
    /// Ничего не запускать и не открывать: команды записываются в [`Hub::dry_log`].
    /// Для тестов и снимков экрана.
    pub dry_run: bool,
    /// Сразу открыть этот проект (`--open <путь>`).
    pub open: Option<PathBuf>,
    /// Где искать программы движка (`piska-editor`, `piska-player`, `piska-modeler`);
    /// `None` — рядом с хабом и в каталоге сборки. Тесты подставляют свой каталог.
    pub tools_dir: Option<PathBuf>,
    /// Каталоги и файлы проектов для импорта: открыть страницу импорта с ними
    /// (`piska-hub <файл>`, двойной щелчок по файлу проекта).
    pub import: Vec<PathBuf>,
}

/// Сообщение в строке состояния.
#[derive(Clone, Debug)]
pub struct Status {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// Piska Hub.
pub struct Hub {
    pub(crate) engine_root: Option<PathBuf>,
    pub(crate) config_dir: Option<PathBuf>,
    pub config: HubConfig,
    pub(crate) listed: Vec<Listed>,
    pub(crate) docs: Vec<Doc>,
    pub(crate) page: Page,
    pub(crate) search: String,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) wizard: Wizard,
    pub(crate) job: Option<Job>,
    pub(crate) processes: Processes,
    pub(crate) dry_run: bool,
    /// Что хаб запустил бы (в режиме `dry_run`).
    pub dry_log: Vec<String>,
    pub(crate) status: Option<Status>,
    pub(crate) textures: HashMap<String, Option<egui::TextureHandle>>,
    pub(crate) prefs: Preferences,
    pub(crate) actions: Vec<HubAction>,
    /// Идентификатор поля поиска в последнем кадре (для тестов).
    pub(crate) search_id: Option<egui::Id>,
    /// План мастера для текущих полей: (поля, каталог / в рабочем пространстве / файлы или ошибка).
    pub(crate) wizard_plan: Option<(NewProject, WizardPlan)>,
    /// Ключ выделенного проекта ([`paths::key`]).
    pub(crate) selected_key: Option<String>,
    /// Идёт фоновый обход проектов; результат придёт сюда.
    refresh_rx: Option<std::sync::mpsc::Receiver<Scan>>,
    /// Пока шёл обход, попросили ещё один.
    refresh_again: bool,
    pub(crate) prebuild: Option<Prebuild>,
    /// Очередь сборок: идут по одной, хаб тем временем работает.
    pub(crate) queue: std::collections::VecDeque<Queued>,
    /// Подробное окно хода сборки открыто.
    pub(crate) show_job: bool,
    /// Страница импорта проектов.
    pub(crate) import: crate::import_flow::ImportPage,
    /// Фоновые задачи: сохранение проекта в файл, импорт, тесты.
    pub(crate) works: Vec<crate::tasks::Work>,
    pub(crate) work_counter: u64,
    /// Проект, который надо открыть, когда импорт закончится.
    pub(crate) pending_open_after_import: Option<PathBuf>,
    /// Расчёт «сколько войдёт в файл» для окна сохранения.
    pub(crate) pack_estimate: crate::import_flow::EstimateChannel,
    /// Файлы и каталоги, которые надо добавить в импорт при запуске (`piska-hub <файл>`).
    pending_imports: Vec<PathBuf>,
    /// Окно сохранения, скрытое на время выбора каталога.
    pub(crate) pack_return: Option<Box<crate::import_flow::PackDialog>>,
    /// Поле имени мастера ждёт фокуса (страница только что открылась).
    pub(crate) focus_wizard_name: bool,
    tools_dir: Option<PathBuf>,
    /// Картинки, декодированные в фоне: (ключ, картинка).
    pub(crate) images: ImageChannel,
    pub(crate) images_loading: std::collections::HashSet<String>,
    /// Что-то поменялось с прошлого кадра (окно перерисуется).
    pub(crate) changed: bool,
    window: WindowRequest,
    /// Окно спрятано, пока открыт редактор.
    hidden_for_editor: bool,
    /// После закрытия редактора — выйти (настройка «Close hub»).
    quit_when_done: bool,
    pending_open: Option<PathBuf>,
    last_alive: Option<Instant>,
    last_request_check: Instant,
    /// «Сейчас» для подписей времени (тесты подставляют своё).
    pub(crate) now: Option<u64>,
    /// Заставка оконного запуска (`None` — без неё: тесты, снимки).
    pub(crate) splash: Option<engine_editor::kit::window::LoadingScreen>,
    /// Нарисован хотя бы один полный кадр в цветах темы (окно можно показывать).
    pub(crate) themed: bool,
    /// Открытое системное окно выбора папки / файла и куда пойдёт ответ.
    native_browse: Option<(BrowseTarget, engine_build::file_dialog::Dialog)>,
    /// Системного окна нет (не открылось): дальше — встроенный обзор.
    native_browse_failed: bool,
}

impl Hub {
    pub fn new(options: HubOptions) -> Self {
        let engine_root = options.engine_root.or_else(paths::find_engine_root);
        let config_dir = options.config_dir.or_else(paths::config_dir);
        let (config, problem) = match &config_dir {
            Some(dir) => HubConfig::load(&dir.join(paths::CONFIG_FILE)),
            None => (HubConfig::default(), None),
        };
        let prefs = if options.dry_run { Preferences::default() } else { Preferences::load() };
        if let Some((_, color)) = theme::ACCENTS.get(prefs.accent) {
            theme::set_accent(*color);
        }
        let template = config
            .settings
            .last_template
            .as_deref()
            .and_then(|t| Template::ALL.into_iter().find(|x| x.title() == t))
            .unwrap_or(Template::Empty3D);
        let location = config
            .settings
            .last_location
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| config.settings.default_location.clone())
            .or_else(|| engine_root.as_ref().map(|root| root.join("games")))
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut hub = Self {
            docs: Vec::new(),
            engine_root,
            config_dir,
            config,
            listed: Vec::new(),
            page: Page::Projects,
            search: String::new(),
            selected: None,
            dialog: None,
            wizard: Wizard {
                name: String::new(),
                location,
                template,
                cpp: false,
                created: None,
                error: None,
            },
            job: None,
            processes: Processes::default(),
            dry_run: options.dry_run,
            dry_log: Vec::new(),
            status: None,
            textures: HashMap::new(),
            prefs,
            actions: Vec::new(),
            search_id: None,
            wizard_plan: None,
            selected_key: None,
            refresh_rx: None,
            refresh_again: false,
            prebuild: None,
            queue: std::collections::VecDeque::new(),
            show_job: false,
            import: crate::import_flow::ImportPage::new(PathBuf::new()),
            works: Vec::new(),
            work_counter: 0,
            pending_open_after_import: None,
            pack_estimate: std::sync::mpsc::channel(),
            pending_imports: options.import,
            pack_return: None,
            focus_wizard_name: false,
            tools_dir: options.tools_dir,
            images: std::sync::mpsc::channel(),
            images_loading: std::collections::HashSet::new(),
            changed: true,
            window: WindowRequest {
                visible: true,
                focus: false,
                quit: false,
            },
            hidden_for_editor: false,
            quit_when_done: false,
            pending_open: options.open,
            last_alive: None,
            last_request_check: Instant::now(),
            now: None,
            splash: None,
            themed: false,
            native_browse: None,
            native_browse_failed: false,
        };
        if hub.engine_root.is_none() {
            hub.set_status(
                tr_s("The engine checkout was not found: set PISKA_ENGINE_ROOT or start the hub from the engine folder"),
                true,
            );
        }
        if let Some(problem) = problem {
            hub.set_status(problem, true);
        }
        hub.refresh();
        let import_location = hub
            .config
            .settings
            .last_import_location
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| hub.config.settings.default_location.clone())
            .or_else(|| hub.engine_root.as_ref().map(|root| root.join("games")))
            .unwrap_or_default();
        hub.import = crate::import_flow::ImportPage::new(import_location);
        for path in std::mem::take(&mut hub.pending_imports) {
            hub.apply(HubAction::ImportAdd(path));
        }
        hub
    }

    // -----------------------------------------------------------------------
    // Чтение состояния
    // -----------------------------------------------------------------------

    /// Ответ системного окна выбора: путь — как из встроенного обзора, «Отмена» — ничего,
    /// окна нет — встроенный обзор.
    fn poll_native_browse(&mut self) {
        let Some((target, dialog)) = &self.native_browse else { return };
        let Some(outcome) = dialog.poll() else { return };
        let target = *target;
        self.native_browse = None;
        self.changed = true;
        match outcome {
            engine_build::file_dialog::Outcome::Chosen(path) => match target {
                // Окно «Сохранить в файл» остаётся открытым: меняется только папка.
                BrowseTarget::PackFolder => {
                    if let Some(Dialog::Pack(pack)) = &mut self.dialog {
                        let name = Path::new(pack.out.trim()).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty());
                        let name = name.unwrap_or_else(|| engine_editor::kit::i18n::brand::project_file_name(&pack.package));
                        pack.out = path.join(name).display().to_string();
                    }
                }
                _ => self.apply(HubAction::SetBrowsed { target, path }),
            },
            engine_build::file_dialog::Outcome::Cancelled => {}
            engine_build::file_dialog::Outcome::Unavailable => {
                self.native_browse_failed = true;
                self.apply(HubAction::Browse(target));
            }
        }
    }

    /// Системное окно выбора открыто (ждём ответа).
    pub fn native_browse_open(&self) -> bool {
        self.native_browse.is_some()
    }

    /// Показывать заставку загрузки, пока идёт первый обход проектов (оконный запуск).
    pub fn enable_splash(&mut self) {
        self.splash = Some(Default::default());
    }

    /// Полный кадр в цветах темы уже нарисован: окно (созданное невидимым) можно показать
    /// без белого или серого кадра системы.
    pub fn themed(&self) -> bool {
        self.themed
    }

    /// Заставка ещё на экране.
    pub fn splash_visible(&self) -> bool {
        self.splash.as_ref().is_some_and(|s| s.visible())
    }

    /// Что хаб делает сейчас при запуске (`None` — готов): то, что пишет заставка.
    pub fn startup_step(&self) -> Option<String> {
        if self.refreshing() {
            return Some(tr_s("Scanning projects and checking their editors…").to_string());
        }
        if !self.images_loading.is_empty() {
            return Some(engine_editor::kit::i18n::trn!("Loading {n} thumbnail…", "Loading {n} thumbnails…", self.images_loading.len()));
        }
        None
    }

    pub fn engine_root(&self) -> Option<&Path> {
        self.engine_root.as_deref()
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn set_search(&mut self, text: &str) {
        self.search = text.to_string();
    }

    /// Все проекты списка (в порядке показа, без учёта поиска).
    pub fn projects(&self) -> &[Listed] {
        &self.listed
    }

    /// Проекты, которые видны с текущим поиском.
    pub fn visible_projects(&self) -> Vec<&Listed> {
        self.listed.iter().filter(|l| project::matches(l, &self.search)).collect()
    }

    pub fn selected(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    pub fn wizard(&self) -> &Wizard {
        &self.wizard
    }

    pub fn wizard_mut(&mut self) -> &mut Wizard {
        &mut self.wizard
    }

    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    pub fn dialog_mut(&mut self) -> Option<&mut Dialog> {
        self.dialog.as_mut()
    }

    pub fn job(&self) -> Option<&Job> {
        self.job.as_ref()
    }

    pub fn job_mut(&mut self) -> Option<&mut Job> {
        self.job.as_mut()
    }

    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    pub fn window_request(&mut self) -> WindowRequest {
        let request = self.window;
        self.window.focus = false;
        request
    }

    /// Каталог настроек хаба.
    pub fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref()
    }

    pub fn preferences(&self) -> &Preferences {
        &self.prefs
    }

    /// Подставить «сейчас» для подписей времени.
    pub fn set_now(&mut self, now: u64) {
        self.now = Some(now);
    }

    pub(crate) fn now(&self) -> u64 {
        self.now.unwrap_or_else(now_unix)
    }

    /// Идентификатор строки проекта (для тестов, которые щёлкают по списку).
    pub fn row_id(path: &Path) -> egui::Id {
        Self::row_id_key(&paths::key(path))
    }

    /// То же по готовому ключу пути (кадр интерфейса не обращается к диску).
    pub fn row_id_key(key: &str) -> egui::Id {
        egui::Id::new("hub-row").with(key)
    }

    /// Идентификатор кнопки навигации.
    pub fn nav_id(page: Page) -> egui::Id {
        egui::Id::new("hub-nav").with(page)
    }

    /// Идентификатор поля поиска в последнем кадре.
    pub fn search_field_id(&self) -> Option<egui::Id> {
        self.search_id
    }

    pub const WIZARD_NAME_ID: &'static str = "hub-wizard-name";
    pub const CREATE_BUTTON_ID: &'static str = "hub-wizard-create";
    pub const CREATE_OPEN_BUTTON_ID: &'static str = "hub-wizard-create-open";
    /// Кнопка «Details» полосы задач.
    pub const TASK_DETAILS_ID: &'static str = "hub-task-details";

    pub(crate) fn listed(&self, path: &Path) -> Option<&Listed> {
        let key = paths::key(path);
        self.listed.iter().find(|l| l.key == key)
    }

    /// Выделить проект (или снять выделение).
    pub(crate) fn select(&mut self, path: Option<PathBuf>) {
        self.selected_key = path.as_deref().map(paths::key);
        self.selected = path;
    }

    /// Идёт фоновый обход проектов.
    pub fn refreshing(&self) -> bool {
        self.refresh_rx.is_some()
    }

    /// Дождаться фонового обхода (тесты).
    pub fn wait_for_refresh(&mut self) {
        if let Some(rx) = self.refresh_rx.take() {
            if let Ok(scan) = rx.recv() {
                self.take_scan(scan);
            }
        }
    }

    fn take_scan(&mut self, (listed, docs): Scan) {
        self.listed = listed;
        if !docs.is_empty() {
            self.docs = docs;
        }
        self.changed = true;
    }

    /// «Prebuild all», если идёт или только что закончился.
    pub fn prebuild(&self) -> Option<&Prebuild> {
        self.prebuild.as_ref()
    }

    /// Состояние сборки простыми словами (ожидание блокировки, сломанный движок, ошибка
    /// проекта) — для окна хода.
    pub fn job_diagnosis(&self) -> Option<Diagnosis> {
        let job = self.job.as_ref()?;
        let progress = &job.progress;
        if job.is_building() && progress.waiting_for_lock {
            return Some(Diagnosis::WaitingForLock {
                what: if progress.lock_what.is_empty() { "build directory".to_string() } else { progress.lock_what.clone() },
                seconds: progress.lock_since.map_or(0, |t| t.elapsed().as_secs()),
            });
        }
        let error = progress.errors.first()?.clone();
        let package = self
            .listed
            .iter()
            .find(|l| l.path == job.project)
            .and_then(|l| l.info.as_ref().ok())
            .map(|i| i.package.clone())
            .or_else(|| job.project.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        // Старый формат id пакета даёт имя каталога, новый — имя пакета: своё — любое из двух.
        let folder = job.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let foreign_package = error.package.as_deref().filter(|p| *p != package && *p != folder);
        // Без id пакета (старый cargo) — по файлу: абсолютный путь вне проекта или
        // `crates/…` движка от корня рабочего пространства.
        let foreign_file = error.package.is_none()
            && error.file.as_deref().is_some_and(|file| {
                let path = Path::new(file);
                if path.is_absolute() {
                    !path.starts_with(&job.project)
                } else {
                    file.starts_with("crates/") || file.starts_with("crates\\")
                }
            });
        match (foreign_package, foreign_file) {
            (Some(other), _) => Some(Diagnosis::EngineBroken { package: other.to_string(), error }),
            (None, true) => Some(Diagnosis::EngineBroken {
                package: "the engine".to_string(),
                error,
            }),
            _ => Some(Diagnosis::ProjectError { error }),
        }
    }

    /// Программа движка (`piska-editor`, `piska-player`, `piska-modeler`), если собрана.
    pub fn tool_exe(&self, name: &str) -> Option<PathBuf> {
        crate::builds::tool_exe(name, self.tools_dir.as_deref(), self.engine_root.as_deref())
    }

    /// Ждущие сборки.
    pub fn queued(&self) -> impl Iterator<Item = &Queued> {
        self.queue.iter()
    }

    /// Программа прошлой сборки проекта (редактор для Open, игра для Play).
    pub fn last_build(&self, path: &Path, kind: &JobKind) -> Option<PathBuf> {
        let listed = self.listed(path);
        let entry = self.config.entry(path);
        let (from_list, remembered) = match kind {
            JobKind::Open => (listed.and_then(|l| l.builds.editor_exe.clone()), entry.and_then(|e| e.editor_exe.clone())),
            JobKind::Play => (listed.and_then(|l| l.builds.game_exe.clone()), entry.and_then(|e| e.game_exe.clone())),
            JobKind::Export { .. } | JobKind::Tool { .. } => return None,
        };
        from_list.or(remembered).filter(|p| self.dry_run || p.is_file())
    }

    /// Можно ли удалить проект с диска; `Err` — почему нет.
    pub fn can_delete(&self, path: &Path) -> Result<(), String> {
        let info = project::inspect(path).map_err(|e| trf!("Not deleted: {e}", e = e))?;
        if let Some(root) = &self.engine_root {
            if samples::sample_for(path, Some(root)).is_some() || paths::same(path, &root.join("games").join("template")) {
                return Err(trf!(
                    "`{}` is one of the engine's own samples; it is not deleted from the hub",
                    info.package
                ));
            }
        }
        if self.processes.any_running(path) {
            return Err(trf!("`{}` is open; close it first", info.package));
        }
        let mut roots: Vec<PathBuf> = self.config.settings.project_roots.clone();
        roots.extend(self.config.settings.default_location.clone());
        roots.extend(self.engine_root.as_ref().map(|root| root.join("games")));
        let parent = path.parent().unwrap_or(path);
        if !roots.iter().any(|root| paths::same(root, parent)) {
            return Err(trf!(
                "`{}` is not in a project folder the hub knows (the engine's games/, the default \
                 location or a folder where the hub created projects); delete it in the file manager",
                path.display()
            ));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Изменение состояния
    // -----------------------------------------------------------------------

    pub(crate) fn set_status(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            log::warn!("{text}");
        } else {
            log::info!("{text}");
        }
        self.status = Some(Status {
            text,
            error,
            at: Instant::now(),
        });
    }

    /// Перечитать список проектов с диска: в фоновом потоке (в `dry_run` — сразу).
    /// Пока обход идёт, на экране прежний список.
    pub fn refresh(&mut self) {
        if self.refresh_rx.is_some() {
            self.refresh_again = true;
            return;
        }
        let root = self.engine_root.clone();
        let config = self.config.clone();
        let scan = move || -> Scan {
            let mut listed = project::collect(root.as_deref(), &config);
            project::sort(&mut listed, config.settings.sort);
            let docs = root.as_deref().map(samples::docs).unwrap_or_default();
            (listed, docs)
        };
        if self.dry_run {
            let result = scan();
            self.take_scan(result);
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(scan());
        });
        self.refresh_rx = Some(receiver);
    }

    fn poll_refresh(&mut self) {
        let Some(rx) = &self.refresh_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(scan) => {
                self.refresh_rx = None;
                self.take_scan(scan);
                if std::mem::take(&mut self.refresh_again) {
                    self.refresh();
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.refresh_rx = None,
        }
    }

    pub(crate) fn save_config(&mut self) {
        let Some(dir) = self.config_dir.clone() else {
            return;
        };
        if let Err(err) = self.config.save(&dir.join(paths::CONFIG_FILE)) {
            self.set_status(trf!("Cannot save the project list to `{p0}`: {err}", p0 = dir.display(), err = err), true);
            return;
        }
        // Недавние — из записей файла (список на экране может быть ещё не перечитан).
        let recent: Vec<(u64, String, PathBuf)> = self
            .config
            .projects
            .iter()
            .filter(|entry| !entry.hidden && entry.path.exists())
            .filter_map(|entry| {
                let title = self
                    .listed(&entry.path)
                    .map(Listed::title)
                    .or_else(|| entry.path.file_name().map(|n| n.to_string_lossy().into_owned()))?;
                Some((entry.last_opened?, title, entry.path.clone()))
            })
            .collect();
        let _ = std::fs::write(dir.join(paths::RECENT_FILE), crate::config::recent_text(&recent));
    }

    fn save_prefs(&mut self) {
        if !self.dry_run {
            self.prefs.save();
        }
    }

    /// Выполнить действие.
    pub fn apply(&mut self, action: HubAction) {
        match action {
            HubAction::Navigate(page) => {
                if page == Page::NewProject && self.page != Page::NewProject {
                    self.focus_wizard_name = true;
                }
                if page == Page::Import && self.page != Page::Import {
                    self.import.focus_path = true;
                }
                self.page = page;
                if page == Page::NewProject && self.wizard.created.is_some() {
                    self.wizard.created = None;
                    self.wizard.name.clear();
                }
            }
            HubAction::Select(path) => self.select(path),
            HubAction::Open(path) => self.start_with(&path, JobKind::Open, StartMode::Auto),
            HubAction::Play(path) => self.start_with(&path, JobKind::Play, StartMode::Auto),
            HubAction::RebuildAndOpen(path) => self.start_with(&path, JobKind::Open, StartMode::Rebuild),
            HubAction::RebuildAndPlay(path) => self.start_with(&path, JobKind::Play, StartMode::Rebuild),
            HubAction::OpenLastBuild(path) => self.start_with(&path, JobKind::Open, StartMode::LastBuild),
            HubAction::PlayLastBuild(path) => self.start_with(&path, JobKind::Play, StartMode::LastBuild),
            HubAction::PrebuildAll => self.start_prebuild(),
            HubAction::CreateAndOpen => {
                self.create_project();
                if let Some(dir) = self.wizard.created.clone() {
                    self.wizard.created = None;
                    self.wizard.name.clear();
                    self.page = Page::Projects;
                    self.apply(HubAction::Open(dir));
                }
            }
            HubAction::ShowJob(show) => self.show_job = show && (self.job.is_some() || self.prebuild.is_some()),
            HubAction::DismissJob => {
                if !self.job.as_ref().is_some_and(Job::is_building) {
                    self.job = None;
                    self.show_job = false;
                    if self.prebuild.as_ref().is_some_and(|p| p.finished) {
                        self.prebuild = None;
                    }
                    self.start_next();
                }
            }
            HubAction::OpenModeler(file) => {
                let Some(root) = self.engine_root.clone() else {
                    self.set_status(tr_s("The engine checkout was not found, so Piska Modeler cannot be started"), true);
                    return;
                };
                let args: Vec<String> = file.iter().map(|f| f.display().to_string()).collect();
                let tool = ToolStart {
                    package: "piska-modeler".into(),
                    bin: "piska-modeler".into(),
                    args,
                };
                self.launch_tool(&root.join("tools").join("piska-modeler"), JobKind::Tool { name: "piska-modeler".into() }, tool, "Piska Modeler");
            }
            HubAction::CancelPrebuild => {
                if let Some(job) = &mut self.job {
                    if job.is_building() {
                        job.cancel();
                    }
                }
                self.job = None;
                if let Some(prebuild) = &mut self.prebuild {
                    prebuild.finished = true;
                }
                self.start_next();
            }
            HubAction::ShowExport(path) => {
                let title = self.listed(&path).map(Listed::title).unwrap_or_default();
                self.dialog = Some(Dialog::Export {
                    path,
                    title,
                    platform: "web",
                    release: false,
                });
            }
            HubAction::Export { path, platform, release } => {
                self.dialog = None;
                self.start_with(&path, JobKind::Export { platform, release }, StartMode::Rebuild);
            }
            HubAction::Reveal(path) => self.os_action("reveal", &path, platform::reveal),
            HubAction::OpenFile(path) => self.os_action("open", &path, platform::open_with_default),
            HubAction::TogglePin(path) => {
                let entry = self.config.entry_mut(&path);
                entry.favorite = !entry.favorite;
                self.save_config();
                self.refresh();
            }
            HubAction::Remove(path) => {
                let title = self.listed(&path).map(Listed::title).unwrap_or_default();
                let workspace = self.listed(&path).is_some_and(|l| l.source == Source::Workspace) && path.exists();
                if workspace {
                    // Игра движка нашлась бы снова — запоминаем, что её убрали.
                    let entry = self.config.entry_mut(&path);
                    entry.hidden = true;
                    entry.favorite = false;
                } else {
                    self.config.forget(&path);
                }
                if self.selected_key.as_deref() == Some(paths::key(&path).as_str()) {
                    self.select(None);
                }
                self.save_config();
                self.refresh();
                self.set_status(trf!("Removed “{title}” from the list. The folder was not touched.", title = title), false);
            }
            HubAction::AskDelete(path) => {
                let title = self.listed(&path).map(Listed::title).unwrap_or_default();
                // Проверка читает диск — один раз здесь, а не в каждом кадре диалога.
                let verdict = self.can_delete(&path);
                self.dialog = Some(Dialog::Delete { path, title, verdict });
            }
            HubAction::Delete(path) => {
                self.dialog = None;
                match self.can_delete(&path) {
                    Err(reason) => self.set_status(reason, true),
                    Ok(()) => match std::fs::remove_dir_all(&path) {
                        Ok(()) => {
                            self.config.forget(&path);
                            self.save_config();
                            self.refresh();
                            self.set_status(trf!("Deleted `{}` from disk.", path.display()), false);
                        }
                        Err(err) => {
                            self.refresh();
                            self.set_status(trf!("Could not delete `{p0}` completely: {err}", p0 = path.display(), err = err), true);
                        }
                    },
                }
            }
            HubAction::ShowAdd { locate: None } => self.apply(HubAction::Navigate(Page::Import)),
            HubAction::ShowAdd { locate } => {
                let start = locate
                    .as_deref()
                    .and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf))
                    .or_else(|| self.config.settings.default_location.clone())
                    .or_else(|| self.engine_root.as_ref().map(|r| r.join("games")))
                    .unwrap_or_default();
                self.dialog = Some(Dialog::Add {
                    locate,
                    browser: FolderBrowser::new(&start),
                });
            }
            HubAction::Add(path) => match project::inspect(&path) {
                Ok(info) => {
                    self.dialog = None;
                    let entry = self.config.entry_mut(&info.dir);
                    entry.hidden = false;
                    self.save_config();
                    self.refresh();
                    self.select(Some(info.dir.clone()));
                    self.page = Page::Projects;
                    self.set_status(trf!("Added “{}”.", info.title), false);
                }
                Err(err) => self.set_status(err.to_string(), true),
            },
            HubAction::Locate { old, new } => match project::inspect(&new) {
                Ok(info) => {
                    self.dialog = None;
                    let previous = self.config.entry(&old).cloned().unwrap_or_default();
                    self.config.forget(&old);
                    let entry = self.config.entry_mut(&info.dir);
                    entry.favorite |= previous.favorite;
                    entry.last_opened = entry.last_opened.max(previous.last_opened);
                    entry.build_units = entry.build_units.or(previous.build_units);
                    entry.hidden = false;
                    self.save_config();
                    self.refresh();
                    self.set_status(trf!("“{}” found at `{}`.", info.title, info.dir.display()), false);
                }
                Err(err) => self.set_status(err.to_string(), true),
            },
            HubAction::Browse(target) => {
                let start = match target {
                    BrowseTarget::WizardLocation => PathBuf::from(self.wizard.location.trim()),
                    BrowseTarget::DefaultLocation => self
                        .config
                        .settings
                        .default_location
                        .clone()
                        .or_else(|| self.engine_root.as_ref().map(|r| r.join("games")))
                        .unwrap_or_default(),
                    BrowseTarget::ImportFolder | BrowseTarget::ImportFile => {
                        let typed = PathBuf::from(self.import.path.trim());
                        if typed.as_os_str().is_empty() {
                            self.config.settings.last_pack_dir.clone().or_else(|| self.config.settings.default_location.clone()).unwrap_or_default()
                        } else {
                            typed
                        }
                    }
                    BrowseTarget::ImportLocation => self.import.settings.location.clone(),
                    BrowseTarget::PackFolder => match &self.dialog {
                        Some(Dialog::Pack(pack)) => PathBuf::from(pack.out.trim()),
                        _ => PathBuf::new(),
                    },
                };
                let start = start.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf).unwrap_or_default();
                // Системное окно (Windows — проводник, Linux — zenity / kdialog), если программа
                // его включила; встроенный обзор — если его нет.
                if !self.native_browse_failed && engine_build::file_dialog::available() {
                    if self.native_browse.is_none() {
                        use engine_build::file_dialog::{Dialog, Kind, Request};
                        let request = if target == BrowseTarget::ImportFile {
                            let extension = engine_editor::kit::i18n::brand::PROJECT_EXTENSION;
                            Request::new(Kind::Open, tr_s("Open a Project File"))
                                .filter(tr_s("Piska project"), &[extension])
                                .filter(tr_s("All files"), &[])
                        } else {
                            Request::new(Kind::Folder, tr_s("Choose a Folder"))
                        };
                        self.native_browse = Some((target, Dialog::spawn(request.folder(&start))));
                    }
                    return;
                }
                let browser = if target == BrowseTarget::ImportFile {
                    FolderBrowser::with_files(&start, engine_editor::kit::i18n::brand::PROJECT_EXTENSION)
                } else {
                    FolderBrowser::new(&start)
                };
                // Окно сохранения остаётся под выбором каталога: возвращаемся к нему после.
                if target == BrowseTarget::PackFolder {
                    self.pack_return = match self.dialog.take() {
                        Some(Dialog::Pack(pack)) => Some(pack),
                        other => {
                            self.dialog = other;
                            None
                        }
                    };
                }
                self.dialog = Some(Dialog::Browse { target, browser });
            }
            HubAction::SetBrowsed { target, path } => {
                self.dialog = None;
                match target {
                    BrowseTarget::WizardLocation => self.wizard.location = path.display().to_string(),
                    BrowseTarget::DefaultLocation => self.apply(HubAction::SetDefaultLocation(Some(path))),
                    BrowseTarget::ImportFolder | BrowseTarget::ImportFile => self.apply(HubAction::ImportAdd(path)),
                    BrowseTarget::ImportLocation => self.import.settings.location = path,
                    BrowseTarget::PackFolder => {
                        if let Some(mut pack) = self.pack_return.take() {
                            let name = Path::new(pack.out.trim()).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty());
                            let name = name.unwrap_or_else(|| engine_editor::kit::i18n::brand::project_file_name(&pack.package));
                            pack.out = path.join(name).display().to_string();
                            self.dialog = Some(Dialog::Pack(pack));
                        }
                    }
                }
            }
            HubAction::CreateProject => self.create_project(),
            HubAction::CancelJob => {
                if let Some(job) = &mut self.job {
                    job.cancel();
                    self.set_status(tr_s("Build cancelled."), false);
                }
                // Следующая в очереди — сразу; отменённая остаётся видна до «Dismiss».
                if !self.queue.is_empty() {
                    self.start_next();
                }
            }
            HubAction::CloseJob => {
                // Подробное окно закрывается всегда; сборка (если идёт) продолжается в фоне.
                self.show_job = false;
                if self.job.as_ref().is_some_and(Job::is_building) {
                    return;
                }
                self.job = None;
                if self.prebuild.as_ref().is_some_and(|p| p.finished) {
                    self.prebuild = None;
                }
                self.start_next();
            }
            HubAction::RetryJob => {
                if let Some(job) = self.job.take() {
                    let (project, kind) = (job.project.clone(), job.kind.clone());
                    drop(job);
                    self.start_with(&project, kind, StartMode::Rebuild);
                }
            }
            HubAction::SetView(view) => {
                self.config.settings.view = view;
                self.save_config();
            }
            HubAction::SetSort(sort) => {
                self.config.settings.sort = sort;
                self.save_config();
                self.refresh();
            }
            HubAction::SetAfterOpen(after) => {
                self.config.settings.after_open = after;
                self.save_config();
            }
            HubAction::SetDefaultLocation(location) => {
                let in_engine_games = location
                    .as_deref()
                    .zip(self.engine_root.as_deref())
                    .is_some_and(|(l, root)| paths::same(l, &root.join("games")));
                self.config.settings.default_location = if in_engine_games { None } else { location };
                self.wizard.location = self
                    .config
                    .settings
                    .default_location
                    .clone()
                    .or_else(|| self.engine_root.as_ref().map(|r| r.join("games")))
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.save_config();
            }
            HubAction::SetUiScale(scale) => {
                self.prefs.ui_scale = scale.clamp(0.5, 3.0);
                self.save_prefs();
            }
            HubAction::SetLanguage(language) => {
                self.prefs.language = language;
                self.prefs.apply_language();
                self.save_prefs();
            }
            HubAction::SetAccent(index) => {
                if let Some((_, color)) = theme::ACCENTS.get(index) {
                    self.prefs.accent = index;
                    theme::set_accent(*color);
                    self.save_prefs();
                }
            }
            HubAction::RestoreHidden => {
                for entry in &mut self.config.projects {
                    entry.hidden = false;
                }
                self.save_config();
                self.refresh();
            }
            HubAction::Refresh => {
                self.refresh();
                self.textures.clear();
                self.images_loading.clear();
            }
            HubAction::CloseDialog => self.dialog = self.pack_return.take().map(Dialog::Pack),
            HubAction::ShowImport => self.apply(HubAction::Navigate(Page::Import)),
            HubAction::ImportAdd(path) => {
                self.show_job = false;
                self.import_add(&path);
            }
            HubAction::ImportScan(dir) => {
                self.page = Page::Import;
                self.import.message = None;
                self.import_scan(dir);
            }
            HubAction::ImportRemove(index) => self.import_remove(index),
            HubAction::ImportRun => self.import_run(),
            HubAction::ImportCancel => self.import_cancel(),
            HubAction::ImportReset => self.import_reset(),
            HubAction::ShowPack(path) => self.show_pack(path),
            HubAction::StartPack(dialog) => match self.start_pack(&dialog) {
                Ok(()) => self.dialog = None,
                Err(reason) => {
                    let mut dialog = *dialog;
                    dialog.error = Some(reason);
                    self.dialog = Some(Dialog::Pack(Box::new(dialog)));
                }
            },
            HubAction::CancelWork(id) => {
                if let Some(work) = self.works.iter().find(|w| w.id == id) {
                    work.cancel();
                }
            }
            HubAction::DismissWork(id) => self.dismiss_work(id),
        }
    }

    fn os_action(&mut self, what: &str, path: &Path, run: fn(&Path) -> Result<(), String>) {
        if self.dry_run {
            self.dry_log.push(format!("{what} {}", path.display()));
            return;
        }
        if let Err(err) = run(path) {
            self.set_status(err, true);
        }
    }

    /// Открыть / запустить / экспортировать с выбранным способом сборки.
    fn start_with(&mut self, path: &Path, kind: JobKind, mode: StartMode) {
        if matches!(kind, JobKind::Export { .. }) {
            return self.start(path, kind, true);
        }
        if self.open_elsewhere(path, &kind) {
            return;
        }
        let last = self.last_build(path, &kind);
        let fresh = self.listed(path).is_some_and(|l| {
            let state = if kind == JobKind::Open { l.builds.editor } else { l.builds.game };
            matches!(state, crate::builds::BuildState::Fresh { .. })
        });
        // Своего кода нет — общий редактор / проигрыватель, без компиляции. Код есть, но
        // программа проекта не собрана (или устарела) — общий редактор сразу, а он сам
        // соберёт программу проекта и перезапустится в неё (`--build`).
        let native = self.listed(path).and_then(|l| l.info.as_ref().ok()).is_none_or(|i| i.native);
        if mode == StartMode::Auto && matches!(kind, JobKind::Open | JobKind::Play) {
            let dir = path.display().to_string();
            match (&kind, native) {
                (JobKind::Open, false) => {
                    return self.launch_tool(path, kind, ToolStart::editor(vec!["--project".into(), dir]), tr_s("Piska Editor"));
                }
                (JobKind::Play, false) => {
                    let tool = ToolStart {
                        package: "piska-editor".into(),
                        bin: "piska-player".into(),
                        args: vec!["--project".into(), dir],
                    };
                    return self.launch_tool(path, kind, tool, tr_s("Piska Player"));
                }
                (JobKind::Open, true) if !fresh && self.tool_exe("piska-editor").is_some() => {
                    let args = vec!["--project".into(), dir, "--build".into()];
                    self.set_status(tr_s("The project's scripts build inside the editor; it switches to them when they are ready."), false);
                    return self.launch_tool(path, kind, ToolStart::editor(args), tr_s("Piska Editor"));
                }
                _ => {}
            }
        }
        match (mode, last) {
            (StartMode::LastBuild, Some(exe)) => self.launch_last(path, kind, &exe),
            (StartMode::LastBuild, None) => {
                self.set_status(tr_s("There is no previous build of this project yet; build it first."), true)
            }
            (StartMode::Auto, Some(exe)) if fresh => self.launch_last(path, kind, &exe),
            _ => self.start(path, kind, true),
        }
    }

    /// Проект уже открыт в редакторе, запущенном не этим хабом (или до его перезапуска):
    /// второй редактор не запускается (метка `engine_editor::instance`).
    fn open_elsewhere(&mut self, path: &Path, kind: &JobKind) -> bool {
        if *kind != JobKind::Open || self.processes.running_key(&paths::key(path), kind).is_some() {
            return false;
        }
        let Some(pid) = engine_editor::instance::running_editor(path) else { return false };
        // Поднять окно того редактора (он смотрит просьбу между кадрами).
        if !self.dry_run {
            let _ = engine_editor::instance::request_focus(path);
        }
        let title = self.listed(path).map(Listed::title).unwrap_or_default();
        let what = "open in the editor";
        self.set_status(trf!("“{title}” is already {what} (process {pid}).", title = title, what = what, pid = pid), true);
        true
    }

    /// Запустить готовую программу, не собирая. Идущая сборка того же проекта отменяется.
    fn launch_last(&mut self, path: &Path, kind: JobKind, exe: &Path) {
        if self.open_elsewhere(path, &kind) {
            return;
        }
        let key = paths::key(path);
        let title = self.listed(path).map(Listed::title).unwrap_or_default();
        if let Some(pid) = self.processes.running_key(&key, &kind) {
            let what = if kind == JobKind::Open { "open in the editor" } else { "running" };
            self.set_status(trf!("“{title}” is already {what} (process {pid}).", title = title, what = what, pid = pid), true);
            return;
        }
        // Отменяется только сборка этого же проекта; чужая идёт дальше в фоне.
        if self.job.as_ref().is_some_and(|j| paths::same(&j.project, path) && !j.tool) {
            if let Some(job) = &mut self.job {
                if job.is_building() {
                    job.cancel();
                }
            }
            self.job = None;
            self.start_next();
        }
        if kind == JobKind::Open {
            let now = self.now();
            self.config.touch(path, now);
            self.save_config();
            self.refresh();
        }
        self.select(Some(path.to_path_buf()));
        if self.dry_run {
            self.dry_log.push(trf!("launch {}", exe.display()));
            self.launched(&kind, &title, 0);
            return;
        }
        match self.processes.launch(path, kind.clone(), exe) {
            Ok(pid) => self.launched(&kind, &title, pid),
            Err(err) => self.set_status(err, true),
        }
    }

    /// Программа запущена: строка состояния, спрятаться на время редактора.
    fn launched(&mut self, kind: &JobKind, title: &str, pid: u32) {
        if *kind == JobKind::Open {
            self.set_status(trf!("“{title}” is open in the editor (process {pid}).", title = title, pid = pid), false);
            if !self.dry_run {
                self.hidden_for_editor = true;
                self.window.visible = false;
                if self.config.settings.after_open == AfterOpen::Close {
                    self.quit_when_done = true;
                }
            }
        } else {
            self.set_status(trf!("“{title}” is running (process {pid}).", title = title, pid = pid), false);
        }
        self.changed = true;
    }

    /// «Prebuild all»: редакторы всех проектов, которым нужна сборка, по очереди.
    fn start_prebuild(&mut self) {
        if self.job.as_ref().is_some_and(Job::is_building) {
            self.set_status(tr_s("A build is already running; wait for it or cancel it first."), true);
            return;
        }
        // Программы движка, которых ещё нет (общий редактор, проигрыватель, Piska Modeler —
        // её ищет и Tools → Model Editor в редакторе): после редакторов проектов.
        if let Some(root) = self.engine_root.clone() {
            for (package, bin) in [("piska-editor", "piska-editor"), ("piska-editor", "piska-player"), ("piska-modeler", "piska-modeler")] {
                if self.tool_exe(bin).is_none() {
                    self.enqueue(Queued {
                        path: root.join("tools").join(package),
                        kind: JobKind::Tool { name: bin.to_string() },
                        launch: false,
                        tool: Some(ToolStart {
                            package: package.to_string(),
                            bin: bin.to_string(),
                            args: Vec::new(),
                        }),
                    });
                }
            }
        }
        let mut prebuild = Prebuild::default();
        for listed in &self.listed {
            let Ok(info) = &listed.info else {
                continue;
            };
            if info.editor_bin.is_none() {
                continue;
            }
            if matches!(listed.builds.editor, crate::builds::BuildState::Fresh { .. }) {
                prebuild.skipped += 1;
            } else {
                prebuild.queue.push((listed.path.clone(), listed.title()));
            }
        }
        if prebuild.queue.is_empty() {
            self.set_status(trf!("All {} editors are already built and up to date.", prebuild.skipped), false);
            self.start_next();
            return;
        }
        self.prebuild = Some(prebuild);
        self.next_prebuild();
    }

    /// Следующий проект очереди «Prebuild all» (или конец).
    fn next_prebuild(&mut self) {
        let Some(prebuild) = &mut self.prebuild else {
            return;
        };
        let Some((path, _)) = prebuild.queue.get(prebuild.index).cloned() else {
            prebuild.finished = true;
            self.job = None;
            let failed = prebuild.results.iter().filter(|r| !r.1).count();
            let text = if failed == 0 {
                trf!("Prebuilt {} editors.", prebuild.results.len())
            } else {
                trf!("Prebuilt {p0} editors, {failed} failed.", p0 = prebuild.results.len() - failed, failed = failed)
            };
            self.set_status(text, failed > 0);
            self.refresh();
            self.start_next();
            return;
        };
        prebuild.index += 1;
        self.job = None;
        self.start(&path, JobKind::Open, false);
        if self.job.is_none() {
            // Не запустилось (ошибка уже в строке состояния) — дальше по очереди.
            let reason = self.status.as_ref().map(|s| s.text.clone()).unwrap_or_default();
            let name = self.listed(&path).map(Listed::title).unwrap_or_default();
            if let Some(prebuild) = &mut self.prebuild {
                prebuild.results.push((name, false, reason));
            }
            self.next_prebuild();
        }
    }

    /// Собрать (и, если `launch`, запустить) или экспортировать.
    fn start(&mut self, path: &Path, kind: JobKind, launch: bool) {
        if self.job.as_ref().is_some_and(Job::is_building) {
            self.enqueue(Queued {
                path: path.to_path_buf(),
                kind,
                launch,
                tool: None,
            });
            return;
        }
        let info = match project::inspect(path) {
            Ok(info) => info,
            Err(err) => {
                self.set_status(err.to_string(), true);
                return;
            }
        };
        if matches!(kind, JobKind::Open | JobKind::Play) {
            if let Some(pid) = self.processes.running(&info.dir, &kind) {
                let what = if kind == JobKind::Open { "open in the editor" } else { "running" };
                self.set_status(trf!("“{p0}” is already {what} (process {pid}).", p0 = info.title, what = what, pid = pid), true);
                return;
            }
        }
        let command = match jobs::command_for(&info, &kind, self.engine_root.as_deref()) {
            Ok(command) => command,
            Err(err) => {
                self.set_status(err, true);
                return;
            }
        };
        let expected = self.config.entry(&info.dir).and_then(|e| e.build_units);
        let mut job = if self.dry_run {
            self.dry_log.push(command.display());
            Job::detached(info.dir.clone(), info.title.clone(), kind.clone(), command, expected)
        } else {
            match Job::spawn(info.dir.clone(), info.title.clone(), kind.clone(), command, expected) {
                Ok(job) => job,
                Err(err) => {
                    self.set_status(err, true);
                    return;
                }
            }
        };
        job.launch = launch;
        self.retire_finished_job();
        self.job = Some(job);
        self.select(Some(info.dir.clone()));
        self.changed = true;
        if kind == JobKind::Open && launch {
            let now = self.now();
            self.config.touch(&info.dir, now);
            self.save_config();
            self.refresh();
        }
        self.window.visible = true;
    }

    /// Создать проект по мастеру.
    fn create_project(&mut self) {
        let Some(root) = self.engine_root.clone() else {
            self.wizard.error = Some(tr_s("The engine checkout was not found").to_string());
            return;
        };
        let new = self.wizard_project();
        let result = templates::plan(&new, &root).and_then(|plan| templates::create(&plan, &root).map(|()| plan));
        match result {
            Ok(plan) => {
                let dir = paths::clean(&plan.dir);
                self.config.entry_mut(&dir).hidden = false;
                if !plan.in_workspace {
                    let parent = dir.parent().map(Path::to_path_buf).unwrap_or_default();
                    if !self.config.settings.project_roots.iter().any(|r| paths::same(r, &parent)) {
                        self.config.settings.project_roots.push(parent);
                    }
                }
                self.save_config();
                self.refresh();
                self.wizard.created = Some(dir.clone());
                self.wizard_plan = None;
                self.wizard.error = None;
                self.select(Some(dir.clone()));
                self.config.settings.last_location = Some(paths::clean(&new.location));
                self.config.settings.last_template = Some(new.template.title().to_string());
                self.save_config();
                self.set_status(trf!("Created “{}”.", templates::title_of(&plan.package)), false);
                // Пока пользователь работает в общем редакторе, программа редактора проекта
                // собирается в фоне (последней в очереди): первый скрипт на Rust потом
                // перезапускает редактор быстро. Только в `games/` — там движок уже собран.
                if plan.in_workspace && !self.dry_run {
                    self.enqueue(Queued {
                        path: dir,
                        kind: JobKind::Open,
                        launch: false,
                        tool: None,
                    });
                }
            }
            Err(err) => self.wizard.error = Some(err.to_string()),
        }
    }

    /// План для текущих полей мастера (пересчитывается, когда поля меняются).
    pub fn wizard_plan(&mut self) -> WizardPlan {
        let new = self.wizard_project();
        if let Some((cached, plan)) = &self.wizard_plan {
            if *cached == new {
                return plan.clone();
            }
        }
        let plan = match &self.engine_root {
            None => Err(tr_s("The engine checkout was not found").to_string()),
            Some(root) => templates::plan(&new, root)
                .map(|plan| (plan.dir.clone(), plan.in_workspace, plan.file_names().into_iter().map(str::to_string).collect()))
                .map_err(|e| e.to_string()),
        };
        self.wizard_plan = Some((new, plan.clone()));
        plan
    }

    /// Проект, который описывает мастер.
    pub fn wizard_project(&self) -> NewProject {
        NewProject {
            name: self.wizard.name.trim().to_string(),
            location: PathBuf::from(self.wizard.location.trim()),
            template: self.wizard.template,
            cpp: self.wizard.cpp,
        }
    }

    /// Фоновая работа между кадрами: вывод сборки, завершившиеся программы, просьбы
    /// редактора, отметка «хаб жив».
    /// Возвращает `true`, если что-то поменялось и окно стоит перерисовать.
    pub fn update(&mut self) -> bool {
        self.poll_native_browse();
        self.poll_refresh();
        self.update_job();
        self.update_works();
        for exited in self.processes.poll() {
            let title = self.listed(&exited.project).map(Listed::title).unwrap_or_default();
            if !exited.success {
                self.set_status(trf!("“{title}” exited with an error; see its console output.", title = title), true);
            }
            if exited.kind == JobKind::Open && self.processes.editors() == 0 {
                if self.quit_when_done && self.processes.is_empty() {
                    self.window.quit = true;
                } else if self.hidden_for_editor {
                    self.hidden_for_editor = false;
                    self.window.visible = true;
                    self.window.focus = true;
                }
                self.changed = true;
                self.refresh();
            }
        }
        if self.quit_when_done && self.processes.is_empty() && self.job.is_none() {
            self.window.quit = true;
        }
        if !self.dry_run {
            if let Some(dir) = self.config_dir.clone() {
                if self.last_alive.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                    self.last_alive = Some(Instant::now());
                    link::write_alive(&dir, now_unix());
                }
                if self.last_request_check.elapsed() >= Duration::from_millis(400) {
                    self.last_request_check = Instant::now();
                    match link::take_request(&dir) {
                        Some(Request::Show) => {
                            self.window.visible = true;
                            self.window.focus = true;
                            self.quit_when_done = false;
                        }
                        Some(Request::Open(path)) => self.pending_open = Some(path),
                        Some(Request::Import(path)) => {
                            self.window.visible = true;
                            self.window.focus = true;
                            self.quit_when_done = false;
                            self.apply(HubAction::ImportAdd(path));
                        }
                        None => {}
                    }
                }
            }
        }
        // Проект на очереди открывается, когда его прежний редактор закрылся.
        if let Some(path) = self.pending_open.clone() {
            if self.processes.running(&path, &JobKind::Open).is_none() && !self.job.as_ref().is_some_and(Job::is_building) {
                self.pending_open = None;
                self.quit_when_done = false;
                self.window.visible = true;
                self.window.focus = true;
                self.apply(HubAction::Open(path));
            }
        }
        std::mem::take(&mut self.changed)
    }

    fn update_job(&mut self) {
        let Some(job) = &mut self.job else {
            return;
        };
        if !job.is_building() {
            return;
        }
        let polled = job.poll();
        if job.take_dirty() {
            self.changed = true;
        }
        let Some(cargo_ok) = polled else {
            return;
        };
        self.changed = true;
        let success = cargo_ok && job.progress.finished != Some(false);
        let project = job.project.clone();
        let kind = job.kind.clone();
        let title = job.title.clone();
        let launch = job.launch;
        if !success {
            let errors = job.progress.errors.len();
            let reason = if errors > 0 {
                trn!("Build failed with {n} error.", "Build failed with {n} errors.", errors)
            } else {
                tr_s("Build failed; see the output below.").to_string()
            };
            job.finish(JobState::Failed(reason.clone()));
            let failed_title = title.clone();
            if !self.prebuild.as_ref().is_some_and(|p| !p.finished) && !self.queue.is_empty() {
                self.set_status(trf!("Build of “{failed_title}” failed: {reason} Open its details from the task bar.", failed_title = failed_title, reason = reason), true);
                self.start_next();
                return;
            }
            if self.prebuild.as_ref().is_some_and(|p| !p.finished) {
                let detail = job.progress.errors.first().map(|e| format!("{} {}", e.location(), e.message)).unwrap_or(reason);
                if let Some(prebuild) = &mut self.prebuild {
                    prebuild.results.push((title, false, detail));
                }
                self.next_prebuild();
            }
            return;
        }
        let units = job.progress.artifacts;
        let target = job.command.launch_target.clone();
        let executable = target.as_deref().and_then(|t| job.progress.executable(t)).cloned();
        let args = job.command.launch_args.clone();
        let tool = job.tool;
        if !tool {
            let entry = self.config.entry_mut(&project);
            if units > 0 {
                entry.build_units = Some(units);
            }
            if let Some(exe) = &executable {
                match kind {
                    JobKind::Open => entry.editor_exe = Some(exe.clone()),
                    JobKind::Play => entry.game_exe = Some(exe.clone()),
                    JobKind::Export { .. } | JobKind::Tool { .. } => {}
                }
            }
        }
        self.save_config();
        self.refresh();
        if !launch {
            if let Some(job) = &mut self.job {
                job.finish(JobState::Succeeded);
            }
            if let Some(prebuild) = self.prebuild.as_mut().filter(|p| !p.finished) {
                prebuild.results.push((title, true, String::new()));
                self.next_prebuild();
            } else {
                self.start_next();
            }
            return;
        }
        let Some(target) = target else {
            if let Some(job) = &mut self.job {
                job.finish(JobState::Succeeded);
            }
            self.set_status(trf!("“{title}” exported.", title = title), false);
            self.start_next();
            return;
        };
        let Some(executable) = executable else {
            if let Some(job) = &mut self.job {
                job.finish(JobState::Failed(trf!("cargo did not report the program `{target}`", target = target)));
            }
            return;
        };
        if self.dry_run {
            self.dry_log.push(trf!("launch {}{}", executable.display(), args.iter().map(|a| format!(" {a}")).collect::<String>()));
            self.job = None;
            self.launched(&kind, &title, 0);
            self.start_next();
            return;
        }
        match self.processes.launch_with(&project, kind.clone(), &executable, &args) {
            Ok(pid) => {
                self.job = None;
                self.launched(&kind, &title, pid);
                self.start_next();
            }
            Err(err) => {
                if let Some(job) = &mut self.job {
                    job.finish(JobState::Failed(err));
                }
            }
        }
    }

    /// Поставить сборку в очередь (если такая уже ждёт — не дублировать).
    pub(crate) fn enqueue(&mut self, queued: Queued) {
        if self.queue.iter().any(|q| q.kind == queued.kind && q.tool == queued.tool && paths::same(&q.path, &queued.path)) {
            return;
        }
        let title = self
            .listed(&queued.path)
            .map(Listed::title)
            .unwrap_or_else(|| queued.tool.as_ref().map(|t| t.bin.clone()).unwrap_or_default());
        self.queue.push_back(queued);
        self.set_status(
            trf!("“{title}” is queued; it builds after the current build ({p0} waiting).", p0 = self.queue.len(), title = title),
            false,
        );
        self.changed = true;
    }

    /// Законченная сборка уступает место следующей.
    fn retire_finished_job(&mut self) {
        if self.job.as_ref().is_some_and(|j| !j.is_building()) {
            self.job = None;
        }
    }

    /// Следующая сборка очереди, если сейчас ничего не собирается.
    fn start_next(&mut self) {
        if self.job.as_ref().is_some_and(Job::is_building) || self.prebuild.as_ref().is_some_and(|p| !p.finished) {
            return;
        }
        while let Some(next) = self.queue.pop_front() {
            match next.tool {
                Some(tool) => self.start_tool_build(&next.path, next.kind, tool, tr_s("Piska tools"), next.launch),
                None => self.start(&next.path, next.kind, next.launch),
            }
            if self.job.as_ref().is_some_and(Job::is_building) {
                break;
            }
        }
        self.changed = true;
    }

    /// Запустить программу движка (собранную) или поставить её сборку в очередь.
    fn launch_tool(&mut self, path: &Path, kind: JobKind, tool: ToolStart, label: &str) {
        let key = paths::key(path);
        let title = self.listed(path).map(Listed::title).unwrap_or_else(|| label.to_string());
        if let Some(pid) = self.processes.running_key(&key, &kind) {
            let what = if kind == JobKind::Open { "open in the editor" } else { "running" };
            self.set_status(trf!("“{title}” is already {what} (process {pid}).", title = title, what = what, pid = pid), true);
            return;
        }
        if kind == JobKind::Open {
            let now = self.now();
            self.config.touch(path, now);
            self.save_config();
            self.refresh();
            self.select(Some(path.to_path_buf()));
        }
        let Some(exe) = self.tool_exe(&tool.bin) else {
            self.start_tool_build(path, kind, tool, label, true);
            return;
        };
        if self.dry_run {
            self.dry_log.push(trf!("launch {}{}", exe.display(), tool.args.iter().map(|a| format!(" {a}")).collect::<String>()));
            self.launched(&kind, &title, 0);
            return;
        }
        let cwd = if path.is_dir() { path.to_path_buf() } else { self.engine_root.clone().unwrap_or_default() };
        match self.processes.launch_with(&cwd, kind.clone(), &exe, &tool.args) {
            Ok(pid) => self.launched(&kind, &title, pid),
            Err(err) => self.set_status(err, true),
        }
    }

    /// Собрать программу движка один раз (в очереди), затем запустить.
    fn start_tool_build(&mut self, path: &Path, kind: JobKind, tool: ToolStart, label: &str, launch: bool) {
        let Some(root) = self.engine_root.clone() else {
            self.set_status(trf!("{label} is not built and the engine checkout was not found to build it", label = label), true);
            return;
        };
        if self.job.as_ref().is_some_and(Job::is_building) {
            self.enqueue(Queued {
                path: path.to_path_buf(),
                kind,
                launch,
                tool: Some(tool),
            });
            return;
        }
        let command = jobs::tool_command(&root, &tool.package, &tool.bin, tool.args.clone());
        let title = trf!("{} (one-time build)", tool.bin);
        let mut job = if self.dry_run {
            self.dry_log.push(command.display());
            Job::detached(path.to_path_buf(), title, kind, command, None)
        } else {
            match Job::spawn(path.to_path_buf(), title, kind, command, None) {
                Ok(job) => job,
                Err(err) => {
                    self.set_status(err, true);
                    return;
                }
            }
        };
        job.tool = true;
        job.launch = launch;
        self.retire_finished_job();
        self.job = Some(job);
        self.set_status(trf!("Building {label} once; after that it opens projects instantly.", label = label), false);
        self.changed = true;
    }

    /// Окно закрывают. `true` — можно выходить. Пока работают открытые хабом редактор или
    /// игра, окно только прячется, а процесс ждёт их: хаб, запущенный через `cargo run`,
    /// на Windows унёс бы их с собой.
    pub fn close_requested(&mut self) -> bool {
        if let Some(job) = &mut self.job {
            if job.is_building() {
                job.cancel();
            }
        }
        self.job = None;
        if self.processes.is_empty() {
            return true;
        }
        self.window.visible = false;
        self.hidden_for_editor = false;
        self.quit_when_done = true;
        false
    }

    /// Хаб закрывается: убрать отметку «жив».
    pub fn shutdown(&mut self) {
        if let Some(dir) = &self.config_dir {
            if !self.dry_run {
                link::remove_alive(dir);
            }
        }
    }

    /// Что не так с проектом (для подсказок строки).
    pub(crate) fn problem(listed: &Listed) -> Option<String> {
        match &listed.info {
            Ok(info) => info.editor_problem(),
            Err(ProjectError::Missing(path)) => Some(trf!("Folder not found: {}", path.display())),
            Err(err) => Some(err.to_string()),
        }
    }

    /// Нужна ли перерисовка без ввода (идёт сборка, работают программы).
    pub fn busy(&self) -> bool {
        self.job.as_ref().is_some_and(Job::is_building)
            || self.pending_open.is_some()
            || self.refresh_rx.is_some()
            || self.works.iter().any(crate::tasks::Work::is_running)
            || self.import.scanning.is_some()
            || self.import.items.iter().any(|i| i.report.is_none())
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.shutdown();
    }
}
