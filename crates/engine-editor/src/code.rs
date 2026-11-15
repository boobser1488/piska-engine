//! Вкладка Code: редактор исходников игры (Rust, C++) и текстовых ассетов.
//!
//! Правка → Ctrl+S → Ctrl+B. Сборку выполняют команды из [`EditorConfig`]
//! (`cargo`, `cmake` — что угодно), их вывод идёт в Console. C++-плагин после
//! пересборки подхватывается на лету; код на Rust вкомпилирован в саму программу,
//! поэтому вступает в силу после перезапуска редактора.

use crate::i18n::{tr_s, trf};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::icons::{self, Icon};
use crate::state::{Action, BottomTab, Editor};
use crate::theme;

/// Команда сборки, которую запускает кнопка Build.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildCommand {
    /// Как команда называется в консоли: «C++ plugin», «Rust check».
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    /// Рабочий каталог (по умолчанию — текущий каталог редактора).
    pub dir: Option<PathBuf>,
    /// Результат вступает в силу только после перезапуска (код на Rust).
    pub restart_required: bool,
    /// Переменные окружения процесса (пути инструментов из Preferences → Build & Tools).
    pub env: Vec<(String, String)>,
}

impl BuildCommand {
    pub fn new(label: impl Into<String>, program: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            program: program.into(),
            args: Vec::new(),
            dir: None,
            restart_required: false,
            env: Vec::new(),
        }
    }

    pub fn args<S: Into<String>>(mut self, args: impl IntoIterator<Item = S>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Задать переменную окружения процесса.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Пометить, что результат сборки заработает после перезапуска редактора.
    pub fn restart_required(mut self) -> Self {
        self.restart_required = true;
        self
    }
}

/// Чем закончилась последняя сборка.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildStatus {
    /// Сборку ещё не запускали.
    Idle,
    Running,
    Succeeded { seconds: f32 },
    /// `command` — метка команды, на которой сборка остановилась.
    Failed { command: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    Rust,
    Cpp,
    Ron,
    Wgsl,
    Scenario,
    Plain,
}

impl Language {
    pub(crate) fn of(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" => Language::Rust,
            "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => Language::Cpp,
            "ron" | "scene" | "prefab" | "meta" | "material" | "json" => Language::Ron,
            // Свой шейдер (`.shader`) — тот же WGSL с заголовком в комментариях `//!`.
            "wgsl" | "shader" => Language::Wgsl,
            "scenario" => Language::Scenario,
            _ => Language::Plain,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Language::Rust => tr_s("Rust"),
            Language::Cpp => tr_s("C++"),
            Language::Ron => tr_s("RON"),
            Language::Wgsl => "WGSL",
            Language::Scenario => tr_s("Scenario"),
            Language::Plain => tr_s("Text"),
        }
    }

    pub(crate) fn keywords(self) -> &'static [&'static str] {
        match self {
            Language::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
                "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
                "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
                "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            Language::Cpp => &[
                "auto", "bool", "break", "case", "catch", "char", "class", "const", "constexpr",
                "continue", "default", "delete", "do", "double", "else", "enum", "explicit", "extern",
                "false", "float", "for", "if", "inline", "int", "long", "namespace", "new", "noexcept",
                "nullptr", "operator", "override", "private", "protected", "public", "return", "short",
                "signed", "sizeof", "static", "struct", "switch", "template", "this", "throw", "true",
                "try", "typedef", "typename", "union", "unsigned", "using", "virtual", "void", "while",
            ],
            Language::Ron => &["Some", "None", "true", "false"], // i18n-skip: ключевые слова RON
            Language::Wgsl => &[
                "fn", "let", "var", "const", "struct", "return", "if", "else", "for", "while", "loop",
                "break", "continue", "true", "false", "switch", "case", "default", "discard",
            ],
            // Ключевые слова Scenario (обе формы: Lua и JavaScript).
            Language::Scenario => &[
                "let", "const", "var", "local", "function", "return", "if", "then", "else", "elseif", "end",
                "while", "do", "for", "in", "of", "repeat", "until", "break", "continue", "true", "false",
                "nil", "null", "and", "or", "not", "typeof", "try", "catch", "throw", "self",
            ],
            Language::Plain => &[],
        }
    }
}

// Подсветка — по строкам, с запомненным состоянием разбора (`code_highlight.rs`).
#[cfg(test)]
pub(crate) use crate::code_highlight::highlight;
#[cfg(test)]
use crate::code_highlight::{tokenize, Token};

/// Время изменения и размер файла на диске.
pub(crate) fn disk_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub(crate) struct Document {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    /// Что лежит на диске; отличается от `text` — есть несохранённые правки.
    pub(crate) saved: String,
    pub(crate) language: Language,
    /// Переводы строк, BOM и отступ файла на диске (сохраняются такими же).
    pub(crate) format: crate::code_files::FileFormat,
    /// Текст прошлого кадра: по разнице с ним диагностики и закладки следуют за правками.
    pub(crate) shadow: String,
    /// Номер версии текста (растёт с каждой правкой) и время последней правки.
    pub(crate) version: u64,
    pub(crate) changed_at: f64,
    /// Версия, которую проверили (мгновенная проверка); `None` — ещё не проверяли.
    pub(crate) checked: Option<u64>,
    /// Результат мгновенной проверки.
    pub(crate) syntax: Vec<crate::code_diagnostics::Diagnostic>,
    /// Фоновая проверка большого файла.
    pub(crate) job: Option<crate::code_checks::SyntaxJob>,
    /// Сколько миллисекунд шла последняя проверка.
    pub(crate) check_ms: f32,
    /// Закладки: байты начал строк (Ctrl+K Ctrl+K).
    pub(crate) bookmarks: Vec<usize>,
    /// Время изменения файла на диске, когда его читали или писали.
    /// (время изменения, размер): одно время у двух записей подряд не спрячет правку.
    pub(crate) disk_time: Option<(std::time::SystemTime, u64)>,
    /// Файл изменили на диске, пока в редакторе были несохранённые правки.
    pub(crate) conflict: bool,
    /// Закреплённая вкладка (стоит первой, средняя кнопка её не закрывает).
    pub(crate) pinned: bool,
    /// Несохранённый текст прошлого сеанса (файл восстановления): предложить вернуть.
    pub(crate) recovered: Option<String>,
    /// Версия текста, записанная в файл восстановления.
    pub(crate) recovery_version: Option<u64>,
}

impl Document {
    /// Новый документ с текстом, прочитанным с диска.
    pub(crate) fn new(path: &Path, text: String, format: crate::code_files::FileFormat) -> Self {
        Self {
            path: path.to_path_buf(),
            saved: text.clone(),
            shadow: text.clone(),
            text,
            language: Language::of(path),
            format,
            version: 0,
            changed_at: 0.0,
            checked: None,
            syntax: Vec::new(),
            job: None,
            check_ms: 0.0,
            bookmarks: Vec::new(),
            disk_time: disk_stamp(path),
            conflict: false,
            pinned: false,
            recovered: None,
            recovery_version: None,
        }
    }

    /// Сдвинуть закладки после правки.
    pub(crate) fn shift_marks(&mut self, change: (usize, usize, usize)) {
        for mark in &mut self.bookmarks {
            let mut range = *mark..*mark;
            crate::code_diagnostics::shift_range(&mut range, change);
            *mark = range.start;
        }
        self.bookmarks.dedup();
    }

    /// Записать текст на диск в формате файла.
    pub(crate) fn write(&mut self, trim: bool, final_newline: bool) -> Result<(), String> {
        let bytes = crate::code_files::encode(&self.text, &self.format, trim, final_newline);
        std::fs::write(&self.path, &bytes).map_err(|e| trf!("cannot save `{p0}`: {e}", p0 = self.path.display(), e = e))?;
        // То, что легло на диск (без BOM и с `\n`), — новый «сохранённый» текст.
        let written = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').replace("\r\n", "\n");
        if written != self.text {
            self.text = written;
        }
        self.saved = self.text.clone();
        self.disk_time = disk_stamp(&self.path);
        Ok(())
    }
    fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub(crate) fn dirty(&self) -> bool {
        self.text != self.saved
    }

    /// Id текстового поля документа: по пути, чтобы история Undo и курсор файла
    /// переживали переключение и закрытие других вкладок.
    pub(crate) fn text_id(&self) -> egui::Id {
        egui::Id::new(("code_text", self.path.to_string_lossy().to_lowercase()))
    }
}

struct Build {
    status: Arc<Mutex<BuildStatus>>,
    /// Статус, который уже показан пользователю (чтобы один раз открыть консоль при ошибке).
    seen: BuildStatus,
}

/// Поиск в файле (Ctrl+F).
#[derive(Default)]
pub(crate) struct Find {
    pub(crate) open: bool,
    /// Учёт регистра, целые слова, регулярное выражение, сохранение регистра при замене.
    pub(crate) options: crate::code_find::FindOptions,
    /// Ошибка в регулярном выражении (показывается в строке поиска).
    pub(crate) error: Option<String>,
    pub(crate) query: String,
    /// Номер текущего совпадения.
    pub(crate) current: usize,
    /// Перейти к текущему совпадению в этом кадре.
    pub(crate) jump: bool,
    pub(crate) focus: bool,
    /// Строка «Replace» (Ctrl+H) под поиском и то, на что заменять.
    pub(crate) replace: bool,
    pub(crate) replacement: String,
    /// Строка «Go to line» (Ctrl+G): введённый номер; фокус в поле при открытии.
    pub(crate) goto_line: Option<String>,
    pub(crate) goto_focus: bool,
}


