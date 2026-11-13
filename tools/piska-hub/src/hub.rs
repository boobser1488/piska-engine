//! Состояние хаба и всё, что он делает. Интерфейс (`ui.rs`) только складывает
//! [`HubAction`]; здесь они выполняются — поэтому хаб проверяется без окна.

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
    Settings,
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
}

/// Куда пойдёт выбранный каталог.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseTarget {
    WizardLocation,
    DefaultLocation,
}

/// Встроенный обзор каталогов: путь, подкаталоги, отметка проектов Piska.
#[derive(Clone, Debug)]
pub struct FolderBrowser {
    /// Текст поля пути (его можно править руками).
    pub path: String,
    /// Подкаталоги показанного каталога: (имя, путь, это проект Piska).
    pub entries: Vec<(String, PathBuf, bool)>,
    /// Каталог, для которого собраны `entries`.
    pub listed: Option<PathBuf>,
    /// Последняя проверка пути как проекта: (путь, результат).
    checked: Option<(String, Result<ProjectInfo, ProjectError>)>,
}

impl FolderBrowser {
    pub fn new(start: &Path) -> Self {
        let mut browser = Self {
            path: start.display().to_string(),
            entries: Vec::new(),
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
    /// Картинки, декодированные в фоне: (ключ, картинка).
    pub(crate) images: ImageChannel,
    pub(crate) images_loading: std::collections::HashSet<String>,
    /// Что-то поменялось с прошлого кадра (окно перерисуется).
    changed: bool,
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
        let location = config
            .settings
            .default_location
            .clone()
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
                template: Template::Empty2D,
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
        };
        if hub.engine_root.is_none() {
            hub.set_status(
                "The engine checkout was not found: set PISKA_ENGINE_ROOT or start the hub from the engine folder",
                true,
            );
        }
        if let Some(problem) = problem {
            hub.set_status(problem, true);
        }
        hub.refresh();
        hub
    }

    // -----------------------------------------------------------------------
    // Чтение состояния
    // -----------------------------------------------------------------------

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

    /// Программа прошлой сборки проекта (редактор для Open, игра для Play).
    pub fn last_build(&self, path: &Path, kind: &JobKind) -> Option<PathBuf> {
        let listed = self.listed(path);
        let entry = self.config.entry(path);
        let (from_list, remembered) = match kind {
            JobKind::Open => (listed.and_then(|l| l.builds.editor_exe.clone()), entry.and_then(|e| e.editor_exe.clone())),
            JobKind::Play => (listed.and_then(|l| l.builds.game_exe.clone()), entry.and_then(|e| e.game_exe.clone())),
            JobKind::Export { .. } => return None,
        };
        from_list.or(remembered).filter(|p| self.dry_run || p.is_file())
    }

