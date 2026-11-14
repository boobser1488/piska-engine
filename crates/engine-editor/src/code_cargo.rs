//! Ошибки компилятора во вкладке Code: `cargo check --message-format=json` после
//! сохранения (и по F7) и разбор вывода MSVC / GCC / Clang после сборки C++-плагина.
//!
//! * Проверка идёт в фоне, процесс — через `engine_build::host::command` (без окна
//!   консоли). Одновременно — не больше одного cargo на проект: новое сохранение во время
//!   проверки останавливает устаревший процесс и запускает проверку заново.
//! * Сообщения rustc (основной и вторичные диапазоны, заметки, `help:` с
//!   машинно-применимой заменой) превращаются в диагностики с местом в файле, в том числе
//!   в файлах, которые не открыты.

use crate::i18n::{tr_s, trf};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::code_diagnostics::{Diagnostic, LineIndex, Origin, QuickFix, Severity, TextEdit};

/// Место в файле по строке и столбцу (с единицы, столбцы — в символах), как в rustc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawSpan {
    pub file: PathBuf,
    pub line_start: usize,
    pub col_start: usize,
    pub line_end: usize,
    pub col_end: usize,
}

/// Сообщение компилятора до привязки к тексту файла.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RawDiagnostic {
    pub severity: Severity,
    pub code: Option<String>,
    pub message: String,
    pub span: RawSpan,
    pub hint: Option<String>,
    pub notes: Vec<String>,
    /// Машинно-применимые замены: (заголовок, [(место, новый текст)]).
    pub fixes: Vec<(String, Vec<(RawSpan, String)>)>,
}

impl RawDiagnostic {
    /// Привязать к тексту файла (переводы строк — `\n`): байтовые диапазоны.
    pub fn resolve(&self, text: &str, index: &LineIndex) -> Diagnostic {
        let range_of = |s: &RawSpan| {
            let start = index.offset(text, s.line_start.saturating_sub(1), s.col_start.saturating_sub(1));
            let end = index.offset(text, s.line_end.saturating_sub(1), s.col_end.saturating_sub(1));
            start..end.max(start)
        };
        let mut range = range_of(&self.span);
        if range.is_empty() && range.start < text.len() && !text[range.start..].starts_with('\n') {
            range.end = crate::code_diagnostics::ceil_char(text, range.start + 1);
        }
        let mut d = Diagnostic::new(self.severity, Origin::Compiler, self.message.clone(), range);
        d.code = self.code.clone();
        d.hint = self.hint.clone();
        d.notes = self.notes.clone();
        d.line = self.span.line_start.max(1);
        d.column = self.span.col_start.max(1);
        d.fixes = self
            .fixes
            .iter()
            .map(|(title, edits)| QuickFix {
                title: title.clone(),
                edits: edits.iter().map(|(span, text)| TextEdit { range: range_of(span), text: text.clone() }).collect(),
            })
            .collect();
        d
    }
}

fn severity_of(level: &str) -> Option<Severity> {
    match level {
        "error" | "error: internal compiler error" => Some(Severity::Error),
        "warning" => Some(Severity::Warning),
        "note" | "help" => Some(Severity::Info),
        _ => None,
    }
}

/// Найти файл из сообщения rustc: путь абсолютный, или относительно корня рабочего
/// пространства — одного из предков каталога манифеста.
pub(crate) fn resolve_file(name: &str, manifest: Option<&Path>, dirs: &[PathBuf]) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(manifest) = manifest.and_then(Path::parent) {
        bases.extend(manifest.ancestors().map(Path::to_path_buf));
    }
    for dir in dirs {
        bases.extend(dir.ancestors().map(Path::to_path_buf));
    }
    bases.into_iter().map(|base| base.join(path)).find(|p| p.is_file())
}

fn span_of(value: &serde_json::Value, file: PathBuf) -> RawSpan {
    let n = |key: &str| value.get(key).and_then(serde_json::Value::as_u64).unwrap_or(1) as usize;
    RawSpan { file, line_start: n("line_start"), col_start: n("column_start"), line_end: n("line_end"), col_end: n("column_end") }
}

