//! Ход сборки cargo: сообщения `--message-format=json` (stdout) и строки состояния
//! (stderr: `Compiling …`, полоса `Building [==>  ] 12/345: …`, шаги `piska-export`).

use std::collections::VecDeque;
use std::path::PathBuf;

/// Сколько последних строк вывода хранить.
pub const TAIL: usize = 400;

/// Уровень сообщения компилятора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

/// Ошибка или предупреждение компилятора с местом в коде.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Текст, как его печатает компилятор (несколько строк).
    pub rendered: String,
    /// Пакет, при сборке которого случилась ошибка (`engine-render`, `lantern`).
    pub package: Option<String>,
}

impl Diagnostic {
    /// `src/lib.rs:12:5` (или пусто, если места нет).
    pub fn location(&self) -> String {
        match (&self.file, self.line, self.column) {
            (Some(file), Some(line), Some(column)) => format!("{file}:{line}:{column}"),
            (Some(file), Some(line), None) => format!("{file}:{line}"),
            (Some(file), None, _) => file.clone(),
            _ => String::new(),
        }
    }
}

/// Одно событие из вывода cargo.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Единица компиляции готова (`fresh` — не пересобиралась).
    Artifact {
        package: String,
        target: String,
        fresh: bool,
        executable: Option<PathBuf>,
    },
    /// cargo начал компилировать пакет.
    Compiling(String),
    /// Полоса хода сборки cargo: сделано / всего, что компилируется сейчас.
    Progress { done: u32, total: u32, active: Vec<String> },
    /// Шаг `piska-export: [n/m] …`.
    Step { done: u32, total: u32, title: String },
    Diagnostic(Diagnostic),
    /// Сборка закончилась.
    Finished { success: bool },
    /// Прочий текст (вывод программы, сообщения cargo).
    Text(String),
}

/// Идентификатор пакета cargo → имя пакета. Понимает оба формата:
/// `serde 1.0.1 (registry+…)` и `registry+https://…#serde@1.0.1` / `path+file:///…/games/lantern#0.0.1`.
pub fn package_name(id: &str) -> String {
    if let Some((url, fragment)) = id.split_once('#') {
        if let Some((name, _version)) = fragment.split_once('@') {
            return name.to_string();
        }
        return url.trim_end_matches('/').rsplit('/').next().unwrap_or(url).to_string();
    }
    id.split_whitespace().next().unwrap_or(id).to_string()
}

/// Разобрать строку stdout cargo (`--message-format=json`). Не JSON — `Text`.
pub fn parse_stdout(line: &str) -> Option<Event> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('{') {
        return Some(Event::Text(trimmed.to_string()));
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Some(Event::Text(trimmed.to_string()));
    };
    let text = |v: &serde_json::Value| v.as_str().map(str::to_string);
    match value.get("reason").and_then(|r| r.as_str()) {
        Some("compiler-artifact") => {
            let package = value.get("package_id").and_then(|p| p.as_str()).map(package_name).unwrap_or_default();
            let target = value
                .get("target")
                .and_then(|t| t.get("name"))
                .and_then(text)
                .unwrap_or_default();
            Some(Event::Artifact {
                package,
                target,
                fresh: value.get("fresh").and_then(|f| f.as_bool()).unwrap_or(false),
                executable: value.get("executable").and_then(text).map(PathBuf::from),
            })
        }
        Some("compiler-message") => {
            let message = value.get("message")?;
            let level = match message.get("level").and_then(|l| l.as_str()) {
                Some("error") | Some("error: internal compiler error") => Level::Error,
                Some("warning") => Level::Warning,
                _ => return None,
            };
            let spans = message.get("spans").and_then(|s| s.as_array());
            let primary = spans.and_then(|spans| {
                spans
                    .iter()
                    .find(|span| span.get("is_primary").and_then(|p| p.as_bool()) == Some(true))
                    .or_else(|| spans.first())
            });
            let number = |key: &str| primary.and_then(|s| s.get(key)).and_then(|n| n.as_u64()).map(|n| n as u32);
            Some(Event::Diagnostic(Diagnostic {
                level,
                message: message.get("message").and_then(text).unwrap_or_default(),
                file: primary.and_then(|s| s.get("file_name")).and_then(text),
                line: number("line_start"),
                column: number("column_start"),
                rendered: message.get("rendered").and_then(text).unwrap_or_default(),
                package: value.get("package_id").and_then(|p| p.as_str()).map(package_name),
            }))
        }
        Some("build-finished") => Some(Event::Finished {
            success: value.get("success").and_then(|s| s.as_bool()).unwrap_or(false),
        }),
        // build-script-executed и прочее — не нужно.
        _ => None,
    }
}

