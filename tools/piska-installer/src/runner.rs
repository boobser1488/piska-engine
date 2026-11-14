//! Запуск скрипта в фоне: дочерний процесс (или поддельный — для тестов), его stdout и
//! stderr читаются в своих потоках, события приходят в канал. Окно никогда не ждёт
//! процесс: [`Session::poll`] только забирает то, что уже пришло.
//!
//! * Ответы на вопросы скрипта — строка в его stdin ([`Session::answer`]).
//! * Отмена — дерево процессов завершается (`taskkill /T` на Windows, группа процессов на
//!   Linux / macOS); файл состояния скрипта позволяет продолжить потом (`-Resume`).
//! * Сессия, которую уронили, не оставляет процесс: `Drop` отменяет.
//! * Подделка [`FakeScript`] проигрывает записанные события — так проверяются сбой посреди
//!   установки, отказ в правах, лицензия, мусор в выводе и зависание без настоящей установки.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::{parse_line, Line};
use crate::scripts::{Interpreter, ScriptOptions, Shell};

/// Что приходит от запущенного скрипта.
#[derive(Clone, Debug, PartialEq)]
pub enum RunMsg {
    Line(Line),
    /// Строка stderr (сообщения PowerShell / bash, ошибки команд).
    Stderr(String),
    /// Процесс закончился: код выхода (`None` — убит / неизвестен).
    Exited(Option<i32>),
    /// Не удалось запустить.
    SpawnFailed(String),
}

enum Control {
    Answer(String),
    Cancel,
}

/// Зачем запущен скрипт.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Проверка «что уже стоит» при запуске (сухой прогон всех компонентов).
    Detect,
    /// Точный план для страницы Review (сухой прогон с выбранными параметрами).
    Plan,
    /// Установка (или сухой прогон, если выбран Dry run).
    Install,
    /// Список того, что удалит удаление (сухой прогон `-Uninstall`).
    UninstallPlan,
    Uninstall,
}

/// Запрос на запуск: зачем, с какими параметрами, в каком окружении.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRequest {
    pub purpose: Purpose,
    pub options: ScriptOptions,
    /// Дополнительные переменные окружения (прокси; в тестах — поддельные каталоги).
    pub env: Vec<(String, String)>,
}

/// Работающий (или закончившийся) скрипт.
pub struct Session {
    rx: Receiver<RunMsg>,
    ctl: Sender<Control>,
    pub request: RunRequest,
    pub started: Instant,
    /// Когда последний раз что-то пришло (для «всё ещё работает…»).
    pub last_activity: Instant,
    pub finished: bool,
    /// Отменена человеком.
    pub cancelled: bool,
}

impl Session {
    fn new(request: RunRequest, rx: Receiver<RunMsg>, ctl: Sender<Control>) -> Session {
        let now = Instant::now();
        Session { rx, ctl, request, started: now, last_activity: now, finished: false, cancelled: false }
    }

    /// Забрать всё, что пришло (не ждёт).
    pub fn poll(&mut self) -> Vec<RunMsg> {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            if matches!(msg, RunMsg::Exited(_) | RunMsg::SpawnFailed(_)) {
                self.finished = true;
            }
            out.push(msg);
        }
        if !out.is_empty() {
            self.last_activity = Instant::now();
        }
        out
    }

    /// Ответить на вопрос скрипта.
    pub fn answer(&self, yes: bool) {
        let _ = self.ctl.send(Control::Answer(crate::protocol::answer(yes).to_string()));
    }

    /// Остановить скрипт со всеми его процессами.
    pub fn cancel(&mut self) {
        if !self.finished {
            self.cancelled = true;
            let _ = self.ctl.send(Control::Cancel);
        }
    }

    /// Дождаться конца (тесты, режим без окна): сообщения собираются в `out`.
    pub fn wait(&mut self, limit: Duration, out: &mut Vec<RunMsg>) -> bool {
        let end = Instant::now() + limit;
        while !self.finished && Instant::now() < end {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(msg) => {
                    if matches!(msg, RunMsg::Exited(_) | RunMsg::SpawnFailed(_)) {
                        self.finished = true;
                    }
                    self.last_activity = Instant::now();
                    out.push(msg);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.finished
    }

    /// Секунд без единого события.
    pub fn quiet_for(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.ctl.send(Control::Cancel);
        }
    }
}

// ---------------------------------------------------------------------------
// Настоящий процесс
// ---------------------------------------------------------------------------

