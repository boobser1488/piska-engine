//! Импорт проекта: предварительный отчёт, настройки и выполнение.
//!
//! Источник — каталог с проектом или файл проекта (`docs/api/project-file.md`). Ничего не
//! пишется на диск, пока пользователь не нажал «Import»: сначала [`analyze`] читает источник
//! и составляет [`Report`] (что внутри, какие проблемы), потом [`check_plan`] добавляет
//! проблемы, зависящие от настроек (имя уже в списке, каталог назначения занят…).
//! [`run_item`] выполняет импорт: готовит проект во временном каталоге рядом с назначением,
//! правит его (имя, зависимость от движка, GUID) и переименовывает на место. Источник не
//! изменяется никогда; отмена и ошибка не оставляют половину проекта.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine_editor::kit::i18n::{n_, tr_s, trf, trn};
use engine_project::{
    collect, merge_tree, read_info, read_manifest_entries, CollectOptions, Conflict, Control, Error as ProjectError, FileRecord, Limits,
    Manifest, Phase, Progress, SkipReason, UnpackOptions,
};

use crate::project;
use crate::templates::{self, title_of};

/// Версия движка, который пишет и читает файлы проектов.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Откуда импортируем.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Каталог проекта.
    Folder(PathBuf),
    /// Файл проекта (`.pks`).
    Archive(PathBuf),
}

impl Source {
    pub fn path(&self) -> &Path {
        match self {
            Source::Folder(p) | Source::Archive(p) => p,
        }
    }

    pub fn is_archive(&self) -> bool {
        matches!(self, Source::Archive(_))
    }
}

/// Насколько серьёзна проблема.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warn,
    /// Импорт невозможен.
    Error,
}

/// Вид проблемы (тесты и интерфейс смотрят на него, а не на текст).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    NotAProject,
    ArchiveDamaged,
    NewerFormat,
    UnsafePaths,
    TooBig,
    CaseCollision,
    EngineVersion,
    ForeignPath,
    PathMissing,
    MissingMeta,
    DuplicateGuids,
    EditionDiffers,
    ToolchainOld,
    ToolchainMissing,
    CmakeMissing,
    Skipped,
    Large,
    HasBuilds,
    // Зависят от настроек:
    NameInvalid,
    NameInList,
    AlreadyListed,
    DestinationExists,
    DestinationInsideSource,
    InsideEngine,
    EnginePathBroken,
    EnginePathRewritten,
    ManifestConverted,
    NoEngineCheckout,
    InPlaceNotEdited,
}

/// Проблема или замечание. Текст строится при показе (язык может смениться).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub level: Level,
    pub code: Code,
    pub args: Vec<String>,
}

impl Problem {
    fn new(level: Level, code: Code, args: &[&str]) -> Self {
        Self {
            level,
            code,
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn arg(&self, i: usize) -> &str {
        self.args.get(i).map(String::as_str).unwrap_or("")
    }

    /// Понятный текст: что не так и что делать.
    pub fn text(&self) -> String {
        let (a, b) = (self.arg(0), self.arg(1));
        match self.code {
            Code::NotAProject => trf!("This is not a project the hub can open: {a}", a = a),
            Code::ArchiveDamaged => trf!("The project file is damaged: {a}", a = a),
            Code::NewerFormat => trf!("The project file was written by a newer version of the engine (format {a}); update the engine to read it", a = a),
            Code::UnsafePaths => trf!("The project file contains an unsafe path and was refused: {a}", a = a),
            Code::TooBig => trf!("The project file is too big to unpack safely: {a}", a = a),
            Code::CaseCollision => trf!("`{a}` and `{b}` differ only in letter case; they cannot both exist on Windows or macOS", a = a, b = b),
            Code::EngineVersion => trf!("Made with engine {a}, this is engine {b}: open the project and check it builds", a = a, b = b),
            Code::ForeignPath => trf!("Dependency `{a}` points to `{b}`, a folder that does not exist on this computer; fix the path in Cargo.toml after the import", a = a, b = b),
            Code::PathMissing => trf!("Dependency `{a}` points to `{b}`, which does not exist next to the project; fix the path in Cargo.toml after the import", a = a, b = b),
            Code::MissingMeta => trn!(
                "{n} asset has no .meta file (for example {b}); the editor will give it a new GUID, so scenes that reference it by GUID may lose the link",
                "{n} assets have no .meta file (for example {b}); the editor will give them new GUIDs, so scenes that reference them by GUID may lose the link",
                a.parse::<usize>().unwrap_or(0),
                b = b
            ),
            Code::DuplicateGuids => trn!(
                "{n} asset GUID is used by more than one file (for example {b}); regenerate GUIDs or fix the copies",
                "{n} asset GUIDs are used by more than one file (for example {b}); regenerate GUIDs or fix the copies",
                a.parse::<usize>().unwrap_or(0),
                b = b
            ),
            Code::EditionDiffers => trf!("The project uses Rust edition {a}, the engine uses {b}; this builds, but language features differ", a = a, b = b),
            Code::ToolchainOld => trf!("The project needs Rust {a}, installed is {b}; run `rustup update`", a = a, b = b),
            Code::ToolchainMissing => tr_s("Rust was not found (`rustc` is not on PATH); install it with rustup to build the project").to_string(),
            Code::CmakeMissing => tr_s("The C++ part uses CMake, but `cmake` was not found on PATH; install CMake to build it").to_string(),
            Code::Skipped => trn!(
                "{n} folder or file is left out: build output, caches, links, version control",
                "{n} folders and files are left out: build output, caches, links, version control",
                a.parse::<usize>().unwrap_or(0)
            ),
            Code::Large => trf!("{a} to copy; this may take a while", a = a),
            Code::HasBuilds => trf!("Contains exported builds ({a}); they are left out unless you include them", a = a),
            Code::NameInvalid => a.to_string(),
            Code::NameInList => trf!("A project named `{a}` is already in the list ({b}); rename this one or regenerate GUIDs", a = a, b = b),
            Code::AlreadyListed => tr_s("This folder is already in the project list").to_string(),
            Code::DestinationExists => trf!("`{a}` already exists and has {b} files; clashing files follow the conflict setting below", a = a, b = b),
            Code::DestinationInsideSource => trf!("The destination `{a}` is inside the source project; choose another folder", a = a),
            Code::InsideEngine => trf!("`{a}` is inside the engine checkout; projects there must go to its games/ folder, otherwise choose a folder outside the engine", a = a),
            Code::EnginePathBroken => trf!("The engine dependency `{a}` will not resolve from the new location; choose “Use this engine”", a = a),
            Code::EnginePathRewritten => trf!("The engine dependency will be changed from `{a}` to `{b}`", a = a, b = b),
            Code::ManifestConverted => trf!("Cargo.toml is converted: {a}", a = a),
            Code::NoEngineCheckout => tr_s("The engine checkout was not found, so the dependency cannot be rewritten; it is kept as it is").to_string(),
            Code::InPlaceNotEdited => tr_s("Used in place: nothing is copied or edited, the project is only added to the list").to_string(),
        }
    }
}

/// Что внутри.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub files: u64,
    pub bytes: u64,
    /// Размер ассетов (без `.meta`).
    pub assets_bytes: u64,
    pub scenes: u64,
    pub prefabs: u64,
    pub rust_scripts: u64,
    pub cpp_files: u64,
    pub scenarios: u64,
    pub tests: u64,
    /// Файлов и размер `tests/`, `cpp/`, `dist/`, настроек IDE — для переключателей частей.
    pub tests_bytes: u64,
    pub cpp_bytes: u64,
    pub builds_files: u64,
    pub builds_bytes: u64,
    pub ide_files: u64,
}

fn count_path(counts: &mut Counts, path: &str, size: u64) {
    counts.files += 1;
    counts.bytes += size;
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if path.starts_with("assets/") && ext != "meta" {
        counts.assets_bytes += size;
    }
    match ext.as_str() {
        "scene" => counts.scenes += 1,
        "prefab" => counts.prefabs += 1,
        "scenario" => counts.scenarios += 1,
        _ => {}
    }
    if path.starts_with("src/scripts/") && ext == "rs" && !path.ends_with("/mod.rs") {
        counts.rust_scripts += 1;
    }
    if path.starts_with("cpp/") {
        counts.cpp_bytes += size;
        if matches!(ext.as_str(), "cpp" | "cc" | "cxx" | "hpp" | "h") {
            counts.cpp_files += 1;
        }
    }
    if path.starts_with("tests/") {
        counts.tests_bytes += size;
        if ext == "rs" {
            counts.tests += 1;
        }
    }
    if path.starts_with("dist/") {
        counts.builds_files += 1;
        counts.builds_bytes += size;
    }
    if path.starts_with(".vscode/") || path.starts_with(".idea/") || path.starts_with(".vs/") || path.ends_with(".code-workspace") {
        counts.ide_files += 1;
    }
}

/// Что известно о проекте из `Cargo.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// `path` зависимости `engine`.
    pub engine_path: Option<String>,
    pub edition: Option<String>,
    pub rust_version: Option<String>,
    /// Свой `[workspace]` (проект вне рабочего пространства движка).
    pub workspace_table: bool,
    /// Поля вида `version.workspace = true`.
    pub inherits_workspace: bool,
    /// C++ собирается через CMake (`cpp/CMakeLists.txt`).
    pub cmake: bool,
}