/// Разобрать кусок stderr cargo. Полоса хода сборки перерисовывается через `\r`,
/// поэтому одна «строка» может содержать несколько состояний.
pub fn parse_stderr(line: &str) -> Vec<Event> {
    let mut events = Vec::new();
    for part in line.split('\r') {
        let part = strip_ansi(part);
        let text = part.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(rest) = text.strip_prefix("Building [") {
            if let Some(event) = parse_progress(rest) {
                events.push(event);
                continue;
            }
        }
        if let Some(rest) = text.strip_prefix("Compiling ") {
            let name = rest.split_whitespace().next().unwrap_or(rest).to_string();
            events.push(Event::Compiling(name));
            events.push(Event::Text(text.to_string()));
            continue;
        }
        if let Some(rest) = text.strip_prefix("piska-export: [") {
            if let Some((counts, title)) = rest.split_once(']') {
                if let Some((done, total)) = counts.split_once('/') {
                    if let (Ok(done), Ok(total)) = (done.trim().parse(), total.trim().parse()) {
                        events.push(Event::Step {
                            done,
                            total,
                            title: title.trim().to_string(),
                        });
                    }
                }
            }
        }
        events.push(Event::Text(text.to_string()));
    }
    events
}

/// `=====>    ] 123/456: serde, wgpu-core, naga` → Progress.
fn parse_progress(rest: &str) -> Option<Event> {
    let (_, after) = rest.split_once(']')?;
    let after = after.trim();
    let (counts, active) = after.split_once(':').unwrap_or((after, ""));
    let (done, total) = counts.trim().split_once('/')?;
    Some(Event::Progress {
        done: done.trim().parse().ok()?,
        total: total.trim().parse().ok()?,
        active: active
            .split(',')
            .map(|name| name.trim().trim_end_matches("...").trim().to_string())
            .filter(|name| !name.is_empty())
            .collect(),
    })
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ … буква
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Состояние сборки для окна хода: что делается, сколько сделано, ошибки, хвост вывода.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    /// Готовых единиц компиляции (из сообщений `compiler-artifact`).
    pub artifacts: u32,
    /// Из них пересобранных.
    pub compiled: u32,
    /// Не пересобиравшихся (`fresh`).
    pub fresh: u32,
    /// Всего единиц: из полосы cargo, иначе из прошлой сборки этого проекта.
    pub total: Option<u32>,
    /// Последний пакет, который начал компилироваться.
    pub current: Option<String>,
    /// cargo ждёт, пока другая сборка освободит каталог сборки или кэш пакетов.
    pub waiting_for_lock: bool,
    /// Что заперто: `build directory` или `package cache`.
    pub lock_what: String,
    /// С какого момента ждём (для «ждёт 2:15»).
    pub lock_since: Option<std::time::Instant>,
    /// Шаг экспорта (`piska-export: [n/m] …`).
    pub step: Option<(u32, u32, String)>,
    pub errors: Vec<Diagnostic>,
    pub warnings: u32,
    pub tail: VecDeque<String>,
    /// Сборка закончилась: удачно или нет.
    pub finished: Option<bool>,
    /// Программы, которые собрала сборка: (имя цели, путь).
    pub executables: Vec<(String, PathBuf)>,
}

impl Progress {
    /// `expected` — сколько единиц было в прошлый раз (если известно).
    pub fn new(expected: Option<u32>) -> Self {
        Self {
            total: expected,
            ..Self::default()
        }
    }

