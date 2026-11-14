//! Состояние мастера и всё, что он делает. Интерфейс (`ui.rs`) только рисует состояние и
//! кладёт [`Action`]; действия применяет [`Installer::apply`], фоновую работу (скрипты,
//! doctor) забирает [`Installer::update`] — поэтому мастер проходится тестами без окна, а
//! окно никогда не ждёт ни скрипт, ни диск.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use engine_editor::kit::i18n::{self, tr_s, trf, Language};

use crate::diag;
use crate::manifest::{self, Existing};
use crate::packages::Packages;
use crate::protocol::{Event, Hello, Line, LogLevel, PlanStep, Prompt, StepEvent, Summary, UninstallItem};
use crate::runner::{Backend, Purpose, RunMsg, RunRequest, Session};
use crate::scripts::{ScriptOptions, Shell};
use crate::settings::Settings;

/// Нет событий дольше — «всё ещё работает…» (скрипт не убивается).
pub const STILL_WORKING: Duration = Duration::from_secs(45);
/// Строк журнала в памяти.
const LOG_LIMIT: usize = 5000;

/// Страница мастера.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Welcome,
    Components,
    Options,
    Licences,
    Review,
    Installing,
    Finish,
    /// Что удалит удаление.
    Uninstall,
}

/// Что делает мастер.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Install,
    /// Есть прежняя установка: добавить компоненты.
    Modify,
    /// Повторить то, что не получилось или пропало.
    Repair,
    Uninstall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Minimal,
    Typical,
    Everything,
    Custom,
}

impl Preset {
    pub fn components(self) -> &'static [&'static str] {
        match self {
            Preset::Minimal => &["core"],
            Preset::Typical => &["core", "cpp", "web"],
            Preset::Everything => &["core", "cpp", "web", "android", "linux-cross", "engine"],
            Preset::Custom => &[],
        }
    }
}

/// Откуда исходники движка.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Каталог, который уже есть (этот checkout).
    Existing,
    /// Клонировать репозиторий.
    Download,
}

/// Что выбирается во встроенном обзоре каталогов.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseTarget {
    InstallDir,
    SourceDir,
    Cache,
    /// Куда сохранить план.
    PlanFolder,
}

/// Модальные окна.
#[derive(Clone, Debug)]
pub enum Dialog {
    Browse(BrowseTarget, Box<piska_hub::FolderBrowser>),
    /// Перед установкой: какие шаги попросят права администратора.
    ConfirmElevation(Vec<(String, String, String)>),
    ConfirmCancel,
    /// Закрыть окно во время установки.
    ConfirmClose,
    /// Вопрос скрипта, на который ответа заранее нет.
    Prompt(Box<Prompt>),
}

/// Состояние компонента на этой машине.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Проверка ещё идёт.
    Checking,
    Installed { version: Option<String> },
    /// Чего-то не хватает: сколько шагов из скольких нужно сделать.
    Partial { missing: usize, total: usize },
    Outdated { version: Option<String> },
    Missing,
    /// Проверка не удалась.
    Unknown,
}

/// Строка хода установки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Running,
    Done,
    Skipped,
    Failed,
    Declined,
    /// Сухой прогон: шаг был бы выполнен / пропущен.
    Planned,
    WouldSkip,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRow {
    pub id: String,
    pub title: String,
    pub component: String,
    pub state: StepState,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub level: LogLevel,
    pub step: Option<String>,
    pub text: String,
    /// Строка не была событием (мусор / stderr) — показывается как есть.
    pub raw: bool,
}

/// Скачивание сейчас.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub step: Option<String>,
    pub url: String,
    pub bytes: u64,
    pub total: Option<u64>,
    pub estimated: bool,
    /// Байт в секунду (по двум последним отметкам).
    pub speed: Option<f64>,
    pub at: Instant,
}

/// Сбой: что показать на странице Installing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub step: Option<String>,
    pub title: String,
    pub error: String,
    pub log: Option<String>,
    pub skippable: bool,
    pub elevation_refused: bool,
    /// Linux: эту команду нужно выполнить в терминале (пароль не проходит через установщик).
    pub manual_command: Option<String>,
    pub exit: Option<i32>,
}

/// Исход прогона.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Running,
    /// Отмена отправлена, процесс ещё закрывается.
    Cancelling,
    Succeeded,
    Failed(Failure),
    Cancelled,
    /// Нужна перезагрузка, потом продолжить.
    Reboot,
    /// Лицензия или права не приняты.
    Declined(String),
}

/// Прогон скрипта (установка, сухой прогон или удаление).
pub struct Run {
    pub purpose: Purpose,
    pub options: ScriptOptions,
    pub session: Option<Session>,
    pub hello: Option<Hello>,
    pub steps: Vec<StepRow>,
    pub log: Vec<LogEntry>,
    pub progress: Option<Progress>,
    pub summary: Option<Summary>,
    pub outcome: Outcome,
    pub current: Option<String>,
    /// Объяснение запроса прав, который появится сейчас.
    pub elevation: Option<String>,
    pub malformed: usize,
    pub errors: Vec<String>,
    pub uninstall: Vec<UninstallItem>,
    pub started: Instant,
    pub ended: Option<Instant>,
}

impl Run {
    fn new(purpose: Purpose, options: ScriptOptions, session: Session) -> Run {
        Run {
            purpose,
            options,
            session: Some(session),
            hello: None,
            steps: Vec::new(),
            log: Vec::new(),
            progress: None,
            summary: None,
            outcome: Outcome::Running,
            current: None,
            elevation: None,
            malformed: 0,
            errors: Vec::new(),
            uninstall: Vec::new(),
            started: Instant::now(),
            ended: None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.outcome, Outcome::Running | Outcome::Cancelling)
    }

    /// Шагов сделано (или пропущено) из всех.
    pub fn fraction(&self) -> f32 {
        if self.steps.is_empty() {
            return 0.0;
        }
        let done = self.steps.iter().filter(|s| !matches!(s.state, StepState::Pending | StepState::Running)).count();
        done as f32 / self.steps.len() as f32
    }

    /// Последние строки журнала (для сбоя).
    pub fn tail(&self, n: usize) -> Vec<&LogEntry> {
        let start = self.log.len().saturating_sub(n);
        self.log[start..].iter().collect()
    }

    fn push_log(&mut self, entry: LogEntry) {
        if self.log.len() >= LOG_LIMIT {
            self.log.drain(..LOG_LIMIT / 10);
        }
        self.log.push(entry);
    }

    fn row(&mut self, id: &str) -> Option<&mut StepRow> {
        self.steps.iter_mut().find(|s| s.id == id)
    }

    /// Сколько секунд без событий (пока скрипт работает).
    pub fn quiet(&self) -> Option<Duration> {
        self.session.as_ref().filter(|s| !s.finished).map(|s| s.quiet_for())
    }
}

