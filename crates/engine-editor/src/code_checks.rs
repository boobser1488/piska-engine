//! Проверки вкладки Code в редакторе: когда запускать мгновенную проверку (через паузу
//! после правки, большие файлы — в фоне), когда — `cargo check` (после сохранения, F7),
//! куда класть результаты, как диагностики следуют за правками, F8 / Shift+F8 и Quick Fix.

use crate::i18n::{tr_s, trf};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::code::{BuildStatus, Document};
use crate::code_cargo::{CheckCommand, CheckState, CompilerCheck, RawDiagnostic};
use crate::code_check_data::{check_file, CheckKind, Schema};
use crate::code_diagnostics::{same_path, shift_all, text_change, Counts, Diagnostic, LineIndex, Origin, Severity};
use crate::state::Editor;

/// Файлы больше этого проверяются в фоновом потоке (меньшие — сразу, за миллисекунды).
pub(crate) const BACKGROUND_BYTES: usize = 96 * 1024;

/// Настройки проверок (Preferences → Code Editor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CheckSettings {
    pub on_type: bool,
    /// Пауза после правки, секунды.
    pub delay: f64,
    pub on_save: bool,
}

impl Default for CheckSettings {
    fn default() -> Self {
        Self { on_type: true, delay: 0.3, on_save: true }
    }
}

/// Фоновая проверка документа: версия текста и канал с результатом.
pub(crate) struct SyntaxJob {
    pub version: u64,
    pub receiver: Receiver<(Vec<Diagnostic>, f32)>,
}

/// Состояние проверок вкладки Code.
#[derive(Default)]
pub(crate) struct Checks {
    pub compiler: CompilerCheck,
    /// Диагностики компилятора по файлам (диапазоны — в тексте открытого документа или
    /// файла на диске).
    pub by_file: BTreeMap<PathBuf, Vec<Diagnostic>>,
    pub settings: CheckSettings,
    schema: Arc<Schema>,
    schema_types: usize,
    /// Вывод последней сборки C++: (строка, рабочий каталог команды).
    pub cpp_output: Arc<Mutex<Vec<(String, Option<PathBuf>)>>>,
    /// Файлы, у которых диагностики пришли из сборки C++ (их заменяет следующая сборка).
    cpp_files: BTreeSet<PathBuf>,
    build_running: bool,
    /// Время последней мгновенной проверки, мс (строка состояния, тесты).
    pub last_syntax_ms: f32,
    /// Какую проверку компилятором запустить, когда закончится сборка.
    deferred: bool,
}

impl Checks {
    /// Все диагностики компилятора.
    pub fn compiler_counts(&self) -> Counts {
        Counts::of(self.by_file.values().flatten())
    }

    pub fn compiler_for(&self, path: &Path) -> Option<&Vec<Diagnostic>> {
        self.by_file.iter().find(|(p, _)| same_path(p, path)).map(|(_, d)| d)
    }

    fn compiler_for_mut(&mut self, path: &Path) -> Option<&mut Vec<Diagnostic>> {
        self.by_file.iter_mut().find(|(p, _)| same_path(p, path)).map(|(_, d)| d)
    }

    /// Положить результаты компилятора (заменяют прежние; для C++ — только файлы C++).
    pub fn store_compiler(&mut self, raw: Vec<RawDiagnostic>, documents: &[Document], cpp: bool) {
        if cpp {
            for path in std::mem::take(&mut self.cpp_files) {
                self.by_file.remove(&path);
            }
        } else {
            let cpp_files = self.cpp_files.clone();
            self.by_file.retain(|path, _| cpp_files.contains(path));
        }
        let mut texts: BTreeMap<PathBuf, (String, LineIndex, Option<String>)> = BTreeMap::new();
        for raw in raw {
            let path = raw.span.file.clone();
            if !texts.contains_key(&path) {
                // Открытый документ: позиции компилятора — в сохранённом тексте; правки после
                // сохранения сдвигают их (и делают устаревшими).
                let open = documents.iter().find(|d| same_path(&d.path, &path));
                let (base, current) = match open {
                    Some(doc) => (doc.saved.clone(), Some(doc.text.clone())),
                    None => match crate::code_files::read(&path) {
                        Ok((text, _)) => (text, None),
                        Err(_) => (String::new(), None),
                    },
                };
                let index = LineIndex::new(&base);
                texts.insert(path.clone(), (base, index, current));
            }
            let (base, index, _) = &texts[&path];
            let diagnostic = raw.resolve(base, index);
            self.by_file.entry(path.clone()).or_default().push(diagnostic);
            if cpp {
                self.cpp_files.insert(path);
            }
        }
        for (path, (base, _, current)) in texts {
            let Some(list) = self.by_file.get_mut(&path) else { continue };
            if let Some(change) = current.as_deref().and_then(|current| text_change(&base, current)) {
                shift_all(list, change);
            }
            let text = current.unwrap_or(base);
            let index = LineIndex::new(&text);
            for d in list.iter_mut() {
                d.locate(&index, &text);
            }
            list.sort_by_key(|d| (d.range.start, d.severity));
        }
    }
}