    /// Можно ли удалить проект с диска; `Err` — почему нет.
    pub fn can_delete(&self, path: &Path) -> Result<(), String> {
        let info = project::inspect(path).map_err(|e| format!("Not deleted: {e}"))?;
        if let Some(root) = &self.engine_root {
            if samples::sample_for(path, Some(root)).is_some() || paths::same(path, &root.join("games").join("template")) {
                return Err(format!(
                    "`{}` is one of the engine's own samples; it is not deleted from the hub",
                    info.package
                ));
            }
        }
        if self.processes.any_running(path) {
            return Err(format!("`{}` is open; close it first", info.package));
        }
        let mut roots: Vec<PathBuf> = self.config.settings.project_roots.clone();
        roots.extend(self.config.settings.default_location.clone());
        roots.extend(self.engine_root.as_ref().map(|root| root.join("games")));
        let parent = path.parent().unwrap_or(path);
        if !roots.iter().any(|root| paths::same(root, parent)) {
            return Err(format!(
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

    fn save_config(&mut self) {
        let Some(dir) = self.config_dir.clone() else {
            return;
        };
        if let Err(err) = self.config.save(&dir.join(paths::CONFIG_FILE)) {
            self.set_status(format!("Cannot save the project list to `{}`: {err}", dir.display()), true);
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
                self.set_status(format!("Removed “{title}” from the list. The folder was not touched."), false);
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
                            self.set_status(format!("Deleted `{}` from disk.", path.display()), false);
                        }
                        Err(err) => {
                            self.refresh();
                            self.set_status(format!("Could not delete `{}` completely: {err}", path.display()), true);
                        }
                    },
                }
            }
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
                    self.set_status(format!("Added “{}”.", info.title), false);
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
                    self.set_status(format!("“{}” found at `{}`.", info.title, info.dir.display()), false);
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
                };
                let start = start.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf).unwrap_or_default();
                self.dialog = Some(Dialog::Browse {
                    target,
                    browser: FolderBrowser::new(&start),
                });
            }
            HubAction::SetBrowsed { target, path } => {
                self.dialog = None;
                match target {
                    BrowseTarget::WizardLocation => self.wizard.location = path.display().to_string(),
                    BrowseTarget::DefaultLocation => self.apply(HubAction::SetDefaultLocation(Some(path))),
                }
            }
            HubAction::CreateProject => self.create_project(),
            HubAction::CancelJob => {
                if let Some(job) = &mut self.job {
                    job.cancel();
                    self.set_status("Build cancelled.", false);
                }
            }
            HubAction::CloseJob => {
                if self.job.as_ref().is_some_and(Job::is_building) {
                    return;
                }
                self.job = None;
                if self.prebuild.as_ref().is_some_and(|p| p.finished) {
                    self.prebuild = None;
                }
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
            HubAction::CloseDialog => self.dialog = None,
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
        let last = self.last_build(path, &kind);
        let fresh = self.listed(path).is_some_and(|l| {
            let state = if kind == JobKind::Open { l.builds.editor } else { l.builds.game };
            matches!(state, crate::builds::BuildState::Fresh { .. })
        });
        match (mode, last) {
            (StartMode::LastBuild, Some(exe)) => self.launch_last(path, kind, &exe),
            (StartMode::LastBuild, None) => {
                self.set_status("There is no previous build of this project yet; build it first.", true)
            }
            (StartMode::Auto, Some(exe)) if fresh => self.launch_last(path, kind, &exe),
            _ => self.start(path, kind, true),
        }
    }

    /// Запустить готовую программу, не собирая. Идущая сборка того же проекта отменяется.
    fn launch_last(&mut self, path: &Path, kind: JobKind, exe: &Path) {
        let key = paths::key(path);
        let title = self.listed(path).map(Listed::title).unwrap_or_default();
        if let Some(pid) = self.processes.running_key(&key, &kind) {
            let what = if kind == JobKind::Open { "open in the editor" } else { "running" };
            self.set_status(format!("“{title}” is already {what} (process {pid})."), true);
            return;
        }
        if let Some(job) = &mut self.job {
            if job.is_building() {
                job.cancel();
            }
        }
        self.job = None;
        if kind == JobKind::Open {
            let now = self.now();
            self.config.touch(path, now);
            self.save_config();
            self.refresh();
        }
        self.select(Some(path.to_path_buf()));
        if self.dry_run {
            self.dry_log.push(format!("launch {}", exe.display()));
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
            self.set_status(format!("“{title}” is open in the editor (process {pid})."), false);
            if !self.dry_run {
                self.hidden_for_editor = true;
                self.window.visible = false;
                if self.config.settings.after_open == AfterOpen::Close {
                    self.quit_when_done = true;
                }
            }
        } else {
            self.set_status(format!("“{title}” is running (process {pid})."), false);
        }
        self.changed = true;
    }

    /// «Prebuild all»: редакторы всех проектов, которым нужна сборка, по очереди.
    fn start_prebuild(&mut self) {
        if self.job.as_ref().is_some_and(Job::is_building) {
            self.set_status("A build is already running; wait for it or cancel it first.", true);
            return;
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
            self.set_status(format!("All {} editors are already built and up to date.", prebuild.skipped), false);
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
                format!("Prebuilt {} editors.", prebuild.results.len())
            } else {
                format!("Prebuilt {} editors, {failed} failed.", prebuild.results.len() - failed)
            };
            self.set_status(text, failed > 0);
            self.refresh();
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
            self.set_status("A build is already running; wait for it or cancel it first.", true);
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
                self.set_status(format!("“{}” is already {what} (process {pid}).", info.title), true);
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
            self.wizard.error = Some("The engine checkout was not found".to_string());
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
                self.select(Some(dir));
                self.set_status(format!("Created “{}”.", templates::title_of(&plan.package)), false);
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
            None => Err("The engine checkout was not found".to_string()),
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
        self.poll_refresh();
        self.update_job();
        for exited in self.processes.poll() {
            let title = self.listed(&exited.project).map(Listed::title).unwrap_or_default();
            if !exited.success {
                self.set_status(format!("“{title}” exited with an error; see its console output."), true);
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
                format!("Build failed with {errors} error{}.", if errors == 1 { "" } else { "s" })
            } else {
                "Build failed; see the output below.".to_string()
            };
            job.finish(JobState::Failed(reason.clone()));
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
        {
            let entry = self.config.entry_mut(&project);
            if units > 0 {
                entry.build_units = Some(units);
            }
            if let Some(exe) = &executable {
                match kind {
                    JobKind::Open => entry.editor_exe = Some(exe.clone()),
                    JobKind::Play => entry.game_exe = Some(exe.clone()),
                    JobKind::Export { .. } => {}
                }
            }
        }
        self.save_config();
        self.refresh();
        if !launch {
            if let Some(job) = &mut self.job {
                job.finish(JobState::Succeeded);
            }
            if let Some(prebuild) = &mut self.prebuild {
                prebuild.results.push((title, true, String::new()));
            }
            self.next_prebuild();
            return;
        }
        let Some(target) = target else {
            if let Some(job) = &mut self.job {
                job.finish(JobState::Succeeded);
            }
            self.set_status(format!("“{title}” exported."), false);
            return;
        };
        let Some(executable) = executable else {
            if let Some(job) = &mut self.job {
                job.finish(JobState::Failed(format!("cargo did not report the program `{target}`")));
            }
            return;
        };
        if self.dry_run {
            self.dry_log.push(format!("launch {}", executable.display()));
            self.job = None;
            self.launched(&kind, &title, 0);
            return;
        }
        match self.processes.launch(&project, kind.clone(), &executable) {
            Ok(pid) => {
                self.job = None;
                self.launched(&kind, &title, pid);
            }
            Err(err) => {
                if let Some(job) = &mut self.job {
                    job.finish(JobState::Failed(err));
                }
            }
        }
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
            Err(ProjectError::Missing(path)) => Some(format!("Folder not found: {}", path.display())),
            Err(err) => Some(err.to_string()),
        }
    }

    /// Нужна ли перерисовка без ввода (идёт сборка, работают программы).
    pub fn busy(&self) -> bool {
        self.job.as_ref().is_some_and(Job::is_building) || self.pending_open.is_some() || self.refresh_rx.is_some()
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.shutdown();
    }
}