/// Результат разбора источника.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub source: Source,
    /// Имя пакета.
    pub package: String,
    pub title: String,
    /// Версия движка, с которой сделан проект (если известна).
    pub made_with: Option<String>,
    pub counts: Counts,
    pub facts: Facts,
    /// Проблемы, которые не зависят от настроек.
    pub problems: Vec<Problem>,
    /// Что не войдёт: каталоги и файлы (путь, причина).
    pub skipped: Vec<(String, String)>,
    pub dimension: Option<&'static str>,
}

impl Report {
    /// Есть ли блокирующая проблема.
    pub fn blocked(&self) -> bool {
        self.problems.iter().any(|p| p.level == Level::Error)
    }
}

/// Окружение для разбора.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub engine_root: Option<PathBuf>,
}

fn why(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::Derived => n_("build output, cache or version control"),
        SkipReason::Junk => n_("temporary or system file"),
        SkipReason::Gitignore => n_("excluded by .gitignore"),
        SkipReason::Link => n_("link (not followed)"),
        SkipReason::Option(_) => n_("left out by the settings"),
        SkipReason::Name => n_("unusable file name"),
    }
}

/// Что нашлось в `Cargo.toml`: имя, заголовок, факты и зависимости-пути не на движок.
type ParsedManifest = (String, Option<String>, Facts, Vec<(String, String)>);

fn parse_facts(text: &str) -> Result<ParsedManifest, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
    let package = table.get("package").and_then(|p| p.as_table()).ok_or("there is no [package] table")?;
    let name = package.get("name").and_then(|n| n.as_str()).ok_or("[package] has no name")?.to_string();
    let title = package
        .get("metadata")
        .and_then(|m| m.get("piska"))
        .and_then(|p| p.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let mut facts = Facts {
        workspace_table: table.contains_key("workspace"),
        ..Facts::default()
    };
    let is_inherit = |v: &toml::Value| v.as_table().is_some_and(|t| t.get("workspace").and_then(|w| w.as_bool()) == Some(true));
    facts.edition = package.get("edition").and_then(|e| e.as_str()).map(str::to_string);
    facts.rust_version = package.get("rust-version").and_then(|e| e.as_str()).map(str::to_string);
    facts.inherits_workspace = ["version", "edition", "license"].iter().any(|k| package.get(*k).is_some_and(is_inherit));
    if facts.edition.is_none() && package.get("edition").is_some_and(is_inherit) {
        facts.edition = Some("2021".to_string());
    }
    let mut paths = Vec::new();
    for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let Some(deps) = table.get(section).and_then(|d| d.as_table()) else {
            continue;
        };
        for (dep, spec) in deps {
            if is_inherit(spec) {
                facts.inherits_workspace = true;
            }
            let Some(path) = spec.as_table().and_then(|t| t.get("path")).and_then(|p| p.as_str()) else {
                continue;
            };
            if dep == "engine" {
                facts.engine_path = Some(path.to_string());
            }
            if !engine_crate_path(path) {
                paths.push((dep.clone(), path.to_string()));
            }
        }
    }
    Ok((name, title, facts, paths))
}

/// `…/crates/engine`, `../../crates/engine-cpp-build` — путь к крейту движка.
fn engine_crate_path(path: &str) -> bool {
    let normal = path.replace('\\', "/");
    let suffix = normal.rsplit_once("crates/").map(|(_, s)| s).unwrap_or("");
    !suffix.is_empty() && suffix.starts_with("engine")
}

fn is_absolute_like(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.as_bytes().get(1) == Some(&b':')
}

/// GUID из текста `.meta`: `guid: "…"`.
fn guid_of(text: &str) -> Option<&str> {
    let at = text.find("guid:")?;
    let rest = &text[at + 5..];
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')?;
    let guid = &rest[start..start + end];
    (!guid.is_empty()).then_some(guid)
}

/// Установленный `rustc`: (major, minor) или `None`.
fn installed_rust() -> Option<(u32, u32)> {
    static CACHE: std::sync::OnceLock<Option<(u32, u32)>> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        let mut command = engine_build::host::command("rustc");
        command.arg("--version");
        crate::tasks::hide_window(&mut command);
        let output = command.output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        parse_version(text.split_whitespace().nth(1)?)
    })
}

fn parse_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let base = dir.join(program);
        base.is_file() || (cfg!(windows) && (base.with_extension("exe").is_file() || base.with_extension("bat").is_file() || base.with_extension("cmd").is_file()))
    })
}

fn engine_edition(root: Option<&Path>) -> Option<String> {
    let text = fs::read_to_string(root?.join("Cargo.toml")).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table.get("workspace")?.get("package")?.get("edition")?.as_str().map(str::to_string)
}

/// Разобрать источник. Читает, ничего не пишет. Отмена возвращает отчёт с тем, что успело
/// прочитаться (вызывающий её не показывает).
pub fn analyze(source: &Source, env: &Environment, control: &Control) -> Report {
    match source {
        Source::Folder(dir) => analyze_folder(dir, env, control),
        Source::Archive(file) => analyze_archive(file, env, control),
    }
}