/// Одна строка `--message-format=json` → диагностика (если это сообщение компилятора
/// с местом в файле проекта). `local` решает, чей файл: файлы зависимостей пропускаются.
pub(crate) fn parse_cargo_line(line: &str, dirs: &[PathBuf], local: &dyn Fn(&Path) -> bool) -> Option<RawDiagnostic> {
    if !line.starts_with('{') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("reason")?.as_str()? != "compiler-message" {
        return None;
    }
    let manifest = value.get("manifest_path").and_then(|m| m.as_str()).map(PathBuf::from);
    let message = value.get("message")?;
    let severity = severity_of(message.get("level")?.as_str()?)?;
    let text = message.get("message")?.as_str()?.to_string();
    let code = message.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()).map(str::to_string);
    let resolve = |span: &serde_json::Value| -> Option<RawSpan> {
        let name = span.get("file_name")?.as_str()?;
        let file = resolve_file(name, manifest.as_deref(), dirs)?;
        local(&file).then(|| span_of(span, file))
    };
    let spans: Vec<&serde_json::Value> = message.get("spans").and_then(|s| s.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
    // Основной диапазон; если он в макросе чужого крейта — место вызова макроса.
    let mut primary = None;
    for span in spans.iter().filter(|s| s.get("is_primary").and_then(|p| p.as_bool()) == Some(true)) {
        let mut current = Some(*span);
        while let Some(s) = current {
            if let Some(found) = resolve(s) {
                primary = Some((found, s.get("label").and_then(|l| l.as_str()).map(str::to_string)));
                break;
            }
            current = s.get("expansion").and_then(|e| e.get("span"));
        }
        if primary.is_some() {
            break;
        }
    }
    let (span, label) = primary?;
    let mut d = RawDiagnostic { severity, code, message: text, span, hint: None, notes: Vec::new(), fixes: Vec::new() };
    if let Some(label) = label.filter(|l| !l.is_empty()) {
        d.notes.push(label);
    }
    for span in spans.iter().filter(|s| s.get("is_primary").and_then(|p| p.as_bool()) != Some(true)) {
        if let (Some(found), Some(label)) = (resolve(span), span.get("label").and_then(|l| l.as_str())) {
            let file = found.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            d.notes.push(format!("{file}:{}: {label}", found.line_start));
        }
    }
    for child in message.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
        let level = child.get("level").and_then(|l| l.as_str()).unwrap_or("");
        let child_text = child.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let child_spans: Vec<&serde_json::Value> = child.get("spans").and_then(|s| s.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
        let edits: Vec<(RawSpan, String)> = child_spans
            .iter()
            .filter(|s| s.get("suggestion_applicability").and_then(|a| a.as_str()) == Some("MachineApplicable"))
            .filter_map(|s| Some((resolve(s)?, s.get("suggested_replacement")?.as_str()?.to_string())))
            .collect();
        if !edits.is_empty() {
            let title = if child_text.is_empty() { tr_s("Apply the compiler's suggestion").to_string() } else { capitalize(&child_text) };
            d.fixes.push((title, edits));
        }
        if child_text.is_empty() {
            continue;
        }
        if level == "help" && d.hint.is_none() {
            let replacement = child_spans
                .first()
                .and_then(|s| s.get("suggested_replacement"))
                .and_then(|r| r.as_str())
                .filter(|r| !r.is_empty() && r.len() < 80);
            d.hint = Some(match replacement {
                Some(r) => format!("{child_text}: `{r}`"),
                None => child_text,
            });
        } else {
            d.notes.push(format!("{level}: {child_text}"));
        }
    }
    Some(d)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Строка вывода компилятора C++ → диагностика.
/// MSVC: `file(12,5): error C2065: 'x': undeclared identifier`;
/// GCC / Clang: `file:12:5: error: 'x' was not declared in this scope`.
pub(crate) fn parse_cpp_line(line: &str, dir: Option<&Path>) -> Option<RawDiagnostic> {
    let line = line.trim();
    let line = line.strip_prefix("FAILED: ").unwrap_or(line);
    let resolve = |file: &str| -> PathBuf {
        let path = PathBuf::from(file.trim());
        match dir {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path,
        }
    };
    // MSVC.
    if let Some(close) = line.find("): ") {
        if let Some(open) = line[..close].rfind('(') {
            let numbers: Vec<usize> = line[open + 1..close].split(',').filter_map(|n| n.trim().parse().ok()).collect();
            let rest = &line[close + 3..];
            let (kind, after) = ["fatal error ", "error ", "warning ", "note"]
                .iter()
                .find_map(|k| rest.strip_prefix(k).map(|a| (*k, a)))?;
            if !numbers.is_empty() && open > 0 {
                let (code, message) = match after.split_once(':') {
                    Some((code, message)) if code.trim().chars().all(|c| c.is_ascii_alphanumeric()) => (Some(code.trim().to_string()), message.trim()),
                    _ => (None, after.trim().trim_start_matches(':').trim()),
                };
                let severity = if kind.starts_with("warning") { Severity::Warning } else if kind.starts_with("note") { Severity::Info } else { Severity::Error };
                let line_no = numbers[0];
                let col = numbers.get(1).copied().unwrap_or(1);
                return Some(RawDiagnostic {
                    severity,
                    code: code.filter(|c| !c.is_empty()),
                    message: message.to_string(),
                    span: RawSpan { file: resolve(&line[..open]), line_start: line_no, col_start: col, line_end: line_no, col_end: col },
                    hint: None,
                    notes: Vec::new(),
                    fixes: Vec::new(),
                });
            }
        }
    }
    // GCC / Clang.
    for (marker, severity) in [(": fatal error: ", Severity::Error), (": error: ", Severity::Error), (": warning: ", Severity::Warning), (": note: ", Severity::Info)] {
        let Some(at) = line.find(marker) else { continue };
        let location = &line[..at];
        let message = line[at + marker.len()..].trim();
        let mut parts = location.rsplitn(3, ':');
        let a = parts.next()?;
        let b = parts.next()?;
        let (file, line_no, col) = match (a.parse::<usize>(), b.parse::<usize>()) {
            (Ok(col), Ok(line_no)) => (parts.next()?, line_no, col),
            (Ok(line_no), Err(_)) => (location.rsplit_once(':')?.0, line_no, 1),
            _ => return None,
        };
        // `-Wunused-variable` в конце — код предупреждения.
        let (message, code) = match message.rfind(" [-W") {
            Some(p) if message.ends_with(']') => (message[..p].to_string(), Some(message[p + 2..message.len() - 1].to_string())),
            _ => (message.to_string(), None),
        };
        return Some(RawDiagnostic {
            severity,
            code,
            message,
            span: RawSpan { file: resolve(file), line_start: line_no, col_start: col, line_end: line_no, col_end: col },
            hint: None,
            notes: Vec::new(),
            fixes: Vec::new(),
        });
    }
    None
}

/// Разобрать весь вывод сборки C++: заметки (`note:`) присоединяются к предыдущей ошибке.
pub(crate) fn parse_cpp_output(lines: &[(String, Option<PathBuf>)]) -> Vec<RawDiagnostic> {
    let mut out: Vec<RawDiagnostic> = Vec::new();
    for (line, dir) in lines {
        let Some(d) = parse_cpp_line(line, dir.as_deref()) else { continue };
        if d.severity == Severity::Info {
            if let Some(last) = out.last_mut() {
                let file = d.span.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                last.notes.push(format!("{file}:{}: {}", d.span.line_start, d.message));
                continue;
            }
        }
        // Один и тот же файл:строка:сообщение (MSVC повторяет при нескольких единицах).
        if !out.iter().any(|o| o.span == d.span && o.message == d.message) {
            out.push(d);
        }
    }
    out
}

/// Что и где запускать.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckCommand {
    pub program: String,
    pub args: Vec<String>,
    pub dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    /// Каталоги кода проекта: относительные пути сообщений ищутся от них.
    pub code_dirs: Vec<PathBuf>,
}