/// Завершить процесс и всех его потомков.
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        let _ = engine_build::host::command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // Скрипт запущен своей группой процессов: сигнал всей группе (sudo / pkexec / curl …).
        let group = format!("-{pid}");
        let _ = std::process::Command::new("kill").args(["-TERM", "--", &group]).status();
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = std::process::Command::new("kill").args(["-KILL", "--", &group]).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Запустить скрипт `interpreter` с параметрами запроса.
pub fn spawn(interpreter: &Interpreter, request: RunRequest) -> Session {
    let (tx, rx) = channel();
    let (ctl_tx, ctl_rx) = channel::<Control>();
    let mut command = engine_build::host::command(&interpreter.program);
    command
        .args(&interpreter.leading)
        .args(request.options.args(interpreter.shell))
        .env_remove("PISKA_INSTALL_NOMAIN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &request.env {
        command.env(key, value);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let shell = interpreter.shell;
    std::thread::spawn(move || {
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = tx.send(RunMsg::SpawnFailed(err.to_string()));
                return;
            }
        };
        let (done_tx, done_rx) = channel::<()>();
        let mut readers = 0;
        if let Some(stdout) = child.stdout.take() {
            let (tx, done) = (tx.clone(), done_tx.clone());
            readers += 1;
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
                    let text = String::from_utf8_lossy(&line).trim_end_matches('\r').to_string();
                    if !text.trim().is_empty() && tx.send(RunMsg::Line(parse_line(&text))).is_err() {
                        break;
                    }
                }
                let _ = done.send(());
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let (tx, done) = (tx.clone(), done_tx.clone());
            readers += 1;
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
                    let text = String::from_utf8_lossy(&line).trim_end().to_string();
                    if !text.trim().is_empty() && tx.send(RunMsg::Stderr(text)).is_err() {
                        break;
                    }
                }
                let _ = done.send(());
            });
        }
        drop(done_tx);
        let mut stdin = child.stdin.take();
        let mut killed = false;
        let status = loop {
            match ctl_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Control::Answer(text)) => {
                    if let Some(input) = stdin.as_mut() {
                        let newline = if shell == Shell::PowerShell { "\r\n" } else { "\n" };
                        let _ = input.write_all(format!("{text}{newline}").as_bytes());
                        let _ = input.flush();
                    }
                }
                Ok(Control::Cancel) | Err(RecvTimeoutError::Disconnected) => {
                    stdin = None;
                    kill_tree(&mut child);
                    killed = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if let Ok(Some(status)) = child.try_wait() {
                break status;
            }
        };
        drop(stdin);
        // Всё, что процесс успел написать, приходит раньше, чем «закончился» (но не дольше 2 с:
        // внук, унаследовавший вывод, может держать его открытым).
        let end = Instant::now() + Duration::from_secs(2);
        for _ in 0..readers {
            let left = end.saturating_duration_since(Instant::now());
            if done_rx.recv_timeout(left).is_err() {
                break;
            }
        }
        let _ = tx.send(RunMsg::Exited(if killed { None } else { status.code() }));
    });
    Session::new(request, rx, ctl_tx)
}

// ---------------------------------------------------------------------------
// Подделка для тестов
// ---------------------------------------------------------------------------

/// Шаг поддельного скрипта.
#[derive(Clone, Debug)]
pub enum FakeStep {
    /// Строка stdout (событие JSON или мусор).
    Out(String),
    Err(String),
    Sleep(Duration),
    /// Ждать ответа на вопрос: «да» — `yes`, «нет» (или конец ввода) — `no`.
    Answer { yes: Vec<FakeStep>, no: Vec<FakeStep> },
    /// Молчать, пока не отменят.
    Hang,
    /// Закончиться с кодом.
    Exit(i32),
}

/// Записанный поток событий.
#[derive(Clone, Debug, Default)]
pub struct FakeScript {
    pub steps: Vec<FakeStep>,
}

impl FakeScript {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn out(mut self, line: impl Into<String>) -> Self {
        self.steps.push(FakeStep::Out(line.into()));
        self
    }

    pub fn err(mut self, line: impl Into<String>) -> Self {
        self.steps.push(FakeStep::Err(line.into()));
        self
    }

    pub fn sleep_ms(mut self, ms: u64) -> Self {
        self.steps.push(FakeStep::Sleep(Duration::from_millis(ms)));
        self
    }

    pub fn answer(mut self, yes: FakeScript, no: FakeScript) -> Self {
        self.steps.push(FakeStep::Answer { yes: yes.steps, no: no.steps });
        self
    }

    pub fn hang(mut self) -> Self {
        self.steps.push(FakeStep::Hang);
        self
    }

    pub fn exit(mut self, code: i32) -> Self {
        self.steps.push(FakeStep::Exit(code));
        self
    }
}

/// Что поддельный скрипт получил: запросы и ответы (проверяют тесты).
#[derive(Clone, Debug, Default)]
pub struct FakeLog {
    pub requests: Vec<RunRequest>,
    pub answers: Vec<String>,
    pub cancelled: u32,
}

