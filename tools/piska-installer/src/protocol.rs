//! Машиночитаемый вывод скриптов установки (`install.ps1 -Json`, `install.sh --json`):
//! одна строка stdout — один объект JSON `{"event": "<имя>", …}`. Полное описание —
//! `docs/install.md`, раздел «Машиночитаемый режим».
//!
//! Разбор терпим к мусору: строка, которая не JSON-объект с полем `event`, становится
//! [`Line::Malformed`] (установщик показывает её в журнале как есть), неизвестное событие —
//! [`Event::Unknown`] (новые события старому установщику не мешают), отсутствующие поля —
//! значения по умолчанию.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Версия протокола, которую понимает установщик (`hello.protocol`).
pub const PROTOCOL: u32 = 1;

/// Первое событие: что запущено и где.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hello {
    pub protocol: u32,
    pub script: String,
    pub shell: String,
    pub os: String,
    pub mode: String,
    pub dry_run: bool,
    pub assume_clean: bool,
    pub components: Vec<String>,
    pub install_dir: String,
    pub repo_dir: String,
    pub in_checkout: bool,
    pub cache: String,
    pub state_file: String,
    pub manifest_file: String,
    pub prefs_file: String,
    pub log: Option<String>,
    /// Свободно на диске каталога установки, байт.
    pub free_bytes: Option<u64>,
    pub is_admin: bool,
}

/// Лицензия, которую нужно принять (`plan.licence`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LicenceRef {
    /// `vs-buildtools`, `android-sdk` — то, что принимает `-AcceptLicense`.
    pub id: Option<String>,
    pub name: String,
    pub url: String,
}

/// Скачивание шага (из `packages.toml`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Download {
    pub name: String,
    pub url: String,
    pub size_mb: Option<u64>,
    /// Закреплённая сумма (если есть).
    pub sha256: Option<String>,
    /// Файл суммы у издателя (проверка в момент установки).
    pub sha256_url: Option<String>,
    pub pinned: bool,
    pub pin_status: Option<String>,
    /// Где сумма сверяется дополнительно (index.json zig).
    pub cross_check: Option<String>,
}

/// Строка плана: `run`, `download`, `verify`, `cache`, `extract`, `env`, `file`, `prefs`,
/// `registry`, `shortcut`, `copy`, `note`, `block`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlanLine {
    pub kind: String,
    pub text: String,
}

/// Шаг плана (`plan`): то же, что строки сухого прогона для людей, плюс поля.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlanStep {
    pub n: u32,
    pub total: u32,
    pub id: String,
    pub component: String,
    pub title: String,
    /// Шаг будет пропущен (уже есть / сделан раньше / по просьбе).
    pub skip: bool,
    /// `found`, `done-earlier`, `requested`.
    pub reason: Option<String>,
    /// Что нашла проверка («rustc 1.97.1», путь …).
    pub found: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
    /// Почему шагу нужны права администратора (`None` — не нужны).
    pub elevation: Option<String>,
    pub licence: Option<LicenceRef>,
    /// Скрипт разрешает пропустить шаг (`-SkipSteps`).
    pub skippable: bool,
    pub download_mb: u64,
    pub downloads: Vec<Download>,
    pub lines: Vec<PlanLine>,
}

/// Ход установки (`step`): `start`, `skip`, `done`, `failed`, `declined`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StepEvent {
    pub phase: String,
    pub step: String,
    pub n: u32,
    pub total: u32,
    pub title: String,
    pub component: Option<String>,
    pub reason: Option<String>,
    pub found: Option<String>,
    pub error: Option<String>,
    pub log: Option<String>,
    pub skippable: bool,
    pub elevation_refused: bool,
    /// Linux без pkexec: команда, которую нужно выполнить в терминале.
    pub manual_command: Option<String>,
    pub licence: Option<String>,
}

/// Вопрос скрипта (`prompt`): ответ — одна строка в stdin (`yes` / `no`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Prompt {
    /// `confirm` или `licence`.
    pub kind: String,
    pub step: Option<String>,
    /// Что спрашивается: `prefs-overwrite`, `cargo-config`, `file-associations`, `elevation`,
    /// `uninstall-proceed`, `uninstall-item`, `confirm`.
    pub key: Option<String>,
    pub subject: Option<String>,
    pub question: Option<String>,
    pub default: Option<bool>,
    pub licence: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
}