impl Document {
    /// Текст изменился с прошлого кадра: сдвинуть диагностики и запомнить время правки.
    pub(crate) fn sync(&mut self, checks: &mut Checks, now: f64) -> bool {
        if self.text.len() == self.shadow.len() && self.text == self.shadow {
            return false;
        }
        if let Some(change) = text_change(&self.shadow, &self.text) {
            shift_all(&mut self.syntax, change);
            for d in &mut self.syntax {
                // Синтаксис перепроверится сам — до того показываем сдвинутое.
                d.stale = false;
            }
            if let Some(list) = checks.compiler_for_mut(&self.path) {
                shift_all(list, change);
                let index = LineIndex::new(&self.text);
                for d in list.iter_mut() {
                    d.locate(&index, &self.text);
                }
            }
            self.shift_marks(change);
        }
        self.shadow.clone_from(&self.text);
        self.version += 1;
        self.changed_at = now;
        true
    }

    /// Запустить мгновенную проверку текущего текста (маленький файл — сразу).
    fn start_check(&mut self, schema: &Arc<Schema>) {
        if CheckKind::of(&self.path).is_none() {
            self.checked = Some(self.version);
            return;
        }
        if self.text.len() < BACKGROUND_BYTES {
            let started = Instant::now();
            let found = check_file(&self.path, &self.text, schema);
            self.check_ms = started.elapsed().as_secs_f32() * 1000.0;
            self.syntax = found;
            self.checked = Some(self.version);
            self.job = None;
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let path = self.path.clone();
        let text = self.text.clone();
        let schema = schema.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let found = check_file(&path, &text, &schema);
            let _ = sender.send((found, started.elapsed().as_secs_f32() * 1000.0));
        });
        self.job = Some(SyntaxJob { version: self.version, receiver });
    }

    /// Результат фоновой проверки пришёл.
    fn poll_check(&mut self) {
        let Some(job) = &self.job else { return };
        let Ok((found, ms)) = job.receiver.try_recv() else { return };
        let version = job.version;
        self.job = None;
        self.check_ms = ms;
        if version == self.version {
            self.syntax = found;
            self.checked = Some(version);
        }
    }

    /// Проверка ещё не догнала текст.
    pub(crate) fn checking(&self) -> bool {
        CheckKind::of(&self.path).is_some() && (self.job.is_some() || self.checked != Some(self.version))
    }
}

/// Схема проекта (типы компонентов и их поля) — из реестра редактора.
fn schema(editor: &mut Editor) -> Arc<Schema> {
    let registry = &editor.app.world.registry;
    let count = registry.names().count();
    if count != editor.code.checks.schema_types || editor.code.checks.schema.components.is_empty() && count > 0 {
        let mut schema = Schema::default();
        for name in registry.names() {
            let fields = registry.create(name).map(|c| c.field_names().iter().map(|f| f.to_string()).collect()).unwrap_or_default();
            schema.components.insert(name.to_string(), fields);
        }
        editor.code.checks.schema = Arc::new(schema);
        editor.code.checks.schema_types = count;
    }
    editor.code.checks.schema.clone()
}

