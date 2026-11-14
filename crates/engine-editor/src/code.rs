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

use egui::text::{LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

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
            "wgsl" => Language::Wgsl,
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

    fn keywords(self) -> &'static [&'static str] {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Plain,
    Keyword,
    Type,
    Function,
    Macro,
    String,
    Number,
    Comment,
}

fn token_color(token: Token) -> Color32 {
    match token {
        Token::Plain => theme::TEXT,
        // Приглушённая схема в духе IDE JetBrains: читается, но не пёстрая.
        Token::Keyword => Color32::from_rgb(0xcf, 0x8e, 0x6d),
        Token::Type => Color32::from_rgb(0x7f, 0xa8, 0xd8),
        Token::Function => Color32::from_rgb(0xd6, 0xbf, 0x7c),
        Token::Macro => Color32::from_rgb(0xb3, 0x96, 0xcf),
        Token::String => Color32::from_rgb(0x8e, 0xb5, 0x7c),
        Token::Number => Color32::from_rgb(0x6f, 0xaa, 0xb8),
        Token::Comment => theme::TEXT_DISABLED,
    }
}

/// Разбить текст на окрашенные отрезки. Разбор лексический и прощающий: незакрытая
/// строка или комментарий просто тянутся до конца — при наборе текст всегда «сломан».
fn tokenize(text: &str, language: Language) -> Vec<(Token, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let keywords = language.keywords();
    let c_like = !matches!(language, Language::Plain);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();
    let mut push = |token: Token, range: std::ops::Range<usize>| {
        match tokens.last_mut() {
            // Соседние отрезки одного цвета склеиваются: меньше секций в раскладке.
            Some((last, last_range)) if *last == token && last_range.end == range.start => {
                last_range.end = range.end;
            }
            _ => tokens.push((token, range)),
        }
    };

    let mut i = 0;
    let mut line_start = true;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if !c_like {
            i = bytes.len();
            push(Token::Plain, start..i);
            break;
        }
        if c == b'\n' {
            i += 1;
            line_start = true;
            push(Token::Plain, start..i);
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' {
            i += 1;
            push(Token::Plain, start..i);
            continue;
        }
        let at_line_start = std::mem::replace(&mut line_start, false);

        if language == Language::Scenario && c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            // Комментарий Scenario: `-- …` до конца строки или `--[[ … ]]`.
            if bytes.get(i + 2) == Some(&b'[') && bytes.get(i + 3) == Some(&b'[') {
                i += 4;
                while i < bytes.len() && !(bytes[i] == b']' && bytes.get(i + 1) == Some(&b']')) {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            } else {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            push(Token::Comment, start..i);
        } else if language == Language::Scenario && (c == b'\'' || c == b'`') {
            // Строка в одинарных кавычках или шаблон `…${x}…` (может занимать несколько строк).
            i += 1;
            while i < bytes.len() && bytes[i] != c && (c == b'`' || bytes[i] != b'\n') {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            push(Token::String, start..i);
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            push(Token::Comment, start..i);
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            push(Token::Comment, start..i);
        } else if c == b'#' && language == Language::Cpp && at_line_start {
            // Директива препроцессора — до конца строки.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            push(Token::Macro, start..i);
        } else if c == b'#' && language == Language::Rust && matches!(bytes.get(i + 1), Some(b'[') | Some(b'!')) {
            // Атрибут `#[...]`.
            while i < bytes.len() && bytes[i] != b']' && bytes[i] != b'\n' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            push(Token::Macro, start..i);
        } else if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            push(Token::String, start..i);
        } else if c == b'\'' && language != Language::Ron {
            // Символьный литерал `'x'` / `'\n'`; время жизни `'a` в Rust — не литерал.
            let close = if bytes.get(i + 1) == Some(&b'\\') { i + 3 } else { i + 2 };
            if bytes.get(close) == Some(&b'\'') {
                i = close + 1;
                push(Token::String, start..i);
            } else {
                i += 1;
                push(Token::Plain, start..i);
            }
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_') {
                i += 1;
            }
            push(Token::Number, start..i);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            let next = bytes[i..].iter().copied().find(|b| *b != b' ');
            let token = if keywords.contains(&word) {
                Token::Keyword
            } else if language == Language::Rust
                && bytes.get(i) == Some(&b'!')
                && bytes.get(i + 1) != Some(&b'=')
            {
                // `name!` вплотную — макрос; `x != y` — нет.
                Token::Macro
            } else if next == Some(b'(') {
                Token::Function
            } else if word.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                Token::Type
            } else {
                Token::Plain
            };
            push(token, start..i);
        } else {
            // Знак препинания или не-ASCII символ (целиком, по границе UTF-8).
            i += 1;
            while i < bytes.len() && !text.is_char_boundary(i) {
                i += 1;
            }
            push(Token::Plain, start..i);
        }
    }
    tokens
}

