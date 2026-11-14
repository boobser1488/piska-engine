//! Сборка и запуск: команда cargo для проекта, чтение её вывода в фоне, запущенные
//! редакторы и игры (один и тот же проект дважды не открывается).

use engine_editor::kit::i18n::{tr_s, trf};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::cargo::{self, Progress};
use crate::paths;
use crate::project::ProjectInfo;

/// Что делается с проектом.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// Собрать программу редактора и открыть её.
    Open,
    /// Собрать игру и запустить.
    Play,
    /// Экспорт через `piska-export`.
    Export { platform: String, release: bool },
    /// Программа движка без проекта (Piska Modeler).
    Tool { name: String },
}

impl JobKind {
    pub fn verb(&self) -> &'static str {
        match self {
            JobKind::Open => tr_s("Opening"),
            JobKind::Play => tr_s("Starting"),
            JobKind::Export { .. } => tr_s("Exporting"),
            JobKind::Tool { .. } => tr_s("Preparing"),
        }
    }
}

/// Команда, которую хаб запустит (её проверяют тесты и показывает окно хода).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Добавочные переменные окружения (остальное наследуется от хаба).
    pub env: Vec<(String, String)>,
    /// Какую программу запустить после удачной сборки (имя цели cargo).
    pub launch_target: Option<String>,
    /// Аргументы запуска собранной программы (`--project <каталог>` у общего редактора).
    pub launch_args: Vec<String>,
}

impl CommandSpec {
    /// Строка для показа: `cargo build -p lantern …`.
    pub fn display(&self) -> String {
        // Полный путь из `CARGO` в подписи не нужен: `cargo build …`.
        let mut text = std::path::Path::new(&self.program)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.program.clone());
        for arg in &self.args {
            text.push(' ');
            if arg.contains(' ') {
                text.push_str(&format!("\"{arg}\""));
            } else {
                text.push_str(arg);
            }
        }
        text
    }
}

/// Переменные, с которыми cargo печатает полосу хода сборки и в канал (не только в терминал).
fn progress_env() -> Vec<(String, String)> {
    vec![
        ("CARGO_TERM_PROGRESS_WHEN".into(), "always".into()),
        ("CARGO_TERM_PROGRESS_WIDTH".into(), "200".into()),
        ("CARGO_TERM_COLOR".into(), "never".into()),
    ]
}

/// Команда сборки для проекта. `CARGO_TARGET_DIR` и прочее окружение наследуются.
pub fn command_for(info: &ProjectInfo, kind: &JobKind, engine_root: Option<&Path>) -> Result<CommandSpec, String> {
    let cargo = std::env::var("CARGO").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| "cargo".to_string());
    match kind {
        JobKind::Open => {
            let bin = info.editor_bin.clone().ok_or_else(|| info.editor_problem().unwrap_or_default())?;
            Ok(CommandSpec {
                program: cargo,
                args: vec![
                    "build".into(),
                    "-p".into(),
                    info.package.clone(),
                    "--features".into(),
                    "editor".into(),
                    "--bin".into(),
                    bin.clone(),
                    "--message-format=json".into(),
                ],
                cwd: info.dir.clone(),
                env: progress_env(),
                launch_target: Some(bin),
                launch_args: Vec::new(),
            })
        }
        JobKind::Play => {
            let bin = info.game_bin.clone().ok_or_else(|| {
                format!("`{}` has no game program: add src/main.rs that calls `{}::main()`", info.package, info.lib_name())
            })?;
            Ok(CommandSpec {
                program: cargo,
                args: vec![
                    "build".into(),
                    "-p".into(),
                    info.package.clone(),
                    "--bin".into(),
                    bin.clone(),
                    "--message-format=json".into(),
                ],
                cwd: info.dir.clone(),
                env: progress_env(),
                launch_target: Some(bin),
                launch_args: Vec::new(),
            })
        }
        JobKind::Tool { name } => Err(format!("`{name}` is built with jobs::tool_command")),
        JobKind::Export { platform, release } => {
            let root = engine_root.ok_or("the engine checkout was not found, so piska-export cannot run")?;
            if !paths::is_within(&info.dir, &root.join("games")) {
                return Err(trf!(
                    "piska-export builds games from `games/` of the engine; `{}` is outside it. \
                     Copy or move the project to games/ to export it",
                    info.dir.display()
                ));
            }
            let game = info.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mut args = vec![
                "run".into(),
                "-p".into(),
                "piska-export".into(),
                "--message-format=json".into(),
                "--".into(),
                platform.clone(),
                "--game".into(),
                game,
            ];
            if *release {
                args.push("--release".into());
            }
            Ok(CommandSpec {
                program: cargo,
                args,
                cwd: root.to_path_buf(),
                env: progress_env(),
                launch_target: None,
                launch_args: Vec::new(),
            })
        }
    }
}