/// Раз в кадр: правки → сдвиг диагностик; пауза прошла → проверка; результаты фоновых
/// проверок и компилятора; конец сборки C++ → разбор её вывода.
pub(crate) fn tick(editor: &mut Editor, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let schema = schema(editor);
    let settings = editor.code.checks.settings;
    let mut wake: Option<f64> = None;
    let code = &mut editor.code;
    for doc in &mut code.documents {
        doc.sync(&mut code.checks, now);
        doc.poll_check();
        if doc.job.is_some() {
            wake = Some(0.05);
            continue;
        }
        if doc.checked == Some(doc.version) {
            continue;
        }
        // Только что открытый файл проверяется сразу; правка — после паузы.
        let wait = if doc.checked.is_none() { 0.0 } else if settings.on_type { settings.delay } else { f64::INFINITY };
        let left = doc.changed_at + wait - now;
        if left <= 0.0 {
            doc.start_check(&schema);
            code.checks.last_syntax_ms = doc.check_ms;
            if doc.job.is_some() {
                wake = Some(0.05);
            }
        } else if left.is_finite() {
            wake = Some(wake.map_or(left, |w: f64| w.min(left)));
        }
    }
    // Компилятор.
    if let Some(raw) = code.checks.compiler.take_result() {
        let documents = std::mem::take(&mut code.documents);
        code.checks.store_compiler(raw, &documents, false);
        code.documents = documents;
        if let CheckState::Done { seconds, .. } = code.checks.compiler.state {
            let counts = code.checks.compiler_counts();
            log::info!(target: "build", "cargo check: {} error(s), {} warning(s) in {seconds:.1} s", counts.errors, counts.warnings); // i18n-skip: журнал
        }
    }
    if code.checks.compiler.is_running() {
        wake = Some(wake.map_or(0.25, |w| w.min(0.25)));
    }
    // Сборка C++ закончилась — её вывод в диагностики.
    let status = code.build_status();
    let running = status == BuildStatus::Running;
    if code.checks.build_running && !running {
        let lines = code.checks.cpp_output.lock().map(|l| l.clone()).unwrap_or_default();
        let raw = crate::code_cargo::parse_cpp_output(&lines);
        let documents = std::mem::take(&mut code.documents);
        code.checks.store_compiler(raw, &documents, true);
        code.documents = documents;
        if code.checks.deferred {
            code.checks.deferred = false;
            request_compiler(editor, None, now);
        }
    }
    editor.code.checks.build_running = running;
    if let Some(after) = wake {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(after.clamp(0.0, 1.0)));
    }
}

/// Команда `cargo check` для проекта: по команде сборки игры, а без неё — по ближайшему
/// `Cargo.toml` над файлом (или над каталогом кода).
pub(crate) fn check_command(editor: &Editor, near: Option<&Path>) -> Option<CheckCommand> {
    let code_dirs = editor.config.code_dirs.clone();
    let cargo = if editor.prefs.tool_cargo.trim().is_empty() { "cargo".to_string() } else { editor.prefs.tool_cargo.trim().to_string() };
    if let Some(relaunch) = &editor.config.relaunch {
        let is_cargo = Path::new(&relaunch.build.program).file_stem().is_some_and(|s| s == "cargo");
        if is_cargo {
            let program = if relaunch.build.program == "cargo" { cargo } else { relaunch.build.program.clone() };
            let mut command = crate::code_cargo::check_from_build(&program, &relaunch.build.args, relaunch.build.dir.clone(), code_dirs);
            command.env.extend(relaunch.build.env.iter().cloned());
            if let Some(dir) = &relaunch.target_dir {
                if std::env::var_os("CARGO_TARGET_DIR").is_none() && !command.env.iter().any(|(k, _)| k == "CARGO_TARGET_DIR") {
                    command.env.push(("CARGO_TARGET_DIR".to_string(), dir.to_string_lossy().into_owned()));
                }
            }
            return Some(command);
        }
    }
    let start = near.map(Path::to_path_buf).or_else(|| code_dirs.first().cloned())?;
    let manifest_dir = start.ancestors().find(|dir| dir.join("Cargo.toml").is_file())?.to_path_buf();
    let mut command = crate::code_cargo::check_from_build(&cargo, &["build".to_string()], Some(manifest_dir.clone()), code_dirs);
    if command.code_dirs.is_empty() {
        command.code_dirs.push(manifest_dir);
    }
    Some(command)
}

/// Запустить `cargo check` (F7 или после сохранения). Во время сборки — после неё.
pub(crate) fn request_compiler(editor: &mut Editor, near: Option<&Path>, now: f64) -> bool {
    if editor.code.build_status() == BuildStatus::Running {
        editor.code.checks.deferred = true;
        return true;
    }
    let Some(command) = check_command(editor, near) else {
        log::warn!(target: "build", "cargo check: no Cargo.toml above the code folders; open a game project to check Rust code"); // i18n-skip: журнал
        return false;
    };
    editor.code.checks.compiler.request(command, now);
    true
}

/// Файл сохранили: Rust — `cargo check` (если включено в настройках).
pub(crate) fn saved(editor: &mut Editor, path: &Path) {
    if editor.code.checks.settings.on_save && CheckKind::of(path) == Some(CheckKind::Rust) {
        request_compiler(editor, Some(path), 0.0);
    }
}

/// Диагностика для списка: файл и ссылка на неё.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
    /// Файл открыт во вкладке.
    pub open: bool,
}