fn highlight(text: &str, language: Language, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for (token, range) in tokenize(text, language) {
        job.append(
            &text[range],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: token_color(token),
                ..TextFormat::default()
            },
        );
    }
    job
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
    /// Сколько миллисекунд шла последняя проверка.
    pub(crate) check_ms: f32,
    /// Закладки: байты начал строк (Ctrl+K Ctrl+K).
    pub(crate) bookmarks: Vec<usize>,
    /// Время изменения файла на диске, когда его читали или писали.
    pub(crate) disk_time: Option<std::time::SystemTime>,
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
            check_ms: 0.0,
            bookmarks: Vec::new(),
            disk_time: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
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
        self.disk_time = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
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
struct Find {
    open: bool,
    query: String,
    /// Номер текущего совпадения.
    current: usize,
    /// Перейти к текущему совпадению в этом кадре.
    jump: bool,
    focus: bool,
    /// Строка «Replace» (Ctrl+H) под поиском и то, на что заменять.
    replace: bool,
    replacement: String,
    /// Строка «Go to line» (Ctrl+G): введённый номер; фокус в поле при открытии.
    goto_line: Option<String>,
    goto_focus: bool,
}

/// Подсветка последнего показанного текста: пересчитывается, только когда текст
/// или поиск изменились (большой файл не размечается заново каждый кадр).
struct HighlightCache {
    key: u64,
    job: LayoutJob,
}

pub(crate) struct CodeState {
    documents: Vec<Document>,
    active: usize,
    build: Build,
    /// Файлы каталогов с кодом (относительное имя для показа, полный путь).
    pub files: Vec<(String, PathBuf)>,
    find: Find,
    /// Перейти к строке файла, когда он будет показан: (файл, строка с 1).
    goto: Option<(PathBuf, usize)>,
    cache: Option<HighlightCache>,
    /// Свёрнутые каталоги дерева файлов.
    collapsed: std::collections::BTreeSet<String>,
    /// Строка и столбец курсора (с 1) — для строки состояния.
    cursor: (usize, usize),
}

impl Default for CodeState {
    fn default() -> Self {
        Self {
            documents: Vec::new(),
            active: 0,
            build: Build {
                status: Arc::new(Mutex::new(BuildStatus::Idle)),
                seen: BuildStatus::Idle,
            },
            files: Vec::new(),
            find: Find::default(),
            goto: None,
            cache: None,
            collapsed: std::collections::BTreeSet::new(),
            cursor: (1, 1),
        }
    }
}

const CODE_EXTENSIONS: [&str; 16] = [
    "rs", "cpp", "cc", "cxx", "c", "h", "hpp", "wgsl", "toml", "txt", "md", "ron", "scene", "prefab",
    "cmake", "json",
];
const SKIPPED_DIRS: [&str; 6] = ["target", "build", "bin", ".git", "node_modules", ".vs"];

