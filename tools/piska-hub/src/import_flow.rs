//! Состояние страницы «Import Project» и фоновая работа вокруг неё: источники, разбор в
//! фоне, запуск импорта, внесение проектов в список, сохранение проекта в файл.
//! Рисует всё это `import_ui.rs`; здесь — то, что проверяется тестами без окна.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use engine_editor::kit::i18n::brand;
use engine_editor::kit::i18n::{tr_s, trf, trn};
use engine_project::{CollectOptions, Control, PackOptions};

use crate::hub::{Dialog, Hub, HubAction, Page, Queued};
use crate::import::{
    self, analyze, check_plan, Environment, ImportSettings, Known, Level, Mode, Outcome, PlanItem, Problem, Report, RunEnv, Source,
};
use crate::jobs::JobKind;
use crate::paths;
use crate::tasks::{Work, WorkError, WorkKind, WorkResult, WorkState};

/// Ключ, по которому кэшируются проблемы, зависящие от настроек.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanKey {
    name: String,
    settings: ImportSettings,
    known: usize,
    ticked_same_name: bool,
    existing: bool,
}

/// Источник в списке импорта.
#[derive(Clone, Debug)]
pub struct ImportItem {
    pub source: Source,
    pub ticked: bool,
    /// Имя пакета после импорта.
    pub name: String,
    /// Имя правили руками — разбор его не трогает.
    pub name_touched: bool,
    /// `None`, пока источник разбирается.
    pub report: Option<Report>,
    plan: Option<(PlanKey, Vec<Problem>)>,
}

/// Где мы на странице.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportStage {
    /// Выбор источников и настройки.
    Choose,
    /// Идёт импорт (страница показывает ход).
    Running,
    /// Результат.
    Result,
}

/// Результат импорта.
#[derive(Clone, Debug)]
pub struct ImportResult {
    pub outcomes: Vec<Outcome>,
    /// Не удалось: имя и причина.
    pub failures: Vec<(String, String)>,
    pub cancelled: bool,
    pub took: Duration,
}

/// Результат фонового разбора источника.
type Analysed = (Source, Report);
/// Результат фонового поиска проектов в каталоге.
type Scanned = (PathBuf, Vec<PathBuf>);

/// Страница «Import Project».
pub struct ImportPage {
    pub items: Vec<ImportItem>,
    pub selected: usize,
    /// Поле пути вверху.
    pub path: String,
    pub settings: ImportSettings,
    /// Раздел «Advanced» раскрыт.
    pub advanced: bool,
    pub stage: ImportStage,
    pub result: Option<ImportResult>,
    /// Поле пути ждёт фокуса.
    pub focus_path: bool,
    /// Сообщение под полем пути (ошибка добавления).
    pub message: Option<String>,
    /// Идёт поиск проектов в каталоге.
    pub scanning: Option<PathBuf>,
    /// Работа импорта (`Work::id`).
    pub work: Option<u64>,
    reports: (Sender<Analysed>, Receiver<Analysed>),
    scans: (Sender<Scanned>, Receiver<Scanned>),
    pub(crate) control: Control,
}

impl ImportPage {
    pub fn new(location: PathBuf) -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            path: String::new(),
            settings: ImportSettings::new(location),
            advanced: false,
            stage: ImportStage::Choose,
            result: None,
            focus_path: false,
            message: None,
            scanning: None,
            work: None,
            reports: channel(),
            scans: channel(),
            control: Control::new(),
        }
    }

    pub fn ticked(&self) -> impl Iterator<Item = &ImportItem> {
        self.items.iter().filter(|i| i.ticked)
    }

    fn find(&self, source: &Source) -> Option<usize> {
        self.items.iter().position(|i| match (&i.source, source) {
            (Source::Folder(a), Source::Folder(b)) | (Source::Archive(a), Source::Archive(b)) => paths::same(a, b),
            _ => false,
        })
    }
}

/// Вопрос «Save Project As File…».
#[derive(Clone, Debug, PartialEq)]
pub struct PackDialog {
    pub path: PathBuf,
    pub title: String,
    pub package: String,
    /// Путь файла (текст поля).
    pub out: String,
    pub tests: bool,
    pub cpp: bool,
    pub builds: bool,
    pub error: Option<String>,
    /// Сколько файлов и байт войдёт (считается в фоне).
    pub estimate: Option<Result<Estimate, String>>,
    pub estimate_key: Option<(bool, bool, bool)>,
}