    pub fn apply(&mut self, event: Event) {
        if !matches!(event, Event::Text(_)) {
            self.waiting_for_lock = false;
            self.lock_since = None;
        }
        match event {
            Event::Artifact {
                package,
                target,
                fresh,
                executable,
            } => {
                self.artifacts += 1;
                if fresh {
                    self.fresh += 1;
                } else {
                    self.compiled += 1;
                    self.current = Some(package);
                }
                if let Some(path) = executable {
                    self.executables.push((target, path));
                }
            }
            Event::Compiling(name) => self.current = Some(name),
            Event::Progress { done, total, active } => {
                // Полоса cargo считает и то, что ещё не началось: её «всего» точнее.
                self.total = Some(total.max(self.artifacts));
                if let Some(first) = active.into_iter().next() {
                    self.current = Some(first);
                }
                self.artifacts = self.artifacts.max(done);
            }
            Event::Step { done, total, title } => self.step = Some((done, total, title)),
            Event::Diagnostic(diagnostic) => {
                match diagnostic.level {
                    Level::Error => self.errors.push(diagnostic.clone()),
                    Level::Warning => self.warnings += 1,
                }
                for line in diagnostic.rendered.lines() {
                    self.push_line(line.to_string());
                }
            }
            Event::Finished { success } => self.finished = Some(success),
            Event::Text(text) => {
                if let Some(rest) = text.strip_prefix("Blocking waiting for file lock") {
                    if !self.waiting_for_lock {
                        self.lock_since = Some(std::time::Instant::now());
                    }
                    self.waiting_for_lock = true;
                    self.lock_what = rest.trim().trim_start_matches("on ").to_string();
                } else if text.starts_with("Compiling ") || text.starts_with("Checking ") {
                    self.waiting_for_lock = false;
                    self.lock_since = None;
                }
                self.push_line(text);
            }
        }
    }