impl CodeState {
    /// Перечитать список файлов в каталогах с кодом.
    pub fn scan(&mut self, dirs: &[PathBuf]) {
        fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    if depth < 4 && !SKIPPED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                        walk(root, &path, depth + 1, out);
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
            walk(dir, dir, 0, &mut files);
        }
        files.sort();
        self.files = files;
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        if let Some(index) = self.documents.iter().position(|d| d.path == path) {
            self.active = index;
            return Ok(());
        }
        // Формат файла (переводы строк, BOM, отступ) запоминается и сохраняется таким же.
        let (text, format) = crate::code_files::read(path)?;
        self.documents.push(Document::new(path, text, format));
        self.active = self.documents.len() - 1;
        Ok(())
    }

    pub fn close(&mut self, index: usize) {
        if index < self.documents.len() {
            self.documents.remove(index);
            self.active = self.active.min(self.documents.len().saturating_sub(1));
        }
    }

    /// Записать активный документ на диск. Возвращает путь сохранённого файла.
    pub fn save_active(&mut self) -> Result<Option<PathBuf>, String> {
        let Some(document) = self.documents.get_mut(self.active) else {
            return Ok(None);
        };
        std::fs::write(&document.path, &document.text)
            .map_err(|e| trf!("cannot save `{p0}`: {e}", p0 = document.path.display(), e = e))?;
        document.saved = document.text.clone();
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
        for document in self.documents.iter_mut().filter(|d| d.dirty()) {
            std::fs::write(&document.path, &document.text)
                .map_err(|e| trf!("cannot save `{p0}`: {e}", p0 = document.path.display(), e = e))?;
            document.saved = document.text.clone();
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
        if let Ok(mut status) = status.lock() {
            *status = BuildStatus::Running;
        }
        self.build.seen = BuildStatus::Running;
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut outcome = BuildStatus::Succeeded { seconds: 0.0 };
            for command in &commands {
                log::info!(target: "build", "{}: {} {}", command.label, command.program, command.args.join(" "));
                if !run_command(command) {
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
fn run_command(command: &BuildCommand) -> bool {
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

    fn forward(stream: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                log_build_line(&line);
            }
        })
    }
    let readers: Vec<_> = [
        child.stdout.take().map(forward),
        child.stderr.take().map(forward),
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

/// Подкрасить фон совпадений поиска в уже размеченном тексте.
fn mark_matches(job: &mut LayoutJob, matches: &[std::ops::Range<usize>], current: usize) {
    if matches.is_empty() {
        return;
    }
    let mut sections = Vec::with_capacity(job.sections.len() + matches.len() * 2);
    for section in job.sections.drain(..) {
        let mut cursor = section.byte_range.start;
        let end = section.byte_range.end;
        for (index, range) in matches.iter().enumerate() {
            if range.end <= cursor || range.start >= end {
                continue;
            }
            let start = range.start.max(cursor);
            let stop = range.end.min(end);
            if start > cursor {
                let mut plain = section.clone();
                plain.byte_range = cursor..start;
                sections.push(plain);
            }
            let mut marked = section.clone();
            marked.byte_range = start..stop;
            marked.format.background = if index == current {
                Color32::from_rgb(0x8a, 0x6a, 0x10)
            } else {
                Color32::from_rgb(0x4a, 0x41, 0x22)
            };
            sections.push(marked);
            cursor = stop;
        }
        if cursor < end {
            let mut rest = section;
            rest.byte_range = cursor..end;
            sections.push(rest);
        }
    }
    // Отступ первой секции относится к началу строки — у остальных его нет.
    for section in sections.iter_mut().skip(1) {
        section.leading_space = 0.0;
    }
    job.sections = sections;
}

fn char_index_of_line(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut seen = 1;
    for (index, ch) in text.chars().enumerate() {
        if ch == '\n' {
            seen += 1;
            if seen == line {
                return index + 1;
            }
        }
    }
    text.chars().count()
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
        .show_inside(ui, |ui| file_tree(editor, ui));

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
    for (index, document) in editor.code.documents.iter().enumerate() {
        let selected = index == editor.code.active;
        let (tab, closed) = document_tab(&mut tabs, index, document, selected);
        if tab.clicked() {
            activate = Some(index);
        }
        if closed || tab.middle_clicked() {
            close = Some(index);
        }
        tab.on_hover_text(editor_display(&editor.config.code_dirs, &document.path));
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
        editor.code.close(index);
    }

    // --- Строка состояния внизу ---
    let language = editor.code.documents.get(editor.code.active).map(|d| d.language);
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
                    if let Some(language) = language {
                        let (line, column) = editor.code.cursor;
                        for text in ["LF", "UTF-8", language.name(), &trf!("Ln {line}, Col {column}", line = line, column = column)] {
                            ui.label(egui::RichText::new(text).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
                            ui.add_space(theme::SP_S);
                        }
                    }
                });
            });
        });

    // Исходники на Rust новее программы — предложить Build с перезапуском.
    crate::rust_scripts::code_banner(editor, ui);

    let active = editor.code.active;
    let code_dirs = editor.config.code_dirs.clone();
    let CodeState {
        documents,
        find,
        goto,
        cache,
        cursor,
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
    let mut matches = if find.open { find_matches(&document.text, &find.query) } else { Vec::new() };
    if find.open {
        find_bar(ui, find, matches.len());
        if find.replace && replace_bar(ui, find, &mut document.text, &matches) {
            *cache = None;
            matches = find_matches(&document.text, &find.query);
        }
    }
    let mut goto_request = None;
    if find.goto_line.is_some() {
        goto_request = goto_bar(ui, find, document.text.lines().count().max(1));
    }
    if !matches.is_empty() {
        find.current = find.current.min(matches.len() - 1);
    }

    // --- Текст с номерами строк ---
    let font = theme::mono();
    let line_height = ui.fonts(|fonts| fonts.row_height(&font));
    let language = document.language;
    let lines = document.text.split('\n').count().max(1);
    let gutter_digits = lines.to_string().len().max(3);
    let jump_to_match = find.jump && !matches.is_empty();
    let current_match = find.current;
    let text_id = egui::Id::new(("code_text", active));
    // Переход к строке другого файла ждёт, пока тот файл покажут.
    let mut pending_goto = None;
    if goto.as_ref().is_some_and(|(path, _)| *path == document.path) {
        pending_goto = goto.take();
    }
    if let Some(line) = goto_request {
        pending_goto = Some((document.path.clone(), line));
    }
    if jump_to_match {
        let start = matches[current_match].start;
        let line = document.text[..start].matches('\n').count() + 1;
        pending_goto = Some((document.path.clone(), line));
    }

    egui::Frame::none().fill(theme::BG_INSET).show(ui, |ui| {
        egui::ScrollArea::both()
            .id_salt(("code_scroll", active))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // Колонка номеров строк.
                    let gutter_width = gutter_digits as f32 * ui.fonts(|f| f.glyph_width(&font, '0')) + 28.0;
                    let (gutter, _) = ui.allocate_exact_size(
                        vec2(gutter_width, lines as f32 * line_height + 8.0),
                        Sense::hover(),
                    );
                    ui.painter().rect_filled(gutter, 0.0, theme::BG_HEADER);
                    ui.add_space(theme::SP_6);

                    let query = if find.open { find.query.clone() } else { String::new() };
                    let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        std::hash::Hash::hash(text, &mut hasher);
                        std::hash::Hash::hash(&query, &mut hasher);
                        std::hash::Hash::hash(&current_match, &mut hasher);
                        std::hash::Hash::hash(&(language as u8), &mut hasher);
                        let key = std::hash::Hasher::finish(&hasher);
                        if cache.as_ref().is_none_or(|c| c.key != key) {
                            let mut job = highlight(text, language, font.clone());
                            mark_matches(&mut job, &find_matches(text, &query), current_match);
                            *cache = Some(HighlightCache { key, job });
                        }
                        let job = cache.as_ref().map(|c| c.job.clone()).unwrap_or_default();
                        ui.fonts(|fonts| fonts.layout_job(job))
                    };
                    let output = egui::TextEdit::multiline(&mut document.text)
                        .id(text_id)
                        .code_editor()
                        .frame(false)
                        .margin(egui::Margin::symmetric(0.0, 4.0))
                        .desired_width(f32::INFINITY)
                        .desired_rows(lines)
                        .lock_focus(true)
                        .layouter(&mut layouter)
                        .show(ui);

                    // Номера строк — по строкам уже разложенного текста: колонка не
                    // «уплывает» от текста, какой бы ни была высота строки шрифта.
                    let clip = ui.clip_rect();
                    let mut line = 1;
                    let mut line_start = true;
                    for row in &output.galley.rows {
                        let top = output.galley_pos.y + row.rect.top();
                        if line_start && top + row.rect.height() >= clip.top() && top <= clip.bottom() {
                            let current = line == cursor.0;
                            ui.painter().text(
                                pos2(gutter.right() - 14.0, top),
                                Align2::RIGHT_TOP,
                                line.to_string(),
                                font.clone(),
                                if current { theme::TEXT } else { theme::TEXT_DISABLED },
                            );
                        }
                        line_start = row.ends_with_newline;
                        if row.ends_with_newline {
                            line += 1;
                        }
                    }
                    if let Some(range) = output.cursor_range {
                        let rcursor = range.primary.rcursor;
                        *cursor = (rcursor.row + 1, rcursor.column + 1);
                    }
                    if let Some((_, line)) = pending_goto.take() {
                        let index = if jump_to_match {
                            document.text[..matches[current_match].start].chars().count()
                        } else {
                            char_index_of_line(&document.text, line)
                        };
                        let mut state = output.state.clone();
                        let ccursor = egui::text::CCursor::new(index);
                        let range = if jump_to_match {
                            let end = index + document.text[matches[current_match].clone()].chars().count();
                            egui::text::CCursorRange::two(ccursor, egui::text::CCursor::new(end))
                        } else {
                            egui::text::CCursorRange::one(ccursor)
                        };
                        state.cursor.set_char_range(Some(range));
                        state.store(ui.ctx(), text_id);
                        if !jump_to_match {
                            ui.ctx().memory_mut(|m| m.request_focus(text_id));
                        }
                        let target = Rect::from_min_size(
                            pos2(output.galley_pos.x, output.galley_pos.y + 4.0 + (line - 1) as f32 * line_height),
                            vec2(10.0, line_height),
                        );
                        ui.scroll_to_rect(target, Some(egui::Align::Center));
                        *cursor = (line, 1);
                    }
                });
            });
    });
    find.jump = false;
    let _ = code_dirs;
}