/// Сухой прогон в фоне: проверка при запуске или точный план для Review.
pub struct DryRun {
    pub options: ScriptOptions,
    session: Option<Session>,
    pub hello: Option<Hello>,
    pub steps: Vec<PlanStep>,
    pub summary: Option<Summary>,
    pub uninstall: Option<Vec<UninstallItem>>,
    pub error: Option<String>,
    pub done: bool,
    pub notes: Vec<String>,
}

impl DryRun {
    fn new(options: ScriptOptions, session: Session) -> DryRun {
        DryRun { options, session: Some(session), hello: None, steps: Vec::new(), summary: None, uninstall: None, error: None, done: false, notes: Vec::new() }
    }

    /// `true` — что-то пришло.
    fn pump(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let messages = session.poll();
        let changed = !messages.is_empty();
        for msg in messages {
            match msg {
                RunMsg::Line(Line::Event(Event::Hello(h))) => self.hello = Some(*h),
                RunMsg::Line(Line::Event(Event::Plan(p))) => self.steps.push(*p),
                RunMsg::Line(Line::Event(Event::UninstallPlan { items, .. })) => self.uninstall = Some(items),
                RunMsg::Line(Line::Event(Event::Summary(s))) => self.summary = Some(*s),
                RunMsg::Line(Line::Event(Event::Error { text })) => self.error = Some(text),
                RunMsg::Line(Line::Event(Event::Log { level: LogLevel::Warn, text, .. })) => self.notes.push(text),
                RunMsg::Line(Line::Malformed(text)) | RunMsg::Stderr(text) => self.notes.push(text),
                RunMsg::Line(Line::Event(_)) => {}
                RunMsg::SpawnFailed(err) => {
                    self.error = Some(err);
                    self.done = true;
                }
                RunMsg::Exited(code) => {
                    self.done = true;
                    if self.error.is_none() && code != Some(0) && self.summary.as_ref().is_none_or(|s| !s.ok) {
                        let tail = self.notes.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join("\n");
                        self.error = Some(match code {
                            Some(c) => (trf!("exit code {code}", code = c) + "\n" + &tail).trim().to_string(),
                            None => tail,
                        });
                    }
                }
            }
        }
        if self.done {
            self.session = None;
        }
        changed
    }

    pub fn running(&self) -> bool {
        !self.done
    }
}

/// Действия интерфейса.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Next,
    Back,
    GoTo(Page),
    SetLanguage(Language),
    SetMode(Mode),
    Toggle(String),
    Preset(Preset),
    SetSource(Source),
    Browse(BrowseTarget),
    /// Выбрать каталог из открытого обзора.
    BrowseChoose,
    CloseDialog,
    AcceptLicence(String, bool),
    OpenUrl(String),
    RefreshPlan,
    CopyPlan,
    Install,
    /// Права администратора подтверждены в окне.
    InstallConfirmed,
    Cancel,
    CancelConfirmed,
    Retry,
    SkipStep,
    OpenLog,
    CopyError,
    CopyCommand,
    /// Ответ на вопрос скрипта.
    Answer(bool),
    LaunchHub,
    OpenDocs,
    ShowLog,
    ToggleKeep(String),
    StartUninstall,
    /// Окно просят закрыть (крестик).
    Close,
    CloseConfirmed,
}

/// Параметры запуска (командная строка, тесты).
#[derive(Clone, Default)]
pub struct InstallerOptions {
    /// `None` — настоящие скрипты.
    pub backend: Option<Backend>,
    /// Каталог настроек (`None` — каталог бренда; в тестах — временный).
    pub config_dir: Option<PathBuf>,
    pub dry_run: bool,
    pub components: Option<Vec<String>>,
    pub install_dir: Option<String>,
    pub source: Option<String>,
    pub language: Option<Language>,
    pub uninstall: bool,
    /// Переменные окружения для скриптов и поиска манифеста (тесты: поддельные каталоги).
    pub env: Vec<(String, String)>,
    /// Checkout, из которого запущен установщик (`None` — искать рядом с программой).
    pub engine_root: Option<PathBuf>,
    /// Тесты: свободное место вместо того, что сообщит скрипт.
    pub free_bytes: Option<u64>,
    /// Тесты: не открывать ссылки, файлы и программы, а записывать (см. [`Installer::opened`]).
    pub sandbox: bool,
    /// Тесты: таблица doctor вместо настоящей проверки.
    pub doctor: Option<Vec<diag::Row>>,
    /// Windows: запускать `pwsh` 7, а не `powershell.exe` 5.1.
    pub prefer_pwsh: bool,
    /// Через сколько тишины показать «всё ещё работает…» (тесты; по умолчанию [`STILL_WORKING`]).
    pub still_working: Option<Duration>,
}

/// Установщик.
pub struct Installer {
    pub packages: Packages,
    backend: Option<Backend>,
    pub backend_error: Option<String>,
    pub page: Page,
    pub mode: Mode,
    pub language: Language,
    pub ui_scale: f32,
    /// Отмеченные компоненты (без зависимостей; с ними — [`Installer::resolved`]).
    pub selected: Vec<String>,
    pub preset: Preset,
    pub install_dir: String,
    pub source: Source,
    pub source_dir: String,
    pub engine_root: Option<PathBuf>,
    pub start_menu: bool,
    pub desktop: bool,
    pub file_associations: bool,
    pub write_prefs: bool,
    pub dry_run: bool,
    /// `--dry-run`: сухой прогон нельзя выключить.
    pub forced_dry_run: bool,
    pub cache: String,
    pub proxy: String,
    pub accepted: BTreeSet<String>,
    pub detect: Option<DryRun>,
    pub plan: Option<DryRun>,
    pub run: Option<Run>,
    pub existing: Option<Existing>,
    pub uninstall_plan: Option<DryRun>,
    /// Цели удаления, которые человек решил оставить.
    pub keep: BTreeSet<String>,
    pub doctor: Option<Result<Vec<diag::Row>, String>>,
    doctor_rx: Option<Receiver<Result<Vec<diag::Row>, String>>>,
    pub dialog: Option<Dialog>,
    /// Короткое сообщение внизу («Plan copied»).
    pub toast: Option<(String, Instant)>,
    /// Текст для буфера обмена (забирает окно).
    pub clipboard: Option<String>,
    /// Что открыто бы (sandbox) или открыто: ссылки, файлы, программы.
    pub opened: Vec<String>,
    options: InstallerOptions,
    config_dir: Option<PathBuf>,
    pub settings: Settings,
    actions: Vec<Action>,
    /// Прямоугольники именованных кнопок последнего кадра (для тестов).
    pub(crate) hits: HashMap<String, egui::Rect>,
    pub quit: bool,
    pub window_size: Option<(f32, f32)>,
}

/// Каталог — checkout движка (тот же признак, что у скриптов).
pub fn is_checkout(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file() && dir.join("tools").join("piska-export").is_dir()
}