fn blank(source: &Source) -> Report {
    let name = source
        .path()
        .file_stem()
        .or_else(|| source.path().file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Report {
        source: source.clone(),
        package: name.clone(),
        title: name,
        made_with: None,
        counts: Counts::default(),
        facts: Facts::default(),
        problems: Vec::new(),
        skipped: Vec::new(),
        dimension: None,
    }
}

/// Проблемы, которые видны из текста `Cargo.toml` и списка файлов.
#[allow(clippy::too_many_arguments)]
fn common_checks(report: &mut Report, manifest_text: &str, env: &Environment, dir: Option<&Path>, rels: &HashSet<String>, metas: &[(String, String)]) {
    match parse_facts(manifest_text) {
        Ok((name, title, mut facts, foreign)) => {
            report.package = name;
            report.title = title.unwrap_or_else(|| report.package.clone());
            facts.cmake = rels.contains("cpp/CMakeLists.txt");
            for (dep, path) in &foreign {
                if is_absolute_like(path) {
                    if !Path::new(path).exists() {
                        report.problems.push(Problem::new(Level::Warn, Code::ForeignPath, &[dep, path]));
                    }
                } else if let Some(dir) = dir {
                    if !dir.join(path).exists() {
                        report.problems.push(Problem::new(Level::Warn, Code::PathMissing, &[dep, path]));
                    }
                }
            }
            if let (Some(edition), Some(engine)) = (&facts.edition, engine_edition(env.engine_root.as_deref())) {
                if *edition != engine {
                    report.problems.push(Problem::new(Level::Info, Code::EditionDiffers, &[edition, &engine]));
                }
            }
            match installed_rust() {
                None => report.problems.push(Problem::new(Level::Warn, Code::ToolchainMissing, &[])),
                Some(have) => {
                    if let Some(need) = facts.rust_version.as_deref().and_then(parse_version) {
                        if need > have {
                            report.problems.push(Problem::new(
                                Level::Warn,
                                Code::ToolchainOld,
                                &[&format!("{}.{}", need.0, need.1), &format!("{}.{}", have.0, have.1)],
                            ));
                        }
                    }
                }
            }
            if facts.cmake && !on_path("cmake") {
                report.problems.push(Problem::new(Level::Warn, Code::CmakeMissing, &[]));
            }
            report.facts = facts;
        }
        Err(reason) => report.problems.push(Problem::new(Level::Error, Code::NotAProject, &[&format!("Cargo.toml: {reason}")])),
    }
    // Версия движка.
    if let Some(made) = report.made_with.clone() {
        if made != ENGINE_VERSION {
            report.problems.push(Problem::new(Level::Warn, Code::EngineVersion, &[&made, ENGINE_VERSION]));
        }
    }
    // Файлы `.meta`: нет у ассета, дубли GUID.
    let mut missing: Vec<&String> = rels
        .iter()
        .filter(|p| {
            p.starts_with("assets/")
                && !p.ends_with(".meta")
                && !p.ends_with("/project.settings")
                && !p.rsplit('/').next().is_some_and(|n| n.starts_with('.'))
                && !rels.contains(&format!("{p}.meta"))
        })
        .collect();
    if !missing.is_empty() {
        missing.sort();
        report.problems.push(Problem::new(Level::Warn, Code::MissingMeta, &[&missing.len().to_string(), missing[0]]));
    }
    let mut by_guid: HashMap<&str, Vec<&str>> = HashMap::new();
    for (path, text) in metas {
        if let Some(guid) = guid_of(text) {
            by_guid.entry(guid).or_default().push(path);
        }
    }
    let mut dupes: Vec<(&&str, &Vec<&str>)> = by_guid.iter().filter(|(_, v)| v.len() > 1).collect();
    if !dupes.is_empty() {
        dupes.sort();
        report.problems.push(Problem::new(
            Level::Warn,
            Code::DuplicateGuids,
            &[&dupes.len().to_string(), &format!("{} / {}", dupes[0].1[0], dupes[0].1[1])],
        ));
    }
    // Совпадения с точностью до регистра.
    let mut lower: HashMap<String, &str> = HashMap::new();
    let mut sorted: Vec<&String> = rels.iter().collect();
    sorted.sort();
    for path in sorted {
        if let Some(other) = lower.insert(path.to_lowercase(), path) {
            let level = if cfg!(unix) && !cfg!(target_os = "macos") { Level::Warn } else { Level::Error };
            report.problems.push(Problem::new(level, Code::CaseCollision, &[other, path]));
            break;
        }
    }
    if report.counts.bytes > 1 << 30 {
        report.problems.push(Problem::new(Level::Info, Code::Large, &[&human(report.counts.bytes)]));
    }
}

/// Размер для людей: `12.3 MB`.
pub fn human(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        trf!("{value} GB", value = format!("{:.1}", b / (KB * KB * KB)))
    } else if b >= KB * KB {
        trf!("{value} MB", value = format!("{:.1}", b / (KB * KB)))
    } else if b >= KB {
        trf!("{value} KB", value = format!("{:.0}", b / KB))
    } else {
        trf!("{bytes} B", bytes = bytes)
    }
}

fn analyze_folder(dir: &Path, env: &Environment, control: &Control) -> Report {
    let source = Source::Folder(dir.to_path_buf());
    let mut report = blank(&source);
    let info = match project::inspect(dir) {
        Ok(info) => info,
        Err(err) => {
            report.problems.push(Problem::new(Level::Error, Code::NotAProject, &[&err.to_string()]));
            return report;
        }
    };
    report.package = info.package.clone();
    report.title = info.title.clone();
    report.dimension = info.dimension.label();
    let options = CollectOptions {
        builds: true,
        ..CollectOptions::default()
    };
    let collected = match collect(dir, &options, control) {
        Ok(c) => c,
        Err(err) => {
            report.problems.push(Problem::new(Level::Error, Code::NotAProject, &[&err.to_string()]));
            return report;
        }
    };
    for file in &collected.files {
        count_path(&mut report.counts, &file.rel, file.size);
    }
    let rels: HashSet<String> = collected.files.iter().map(|f| f.rel.clone()).collect();
    let mut metas = Vec::new();
    for file in collected.files.iter().filter(|f| f.rel.ends_with(".meta") && f.size < 64 * 1024).take(100_000) {
        if control.is_cancelled() {
            return report;
        }
        if let Ok(text) = fs::read_to_string(&file.path) {
            metas.push((file.rel.clone(), text));
        }
    }
    report.made_with = engine_version_from_lock(dir);
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    common_checks(&mut report, &manifest, env, Some(dir), &rels, &metas);
    let skipped_dirs = collected.skipped.iter().filter(|s| !matches!(s.reason, SkipReason::Option(_))).count();
    if skipped_dirs > 0 {
        report.problems.push(Problem::new(Level::Info, Code::Skipped, &[&skipped_dirs.to_string()]));
    }
    report.skipped = collected
        .skipped
        .iter()
        .filter(|s| !matches!(s.reason, SkipReason::Option(_)))
        .take(200)
        .map(|s| (if s.dir { format!("{}/", s.rel) } else { s.rel.clone() }, why(s.reason).to_string()))
        .collect();
    if report.counts.builds_files > 0 {
        report.problems.push(Problem::new(Level::Info, Code::HasBuilds, &[&human(report.counts.builds_bytes)]));
    }
    report
}

/// Версия пакета `engine` из `Cargo.lock` проекта — с какой версией движка его собирали.
fn engine_version_from_lock(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("Cargo.lock")).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table
        .get("package")?
        .as_array()?
        .iter()
        .filter_map(|p| p.as_table())
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("engine"))
        .and_then(|p| p.get("version")?.as_str().map(str::to_string))
}

fn analyze_archive(file: &Path, env: &Environment, control: &Control) -> Report {
    let source = Source::Archive(file.to_path_buf());
    let mut report = blank(&source);
    let info = match read_info(file) {
        Ok(info) => info,
        Err(err) => {
            let (level, code) = match &err {
                ProjectError::UnsupportedVersion { .. } => (Level::Error, Code::NewerFormat),
                _ => (Level::Error, Code::ArchiveDamaged),
            };
            let arg = match &err {
                ProjectError::UnsupportedVersion { found, .. } => found.to_string(),
                other => other.to_string(),
            };
            report.problems.push(Problem::new(level, code, &[&arg]));
            return report;
        }
    };
    let manifest: &Manifest = &info.manifest;
    report.package = manifest.project.name.clone();
    report.title = manifest.project.title.clone();
    if !manifest.writer.engine_version.is_empty() {
        report.made_with = Some(manifest.writer.engine_version.clone());
    }
    for record in &manifest.files {
        count_path(&mut report.counts, &record.path, record.size);
    }
    if let Err(err) = manifest.validate(&Limits::default()) {
        let code = match &err {
            ProjectError::UnsafePath { .. } => Code::UnsafePaths,
            ProjectError::TooManyFiles { .. } | ProjectError::TooLarge { .. } => Code::TooBig,
            ProjectError::CaseCollision { a, b } => {
                let level = if cfg!(unix) && !cfg!(target_os = "macos") { Level::Warn } else { Level::Error };
                report.problems.push(Problem::new(level, Code::CaseCollision, &[a, b]));
                return finish_archive(report, &info, env, control, file);
            }
            _ => Code::ArchiveDamaged,
        };
        report.problems.push(Problem::new(Level::Error, code, &[&err.to_string()]));
        return report;
    }
    finish_archive(report, &info, env, control, file)
}

fn finish_archive(mut report: Report, info: &engine_project::Info, env: &Environment, control: &Control, file: &Path) -> Report {
    let rels: HashSet<String> = info.manifest.files.iter().map(|f| f.path.clone()).collect();
    let wanted = |r: &FileRecord| r.path == "Cargo.toml" || (r.path.ends_with(".meta") && r.size < 64 * 1024);
    match read_manifest_entries(file, info, &wanted, 1 << 20, control) {
        Ok(entries) => {
            let mut manifest_text = String::new();
            let mut metas = Vec::new();
            for (path, bytes) in entries {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if path == "Cargo.toml" {
                    manifest_text = text;
                } else {
                    metas.push((path, text));
                }
            }
            if manifest_text.is_empty() {
                report.problems.push(Problem::new(Level::Error, Code::NotAProject, &["the file has no Cargo.toml"]));
            } else {
                common_checks(&mut report, &manifest_text, env, None, &rels, &metas);
                if !rels.contains("src/lib.rs") {
                    report.problems.push(Problem::new(Level::Error, Code::NotAProject, &["the file has no src/lib.rs"]));
                }
            }
        }
        Err(ProjectError::Cancelled) => {}
        Err(err) => report.problems.push(Problem::new(Level::Error, Code::ArchiveDamaged, &[&err.to_string()])),
    }
    if report.counts.builds_files > 0 {
        report.problems.push(Problem::new(Level::Info, Code::HasBuilds, &[&human(report.counts.builds_bytes)]));
    }
    report
}