/// Вкладка открытого файла: иконка языка, имя, точка «не сохранено» или крестик.
fn document_tab(ui: &mut egui::Ui, index: usize, document: &Document, selected: bool) -> (egui::Response, bool) {
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
    if document.dirty() && !close.hovered() {
        ui.painter().circle_filled(close_rect.center(), 4.0, theme::TEXT);
    } else if close.hovered() || response.hovered() || selected {
        if close.hovered() {
            ui.painter().rect_filled(close_rect, theme::R_S, theme::BG_HOVER);
        }
        icons::paint(ui.painter(), close_rect.shrink(6.0), Icon::Close, theme::TEXT_MUTED);
    }
    let tip = if document.dirty() { tr_s("Close (unsaved changes are kept in memory until you close)") } else { tr_s("Close") };
    let closed = close.on_hover_text(tip).clicked();
    (response, closed)
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
fn replace_bar(ui: &mut egui::Ui, find: &mut Find, text: &mut String, matches: &[std::ops::Range<usize>]) -> bool {
    let mut changed = false;
    theme::toolbar_strip(ui, |ui| {
        crate::controls::text_field(ui, &mut find.replacement, tr_s("Replace with"), 300.0);
        let current = matches.get(find.current.min(matches.len().saturating_sub(1))).cloned();
        if ui.add_enabled_ui(current.is_some(), |ui| theme::button(ui, tr_s("Replace"))).inner.on_hover_text(tr_s("Replace the current match and go to the next one")).clicked() {
            if let Some(range) = current {
                text.replace_range(range, &find.replacement);
                find.jump = true;
                changed = true;
            }
        }
        if ui.add_enabled_ui(!matches.is_empty(), |ui| theme::button(ui, tr_s("Replace All"))).inner.clicked() {
            *text = replace_all(text, matches, &find.replacement);
            find.current = 0;
            changed = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Close, tr_s("Hide replace")).clicked() {
                find.replace = false;
            }
        });
    });
    changed
}