/// Сборка программы движка (`piska-editor`, `piska-player`, `piska-modeler`) в корне
/// рабочего пространства: собирается один раз и потом открывает любые проекты.
pub fn tool_command(engine_root: &Path, package: &str, bin: &str, args: Vec<String>) -> CommandSpec {
    let cargo = std::env::var("CARGO").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| "cargo".to_string());
    CommandSpec {
        program: cargo,
        args: {
            let mut build = vec!["build".to_string()];
            // Piska Modeler живёт в собственном рабочем пространстве `modeler/` (отдельный
            // репозиторий, подключённый как подмодуль): его собирают по манифесту оттуда.
            // Каталог сборки общий (`CARGO_TARGET_DIR`), так что exe ложится рядом с остальными.
            let own_workspace = engine_root.join(package.trim_start_matches("piska-")).join("Cargo.toml");
            if package == "piska-modeler" && own_workspace.is_file() {
                build.push("--manifest-path".into());
                build.push(own_workspace.to_string_lossy().into_owned());
            }
            build.extend(["-p".to_string(), package.to_string(), "--bin".to_string(), bin.to_string()]);
            build.push("--message-format=json".into());
            build
        },
        cwd: engine_root.to_path_buf(),
        env: progress_env(),
        launch_target: Some(bin.to_string()),
        launch_args: args,
    }
}

/// Строка вывода фоновой сборки.
#[derive(Debug)]
enum Line {
    Stdout(String),
    Stderr(String),
    /// Поток закрылся.
    Closed,
}

/// Состояние задания.
#[derive(Clone, Debug, PartialEq)]
pub enum JobState {
    Building,
    /// Сборка удалась, программа запущена (pid).
    Launched(u32),
    /// Сборка удалась, запускать нечего (экспорт).
    Succeeded,
    Failed(String),
    Cancelled,
}

/// Сборка проекта с окном хода.
pub struct Job {
    pub project: PathBuf,
    pub title: String,
    pub kind: JobKind,
    pub command: CommandSpec,
    pub progress: Progress,
    pub state: JobState,
    pub started: Instant,
    pub finished_at: Option<Instant>,
    child: Option<Child>,
    lines: Option<Receiver<Line>>,
    open_streams: u8,
    /// Запустить собранную программу после удачной сборки (Prebuild — нет).
    pub launch: bool,
    /// Собирается программа движка, а не проекта: её путь не запоминается как сборка проекта.
    pub tool: bool,
    /// С прошлой проверки пришёл новый вывод.
    dirty: bool,
    /// Завершение задания без процесса (тесты): `Some(успех)`.
    simulated_exit: Option<bool>,
}

impl Job {
    /// Задание без процесса: строки вывода подаются через [`Job::feed_stdout`] /
    /// [`Job::feed_stderr`] (тесты, снимки экрана).
    pub fn detached(project: PathBuf, title: String, kind: JobKind, command: CommandSpec, expected: Option<u32>) -> Self {
        Self {
            project,
            title,
            kind,
            command,
            progress: Progress::new(expected),
            state: JobState::Building,
            started: Instant::now(),
            finished_at: None,
            child: None,
            lines: None,
            open_streams: 0,
            launch: true,
            tool: false,
            dirty: true,
            simulated_exit: None,
        }
    }

    /// Закончить задание без процесса, как будто cargo вышел (тесты, снимки).
    pub fn simulate_exit(&mut self, success: bool) {
        self.simulated_exit = Some(success);
    }