pub(crate) struct CodeState {
    pub(crate) documents: Vec<Document>,
    pub(crate) active: usize,
    /// Проверки: мгновенные, `cargo check`, вывод сборки C++ (`code_checks.rs`).
    pub(crate) checks: crate::code_checks::Checks,
    /// Выделить диапазон байтов файла, когда он будет показан (F8, Error List).
    pub(crate) reveal: Option<(PathBuf, std::ops::Range<usize>)>,
    /// Открытое меню быстрых исправлений (Ctrl+.).
    pub(crate) quick_fix: Option<crate::code_view::QuickFixMenu>,
    /// Длина выделения в символах — для строки состояния.
    pub(crate) selection: usize,
    /// Фильтры и выбор окна Error List.
    pub(crate) errors: crate::error_list::ErrorListState,
    /// Переход к файлам и символам, история мест, порядок вкладок.
    pub(crate) nav: crate::code_nav::NavState,
    /// Find in Files / Find All References (окно Find Results).
    pub(crate) search: crate::find_in_files::FindInFiles,
    /// Список подсказок (Ctrl+Space) и подсказка параметров (Ctrl+Shift+Space).
    pub(crate) completion: Option<crate::code_complete::Popup>,
    pub(crate) parameter_hint: Option<(String, egui::Pos2)>,
    /// Символы проекта для подсказок: (время, символы) — пересобираются раз в 10 с.
    pub(crate) symbol_cache: Option<(f64, std::sync::Arc<Vec<crate::code_symbols::Symbol>>)>,
    /// Диалог Rename (F2).
    pub(crate) rename: Option<crate::code_rename::Rename>,
    /// Форматировать при сохранении (Preferences → Code Editor).
    pub(crate) format_on_save: bool,
    /// Схема документа (outline): (файл, версия текста, символы).
    pub(crate) outline: Option<(PathBuf, u64, Vec<crate::code_symbols::Symbol>)>,
    /// Где на экране первая диагностика активного файла (для снимков и тестов наведения).
    pub(crate) first_diagnostic_at: Option<egui::Pos2>,
    /// Где текст на экране: (левый верх строки 1 в столбце 0, ширина символа, высота строки) — для тестов и снимков.
    pub(crate) geometry: Option<(egui::Pos2, f32, f32)>,
    /// Прежние выделения для «сузить выделение» (Shift+Alt+←).
    pub(crate) selections: Vec<std::ops::Range<usize>>,
    /// Команды вкладки Code из реестра горячих клавиш за этот кадр (`quick-fix`…).
    pub(crate) commands: Vec<&'static str>,
    /// При сохранении: убрать пробелы в концах строк; добавить перевод строки в конец.
    pub(crate) save_trim: bool,
    pub(crate) save_final_newline: bool,
    build: Build,
    /// Файлы каталогов с кодом (относительное имя для показа, полный путь).
    pub files: Vec<(String, PathBuf)>,
    /// Каталоги дерева файлов (и пустые; корни — без `/` в имени): (имя для показа, путь).
    pub(crate) folders: Vec<(String, PathBuf)>,
    pub(crate) find: Find,
    /// Перейти к строке файла, когда он будет показан: (файл, строка с 1).
    goto: Option<(PathBuf, usize)>,
    /// Свёрнутые каталоги дерева файлов.
    pub(crate) collapsed: std::collections::BTreeSet<String>,
    /// Дерево файлов: выделение, переименование, буфер файлов, окна (`code_tree.rs`).
    pub(crate) tree: crate::code_tree::Tree,
    /// Команды дереву от сочетаний клавиш (F2, Ctrl+N) — выполняются в его кадре.
    pub(crate) tree_commands: Vec<crate::code_tree::TreeCmd>,
    /// Строка и столбец курсора (с 1) — для строки состояния.
    pub(crate) cursor: (usize, usize),
    /// Автосохранение несохранённых правок для восстановления после сбоя.
    pub(crate) recovery: crate::code_recovery::Recovery,
    /// Вопрос «Save changes?» о закрываемых вкладках (`code_close.rs`).
    pub(crate) close_ask: Option<crate::code_close::CloseAsk>,
    /// Ошибка записи кода при выходе из редактора (показывает вопрос о сцене).
    pub(crate) leave_error: Option<String>,
}

impl Default for CodeState {
    fn default() -> Self {
        Self {
            documents: Vec::new(),
            active: 0,
            checks: Default::default(),
            reveal: None,
            quick_fix: None,
            selection: 0,
            errors: Default::default(),
            selections: Vec::new(),
            nav: Default::default(),
            search: Default::default(),
            completion: None,
            parameter_hint: None,
            symbol_cache: None,
            rename: None,
            format_on_save: false,
            outline: None,
            first_diagnostic_at: None,
            geometry: None,
            commands: Vec::new(),
            save_trim: false,
            save_final_newline: false,
            build: Build {
                status: Arc::new(Mutex::new(BuildStatus::Idle)),
                seen: BuildStatus::Idle,
            },
            files: Vec::new(),
            folders: Vec::new(),
            find: Find::default(),
            goto: None,
            collapsed: std::collections::BTreeSet::new(),
            tree: Default::default(),
            tree_commands: Vec::new(),
            cursor: (1, 1),
            recovery: Default::default(),
            close_ask: None,
            leave_error: None,
        }
    }
}

const CODE_EXTENSIONS: [&str; 19] = [
    "rs", "cpp", "cc", "cxx", "c", "h", "hpp", "wgsl", "toml", "txt", "md", "ron", "scene", "prefab",
    "cmake", "json", "scenario", "material", "shader",
];
const SKIPPED_DIRS: [&str; 6] = ["target", "build", "bin", ".git", "node_modules", ".vs"];