/// Заменить все совпадения (диапазоны не пересекаются и идут по порядку).
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
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let shift = ui.input(|i| i.modifiers.shift);
        let summary = match count {
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

/// Дерево файлов каталогов с кодом.
fn file_tree(editor: &mut Editor, ui: &mut egui::Ui) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::TAB_BAR), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, theme::BG_HEADER);
    ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
    ui.painter().text(
        pos2(bar.left() + theme::SP_L, bar.center().y),
        Align2::LEFT_CENTER,
        "FILES",
        theme::strong(theme::FONT_SECTION),
        theme::TEXT_MUTED,
    );
    if editor.code.files.is_empty() {
        ui.add_space(theme::SP_L);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(theme::SP_L);
            ui.label(
                egui::RichText::new(tr_s("No code folders. Start the editor with `EditorConfig::code_dir(\"src\")`."))
                    .color(theme::TEXT_MUTED),
            );
        });
        return;
    }
    let active = editor.code.documents.get(editor.code.active).map(|d| d.path.clone());
    let files = editor.code.files.clone();
    egui::ScrollArea::vertical()
        .id_salt("code_files_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(theme::SP_S);
            let mut shown_dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for (name, path) in &files {
                let parts: Vec<&str> = name.split('/').collect();
                // Каталоги на пути к файлу.
                let mut hidden = false;
                let mut dir = String::new();
                for (depth, part) in parts[..parts.len() - 1].iter().enumerate() {
                    if !dir.is_empty() {
                        dir.push('/');
                    }
                    dir.push_str(part);
                    if hidden {
                        break;
                    }
                    if shown_dirs.insert(dir.clone()) {
                        let open = !editor.code.collapsed.contains(&dir);
                        if tree_line(ui, &dir, part, depth, true, open, false) {
                            if open {
                                editor.code.collapsed.insert(dir.clone());
                            } else {
                                editor.code.collapsed.remove(&dir);
                            }
                        }
                    }
                    hidden = editor.code.collapsed.contains(&dir);
                }
                if hidden {
                    continue;
                }
                let file = parts[parts.len() - 1];
                let selected = active.as_deref() == Some(path.as_path());
                if tree_line(ui, name, file, parts.len() - 1, false, false, selected) {
                    editor.actions.push(Action::OpenCode(path.clone()));
                }
            }
        });
}