/// Все диагностики: мгновенные открытых документов и компиляторные по файлам, по
/// порядку (файл, место). Компиляторная, совпадающая с мгновенной, не дублируется.
pub(crate) fn entries(code: &crate::code::CodeState) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for doc in &code.documents {
        for d in &doc.syntax {
            out.push(Entry { path: doc.path.clone(), diagnostic: d.clone(), open: true });
        }
    }
    for (path, list) in &code.checks.by_file {
        let open = code.documents.iter().any(|d| same_path(&d.path, path));
        let path = code.documents.iter().find(|d| same_path(&d.path, path)).map_or(path.clone(), |d| d.path.clone());
        for d in list {
            let duplicate = out.iter().any(|e| e.path == path && e.diagnostic.line == d.line && e.diagnostic.message == d.message);
            if !duplicate {
                out.push(Entry { path: path.clone(), diagnostic: d.clone(), open });
            }
        }
    }
    out.sort_by(|a, b| (&a.path, a.diagnostic.line, a.diagnostic.column).cmp(&(&b.path, b.diagnostic.line, b.diagnostic.column)));
    out
}

/// Диагностики документа для показа (мгновенные + компиляторные).
pub(crate) fn of_document<'a>(doc: &'a Document, checks: &'a Checks) -> Vec<&'a Diagnostic> {
    let mut out: Vec<&Diagnostic> = doc.syntax.iter().collect();
    if let Some(list) = checks.compiler_for(&doc.path) {
        for d in list {
            if !doc.syntax.iter().any(|s| s.range == d.range && s.message == d.message) {
                out.push(d);
            }
        }
    }
    out
}

/// Подпись проверки активного документа для строки состояния: текст и цвет.
pub(crate) fn status(code: &crate::code::CodeState, doc: &Document) -> (String, Severity, bool) {
    let all = of_document(doc, &code.checks);
    let counts = Counts::of(all.iter().copied());
    if code.checks.compiler.is_running() {
        return (tr_s("Checking… (cargo check)").to_string(), Severity::Info, true);
    }
    if doc.checking() {
        return (tr_s("Checking…").to_string(), Severity::Info, true);
    }
    let text = crate::code_check_data::summary(counts.errors, counts.warnings);
    let severity = if counts.errors > 0 {
        Severity::Error
    } else if counts.warnings > 0 {
        Severity::Warning
    } else {
        Severity::Info
    };
    (text, severity, false)
}

/// Следующая (или предыдущая) диагностика после места `(файл, байт)`: F8 / Shift+F8.
pub(crate) fn next_entry(entries: &[Entry], current: Option<(&Path, usize)>, forward: bool, errors_first: bool) -> Option<Entry> {
    let pool: Vec<&Entry> = if errors_first && entries.iter().any(|e| e.diagnostic.severity == Severity::Error) {
        entries.iter().filter(|e| e.diagnostic.severity == Severity::Error).collect()
    } else {
        entries.iter().collect()
    };
    if pool.is_empty() {
        return None;
    }
    let key = |e: &Entry| (e.path.clone(), e.diagnostic.range.start);
    let Some((path, offset)) = current else {
        return Some(if forward { pool[0].clone() } else { pool[pool.len() - 1].clone() });
    };
    let here = (path.to_path_buf(), offset);
    let found = if forward {
        pool.iter().find(|e| key(e) > here)
    } else {
        pool.iter().rev().find(|e| key(e) < here)
    };
    Some(found.copied().unwrap_or(if forward { pool[0] } else { pool[pool.len() - 1] }).clone())
}

/// Строка для «Copy» в Error List и Console.
pub(crate) fn entry_text(entry: &Entry) -> String {
    let d = &entry.diagnostic;
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    let code = d.code.as_deref().map(|c| format!("[{c}]")).unwrap_or_default();
    format!("{}:{}:{}: {severity}{code}: {}", entry.path.display(), d.line, d.column, d.message)
}

/// Подпись источника диагностики.
pub(crate) fn origin_label(origin: Origin) -> &'static str {
    match origin {
        Origin::Syntax => tr_s("syntax check"),
        Origin::Compiler => tr_s("compiler"),
    }
}

/// Подсказка к диагностике: что это и что сделать.
pub(crate) fn tooltip_text(d: &Diagnostic, fix_shortcut: &str) -> String {
    let mut text = match &d.code {
        Some(code) => format!("{code}: {}", d.message),
        None => d.message.clone(),
    };
    if let Some(hint) = &d.hint {
        text.push_str("\n");
        text.push_str(&trf!("hint: {hint}", hint = hint));
    }
    for note in d.notes.iter().take(6) {
        text.push_str("\n• ");
        text.push_str(note);
    }
    if d.stale {
        text.push_str("\n");
        text.push_str(tr_s("(the text changed since this check; save to check again)"));
    }
    if !d.fixes.is_empty() {
        text.push_str("\n");
        text.push_str(&trf!("Quick Fix… ({keys})", keys = fix_shortcut));
    }
    text
}