/// Состояние фоновой проверки компилятором.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CheckState {
    Idle,
    Running { since: f64 },
    /// Проверка закончилась: сколько секунд шла; `ok` — cargo вернул 0.
    Done { seconds: f32, ok: bool },
    /// Не удалось запустить (нет cargo и т. п.).
    Failed(String),
}

#[derive(Default)]
struct Shared {
    running: bool,
    /// Следующая проверка (сохранили во время текущей): текущая останавливается.
    pending: Option<CheckCommand>,
    cancel: bool,
    /// Результат последней завершённой проверки — забирает интерфейс.
    result: Option<(Vec<RawDiagnostic>, CheckState)>,
}

/// Фоновый `cargo check`: не больше одного процесса, устаревший останавливается.
pub(crate) struct CompilerCheck {
    shared: Arc<Mutex<Shared>>,
    pub state: CheckState,
    started: Option<Instant>,
}

impl Default for CompilerCheck {
    fn default() -> Self {
        Self { shared: Arc::new(Mutex::new(Shared::default())), state: CheckState::Idle, started: None }
    }
}

impl CompilerCheck {
    /// Запустить проверку (или перезапустить: идущая останавливается).
    pub fn request(&mut self, command: CheckCommand, now: f64) {
        self.started = Some(Instant::now());
        self.state = CheckState::Running { since: now };
        let mut shared = self.shared.lock().expect("check state");
        if shared.running {
            shared.pending = Some(command);
            shared.cancel = true;
            return;
        }
        shared.running = true;
        shared.cancel = false;
        drop(shared);
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let mut command = command;
            loop {
                let started = Instant::now();
                let outcome = run_check(&command, &shared);
                let mut guard = shared.lock().expect("check state");
                if let Some(next) = guard.pending.take() {
                    guard.cancel = false;
                    drop(guard);
                    command = next;
                    continue;
                }
                let seconds = started.elapsed().as_secs_f32();
                guard.result = Some(match outcome {
                    Ok((diagnostics, ok)) => (diagnostics, CheckState::Done { seconds, ok }),
                    Err(message) => (Vec::new(), CheckState::Failed(message)),
                });
                guard.running = false;
                break;
            }
        });
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, CheckState::Running { .. })
    }

    /// Забрать результат завершённой проверки.
    pub fn take_result(&mut self) -> Option<Vec<RawDiagnostic>> {
        let (diagnostics, state) = self.shared.lock().ok()?.result.take()?;
        self.state = state;
        Some(diagnostics)
    }

    /// Остановить идущую проверку (редактор закрывается).
    pub fn cancel(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.pending = None;
            shared.cancel = shared.running;
        }
    }

    /// Сколько уже идёт текущая проверка.
    pub fn elapsed(&self) -> Option<Duration> {
        self.is_running().then(|| self.started.map(|s| s.elapsed())).flatten()
    }
}