/// Запустить подделку.
pub fn spawn_fake(script: FakeScript, request: RunRequest, log: Arc<Mutex<FakeLog>>) -> Session {
    let (tx, rx) = channel();
    let (ctl_tx, ctl_rx) = channel::<Control>();
    if let Ok(mut l) = log.lock() {
        l.requests.push(request.clone());
    }
    std::thread::spawn(move || {
        // Ждать `d` или отмену; `true` — отменили.
        let wait = |d: Option<Duration>, answer: &mut Option<String>| -> bool {
            let end = d.map(|d| Instant::now() + d);
            loop {
                let left = end.map_or(Duration::from_millis(50), |e| e.saturating_duration_since(Instant::now()).min(Duration::from_millis(50)));
                match ctl_rx.recv_timeout(left) {
                    Ok(Control::Cancel) | Err(RecvTimeoutError::Disconnected) => return true,
                    Ok(Control::Answer(a)) => {
                        if let Ok(mut l) = log.lock() {
                            l.answers.push(a.clone());
                        }
                        *answer = Some(a);
                        if d.is_none() {
                            return false;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if end.is_some_and(|e| Instant::now() >= e) {
                    return false;
                }
            }
        };
        let mut queue: std::collections::VecDeque<FakeStep> = script.steps.into();
        let cancelled = |log: &Arc<Mutex<FakeLog>>| {
            if let Ok(mut l) = log.lock() {
                l.cancelled += 1;
            }
        };
        while let Some(step) = queue.pop_front() {
            match step {
                FakeStep::Out(line) => {
                    let _ = tx.send(RunMsg::Line(parse_line(&line)));
                }
                FakeStep::Err(line) => {
                    let _ = tx.send(RunMsg::Stderr(line));
                }
                FakeStep::Sleep(d) => {
                    let mut ignored = None;
                    if wait(Some(d), &mut ignored) {
                        cancelled(&log);
                        let _ = tx.send(RunMsg::Exited(None));
                        return;
                    }
                }
                FakeStep::Answer { yes, no } => {
                    let mut answer = None;
                    if wait(None, &mut answer) {
                        cancelled(&log);
                        let _ = tx.send(RunMsg::Exited(None));
                        return;
                    }
                    let branch = if answer.as_deref().is_some_and(|a| a == "yes") { yes } else { no };
                    for s in branch.into_iter().rev() {
                        queue.push_front(s);
                    }
                }
                FakeStep::Hang => {
                    let mut ignored = None;
                    while !wait(Some(Duration::from_secs(3600)), &mut ignored) {}
                    cancelled(&log);
                    let _ = tx.send(RunMsg::Exited(None));
                    return;
                }
                FakeStep::Exit(code) => {
                    let _ = tx.send(RunMsg::Exited(Some(code)));
                    return;
                }
            }
        }
        let _ = tx.send(RunMsg::Exited(Some(0)));
    });
    Session::new(request, rx, ctl_tx)
}

/// Как запускать скрипты.
#[derive(Clone)]
pub enum Backend {
    /// Настоящие скрипты (вшитые, во временном каталоге).
    Real {
        /// Каталог со скриптами (держится, пока жив установщик).
        scripts: Arc<crate::scripts::Extracted>,
        shell: Shell,
        prefer_pwsh: bool,
    },
    /// Подделка: сценарий по запросу.
    Fake {
        scenario: Arc<dyn Fn(&RunRequest) -> FakeScript + Send + Sync>,
        log: Arc<Mutex<FakeLog>>,
    },
}

impl Backend {
    /// Настоящие скрипты: извлечь во временный каталог.
    pub fn real(prefer_pwsh: bool) -> Result<Backend, String> {
        Ok(Backend::Real { scripts: Arc::new(crate::scripts::extract()?), shell: Shell::native(), prefer_pwsh })
    }

    pub fn fake(scenario: impl Fn(&RunRequest) -> FakeScript + Send + Sync + 'static) -> (Backend, Arc<Mutex<FakeLog>>) {
        let log = Arc::new(Mutex::new(FakeLog::default()));
        (Backend::Fake { scenario: Arc::new(scenario), log: log.clone() }, log)
    }

    pub fn shell(&self) -> Shell {
        match self {
            Backend::Real { shell, .. } => *shell,
            Backend::Fake { .. } => Shell::native(),
        }
    }

    pub fn is_fake(&self) -> bool {
        matches!(self, Backend::Fake { .. })
    }

    /// Каталог извлечённых скриптов (для «Show the log» и справки).
    pub fn scripts_dir(&self) -> Option<PathBuf> {
        match self {
            Backend::Real { scripts, .. } => Some(scripts.dir.clone()),
            Backend::Fake { .. } => None,
        }
    }

    pub fn start(&self, request: RunRequest) -> Session {
        match self {
            Backend::Real { scripts, shell, prefer_pwsh } => match crate::scripts::interpreter(&scripts.script(*shell), *shell, *prefer_pwsh) {
                Ok(interpreter) => spawn(&interpreter, request),
                Err(err) => {
                    let (tx, rx) = channel();
                    let (ctl, _) = channel();
                    let _ = tx.send(RunMsg::SpawnFailed(err));
                    Session::new(request, rx, ctl)
                }
            },
            Backend::Fake { scenario, log } => spawn_fake(scenario(&request), request, log.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RunRequest {
        RunRequest { purpose: Purpose::Install, options: ScriptOptions::default(), env: vec![] }
    }

    #[test]
    fn the_fake_replays_answers_and_cancels() {
        let log = Arc::new(Mutex::new(FakeLog::default()));
        let script = FakeScript::new()
            .out(r#"{"event":"prompt","kind":"confirm"}"#)
            .answer(FakeScript::new().out("yes-branch"), FakeScript::new().out("no-branch"))
            .exit(3);
        let mut s = spawn_fake(script, request(), log.clone());
        s.answer(true);
        let mut got = Vec::new();
        assert!(s.wait(Duration::from_secs(5), &mut got));
        assert_eq!(got.last(), Some(&RunMsg::Exited(Some(3))));
        assert!(got.contains(&RunMsg::Line(Line::Malformed("yes-branch".into()))));
        assert_eq!(log.lock().unwrap().answers, ["yes"]);

        let mut hung = spawn_fake(FakeScript::new().hang(), request(), log.clone());
        let mut none = Vec::new();
        assert!(!hung.wait(Duration::from_millis(200), &mut none), "a hang does not end by itself");
        hung.cancel();
        let mut rest = Vec::new();
        assert!(hung.wait(Duration::from_secs(5), &mut rest));
        assert_eq!(rest.last(), Some(&RunMsg::Exited(None)));
        assert_eq!(log.lock().unwrap().cancelled, 1);
    }

    /// Настоящий дочерний процесс (настоящий интерпретатор, свой маленький скрипт): строки
    /// stdout и stderr, код выхода, отмена вместе с внуком.
    #[test]
    fn a_real_child_is_read_line_by_line_and_can_be_killed() {
        let dir = std::env::temp_dir().join(format!("piska-installer-runner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shell = Shell::native();
        let (talk, sleep, ext) = if shell == Shell::PowerShell {
            (
                "param([switch]$Json)
[Console]::Out.WriteLine('{\"event\":\"log\",\"level\":\"info\",\"text\":\"hi\"}')
[Console]::Out.WriteLine('not json')
[Console]::Error.WriteLine('oops')
exit 7
",
                "param([switch]$Json)
& ping.exe -n 30 127.0.0.1 | Out-Null
",
                "ps1",
            )
        } else {
            (
                "echo '{\"event\":\"log\",\"level\":\"info\",\"text\":\"hi\"}'
echo 'not json'
echo oops >&2
exit 7
",
                "sleep 30
",
                "sh",
            )
        };
        let talk_file = dir.join(format!("talk.{ext}"));
        std::fs::write(&talk_file, talk).unwrap();
        let interp = crate::scripts::interpreter(&talk_file, shell, false).unwrap();
        let mut s = spawn(&interp, request());
        let mut got = Vec::new();
        assert!(s.wait(Duration::from_secs(60), &mut got), "the child ends");
        assert!(got.iter().any(|m| matches!(m, RunMsg::Line(Line::Event(crate::protocol::Event::Log { text, .. })) if text == "hi")), "{got:?}");
        assert!(got.contains(&RunMsg::Line(Line::Malformed("not json".into()))), "{got:?}");
        assert!(got.contains(&RunMsg::Stderr("oops".into())), "{got:?}");
        assert_eq!(got.last(), Some(&RunMsg::Exited(Some(7))));

        let sleep_file = dir.join(format!("sleep.{ext}"));
        std::fs::write(&sleep_file, sleep).unwrap();
        let interp = crate::scripts::interpreter(&sleep_file, shell, false).unwrap();
        let mut s = spawn(&interp, request());
        std::thread::sleep(Duration::from_millis(1500));
        let started = Instant::now();
        s.cancel();
        let mut rest = Vec::new();
        assert!(s.wait(Duration::from_secs(20), &mut rest), "cancel stops the child");
        assert!(started.elapsed() < Duration::from_secs(15));
        assert_eq!(rest.last(), Some(&RunMsg::Exited(None)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
