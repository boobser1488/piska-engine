//! Фоновые задачи хаба, которые не cargo-сборки: сохранение проекта в файл, импорт
//! проектов, тесты после импорта. Каждая идёт в своём потоке; интерфейс видит ход работы и
//! может отменить. Хаб остаётся рабочим: окно не ждёт ни диск, ни сжатие.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine_project::{Control, PackSummary, Progress};

use crate::import::Outcome;

/// Не показывать окно консоли у дочерних процессов (Windows).
pub fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Вид задачи.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    /// Сохранение проекта в файл.
    Pack,
    /// Импорт проектов.
    Import,
    /// `cargo test` импортированного проекта.
    Test,
}

/// Состояние задачи.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkState {
    Running,
    Done,
    Failed(String),
    Cancelled,
}

/// Результат.
#[derive(Clone, Debug)]
pub enum WorkResult {
    Pack {
        out: PathBuf,
        summary: PackSummary,
    },
    Import {
        outcomes: Vec<Outcome>,
        /// Проекты, которые не удалось импортировать: имя и причина.
        failures: Vec<(String, String)>,
    },
    Test {
        passed: bool,
        /// Последние строки вывода.
        tail: Vec<String>,
    },
}

/// Ошибка задачи: отмена отличается от сбоя.
#[derive(Debug)]
pub enum WorkError {
    Cancelled,
    Failed(String),
}

#[derive(Default)]
struct Shared {
    stage: String,
    progress: Option<Progress>,
    done: Option<Result<WorkResult, WorkError>>,
}

/// Задача в фоне.
pub struct Work {
    pub id: u64,
    pub kind: WorkKind,
    pub title: String,
    pub stage: String,
    pub progress: Option<Progress>,
    pub state: WorkState,
    pub result: Option<WorkResult>,
    pub started: Instant,
    pub took: Option<Duration>,
    pub control: Control,
    shared: Arc<Mutex<Shared>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Результат уже обработан хабом (проекты внесены в список).
    pub(crate) handled: bool,
}

/// Отчёт о ходе из потока задачи.
pub type Report<'a> = &'a mut dyn FnMut(&str, &Progress);

impl Work {
    /// Запустить задачу в потоке.
    pub fn spawn<F>(id: u64, kind: WorkKind, title: String, job: F) -> Self
    where
        F: FnOnce(&Control, Report) -> Result<WorkResult, WorkError> + Send + 'static,
    {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let control = Control::new();
        let thread = {
            let shared = shared.clone();
            let control = control.clone();
            std::thread::Builder::new()
                .name(format!("hub-task-{id}"))
                .spawn(move || {
                    let mut last = Instant::now() - Duration::from_secs(1);
                    let mut report = |stage: &str, progress: &Progress| {
                        // Не чаще 20 раз в секунду: интерфейс всё равно не покажет больше.
                        if last.elapsed() < Duration::from_millis(50) {
                            return;
                        }
                        last = Instant::now();
                        if let Ok(mut slot) = shared.lock() {
                            slot.stage = stage.to_string();
                            slot.progress = Some(progress.clone());
                        }
                    };
                    let result = job(&control, &mut report);
                    if let Ok(mut slot) = shared.lock() {
                        slot.done = Some(result);
                    }
                })
                .ok()
        };
        Self {
            id,
            kind,
            title,
            stage: String::new(),
            progress: None,
            state: if thread.is_some() { WorkState::Running } else { WorkState::Failed("cannot start a thread".to_string()) },
            result: None,
            started: Instant::now(),
            took: None,
            control,
            shared,
            thread,
            handled: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.state == WorkState::Running
    }

    /// Забрать новое из потока. `true` — что-то поменялось (перерисовать).
    pub fn poll(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        let Ok(mut slot) = self.shared.lock() else {
            return false;
        };
        let mut changed = false;
        if slot.progress.is_some() {
            self.progress = slot.progress.take();
            self.stage = slot.stage.clone();
            changed = true;
        }
        if let Some(done) = slot.done.take() {
            drop(slot);
            self.took = Some(self.started.elapsed());
            match done {
                Ok(result) => {
                    self.result = Some(result);
                    self.state = WorkState::Done;
                }
                Err(WorkError::Cancelled) => self.state = WorkState::Cancelled,
                Err(WorkError::Failed(reason)) => self.state = WorkState::Failed(reason),
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            changed = true;
        }
        changed
    }

    /// Дождаться конца (тесты).
    pub fn wait(&mut self) {
        let start = Instant::now();
        while self.is_running() && start.elapsed() < Duration::from_secs(120) {
            if !self.poll() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// Доля выполненного 0..1 (по байтам, иначе по файлам).
    pub fn fraction(&self) -> Option<f32> {
        let p = self.progress.as_ref()?;
        if p.bytes_total > 0 {
            Some((p.bytes_done as f64 / p.bytes_total as f64).clamp(0.0, 1.0) as f32)
        } else if p.files_total > 0 {
            Some((p.files_done as f64 / p.files_total as f64).clamp(0.0, 1.0) as f32)
        } else {
            None
        }
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        // Хаб закрывается или задачу убрали: остановить и дождаться, чтобы временные файлы
        // были убраны потоком.
        self.control.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `cargo test -p <пакет>` в каталоге проекта; возвращает хвост вывода. Отмена убивает процесс.
pub fn run_cargo_test(dir: &std::path::Path, package: &str, control: &Control, report: Report) -> Result<WorkResult, WorkError> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let mut command = engine_build::host::command("cargo");
    command
        .args(["test", "-p", package, "--color", "never"])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_window(&mut command);
    let mut child = command.spawn().map_err(|e| WorkError::Failed(format!("cannot start cargo: {e}; is Rust installed?")))?;
    let tail = Arc::new(Mutex::new(std::collections::VecDeque::<String>::new()));
    let mut readers = Vec::new();
    for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)]
        .into_iter()
        .flatten()
    {
        let tail = tail.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if let Ok(mut lines) = tail.lock() {
                    lines.push_back(line);
                    while lines.len() > 40 {
                        lines.pop_front();
                    }
                }
            }
        }));
    }
    let progress = Progress {
        phase: engine_project::Phase::Working,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current: String::new(),
    };
    let status = loop {
        if control.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            for reader in readers {
                let _ = reader.join();
            }
            return Err(WorkError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let last = tail.lock().ok().and_then(|l| l.back().cloned()).unwrap_or_default();
                report(&last, &progress);
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(WorkError::Failed(e.to_string())),
        }
    };
    for reader in readers {
        let _ = reader.join();
    }
    let tail: Vec<String> = tail.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default();
    Ok(WorkResult::Test {
        passed: status.success(),
        tail,
    })
}