/// Что войдёт в файл.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Estimate {
    pub files: u64,
    pub bytes: u64,
    /// Размер `dist/` (если есть) — чтобы показать цену переключателя.
    pub builds_bytes: u64,
    pub left_out: usize,
}

/// Расчёт размера для окна сохранения идёт в потоке; результат придёт сюда.
pub(crate) type EstimateChannel = (Sender<((bool, bool, bool), Result<Estimate, String>)>, Receiver<((bool, bool, bool), Result<Estimate, String>)>);

impl Hub {
    pub fn import_page(&self) -> &ImportPage {
        &self.import
    }

    pub fn import_page_mut(&mut self) -> &mut ImportPage {
        &mut self.import
    }

    pub(crate) fn environment(&self) -> Environment {
        Environment {
            engine_root: self.engine_root.clone(),
        }
    }

    fn known(&self) -> Known {
        Known {
            projects: self
                .listed
                .iter()
                .filter_map(|l| l.info.as_ref().ok().map(|i| (i.package.clone(), l.path.clone())))
                .collect(),
        }
    }

    /// Работы в фоне (сохранение, импорт, тесты).
    pub fn works(&self) -> &[Work] {
        &self.works
    }

    /// Добавить свою фоновую задачу в полосу задач (снимки экрана, тесты).
    pub fn add_work(&mut self, work: Work) {
        self.works.push(work);
    }

    /// Дождаться всех задач, кроме одной (она «идёт», пока её не отменят).
    pub fn wait_for_works_except(&mut self, id: u64) {
        for work in &mut self.works {
            if work.id != id {
                work.wait();
            }
        }
        self.update_works();
    }

    fn next_work_id(&mut self) -> u64 {
        self.work_counter += 1;
        self.work_counter
    }

    // -----------------------------------------------------------------------
    // Источники
    // -----------------------------------------------------------------------

    /// Добавить источник: каталог с проектом, каталог с проектами (поиск в фоне) или файл
    /// проекта. Ошибка — в `import.message`.
    pub(crate) fn import_add(&mut self, path: &Path) {
        self.page = Page::Import;
        self.import.stage = if self.import.stage == ImportStage::Result { ImportStage::Choose } else { self.import.stage };
        if self.import.stage == ImportStage::Choose && self.import.result.is_some() {
            self.import.result = None;
        }
        self.import.message = None;
        let path = paths::clean(path);
        if path.is_file() {
            if brand::is_project_file(&path) {
                self.import_push(Source::Archive(path), true, true);
            } else if path.file_name().is_some_and(|n| n == "Cargo.toml") {
                if let Some(dir) = path.parent() {
                    self.import_push(Source::Folder(dir.to_path_buf()), true, true);
                }
            } else {
                self.import.message = Some(trf!(
                    "`{}` is not a project file; a project file ends with .{} (made with Save Project As File…)",
                    path.display(),
                    brand::PROJECT_EXTENSION
                ));
            }
            return;
        }
        if !path.is_dir() {
            self.import.message = Some(trf!("`{}` does not exist", path.display()));
            return;
        }
        if path.join("Cargo.toml").is_file() {
            self.import_push(Source::Folder(path), true, true);
            return;
        }
        self.import_scan(path);
    }

    /// Искать проекты в каталоге (в фоне; в `dry_run` — сразу).
    pub(crate) fn import_scan(&mut self, dir: PathBuf) {
        self.import.scanning = Some(dir.clone());
        let tx = self.import.scans.0.clone();
        let control = self.import.control.clone();
        let job = move || {
            let found = import::scan_for_projects(&dir, &control);
            let _ = tx.send((dir, found));
        };
        if self.dry_run {
            job();
            self.poll_import();
        } else {
            std::thread::spawn(job);
        }
    }