/// Что удалит удаление (`uninstall-plan`), по манифесту установки.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UninstallItem {
    pub action: String,
    pub target: String,
    /// Скрипт спросит перед этим действием.
    pub ask: bool,
    pub kind: Option<String>,
    pub component: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StepCounts {
    pub total: u32,
    pub done: u32,
    pub skipped: u32,
    pub failed: u32,
}

/// Последнее событие (пишется всегда, и после ошибки).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Summary {
    pub exit: i32,
    pub ok: bool,
    pub dry_run: bool,
    pub failed_step: Option<String>,
    pub error: Option<String>,
    pub manual_command: Option<String>,
    pub reboot: bool,
    pub elevation_refused: bool,
    pub steps: StepCounts,
    pub log: Option<String>,
    pub state_file: Option<String>,
    pub manifest_file: Option<String>,
    pub repo_dir: Option<String>,
    /// PATH после установки (новые программы ещё не видны процессу установщика).
    pub path: Option<String>,
    /// Переменные окружения пользователя, которые знает скрипт.
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    /// Вывод запущенной программы (winget, rustup, cargo …).
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Hello(Box<Hello>),
    Plan(Box<PlanStep>),
    Step(Box<StepEvent>),
    Progress { step: Option<String>, url: String, bytes: u64, total: Option<u64>, estimated: bool },
    Log { level: LogLevel, step: Option<String>, text: String },
    Prompt(Box<Prompt>),
    /// Лицензия принята (параметром или ответом).
    Licence { step: String, licence: Option<String>, name: String, url: String },
    /// Сейчас появится запрос прав администратора (UAC, pkexec): объяснение для человека.
    Elevation { step: Option<String>, reason: String, command: Option<String>, confirmed: bool },
    Reboot { step: String, title: String },
    UninstallPlan { manifest: String, dry_run: bool, items: Vec<UninstallItem> },
    Error { text: String },
    Summary(Box<Summary>),
    /// Событие более новой версии скрипта.
    Unknown { name: String },
}

/// Строка stdout скрипта.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Event(Event),
    /// Не событие: показывается в журнале как есть.
    Malformed(String),
}

fn field_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn field_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    let v = value.get(key)?;
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
}

fn typed<T: for<'de> Deserialize<'de> + Default>(value: serde_json::Value) -> Option<T> {
    serde_json::from_value(value).ok()
}

/// Разобрать одну строку вывода.
pub fn parse_line(line: &str) -> Line {
    let trimmed = line.trim();
    let malformed = || Line::Malformed(line.trim_end().to_string());
    if !trimmed.starts_with('{') {
        return malformed();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return malformed();
    };
    let Some(name) = value.get("event").and_then(|e| e.as_str()).map(str::to_string) else {
        return malformed();
    };
    let event = match name.as_str() {
        "hello" => typed::<Hello>(value).map(|h| Event::Hello(Box::new(h))),
        "plan" => typed::<PlanStep>(value).map(|p| Event::Plan(Box::new(p))),
        "step" => typed::<StepEvent>(value).map(|s| Event::Step(Box::new(s))),
        "progress" => Some(Event::Progress {
            step: field_str(&value, "step"),
            url: field_str(&value, "url").unwrap_or_default(),
            bytes: field_u64(&value, "bytes").unwrap_or(0),
            total: field_u64(&value, "total"),
            estimated: value.get("estimated").and_then(|v| v.as_bool()).unwrap_or(false),
        }),
        "log" => Some(Event::Log {
            level: match field_str(&value, "level").as_deref() {
                Some("warn") => LogLevel::Warn,
                Some("error") => LogLevel::Error,
                Some("output") => LogLevel::Output,
                _ => LogLevel::Info,
            },
            step: field_str(&value, "step"),
            text: field_str(&value, "text").unwrap_or_default(),
        }),
        "prompt" => typed::<Prompt>(value).map(|p| Event::Prompt(Box::new(p))),
        "licence" => Some(Event::Licence {
            step: field_str(&value, "step").unwrap_or_default(),
            licence: field_str(&value, "licence"),
            name: field_str(&value, "name").unwrap_or_default(),
            url: field_str(&value, "url").unwrap_or_default(),
        }),
        "elevation" => Some(Event::Elevation {
            step: field_str(&value, "step"),
            reason: field_str(&value, "reason").unwrap_or_default(),
            command: field_str(&value, "command"),
            confirmed: value.get("confirmed").and_then(|v| v.as_bool()).unwrap_or(false),
        }),
        "reboot" => Some(Event::Reboot { step: field_str(&value, "step").unwrap_or_default(), title: field_str(&value, "title").unwrap_or_default() }),
        "uninstall-plan" => Some(Event::UninstallPlan {
            manifest: field_str(&value, "manifest").unwrap_or_default(),
            dry_run: value.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false),
            items: value.get("items").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        }),
        "error" => Some(Event::Error { text: field_str(&value, "text").unwrap_or_default() }),
        "summary" => typed::<Summary>(value).map(|s| Event::Summary(Box::new(s))),
        _ => Some(Event::Unknown { name }),
    };
    match event {
        Some(event) => Line::Event(event),
        None => malformed(),
    }
}