impl CodeState {
    /// Перечитать список файлов: каталоги с кодом и сценарии (`.scenario`) из ассетов
    /// проекта `assets` — они лежат среди ассетов, но это такой же код игры.
    pub fn scan(&mut self, dirs: &[PathBuf], assets: Option<&Path>) {
        let mut folders: Vec<(String, PathBuf)> = Vec::new();
        fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>, folders: &mut Vec<(String, PathBuf)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    if depth < 4 && !SKIPPED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                        // Каталоги — и пустые: в них можно создать файл.
                        let shown = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                        let prefix = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        folders.push((format!("{prefix}/{shown}"), path.clone()));
                        walk(root, &path, depth + 1, out, folders);
                    }
                    continue;
                }
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                let listed = CODE_EXTENSIONS.contains(&extension.as_str()) || name == "CMakeLists.txt";
                if listed && !name.contains(".live-") {
                    let shown = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                    let prefix = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    out.push((format!("{prefix}/{shown}"), path));
                }
            }
        }
        let mut files = Vec::new();
        for dir in dirs {
            let prefix = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            folders.push((prefix, dir.clone()));
            walk(dir, dir, 0, &mut files, &mut folders);
        }
        if let Some(assets) = assets {
            fn scenarios(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    return;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if path.is_dir() {
                        if depth < 8 && !name.starts_with('.') {
                            scenarios(root, &path, depth + 1, out);
                        }
                    } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("scenario") || e.eq_ignore_ascii_case("shader")) {
                        let shown = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                        let prefix = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        out.push((format!("{prefix}/{shown}"), path));
                    }
                }
            }
            let mut found = Vec::new();
            scenarios(assets, assets, 0, &mut found);
            // Ассеты могут уже быть каталогом с кодом: второй раз файл не показываем.
            found.retain(|(_, path)| !files.iter().any(|(_, listed)| listed == path));
            // Каталоги ассетов — корень и те, где лежат сценарии.
            if !folders.iter().any(|(_, p)| p == assets) {
                let prefix = assets.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                folders.push((prefix, assets.to_path_buf()));
                for (shown, path) in &found {
                    let mut dir = path.parent();
                    let mut name = shown.rsplit_once('/').map(|(d, _)| d.to_string());
                    while let (Some(d), Some(n)) = (dir, name.clone()) {
                        if d == assets || folders.iter().any(|(_, p)| p == d) {
                            break;
                        }
                        folders.push((n.clone(), d.to_path_buf()));
                        dir = d.parent();
                        name = n.rsplit_once('/').map(|(d, _)| d.to_string());
                    }
                }
            }
            files.extend(found);
        }
        files.sort();
        folders.sort();
        folders.dedup();
        self.files = files;
        self.folders = folders;
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        if let Some(index) = self.documents.iter().position(|d| d.path == path) {
            self.active = index;
            self.touch_mru();
            return Ok(());
        }
        // Формат файла (переводы строк, BOM, отступ) запоминается и сохраняется таким же.
        let (text, format) = crate::code_files::read(path)?;
        let mut document = Document::new(path, text, format);
        // Редактор закрылся с несохранёнными правками этого файла — предложить их вернуть.
        if let Some(dir) = &self.recovery.dir {
            document.recovered = crate::code_recovery::find(dir, path, &document.text);
        }
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        self.touch_mru();
        Ok(())
    }

    /// Закрепить / открепить вкладку: закреплённые стоят первыми, в порядке закрепления.
    pub(crate) fn set_pinned(&mut self, index: usize, pinned: bool) {
        let Some(document) = self.documents.get_mut(index) else { return };
        document.pinned = pinned;
        let active = self.documents.get(self.active).map(|d| d.path.clone());
        let document = self.documents.remove(index);
        let at = self.documents.iter().take_while(|d| d.pinned).count();
        self.documents.insert(at, document);
        if let Some(path) = active {
            self.active = self.documents.iter().position(|d| d.path == path).unwrap_or(0);
        }
    }

    /// Активная вкладка — первая в порядке недавнего использования (Ctrl+Tab).
    fn touch_mru(&mut self) {
        if let Some(path) = self.documents.get(self.active).map(|d| d.path.clone()) {
            self.nav.mru.retain(|p| *p != path);
            self.nav.mru.insert(0, path);
        }
    }

    pub fn close(&mut self, index: usize) {
        if index < self.documents.len() {
            let closed = self.documents.remove(index);
            // Закрыли — несохранённое отброшено намеренно, восстанавливать нечего.
            if let Some(dir) = &self.recovery.dir {
                if closed.recovery_version.is_some() || closed.recovered.is_some() {
                    crate::code_recovery::remove(dir, &closed.path);
                }
            }
            self.nav.mru.retain(|p| *p != closed.path);
            self.nav.closed.push(closed.path);
            self.active = self.active.min(self.documents.len().saturating_sub(1));
        }
    }

    /// Записать активный документ на диск. Возвращает путь сохранённого файла.
    pub fn save_active(&mut self) -> Result<Option<PathBuf>, String> {
        self.save_index(self.active)
    }

    /// Записать документ вкладки `index` на диск.
    pub(crate) fn save_index(&mut self, index: usize) -> Result<Option<PathBuf>, String> {
        let Some(document) = self.documents.get_mut(index) else {
            return Ok(None);
        };
        if self.format_on_save {
            format_before_save(document);
        }
        // Свои переводы строк и BOM файла; правила сохранения — Preferences → Code Editor.
        document.write(self.save_trim, self.save_final_newline)?;
        // Сохранено — файл восстановления больше не нужен.
        if let Some(dir) = &self.recovery.dir {
            crate::code_recovery::remove(dir, &document.path);
            document.recovery_version = None;
        }
        Ok(Some(document.path.clone()))
    }

    /// Активный документ: путь, текст, есть ли несохранённые правки.
    pub fn active(&self) -> Option<(&Path, &str, bool)> {
        self.documents
            .get(self.active)
            .map(|d| (d.path.as_path(), d.text.as_str(), d.dirty()))
    }

    pub fn set_active_text(&mut self, text: &str) {
        if let Some(document) = self.documents.get_mut(self.active) {
            document.text = text.to_string();
        }
    }

    /// Перейти к строке `line` (с 1) файла `path`, когда он откроется во вкладке.
    pub fn goto_line(&mut self, path: &Path, line: usize) {
        self.goto = Some((path.to_path_buf(), line.max(1)));
    }

    pub fn has_unsaved(&self) -> bool {
        self.documents.iter().any(Document::dirty)
    }

    /// Записать на диск все документы с несохранёнными правками.
    pub(crate) fn save_all(&mut self) -> Result<(), String> {
        let (trim, final_newline) = (self.save_trim, self.save_final_newline);
        let format = self.format_on_save;
        for document in self.documents.iter_mut().filter(|d| d.dirty()) {
            if format {
                format_before_save(document);
            }
            document.write(trim, final_newline)?;
            if let Some(dir) = &self.recovery.dir {
                crate::code_recovery::remove(dir, &document.path);
                document.recovery_version = None;
            }
        }
        Ok(())
    }

    /// Открытые файлы, номер активного и строка курсора в нём — для перезапуска.
    pub(crate) fn session_documents(&self) -> (Vec<PathBuf>, usize, usize) {
        let paths = self.documents.iter().map(|d| d.path.clone()).collect();
        (paths, self.active, self.cursor.0)
    }

    /// Файл переименовали на диске: открытый документ следует за ним.
    pub(crate) fn rename_document(&mut self, from: &Path, to: &Path) {
        for document in self.documents.iter_mut().filter(|d| d.path == from) {
            document.path = to.to_path_buf();
        }
    }

    /// Файл удалили: закрыть его вкладку.
    pub(crate) fn close_path(&mut self, path: &Path) {
        if let Some(index) = self.documents.iter().position(|d| d.path == path) {
            self.close(index);
        }
    }

    pub fn build_status(&self) -> BuildStatus {
        self.build
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or(BuildStatus::Idle)
    }

    /// Запустить команды сборки по очереди в фоновом потоке; вывод идёт в лог.
    pub fn start_build(&mut self, commands: Vec<BuildCommand>) {
        self.start_build_with(commands, None);
    }

    /// То же, и после команд — `last` в том же потоке (сборка на Rust с перезапуском).
    /// `false` из `last` — сборка не удалась.
    pub(crate) fn start_build_with(&mut self, commands: Vec<BuildCommand>, last: Option<FinalStep>) {
        if self.build_status() == BuildStatus::Running {
            return;
        }
        if commands.is_empty() && last.is_none() {
            log::warn!(
                target: "build",
                "no build commands are configured. Add them when starting the editor: \
                 `EditorConfig::default().build_command(BuildCommand::new(\"C++ plugin\", \"cmake\").args([\"--build\", \"build\"]))`"
            );
            return;
        }
        let status = self.build.status.clone();
        // Вывод компилятора C++ — для Error List (разбирается, когда сборка закончится).
        let output = self.checks.cpp_output.clone();
        if let Ok(mut lines) = output.lock() {
            lines.clear();
        }
        if let Ok(mut status) = status.lock() {
            *status = BuildStatus::Running;
        }
        self.build.seen = BuildStatus::Running;
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut outcome = BuildStatus::Succeeded { seconds: 0.0 };
            for command in &commands {
                log::info!(target: "build", "{}: {} {}", command.label, command.program, command.args.join(" "));
                if !run_command(command, &output) {
                    outcome = BuildStatus::Failed {
                        command: command.label.clone(),
                    };
                    break;
                }
                if command.restart_required {
                    log::info!(
                        target: "build",
                        "{}: OK. Rust code is compiled into the program — restart the editor to run the new version",
                        command.label
                    );
                }
            }
            if let (Some(last), BuildStatus::Succeeded { .. }) = (last, &outcome) {
                if !last() {
                    outcome = BuildStatus::Failed {
                        command: "Rust: build".to_string(),
                    };
                }
            }
            if let BuildStatus::Succeeded { seconds } = &mut outcome {
                *seconds = started.elapsed().as_secs_f32();
                log::info!(target: "build", "build finished in {seconds:.1} s");
            }
            if let Ok(mut status) = status.lock() {
                *status = outcome;
            }
        });
    }
}

/// Последний шаг сборки, который выполняется в её потоке после команд.
pub(crate) type FinalStep = Box<dyn FnOnce() -> bool + Send>;

/// Строка вывода компилятора — в лог с уровнем по её содержимому.
pub(crate) fn log_build_line(line: &str) {
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    let lower = line.to_ascii_lowercase();
    // Компиляторы пишут по-разному: `error:`, `error[E0308]`, `error C2065`.
    if lower.contains("error:") || lower.contains("error[") || lower.contains(" error c") || lower.contains("fatal error") {
        log::error!(target: "build", "{line}");
    } else if lower.contains("warning:") || lower.contains(" warning c") {
        log::warn!(target: "build", "{line}");
    } else {
        log::info!(target: "build", "{line}");
    }
}

/// Выполнить команду, переслав её вывод построчно в лог. `true` — код возврата 0.
fn run_command(command: &BuildCommand, output: &crate::code_checks::BuildOutput) -> bool {
    let mut process = engine_build::host::command(&command.program);
    process
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &command.dir {
        process.current_dir(dir);
    }
    process.envs(command.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(err) => {
            log::error!(
                target: "build",
                "{}: cannot start `{}`: {err}. Is it installed and on PATH?",
                command.label,
                command.program
            );
            return false;
        }
    };

    let forward = |stream: Box<dyn std::io::Read + Send>| -> std::thread::JoinHandle<()> {
        let output = output.clone();
        let dir = command.dir.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                log_build_line(&line);
                if let Ok(mut lines) = output.lock() {
                    lines.push((line, dir.clone()));
                }
            }
        })
    };
    let readers: Vec<_> = [
        child.stdout.take().map(|s| forward(Box::new(s))),
        child.stderr.take().map(|s| forward(Box::new(s))),
    ]
    .into_iter()
    .flatten()
    .collect();
    let status = child.wait();
    for reader in readers {
        let _ = reader.join();
    }
    match status {
        Ok(status) if status.success() => true,
        Ok(status) => {
            log::error!(target: "build", "{}: failed ({status})", command.label);
            false
        }
        Err(err) => {
            log::error!(target: "build", "{}: {err}", command.label);
            false
        }
    }
}

/// Все вхождения `query` в `text` без учёта регистра (ASCII), байтовые диапазоны.
/// Совпадения строки поиска с её параметрами; ошибка регулярного выражения — отдельно.
fn search(text: &str, query: &str, options: crate::code_find::FindOptions) -> (Vec<std::ops::Range<usize>>, Option<String>) {
    match crate::code_find::find_all(text, query, options) {
        Ok(found) => (found.into_iter().map(|m| m.range).collect(), None),
        Err(error) => (Vec::new(), Some(error)),
    }
}

#[cfg(test)]
fn find_matches(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let haystack = text.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        if text.is_char_boundary(start) && text.is_char_boundary(end) {
            matches.push(start..end);
        }
        from = end.max(start + 1);
        while from < text.len() && !text.is_char_boundary(from) {
            from += 1;
        }
        if from >= text.len() {
            break;
        }
    }
    matches
}


pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    // Сборка закончилась с ошибкой — один раз показать консоль.
    let status = editor.code.build_status();
    if status != editor.code.build.seen {
        if matches!(status, BuildStatus::Failed { .. }) {
            editor.bottom_tab = BottomTab::Console;
        }
        editor.code.build.seen = status.clone();
    }
    if status == BuildStatus::Running {
        ui.ctx().request_repaint();
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
        editor.code.find.open = true;
        editor.code.find.focus = true;
    }
    // Ctrl+H — поиск с заменой, Ctrl+G — перейти к строке.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::H)) {
        editor.code.find.open = true;
        editor.code.find.replace = true;
        editor.code.find.focus = true;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::G)) {
        editor.code.find.goto_line = Some(String::new());
        editor.code.find.goto_focus = true;
    }

    // --- Дерево файлов слева ---
    egui::SidePanel::left("code_files")
        .resizable(true)
        .default_width(230.0)
        .width_range(160.0..=420.0)
        .frame(egui::Frame::none().fill(theme::BG_PANEL))
        .show_inside(ui, |ui| {
            egui::TopBottomPanel::bottom("code_outline")
                .resizable(true)
                .default_height(220.0)
                .height_range(80.0..=600.0)
                .frame(egui::Frame::none().fill(theme::BG_PANEL))
                .show_inside(ui, |ui| outline(editor, ui));
            crate::code_tree::panel(editor, ui);
        });

    // --- Вкладки документов и кнопки ---
    let mut close = None;
    let mut activate = None;
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::TAB_BAR), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, theme::BG_HEADER);
    ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
    let buttons_width = 260.0_f32.min(bar.width() * 0.5);
    let tabs_rect = Rect::from_min_max(bar.min + vec2(theme::SP_S, 0.0), pos2(bar.right() - buttons_width, bar.bottom()));
    let mut tabs = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(tabs_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Max)),
    );
    tabs.set_clip_rect(tabs_rect);
    tabs.spacing_mut().item_spacing.x = 1.0;
    let mut pin = None;
    let mut tab_action: Option<crate::code_menu::TabAction> = None;
    let close_keys = editor.prefs.shortcuts.text("close-tab"); // i18n-skip: id команды
    let tab_count = editor.code.documents.len();
    for (index, document) in editor.code.documents.iter().enumerate() {
        let selected = index == editor.code.active;
        let (tab, closed, unpinned) = document_tab(&mut tabs, index, document, selected);
        if tab.clicked() {
            activate = Some(index);
        }
        // Закреплённая вкладка не закрывается средней кнопкой (как в Visual Studio).
        if closed || (tab.middle_clicked() && !document.pinned) {
            close = Some(index);
        }
        if unpinned {
            pin = Some((index, false));
        }
        tab.context_menu(|ui| crate::code_menu::tab_menu(ui, index, tab_count, document.pinned, &document.path, &close_keys, &mut tab_action));
        tab.on_hover_text(editor_display(&editor.config.code_dirs, &document.path));
    }
    if let Some((index, pinned)) = pin {
        editor.code.set_pinned(index, pinned);
    }
    if let Some(action) = tab_action {
        let ctx = ui.ctx().clone();
        crate::code_menu::run_tab(editor, &ctx, action);
    }
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar.shrink2(vec2(theme::SP_M, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right.spacing_mut().item_spacing.x = theme::SP_S;
    let running = status == BuildStatus::Running;
    let buildable = editor.can_build();
    let build = right.add_enabled_ui(!running && buildable, |ui| theme::icon_text_button(ui, Icon::Build, tr_s("Build")));
    let build = build
        .inner
        .on_hover_text(trf!("{}; output goes to the Console", editor.prefs.shortcuts.tip(tr_s("Save and run the build commands"), "build-code")))
        .on_disabled_hover_text(if running {
            tr_s("A build is running")
        } else {
            tr_s("No build commands: add them with `EditorConfig::build_command`")
        });
    if build.clicked() {
        editor.actions.push(Action::Build);
    }
    let has_document = !editor.code.documents.is_empty();
    let save = right.add_enabled_ui(has_document, |ui| theme::icon_text_button(ui, Icon::Save, tr_s("Save")));
    if save.inner.on_hover_text(editor.prefs.shortcuts.tip(tr_s("Save the file"), "save")).clicked() {
        editor.actions.push(Action::SaveCode);
    }
    // Внешний редактор (Preferences → General): кнопка есть, только когда он задан.
    if has_document && !editor.prefs.external_editor.trim().is_empty() {
        let external = theme::icon_text_button(&mut right, Icon::External, tr_s("Open Externally"));
        let external = right
            .interact(external.rect, Editor::open_externally_id(), egui::Sense::click())
            .on_hover_text(tr_s("Open this file in the external code editor set in Preferences → General"));
        if external.clicked() {
            editor.open_active_externally();
        }
    }
    if let Some(index) = activate {
        editor.code.active = index;
    }
    if let Some(index) = close {
        editor.code.request_close(&[index]);
    }

    // --- Строка состояния внизу ---
    let status_info = editor.code.documents.get(editor.code.active).map(|doc| {
        let (check, severity, busy) = crate::code_checks::status(&editor.code, doc);
        (doc.language, doc.format.clone(), check, severity, busy)
    });
    let selection_len = editor.code.selection;
    let mut open_error_list = false;
    egui::TopBottomPanel::bottom("code_status")
        .exact_height(theme::ROW + 2.0)
        .frame(
            egui::Frame::none()
                .fill(theme::BG_HEADER)
                .inner_margin(egui::Margin::symmetric(theme::SP_M, 0.0)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let (text, color, icon) = match &status {
                    BuildStatus::Idle => (tr_s("Not built yet").to_string(), theme::TEXT_MUTED, Icon::Build),
                    BuildStatus::Running => (tr_s("Building…").to_string(), theme::WARN, Icon::Build),
                    BuildStatus::Succeeded { seconds } => (trf!("Build succeeded in {seconds} s", seconds = format!("{:.1}", seconds)), theme::OK, Icon::Check),
                    BuildStatus::Failed { command } => (trf!("Build failed: {command} — see the Console", command = command), theme::ERROR, Icon::Error),
                };
                icons::inline(ui, icon, color);
                ui.label(egui::RichText::new(text).size(theme::FONT_CAPTION).color(color));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((language, format, check, severity, busy)) = &status_info {
                        let (line, column) = editor.code.cursor;
                        let mut parts = vec![trf!("Ln {line}, Col {column}", line = line, column = column)];
                        if selection_len > 0 {
                            parts.push(trf!("Sel {n}", n = selection_len));
                        }
                        parts.push(format.indent.label());
                        parts.push(format.line_ending.label().to_string());
                        parts.push(crate::code_files::encoding_label(format).to_string());
                        parts.push(language.name().to_string());
                        for text in parts {
                            ui.label(egui::RichText::new(text).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
                            ui.add_space(theme::SP_S);
                        }
                        // Состояние проверки: щелчок открывает Error List.
                        let color = if *busy {
                            theme::TEXT_MUTED
                        } else {
                            match severity {
                                crate::code_diagnostics::Severity::Error => theme::ERROR,
                                crate::code_diagnostics::Severity::Warning => theme::WARN,
                                crate::code_diagnostics::Severity::Info => theme::OK,
                            }
                        };
                        let response = ui
                            .add(egui::Label::new(egui::RichText::new(check.as_str()).size(theme::FONT_CAPTION).color(color)).sense(Sense::click()))
                            .on_hover_text(tr_s("Problems of this file (syntax check while you type, cargo check after saving). Click to open the Error List"));
                        if response.clicked() {
                            open_error_list = true;
                        }
                        let icon = if *busy {
                            Icon::Refresh
                        } else if *severity == crate::code_diagnostics::Severity::Info {
                            Icon::Check
                        } else {
                            crate::code_view::severity_icon(*severity)
                        };
                        icons::inline(ui, icon, color);
                    }
                });
            });
        });
    if open_error_list {
        editor.actions.push(Action::Layout(crate::dock::LayoutAction::Focus(crate::dock::Panel::ErrorList)));
    }

    // Исходники на Rust новее программы — предложить Build с перезапуском.
    crate::rust_scripts::code_banner(editor, ui);
    let now = ui.input(|i| i.time);

    let fix_keys = editor.prefs.shortcuts.text("quick-fix"); // i18n-skip: имя типа / ключ
    let text_commands = std::mem::take(&mut editor.code.commands);
    let quick_fix_pressed = text_commands.contains(&"quick-fix");
    // Ctrl+щелчок прошлого кадра: курсор уже на слове.
    if text_commands.contains(&"go-to-definition") {
        editor.go_to_definition(false);
    }
    // Ctrl+колесо над вкладкой — размер шрифта кода.
    let zoom = ui.input(|i| i.zoom_delta());
    if (zoom - 1.0).abs() > 1e-3 && ui.rect_contains_pointer(ui.max_rect()) {
        let size = &mut editor.prefs.code.font_size;
        *size = (*size * zoom).clamp(8.0, 32.0);
    }
    let code_prefs = editor.prefs.code.clone();
    // Набор этого кадра (буква, `.`, `:`) или Ctrl+Space — пересчитать подсказки.
    let typed = ui.input(|i| {
        i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':')))
    });
    let explicit_complete = text_commands.contains(&"complete");
    let want_hint = text_commands.contains(&"parameter-info");
    let project_symbols = if typed || explicit_complete || want_hint || editor.code.completion.is_some() {
        let fresh = editor.code.symbol_cache.as_ref().is_some_and(|(at, _)| now - at < 10.0);
        if !fresh {
            let symbols = std::sync::Arc::new(editor.project_symbols());
            editor.code.symbol_cache = Some((now, symbols));
        }
        editor.code.symbol_cache.as_ref().map(|(_, s)| s.clone()).unwrap_or_default()
    } else {
        std::sync::Arc::new(Vec::new())
    };
    let active = editor.code.active;
    let CodeState {
        documents,
        find,
        goto,
        cursor,
        checks,
        reveal,
        quick_fix,
        selection,
        selections,
        commands,
        completion,
        parameter_hint,
        first_diagnostic_at,
        geometry,
        recovery,
        ..
    } = &mut editor.code;
    let Some(document) = documents.get_mut(active) else {
        theme::empty_state(
            ui,
            Icon::Code,
            tr_s("No file is open"),
            tr_s("Pick a file in the tree on the left, or double-click a script\nin the Project window. Ctrl+S saves, Ctrl+B builds."),
        );
        return;
    };

    // --- Поиск ---
    // F3 / Shift+F3 / Ctrl+F3.
    let text_id_for_find = document.text_id();
    for command in text_commands.iter().filter(|c| matches!(**c, "find-next" | "find-prev" | "find-selected")) {
        let caret = crate::code_keys::selection(ui.ctx(), text_id_for_find, &document.text);
        if *command == "find-selected" {
            if let Some((a, c)) = caret {
                let range = if a != c { a.min(c)..a.max(c) } else { crate::code_symbols::ident_at(&document.text, c).unwrap_or(c..c) };
                if !range.is_empty() && !document.text[range.clone()].contains('\n') {
                    find.query = document.text[range].to_string();
                    find.open = true;
                }
            }
        }
        if find.query.is_empty() {
            find.open = true;
            find.focus = true;
            continue;
        }
        let (found, _) = search(&document.text, &find.query, find.options);
        if found.is_empty() {
            continue;
        }
        let at = caret.map_or(0, |(a, c)| a.max(c));
        let start = caret.map_or(0, |(a, c)| a.min(c));
        find.current = if *command == "find-prev" {
            found.iter().rposition(|m| m.start < start).unwrap_or(found.len() - 1)
        } else {
            found.iter().position(|m| m.start >= at).unwrap_or(0)
        };
        find.jump = true;
    }
    let (mut matches, error) = if find.open { search(&document.text, &find.query, find.options) } else { (Vec::new(), None) };
    find.error = error;
    if find.open {
        find_bar(ui, find, matches.len());
        // Запрос или параметры сменились в этом кадре — совпадения заново (переход к первому).
        if find.jump {
            let (found, error) = search(&document.text, &find.query, find.options);
            matches = found;
            find.error = error;
        }
        if find.replace {
            if let Some((new_text, caret)) = replace_bar(ui, find, &document.text, &matches) {
                replace_text_undoable(ui.ctx(), document.text_id(), document, new_text, caret..caret);
                matches = search(&document.text, &find.query, find.options).0;
            }
        }
    }
    let mut goto_request = None;
    if find.goto_line.is_some() {
        goto_request = goto_bar(ui, find, document.text.lines().count().max(1));
    }
    if !matches.is_empty() {
        find.current = find.current.min(matches.len() - 1);
    }

    // Меню быстрых исправлений забирает стрелки и Enter раньше текста.
    let mut chosen_fix = None;
    if let Some(menu) = quick_fix.as_mut().filter(|m| m.path == document.path) {
        match crate::code_view::quick_fix_menu(ui.ctx(), menu) {
            Ok(Some(fix)) => {
                chosen_fix = Some(fix);
                *quick_fix = None;
            }
            Ok(None) => {}
            Err(()) => *quick_fix = None,
        }
    }
    let text_id = document.text_id();
    if let Some(popup) = completion.as_mut().filter(|p| p.path == document.path) {
        match crate::code_complete::show(ui.ctx(), popup) {
            Ok(Some(item)) => {
                if let Some((_, caret)) = crate::code_keys::selection(ui.ctx(), text_id, &document.text) {
                    let (text, at) = crate::code_complete::accept(&document.text, popup.anchor.min(caret), caret, &item);
                    replace_text_undoable(ui.ctx(), text_id, document, text, at..at);
                }
                *completion = None;
            }
            Ok(None) => {}
            Err(()) => *completion = None,
        }
    }
    if let Some((hint, at)) = parameter_hint.as_ref() {
        egui::Area::new(egui::Id::new("code_parameter_hint")).fade_in(false).order(egui::Order::Tooltip).fixed_pos(*at).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(hint).font(theme::mono()).color(theme::TEXT_STRONG));
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter) || i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.contains(')')))) {
            *parameter_hint = None;
        }
    }
    if let Some(fix) = chosen_fix {
        apply_fix(ui.ctx(), text_id, document, &fix);
    }
    if text_commands.contains(&"format-document") {
        let unit = if document.format.indent.tabs { "\t".to_string() } else { " ".repeat(document.format.indent.size.max(1)) };
        match crate::code_format::format(&document.path, &document.text, document.language, &unit) {
            Ok(Some(formatted)) if formatted != document.text => {
                let caret = crate::code_keys::selection(ui.ctx(), text_id, &document.text).map_or(0, |(_, c)| c).min(formatted.len());
                let caret = crate::code_diagnostics::floor_char(&formatted, caret);
                replace_text_undoable(ui.ctx(), text_id, document, formatted, caret..caret);
            }
            Ok(_) => {}
            Err(error) => log::error!("{error}"),
        }
    }
    if document.recovered.is_some() {
        let mut restore = false;
        let mut discard = false;
        theme::toolbar_strip(ui, |ui| {
            icons::inline(ui, Icon::Warn, theme::WARN);
            ui.label(egui::RichText::new(tr_s("Unsaved changes from an earlier session were found (the editor closed without saving them).")).color(theme::WARN));
            if theme::button(ui, tr_s("Restore")).on_hover_text(tr_s("Put the unsaved text back (Ctrl+Z returns the file as it is on disk)")).clicked() {
                restore = true;
            }
            if theme::button(ui, tr_s("Discard")).on_hover_text(tr_s("Forget the unsaved text and keep the file as it is on disk")).clicked() {
                discard = true;
            }
        });
        if restore {
            if let Some(text) = document.recovered.take() {
                replace_text_undoable(ui.ctx(), text_id, document, text, 0..0);
            }
        }
        if discard {
            document.recovered = None;
            if let Some(dir) = recovery.dir.as_ref() {
                crate::code_recovery::remove(dir, &document.path);
            }
        }
    }
    if document.conflict {
        let mut reload = false;
        let mut keep = false;
        theme::toolbar_strip(ui, |ui| {
            icons::inline(ui, Icon::Warn, theme::WARN);
            ui.label(egui::RichText::new(tr_s("This file was changed on disk while it has unsaved edits here.")).color(theme::WARN));
            if theme::button(ui, tr_s("Reload from Disk")).on_hover_text(tr_s("Replace the text with the file on disk (Ctrl+Z brings your edits back)")).clicked() {
                reload = true;
            }
            if theme::button(ui, tr_s("Keep Mine")).on_hover_text(tr_s("Keep the edits here; saving overwrites the file on disk")).clicked() {
                keep = true;
            }
        });
        if reload {
            if let Ok((text, format)) = crate::code_files::read(&document.path) {
                replace_text_undoable(ui.ctx(), text_id, document, text.clone(), 0..0);
                document.saved = text;
                document.format = format;
            }
            document.disk_time = disk_stamp(&document.path);
            document.conflict = false;
        }
        if keep {
            document.disk_time = disk_stamp(&document.path);
            document.conflict = false;
        }
    }
    // Команды правки, нескольких курсоров и свёртки — у каждого курсора (набор, стрелки и
    // мышь обрабатывает сам виджет).
    let indent = if document.format.indent_known {
        document.format.indent
    } else {
        crate::code_files::Indent { tabs: !code_prefs.insert_spaces, size: code_prefs.tab_size }
    };
    if ui.ctx().memory(|m| m.has_focus(text_id)) {
        let mut keys = crate::code_keys::KeyContext { language: document.language, selections };
        for command in &text_commands {
            if *command == "quick-fix" || crate::code_keys::VIEW_COMMANDS.contains(command) {
                continue;
            }
            crate::code_keys::run(ui.ctx(), text_id, document, command, &mut keys);
        }
    }

    // --- Текст с номерами строк: свой виджет, раскладка только видимых строк ---
    let mut font = theme::mono();
    font.size = code_prefs.font_size;
    let wrap = code_prefs.word_wrap;
    let language = document.language;
    let mut look = crate::code_widget::Look::new(ui.ctx(), font.clone(), language);
    let line_height = look.line_h;
    let jump_to_match = find.jump && !matches.is_empty();
    let current_match = find.current;
    // Что выделить, когда текст разложен: (начало, конец в байтах, ставить ли фокус).
    let mut pending_select: Option<(usize, usize, bool)> = None;
    if goto.as_ref().is_some_and(|(path, _)| *path == document.path) {
        if let Some((_, line)) = goto.take() {
            let at = byte_of_line(&document.text, line);
            pending_select = Some((at, at, true));
        }
    }
    if reveal.as_ref().is_some_and(|(path, _)| *path == document.path) {
        if let Some((_, range)) = reveal.take() {
            let start = crate::code_diagnostics::floor_char(&document.text, range.start);
            pending_select = Some((start, start, true));
        }
    }
    if let Some(line) = goto_request {
        let at = byte_of_line(&document.text, line);
        pending_select = Some((at, at, true));
    }
    if jump_to_match {
        let range = matches[current_match].clone();
        pending_select = Some((range.start, range.end, false));
    }

    // Справа — полоса обзора во всю высоту.
    let full = ui.available_rect_before_wrap();
    let ruler_rect = Rect::from_min_max(pos2(full.right() - RULER_W, full.top()), full.max);
    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(full.min, pos2(full.right() - RULER_W, full.bottom()))));
    let mut view_lines = 0..0;
    let mut fix_request = None;
    let mut go_to_definition_click = false;
    let mut text_response: Option<egui::Response> = None;
    let ctx = ui.ctx().clone();
    let mut st = crate::code_widget::take(&ctx, text_id);
    st.observe(&document.text, language, now);
    document.sync(checks, now);
    let gutter_digits = st.index.lines().to_string().len().max(3);
    let modifiers = ui.input(|i| i.modifiers);
    egui::Frame::none().fill(theme::BG_INSET).show(&mut body, |ui| {
        let scroll = if wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        let glyph = look.cw;
        let gutter_w = gutter_digits as f32 * glyph + 28.0 + MARK_W;
        scroll
            .id_salt(("code_scroll", document.path.to_string_lossy().to_lowercase()))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.max_rect().min;
                let clip = ui.clip_rect();
                if wrap {
                    look.wrap_width = (clip.width() - gutter_w - theme::SP_6 - glyph * 2.0).max(glyph * 16.0);
                }
                let mut view = crate::code_widget::layout(&ctx, &mut st, &document.text, &look, viewport, origin, clip, gutter_w, now);
                // Область текста (с колонкой номеров) — высотой в текст, как у прежнего поля;
                // щелчок ниже текста ставит курсор в конец.
                let rect = Rect::from_min_max(pos2(clip.left(), origin.y), pos2(clip.right(), origin.y + view.content.y));
                let response = ui.interact(rect, text_id, Sense::click_and_drag());
                let below_rect = Rect::from_min_max(pos2(clip.left(), rect.bottom()), pos2(clip.right(), clip.bottom().max(rect.bottom())));
                let below = ui.interact(below_rect, text_id.with("below"), Sense::click_and_drag());
                text_response = Some(response.clone());
                if (response.hovered() || below.hovered()) && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| p.x >= view.text_clip.left()) {
                    ctx.set_cursor_icon(egui::CursorIcon::Text);
                }
                let focused = ctx.memory(|m| m.has_focus(text_id));
                if focused {
                    let escape = st.carets.is_multi() || st.box_sel.is_some() || st.preedit.is_some();
                    ctx.memory_mut(|m| m.set_focus_lock_filter(text_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape }));
                }
                // Команды, которым нужна раскладка: курсор выше / ниже, выделение столбцом.
                let mut ran_view_command = false;
                if focused {
                    let map = crate::code_widget::Mapper { ctx: &ctx, look: &look, view: &view };
                    for command in text_commands.iter().filter(|c| crate::code_keys::VIEW_COMMANDS.contains(c)) {
                        match *command {
                            "add-caret-above" => crate::code_widget::add_caret_vertical(&mut st, &document.text, &map, false),
                            "add-caret-below" => crate::code_widget::add_caret_vertical(&mut st, &document.text, &map, true),
                            "box-select-left" => crate::code_widget::box_select_step(&mut st, &document.text, &map, egui::Key::ArrowLeft),
                            "box-select-right" => crate::code_widget::box_select_step(&mut st, &document.text, &map, egui::Key::ArrowRight),
                            "box-select-up" => crate::code_widget::box_select_step(&mut st, &document.text, &map, egui::Key::ArrowUp),
                            "box-select-down" => crate::code_widget::box_select_step(&mut st, &document.text, &map, egui::Key::ArrowDown),
                            _ => {}
                        }
                        ran_view_command = true;
                    }
                }
                let env = crate::code_widget::TypingEnv { prefs: &code_prefs, indent, now, focused: ctx.memory(|m| m.has_focus(text_id)) };
                let input = {
                    let map = crate::code_widget::Mapper { ctx: &ctx, look: &look, view: &view };
                    crate::code_widget::interact(ui, &mut st, &mut document.text, &map, &response, &below, &env)
                };
                let had_pending = pending_select.is_some();
                if let Some((start, end, focus)) = pending_select.take() {
                    let len = document.text.len();
                    let (start, end) = (start.min(len), end.min(len).max(start.min(len)));
                    // Место в свёрнутом блоке — развернуть.
                    st.reveal(start);
                    st.set_selection(&document.text, start, end);
                    st.scroll_to_caret = Some(true);
                    if focus {
                        ctx.memory_mut(|m| m.request_focus(text_id));
                    }
                }
                st.observe(&document.text, language, now);
                document.sync(checks, now);
                if input.changed || input.moved || had_pending || ran_view_command {
                    view = crate::code_widget::layout(&ctx, &mut st, &document.text, &look, viewport, origin, clip, gutter_w, now);
                }
                ui.set_min_size(view.content);
                let focused = ctx.memory(|m| m.has_focus(text_id));
                let primary = st.carets.primary();
                // Прокрутить к главному курсору (с запасом слева на колонку номеров).
                let caret_rect = {
                    let map = crate::code_widget::Mapper { ctx: &ctx, look: &look, view: &view };
                    map.caret_rect(&mut st, &document.text, primary.caret)
                };
                if let Some(center) = st.scroll_to_caret.take() {
                    let target = Rect::from_min_max(pos2(caret_rect.left() - gutter_w - theme::SP_6 - glyph, caret_rect.top()), pos2(caret_rect.right() + glyph * 2.0, caret_rect.bottom()));
                    ui.scroll_to_rect(target, center.then_some(egui::Align::Center));
                }
                if focused {
                    ctx.output_mut(|o| o.ime = Some(egui::output::IMEOutput { rect: view.text_clip, cursor_rect: caret_rect }));
                }

                crate::code_widget::paint_under(ui.painter(), &st, &document.text, &view, &matches, current_match, focused);
                crate::code_widget::paint_text(ui.painter(), &view, look.ppp);

                // Ctrl+щелчок — к определению (курсор уже встал под указатель).
                if response.clicked() && modifiers.command && !modifiers.alt {
                    go_to_definition_click = true;
                }
                *geometry = Some((pos2(view.text_left, view.line_top(0)), view.cw, view.line_h));
                let laid = crate::code_view::Laid { view: &view, text: &document.text, index: &st.index };
                view_lines = laid.visible_lines(clip);
                let painter = ui.painter().with_clip_rect(view.text_clip);
                let caret_byte = focused.then_some(primary.caret);
                // Направляющие отступов, линейка, пробелы, парная скобка.
                crate::code_view::paint_guides(&painter, &laid, clip, indent.size.max(1), glyph, code_prefs.show_whitespace);
                if code_prefs.ruler > 0 {
                    let x = view.text_left + code_prefs.ruler as f32 * glyph;
                    painter.vline(x.round() + 0.5, clip.y_range(), Stroke::new(1.0_f32, theme::LINE_SOFT));
                }
                if let Some(at) = caret_byte {
                    if let Some((a, b)) = crate::code_edit::matching_bracket(&document.text, at) {
                        for o in [a, b] {
                            let r = laid.char_rect(o);
                            let w = laid.char_rect(o + 1).left() - r.left();
                            let rect = Rect::from_min_size(r.min, vec2(w.max(glyph), r.height()));
                            painter.rect_stroke(rect, theme::R_S, Stroke::new(1.0_f32, theme::TEXT_MUTED));
                        }
                    }
                }

                // Диагностики: волны, значки, подсказка при наведении.
                let diagnostics = crate::code_checks::of_document(document, checks);
                crate::code_view::paint_squiggles(&painter, &laid, &diagnostics, clip);
                *first_diagnostic_at = diagnostics.iter().filter(|d| !laid.is_hidden(st.index.line_of(d.range.start))).min_by_key(|d| d.range.start).map(|d| laid.char_rect(d.range.start).center() + vec2(2.0, 0.0));

                let folded = crate::code_widget::paint_over(ui, &st, &document.text, &view, &look, focused, st.index.line_of(primary.caret) + 1, text_id);
                let gutter_painter = ui.painter().with_clip_rect(view.gutter.intersect(clip));
                crate::code_view::paint_gutter_marks(&gutter_painter, &laid, &diagnostics, view.gutter.left() + MARK_W * 0.5 + 2.0, clip);
                if let Some(pointer) = ctx.pointer_hover_pos().filter(|p| clip.contains(*p)) {
                    let hovered = if view.gutter.contains(pointer) {
                        let line = view.line_at_y(pointer.y);
                        diagnostics.iter().find(|d| st.index.line_of(d.range.start) == line).copied()
                    } else if view.text_clip.contains(pointer) && rect.contains(pointer) {
                        let at = laid.offset_at(pointer);
                        let row = laid.char_rect(at);
                        let near = (pointer.y - row.center().y).abs() <= row.height();
                        diagnostics
                            .iter()
                            .filter(|d| near && (d.range.contains(&at) || d.range.end == at || (d.range.is_empty() && d.range.start == at)))
                            .min_by_key(|d| d.severity)
                            .copied()
                    } else {
                        None
                    };
                    if let Some(d) = hovered {
                        egui::show_tooltip_at_pointer(&ctx, ui.layer_id(), egui::Id::new("code_diagnostic_tip"), |ui| {
                            crate::code_view::diagnostic_tooltip(ui, d, &fix_keys);
                        });
                    }
                }

                // Строка состояния: строка и столбец главного курсора, длина выделений.
                let (line, column) = st.index.line_col(&document.text, primary.caret);
                *cursor = (line + 1, column + 1);
                *selection = st.carets.sels.iter().map(|s| document.text[s.range()].chars().count()).sum();
                // Ctrl+. / Alt+Enter: быстрые исправления под курсором (или на его строке).
                if quick_fix_pressed {
                    let at = primary.caret;
                    let mut fixes: Vec<_> = diagnostics.iter().filter(|d| d.range.start <= at && at <= d.range.end).flat_map(|d| d.fixes.iter().cloned()).collect();
                    if fixes.is_empty() {
                        fixes = diagnostics.iter().filter(|d| st.index.line_of(d.range.start) == line).flat_map(|d| d.fixes.iter().cloned()).collect();
                    }
                    if !fixes.is_empty() {
                        fix_request = Some((fixes, laid.char_rect(at).left_bottom()));
                    }
                }
                if let Some(at) = caret_byte {
                    if typed || explicit_complete {
                        let (anchor, items) = crate::code_complete::complete(&document.path, &document.text, at, language, &project_symbols, explicit_complete);
                        *completion = (!items.is_empty()).then(|| crate::code_complete::Popup { path: document.path.clone(), anchor, items, selected: 0, at: laid.char_rect(at).left_bottom() + vec2(0.0, 2.0) });
                    }
                    if want_hint {
                        *parameter_hint = crate::code_complete::parameter_hint(&document.path, &document.text, at, &project_symbols).map(|h| (h, laid.char_rect(at).left_top() - vec2(0.0, line_height + 8.0)));
                    }
                }
                if let Some(header) = folded {
                    st.toggle_fold_at(&document.text, language, header, now);
                    ctx.request_repaint();
                }
            });
    });

    // Полоса обзора: диагностики, совпадения поиска, закладки, курсор.
    let ruler_jump = {
        use crate::code_view::{Lane, RulerMark};
        let index = &st.index;
        let mut marks = Vec::new();
        for m in matches.iter().take(2000) {
            marks.push(RulerMark { line: index.line_of(m.start), color: theme::WARN.gamma_multiply(0.7), lane: Lane::Center });
        }
        for b in &document.bookmarks {
            marks.push(RulerMark { line: index.line_of(*b), color: theme::accent(), lane: Lane::Left });
        }
        let mut diagnostics = crate::code_checks::of_document(document, checks);
        diagnostics.sort_by_key(|d| std::cmp::Reverse(d.severity));
        for d in diagnostics {
            let mut color = crate::code_view::severity_color(d.severity);
            if d.stale {
                color = color.gamma_multiply(0.5);
            }
            marks.push(RulerMark { line: index.line_of(d.range.start), color, lane: Lane::Right });
        }
        marks.push(RulerMark { line: cursor.0.saturating_sub(1), color: theme::TEXT, lane: Lane::Full });
        crate::code_view::overview_ruler(ui, ruler_rect, index.lines(), &marks, view_lines.clone(), egui::Id::new("code_ruler"))
    };
    st.end_frame();
    crate::code_widget::put(&ctx, text_id, st);
    if let Some(line) = ruler_jump {
        *goto = Some((document.path.clone(), line + 1));
    }
    if go_to_definition_click {
        commands.push("go-to-definition");
    }
    if let Some((fixes, at)) = fix_request {
        *quick_fix = Some(crate::code_view::QuickFixMenu { path: document.path.clone(), at, fixes, selected: 0 });
    }
    find.jump = false;
    // Правый щелчок по тексту — меню.
    if let Some(response) = text_response {
        crate::code_menu::text_menu(editor, &response);
    }
}