// ---------------------------------------------------------------------------
// Настройки
// ---------------------------------------------------------------------------

/// Что делать с проектом-папкой.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Скопировать в каталог проектов (по умолчанию).
    Copy,
    /// Не копировать: добавить в список как есть.
    InPlace,
}

/// Зависимость от движка.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineDep {
    /// Указать на этот движок (по умолчанию).
    Rewrite,
    /// Оставить `Cargo.toml` как есть.
    Keep,
}

/// Настройки импорта (общие для всех отмеченных проектов, кроме имени).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportSettings {
    /// Каталог, в котором появится каталог проекта.
    pub location: PathBuf,
    pub mode: Mode,
    /// Переименовать пакет и всё, что на него ссылается.
    pub rename_inside: bool,
    pub engine: EngineDep,
    /// Что делать с файлами, которые уже есть в каталоге назначения.
    pub conflict: Conflict,
    /// Новые GUID ассетов (для копии рядом с оригиналом).
    pub regenerate_guids: bool,
    pub tests: bool,
    pub cpp: bool,
    pub builds: bool,
    pub ide: bool,
    /// После импорта: открыть в редакторе, собрать редактор в фоне, запустить тесты.
    pub open: bool,
    pub build: bool,
    pub test: bool,
}

impl ImportSettings {
    pub fn new(location: PathBuf) -> Self {
        Self {
            location,
            mode: Mode::Copy,
            rename_inside: true,
            engine: EngineDep::Rewrite,
            conflict: Conflict::KeepBoth,
            regenerate_guids: false,
            tests: true,
            cpp: true,
            builds: false,
            ide: false,
            open: false,
            build: false,
            test: false,
        }
    }
}

/// Куда попадёт проект: `<location>/<name>`.
pub fn destination(settings: &ImportSettings, name: &str) -> PathBuf {
    settings.location.join(name)
}

/// Известные хабу проекты: пакет и каталог.
#[derive(Clone, Debug, Default)]
pub struct Known {
    pub projects: Vec<(String, PathBuf)>,
}

/// Проблемы, которые зависят от настроек. `name` — имя пакета после импорта.
pub fn check_plan(report: &Report, name: &str, settings: &ImportSettings, env: &Environment, known: &Known) -> Vec<Problem> {
    let mut out = Vec::new();
    let archive = report.source.is_archive();
    let in_place = settings.mode == Mode::InPlace && !archive;
    if in_place {
        out.push(Problem::new(Level::Info, Code::InPlaceNotEdited, &[]));
        if known.projects.iter().any(|(_, d)| crate::paths::same(d, report.source.path())) {
            out.push(Problem::new(Level::Info, Code::AlreadyListed, &[]));
        }
        return out;
    }
    if let Err(err) = templates::validate_name(name) {
        out.push(Problem::new(Level::Error, Code::NameInvalid, &[&err.to_string()]));
        return out;
    }
    if let Some((_, dir)) = known
        .projects
        .iter()
        .find(|(package, dir)| package == name && !crate::paths::same(dir, &destination(settings, name)))
    {
        out.push(Problem::new(Level::Warn, Code::NameInList, &[name, &dir.display().to_string()]));
    }
    let dest = destination(settings, name);
    if settings.location.as_os_str().is_empty() {
        out.push(Problem::new(Level::Error, Code::NameInvalid, &[tr_s("Choose a destination folder")]));
        return out;
    }
    if let Source::Folder(src) = &report.source {
        if crate::paths::is_within(&dest, src) {
            out.push(Problem::new(Level::Error, Code::DestinationInsideSource, &[&dest.display().to_string()]));
        }
    }
    let in_workspace = env
        .engine_root
        .as_ref()
        .is_some_and(|root| crate::paths::same(&settings.location, &root.join("games")));
    if let Some(root) = &env.engine_root {
        if !in_workspace && crate::paths::is_within(&settings.location, root) {
            out.push(Problem::new(Level::Error, Code::InsideEngine, &[&settings.location.display().to_string()]));
        }
    }
    if let Ok(entries) = fs::read_dir(&dest) {
        let count = entries.flatten().count();
        if count > 0 {
            out.push(Problem::new(Level::Warn, Code::DestinationExists, &[&dest.display().to_string(), &count.to_string()]));
        }
    } else if dest.is_file() {
        out.push(Problem::new(Level::Error, Code::DestinationExists, &[&dest.display().to_string(), "1"]));
    }
    // Зависимость от движка.
    match (&settings.engine, &env.engine_root) {
        (EngineDep::Rewrite, Some(_)) => {
            if let Some(old) = &report.facts.engine_path {
                let new = engine_crates(env.engine_root.as_deref(), in_workspace);
                let new_engine = format!("{new}/engine");
                if old.replace('\\', "/") != new_engine {
                    out.push(Problem::new(Level::Info, Code::EnginePathRewritten, &[old, &new_engine]));
                }
            }
        }
        (EngineDep::Rewrite, None) => out.push(Problem::new(Level::Warn, Code::NoEngineCheckout, &[])),
        (EngineDep::Keep, _) => {
            if let Some(old) = &report.facts.engine_path {
                let resolved = if is_absolute_like(old) { PathBuf::from(old) } else { dest.join(old) };
                if !resolved.join("Cargo.toml").is_file() {
                    out.push(Problem::new(Level::Warn, Code::EnginePathBroken, &[old]));
                }
            }
        }
    }
    // Вид `Cargo.toml` меняется между «член рабочего пространства» и «отдельный проект».
    if settings.engine == EngineDep::Rewrite && env.engine_root.is_some() {
        if in_workspace && report.facts.workspace_table {
            out.push(Problem::new(Level::Info, Code::ManifestConverted, &[tr_s("the project's own [workspace] is removed because it joins the engine's games/")]));
        } else if !in_workspace && report.facts.inherits_workspace {
            out.push(Problem::new(Level::Info, Code::ManifestConverted, &[tr_s("settings inherited from the engine workspace are written out, and an own [workspace] is added")]));
        }
    }
    out
}

/// `../../crates` внутри `games/`, иначе абсолютный путь к `crates` движка.
pub fn engine_crates(engine_root: Option<&Path>, in_workspace: bool) -> String {
    if in_workspace {
        "../../crates".to_string()
    } else {
        engine_root
            .map(|root| format!("{}/crates", root.display().to_string().replace('\\', "/")))
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Выполнение
// ---------------------------------------------------------------------------

/// Ошибка импорта: что случилось и что сделать.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Project(#[from] ProjectError),
    #[error("cannot access `{path}`: {reason}")]
    Io { path: String, reason: String },
    #[error("the project after the changes does not pass the hub's check: {0}")]
    Invalid(String),
    #[error("Cargo.toml could not be edited: {0}")]
    Manifest(String),
    #[error("{0}")]
    Refused(String),
    #[error("cancelled")]
    Cancelled,
}

impl ImportError {
    fn io(path: &Path, e: impl std::fmt::Display) -> Self {
        ImportError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        }
    }

    pub fn is_cancel(&self) -> bool {
        matches!(self, ImportError::Cancelled | ImportError::Project(ProjectError::Cancelled))
    }
}

/// Что было изменено (текст строится при показе).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeCode {
    PackageRenamed,
    TitleChanged,
    BinRenamed,
    FileRenamed,
    SourcesUpdated,
    DependencyRewritten,
    WorkspaceRemoved,
    WorkspaceAdded,
    InheritedWritten,
    GuidsRegenerated,
    LockCopied,
    KeptBoth,
    Skipped,
    LeftOut,
    InPlace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub code: ChangeCode,
    pub a: String,
    pub b: String,
}