fn tree_line(ui: &mut egui::Ui, id: &str, label: &str, depth: usize, folder: bool, open: bool, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click());
    let _ = id;
    if selected {
        ui.painter().rect_filled(rect, 0.0, theme::selection_fill());
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme::hover_fill());
    }
    let left = rect.left() + theme::SP_M + depth as f32 * 14.0;
    if folder {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(left + 6.0, rect.center().y), vec2(9.0, 9.0)),
            if open { Icon::ChevronDown } else { Icon::ChevronRight },
            theme::TEXT_MUTED,
        );
    }
    let (icon, color) = if folder {
        (if open { Icon::FolderOpen } else { Icon::Folder }, theme::FOLDER)
    } else {
        let language = Language::of(Path::new(label));
        let color = match language {
            Language::Rust => Color32::from_rgb(0xc8, 0x8a, 0x64),
            Language::Cpp => Color32::from_rgb(0x7f, 0xa8, 0xd8),
            Language::Ron => theme::PREFAB,
            Language::Wgsl => Color32::from_rgb(0xa0, 0x8c, 0xcc),
            Language::Scenario => Color32::from_rgb(0x6c, 0xb0, 0xa8),
            Language::Plain => theme::TEXT_MUTED,
        };
        (if language == Language::Plain { Icon::Text } else { Icon::Script }, color)
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(left + 22.0, rect.center().y), vec2(14.0, 14.0)),
        icon,
        color,
    );
    ui.painter().with_clip_rect(rect).text(
        pos2(left + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::body(),
        if selected { theme::TEXT_STRONG } else { theme::TEXT },
    );
    response.clicked()
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