/// Форматировать документ перед записью (ошибка форматирования — файл сохраняется как есть).
fn format_before_save(document: &mut Document) {
    let unit = if document.format.indent.tabs { "\t".to_string() } else { " ".repeat(document.format.indent.size.max(1)) };
    match crate::code_format::format(&document.path, &document.text, document.language, &unit) {
        Ok(Some(formatted)) => document.text = formatted,
        Ok(None) => {}
        Err(error) => log::warn!("{error}"),
    }
}

/// Ширина полосы обзора справа от текста и колонки значков в колонке номеров.
const RULER_W: f32 = 12.0;
const MARK_W: f32 = crate::code_widget::MARK_W;

/// Байт начала строки `line` (с 1).
fn byte_of_line(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n').nth(line - 2).map_or(text.len(), |(i, _)| i + 1)
}

/// Заменить текст документа одним шагом Undo и поставить выделение `cursor`
/// (курсор — в его конце).
pub(crate) fn replace_text_undoable(ctx: &egui::Context, id: egui::Id, document: &mut Document, new_text: String, cursor: std::ops::Range<usize>) {
    let now = ctx.input(|i| i.time);
    let mut st = crate::code_widget::take(ctx, id);
    st.replace_all(&mut document.text, new_text, cursor.start, cursor.end.max(cursor.start), now);
    crate::code_widget::put(ctx, id, st);
}