impl Change {
    fn new(code: ChangeCode, a: impl Into<String>, b: impl Into<String>) -> Self {
        Self {
            code,
            a: a.into(),
            b: b.into(),
        }
    }

    pub fn text(&self) -> String {
        let (a, b) = (self.a.as_str(), self.b.as_str());
        match self.code {
            ChangeCode::PackageRenamed => trf!("Package renamed: `{a}` → `{b}`", a = a, b = b),
            ChangeCode::TitleChanged => trf!("Title changed: “{a}” → “{b}”", a = a, b = b),
            ChangeCode::BinRenamed => trf!("Program renamed in Cargo.toml: `{a}` → `{b}`", a = a, b = b),
            ChangeCode::FileRenamed => trf!("File renamed: {a} → {b}", a = a, b = b),
            ChangeCode::SourcesUpdated => trn!(
                "{n} source file updated for the new name",
                "{n} source files updated for the new name",
                a.parse::<usize>().unwrap_or(0)
            ),
            ChangeCode::DependencyRewritten => trf!("Dependency path rewritten: `{a}` → `{b}`", a = a, b = b),
            ChangeCode::WorkspaceRemoved => tr_s("The project's own [workspace] and profiles were removed: it is a member of the engine workspace now").to_string(),
            ChangeCode::WorkspaceAdded => tr_s("An own [workspace] and build profiles were added: the project builds on its own now").to_string(),
            ChangeCode::InheritedWritten => trf!("Inherited setting written out: {a}", a = a),
            ChangeCode::GuidsRegenerated => trn!(
                "{n} asset GUID regenerated; references in scenes, prefabs and settings were updated",
                "{n} asset GUIDs regenerated; references in scenes, prefabs and settings were updated",
                a.parse::<usize>().unwrap_or(0)
            ),
            ChangeCode::LockCopied => tr_s("Cargo.lock copied from the engine so the dependencies keep their tested versions").to_string(),
            ChangeCode::KeptBoth => trf!("Kept both: {a} was saved as {b}", a = a, b = b),
            ChangeCode::Skipped => trf!("Skipped, already exists: {a}", a = a),
            ChangeCode::LeftOut => {
                let part = match a {
                    "tests" => tr_s("tests (tests/)"),
                    "cpp" => tr_s("C++ sources (cpp/)"),
                    "builds" => tr_s("exported builds (dist/)"),
                    "ide" => tr_s("editor and IDE settings"),
                    other => other,
                };
                trf!("Left out: {a}", a = part)
            }
            ChangeCode::InPlace => tr_s("Added to the list in place: no files were copied or changed").to_string(),
        }
    }
}

/// Как проект оказался на месте.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Used {
    Copied,
    Unpacked,
    InPlace,
}

/// Результат импорта одного проекта.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub name: String,
    pub title: String,
    pub dir: PathBuf,
    pub source: Source,
    pub used: Used,
    pub files: u64,
    pub bytes: u64,
    pub changes: Vec<Change>,
    /// Пропущено: путь и причина.
    pub skipped: Vec<(String, String)>,
    /// У проекта есть собственный код (общий редактор его не соберёт без компиляции).
    pub native: bool,
    pub has_cpp: bool,
    /// Что пользователю стоит сделать дальше.
    pub notes: Vec<Problem>,
}

/// Окружение выполнения.
#[derive(Clone, Debug, Default)]
pub struct RunEnv {
    pub engine_root: Option<PathBuf>,
}

/// Один проект к импорту.
#[derive(Clone, Debug)]
pub struct PlanItem {
    pub source: Source,
    /// Имя пакета после импорта.
    pub name: String,
    /// Отчёт разбора (прежнее имя, заголовок, вид `Cargo.toml`).
    pub report: Report,
}

/// Этап для строки состояния.
pub type Stage<'a> = &'a mut dyn FnMut(&str, &Progress);

fn check(control: &Control) -> Result<(), ImportError> {
    if control.is_cancelled() {
        Err(ImportError::Cancelled)
    } else {
        Ok(())
    }
}