impl Drop for CompilerCheck {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Выполнить одну проверку. `Ok((диагностики, успех))`; `Err` — процесс не запустился.
fn run_check(command: &CheckCommand, shared: &Arc<Mutex<Shared>>) -> Result<(Vec<RawDiagnostic>, bool), String> {
    let mut process = engine_build::host::command(&command.program);
    process.args(&command.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(dir) = &command.dir {
        process.current_dir(dir);
    }
    process.envs(command.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    log::info!(target: "build", "check: {} {}", command.program, command.args.join(" ")); // i18n-skip: строка журнала
    let mut child: Child = process
        .spawn()
        .map_err(|err| trf!("cannot start `{program}`: {err}. Is it installed and on PATH?", program = command.program, err = err))?;
    let stdout = child.stdout.take();
    let dirs = command.code_dirs.clone();
    let roots = command.code_dirs.clone();
    let reader = std::thread::spawn(move || {
        let local = |path: &Path| roots.iter().any(|root| path.starts_with(root)) || roots.is_empty();
        let mut out = Vec::new();
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(d) = parse_cargo_line(&line, &dirs, &local) {
                    out.push(d);
                }
            }
        }
        out
    });
    let status = loop {
        if shared.lock().map(|s| s.cancel).unwrap_or(false) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Ok((Vec::new(), false));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(err) => return Err(err.to_string()),
        }
    };
    let mut diagnostics = reader.join().unwrap_or_default();
    // rustc повторяет одно сообщение для разных целей (lib и bin) — убрать дубли.
    let mut seen = Vec::new();
    diagnostics.retain(|d| {
        let key = (d.span.clone(), d.message.clone());
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    Ok((diagnostics, status.success()))
}

/// Команда `cargo check` по команде сборки игры (`Relaunch`): тот же пакет и цель.
pub(crate) fn check_from_build(program: &str, build_args: &[String], dir: Option<PathBuf>, code_dirs: Vec<PathBuf>) -> CheckCommand {
    let mut args: Vec<String> = Vec::new();
    for arg in build_args {
        if arg == "build" && args.is_empty() {
            args.push("check".to_string());
        } else if !arg.starts_with("--message-format") {
            args.push(arg.clone());
        }
    }
    if args.is_empty() {
        args.push("check".to_string());
    }
    args.push("--message-format=json".to_string());
    let mut env = Vec::new();
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().and_then(|p| p.parent()).map(Path::to_path_buf)) {
            if dir.join("debug").is_dir() || dir.join("release").is_dir() || dir.file_name().is_some_and(|n| n == "debug" || n == "release") {
                let target = if dir.join("debug").is_dir() || dir.join("release").is_dir() { dir } else { dir.parent().map(Path::to_path_buf).unwrap_or(dir) };
                env.push(("CARGO_TARGET_DIR".to_string(), target.to_string_lossy().into_owned()));
            }
        }
    }
    CheckCommand { program: program.to_string(), args, dir, env, code_dirs }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Записанные сообщения cargo (сокращённые): ошибка с подсказкой-заменой и
    /// предупреждение с машинно-применимым исправлением.
    const E0425: &str = r#"{"reason":"compiler-message","package_id":"path+file:///w/games/demo#0.0.1","manifest_path":"MANIFEST","target":{"name":"demo"},"message":{"rendered":"error[E0425]: cannot find value `sped` in this scope","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"a field with a similar name exists","rendered":null,"spans":[{"byte_end":120,"byte_start":116,"column_end":18,"column_start":14,"expansion":null,"file_name":"src/scripts/mover.rs","is_primary":true,"label":null,"line_end":5,"line_start":5,"suggested_replacement":"self.speed","suggestion_applicability":"MaybeIncorrect","text":[]}]}],"code":{"code":"E0425","explanation":"..."},"level":"error","message":"cannot find value `sped` in this scope","spans":[{"byte_end":120,"byte_start":116,"column_end":18,"column_start":14,"expansion":null,"file_name":"src/scripts/mover.rs","is_primary":true,"label":"not found in this scope","line_end":5,"line_start":5,"suggested_replacement":null,"suggestion_applicability":null,"text":[]}]}}"#;
    const UNUSED: &str = r#"{"reason":"compiler-message","manifest_path":"MANIFEST","message":{"rendered":"warning: unused variable: `x`","children":[{"children":[],"code":null,"level":"note","message":"`#[warn(unused_variables)]` on by default","rendered":null,"spans":[]},{"children":[],"code":null,"level":"help","message":"if this is intentional, prefix it with an underscore","rendered":null,"spans":[{"byte_end":50,"byte_start":49,"column_end":10,"column_start":9,"expansion":null,"file_name":"src/scripts/mover.rs","is_primary":true,"label":null,"line_end":3,"line_start":3,"suggested_replacement":"_x","suggestion_applicability":"MachineApplicable","text":[]}]}],"code":{"code":"unused_variables","explanation":null},"level":"warning","message":"unused variable: `x`","spans":[{"byte_end":50,"byte_start":49,"column_end":10,"column_start":9,"expansion":null,"file_name":"src/scripts/mover.rs","is_primary":true,"label":null,"line_end":3,"line_start":3,"suggested_replacement":null,"suggestion_applicability":null,"text":[]}]}}"#;
    const ARTIFACT: &str = r#"{"reason":"compiler-artifact","package_id":"x","target":{"name":"demo"}}"#;
    const DEP: &str = r#"{"reason":"compiler-message","manifest_path":"MANIFEST","message":{"children":[],"code":null,"level":"warning","message":"in a dependency","spans":[{"column_end":2,"column_start":1,"expansion":null,"file_name":"C:/registry/dep/src/lib.rs","is_primary":true,"label":null,"line_end":1,"line_start":1}]}}"#;