    /// Запустить cargo в фоне.
    pub fn spawn(project: PathBuf, title: String, kind: JobKind, command: CommandSpec, expected: Option<u32>) -> Result<Self, String> {
        let mut process = engine_build::host::command(&command.program);
        process
            .args(&command.args)
            .current_dir(&command.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &command.env {
            process.env(key, value);
        }
        hide_console(&mut process);
        let mut child = process
            .spawn()
            .map_err(|e| trf!("cannot start `{p0}`: {e}. Is Rust (cargo) installed and in PATH?", p0 = command.program, e = e))?;
        let (sender, receiver) = channel();
        if let Some(stdout) = child.stdout.take() {
            pump(stdout, sender.clone(), Line::Stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            pump_stderr(stderr, sender);
        }
        let mut job = Self::detached(project, title, kind, command, expected);
        job.child = Some(child);
        job.lines = Some(receiver);
        job.open_streams = 2;
        Ok(job)
    }

    /// Был ли новый вывод с прошлого вызова.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn feed_stdout(&mut self, line: &str) {
        self.dirty = true;
        if let Some(event) = cargo::parse_stdout(line) {
            self.progress.apply(event);
        }
    }

    pub fn feed_stderr(&mut self, line: &str) {
        self.dirty = true;
        for event in cargo::parse_stderr(line) {
            self.progress.apply(event);
        }
    }

    pub fn is_building(&self) -> bool {
        self.state == JobState::Building
    }

    /// Прошло времени (до конца, если закончилось).
    pub fn elapsed(&self) -> Duration {
        self.finished_at.unwrap_or_else(Instant::now) - self.started
    }

    /// Прочитать накопившийся вывод. Возвращает `Some(успех)`, когда cargo завершился.
    pub fn poll(&mut self) -> Option<bool> {
        if let Some(success) = self.simulated_exit.take() {
            self.finished_at = Some(Instant::now());
            return Some(success);
        }
        if let Some(lines) = &self.lines {
            let mut batch = Vec::new();
            while let Ok(line) = lines.try_recv() {
                batch.push(line);
                if batch.len() > 4000 {
                    break;
                }
            }
            for line in batch {
                match line {
                    Line::Stdout(text) => self.feed_stdout(&text),
                    Line::Stderr(text) => self.feed_stderr(&text),
                    Line::Closed => self.open_streams = self.open_streams.saturating_sub(1),
                }
            }
        }
        if self.open_streams > 0 {
            return None;
        }
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                self.child = None;
                self.finished_at = Some(Instant::now());
                Some(status.success())
            }
            Ok(None) => None,
            Err(_) => {
                self.child = None;
                Some(false)
            }
        }
    }

    /// Остановить сборку (вместе с компиляторами, которые она запустила).
    pub fn cancel(&mut self) {
        if let Some(mut child) = self.child.take() {
            kill_tree(&mut child);
        }
        self.lines = None;
        self.open_streams = 0;
        self.finished_at = Some(Instant::now());
        self.state = JobState::Cancelled;
    }

    /// Пометить законченным.
    pub fn finish(&mut self, state: JobState) {
        self.finished_at.get_or_insert_with(Instant::now);
        self.state = state;
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            kill_tree(&mut child);
        }
    }
}

fn pump(stream: impl Read + Send + 'static, sender: Sender<Line>, wrap: fn(String) -> Line) {
    std::thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.split(b'\n') {
            let Ok(bytes) = line else {
                break;
            };
            if sender.send(wrap(String::from_utf8_lossy(&bytes).into_owned())).is_err() {
                return;
            }
        }
        let _ = sender.send(Line::Closed);
    });
}

/// stderr cargo перерисовывает полосу хода через `\r` без `\n`: читаем кусками.
fn pump_stderr(stream: impl Read + Send + 'static, sender: Sender<Line>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut pending = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buffer[..n]);
                    // Отдаём всё до последнего разделителя.
                    if let Some(cut) = pending.iter().rposition(|b| *b == b'\n' || *b == b'\r') {
                        let chunk: Vec<u8> = pending.drain(..=cut).collect();
                        for line in String::from_utf8_lossy(&chunk).split('\n') {
                            if !line.trim().is_empty() && sender.send(Line::Stderr(line.to_string())).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        }
        if !pending.is_empty() {
            let _ = sender.send(Line::Stderr(String::from_utf8_lossy(&pending).into_owned()));
        }
        let _ = sender.send(Line::Closed);
    });
}