/// Выполнить импорт одного проекта.
pub fn run_item(item: &PlanItem, settings: &ImportSettings, env: &RunEnv, control: &Control, stage: Stage) -> Result<Outcome, ImportError> {
    let mut progress = Progress {
        phase: Phase::Scanning,
        files_done: 0,
        files_total: item.report.counts.files,
        bytes_done: 0,
        bytes_total: item.report.counts.bytes,
        current: String::new(),
    };
    if item.report.blocked() {
        let why = item.report.problems.iter().find(|p| p.level == Level::Error).map(Problem::text).unwrap_or_default();
        return Err(ImportError::Refused(why));
    }
    let in_place = settings.mode == Mode::InPlace && !item.source.is_archive();
    if in_place {
        let dir = crate::paths::clean(item.source.path());
        let info = project::inspect(&dir).map_err(|e| ImportError::Invalid(e.to_string()))?;
        return Ok(Outcome {
            name: info.package.clone(),
            title: info.title.clone(),
            dir,
            source: item.source.clone(),
            used: Used::InPlace,
            files: item.report.counts.files,
            bytes: item.report.counts.bytes,
            changes: vec![Change::new(ChangeCode::InPlace, "", "")],
            skipped: Vec::new(),
            native: info.native,
            has_cpp: info.has_cpp,
            notes: Vec::new(),
        });
    }
    templates::validate_name(&item.name).map_err(|e| ImportError::Refused(e.to_string()))?;
    let dest = destination(settings, &item.name);
    if let Source::Folder(src) = &item.source {
        if crate::paths::is_within(&dest, src) {
            return Err(ImportError::Refused(Problem::new(Level::Error, Code::DestinationInsideSource, &[&dest.display().to_string()]).text()));
        }
    }
    let in_workspace = env
        .engine_root
        .as_ref()
        .is_some_and(|root| crate::paths::same(&settings.location, &root.join("games")));
    if let Some(root) = &env.engine_root {
        if !in_workspace && crate::paths::is_within(&settings.location, root) {
            return Err(ImportError::Refused(Problem::new(Level::Error, Code::InsideEngine, &[&settings.location.display().to_string()]).text()));
        }
    }
    fs::create_dir_all(&settings.location).map_err(|e| ImportError::io(&settings.location, e))?;
    // Временный каталог рядом с назначением: переименование на тот же диск мгновенное.
    let staging = settings.location.join(format!(".{}.importing-{}", item.name, std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let result = fill_and_publish(item, settings, env, control, stage, &mut progress, &staging, &dest, in_workspace);
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn fill_and_publish(
    item: &PlanItem,
    settings: &ImportSettings,
    env: &RunEnv,
    control: &Control,
    stage: Stage,
    progress: &mut Progress,
    staging: &Path,
    dest: &Path,
    in_workspace: bool,
) -> Result<Outcome, ImportError> {
    fs::create_dir_all(staging).map_err(|e| ImportError::io(staging, e))?;
    let mut changes: Vec<Change> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    let include = {
        let (tests, cpp, builds, ide) = (settings.tests, settings.cpp, settings.builds, settings.ide);
        move |path: &str| -> bool {
            !((!tests && path.starts_with("tests/"))
                || (!cpp && path.starts_with("cpp/"))
                || (!builds && path.starts_with("dist/"))
                || (!ide && (path.starts_with(".vscode/") || path.starts_with(".idea/") || path.starts_with(".vs/") || path.ends_with(".code-workspace"))))
        }
    };
    // 1. Содержимое.
    stage(tr_s("Copying files"), progress);
    let (files, bytes, used) = match &item.source {
        Source::Folder(src) => {
            let options = CollectOptions {
                tests: settings.tests,
                cpp: settings.cpp,
                builds: settings.builds,
                ide: settings.ide,
                gitignore: true,
            };
            let collected = collect(src, &options, control)?;
            for s in &collected.skipped {
                let label = if s.dir { format!("{}/", s.rel) } else { s.rel.clone() };
                match s.reason {
                    SkipReason::Option(part) => {
                        let change = Change::new(ChangeCode::LeftOut, part, "");
                        if !changes.contains(&change) {
                            changes.push(change);
                        }
                    }
                    reason => skipped.push((label, why(reason).to_string())),
                }
            }
            progress.phase = Phase::Working;
            progress.files_total = collected.files.len() as u64;
            progress.bytes_total = collected.bytes;
            let mut buffer = vec![0u8; 256 * 1024];
            for file in &collected.files {
                check(control)?;
                progress.current = file.rel.clone();
                stage(tr_s("Copying files"), progress);
                let target = staging.join(&file.rel);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| ImportError::io(parent, e))?;
                }
                copy_file(&file.path, &target, &mut buffer, control, &mut |n| {
                    progress.bytes_done += n;
                })?;
                progress.files_done += 1;
            }
            (collected.files.len() as u64, collected.bytes, Used::Copied)
        }
        Source::Archive(file) => {
            progress.phase = Phase::Working;
            let options = UnpackOptions {
                limits: Limits::default(),
                conflict: Conflict::Fail,
                include: Some(Arc::new(include)),
            };
            let label = tr_s("Unpacking files");
            let summary = {
                let mut forward = |p: &Progress| stage(label, p);
                engine_project::unpack(file, staging, &options, control, &mut forward)?
            };
            progress.files_done = summary.written.len() as u64;
            progress.bytes_done = summary.bytes;
            for (path, _) in &summary.skipped {
                let part = if path.starts_with("tests/") {
                    "tests"
                } else if path.starts_with("cpp/") {
                    "cpp"
                } else if path.starts_with("dist/") {
                    "builds"
                } else {
                    "ide"
                };
                let change = Change::new(ChangeCode::LeftOut, part, "");
                if !changes.contains(&change) {
                    changes.push(change);
                }
            }
            (summary.written.len() as u64, summary.bytes, Used::Unpacked)
        }
    };
    check(control)?;

    // 2. Правки.
    stage(tr_s("Updating the project"), progress);
    let old_name = item.report.package.clone();
    let new_name = item.name.clone();
    let renaming = settings.rename_inside && old_name != new_name;
    let rewriting = settings.engine == EngineDep::Rewrite && env.engine_root.is_some();
    if renaming || rewriting {
        let manifest_path = staging.join("Cargo.toml");
        let text = fs::read_to_string(&manifest_path).map_err(|e| ImportError::io(&manifest_path, e))?;
        let edit = ManifestEdit {
            rename_to: renaming.then_some(new_name.as_str()),
            engine_crates: rewriting.then(|| engine_crates(env.engine_root.as_deref(), in_workspace)),
            in_workspace: rewriting.then_some(in_workspace),
            engine_root: env.engine_root.as_deref(),
        };
        let (edited, manifest_changes) = rewrite_manifest(&text, &edit)?;
        if edited != text {
            fs::write(&manifest_path, edited).map_err(|e| ImportError::io(&manifest_path, e))?;
        }
        changes.extend(manifest_changes);
    }
    if renaming {
        let old_title = item.report.title.clone();
        let auto_title = old_title == title_of(&old_name);
        let new_title = if auto_title { title_of(&new_name) } else { old_title.clone() };
        let (updated, renamed) = rename_sources(staging, &old_name, &new_name, &old_title, &new_title, control)?;
        for (from, to) in renamed {
            changes.push(Change::new(ChangeCode::FileRenamed, from, to));
        }
        if updated > 0 {
            changes.push(Change::new(ChangeCode::SourcesUpdated, updated.to_string(), ""));
        }
    }
    if rewriting && !in_workspace && !staging.join("Cargo.lock").exists() {
        if let Some(root) = &env.engine_root {
            if fs::copy(root.join("Cargo.lock"), staging.join("Cargo.lock")).is_ok() {
                changes.push(Change::new(ChangeCode::LockCopied, "", ""));
            }
        }
    }
    if settings.regenerate_guids {
        stage(tr_s("Regenerating GUIDs"), progress);
        let count = regenerate_guids(staging, control)?;
        if count > 0 {
            changes.push(Change::new(ChangeCode::GuidsRegenerated, count.to_string(), ""));
        }
    }
    check(control)?;

    // 3. Проверка тем же разбором, что у списка проектов.
    stage(tr_s("Checking the result"), progress);
    let info = project::inspect(staging).map_err(|e| ImportError::Invalid(e.to_string()))?;

    // 4. На место.
    stage(tr_s("Moving into place"), progress);
    check(control)?;
    let existing_files = fs::read_dir(dest).map(|d| d.flatten().count()).unwrap_or(0);
    if dest.is_file() {
        return Err(ImportError::Refused(format!("`{}` is a file; choose another name or folder", dest.display())));
    }
    if existing_files == 0 {
        if dest.is_dir() {
            fs::remove_dir(dest).map_err(|e| ImportError::io(dest, e))?;
        }
        if let Err(first) = fs::rename(staging, dest) {
            // Антивирус или индексатор иногда держит каталог — переносим по файлам.
            fs::create_dir_all(dest).map_err(|e| ImportError::io(dest, format!("{first}; {e}")))?;
            merge_tree(staging, dest, Conflict::Fail, control)?;
        }
    } else {
        let merged = merge_tree(staging, dest, settings.conflict, control)?;
        for (from, to) in merged.renamed {
            changes.push(Change::new(ChangeCode::KeptBoth, from, to));
        }
        for path in merged.skipped {
            changes.push(Change::new(ChangeCode::Skipped, path, ""));
        }
    }
    Ok(Outcome {
        name: info.package.clone(),
        title: info.title.clone(),
        dir: crate::paths::clean(dest),
        source: item.source.clone(),
        used,
        files,
        bytes,
        changes,
        skipped,
        native: info.native,
        has_cpp: info.has_cpp,
        notes: Vec::new(),
    })
}

fn copy_file(from: &Path, to: &Path, buffer: &mut [u8], control: &Control, tick: &mut dyn FnMut(u64)) -> Result<(), ImportError> {
    let mut source = fs::File::open(from).map_err(|e| ImportError::io(from, e))?;
    let mut target = fs::File::create(to).map_err(|e| ImportError::io(to, e))?;
    loop {
        let n = source.read(buffer).map_err(|e| ImportError::io(from, e))?;
        if n == 0 {
            break;
        }
        target.write_all(&buffer[..n]).map_err(|e| ImportError::io(to, e))?;
        tick(n as u64);
        check(control)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Правка Cargo.toml
// ---------------------------------------------------------------------------

/// Что править в `Cargo.toml`.
pub struct ManifestEdit<'a> {
    /// Новое имя пакета.
    pub rename_to: Option<&'a str>,
    /// Новый путь к `crates` движка (зависимости на крейты движка).
    pub engine_crates: Option<String>,
    /// Проект окажется в `games/` (`Some(true)`) или вне движка (`Some(false)`).
    pub in_workspace: Option<bool>,
    pub engine_root: Option<&'a Path>,
}

const STANDALONE_TAIL: &str = "[workspace]\n\n[profile.dev]\nopt-level = 1\ndebug = \"line-tables-only\"\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n";

fn manifest_err(e: impl std::fmt::Display) -> ImportError {
    ImportError::Manifest(e.to_string())
}

/// Переписать `Cargo.toml`, не трогая остальной текст и комментарии. Возвращает новый текст
/// и список изменений.
pub fn rewrite_manifest(text: &str, edit: &ManifestEdit) -> Result<(String, Vec<Change>), ImportError> {
    use toml_edit::{value, DocumentMut, Item, Value};
    let mut doc: DocumentMut = text.parse().map_err(manifest_err)?;
    let mut changes = Vec::new();
    let old_name = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| ImportError::Manifest("[package] has no name".to_string()))?
        .to_string();

    if let Some(new_name) = edit.rename_to.filter(|n| *n != old_name) {
        let old_lib = old_name.replace('-', "_");
        let new_lib = new_name.replace('-', "_");
        doc["package"]["name"] = value(new_name);
        changes.push(Change::new(ChangeCode::PackageRenamed, &old_name, new_name));
        // Программы: `<имя>-editor` и `<имя>` (необязательные `[[bin]]`).
        if let Some(bins) = doc.get_mut("bin").and_then(|b| b.as_array_of_tables_mut()) {
            for bin in bins.iter_mut() {
                let Some(name) = bin.get("name").and_then(|n| n.as_str()).map(str::to_string) else {
                    continue;
                };
                let renamed = if name == old_name {
                    Some(new_name.to_string())
                } else {
                    name.strip_prefix(&format!("{old_name}-")).map(|rest| format!("{new_name}-{rest}"))
                };
                if let Some(renamed) = renamed {
                    bin["name"] = value(renamed.as_str());
                    changes.push(Change::new(ChangeCode::BinRenamed, name, renamed));
                }
            }
        }
        if let Some(lib) = doc.get_mut("lib").and_then(|l| l.as_table_mut()) {
            if lib.get("name").and_then(|n| n.as_str()) == Some(old_lib.as_str()) {
                lib["name"] = value(new_lib.as_str());
            }
        }
        // Заголовок и описание, если они были получены из имени автоматически.
        let old_title = title_of(&old_name);
        let new_title = title_of(new_name);
        if let Some(piska) = doc
            .get_mut("package")
            .and_then(|p| p.get_mut("metadata"))
            .and_then(|m| m.get_mut("piska"))
            .and_then(|p| p.as_table_like_mut())
        {
            if piska.get("title").and_then(|t| t.as_str()) == Some(old_title.as_str()) {
                piska.insert("title", value(new_title.as_str()));
                changes.push(Change::new(ChangeCode::TitleChanged, &old_title, &new_title));
            }
        }
        if let Some(description) = doc.get("package").and_then(|p| p.get("description")).and_then(|d| d.as_str()).map(str::to_string) {
            if description.starts_with(&old_title) {
                doc["package"]["description"] = value(description.replacen(&old_title, &new_title, 1));
            }
        }
    }

    if let Some(crates) = &edit.engine_crates {
        for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
            let Some(deps) = doc.get_mut(section).and_then(|d| d.as_table_like_mut()) else {
                continue;
            };
            for (_, item) in deps.iter_mut() {
                let spec: Option<&mut dyn toml_edit::TableLike> = match item {
                    Item::Value(Value::InlineTable(t)) => Some(t),
                    Item::Table(t) => Some(t),
                    _ => None,
                };
                let Some(spec) = spec else {
                    continue;
                };
                let Some(old) = spec.get("path").and_then(|p| p.as_str()).map(str::to_string) else {
                    continue;
                };
                if !engine_crate_path(&old) {
                    continue;
                }
                let normal = old.replace('\\', "/");
                let suffix = normal.rsplit_once("crates/").map(|(_, s)| s).unwrap_or_default();
                let new = format!("{crates}/{suffix}");
                if new != old {
                    spec.insert("path", value(new.as_str()));
                    changes.push(Change::new(ChangeCode::DependencyRewritten, old, new));
                }
            }
        }
    }

    match edit.in_workspace {
        Some(true) => {
            let mut removed = false;
            for key in ["workspace", "profile"] {
                if doc.remove(key).is_some() {
                    removed = true;
                }
            }
            if removed {
                changes.push(Change::new(ChangeCode::WorkspaceRemoved, "", ""));
            }
        }
        Some(false) => {
            let inherited = inline_workspace_values(&mut doc, edit, &mut changes)?;
            let _ = inherited;
            if !doc.contains_key("workspace") {
                let tail: DocumentMut = STANDALONE_TAIL.parse().map_err(manifest_err)?;
                for (key, item) in tail.iter() {
                    if key == "profile" && doc.contains_key("profile") {
                        continue;
                    }
                    doc.insert(key, item.clone());
                }
                changes.push(Change::new(ChangeCode::WorkspaceAdded, "", ""));
            }
        }
        None => {}
    }
    let mut text = doc.to_string();
    // Комментарии с командами (`cargo run -p old --bin old-editor`) тоже получают новое имя.
    if let Some(new_name) = edit.rename_to.filter(|n| *n != old_name) {
        text = rename_text(&text, &old_name, new_name, "", "").0;
    }
    Ok((text, changes))
}