/// Применить быстрое исправление (один шаг Undo).
pub(crate) fn apply_fix(ctx: &egui::Context, id: egui::Id, document: &mut Document, fix: &crate::code_diagnostics::QuickFix) {
    let text = crate::code_diagnostics::apply_edits(&document.text, &fix.edits);
    let caret = fix.edits.iter().map(|e| e.range.start + e.text.len()).min().unwrap_or(0);
    replace_text_undoable(ctx, id, document, text, caret..caret);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// Вкладка открытого файла: иконка языка, имя, точка «не сохранено» или крестик.
fn document_tab(ui: &mut egui::Ui, index: usize, document: &Document, selected: bool) -> (egui::Response, bool, bool) {
    let title = document.title();
    let galley = ui.painter().layout_no_wrap(title, theme::body(), Color32::PLACEHOLDER);
    let width = theme::SP_L + 14.0 + 7.0 + galley.size().x + 8.0 + 20.0 + theme::SP_S;
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::TAB_BAR - 4.0), Sense::hover());
    let response = ui.interact(rect, egui::Id::new(("code_tab", index)), Sense::click());
    let painter = ui.painter();
    let rounding = egui::Rounding { nw: theme::R_S, ne: theme::R_S, sw: 0.0, se: 0.0 };
    if selected {
        painter.rect_filled(rect, rounding, theme::BG_INSET);
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 2.0)), rounding, theme::accent());
    } else if response.hovered() {
        painter.rect_filled(rect, rounding, theme::mix(theme::BG_HEADER, Color32::WHITE, 0.05));
    }
    let icon = match document.language {
        Language::Rust | Language::Cpp | Language::Wgsl | Language::Scenario => Icon::Script,
        Language::Ron => Icon::Scene,
        Language::Plain => Icon::Text,
    };
    let text_color = if selected { theme::TEXT } else { theme::TEXT_MUTED };
    icons::paint(
        painter,
        Rect::from_center_size(pos2(rect.left() + theme::SP_L + 7.0, rect.center().y), vec2(14.0, 14.0)),
        icon,
        if selected { theme::TEXT } else { theme::TEXT_DISABLED },
    );
    painter.galley(
        pos2(rect.left() + theme::SP_L + 21.0, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );
    let close_rect = Rect::from_center_size(pos2(rect.right() - theme::SP_S - 10.0, rect.center().y), vec2(20.0, 20.0));
    let close = ui.interact(close_rect, egui::Id::new(("code_tab_close", index)), Sense::click());
    if document.pinned {
        // Закреплённая: значок замка; щелчок по нему открепляет.
        if close.hovered() {
            ui.painter().rect_filled(close_rect, theme::R_S, theme::BG_HOVER);
        }
        icons::paint(ui.painter(), close_rect.shrink(5.0), Icon::Lock, if document.dirty() { theme::TEXT } else { theme::TEXT_MUTED });
        let unpinned = close.on_hover_text(tr_s("Pinned tab — click to unpin")).clicked();
        return (response, false, unpinned);
    }
    if document.dirty() && !close.hovered() {
        ui.painter().circle_filled(close_rect.center(), 4.0, theme::TEXT);
    } else if close.hovered() || response.hovered() || selected {
        if close.hovered() {
            ui.painter().rect_filled(close_rect, theme::R_S, theme::BG_HOVER);
        }
        icons::paint(ui.painter(), close_rect.shrink(6.0), Icon::Close, theme::TEXT_MUTED);
    }
    let tip = if document.dirty() { tr_s("Close (asks to save unsaved changes)") } else { tr_s("Close") };
    let closed = close.on_hover_text(tip).clicked();
    (response, closed, false)
}