/// Не открывать окно консоли для cargo, когда хаб запущен без консоли (Windows).
fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Завершить процесс и его дочерние (cargo запускает rustc и сборочные скрипты).
pub fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let mut taskkill = engine_build::host::command("taskkill");
        taskkill.args(["/T", "/F", "/PID", &child.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
        hide_console(&mut taskkill);
        let _ = taskkill.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Запущенная хабом программа (редактор или игра).
pub struct Running {
    pub project: PathBuf,
    /// [`paths::key`] каталога проекта.
    pub key: String,
    pub kind: JobKind,
    pub pid: u32,
    pub started: Instant,
    child: Option<Child>,
}

/// Чем закончилась программа.
#[derive(Clone, Debug, PartialEq)]
pub struct Exited {
    pub project: PathBuf,
    pub kind: JobKind,
    pub success: bool,
}

/// Запущенные программы: один проект не открывается в редакторе дважды.
#[derive(Default)]
pub struct Processes {
    running: Vec<Running>,
}

impl Processes {
    /// Запустить собранную программу с рабочим каталогом проекта.
    pub fn launch(&mut self, project: &Path, kind: JobKind, executable: &Path) -> Result<u32, String> {
        self.launch_with(project, kind, executable, &[])
    }

    /// То же с аргументами (`piska-editor --project <каталог>`).
    pub fn launch_with(&mut self, project: &Path, kind: JobKind, executable: &Path, args: &[String]) -> Result<u32, String> {
        let mut command = engine_build::host::command(executable);
        command.args(args).current_dir(project).stdin(Stdio::null());
        let child = command
            .spawn()
            .map_err(|e| trf!("cannot start `{p0}`: {e}", p0 = executable.display(), e = e))?;
        Ok(self.track(project, kind, child))
    }

    /// Следить за уже запущенным процессом.
    pub fn track(&mut self, project: &Path, kind: JobKind, child: Child) -> u32 {
        let pid = child.id();
        self.running.push(Running {
            project: paths::clean(project),
            key: paths::key(project),
            kind,
            pid,
            started: Instant::now(),
            child: Some(child),
        });
        pid
    }

    /// Проект уже открыт (`kind`) — его pid.
    pub fn running(&self, project: &Path, kind: &JobKind) -> Option<u32> {
        if self.running.is_empty() {
            return None;
        }
        self.running_key(&paths::key(project), kind)
    }

    /// То же по готовому ключу пути (без обращения к диску — для кадра интерфейса).
    pub fn running_key(&self, key: &str, kind: &JobKind) -> Option<u32> {
        self.running.iter().find(|r| &r.kind == kind && r.key == key).map(|r| r.pid)
    }

    /// Что-нибудь из проекта запущено.
    pub fn any_running(&self, project: &Path) -> bool {
        if self.running.is_empty() {
            return false;
        }
        let key = paths::key(project);
        self.running.iter().any(|r| r.key == key)
    }

    /// Открытых редакторов.
    pub fn editors(&self) -> usize {
        self.running.iter().filter(|r| r.kind == JobKind::Open).count()
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// Проверить, кто завершился.
    pub fn poll(&mut self) -> Vec<Exited> {
        let mut exited = Vec::new();
        self.running.retain_mut(|running| {
            let Some(child) = running.child.as_mut() else {
                return false;
            };
            match child.try_wait() {
                Ok(None) => true,
                Ok(Some(status)) => {
                    exited.push(Exited {
                        project: running.project.clone(),
                        kind: running.kind.clone(),
                        success: status.success(),
                    });
                    false
                }
                Err(_) => {
                    exited.push(Exited {
                        project: running.project.clone(),
                        kind: running.kind.clone(),
                        success: false,
                    });
                    false
                }
            }
        });
        exited
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Dimension;

    fn info(dir: &Path) -> ProjectInfo {
        ProjectInfo {
            dir: dir.to_path_buf(),
            package: "lantern".into(),
            title: "Lantern".into(),
            description: None,
            dimension: Dimension::TwoD,
            has_cpp: false,
            has_scenario: false,
            editor_bin: Some("lantern-editor".into()),
            game_bin: Some("lantern".into()),
            icon: None,
            standalone: false,
            native: true,
        }
    }

    #[test]
    fn commands_build_the_editor_the_game_or_export() {
        let root = paths::find_engine_root().unwrap();
        let dir = root.join("games").join("lantern");
        let open = command_for(&info(&dir), &JobKind::Open, Some(&root)).unwrap();
        assert!(open.display().ends_with("build -p lantern --features editor --bin lantern-editor --message-format=json"));
        assert_eq!(open.cwd, dir);
        assert_eq!(open.launch_target.as_deref(), Some("lantern-editor"));
        assert!(open.env.iter().any(|(k, v)| k == "CARGO_TERM_PROGRESS_WHEN" && v == "always"));
        assert!(!open.env.iter().any(|(k, _)| k == "CARGO_TARGET_DIR"), "the target dir is inherited, not set");

        let play = command_for(&info(&dir), &JobKind::Play, Some(&root)).unwrap();
        assert!(play.display().ends_with("build -p lantern --bin lantern --message-format=json"));

        let export = command_for(
            &info(&dir),
            &JobKind::Export {
                platform: "web".into(),
                release: true,
            },
            Some(&root),
        )
        .unwrap();
        assert!(export.display().ends_with("run -p piska-export --message-format=json -- web --game lantern --release"));
        assert_eq!(export.cwd, root);
        assert_eq!(export.launch_target, None);

        let mut no_editor = info(&dir);
        no_editor.editor_bin = None;
        assert!(command_for(&no_editor, &JobKind::Open, Some(&root)).unwrap_err().contains("lantern-editor"));
        let outside = info(&std::env::temp_dir().join("elsewhere"));
        let refused = command_for(&outside, &JobKind::Export { platform: "web".into(), release: false }, Some(&root));
        assert!(refused.unwrap_err().contains("games/"));
    }

    #[test]
    fn detached_jobs_take_cargo_output_line_by_line() {
        let command = CommandSpec {
            program: "cargo".into(),
            args: vec![],
            cwd: PathBuf::new(),
            env: vec![],
            launch_target: Some("g-editor".into()),
            launch_args: Vec::new(),
        };
        let mut job = Job::detached(PathBuf::from("g"), "G".into(), JobKind::Open, command, Some(4));
        job.feed_stderr("   Compiling g v0.1.0 (C:/g)");
        job.feed_stdout(r#"{"reason":"compiler-artifact","package_id":"path+file:///C:/g#0.1.0","target":{"name":"g-editor"},"executable":"C:/t/g-editor.exe","fresh":false}"#);
        assert_eq!(job.progress.current.as_deref(), Some("g"));
        assert_eq!(job.progress.executable("g-editor"), Some(&PathBuf::from("C:/t/g-editor.exe")));
        assert_eq!(job.progress.fraction(), Some(0.25));
        assert!(job.is_building());
        assert_eq!(job.poll(), None, "a detached job never finishes by itself");
        job.cancel();
        assert_eq!(job.state, JobState::Cancelled);
    }

    /// Долгий безобидный процесс для проверки слежения.
    fn sleeper(seconds: u32) -> Child {
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("ping");
            c.args(["-n", &(seconds + 1).to_string(), "127.0.0.1"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut c = Command::new("sleep");
            c.arg(seconds.to_string());
            c
        };
        command.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
    }

    #[test]
    fn a_running_project_is_tracked_until_it_exits() {
        let mut processes = Processes::default();
        let dir = std::env::temp_dir().join("piska-hub-running");
        let pid = processes.track(&dir, JobKind::Open, sleeper(1));
        assert_eq!(processes.running(&dir.join(".").join(""), &JobKind::Open), Some(pid));
        assert_eq!(processes.running(&dir, &JobKind::Play), None, "playing is tracked separately");
        assert!(processes.any_running(&dir));
        assert_eq!(processes.editors(), 1);
        assert!(processes.poll().is_empty(), "still running");
        let deadline = Instant::now() + Duration::from_secs(20);
        let exited = loop {
            let exited = processes.poll();
            if !exited.is_empty() || Instant::now() > deadline {
                break exited;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(exited.len(), 1);
        assert_eq!(exited[0].kind, JobKind::Open);
        assert!(processes.is_empty());
        assert_eq!(processes.running(&dir, &JobKind::Open), None);
    }
}