/// `version.workspace = true` и `log.workspace = true` → значения из рабочего пространства
/// движка. `true`, если что-то подставлено.
fn inline_workspace_values(doc: &mut toml_edit::DocumentMut, edit: &ManifestEdit, changes: &mut Vec<Change>) -> Result<bool, ImportError> {
    use toml_edit::{value, Item, Value};
    let workspace: Option<toml::Table> = edit
        .engine_root
        .and_then(|root| fs::read_to_string(root.join("Cargo.toml")).ok())
        .and_then(|t| t.parse().ok());
    let ws_package = workspace.as_ref().and_then(|w| w.get("workspace")?.get("package")?.as_table().cloned());
    let ws_deps = workspace.as_ref().and_then(|w| w.get("workspace")?.get("dependencies")?.as_table().cloned());
    let engine_root = edit.engine_root.map(|r| r.display().to_string().replace('\\', "/"));
    let mut any = false;
    let is_inherit = |item: &Item| match item {
        Item::Table(t) => t.get("workspace").and_then(|w| w.as_bool()) == Some(true),
        Item::Value(Value::InlineTable(t)) => t.get("workspace").and_then(|w| w.as_bool()) == Some(true),
        _ => false,
    };
    if let Some(package) = doc.get_mut("package").and_then(|p| p.as_table_mut()) {
        let keys: Vec<String> = package.iter().filter(|(_, v)| is_inherit(v)).map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let fallback = match key.as_str() {
                "version" => Some("0.1.0"),
                "edition" => Some("2021"),
                _ => None,
            };
            let found = ws_package.as_ref().and_then(|p| p.get(&key)).and_then(|v| v.as_str()).or(fallback);
            match found {
                Some(text) => {
                    package.insert(&key, value(text));
                    changes.push(Change::new(ChangeCode::InheritedWritten, format!("{key} = \"{text}\""), ""));
                }
                None => {
                    package.remove(&key);
                }
            }
            any = true;
        }
    }
    for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let Some(deps) = doc.get_mut(section).and_then(|d| d.as_table_like_mut()) else {
            continue;
        };
        let keys: Vec<String> = deps.iter().filter(|(_, v)| is_inherit(v)).map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let Some(spec) = ws_deps.as_ref().and_then(|d| d.get(&key)) else {
                return Err(ImportError::Manifest(format!(
                    "dependency `{key}` is inherited from a workspace, but the engine workspace does not define it; give it a version in Cargo.toml"
                )));
            };
            let mut text = spec.to_string();
            // Путь внутри рабочего пространства движка становится абсолютным.
            if let (Some(table), Some(root)) = (spec.as_table(), &engine_root) {
                if let Some(path) = table.get("path").and_then(|p| p.as_str()) {
                    let mut copy = table.clone();
                    copy.insert("path".to_string(), toml::Value::String(format!("{root}/{path}")));
                    text = toml::Value::Table(copy).to_string();
                }
            }
            let parsed: Value = text.parse().map_err(manifest_err)?;
            deps.insert(&key, Item::Value(parsed));
            changes.push(Change::new(ChangeCode::InheritedWritten, format!("{key} = {}", spec.to_string().replace('\n', " ")), ""));
            any = true;
        }
    }
    Ok(any)
}

// ---------------------------------------------------------------------------
// Переименование в исходниках
// ---------------------------------------------------------------------------

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Заменить слово `from` на `to` там, где перед ним не идентификатор, а после — одна из
/// допустимых концовок.
fn replace_word(text: &str, from: &str, to: &str, after: &dyn Fn(&str) -> bool) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut changed = false;
    let mut previous: Option<char> = None;
    while let Some(at) = rest.find(from) {
        let before = rest[..at].chars().next_back().or(previous);
        let tail = &rest[at + from.len()..];
        if before.is_none_or(|c| !is_ident_char(c) && c != '-') && after(tail) {
            out.push_str(&rest[..at]);
            out.push_str(to);
            changed = true;
        } else {
            out.push_str(&rest[..at + from.len()]);
        }
        previous = out.chars().next_back();
        rest = tail;
    }
    out.push_str(rest);
    (out, changed)
}

