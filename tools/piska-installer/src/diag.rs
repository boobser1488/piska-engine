//! Таблица `doctor` на страницах установщика: та же диагностика, что `piska-export doctor`
//! (файл `doctor.rs` вшит сюда же, см. `lib.rs`), но программы опрашиваются без окна
//! консоли и с PATH, который получился после установки (процесс установщика новых путей
//! ещё не видит). Только чтение: `--version` у найденных программ и просмотр каталогов.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;

use crate::doctor::{self, Check, Os, Status, Sys};

/// Строка таблицы — то, что показывает страница Finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub component: String,
    pub label: String,
    pub status: RowStatus,
    pub version: Option<String>,
    pub path: Option<String>,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Ok,
    Old,
    Missing,
    Info,
}

impl From<&Check> for Row {
    fn from(c: &Check) -> Row {
        Row {
            component: c.component.to_string(),
            label: c.label.clone(),
            status: match c.status {
                Status::Ok => RowStatus::Ok,
                Status::Old => RowStatus::Old,
                Status::Missing => RowStatus::Missing,
                Status::Info => RowStatus::Info,
            },
            version: c.version.clone(),
            path: c.path.clone(),
            detail: c.detail.clone(),
            fix: c.fix.clone(),
        }
    }
}

/// Система для doctor: PATH и переменные — свои (после установки), программы — без консоли.
pub fn system(path: Option<String>, env: BTreeMap<String, String>) -> Sys {
    let path_list: Vec<PathBuf> = match path {
        Some(p) => std::env::split_paths(&p).collect(),
        None => std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(),
    };
    Sys {
        os: Os::current(),
        var: Box::new(move |name| env.get(name).cloned().or_else(|| std::env::var(name).ok()).filter(|v| !v.is_empty())),
        path: path_list,
        run: Box::new(|program, args| {
            let output = engine_build::host::command(program).args(args).stdin(Stdio::null()).output().ok()?;
            if !output.status.success() {
                return None;
            }
            Some(format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
        }),
    }
}

/// Проверки выбранных компонентов (`None` — все, кроме `engine`, которого doctor не знает).
pub fn run(components: Option<&[String]>, path: Option<String>, env: BTreeMap<String, String>) -> Result<Vec<Row>, String> {
    let data = doctor::Data::load().map_err(|e| e.0)?;
    let known: Option<Vec<String>> = components.map(|list| list.iter().filter(|c| doctor::COMPONENTS.contains(&c.as_str())).cloned().collect());
    let sys = system(path, env);
    Ok(doctor::run_checks(&sys, &data, known.as_deref()).iter().map(Row::from).collect())
}

/// Проверка в фоне: результат придёт в канал.
pub fn spawn(components: Option<Vec<String>>, path: Option<String>, env: BTreeMap<String, String>) -> std::sync::mpsc::Receiver<Result<Vec<Row>, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run(components.as_deref(), path, env));
    });
    rx
}