    fn import_push(&mut self, source: Source, ticked: bool, select: bool) {
        if let Some(existing) = self.import.find(&source) {
            if select {
                self.import.selected = existing;
            }
            self.import.items[existing].ticked |= ticked;
            return;
        }
        let name = source
            .path()
            .file_stem()
            .or_else(|| source.path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.import.items.push(ImportItem {
            source: source.clone(),
            ticked,
            name,
            name_touched: false,
            report: None,
            plan: None,
        });
        if select {
            self.import.selected = self.import.items.len() - 1;
        }
        let env = self.environment();
        let tx = self.import.reports.0.clone();
        let control = self.import.control.clone();
        let job = move || {
            let report = analyze(&source, &env, &control);
            let _ = tx.send((source, report));
        };
        if self.dry_run {
            job();
            self.poll_import();
        } else {
            std::thread::spawn(job);
        }
    }

    /// Забрать результаты фонового разбора и поиска.
    pub(crate) fn poll_import(&mut self) -> bool {
        let mut changed = false;
        while let Ok((dir, found)) = self.import.scans.1.try_recv() {
            changed = true;
            if self.import.scanning.as_deref() == Some(dir.as_path()) {
                self.import.scanning = None;
            }
            if found.is_empty() {
                self.import.message = Some(trf!(
                    "No projects found in `{}` (looked three folders deep). A project is a folder with Cargo.toml and src/lib.rs.",
                    dir.display()
                ));
            }
            for (i, project) in found.into_iter().enumerate() {
                self.import_push(Source::Folder(project), true, i == 0);
            }
        }
        while let Ok((source, report)) = self.import.reports.1.try_recv() {
            changed = true;
            let Some(index) = self.import.find(&source) else {
                continue;
            };
            let known = self.known();
            let taken: Vec<String> = known.projects.iter().map(|(n, _)| n.clone()).collect();
            let item = &mut self.import.items[index];
            if !item.name_touched {
                let mut name = report.package.clone();
                // Копия рядом с оригиналом получает свободное имя.
                if !report.blocked() && taken.contains(&name) {
                    let mut n = 1;
                    let mut candidate = format!("{name}_copy");
                    while taken.contains(&candidate) {
                        n += 1;
                        candidate = format!("{name}_copy{n}");
                    }
                    name = candidate;
                }
                item.name = name;
            }
            item.report = Some(report);
            item.plan = None;
        }
        changed
    }

    /// Дождаться разбора (тесты).
    pub fn wait_for_import(&mut self) {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(30) && (self.import.scanning.is_some() || self.import.items.iter().any(|i| i.report.is_none())) {
            if !self.poll_import() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub(crate) fn import_remove(&mut self, index: usize) {
        if index < self.import.items.len() {
            self.import.items.remove(index);
            self.import.selected = self.import.selected.min(self.import.items.len().saturating_sub(1));
        }
    }

    /// Проблемы элемента: из разбора и из настроек (с кэшем).
    pub fn import_problems(&mut self, index: usize) -> Vec<Problem> {
        let Some(item) = self.import.items.get(index) else {
            return Vec::new();
        };
        let Some(report) = item.report.clone() else {
            return Vec::new();
        };
        let mut all = report.problems.clone();
        let name = item.name.clone();
        let duplicate = self
            .import
            .items
            .iter()
            .enumerate()
            .any(|(j, other)| j != index && other.ticked && item.ticked && other.name == name && self.import.settings.mode == Mode::Copy);
        let key = PlanKey {
            name: name.clone(),
            settings: self.import.settings.clone(),
            known: self.listed.len(),
            ticked_same_name: duplicate,
            existing: false,
        };
        let cached = self.import.items[index].plan.as_ref().filter(|(k, _)| *k == key).map(|(_, p)| p.clone());
        let dynamic = match cached {
            Some(problems) => problems,
            None => {
                let env = self.environment();
                let known = self.known();
                let mut problems = check_plan(&report, &name, &self.import.settings, &env, &known);
                if duplicate {
                    problems.push(Problem {
                        level: Level::Error,
                        code: import::Code::NameInvalid,
                        args: vec![tr_s("Another project in this import has the same name").to_string()],
                    });
                }
                self.import.items[index].plan = Some((key, problems.clone()));
                problems
            }
        };
        all.extend(dynamic);
        all
    }

    /// Можно ли запускать: отмечен хотя бы один проект, все отмеченные разобраны и без ошибок.
    pub fn import_ready(&mut self) -> bool {
        if self.import.stage != ImportStage::Choose {
            return false;
        }
        let ticked: Vec<usize> = self.import.items.iter().enumerate().filter(|(_, i)| i.ticked).map(|(n, _)| n).collect();
        if ticked.is_empty() {
            return false;
        }
        for index in ticked {
            if self.import.items[index].report.is_none() {
                return false;
            }
            if self.import_problems(index).iter().any(|p| p.level == Level::Error) {
                return false;
            }
        }
        true
    }

    // -----------------------------------------------------------------------
    // Запуск
    // -----------------------------------------------------------------------

    pub(crate) fn import_run(&mut self) {
        if !self.import_ready() {
            return;
        }
        let items: Vec<PlanItem> = self
            .import
            .ticked()
            .filter_map(|i| {
                Some(PlanItem {
                    source: i.source.clone(),
                    name: i.name.clone(),
                    report: i.report.clone()?,
                })
            })
            .collect();
        let settings = self.import.settings.clone();
        let env = RunEnv {
            engine_root: self.engine_root.clone(),
        };
        // Запоминаем, куда импортировали в прошлый раз.
        if settings.mode == Mode::Copy {
            self.config.settings.last_import_location = Some(settings.location.clone());
            self.save_config();
        }
        let id = self.next_work_id();
        let title = if items.len() == 1 {
            trf!("Importing “{}”", items[0].report.title)
        } else {
            trn!("Importing {n} project", "Importing {n} projects", items.len())
        };
        let total = items.len();
        let work = Work::spawn(id, WorkKind::Import, title, move |control, report| {
            let mut outcomes = Vec::new();
            let mut failures = Vec::new();
            for (index, item) in items.iter().enumerate() {
                if control.is_cancelled() {
                    return Err(WorkError::Cancelled);
                }
                let label = item.report.title.clone();
                let mut stage = |what: &str, progress: &engine_project::Progress| {
                    let text = if total > 1 { format!("({}/{}) {label}: {what}", index + 1, total) } else { what.to_string() };
                    report(&text, progress);
                };
                match import::run_item(item, &settings, &env, control, &mut stage) {
                    Ok(outcome) => outcomes.push(outcome),
                    Err(err) if err.is_cancel() => return Err(WorkError::Cancelled),
                    Err(err) => failures.push((label, err.to_string())),
                }
            }
            Ok(WorkResult::Import { outcomes, failures })
        });
        self.works.push(work);
        self.import.work = Some(id);
        self.import.stage = ImportStage::Running;
        self.import.result = None;
        self.set_status(trf!("Import started: {} to import", total), false);
    }

    pub(crate) fn import_cancel(&mut self) {
        if let Some(id) = self.import.work {
            if let Some(work) = self.works.iter().find(|w| w.id == id) {
                work.cancel();
            }
        }
    }

    /// Новый импорт: очистить страницу.
    pub(crate) fn import_reset(&mut self) {
        self.import.items.clear();
        self.import.selected = 0;
        self.import.stage = ImportStage::Choose;
        self.import.result = None;
        self.import.message = None;
        self.import.path.clear();
        self.import.focus_path = true;
        self.import.work = None;
    }

    // -----------------------------------------------------------------------
    // Фоновые задачи
    // -----------------------------------------------------------------------

    /// Вызывается из `update()`: забрать новое от задач и обработать законченные.
    pub(crate) fn update_works(&mut self) {
        self.poll_import();
        let mut finished: Vec<u64> = Vec::new();
        for work in &mut self.works {
            if work.poll() {
                self.changed = true;
            }
            if !work.is_running() && !work.handled {
                work.handled = true;
                finished.push(work.id);
            }
        }
        for id in finished {
            self.finish_work(id);
        }
    }

    fn finish_work(&mut self, id: u64) {
        let Some(index) = self.works.iter().position(|w| w.id == id) else {
            return;
        };
        let (kind, state, result, took, title) = {
            let w = &self.works[index];
            (w.kind, w.state.clone(), w.result.clone(), w.took.unwrap_or_default(), w.title.clone())
        };
        match (kind, state, result) {
            (WorkKind::Pack, WorkState::Done, Some(WorkResult::Pack { out, summary })) => {
                if let Some(dir) = out.parent() {
                    self.config.settings.last_pack_dir = Some(dir.to_path_buf());
                    self.save_config();
                }
                self.set_status(
                    trf!(
                        "Saved “{}”: {} files, {}",
                        out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        summary.files,
                        import::human(summary.archive_size)
                    ),
                    false,
                );
            }
            (WorkKind::Pack, WorkState::Failed(reason), _) => self.set_status(trf!("Could not save the project file: {}", reason), true),
            (WorkKind::Pack, WorkState::Cancelled, _) => {
                self.set_status(tr_s("Saving cancelled; nothing was written."), false);
                self.works.remove(index);
            }
            (WorkKind::Import, WorkState::Done, Some(WorkResult::Import { outcomes, failures })) => {
                self.import_finished(outcomes, failures, false, took);
                // Страница результата — постоянное место; полоса задач нужна, только если страницу не смотрят.
                if self.page == Page::Import {
                    self.works.retain(|w| w.id != id);
                }
            }
            (WorkKind::Import, WorkState::Cancelled, _) => {
                self.import_finished(Vec::new(), Vec::new(), true, took);
                self.works.retain(|w| w.id != id);
            }
            (WorkKind::Import, WorkState::Failed(reason), _) => {
                self.import_finished(Vec::new(), vec![(title, reason)], false, took);
            }
            (WorkKind::Test, WorkState::Done, Some(WorkResult::Test { passed, .. })) => {
                if passed {
                    self.set_status(trf!("{}: tests passed", title), false);
                } else {
                    self.set_status(trf!("{}: tests failed; open the details in the task strip", title), true);
                }
            }
            (WorkKind::Test, WorkState::Cancelled, _) => {
                self.works.remove(index);
            }
            (_, WorkState::Failed(reason), _) => self.set_status(reason, true),
            _ => {}
        }
        self.changed = true;
    }

    fn import_finished(&mut self, outcomes: Vec<Outcome>, failures: Vec<(String, String)>, cancelled: bool, took: Duration) {
        self.import.work = None;
        self.import.stage = if cancelled { ImportStage::Choose } else { ImportStage::Result };
        if cancelled {
            self.set_status(tr_s("Import cancelled; nothing was left behind."), false);
            return;
        }
        // Внести проекты в список.
        let mut first: Option<PathBuf> = None;
        for outcome in &outcomes {
            let entry = self.config.entry_mut(&outcome.dir);
            entry.hidden = false;
            if outcome.used != import::Used::InPlace {
                let parent = outcome.dir.parent().map(Path::to_path_buf).unwrap_or_default();
                let in_games = self.engine_root.as_ref().is_some_and(|r| paths::same(&parent, &r.join("games")));
                if !in_games && !self.config.settings.project_roots.iter().any(|r| paths::same(r, &parent)) {
                    self.config.settings.project_roots.push(parent);
                }
            }
            first.get_or_insert_with(|| outcome.dir.clone());
        }
        self.save_config();
        self.refresh();
        if self.dry_run {
            self.wait_for_refresh();
        }
        if let Some(dir) = &first {
            self.select(Some(dir.clone()));
        }
        let settings = self.import.settings.clone();
        // После импорта: редактор, сборка в фоне, тесты.
        for (index, outcome) in outcomes.iter().enumerate() {
            if settings.build && outcome.native {
                self.enqueue(Queued {
                    path: outcome.dir.clone(),
                    kind: JobKind::Open,
                    launch: false,
                    tool: None,
                });
            }
            if settings.test {
                self.start_tests(&outcome.dir, &outcome.name);
            }
            if settings.open && index == 0 {
                self.pending_open_after_import = Some(outcome.dir.clone());
            }
        }
        let message = match (outcomes.len(), failures.len()) {
            (n, 0) => trn!("Imported {n} project", "Imported {n} projects", n),
            (0, f) => trn!("Import failed: {n} project could not be imported", "Import failed: {n} projects could not be imported", f),
            (n, f) => trf!("Imported {n}, failed {f}; see the result page", n = n, f = f),
        };
        self.set_status(message, !failures.is_empty() && outcomes.is_empty());
        self.import.result = Some(ImportResult {
            outcomes,
            failures,
            cancelled: false,
            took,
        });
        if let Some(dir) = self.pending_open_after_import.take() {
            self.apply(HubAction::Open(dir));
        }
    }

    pub(crate) fn start_tests(&mut self, dir: &Path, package: &str) {
        if self.dry_run {
            self.dry_log.push(format!("cargo test -p {package}  (in {})", dir.display()));
            return;
        }
        let id = self.next_work_id();
        let (dir, package) = (dir.to_path_buf(), package.to_string());
        let title = trf!("Tests of “{}”", package);
        self.works.push(Work::spawn(id, WorkKind::Test, title, move |control, report| {
            crate::tasks::run_cargo_test(&dir, &package, control, report)
        }));
    }

    /// Убрать законченную задачу из полосы.
    pub(crate) fn dismiss_work(&mut self, id: u64) {
        if self.works.iter().any(|w| w.id == id && !w.is_running()) {
            self.works.retain(|w| w.id != id);
        }
    }

    /// Дождаться всех задач (тесты).
    pub fn wait_for_works(&mut self) {
        for work in &mut self.works {
            work.wait();
        }
        self.update_works();
    }

    // -----------------------------------------------------------------------
    // Сохранение проекта в файл
    // -----------------------------------------------------------------------

    pub(crate) fn show_pack(&mut self, path: PathBuf) {
        let listed = self.listed(&path);
        let title = listed.map(|l| l.title()).unwrap_or_default();
        let package = listed
            .and_then(|l| l.info.as_ref().ok().map(|i| i.package.clone()))
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let dir = self
            .config
            .settings
            .last_pack_dir
            .clone()
            .filter(|d| d.is_dir())
            .or_else(|| path.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let out = dir.join(brand::project_file_name(&package)).display().to_string();
        self.dialog = Some(Dialog::Pack(Box::new(PackDialog {
            path,
            title,
            package,
            out,
            tests: true,
            cpp: true,
            builds: false,
            error: None,
            estimate: None,
            estimate_key: None,
        })));
    }

    /// Пересчитать «сколько войдёт» для окна сохранения, когда поменялись переключатели.
    pub(crate) fn refresh_pack_estimate(&mut self, dialog: &mut PackDialog) {
        while let Ok((key, result)) = self.pack_estimate.1.try_recv() {
            if Some(key) == Some((dialog.tests, dialog.cpp, dialog.builds)) {
                dialog.estimate = Some(result);
            }
        }
        let key = (dialog.tests, dialog.cpp, dialog.builds);
        if dialog.estimate_key == Some(key) {
            return;
        }
        dialog.estimate_key = Some(key);
        dialog.estimate = None;
        let (path, tx) = (dialog.path.clone(), self.pack_estimate.0.clone());
        let job = move || {
            let options = CollectOptions {
                tests: key.0,
                cpp: key.1,
                builds: key.2,
                ..CollectOptions::default()
            };
            let result = engine_project::collect(&path, &options, &Control::new())
                .map(|c| Estimate {
                    files: c.files.len() as u64,
                    bytes: c.bytes,
                    builds_bytes: c.files.iter().filter(|f| f.rel.starts_with("dist/")).map(|f| f.size).sum(),
                    left_out: c.skipped.len(),
                })
                .map_err(|e| e.to_string());
            let _ = tx.send((key, result));
        };
        if self.dry_run {
            job();
            self.refresh_pack_estimate(dialog);
        } else {
            std::thread::spawn(job);
        }
    }

    pub(crate) fn start_pack(&mut self, dialog: &PackDialog) -> Result<(), String> {
        let mut out = PathBuf::from(dialog.out.trim());
        if dialog.out.trim().is_empty() {
            return Err(tr_s("Choose where to save the file").to_string());
        }
        if out.is_dir() {
            return Err(trf!("`{}` is a folder; add a file name, for example {}", out.display(), brand::project_file_name(&dialog.package)));
        }
        if out.extension().is_none() {
            out.set_extension(brand::PROJECT_EXTENSION);
        }
        let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        if !parent.is_dir() {
            std::fs::create_dir_all(&parent).map_err(|e| trf!("Cannot create `{}`: {}", parent.display(), e))?;
        }
        let options = PackOptions {
            collect: CollectOptions {
                tests: dialog.tests,
                cpp: dialog.cpp,
                builds: dialog.builds,
                ..CollectOptions::default()
            },
            tool: brand::HUB_NAME.to_lowercase().replace(' ', "-"),
            engine_version: import::ENGINE_VERSION.to_string(),
            created: Some(self.now()),
            ..PackOptions::default()
        };
        let (project, out_path) = (dialog.path.clone(), out);
        let id = self.next_work_id();
        let title = trf!("Saving “{}” to a file", dialog.title);
        self.works.push(Work::spawn(id, WorkKind::Pack, title, move |control, report| {
            let mut forward = |p: &engine_project::Progress| report(&p.current.clone(), p);
            match engine_project::pack(&project, &out_path, &options, control, &mut forward) {
                Ok(summary) => Ok(WorkResult::Pack { out: out_path, summary }),
                Err(engine_project::Error::Cancelled) => Err(WorkError::Cancelled),
                Err(err) => Err(WorkError::Failed(err.to_string())),
            }
        }));
        Ok(())
    }
}