    fn project() -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("piska_cargo_json_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src/scripts")).unwrap();
        let text = "use x;\nfn f() {\n    let x = 1;\n    let y = 2;\n    let _ = (sped, y);\n}\n";
        std::fs::write(dir.join("src/scripts/mover.rs"), text).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        (dir, text.to_string())
    }

    #[test]
    fn recorded_cargo_messages_map_onto_the_file() {
        let (dir, text) = project();
        let manifest = dir.join("Cargo.toml").to_string_lossy().replace('\\', "/");
        let local = |p: &Path| p.starts_with(&dir);
        let parse = |line: &str| parse_cargo_line(&line.replace("MANIFEST", &manifest), &[], &local);
        assert!(parse(ARTIFACT).is_none());
        assert!(parse(DEP).is_none(), "messages in dependencies are skipped");

        let error = parse(E0425).expect("an error");
        assert_eq!(error.severity, Severity::Error);
        assert_eq!(error.code.as_deref(), Some("E0425"));
        assert_eq!(error.span.file, dir.join("src/scripts/mover.rs"));
        assert!(error.hint.as_deref().unwrap().contains("self.speed"));
        assert!(error.fixes.is_empty(), "MaybeIncorrect suggestions are hints, not quick fixes");
        let index = LineIndex::new(&text);
        let d = error.resolve(&text, &index);
        assert_eq!(&text[d.range.clone()], "sped");
        assert_eq!((d.line, d.column), (5, 14));
        assert_eq!(d.origin, Origin::Compiler);

        let warning = parse(UNUSED).expect("a warning");
        assert_eq!(warning.severity, Severity::Warning);
        let d = warning.resolve(&text, &index);
        assert_eq!(&text[d.range.clone()], "x");
        assert_eq!(d.fixes.len(), 1);
        let fixed = crate::code_diagnostics::apply_edits(&text, &d.fixes[0].edits);
        assert!(fixed.contains("let _x = 1;"), "{fixed}");
        assert!(d.notes.iter().any(|n| n.contains("unused_variables")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn msvc_gcc_and_clang_lines_are_parsed() {
        let d = parse_cpp_line(r"C:\p\cpp\spin.cpp(12,5): error C2065: 'speed': undeclared identifier", None).unwrap();
        assert_eq!((d.severity, d.code.as_deref(), d.span.line_start, d.span.col_start), (Severity::Error, Some("C2065"), 12, 5));
        assert_eq!(d.message, "'speed': undeclared identifier");
        assert_eq!(d.span.file, PathBuf::from(r"C:\p\cpp\spin.cpp"));
        let d = parse_cpp_line(r"spin.cpp(7): warning C4244: 'argument': conversion from 'double' to 'float'", Some(Path::new("/p"))).unwrap();
        assert_eq!((d.severity, d.span.line_start, d.span.col_start), (Severity::Warning, 7, 1));
        assert_eq!(d.span.file, Path::new("/p").join("spin.cpp"));
        let d = parse_cpp_line("cpp/spin.cpp:3:10: error: 'foo' was not declared in this scope", None).unwrap();
        assert_eq!((d.span.line_start, d.span.col_start), (3, 10));
        assert_eq!(d.span.file, PathBuf::from("cpp/spin.cpp"));
        let d = parse_cpp_line(r"C:\x\a.cpp:4:2: warning: unused variable 'q' [-Wunused-variable]", None).unwrap();
        assert_eq!(d.code.as_deref(), Some("-Wunused-variable"));
        assert_eq!(d.span.file, PathBuf::from(r"C:\x\a.cpp"));
        assert!(parse_cpp_line("[2/3] Building CXX object", None).is_none());
        assert!(parse_cpp_line("-- Configuring done", None).is_none());
        let all = parse_cpp_output(&[
            ("a.cpp:1:1: error: boom".to_string(), None),
            ("a.cpp:0:1: note: declared here".to_string(), None),
            ("a.cpp:1:1: error: boom".to_string(), None),
        ]);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].notes.len(), 1);
    }

    #[test]
    fn the_check_command_mirrors_the_build_command() {
        let c = check_from_build("cargo", &["build".into(), "-p".into(), "demo".into(), "--bin".into(), "demo-editor".into()], None, Vec::new());
        assert_eq!(c.args[..5], ["check", "-p", "demo", "--bin", "demo-editor"]);
        assert_eq!(c.args.last().unwrap(), "--message-format=json");
    }
}