fn editor_display(code_dirs: &[PathBuf], path: &Path) -> String {
    for dir in code_dirs {
        if let Ok(relative) = path.strip_prefix(dir) {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return format!("{name}/{}", relative.to_string_lossy().replace('\\', "/"));
        }
    }
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Полоса поиска: поле, «3 из 12», вверх / вниз, закрыть.
/// Полоса замены под поиском: «на что», Replace (текущее совпадение) и Replace All.
/// `true` — текст изменился.
fn replace_bar(ui: &mut egui::Ui, find: &mut Find, text: &str, matches: &[std::ops::Range<usize>]) -> Option<(String, usize)> {
    let mut result = None;
    theme::toolbar_strip(ui, |ui| {
        crate::controls::text_field(ui, &mut find.replacement, tr_s("Replace with"), 300.0);
        let preserve = crate::controls::toggle_button(ui, find.options.preserve_case, "AB") // i18n-skip: значок «сохранить регистр»
            .on_hover_text(tr_s("Preserve case: `speed` → `pace` turns `Speed` into `Pace` and `SPEED` into `PACE`"));
        if preserve.clicked() {
            find.options.preserve_case = !find.options.preserve_case;
        }
        let found = crate::code_find::find_all(text, &find.query, find.options).unwrap_or_default();
        let current = found.get(find.current.min(found.len().saturating_sub(1))).cloned();
        let tip = if find.options.regex { tr_s("Replace the current match and go to the next one. `$1`, `$2`… insert the groups of the expression") } else { tr_s("Replace the current match and go to the next one") };
        if ui.add_enabled_ui(current.is_some(), |ui| theme::button(ui, tr_s("Replace"))).inner.on_hover_text(tip).clicked() {
            if let Some(m) = current {
                let with = crate::code_find::replacement(text, &m, &find.replacement, find.options);
                let mut out = text.to_string();
                out.replace_range(m.range.clone(), &with);
                find.jump = true;
                result = Some((out, m.range.start + with.len()));
            }
        }
        if ui.add_enabled_ui(!matches.is_empty(), |ui| theme::button(ui, tr_s("Replace All"))).inner.clicked() {
            let out = crate::code_find::replace_all(text, &found, &find.replacement, find.options);
            find.current = 0;
            result = Some((out, 0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Close, tr_s("Hide replace")).clicked() {
                find.replace = false;
            }
        });
    });
    result
}

/// Заменить все совпадения (диапазоны не пересекаются и идут по порядку).
#[cfg(test)]
fn replace_all(text: &str, matches: &[std::ops::Range<usize>], replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for range in matches {
        out.push_str(&text[last..range.start]);
        out.push_str(replacement);
        last = range.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Полоса «перейти к строке» (Ctrl+G): номер и Enter. Возвращает строку для перехода.
fn goto_bar(ui: &mut egui::Ui, find: &mut Find, lines: usize) -> Option<usize> {
    let mut go = None;
    let mut close = false;
    let text = find.goto_line.as_mut()?;
    theme::toolbar_strip(ui, |ui| {
        ui.label(egui::RichText::new(tr_s("Go to line")).color(theme::TEXT_MUTED));
        let response = crate::controls::text_field(ui, text, &trf!("1 – {lines}", lines = lines), 90.0);
        if std::mem::take(&mut find.goto_focus) {
            response.request_focus();
        }
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let number = text.trim().parse::<usize>().ok().filter(|n| (1..=lines).contains(n));
        if theme::button(ui, tr_s("Go")).clicked() || enter {
            go = number;
            close = number.is_some();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Close, tr_s("Close (Esc)")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if close {
        find.goto_line = None;
    }
    go
}

fn find_bar(ui: &mut egui::Ui, find: &mut Find, count: usize) {
    theme::toolbar_strip(ui, |ui| {
        let response = theme::search_field(ui, &mut find.query, tr_s("Find in file"), 300.0);
        if find.focus {
            response.request_focus();
            find.focus = false;
        }
        if response.changed() {
            find.current = 0;
            find.jump = true;
        }
        let toggles = [
            ("Aa", tr_s("Match case"), 0), // i18n-skip: значок
            ("W", tr_s("Match whole word"), 1), // i18n-skip: значок
            (".*", tr_s("Use regular expressions: . \\d \\w \\s \\b [a-z] ^ $ ( ) | * + ? {n,m}"), 2), // i18n-skip: значок
        ];
        for (label, tip, which) in toggles {
            let on = [find.options.match_case, find.options.whole_word, find.options.regex][which];
            if crate::controls::toggle_button(ui, on, label).on_hover_text(tip).clicked() {
                match which {
                    0 => find.options.match_case = !on,
                    1 => find.options.whole_word = !on,
                    _ => find.options.regex = !on,
                }
                find.current = 0;
                find.jump = true;
            }
        }
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let shift = ui.input(|i| i.modifiers.shift);
        let summary = match count {
            _ if find.error.is_some() => find.error.clone().unwrap_or_default(),
            0 if find.query.is_empty() => String::new(),
            0 => tr_s("No results").to_string(),
            n => format!("{} of {n}", find.current.min(n - 1) + 1),
        };
        ui.label(egui::RichText::new(summary).color(if count == 0 && !find.query.is_empty() { theme::ERROR } else { theme::TEXT_MUTED }));
        let mut step = |forward: bool| {
            if count > 0 {
                find.current = if forward { (find.current + 1) % count } else { (find.current + count - 1) % count };
                find.jump = true;
            }
        };
        if icons::button(ui, Icon::ChevronDown, false, None, tr_s("Next match (Enter)")).clicked() || (enter && !shift) {
            step(true);
        }
        if icons::button(ui, Icon::ChevronUp, false, None, tr_s("Previous match (Shift+Enter)")).clicked() || (enter && shift) {
            step(false);
        }
        if enter {
            response.request_focus();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Close, tr_s("Close (Esc)")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                find.open = false;
            }
        });
    });
}

/// Схема (outline) активного документа: символы по порядку, щелчок — перейти.
fn outline(editor: &mut Editor, ui: &mut egui::Ui) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::TAB_BAR), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, theme::BG_HEADER);
    ui.painter().hline(bar.x_range(), bar.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
    ui.painter().text(pos2(bar.left() + theme::SP_L, bar.center().y), Align2::LEFT_CENTER, tr_s("OUTLINE"), theme::strong(theme::FONT_SECTION), theme::TEXT_MUTED);
    let Some(doc) = editor.code.documents.get(editor.code.active) else { return };
    // Большой файл перестраивает схему, только когда печатать перестали (полсекунды).
    let now = ui.input(|i| i.time);
    let other_file = editor.code.outline.as_ref().is_none_or(|(p, _, _)| *p != doc.path);
    let paused = doc.text.len() < 200_000 || now - doc.changed_at > 0.5;
    let stale = other_file || (paused && editor.code.outline.as_ref().is_some_and(|(_, v, _)| *v != doc.version));
    if stale {
        editor.code.outline = Some((doc.path.clone(), doc.version, crate::code_symbols::symbols(&doc.path, &doc.text)));
    }
    let path = doc.path.clone();
    let current_line = editor.code.cursor.0;
    // Без копии: у большого файла символов десятки тысяч.
    let empty = Vec::new();
    let symbols = editor.code.outline.as_ref().map_or(&empty, |(_, _, s)| s);
    if symbols.is_empty() {
        ui.add_space(theme::SP_4);
        crate::controls::muted(ui, tr_s("No symbols in this file"));
        return;
    }
    // Символ, в котором стоит курсор, — подсвечен.
    let here = symbols.iter().rposition(|s| s.line <= current_line);
    let mut go = None;
    egui::ScrollArea::vertical().id_salt("code_outline_scroll").auto_shrink([false, false]).show_rows(ui, theme::ROW, symbols.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for k in range {
            let s = &symbols[k];
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click());
            theme::list_row(ui.painter(), rect, here == Some(k), false, response.hovered());
            let depth = if s.container.is_some() { 1.0 } else { 0.0 };
            let x = rect.left() + theme::SP_M + depth * theme::INDENT;
            let painter = ui.painter_at(rect);
            painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, crate::code_symbols::kind_text(s.kind.label()), theme::caption(), theme::INFO);
            painter.text(pos2(x + 62.0, rect.center().y), Align2::LEFT_CENTER, &s.name, theme::body(), theme::TEXT);
            if response.on_hover_text(&s.signature).clicked() {
                go = Some(s.range.start);
            }
        }
    });
    if let Some(at) = go {
        editor.go_to_place(&path, at, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_all_swaps_every_match_and_keeps_the_rest() {
        let text = "let speed = 1.0; speed += speed;";
        let matches = find_matches(text, "speed");
        assert_eq!(matches.len(), 3);
        assert_eq!(replace_all(text, &matches, "pace"), "let pace = 1.0; pace += pace;");
        assert_eq!(replace_all(text, &[], "x"), text);
    }

    fn kinds(text: &str, language: Language) -> Vec<(Token, &str)> {
        tokenize(text, language)
            .into_iter()
            .filter(|(_, range)| !text[range.clone()].trim().is_empty())
            .map(|(token, range)| (token, &text[range]))
            .collect()
    }

    #[test]
    fn rust_tokens_are_classified() {
        let tokens = kinds("pub fn jump(&mut self) { log::info!(\"up {}\", 1.5); } // go", Language::Rust);
        assert!(tokens.contains(&(Token::Keyword, "pub")));
        assert!(tokens.contains(&(Token::Keyword, "fn")));
        assert!(tokens.contains(&(Token::Function, "jump")));
        assert!(tokens.contains(&(Token::Macro, "info")));
        assert!(tokens.contains(&(Token::String, "\"up {}\"")));
        assert!(tokens.contains(&(Token::Number, "1.5")));
        assert!(tokens.contains(&(Token::Comment, "// go")));
        // Время жизни — не символьный литерал, а `!=` — не макрос.
        let tokens = kinds("fn f<'a>(x: &'a str) { if x != y {} }", Language::Rust);
        assert!(!tokens.iter().any(|(token, _)| *token == Token::String), "{tokens:?}");
        assert!(!tokens.iter().any(|(token, _)| *token == Token::Macro), "{tokens:?}");
    }

    #[test]
    fn scenario_scripts_from_the_assets_are_listed_next_to_the_code() {
        let root = crate::code_tests::temp_dir("scan_scenarios");
        let (src, assets) = (root.join("src"), root.join("assets"));
        std::fs::create_dir_all(src.join("scripts")).unwrap();
        std::fs::create_dir_all(assets.join("scenarios/levels")).unwrap();
        std::fs::write(src.join("scripts/player.rs"), "").unwrap();
        std::fs::write(assets.join("scenarios/intro.scenario"), "").unwrap();
        std::fs::write(assets.join("scenarios/levels/wave_1.scenario"), "").unwrap();
        std::fs::write(assets.join("scenarios/notes.txt"), "").unwrap();
        std::fs::write(assets.join("hero.png"), "").unwrap();

        let mut code = CodeState::default();
        code.scan(std::slice::from_ref(&src), Some(&assets));
        let names: Vec<&str> = code.files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["assets/scenarios/intro.scenario", "assets/scenarios/levels/wave_1.scenario", "src/scripts/player.rs"]);

        // Без ассетов — как раньше; ассеты как каталог с кодом — без повторов.
        code.scan(std::slice::from_ref(&src), None);
        assert_eq!(code.files.len(), 1);
        code.scan(std::slice::from_ref(&assets), Some(&assets));
        let scenarios = code.files.iter().filter(|(name, _)| name.ends_with(".scenario")).count();
        assert_eq!(scenarios, 2, "{:?}", code.files);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scenario_tokens_are_classified() {
        let text = "-- intro\nlocal hint = 'Лови!' // js\nfunction go() { wait(1) }\nlet t = `a ${b}\nc`\n--[[ block\n]] end";
        let tokens = kinds(text, Language::Scenario);
        assert!(tokens.contains(&(Token::Comment, "-- intro")), "{tokens:?}");
        assert!(tokens.contains(&(Token::Keyword, "local")));
        assert!(tokens.contains(&(Token::String, "'Лови!'")));
        assert!(tokens.contains(&(Token::Comment, "// js")));
        assert!(tokens.contains(&(Token::Keyword, "function")));
        assert!(tokens.contains(&(Token::Function, "wait")));
        assert!(tokens.contains(&(Token::String, "`a ${b}\nc`")));
        assert!(tokens.contains(&(Token::Comment, "--[[ block\n]]")));
        assert!(tokens.contains(&(Token::Keyword, "end")));
        assert_eq!(Language::of(Path::new("scenarios/intro.scenario")), Language::Scenario);
    }

    #[test]
    fn scenario_highlighting_never_panics_and_covers_any_input() {
        // Детерминированный «fuzz»: случайные склейки кусков и байтов, включая обрывки
        // многобайтных символов, незакрытые строки и комментарии.
        let pieces = ["--", "--[[", "]]", "'", "`", "${", "}", "\\", "\n", "Ё", "ж", "😀", "let", "x", " ", "\"", "/*", "*/", "//", "1.5", "#"];
        let mut state: u64 = 7;
        for _ in 0..3000 {
            let mut text = String::new();
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = (state >> 33) % 24;
            for _ in 0..n {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push_str(pieces[((state >> 33) as usize) % pieces.len()]);
            }
            let rebuilt: String = tokenize(&text, Language::Scenario).into_iter().map(|(_, range)| &text[range]).collect();
            assert_eq!(rebuilt, text);
        }
    }

    #[test]
    fn cpp_tokens_are_classified() {
        let text = "#include \"engine_component.hpp\"\nstruct Spinner : engine::Component { float speed = 90.0f; /* deg */ };";
        let tokens = kinds(text, Language::Cpp);
        assert_eq!(tokens[0], (Token::Macro, "#include \"engine_component.hpp\""));
        assert!(tokens.contains(&(Token::Keyword, "struct")));
        assert!(tokens.contains(&(Token::Type, "Spinner")));
        assert!(tokens.contains(&(Token::Keyword, "float")));
        assert!(tokens.contains(&(Token::Number, "90.0f")));
        assert!(tokens.contains(&(Token::Comment, "/* deg */")));
    }

    #[test]
    fn tokens_cover_the_whole_text_even_when_it_is_broken() {
        for text in ["let s = \"unterminated", "/* never closed", "x = 'a", "привет // мир", ""] {
            for language in [Language::Rust, Language::Cpp, Language::Ron, Language::Plain] {
                let rebuilt: String = tokenize(text, language)
                    .into_iter()
                    .map(|(_, range)| &text[range])
                    .collect();
                assert_eq!(rebuilt, text, "{language:?}");
            }
        }
    }
}