/// Ответ на вопрос скрипта — строка для его stdin.
pub fn answer(yes: bool) -> &'static str {
    if yes {
        "yes"
    } else {
        "no"
    }
}

/// Размер в людском виде: «1.4 GB», «120 MB», «512 KB».
pub fn human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.0} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        bytes.to_string() + " B"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_parsed_and_garbage_is_kept_as_text() {
        let plan = r#"{"event":"plan","n":3,"total":6,"id":"core.rustup","component":"core","title":"Rust","skip":false,"reason":null,"found":null,"version":null,"path":null,"elevation":null,"licence":null,"skippable":false,"download_mb":14,"downloads":[{"name":"rustup_windows","url":"https://x/rustup-init.exe","size_mb":14,"sha256":null,"sha256_url":"https://x/rustup-init.exe.sha256","pinned":false,"pin_status":null,"cross_check":null}],"lines":[{"kind":"run","text":"rustup-init.exe -y"}]}"#;
        let Line::Event(Event::Plan(step)) = parse_line(plan) else { panic!("plan") };
        assert_eq!((step.n, step.total, step.id.as_str(), step.download_mb), (3, 6, "core.rustup", 14));
        assert!(!step.downloads[0].pinned);
        assert_eq!(step.lines[0].kind, "run");
        assert_eq!(parse_line("WARNING: something odd"), Line::Malformed("WARNING: something odd".into()));
        assert_eq!(parse_line("{not json"), Line::Malformed("{not json".into()));
        assert_eq!(parse_line(r#"{"no":"event"}"#), Line::Malformed(r#"{"no":"event"}"#.into()));
        assert_eq!(parse_line(r#"{"event":"brand-new","x":1}"#), Line::Event(Event::Unknown { name: "brand-new".into() }));
        // Поле не того типа — строка не теряется, а показывается как текст.
        assert!(matches!(parse_line(r#"{"event":"plan","n":"three"}"#), Line::Malformed(_)));
    }

    #[test]
    fn progress_summary_and_prompts() {
        let Line::Event(Event::Progress { bytes, total, .. }) = parse_line(r#"{"event":"progress","step":"a","url":"u","bytes":4000,"total":null}"#) else {
            panic!()
        };
        assert_eq!((bytes, total), (4000, None));
        let Line::Event(Event::Summary(s)) = parse_line(r#"{"event":"summary","exit":1,"ok":false,"failed_step":"core.git","steps":{"total":6,"done":2,"skipped":1,"failed":1},"env":{"PISKA_ZIG":"z"}}"#) else {
            panic!()
        };
        assert_eq!((s.exit, s.failed_step.as_deref(), s.steps.done), (1, Some("core.git"), 2));
        assert_eq!(s.env["PISKA_ZIG"], "z");
        let Line::Event(Event::Prompt(p)) = parse_line(r#"{"event":"prompt","kind":"licence","step":"android.licenses","licence":"android-sdk","name":"the Android SDK","url":"https://d"}"#) else {
            panic!()
        };
        assert_eq!((p.kind.as_str(), p.licence.as_deref()), ("licence", Some("android-sdk")));
        assert_eq!(answer(true), "yes");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(human_bytes(120 * 1024 * 1024), "120 MB");
    }
}