/// Checkout, из которого запущен установщик: рядом с программой (выше по дереву) или текущий каталог.
pub fn find_checkout() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let starts = exe.iter().filter_map(|e| e.parent().map(Path::to_path_buf)).chain(std::env::current_dir().ok());
    for start in starts {
        let mut dir = Some(start.as_path());
        while let Some(d) = dir {
            if is_checkout(d) {
                return Some(d.to_path_buf());
            }
            dir = d.parent();
        }
    }
    None
}

/// Каталог установки по умолчанию (как у скриптов).
pub fn default_install_dir(name: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    if cfg!(windows) {
        let local = env("LOCALAPPDATA").unwrap_or_else(|| "C:\\".into());
        Path::new(&local).join(name).display().to_string()
    } else {
        let data = env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| format!("{h}/.local/share"))).unwrap_or_else(|| "/tmp".into());
        Path::new(&data).join(name).display().to_string()
    }
}

impl Installer {
    pub fn new(options: InstallerOptions) -> Installer {
        let packages = Packages::embedded();
        let config_dir = options.config_dir.clone().or_else(crate::settings::config_dir);
        let settings = config_dir.as_deref().map(Settings::load).unwrap_or_default();
        let env_vars = options.env.clone();
        let var = move |k: &str| env_vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).or_else(|| std::env::var(k).ok()).filter(|v| !v.is_empty());
        let language = options
            .language
            .or_else(|| settings.language.as_deref().and_then(Language::from_code))
            .unwrap_or_else(Language::system);
        i18n::set(language);
        let engine_root = options.engine_root.clone().or_else(find_checkout).filter(|r| is_checkout(r));
        let (backend, backend_error) = match options.backend.clone() {
            Some(b) => (Some(b), None),
            None => match Backend::real(options.prefer_pwsh) {
                Ok(b) => (Some(b), None),
                Err(e) => (None, Some(e)),
            },
        };
        let existing = manifest::find(&var, &packages.state_dir_name, cfg!(windows));
        let mut selected: Vec<String> = options.components.clone().unwrap_or_else(|| {
            if settings.components.is_empty() {
                Preset::Typical.components().iter().map(|s| s.to_string()).collect()
            } else {
                settings.components.clone()
            }
        });
        selected.retain(|c| packages.component(c).is_some());
        let source = match (options.source.as_ref(), settings.source.as_deref(), engine_root.as_ref()) {
            (Some(_), _, _) => Source::Existing,
            (None, Some("download"), _) => Source::Download,
            (None, Some("existing"), _) => Source::Existing,
            (None, _, Some(_)) => Source::Existing,
            (None, _, None) => Source::Download,
        };
        let source_dir = options
            .source
            .clone()
            .or_else(|| settings.source_dir.clone().filter(|d| is_checkout(Path::new(d))))
            .or_else(|| engine_root.as_ref().map(|r| r.display().to_string()))
            .unwrap_or_default();
        let mut me = Installer {
            install_dir: options
                .install_dir
                .clone()
                .or_else(|| settings.install_dir.clone())
                .unwrap_or_else(|| default_install_dir(&packages.install_dir_name, &var)),
            packages,
            backend,
            backend_error,
            page: Page::Welcome,
            mode: if options.uninstall {
                Mode::Uninstall
            } else if existing.is_some() {
                Mode::Modify
            } else {
                Mode::Install
            },
            language,
            ui_scale: settings.ui_scale.clamp(0.8, 2.0),
            preset: Preset::Custom,
            selected,
            source,
            source_dir,
            engine_root,
            start_menu: settings.start_menu,
            desktop: settings.desktop,
            file_associations: settings.file_associations,
            write_prefs: settings.write_prefs,
            dry_run: options.dry_run,
            forced_dry_run: options.dry_run,
            cache: settings.cache.clone().unwrap_or_default(),
            proxy: settings.proxy.clone().unwrap_or_default(),
            accepted: BTreeSet::new(),
            detect: None,
            plan: None,
            run: None,
            existing,
            uninstall_plan: None,
            keep: BTreeSet::new(),
            doctor: None,
            doctor_rx: None,
            dialog: None,
            toast: None,
            clipboard: None,
            opened: Vec::new(),
            window_size: settings.window,
            config_dir,
            settings,
            actions: Vec::new(),
            hits: HashMap::new(),
            quit: false,
            options,
        };
        me.preset = me.preset_of_selection();
        if me.mode == Mode::Modify {
            if let Some(e) = &me.existing {
                me.selected = e.components.iter().filter(|c| me.packages.component(c).is_some()).cloned().collect();
                me.preset = me.preset_of_selection();
            }
        }
        // «Что уже стоит» — сразу, в фоне.
        me.start_detection();
        me.start_doctor(None, None, BTreeMap::new());
        if me.mode == Mode::Uninstall {
            me.page = Page::Uninstall;
            me.start_uninstall_plan();
        }
        me
    }

    // -----------------------------------------------------------------------
    // Вычисляемое
    // -----------------------------------------------------------------------

    /// Выбор вместе с зависимостями.
    pub fn resolved(&self) -> Vec<String> {
        self.packages.resolve(&self.selected)
    }

    fn preset_of_selection(&self) -> Preset {
        let resolved = self.resolved();
        for p in [Preset::Minimal, Preset::Typical, Preset::Everything] {
            let want = self.packages.resolve(&p.components().iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if want == resolved {
                return p;
            }
        }
        Preset::Custom
    }

    /// Компонент отмечен не человеком, а потому что его требуют другие.
    pub fn auto_selected(&self, id: &str) -> Vec<String> {
        if self.selected.iter().any(|s| s == id) {
            return Vec::new();
        }
        let resolved = self.resolved();
        self.packages.required_by(id, &resolved)
    }

    /// Состояние компонента на машине — по шагам сухого прогона (все пропущены — установлен)
    /// и по doctor (старая версия).
    pub fn presence(&self, id: &str) -> Presence {
        let Some(detect) = &self.detect else {
            return Presence::Unknown;
        };
        let steps: Vec<&PlanStep> = detect.steps.iter().filter(|s| s.component == id).collect();
        if steps.is_empty() {
            return if detect.done { Presence::Unknown } else { Presence::Checking };
        }
        let rows: Vec<&diag::Row> = match &self.doctor {
            Some(Ok(rows)) => rows.iter().filter(|r| r.component == id).collect(),
            _ => Vec::new(),
        };
        let version = rows.iter().find_map(|r| r.version.clone()).or_else(|| steps.iter().find_map(|s| s.version.clone()));
        if rows.iter().any(|r| r.status == diag::RowStatus::Old) {
            return Presence::Outdated { version };
        }
        let missing = steps.iter().filter(|s| !s.skip).count();
        if missing == 0 {
            Presence::Installed { version }
        } else if missing == steps.len() {
            Presence::Missing
        } else {
            Presence::Partial { missing, total: steps.len() }
        }
    }

    /// Места нужно (байт): компоненты, которых ещё нет целиком.
    pub fn required_bytes(&self) -> u64 {
        self.resolved()
            .iter()
            .filter(|c| !matches!(self.presence(c), Presence::Installed { .. }))
            .filter_map(|c| self.packages.component(c))
            .map(|c| c.disk)
            .sum()
    }

    pub fn download_bytes(&self) -> u64 {
        self.resolved()
            .iter()
            .filter(|c| !matches!(self.presence(c), Presence::Installed { .. }))
            .filter_map(|c| self.packages.component(c))
            .map(|c| c.download)
            .sum()
    }

    /// Свободно на диске каталога установки (из сухого прогона; тесты подставляют своё).
    pub fn free_bytes(&self) -> Option<u64> {
        self.options.free_bytes.or_else(|| {
            self.plan
                .as_ref()
                .and_then(|p| p.hello.as_ref())
                .filter(|_| self.plan.as_ref().is_some_and(|p| p.options.install_dir.as_deref() == Some(self.install_dir.as_str())))
                .and_then(|h| h.free_bytes)
                .or_else(|| self.detect.as_ref().and_then(|d| d.hello.as_ref()).and_then(|h| h.free_bytes))
        })
    }

    /// Места не хватает.
    pub fn low_disk(&self) -> bool {
        self.free_bytes().is_some_and(|free| self.required_bytes() > free)
    }

    /// Лицензии, которые нужно принять для выбранного: по точному плану, если он есть, иначе по проверке.
    pub fn licences_needed(&self) -> Vec<crate::protocol::LicenceRef> {
        let resolved = self.resolved();
        let source = match &self.plan {
            Some(p) if p.done && p.error.is_none() => &p.steps,
            _ => match &self.detect {
                Some(d) => &d.steps,
                None => return Vec::new(),
            },
        };
        let mut out: Vec<crate::protocol::LicenceRef> = Vec::new();
        for step in source.iter().filter(|s| !s.skip && resolved.contains(&s.component)) {
            if let Some(l) = &step.licence {
                if !out.iter().any(|o| o.id == l.id) {
                    out.push(l.clone());
                }
            }
        }
        out
    }

    pub fn licence_key(l: &crate::protocol::LicenceRef) -> String {
        l.id.clone().unwrap_or_else(|| l.name.clone())
    }

    pub fn licences_accepted(&self) -> bool {
        self.licences_needed().iter().all(|l| self.accepted.contains(&Self::licence_key(l)))
    }

    /// Каталог исходников годится (если они нужны).
    pub fn source_ok(&self) -> bool {
        !self.resolved().iter().any(|c| c == "engine") || self.source == Source::Download || is_checkout(Path::new(self.source_dir.trim()))
    }

    /// Можно ли идти дальше со страницы.
    pub fn can_advance(&self) -> bool {
        match self.page {
            Page::Welcome => self.backend.is_some(),
            Page::Components => !self.selected.is_empty(),
            Page::Options => !self.install_dir.trim().is_empty() && self.source_ok(),
            Page::Licences => self.licences_accepted() && !self.detect.as_ref().is_some_and(DryRun::running),
            Page::Review => self.install_ready(),
            Page::Installing => self.run.as_ref().is_some_and(|r| r.outcome == Outcome::Succeeded),
            Page::Finish => false,
            Page::Uninstall => self.uninstall_plan.as_ref().is_some_and(|p| p.done && p.error.is_none() && p.uninstall.as_ref().is_some_and(|i| !i.is_empty())),
        }
    }

    /// Кнопка Install доступна: план готов, лицензии приняты.
    pub fn install_ready(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| p.done && p.error.is_none() && p.summary.as_ref().is_some_and(|s| s.ok)) && self.licences_accepted()
    }

    /// Страницы мастера по порядку для текущего режима (левая колонка).
    pub fn pages(&self) -> Vec<Page> {
        match self.mode {
            Mode::Install | Mode::Modify => vec![Page::Welcome, Page::Components, Page::Options, Page::Licences, Page::Review, Page::Installing, Page::Finish],
            Mode::Repair => vec![Page::Welcome, Page::Review, Page::Installing, Page::Finish],
            Mode::Uninstall => vec![Page::Welcome, Page::Uninstall, Page::Installing, Page::Finish],
        }
    }

    /// Параметры скрипта из выбора в мастере.
    pub fn script_options(&self) -> ScriptOptions {
        let mut components = self.resolved();
        if self.mode == Mode::Repair {
            if let Some(e) = &self.existing {
                if !e.components.is_empty() {
                    components = self.packages.resolve(&e.components);
                }
            }
        }
        let engine = components.iter().any(|c| c == "engine");
        ScriptOptions {
            components,
            dry_run: self.dry_run,
            install_dir: Some(self.install_dir.trim().to_string()),
            source_dir: (engine && self.source == Source::Existing).then(|| self.source_dir.trim().to_string()),
            cache: Some(self.cache.trim().to_string()).filter(|c| !c.is_empty()),
            accept_licences: self.accepted.iter().cloned().collect(),
            repair: self.mode == Mode::Repair,
            no_prefs: !self.write_prefs,
            no_shortcuts: !self.start_menu,
            desktop_shortcut: self.desktop,
            file_associations: self.file_associations,
            elevate: (Shell::native() == Shell::Bash).then(|| if has_pkexec() { "pkexec" } else { "none" }.to_string()),
            ..ScriptOptions::default()
        }
    }

    fn script_env(&self) -> Vec<(String, String)> {
        let mut env = self.options.env.clone();
        let proxy = self.proxy.trim();
        if !proxy.is_empty() {
            env.push(("HTTPS_PROXY".into(), proxy.to_string()));
            env.push(("HTTP_PROXY".into(), proxy.to_string()));
        }
        env
    }

    fn start(&self, purpose: Purpose, options: ScriptOptions) -> Option<Session> {
        let backend = self.backend.as_ref()?;
        Some(backend.start(RunRequest { purpose, options, env: self.script_env() }))
    }

    // -----------------------------------------------------------------------
    // Фоновая работа
    // -----------------------------------------------------------------------

    fn start_detection(&mut self) {
        let options = ScriptOptions {
            components: self.packages.components.iter().map(|c| c.id.clone()).collect(),
            dry_run: true,
            install_dir: Some(self.install_dir.clone()),
            source_dir: Some(self.source_dir.clone()).filter(|d| is_checkout(Path::new(d))),
            ..ScriptOptions::default()
        };
        self.detect = self.start(Purpose::Detect, options.clone()).map(|s| DryRun::new(options, s));
    }

    fn start_doctor(&mut self, components: Option<Vec<String>>, path: Option<String>, env: BTreeMap<String, String>) {
        if let Some(rows) = &self.options.doctor {
            self.doctor = Some(Ok(rows.clone()));
            return;
        }
        self.doctor = None;
        self.doctor_rx = Some(diag::spawn(components, path, env));
    }

    fn start_plan(&mut self) {
        let mut options = self.script_options();
        options.dry_run = true;
        if self.plan.as_ref().is_some_and(|p| p.options == options && (p.running() || p.error.is_none())) {
            return;
        }
        self.plan = self.start(Purpose::Plan, options.clone()).map(|s| DryRun::new(options, s));
    }

    fn start_uninstall_plan(&mut self) {
        let options = ScriptOptions { uninstall: true, dry_run: true, ..ScriptOptions::default() };
        self.uninstall_plan = self.start(Purpose::UninstallPlan, options.clone()).map(|s| DryRun::new(options, s));
    }

    fn start_run(&mut self, purpose: Purpose, options: ScriptOptions) {
        if let Some(session) = self.start(purpose, options.clone()) {
            let mut run = Run::new(purpose, options, session);
            // Строки шагов из точного плана: ход виден сразу.
            if purpose == Purpose::Install {
                if let Some(plan) = &self.plan {
                    run.steps = plan
                        .steps
                        .iter()
                        .map(|s| StepRow { id: s.id.clone(), title: s.title.clone(), component: s.component.clone(), state: StepState::Pending, note: None })
                        .collect();
                }
            }
            self.run = Some(run);
            self.page = Page::Installing;
        }
    }

    /// Забрать то, что пришло из фона. `true` — что-то поменялось (нужен кадр).
    pub fn update(&mut self) -> bool {
        let mut changed = false;
        if let Some(d) = self.detect.as_mut() {
            changed |= d.pump();
        }
        if let Some(p) = self.plan.as_mut() {
            changed |= p.pump();
        }
        if let Some(p) = self.uninstall_plan.as_mut() {
            changed |= p.pump();
        }
        if let Some(rx) = &self.doctor_rx {
            if let Ok(result) = rx.try_recv() {
                self.doctor = Some(result);
                self.doctor_rx = None;
                changed = true;
            }
        }
        changed |= self.pump_run();
        if self.toast.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(3)) {
            self.toast = None;
            changed = true;
        }
        changed
    }

    fn pump_run(&mut self) -> bool {
        let Some(run) = self.run.as_mut() else {
            return false;
        };
        let Some(session) = run.session.as_mut() else {
            return false;
        };
        let messages = session.poll();
        if messages.is_empty() {
            return false;
        }
        let cancelled = session.cancelled;
        let mut prompts: Vec<Prompt> = Vec::new();
        for msg in messages {
            match msg {
                RunMsg::Line(Line::Event(event)) => match event {
                    Event::Hello(h) => run.hello = Some(*h),
                    Event::Plan(p) => {
                        // Сухой прогон: план вместо хода.
                        let state = if p.skip { StepState::WouldSkip } else { StepState::Planned };
                        let note = p.found.clone();
                        match run.steps.iter_mut().find(|s| s.id == p.id) {
                            Some(row) => {
                                row.state = state;
                                row.note = note;
                            }
                            None => run.steps.push(StepRow { id: p.id.clone(), title: p.title.clone(), component: p.component.clone(), state, note }),
                        }
                    }
                    Event::Step(s) => step_event(run, &s),
                    Event::Progress { step, url, bytes, total, estimated } => {
                        let now = Instant::now();
                        let speed = run.progress.as_ref().filter(|p| p.url == url && bytes > p.bytes).and_then(|p| {
                            let dt = now.duration_since(p.at).as_secs_f64();
                            (dt > 0.05).then(|| {
                                let current = (bytes - p.bytes) as f64 / dt;
                                p.speed.map_or(current, |old| old * 0.7 + current * 0.3)
                            })
                        });
                        let speed = speed.or_else(|| run.progress.as_ref().filter(|p| p.url == url).and_then(|p| p.speed));
                        run.progress = Some(Progress { step, url, bytes, total, estimated, speed, at: now });
                    }
                    Event::Log { level, step, text } => {
                        if matches!(level, LogLevel::Info | LogLevel::Output) && !text.trim().is_empty() {
                            run.current = Some(text.trim().to_string());
                        }
                        run.push_log(LogEntry { level, step, text, raw: false });
                    }
                    Event::Prompt(p) => prompts.push(*p),
                    Event::Licence { name, .. } => {
                        run.push_log(LogEntry { level: LogLevel::Info, step: None, text: trf!("licence accepted: {name}", name = name), raw: false });
                    }
                    Event::Elevation { reason, .. } => run.elevation = Some(reason),
                    Event::Reboot { .. } => run.outcome = Outcome::Reboot,
                    Event::UninstallPlan { items, .. } => run.uninstall = items,
                    Event::Error { text } => {
                        run.push_log(LogEntry { level: LogLevel::Error, step: None, text: text.clone(), raw: false });
                        run.errors.push(text);
                    }
                    Event::Summary(s) => run.summary = Some(*s),
                    Event::Unknown { .. } => {}
                },
                RunMsg::Line(Line::Malformed(text)) => {
                    run.malformed += 1;
                    run.push_log(LogEntry { level: LogLevel::Output, step: run.steps.iter().find(|s| s.state == StepState::Running).map(|s| s.id.clone()), text, raw: true });
                }
                RunMsg::Stderr(text) => run.push_log(LogEntry { level: LogLevel::Error, step: None, text, raw: true }),
                RunMsg::SpawnFailed(err) => {
                    run.outcome = Outcome::Failed(Failure {
                        step: None,
                        title: String::new(),
                        error: crate::ui::start_failed_text(&err),
                        log: None,
                        skippable: false,
                        elevation_refused: false,
                        manual_command: None,
                        exit: None,
                    });
                    run.ended = Some(Instant::now());
                }
                RunMsg::Exited(code) => finish_run(run, code, cancelled),
            }
        }
        for prompt in prompts {
            self.on_prompt(prompt);
        }
        // Удачная установка — сразу к итогу (сухой прогон тоже).
        let succeeded = self.run.as_ref().is_some_and(|r| r.outcome == Outcome::Succeeded && r.ended.is_some());
        if succeeded && self.page == Page::Installing {
            self.finish();
        }
        true
    }

    /// Итог: таблица doctor с PATH после установки.
    fn finish(&mut self) {
        let (path, env, components) = match self.run.as_ref() {
            Some(run) => (
                run.summary.as_ref().and_then(|s| s.path.clone()).filter(|_| !run.options.dry_run),
                run.summary.as_ref().map(|s| s.env.clone()).unwrap_or_default(),
                Some(run.options.components.clone()).filter(|c| !c.is_empty()),
            ),
            None => (None, BTreeMap::new(), None),
        };
        self.start_doctor(components, path, env);
        self.page = Page::Finish;
        self.save_settings();
    }

    /// Вопрос скрипта: ответ из выбора в мастере или окно.
    fn on_prompt(&mut self, prompt: Prompt) {
        let accepted_licence = prompt.kind == "licence" && prompt.licence.as_ref().is_some_and(|l| self.accepted.contains(l));
        let answer = if accepted_licence {
            Some(true)
        } else {
            match (prompt.kind.as_str(), prompt.key.as_deref()) {
                ("confirm", Some("elevation")) => Some(self.run.as_ref().is_some_and(|r| r.options.confirm_elevation)),
                ("confirm", Some("file-associations")) => Some(self.file_associations),
                ("confirm", Some("uninstall-proceed")) => Some(true),
                ("confirm", Some("uninstall-item")) => Some(!prompt.subject.as_ref().is_some_and(|t| self.keep.contains(t))),
                _ => None,
            }
        };
        match answer {
            Some(yes) => {
                if let Some(session) = self.run.as_ref().and_then(|r| r.session.as_ref()) {
                    session.answer(yes);
                }
            }
            None => self.dialog = Some(Dialog::Prompt(Box::new(prompt))),
        }
    }

    // -----------------------------------------------------------------------
    // Действия
    // -----------------------------------------------------------------------

    pub(crate) fn push(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub(crate) fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        match page {
            Page::Review => self.start_plan(),
            Page::Uninstall if self.uninstall_plan.is_none() => self.start_uninstall_plan(),
            _ => {}
        }
    }

    fn toast(&mut self, text: String) {
        self.toast = Some((text, Instant::now()));
    }

    fn open(&mut self, what: String) {
        if !self.options.sandbox {
            let result = if what.starts_with("http://") || what.starts_with("https://") {
                open_url(&what)
            } else {
                piska_hub::platform::open_with_default(Path::new(&what))
            };
            if let Err(err) = result {
                self.toast(err);
            }
        }
        self.opened.push(what);
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Next => {
                if !self.can_advance() {
                    return;
                }
                let pages = self.pages();
                let next = match self.page {
                    Page::Welcome if self.mode == Mode::Uninstall => Some(Page::Uninstall),
                    Page::Review => {
                        self.apply(Action::Install);
                        None
                    }
                    Page::Uninstall => {
                        self.apply(Action::StartUninstall);
                        None
                    }
                    Page::Installing => {
                        self.finish();
                        None
                    }
                    page => pages.iter().position(|p| *p == page).and_then(|i| pages.get(i + 1)).copied(),
                };
                if let Some(next) = next {
                    self.go(next);
                }
            }
            Action::Back => {
                if self.run.as_ref().is_some_and(Run::is_running) {
                    return;
                }
                let pages = self.pages();
                if matches!(self.page, Page::Installing | Page::Finish) {
                    return;
                }
                if let Some(i) = pages.iter().position(|p| *p == self.page).filter(|i| *i > 0) {
                    self.page = pages[i - 1];
                }
            }
            Action::GoTo(page) => {
                // Только назад и только пока ничего не запущено.
                let pages = self.pages();
                let (Some(target), Some(current)) = (pages.iter().position(|p| *p == page), pages.iter().position(|p| *p == self.page)) else {
                    return;
                };
                if target < current && !matches!(self.page, Page::Installing | Page::Finish) {
                    self.go(page);
                }
            }
            Action::SetLanguage(language) => {
                self.language = language;
                i18n::set(language);
                self.settings.language = Some(language.code().to_string());
                self.save_settings();
            }
            Action::SetMode(mode) => {
                if self.existing.is_none() && mode != Mode::Install {
                    return;
                }
                self.mode = mode;
                if mode == Mode::Uninstall && self.uninstall_plan.is_none() {
                    self.start_uninstall_plan();
                }
            }
            Action::Toggle(id) => {
                if self.packages.component(&id).is_none() {
                    return;
                }
                if let Some(i) = self.selected.iter().position(|s| *s == id) {
                    // Нужен другим отмеченным — снять нельзя.
                    let others: Vec<String> = self.selected.iter().filter(|s| **s != id).cloned().collect();
                    if self.packages.resolve(&others).contains(&id) {
                        return;
                    }
                    self.selected.remove(i);
                } else {
                    self.selected.push(id);
                }
                self.preset = self.preset_of_selection();
            }
            Action::Preset(preset) => {
                if preset != Preset::Custom {
                    self.selected = preset.components().iter().map(|s| s.to_string()).collect();
                }
                self.preset = preset;
            }
            Action::SetSource(source) => self.source = source,
            Action::Browse(target) => {
                let start = match target {
                    BrowseTarget::InstallDir => nearest_existing(&self.install_dir),
                    BrowseTarget::SourceDir => nearest_existing(&self.source_dir),
                    BrowseTarget::Cache => nearest_existing(if self.cache.is_empty() { &self.install_dir } else { &self.cache }),
                    BrowseTarget::PlanFolder => nearest_existing(&home_dir()),
                };
                self.dialog = Some(Dialog::Browse(target, Box::new(piska_hub::FolderBrowser::new(&start))));
            }
            Action::BrowseChoose => {
                if let Some(Dialog::Browse(target, browser)) = self.dialog.take() {
                    let path = browser.path.trim().to_string();
                    match target {
                        BrowseTarget::InstallDir => self.install_dir = path,
                        BrowseTarget::SourceDir => self.source_dir = path,
                        BrowseTarget::Cache => self.cache = path,
                        BrowseTarget::PlanFolder => self.save_plan(Path::new(&path)),
                    }
                }
            }
            Action::CloseDialog => {
                if let Some(Dialog::Prompt(_)) = self.dialog {
                    // Закрыть вопрос — ответить «нет» (как конец ввода у скрипта).
                    self.apply(Action::Answer(false));
                } else {
                    self.dialog = None;
                }
            }
            Action::AcceptLicence(id, on) => {
                if on {
                    self.accepted.insert(id);
                } else {
                    self.accepted.remove(&id);
                }
            }
            Action::OpenUrl(url) => self.open(url),
            Action::RefreshPlan => {
                self.plan = None;
                self.start_plan();
            }
            Action::CopyPlan => {
                self.clipboard = Some(self.plan_text());
                self.toast(tr_s("The plan was copied to the clipboard").to_string());
            }
            Action::Install => {
                if !self.install_ready() {
                    return;
                }
                let elevated: Vec<(String, String, String)> = self
                    .plan
                    .as_ref()
                    .map(|p| p.steps.iter().filter(|s| !s.skip).filter_map(|s| s.elevation.clone().map(|e| (s.id.clone(), s.title.clone(), e))).collect())
                    .unwrap_or_default();
                if !elevated.is_empty() && !self.dry_run {
                    self.dialog = Some(Dialog::ConfirmElevation(elevated));
                } else {
                    self.apply(Action::InstallConfirmed);
                }
            }
            Action::InstallConfirmed => {
                self.dialog = None;
                let mut options = self.script_options();
                options.confirm_elevation = !self.dry_run;
                self.save_settings();
                self.start_run(Purpose::Install, options);
            }
            Action::Cancel => {
                if self.run.as_ref().is_some_and(|r| r.outcome == Outcome::Running) {
                    self.dialog = Some(Dialog::ConfirmCancel);
                }
            }
            Action::CancelConfirmed => {
                self.dialog = None;
                if let Some(run) = self.run.as_mut() {
                    if let Some(session) = run.session.as_mut() {
                        session.cancel();
                        run.outcome = Outcome::Cancelling;
                    }
                }
            }
            Action::Retry | Action::SkipStep => {
                let Some(run) = self.run.as_ref() else {
                    return;
                };
                if run.is_running() {
                    return;
                }
                let mut options = run.options.clone();
                if action == Action::SkipStep {
                    let Outcome::Failed(f) = &run.outcome else {
                        return;
                    };
                    let Some(step) = f.step.clone().filter(|_| f.skippable) else {
                        return;
                    };
                    if !options.skip_steps.contains(&step) {
                        options.skip_steps.push(step);
                    }
                }
                options.resume = !options.uninstall && !options.dry_run;
                let purpose = run.purpose;
                self.start_run(purpose, options);
            }
            Action::OpenLog | Action::ShowLog => {
                if let Some(log) = self.log_path() {
                    self.open(log);
                } else {
                    self.toast(tr_s("There is no log file: a dry run does not write one").to_string());
                }
            }
            Action::CopyError => {
                if let Some(Outcome::Failed(f)) = self.run.as_ref().map(|r| &r.outcome) {
                    let tail: Vec<String> = self.run.as_ref().map(|r| r.tail(20).iter().map(|e| e.text.clone()).collect()).unwrap_or_default();
                    self.clipboard = Some(format!("{}\n{}\n\n{}", f.title, f.error, tail.join("\n")).trim().to_string());
                    self.toast(tr_s("The error was copied to the clipboard").to_string());
                }
            }
            Action::CopyCommand => {
                if let Some(Outcome::Failed(Failure { manual_command: Some(c), .. })) = self.run.as_ref().map(|r| &r.outcome) {
                    self.clipboard = Some(c.clone());
                    self.toast(tr_s("The command was copied to the clipboard").to_string());
                }
            }
            Action::Answer(yes) => {
                if let Some(Dialog::Prompt(p)) = &self.dialog {
                    if yes && p.kind == "licence" {
                        if let Some(id) = &p.licence {
                            self.accepted.insert(id.clone());
                        }
                    }
                    self.dialog = None;
                }
                if let Some(session) = self.run.as_ref().and_then(|r| r.session.as_ref()) {
                    session.answer(yes);
                }
            }
            Action::LaunchHub => {
                if let Some(hub) = self.hub_exe() {
                    if self.options.sandbox {
                        self.opened.push(hub.display().to_string());
                    } else {
                        match engine_build::host::command(&hub).spawn() {
                            Ok(_) => self.opened.push(hub.display().to_string()),
                            Err(e) => self.toast(e.to_string()),
                        }
                    }
                }
            }
            Action::OpenDocs => {
                let doc = self.repo_dir().map(|r| r.join("docs").join("install.md")).filter(|d| d.is_file());
                match doc {
                    Some(doc) => self.open(doc.display().to_string()),
                    None => self.open(docs_url(&self.packages)),
                }
            }
            Action::ToggleKeep(target) => {
                if !self.keep.remove(&target) {
                    self.keep.insert(target);
                }
            }
            Action::StartUninstall => {
                if !self.uninstall_plan.as_ref().is_some_and(|p| p.done && p.error.is_none()) {
                    return;
                }
                let options = ScriptOptions { uninstall: true, dry_run: self.dry_run, ..ScriptOptions::default() };
                self.start_run(Purpose::Uninstall, options);
            }
            Action::Close => {
                if self.run.as_ref().is_some_and(Run::is_running) {
                    self.dialog = Some(Dialog::ConfirmClose);
                } else {
                    self.save_settings();
                    self.quit = true;
                }
            }
            Action::CloseConfirmed => {
                self.dialog = None;
                if let Some(session) = self.run.as_mut().and_then(|r| r.session.as_mut()) {
                    session.cancel();
                }
                self.save_settings();
                self.quit = true;
            }
        }
    }

    /// Журнал скрипта (настоящая установка), если он есть.
    pub fn log_path(&self) -> Option<String> {
        let run = self.run.as_ref()?;
        run.summary
            .as_ref()
            .and_then(|s| s.log.clone())
            .or_else(|| run.hello.as_ref().and_then(|h| h.log.clone()))
            .or_else(|| match &run.outcome {
                Outcome::Failed(f) => f.log.clone(),
                _ => None,
            })
    }

    /// Каталог исходников после установки.
    pub fn repo_dir(&self) -> Option<PathBuf> {
        self.run
            .as_ref()
            .and_then(|r| r.summary.as_ref().and_then(|s| s.repo_dir.clone()).or_else(|| r.hello.as_ref().map(|h| h.repo_dir.clone())))
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| self.engine_root.clone())
    }

    /// Собранный хаб, если он есть.
    pub fn hub_exe(&self) -> Option<PathBuf> {
        let repo = self.repo_dir()?;
        let name = if cfg!(windows) { "piska-hub.exe" } else { "piska-hub" };
        let mut candidates = vec![repo.join("target").join("release").join(name)];
        if let Some(t) = std::env::var_os("CARGO_TARGET_DIR") {
            candidates.insert(0, PathBuf::from(t).join("release").join(name));
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    /// План для людей — тот же текст, что печатает сухой прогон скрипта.
    pub fn plan_text(&self) -> String {
        let Some(plan) = &self.plan else {
            return String::new();
        };
        let mut out = Vec::new();
        out.push(format!("Components: {}", plan.options.components.join(", ")));
        for s in &plan.steps {
            if s.skip {
                let why = match (s.reason.as_deref(), &s.found) {
                    (Some("requested"), _) => " (skipped on request)".to_string(),
                    (_, Some(found)) => format!(" (found: {found})"),
                    _ => " (done earlier)".to_string(),
                };
                out.push(format!("[{}/{}] SKIP {} - {}{why}", s.n, s.total, s.id, s.title));
                continue;
            }
            out.push(format!("[{}/{}] STEP {} - {}", s.n, s.total, s.id, s.title));
            if let Some(e) = &s.elevation {
                out.push(format!("         elevation: {e}"));
            }
            if let Some(l) = &s.licence {
                out.push(format!("         licence: {} ({})", l.name, l.url));
            }
            for l in &s.lines {
                out.push(format!("         {:<8} {}", l.kind, l.text));
            }
        }
        out.join("\n") + "\n"
    }

    fn save_plan(&mut self, folder: &Path) {
        let file = folder.join("install-plan.txt");
        match std::fs::write(&file, self.plan_text()) {
            Ok(()) => self.toast(trf!("The plan was saved to {path}", path = file.display())),
            Err(e) => self.toast(trf!("Could not save the plan: {error}", error = e)),
        }
    }

    /// Запомнить выбор и окно.
    pub fn save_settings(&mut self) {
        self.settings.components = self.selected.clone();
        self.settings.install_dir = Some(self.install_dir.clone());
        self.settings.source = Some(match self.source {
            Source::Existing => "existing",
            Source::Download => "download",
        }
        .to_string());
        self.settings.source_dir = Some(self.source_dir.clone()).filter(|d| !d.is_empty());
        self.settings.start_menu = self.start_menu;
        self.settings.desktop = self.desktop;
        self.settings.file_associations = self.file_associations;
        self.settings.write_prefs = self.write_prefs;
        self.settings.cache = Some(self.cache.clone()).filter(|c| !c.is_empty());
        self.settings.proxy = Some(self.proxy.clone()).filter(|c| !c.is_empty());
        self.settings.ui_scale = self.ui_scale;
        self.settings.window = self.window_size;
        if let Some(dir) = &self.config_dir {
            let _ = self.settings.save(dir);
        }
    }

    /// Порог «всё ещё работает…».
    pub fn still_working(&self) -> Duration {
        self.options.still_working.unwrap_or(STILL_WORKING)
    }

    pub fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref()
    }

    pub fn is_fake(&self) -> bool {
        self.backend.as_ref().is_some_and(Backend::is_fake)
    }

    /// Центр именованной кнопки последнего кадра (тесты).
    pub fn hit(&self, name: &str) -> Option<egui::Pos2> {
        self.hits.get(name).map(|r| r.center())
    }

    pub fn hit_rect(&self, name: &str) -> Option<egui::Rect> {
        self.hits.get(name).copied()
    }

    /// Дождаться фоновой работы (тесты, режим без окна): проверка, план, прогон.
    pub fn wait_idle(&mut self, limit: Duration) -> bool {
        let end = Instant::now() + limit;
        loop {
            self.update();
            let busy = self.detect.as_ref().is_some_and(DryRun::running)
                || self.plan.as_ref().is_some_and(DryRun::running)
                || self.uninstall_plan.as_ref().is_some_and(DryRun::running)
                || self.doctor_rx.is_some()
                || self.run.as_ref().is_some_and(|r| r.is_running());
            if !busy {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Installer {
    fn drop(&mut self) {
        // Ни один скрипт не переживает установщик.
        if let Some(session) = self.run.as_mut().and_then(|r| r.session.as_mut()) {
            session.cancel();
        }
    }
}

fn step_event(run: &mut Run, s: &StepEvent) {
    let state = match s.phase.as_str() {
        "start" => StepState::Running,
        "skip" => StepState::Skipped,
        "done" => StepState::Done,
        "failed" => StepState::Failed,
        "declined" => StepState::Declined,
        _ => return,
    };
    if !run.steps.iter().any(|r| r.id == s.step) {
        run.steps.push(StepRow { id: s.step.clone(), title: s.title.clone(), component: s.component.clone().unwrap_or_default(), state: StepState::Pending, note: None });
    }
    let title = s.title.clone();
    if let Some(row) = run.row(&s.step) {
        row.state = state;
        if !title.is_empty() {
            row.title = title;
        }
        if state == StepState::Skipped {
            row.note = s.found.clone().or_else(|| s.reason.clone());
        }
    }
    match state {
        StepState::Running => {
            run.current = Some(s.title.clone());
            run.progress = None;
        }
        StepState::Done | StepState::Skipped => run.elevation = None,
        StepState::Failed => {
            run.elevation = None;
            run.outcome = Outcome::Failed(Failure {
                step: Some(s.step.clone()),
                title: s.title.clone(),
                error: s.error.clone().unwrap_or_default(),
                log: s.log.clone(),
                skippable: s.skippable,
                elevation_refused: s.elevation_refused,
                manual_command: s.manual_command.clone(),
                exit: None,
            });
        }
        StepState::Declined => {
            run.outcome = Outcome::Declined(s.reason.clone().unwrap_or_default());
        }
        _ => {}
    }
}

fn finish_run(run: &mut Run, code: Option<i32>, cancelled: bool) {
    run.ended = Some(Instant::now());
    for row in run.steps.iter_mut().filter(|r| r.state == StepState::Running) {
        row.state = if cancelled { StepState::Pending } else { StepState::Failed };
    }
    if cancelled {
        run.outcome = Outcome::Cancelled;
        return;
    }
    match &mut run.outcome {
        Outcome::Failed(f) => {
            f.exit = code;
            return;
        }
        Outcome::Reboot | Outcome::Declined(_) => return,
        _ => {}
    }
    let ok = code == Some(0) && run.summary.as_ref().is_none_or(|s| s.ok);
    if ok {
        run.outcome = Outcome::Succeeded;
        return;
    }
    // Скрипт упал, не назвав шаг (исключение, убит извне, не запустился PowerShell).
    let failed = run.summary.as_ref().and_then(|s| s.failed_step.clone());
    let mut error = run.summary.as_ref().and_then(|s| s.error.clone()).or_else(|| run.errors.last().cloned()).unwrap_or_default();
    if error.is_empty() {
        error = run.log.iter().rev().find(|e| e.level == LogLevel::Error).map(|e| e.text.clone()).unwrap_or_default();
    }
    run.outcome = Outcome::Failed(Failure {
        title: failed.as_ref().and_then(|id| run.steps.iter().find(|r| &r.id == id)).map(|r| r.title.clone()).unwrap_or_default(),
        step: failed,
        error,
        log: run.summary.as_ref().and_then(|s| s.log.clone()),
        skippable: false,
        elevation_refused: run.summary.as_ref().is_some_and(|s| s.elevation_refused),
        manual_command: run.summary.as_ref().and_then(|s| s.manual_command.clone()),
        exit: code,
    });
}

fn nearest_existing(path: &str) -> PathBuf {
    let mut p = PathBuf::from(path.trim());
    while !p.as_os_str().is_empty() && !p.is_dir() {
        match p.parent() {
            Some(parent) => p = parent.to_path_buf(),
            None => break,
        }
    }
    if p.as_os_str().is_empty() || !p.is_dir() {
        PathBuf::from(home_dir())
    } else {
        p
    }
}

fn home_dir() -> String {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into())
}

fn has_pkexec() -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("pkexec").is_file()))
}

/// Документация в репозитории (если исходников рядом нет).
pub fn docs_url(p: &Packages) -> String {
    let base = p.repo.trim_end_matches(".git");
    format!("{base}/blob/{}/docs/install.md", p.branch)
}

/// Открыть ссылку браузером системы.
fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let result = engine_build::host::command("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}