/// Новое имя в тексте исходника: `old::` → `new::`, `old-editor`, `-p old`, заголовок игры.
pub fn rename_text(text: &str, old: &str, new: &str, old_title: &str, new_title: &str) -> (String, bool) {
    let (old_lib, new_lib) = (old.replace('-', "_"), new.replace('-', "_"));
    let mut text = text.to_string();
    let mut changed = false;
    let mut apply = |from: &str, to: &str, after: &dyn Fn(&str) -> bool| {
        let (next, did) = replace_word(&text, from, to, after);
        text = next;
        changed |= did;
    };
    apply(&format!("{old_lib}::"), &format!("{new_lib}::"), &|_| true);
    apply(&format!("{old}-editor"), &format!("{new}-editor"), &|tail| !tail.starts_with(|c: char| is_ident_char(c) || c == '-'));
    apply(&format!("-p {old}"), &format!("-p {new}"), &|tail| !tail.starts_with(|c: char| is_ident_char(c) || c == '-'));
    if old_title != new_title {
        let quoted_old = format!("\"{old_title}\"");
        let quoted_new = format!("\"{new_title}\"");
        let mut lines: Vec<String> = Vec::new();
        let mut title_changed = false;
        for line in text.split_inclusive('\n') {
            if line.contains(&quoted_old) && (line.contains("GAME_NAME") || line.contains("titled(") || line.contains("title")) {
                lines.push(line.replace(&quoted_old, &quoted_new));
                title_changed = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if title_changed {
            text = lines.concat();
            changed = true;
        }
    }
    (text, changed)
}

fn walk_files(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}{name}");
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk_files(&path, &format!("{rel}/"), out),
            Ok(t) if t.is_file() => out.push((rel, path)),
            _ => {}
        }
    }
}

/// Поправить исходники и переименовать файлы под новое имя. Возвращает число изменённых
/// файлов и список переименований.
fn rename_sources(root: &Path, old: &str, new: &str, old_title: &str, new_title: &str, control: &Control) -> Result<(usize, Vec<(String, String)>), ImportError> {
    let mut files = Vec::new();
    walk_files(root, "", &mut files);
    let mut updated = 0;
    let mut renamed = Vec::new();
    for (rel, path) in &files {
        check(control)?;
        let editable = (rel.ends_with(".rs") || rel.ends_with(".md")) && !rel.starts_with("target/");
        if !editable {
            continue;
        }
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        if meta.len() > 4 << 20 {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let (next, changed) = rename_text(&text, old, new, old_title, new_title);
        if changed {
            fs::write(path, next).map_err(|e| ImportError::io(path, e))?;
            updated += 1;
        }
        // `src/bin/<old>-editor.rs` → `<new>-editor.rs`.
        if let Some(name) = rel.strip_prefix("src/bin/") {
            if let Some(rest) = name.strip_prefix(&format!("{old}-")) {
                let to_rel = format!("src/bin/{new}-{rest}");
                let to = root.join(&to_rel);
                fs::rename(path, &to).map_err(|e| ImportError::io(path, e))?;
                renamed.push((rel.clone(), to_rel));
            }
        }
    }
    Ok((updated, renamed))
}

// ---------------------------------------------------------------------------
// GUID
// ---------------------------------------------------------------------------

/// Расширения файлов, внутри которых могут быть ссылки на GUID.
fn may_reference_guids(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(
        ext.as_str(),
        "scene" | "prefab" | "material" | "meta" | "ron" | "settings" | "scenario" | "anim" | "ui" | "tilemap" | "terrain" | "asset" | "txt" | "json" | "toml"
    )
}

/// Новые GUID для всех `.meta` проекта и обновление ссылок на них. Возвращает число GUID.
pub fn regenerate_guids(root: &Path, control: &Control) -> Result<usize, ImportError> {
    let mut files = Vec::new();
    walk_files(root, "", &mut files);
    let mut map: HashMap<String, String> = HashMap::new();
    for (rel, path) in files.iter().filter(|(rel, _)| rel.ends_with(".meta")) {
        check(control)?;
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(guid) = guid_of(&text) {
            if guid.len() == 32 && guid.chars().all(|c| c.is_ascii_hexdigit()) && !map.contains_key(guid) {
                map.insert(guid.to_string(), templates::new_guid());
            }
        }
        let _ = rel;
    }
    if map.is_empty() {
        return Ok(0);
    }
    for (rel, path) in &files {
        check(control)?;
        if !may_reference_guids(rel) {
            continue;
        }
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        if meta.len() > 64 << 20 {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let (next, changed) = replace_guids(&text, &map);
        if changed {
            fs::write(path, next).map_err(|e| ImportError::io(path, e))?;
        }
    }
    Ok(map.len())
}

/// Заменить в тексте каждую отдельную последовательность из 32 шестнадцатеричных цифр,
/// которая есть в таблице.
pub fn replace_guids(text: &str, map: &HashMap<String, String>) -> (String, bool) {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_hexdigit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            if i - start == 32 {
                if let Some(new) = map.get(&text[start..i]) {
                    out.push_str(&text[copied..start]);
                    out.push_str(new);
                    copied = i;
                    changed = true;
                }
            }
        } else {
            i += 1;
        }
    }
    out.push_str(&text[copied..]);
    (out, changed)
}

// ---------------------------------------------------------------------------
// Поиск проектов в каталоге
// ---------------------------------------------------------------------------

/// Найти проекты в каталоге: сам каталог, если это проект, иначе подкаталоги (до глубины 3,
/// не заходя в `target`, `.git`, `node_modules`, `dist` и в найденные проекты).
pub fn scan_for_projects(dir: &Path, control: &Control) -> Vec<PathBuf> {
    fn is_project(dir: &Path) -> bool {
        dir.join("Cargo.toml").is_file() && dir.join("src").join("lib.rs").is_file()
    }
    fn walk(dir: &Path, depth: usize, control: &Control, out: &mut Vec<PathBuf>) {
        if control.is_cancelled() || out.len() >= 200 {
            return;
        }
        if is_project(dir) {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                !name.starts_with('.') && !matches!(name.as_str(), "target" | "node_modules" | "dist" | "$RECYCLE.BIN" | "System Volume Information")
            })
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for sub in dirs {
            walk(&sub, depth - 1, control, out);
        }
    }
    let mut out = Vec::new();
    walk(dir, 3, control, &mut out);
    out.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_replacement_respects_identifiers() {
        let (text, changed) = rename_text("use old::Thing; use older::X; let a = old_x::y; // cargo run -p old --bin old-editor", "old", "fresh", "Old", "Fresh");
        assert!(changed);
        assert_eq!(text, "use fresh::Thing; use older::X; let a = old_x::y; // cargo run -p fresh --bin fresh-editor");
        let (text, _) = rename_text("pub const GAME_NAME: &str = \"My Game\";\nlet other = \"My Game\";\n", "my_game", "x", "My Game", "X");
        assert_eq!(text, "pub const GAME_NAME: &str = \"X\";\nlet other = \"My Game\";\n", "only the title lines change");
        assert!(!rename_text("nothing here", "a", "b", "A", "B").1);
    }

    #[test]
    fn guids_are_replaced_only_as_whole_tokens() {
        let mut map = HashMap::new();
        map.insert("1a2b3c4d5e6f40718293a4b5c6d7e805".to_string(), "ffffffffffffffffffffffffffffffff".to_string());
        let text = "a: \"1a2b3c4d5e6f40718293a4b5c6d7e805\", b: 1a2b3c4d5e6f40718293a4b5c6d7e8050, c: 1a2b3c4d5e6f40718293a4b5c6d7e805";
        let (out, changed) = replace_guids(text, &map);
        assert!(changed);
        assert_eq!(out.matches("ffffffffffffffffffffffffffffffff").count(), 2);
        assert!(out.contains("1a2b3c4d5e6f40718293a4b5c6d7e8050"), "a longer token is not a GUID");
    }

    #[test]
    fn guid_and_version_parsing() {
        assert_eq!(guid_of("(\n    guid: \"abc\",\n    importer: \"Scene\",\n)"), Some("abc"));
        assert_eq!(guid_of("(importer: \"x\")"), None);
        assert_eq!(parse_version("1.78.0"), Some((1, 78)));
        assert_eq!(human(1536), "2 KB");
        assert!(engine_crate_path("../../crates/engine"));
        assert!(engine_crate_path("C:\\x\\crates\\engine-cpp-build"));
        assert!(!engine_crate_path("../vendor/crates/serde"));
    }
}