    pub fn push_line(&mut self, line: String) {
        if self.tail.len() >= TAIL {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }

    /// Доля сделанного 0…1 (`None` — пока неизвестно сколько всего).
    pub fn fraction(&self) -> Option<f32> {
        if self.finished == Some(true) {
            return Some(1.0);
        }
        if let Some((done, total, _)) = &self.step {
            if *total > 0 {
                return Some((*done as f32 / *total as f32).clamp(0.0, 1.0));
            }
        }
        let total = self.total?;
        if total == 0 {
            return None;
        }
        // Пока сборка идёт, не показываем 100 %: последней единице ещё линковаться.
        Some((self.artifacts as f32 / total as f32).min(0.99))
    }

    /// Программа с таким именем цели, если сборка её сделала.
    pub fn executable(&self, target: &str) -> Option<&PathBuf> {
        self.executables.iter().rev().find(|(name, _)| name == target).map(|(_, path)| path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_ids_of_both_formats_give_the_name() {
        assert_eq!(package_name("serde 1.0.210 (registry+https://github.com/rust-lang/crates.io-index)"), "serde");
        assert_eq!(package_name("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.210"), "serde");
        assert_eq!(package_name("path+file:///C:/engine/games/lantern#0.0.1"), "lantern");
        assert_eq!(package_name("path+file:///C:/engine/games/my-game#my_game@0.1.0"), "my_game");
    }

    #[test]
    fn json_messages_become_events() {
        let artifact = r#"{"reason":"compiler-artifact","package_id":"path+file:///C:/e/games/lantern#0.0.1","manifest_path":"x","target":{"kind":["bin"],"crate_types":["bin"],"name":"lantern-editor","src_path":"x","edition":"2021","doc":true,"doctest":false,"test":true},"profile":{},"features":[],"filenames":["C:/t/debug/lantern-editor.exe"],"executable":"C:/t/debug/lantern-editor.exe","fresh":false}"#;
        assert_eq!(
            parse_stdout(artifact),
            Some(Event::Artifact {
                package: "lantern".into(),
                target: "lantern-editor".into(),
                fresh: false,
                executable: Some(PathBuf::from("C:/t/debug/lantern-editor.exe")),
            })
        );
        let error = r#"{"reason":"compiler-message","package_id":"x#g@0.1.0","manifest_path":"x","target":{"name":"g"},"message":{"rendered":"error[E0425]: cannot find value `speeed`\n --> src/lib.rs:12:9\n","$message_type":"diagnostic","children":[],"code":{"code":"E0425","explanation":null},"level":"error","message":"cannot find value `speeed` in this scope","spans":[{"byte_end":10,"byte_start":4,"column_end":15,"column_start":9,"expansion":null,"file_name":"src/lib.rs","is_primary":true,"label":"not found","line_end":12,"line_start":12,"suggested_replacement":null,"suggestion_applicability":null,"text":[]}]}}"#;
        let Some(Event::Diagnostic(diagnostic)) = parse_stdout(error) else {
            panic!("an error message is a diagnostic");
        };
        assert_eq!(diagnostic.level, Level::Error);
        assert_eq!(diagnostic.location(), "src/lib.rs:12:9");
        assert!(diagnostic.message.contains("speeed"));
        assert_eq!(parse_stdout(r#"{"reason":"build-finished","success":false}"#), Some(Event::Finished { success: false }));
        assert_eq!(parse_stdout(r#"{"reason":"build-script-executed"}"#), None);
        assert_eq!(parse_stdout("hello from the game"), Some(Event::Text("hello from the game".into())));
        assert_eq!(parse_stdout("   "), None);
    }

    #[test]
    fn stderr_status_lines_and_the_progress_bar_are_understood() {
        let events = parse_stderr("   Compiling wgpu-core v23.0.1\r    Building [=====>      ] 123/456: naga, wgpu-core, serde...\r");
        assert_eq!(events[0], Event::Compiling("wgpu-core".into()));
        assert_eq!(
            events[2],
            Event::Progress {
                done: 123,
                total: 456,
                active: vec!["naga".into(), "wgpu-core".into(), "serde".into()],
            }
        );
        let mut progress = Progress::default();
        for event in parse_stderr("    Blocking waiting for file lock on build directory") {
            progress.apply(event);
        }
        assert!(progress.waiting_for_lock);
        assert_eq!(progress.lock_what, "build directory");
        assert!(progress.lock_since.is_some());
        for event in parse_stderr("   Compiling serde v1.0.0") {
            progress.apply(event);
        }
        assert!(!progress.waiting_for_lock);
        let colored = parse_stderr("\u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m engine v0.0.1");
        assert_eq!(colored[0], Event::Compiling("engine".into()));
        let step = parse_stderr("piska-export: [2/5] building wasm");
        assert_eq!(step[0], Event::Step { done: 2, total: 5, title: "building wasm".into() });
    }

    #[test]
    fn progress_counts_artifacts_against_the_best_known_total() {
        let mut progress = Progress::new(Some(10));
        assert_eq!(progress.fraction(), Some(0.0));
        for i in 0..4 {
            progress.apply(Event::Artifact {
                package: format!("p{i}"),
                target: format!("p{i}"),
                fresh: i < 2,
                executable: None,
            });
        }
        assert_eq!((progress.artifacts, progress.compiled), (4, 2));
        assert_eq!(progress.current.as_deref(), Some("p3"));
        assert_eq!(progress.fraction(), Some(0.4));
        progress.apply(Event::Progress { done: 5, total: 20, active: vec!["engine".into()] });
        assert_eq!(progress.fraction(), Some(0.25));
        assert_eq!(progress.current.as_deref(), Some("engine"));
        progress.apply(Event::Artifact {
            package: "g".into(),
            target: "g-editor".into(),
            fresh: false,
            executable: Some(PathBuf::from("g-editor.exe")),
        });
        progress.apply(Event::Diagnostic(Diagnostic {
            level: Level::Warning,
            message: "unused".into(),
            file: None,
            line: None,
            column: None,
            rendered: String::new(),
            package: None,
        }));
        assert_eq!(progress.warnings, 1);
        assert_eq!(progress.executable("g-editor"), Some(&PathBuf::from("g-editor.exe")));
        progress.apply(Event::Finished { success: true });
        assert_eq!(progress.fraction(), Some(1.0));
        // Хвост вывода ограничен.
        for i in 0..(TAIL + 10) {
            progress.push_line(format!("line {i}"));
        }
        assert_eq!(progress.tail.len(), TAIL);
        assert_eq!(progress.tail.back().map(String::as_str), Some(format!("line {}", TAIL + 9).as_str()));
    }
}
