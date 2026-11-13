//! Скрипты на Rust из редактора.
//!
//! Компоненты на Rust вкомпилированы в саму программу, загружать их на лету нечем
//! (и не нужно). Поэтому цикл такой же, как перезагрузка домена скриптов в Unity,
//! только перезагружается процесс: **New Rust Script → правка → Build → редактор
//! пересобирает игру, запускает новую версию себя и возвращается туда же** — сцена
//! (с несохранёнными правками), выделение, открытые файлы, камера вида Scene.
//!
//! Что здесь:
//! * имена, шаблон скрипта и маленький разборщик `component! { … struct Name … }`;
//! * `mod.rs` каталога скриптов — его пишет редактор: `mod` для каждого файла и
//!   `register(app)` со всеми компонентами;
//! * сборка `cargo build` с поиском собранного исполняемого файла, перенос
//!   работающего `.exe` в сторону на Windows (его нельзя перезаписать, но можно
//!   переименовать);
//! * файл сеанса, через который старый процесс передаёт состояние новому
//!   (`--editor-session <файл>`).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::vec2;
use engine_core::missing::MissingScript;
use engine_core::prelude::*;
use engine_core::serialize::{SceneData, SceneSnapshot};

use crate::code::{BuildCommand, BuildStatus};
use crate::icons::{self, Icon};
use crate::state::{id_at, path_of, Action, Editor, EditorConfig, ViewTab};
use crate::{theme, EditorError};

/// Аргумент командной строки, с которым перезапущенный редактор получает файл сеанса.
pub const SESSION_ARG: &str = "--editor-session";
/// То же через переменную окружения.
pub const SESSION_ENV: &str = "PISKA_EDITOR_SESSION";
/// Сколько ждать, пока новый процесс заберёт сеанс.
const HANDOVER_TIMEOUT: Duration = Duration::from_secs(120);

// ----------------------------------------------------------------------
// Настройка
// ----------------------------------------------------------------------

/// Как пересобрать программу, в которой работает редактор, и перезапустить её.
///
/// ```no_run
/// use engine_editor::{EditorConfig, Relaunch};
/// // `cargo build -p engine --example demo3d` в каталоге рабочего пространства:
/// let config = EditorConfig::default()
///     .rust_scripts("examples/demo3d/scripts")
///     .relaunch(Relaunch::example("engine", "demo3d").dir("."));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Relaunch {
    /// Команда сборки: обычно `cargo build -p … --bin|--example …`.
    pub build: BuildCommand,
    /// Имя программы, которую собирает команда (`demo3d`, `template-editor`): по
    /// нему в выводе cargo находится исполняемый файл.
    pub target: String,
    /// Каталог сборки cargo на случай, когда `CARGO_TARGET_DIR` не задан в окружении
    /// редактора (его запустили не из терминала). По умолчанию — каталог, в котором
    /// лежит сам исполняемый файл, если это каталог cargo.
    pub target_dir: Option<PathBuf>,
}

impl Relaunch {
    /// `cargo build <args>`; `target` — имя собираемой программы. Профиль — тот же,
    /// с которым собран редактор (`--release` добавляется сам).
    pub fn cargo<S: Into<String>>(target: impl Into<String>, args: impl IntoIterator<Item = S>) -> Self {
        let mut all: Vec<String> = vec!["build".to_string()];
        all.extend(args.into_iter().map(Into::into));
        if !cfg!(debug_assertions) {
            all.push("--release".to_string());
        }
        Self {
            build: BuildCommand::new("Rust: build", "cargo").args(all).restart_required(),
            target: target.into(),
            target_dir: None,
        }
    }

    /// Пример пакета: `cargo build -p <package> --example <example>`.
    pub fn example(package: &str, example: &str) -> Self {
        Self::cargo(example, ["-p", package, "--example", example])
    }

    /// Программа пакета: `cargo build -p <package> --bin <bin> [--features …]`.
    pub fn bin(package: &str, bin: &str, features: &[&str]) -> Self {
        let mut args = vec!["-p".to_string(), package.to_string(), "--bin".to_string(), bin.to_string()];
        if !features.is_empty() {
            args.push("--features".to_string());
            args.push(features.join(","));
        }
        Self::cargo(bin, args)
    }

    /// Каталог, в котором запускается cargo (рабочее пространство или крейт игры).
    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.build = self.build.dir(dir);
        self
    }

    /// Каталог сборки cargo, если `CARGO_TARGET_DIR` не задан.
    pub fn target_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.target_dir = Some(dir.into());
        self
    }
}

// ----------------------------------------------------------------------
// Имена и шаблон
// ----------------------------------------------------------------------

const RUST_KEYWORDS: [&str; 51] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv",
    "typeof", "unsized", "virtual", "yield", "try",
];

/// Имя типа на Rust в стиле `UpperCamelCase`: `EnemyPatrol`, `Player2D`.
pub(crate) fn validate_rust_name(name: &str) -> Result<(), String> {
    let explain = || {
        format!(
            "`{name}` cannot be a Rust script name: use a type name in UpperCamelCase such as \
             `EnemyPatrol` (Latin letters and digits, starting with a capital letter, no spaces or `_`)"
        )
    };
    let mut chars = name.chars();
    let starts_upper = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    if !starts_upper || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(explain());
    }
    let module = snake_case(name);
    if RUST_KEYWORDS.contains(&name) || RUST_KEYWORDS.contains(&module.as_str()) {
        return Err(format!(
            "`{name}` cannot be a Rust script name: its file would be `{module}.rs`, and `{module}` \
             is a Rust keyword. Pick another name, e.g. `{name}Script`"
        ));
    }
    Ok(())
}

/// `EnemyPatrol` → `enemy_patrol`, `HTTPServer` → `http_server`, `Player2D` → `player2_d`.
pub(crate) fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (index, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            let previous = chars[index - 1];
            let next_lower = chars.get(index + 1).is_some_and(|n| n.is_ascii_lowercase());
            if previous.is_ascii_lowercase() || previous.is_ascii_digit() || (previous.is_ascii_uppercase() && next_lower) {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Текст нового скрипта.
pub(crate) fn rust_template(name: &str) -> String {
    RUST_TEMPLATE.replace("{Name}", name)
}

const RUST_TEMPLATE: &str = r#"//! {Name} — скрипт на Rust. Он вешается на объект так же, как Light или MeshRenderer:
//! в инспекторе — Add Component → {Name}.
//!
//! Код вкомпилирован в программу: после правки нажмите Build (Ctrl+B) — редактор
//! пересоберёт игру и перезапустится с новым кодом, сцена и выделение останутся.

use engine::prelude::*;

component! {
    /// {Name}: опишите здесь, что делает скрипт, — это подсказка в инспекторе.
    pub struct {Name} {
        /// Скорость вращения, градусов в секунду. Поля из `component!` видны в
        /// инспекторе и сохраняются в сцену.
        pub speed: f32 = 90.0,
    }
}

impl Component for {Name} {
    /// Один раз, перед первым кадром объекта: найти другие объекты, прочитать настройки.
    fn on_start(&mut self, _ctx: &mut Ctx) {}

    /// Каждый кадр. Пример: вращать объект вокруг вертикальной оси.
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        ctx.transform().rotate_y(self.speed.to_radians() * dt);
    }

    // Другие хуки (переопределяйте только нужные): on_fixed_update (шаг физики),
    // on_late_update (камеры), on_message, on_collision_enter, on_trigger_enter,
    // on_destroy. Ввод, время, таймеры, твины, сцены, сохранения, случайность —
    // docs/api/gameplay.md.
}
"#;

// ----------------------------------------------------------------------
// Разбор `component! { … }`
// ----------------------------------------------------------------------

/// Компонент, объявленный в файле.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    pub name: String,
    /// Объявлен `pub` / `pub(crate)`: только такой виден из `mod.rs`.
    pub public: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    Punct(char),
}

/// Лексер ровно настолько, чтобы не путаться в комментариях, строках и символах.
fn tokenize(source: &str) -> Result<Vec<(Token, usize)>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let skip_string = |i: &mut usize, line: &mut usize, start_line: usize| -> Result<(), String> {
        // `*i` указывает на открывающую кавычку.
        *i += 1;
        while *i < chars.len() {
            match chars[*i] {
                '\\' => *i += 2,
                '"' => {
                    *i += 1;
                    return Ok(());
                }
                '\n' => {
                    *line += 1;
                    *i += 1;
                }
                _ => *i += 1,
            }
        }
        Err(format!("a string starting on line {start_line} is never closed"))
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            let start = line;
            let mut depth = 0;
            loop {
                if i >= chars.len() {
                    return Err(format!("a comment starting on line {start} is never closed"));
                }
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == '"' {
            let start_line = line;
            skip_string(&mut i, &mut line, start_line)?;
        } else if c == '\'' {
            // Символ `'x'` / `'\n'` или время жизни `'a`.
            if chars.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < chars.len() && chars[i] != '\'' && chars[i] != '\n' {
                    i += 1;
                }
                i += 1;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // Сырые строки `r"…"`, `r#"…"#`, `br"…"`.
            if (word == "r" || word == "br") && matches!(chars.get(i), Some('"') | Some('#')) {
                let mut hashes = 0;
                while chars.get(i + hashes) == Some(&'#') {
                    hashes += 1;
                }
                if chars.get(i + hashes) == Some(&'"') {
                    let start_line = line;
                    i += hashes + 1;
                    loop {
                        if i >= chars.len() {
                            return Err(format!("a raw string starting on line {start_line} is never closed"));
                        }
                        if chars[i] == '"' && (0..hashes).all(|k| chars.get(i + 1 + k) == Some(&'#')) {
                            i += 1 + hashes;
                            break;
                        }
                        if chars[i] == '\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                    continue;
                }
            }
            if word == "b" && chars.get(i) == Some(&'"') {
                let start_line = line;
                skip_string(&mut i, &mut line, start_line)?;
                continue;
            }
            tokens.push((Token::Ident(word), line));
        } else {
            tokens.push((Token::Punct(c), line));
            i += 1;
        }
    }
    Ok(tokens)
}

/// Компоненты, объявленные в исходнике через `component! { … }` (в одном файле их
/// может быть несколько). Ошибка — файл «сломан» (незакрытые скобки, строка,
/// комментарий): такой файл в `mod.rs` не попадает.
pub(crate) fn declared_components(source: &str) -> Result<Vec<Declared>, String> {
    let tokens = tokenize(source)?;
    // Скобки должны сходиться во всём файле.
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut matching: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, (token, line)) in tokens.iter().enumerate() {
        if let Token::Punct(c) = token {
            match c {
                '{' | '(' | '[' => stack.push((*c, index)),
                '}' | ')' | ']' => {
                    let open = match c {
                        '}' => '{',
                        ')' => '(',
                        _ => '[',
                    };
                    match stack.pop() {
                        Some((found, start)) if found == open => {
                            matching.insert(start, index);
                        }
                        Some((found, start)) => {
                            return Err(format!(
                                "line {line}: `{c}` closes `{found}` opened on line {}",
                                tokens[start].1
                            ))
                        }
                        None => return Err(format!("line {line}: `{c}` has no matching opening bracket")),
                    }
                }
                _ => {}
            }
        }
    }
    if let Some((c, index)) = stack.pop() {
        return Err(format!("line {}: `{c}` is never closed", tokens[index].1));
    }

    let mut found = Vec::new();
    let mut index = 0;
    while index + 2 < tokens.len() {
        let is_macro = tokens[index].0 == Token::Ident("component".to_string())
            && tokens[index + 1].0 == Token::Punct('!')
            && matches!(tokens[index + 2].0, Token::Punct('{') | Token::Punct('(') | Token::Punct('['));
        // Вызов по пути (`engine::prelude::component!`) тоже подходит.
        if !is_macro {
            index += 1;
            continue;
        }
        let open = index + 2;
        let close = matching[&open];
        let mut depth = 0;
        let mut k = open + 1;
        while k < close {
            match &tokens[k].0 {
                Token::Punct('{') | Token::Punct('(') | Token::Punct('[') => depth += 1,
                Token::Punct('}') | Token::Punct(')') | Token::Punct(']') => depth -= 1,
                Token::Ident(word) if word == "struct" && depth == 0 => {
                    if let Some((Token::Ident(name), _)) = tokens.get(k + 1) {
                        found.push(Declared {
                            name: name.clone(),
                            public: is_public(&tokens, k),
                        });
                    }
                }
                _ => {}
            }
            k += 1;
        }
        index = close + 1;
    }
    Ok(found)
}

/// Перед `struct` стоит `pub` или `pub(…)`.
fn is_public(tokens: &[(Token, usize)], struct_index: usize) -> bool {
    let Some(previous) = struct_index.checked_sub(1) else {
        return false;
    };
    match &tokens[previous].0 {
        Token::Ident(word) => word == "pub",
        Token::Punct(')') => {
            let mut depth = 0;
            let mut k = previous;
            loop {
                match tokens[k].0 {
                    Token::Punct(')') => depth += 1,
                    Token::Punct('(') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                if k == 0 {
                    return false;
                }
                k -= 1;
            }
            k > 0 && tokens[k - 1].0 == Token::Ident("pub".to_string())
        }
        _ => false,
    }
}

// ----------------------------------------------------------------------
// mod.rs
// ----------------------------------------------------------------------

/// Что нашлось в каталоге скриптов.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct ScriptFolder {
    /// Модуль (имя файла без `.rs`) и компоненты в нём.
    pub modules: Vec<(String, Vec<String>)>,
    /// Пропущенные файлы и компоненты — с объяснением.
    pub warnings: Vec<String>,
}

impl ScriptFolder {
    /// Все компоненты всех модулей.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.modules.iter().flat_map(|(_, names)| names.iter().map(String::as_str))
    }
}

fn is_module_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !RUST_KEYWORDS.contains(&name)
}

/// Прочитать каталог скриптов (только сам каталог, без подкаталогов).
pub(crate) fn scan_scripts(dir: &Path) -> std::io::Result<ScriptFolder> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "rs"))
            .collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err),
    };
    files.sort();
    let mut folder = ScriptFolder::default();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for path in files {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let module = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if module == "mod" {
            continue;
        }
        if !is_module_name(&module) {
            folder.warnings.push(format!(
                "`{file}` is skipped: a script file name must be a Rust module name in snake_case, \
                 such as `enemy_patrol.rs`"
            ));
            continue;
        }
        let source = std::fs::read_to_string(&path)?;
        let declared = match declared_components(&source) {
            Ok(declared) => declared,
            Err(reason) => {
                folder.warnings.push(format!(
                    "`{file}` is skipped until it parses ({reason}); fix it and build again"
                ));
                continue;
            }
        };
        let mut names = Vec::new();
        for component in declared {
            if !component.public {
                folder.warnings.push(format!(
                    "`{file}`: component `{}` is not registered because it is private; write \
                     `pub struct {}` inside `component!`",
                    component.name, component.name
                ));
            } else if let Some(other) = seen.get(&component.name) {
                folder.warnings.push(format!(
                    "`{file}`: component `{}` is already declared in `{other}`; component type names \
                     must be unique, so only that one is registered",
                    component.name
                ));
            } else {
                seen.insert(component.name.clone(), file.clone());
                names.push(component.name);
            }
        }
        folder.modules.push((module, names));
    }
    Ok(folder)
}

/// Текст `mod.rs` для каталога скриптов.
pub(crate) fn mod_rs_text(folder: &ScriptFolder) -> String {
    let mut text = String::from(
        "//! Скрипты на Rust этой игры: каталог, в котором их создаёт редактор (New Rust Script).\n\
         //!\n\
         //! ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР — не правьте его руками: он перезаписывается, когда в\n\
         //! каталоге создают, переименовывают или удаляют скрипт, и перед каждой сборкой из\n\
         //! редактора. В нём `mod` для каждого файла каталога и `register`, который\n\
         //! регистрирует все компоненты, объявленные в них через `component!`.\n\
         //!\n\
         //! Игра вызывает один раз: `scripts::register(&mut app);`\n\n",
    );
    for (module, _) in &folder.modules {
        text.push_str(&format!("pub mod {module};\n"));
    }
    if !folder.modules.is_empty() {
        text.push('\n');
    }
    text.push_str("/// Зарегистрировать компоненты всех скриптов каталога.\n");
    text.push_str("pub fn register(app: &mut engine::prelude::App) {\n");
    let mut any = false;
    for (module, names) in &folder.modules {
        for name in names {
            text.push_str(&format!("    app.register::<{module}::{name}>();\n"));
            any = true;
        }
    }
    if !any {
        text.push_str("    let _ = app;\n");
    }
    text.push_str("}\n");
    text
}

/// Переписать `mod.rs` каталога (только если текст изменился) и сообщить о
/// пропущенных файлах в консоль.
pub(crate) fn regenerate_mod(dir: &Path) -> Result<ScriptFolder, EditorError> {
    let io = |source: std::io::Error| {
        EditorError::Other(format!("cannot update `{}`: {source}", dir.join("mod.rs").display()))
    };
    let folder = scan_scripts(dir).map_err(io)?;
    for warning in &folder.warnings {
        log::warn!("{warning}");
    }
    let text = mod_rs_text(&folder);
    let path = dir.join("mod.rs");
    let current = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    if current != text {
        std::fs::create_dir_all(dir).map_err(io)?;
        std::fs::write(&path, text).map_err(io)?;
    }
    Ok(folder)
}

// ----------------------------------------------------------------------
// Файл сеанса
// ----------------------------------------------------------------------

/// Состояние редактора, которое переживает перезапуск.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Session {
    pub scene_path: Option<String>,
    pub dirty: bool,
    pub scene: Option<SceneData>,
    pub selection: Option<Vec<usize>>,
    pub view_tab: Option<String>,
    pub scene_3d: bool,
    /// Орбитальная камера: цель x, y, z, рысканье, наклон, расстояние.
    pub orbit: [f32; 6],
    /// Камера 2D: центр x, y, половина высоты.
    pub camera_2d: [f32; 3],
    pub documents: Vec<PathBuf>,
    pub active_document: usize,
    pub cursor_line: usize,
    /// Скрипты, ждущие сборки: тип и путь до объекта.
    pub pending: Vec<(String, Vec<usize>)>,
    /// Компоненты, зарегистрированные в старой программе.
    pub registered: Vec<String>,
    pub script_rust: bool,
    /// Когда началась сборка и когда старый процесс запустил новый (секунды Unix).
    pub build_started: f64,
    pub relaunched: f64,
}

const SESSION_HEADER: &str = "piska-editor-session 1";
const SCENE_MARKER: &str = "--- scene ---";

fn path_text(path: &[usize]) -> String {
    path.iter().map(usize::to_string).collect::<Vec<_>>().join("/")
}

fn parse_path(text: &str) -> Option<Vec<usize>> {
    text.split('/').map(|part| part.trim().parse().ok()).collect()
}

fn floats<const N: usize>(text: &str) -> [f32; N] {
    let mut out = [0.0; N];
    for (slot, part) in out.iter_mut().zip(text.split_whitespace()) {
        *slot = part.parse().unwrap_or(0.0);
    }
    out
}

pub(crate) fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Session {
    pub fn to_text(&self) -> String {
        let mut lines = vec![SESSION_HEADER.to_string()];
        let mut put = |key: &str, value: String| lines.push(format!("{key}={value}"));
        if let Some(path) = &self.scene_path {
            put("scene_path", path.clone());
        }
        put("dirty", u8::from(self.dirty).to_string());
        if let Some(selection) = &self.selection {
            put("selection", path_text(selection));
        }
        if let Some(tab) = &self.view_tab {
            put("view_tab", tab.clone());
        }
        put("scene_3d", u8::from(self.scene_3d).to_string());
        put("orbit", self.orbit.map(|v| v.to_string()).join(" "));
        put("camera_2d", self.camera_2d.map(|v| v.to_string()).join(" "));
        for document in &self.documents {
            put("document", document.display().to_string());
        }
        put("active_document", self.active_document.to_string());
        put("cursor_line", self.cursor_line.to_string());
        for (kind, path) in &self.pending {
            put("pending", format!("{kind} {}", path_text(path)));
        }
        put("registered", self.registered.join(","));
        put("script_language", if self.script_rust { "rust" } else { "cpp" }.to_string());
        put("build_started", self.build_started.to_string());
        put("relaunched", self.relaunched.to_string());
        let mut text = lines.join("\n");
        text.push('\n');
        if let Some(scene) = &self.scene {
            text.push_str(SCENE_MARKER);
            text.push('\n');
            text.push_str(&scene.to_ron());
            text.push('\n');
        }
        text
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let text = text.replace("\r\n", "\n");
        let (head, scene) = match text.split_once(&format!("\n{SCENE_MARKER}\n")) {
            Some((head, scene)) => (head.to_string(), Some(scene.to_string())),
            None => (text.clone(), None),
        };
        let mut lines = head.lines();
        if lines.next().map(str::trim) != Some(SESSION_HEADER) {
            return Err("this is not an editor session file".to_string());
        }
        let mut session = Session::default();
        for line in lines {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "scene_path" => session.scene_path = Some(value.to_string()),
                "dirty" => session.dirty = value == "1",
                "selection" => session.selection = parse_path(value),
                "view_tab" => session.view_tab = Some(value.to_string()),
                "scene_3d" => session.scene_3d = value == "1",
                "orbit" => session.orbit = floats(value),
                "camera_2d" => session.camera_2d = floats(value),
                "document" => session.documents.push(PathBuf::from(value)),
                "active_document" => session.active_document = value.parse().unwrap_or(0),
                "cursor_line" => session.cursor_line = value.parse().unwrap_or(1),
                "pending" => {
                    if let Some((kind, path)) = value.split_once(' ') {
                        if let Some(path) = parse_path(path) {
                            session.pending.push((kind.to_string(), path));
                        }
                    }
                }
                "registered" => {
                    session.registered = value.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()
                }
                "script_language" => session.script_rust = value == "rust",
                "build_started" => session.build_started = value.parse().unwrap_or(0.0),
                "relaunched" => session.relaunched = value.parse().unwrap_or(0.0),
                _ => {}
            }
        }
        if let Some(scene) = scene {
            session.scene = Some(SceneData::from_ron(&scene, "editor session").map_err(|e| e.to_string())?);
        }
        Ok(session)
    }
}

/// Файл сеанса из командной строки (`--editor-session <файл>`) или окружения
/// (`PISKA_EDITOR_SESSION`) — так перезапущенный редактор узнаёт, что восстанавливать.
pub fn session_from_args() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == SESSION_ARG)
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(SESSION_ENV).map(PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty())
}

/// Аргументы для нового процесса: те же, без прежнего `--editor-session`.
fn relaunch_args(session: &Path) -> Vec<String> {
    let mut args = Vec::new();
    let mut skip = false;
    for arg in std::env::args().skip(1) {
        if std::mem::take(&mut skip) {
            continue;
        }
        if arg == SESSION_ARG {
            skip = true;
            continue;
        }
        args.push(arg);
    }
    args.push(SESSION_ARG.to_string());
    args.push(session.display().to_string());
    args
}

// ----------------------------------------------------------------------
// Работающий .exe на Windows
// ----------------------------------------------------------------------

/// Файлы, отодвинутые в сторону перед сборкой: (исходное имя, временное имя).
#[derive(Debug, Default)]
pub(crate) struct Aside {
    pub moved: Vec<(PathBuf, PathBuf)>,
}

impl Aside {
    /// Вернуть имена (сборка не удалась), если их не занял новый файл.
    pub fn restore(self) {
        for (original, aside) in self.moved.into_iter().rev() {
            if !original.exists() {
                if let Err(err) = std::fs::rename(&aside, &original) {
                    log::warn!(target: "build", "cannot rename `{}` back: {err}", aside.display());
                }
            }
        }
    }
}

/// Свободное имя `<stem>.old-<pid>[-n].<ext>` рядом с файлом.
fn aside_name(path: &Path, pid: u32) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    (0..)
        .map(|n| {
            let suffix = if n == 0 { String::new() } else { format!("-{n}") };
            dir.join(format!("{stem}.old-{pid}{suffix}{extension}"))
        })
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| dir.join(format!("{stem}.old-{pid}{extension}")))
}

/// Каталог `deps` cargo для исполняемого файла: рядом (`debug/x.exe`) или уровнем
/// выше (`debug/examples/x.exe`).
fn deps_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    [dir.join("deps"), dir.parent()?.join("deps")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

/// Тот же файл (жёсткая ссылка cargo) — совпадают размер и время изменения.
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.len() == b.len() && a.modified().ok().is_some() && a.modified().ok() == b.modified().ok()
}

/// Отодвинуть работающую программу, чтобы компоновщик смог записать новую.
///
/// Windows не даёт перезаписать запущенный `.exe`, но даёт его переименовать.
/// Переносятся: сам файл, если это вывод cargo для `target` (`target.exe` в каталоге
/// с `deps/`), его копия-ссылка `deps/<crate>-<hash>.exe` и отладочные `.pdb`.
/// Копию программы вне каталога cargo трогать незачем — cargo пишет не в неё.
pub(crate) fn set_aside(exe: &Path, target: &str, pid: u32) -> Aside {
    let mut aside = Aside::default();
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(deps) = deps_dir(exe) else {
        return aside;
    };
    if stem != target {
        return aside;
    }
    let crate_name = target.replace('-', "_");
    let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut candidates = vec![exe.to_path_buf()];
    for pdb in [dir.join(format!("{stem}.pdb")), dir.join(format!("{crate_name}.pdb"))] {
        if pdb.is_file() && !candidates.contains(&pdb) {
            candidates.push(pdb);
        }
    }
    if let (Ok(running), Ok(entries)) = (std::fs::metadata(exe), std::fs::read_dir(&deps)) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Программа пакета — `deps/<crate>.exe`, у примеров и тестов бывает хэш: `<crate>-<hash>.exe`.
            let file_stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let is_build = (file_stem == crate_name || name.starts_with(&format!("{crate_name}-")))
                && !name.contains(".old-")
                && path
                    .extension()
                    .map_or(std::env::consts::EXE_EXTENSION.is_empty(), |e| e == std::env::consts::EXE_EXTENSION);
            if is_build && entry.metadata().is_ok_and(|m| m.is_file() && same_file(&m, &running)) {
                let pdb = path.with_extension("pdb");
                candidates.push(path);
                if pdb.is_file() {
                    candidates.push(pdb);
                }
            }
        }
    }
    for original in candidates {
        let target = aside_name(&original, pid);
        match std::fs::rename(&original, &target) {
            Ok(()) => aside.moved.push((original, target)),
            Err(err) => log::warn!(
                target: "build",
                "cannot move `{}` aside ({err}); if the build fails to write it, close other copies of the program",
                original.display()
            ),
        }
    }
    aside
}

/// Удалить отодвинутые прежние версии программы (`*.old-*`) рядом с ней и в `deps/`.
/// Файл, который ещё работает, не удаляется — его уберёт следующий запуск.
pub fn remove_stale_executables() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let crate_name = stem.replace('-', "_");
    let dirs = [exe.parent().map(Path::to_path_buf), deps_dir(&exe)];
    for dir in dirs.into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let ours = name.starts_with(&format!("{stem}.old-"))
                || name.starts_with(&format!("{crate_name}.old-"))
                || name.starts_with(&format!("{crate_name}-"));
            if ours && name.contains(".old-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

// ----------------------------------------------------------------------
// Сборка
// ----------------------------------------------------------------------

/// Путь из JSON-строки cargo: значение ключа `"executable"`.
pub(crate) fn json_executable(line: &str) -> Option<PathBuf> {
    let start = line.find("\"executable\":\"")? + "\"executable\":\"".len();
    let mut out = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(PathBuf::from(out)),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let code: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Каталог сборки cargo, в котором лежит программа: `<target>/debug/x.exe` или
/// `<target>/debug/examples/x.exe` (рядом есть `deps/`).
fn target_dir_of(exe: &Path) -> Option<PathBuf> {
    let deps = deps_dir(exe)?;
    let profile = deps.parent()?;
    profile.parent().map(Path::to_path_buf)
}

/// Собрать программу cargo и вернуть путь к собранному исполняемому файлу.
fn run_cargo(relaunch: &Relaunch) -> Result<PathBuf, String> {
    let build = &relaunch.build;
    let mut process = Command::new(&build.program);
    process.args(&build.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let is_cargo = Path::new(&build.program).file_stem().is_some_and(|s| s == "cargo");
    if is_cargo {
        // Сообщения компилятора — текстом в stderr, а пути к собранному — JSON в stdout.
        process.arg("--message-format=json-render-diagnostics");
    }
    if let Some(dir) = &build.dir {
        process.current_dir(dir);
    }
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        let derived = std::env::current_exe().ok().and_then(|exe| target_dir_of(&exe));
        if let Some(dir) = derived.or_else(|| relaunch.target_dir.clone()) {
            process.env("CARGO_TARGET_DIR", dir);
        }
    }
    log::info!(target: "build", "{}: {} {}", build.label, build.program, build.args.join(" "));
    let mut child = process
        .spawn()
        .map_err(|err| format!("{}: cannot start `{}`: {err}. Is it installed and on PATH?", build.label, build.program))?;
    let stderr = child.stderr.take().map(|stream| {
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                crate::code::log_build_line(&line);
            }
        })
    });
    let mut executable = None;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.starts_with('{') {
                if let Some(path) = json_executable(&line) {
                    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if stem == relaunch.target {
                        executable = Some(path);
                    }
                }
            } else {
                crate::code::log_build_line(&line);
            }
        }
    }
    let status = child.wait().map_err(|err| format!("{}: {err}", build.label))?;
    if let Some(reader) = stderr {
        let _ = reader.join();
    }
    if !status.success() {
        return Err(format!("{}: failed ({status}); the errors are above", build.label));
    }
    executable.ok_or_else(|| {
        format!(
            "{}: cargo did not report the program `{}`; check `EditorConfig::relaunch` — the target \
             name must be the name of the binary or example it builds",
            build.label, relaunch.target
        )
    })
}

// ----------------------------------------------------------------------
// Состояние редактора
// ----------------------------------------------------------------------

/// Новый процесс запущен и ждёт, пока заберёт сеанс.
struct Handover {
    child: Child,
    session: PathBuf,
    started: Instant,
}

/// Новый файл, переименование или удаление скрипта — окно подтверждения.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FileDialog {
    Rename { path: PathBuf, name: String },
    Delete { path: PathBuf },
}

/// Итог шага сборки из её потока: путь к новой программе или ошибка.
type BuildResult = Arc<Mutex<Option<Result<PathBuf, String>>>>;

/// Всё про сборку на Rust и перезапуск, что живёт в [`Editor`].
#[derive(Default)]
pub(crate) struct RustState {
    /// Результат последнего шага сборки: путь к новой программе или ошибка.
    result: Option<BuildResult>,
    build_started: f64,
    handover: Option<Handover>,
    /// Новый процесс забрал сеанс: старый ждёт его и выходит с его кодом.
    successor: Option<Child>,
    /// Скрипт создан, но программа ещё не пересобрана.
    pub build_needed: bool,
    hint_shown: bool,
    /// Кэш «исходники новее программы»: когда проверяли и что вышло.
    stale_checked: Option<(Instant, bool)>,
    pub file_dialog: Option<FileDialog>,
}

impl Editor {
    /// Создан или переименован скрипт на Rust, а программа ещё не пересобрана с ним.
    pub fn is_rust_build_pending(&self) -> bool {
        self.rust.build_needed
    }

    /// Можно ли что-то собрать: есть команды сборки или пересборка на Rust.
    pub(crate) fn can_build(&self) -> bool {
        !self.config.build_commands.is_empty() || self.config.relaunch.is_some()
    }

    /// Открыть окно «New Script» на нужном языке.
    pub(crate) fn open_script_dialog(&mut self, rust: bool, name: String, object: Option<GameObjectId>) {
        self.script_rust = rust;
        self.dialog = Some(crate::state::Dialog::NewScript { name, object });
    }

    /// Создать скрипт на Rust: файл из шаблона, `mod.rs`, вкладка Code, сборка.
    pub(crate) fn new_rust_script(&mut self, name: &str, attach_to: Option<GameObjectId>) -> Result<(), EditorError> {
        let dir = self.config.rust_script_dir.clone().ok_or_else(|| {
            EditorError::Other(
                "cannot create a Rust script: the editor does not know where Rust scripts live. Start it \
                 with `EditorConfig::default().rust_scripts(\"src/scripts\")` (and `.relaunch(…)` so Build \
                 reloads the editor), or open the game with `engine::editor_main!`"
                    .to_string(),
            )
        })?;
        let name = name.trim();
        validate_rust_name(name).map_err(EditorError::Other)?;
        let path = dir.join(format!("{}.rs", snake_case(name)));
        let taken_in_folder = scan_scripts(&dir)
            .map(|folder| folder.components().any(|c| c == name))
            .unwrap_or(false);
        if self.app.world.registry.contains(name) || path.exists() || taken_in_folder {
            return Err(EditorError::Other(format!(
                "a component named `{name}` (or the file `{}`) already exists; pick another name",
                path.display()
            )));
        }
        let io = |source: std::io::Error| SceneError::Io {
            path: path.display().to_string(),
            source,
        };
        std::fs::create_dir_all(&dir).map_err(io)?;
        std::fs::write(&path, rust_template(name)).map_err(io)?;
        log::info!("created `{}`", path.display());
        regenerate_mod(&dir)?;

        self.code.scan(&self.config.code_dirs);
        self.code.open(&path).map_err(EditorError::Other)?;
        self.code.goto_line(&path, 22);
        self.view_tab = ViewTab::Code;
        if let Some(object) = attach_to {
            self.pending_scripts.push((name.to_string(), object));
        }
        self.rust.build_needed = true;
        if self.config.relaunch.is_some() {
            log::info!(
                "building the game with `{name}`; the editor reloads itself when the build succeeds \
                 (the scene, selection and open files are kept)"
            );
            self.start_rust_build()?;
        } else {
            log::warn!(
                "no relaunch command is configured (`EditorConfig::relaunch`): build the game and restart \
                 the editor yourself to start using `{name}`"
            );
        }
        Ok(())
    }

    /// Сборка с перезапуском: команды сборки (C++ и прочее), затем `cargo build`
    /// программы редактора; при успехе — перезапуск.
    pub(crate) fn start_rust_build(&mut self) -> Result<(), EditorError> {
        let Some(relaunch) = self.config.relaunch.clone() else {
            return Ok(());
        };
        if self.code.build_status() == BuildStatus::Running {
            log::warn!(target: "build", "a build is already running");
            return Ok(());
        }
        if self.rust.handover.is_some() {
            return Err(EditorError::Other("the rebuilt editor is starting; wait a moment".to_string()));
        }
        if let Some(dir) = self.config.rust_script_dir.clone() {
            regenerate_mod(&dir)?;
        }
        let result = Arc::new(Mutex::new(None));
        self.rust.result = Some(result.clone());
        self.rust.build_started = unix_now();
        let step: crate::code::FinalStep = Box::new(move || {
            // Работающую программу — в сторону, иначе компоновщик не сможет её заменить.
            let aside = match std::env::current_exe() {
                Ok(exe) if cfg!(windows) => Some(set_aside(&exe, &relaunch.target, std::process::id())),
                _ => None,
            };
            let outcome = run_cargo(&relaunch);
            let ok = outcome.is_ok();
            if let Err(err) = &outcome {
                log::error!(target: "build", "{err}");
                if let Some(aside) = aside {
                    aside.restore();
                }
            }
            if let Ok(mut slot) = result.lock() {
                *slot = Some(outcome);
            }
            ok
        });
        self.code.start_build_with(self.config.build_commands.clone(), Some(step));
        Ok(())
    }

    /// Заняты сборкой с перезапуском или передачей сеанса.
    pub(crate) fn rust_busy(&self) -> bool {
        self.rust.handover.is_some() || self.rust.result.is_some()
    }

    /// Раз в кадр: сборка закончилась — перезапуститься; новый процесс забрал
    /// сеанс — выйти.
    pub(crate) fn poll_rust(&mut self) {
        if let Some(result) = self.rust.result.clone() {
            let finished = result.lock().ok().and_then(|mut slot| slot.take());
            if let Some(outcome) = finished {
                // Ждём, пока поток сборки отметит конец (статус обновляется после шага).
                self.rust.result = None;
                match outcome {
                    Ok(exe) => {
                        self.rust.build_needed = false;
                        if let Err(err) = self.relaunch_into(&exe) {
                            log::error!("{err}");
                        }
                    }
                    Err(_) => log::warn!(
                        target: "build",
                        "the build failed: this editor keeps running the previous code. Fix the errors \
                         (click a `file:line` in the Console) and press Build again"
                    ),
                }
            }
        }
        let Some(handover) = self.rust.handover.as_mut() else {
            return;
        };
        match handover.child.try_wait() {
            Ok(Some(status)) => {
                let session = handover.session.clone();
                self.rust.handover = None;
                let _ = std::fs::remove_file(&session);
                log::error!(
                    "the rebuilt editor exited while starting ({status}); this editor keeps running the \
                     previous code. Its output is in the terminal it was started from"
                );
            }
            Ok(None) if !handover.session.exists() => {
                // Новый процесс восстановил сеанс: этот закрывается.
                let handover = self.rust.handover.take().expect("checked above");
                log::info!("the rebuilt editor took over; closing this one");
                self.rust.successor = Some(handover.child);
                self.quit = true;
            }
            Ok(None) if handover.started.elapsed() > HANDOVER_TIMEOUT => {
                let session = handover.session.clone();
                self.rust.handover = None;
                log::error!(
                    "the rebuilt editor did not pick up the session within {} s; this editor keeps \
                     running. The session file stays at `{}`",
                    HANDOVER_TIMEOUT.as_secs(),
                    session.display()
                );
            }
            Ok(None) => {}
            Err(err) => {
                self.rust.handover = None;
                log::error!("cannot watch the rebuilt editor: {err}");
            }
        }
    }

    /// Процесс, которому передан сеанс (см. [`crate::run`]).
    pub(crate) fn take_successor(&mut self) -> Option<Child> {
        self.rust.successor.take()
    }

    /// Процесс, которого этот дождётся после закрытия окна (запущенный Piska Hub).
    pub(crate) fn set_successor(&mut self, child: Child) {
        self.rust.successor = Some(child);
    }

    /// Состояние для нового процесса.
    pub(crate) fn capture_session(&self) -> Session {
        let scene = &self.app.world.scene;
        let snapshot = self.app.world.snapshot();
        let (documents, active_document, cursor_line) = self.code.session_documents();
        Session {
            scene_path: self.scene_path.clone(),
            dirty: self.dirty,
            scene: Some(SceneData {
                name: snapshot.name,
                objects: snapshot.objects,
            }),
            selection: self.selection.and_then(|id| path_of(scene, id)),
            view_tab: Some(format!("{:?}", self.view_tab)),
            scene_3d: self.scene_3d,
            orbit: [
                self.orbit.target.x,
                self.orbit.target.y,
                self.orbit.target.z,
                self.orbit.yaw,
                self.orbit.pitch,
                self.orbit.distance,
            ],
            camera_2d: [self.cam_center.x, self.cam_center.y, self.cam_half_height],
            documents,
            active_document,
            cursor_line,
            pending: self
                .pending_scripts
                .iter()
                .filter_map(|(kind, id)| Some((kind.clone(), path_of(scene, *id)?)))
                .collect(),
            registered: self.app.world.registry.names().map(str::to_string).collect(),
            script_rust: self.script_rust,
            build_started: self.rust.build_started,
            relaunched: unix_now(),
        }
    }

    /// Записать сеанс в файл — то же, что Build передаёт перезапущенному редактору
    /// (`--editor-session <файл>`, [`Editor::resume_session`]).
    pub fn save_session(&self, path: &Path) -> Result<(), EditorError> {
        std::fs::write(path, self.capture_session().to_text()).map_err(|source| {
            EditorError::from(SceneError::Io {
                path: path.display().to_string(),
                source,
            })
        })
    }

    /// Записать сеанс и запустить новую программу. Старый редактор работает, пока
    /// новый не заберёт сеанс (см. [`Editor::poll_rust`]).
    fn relaunch_into(&mut self, exe: &Path) -> Result<(), EditorError> {
        if self.mode != crate::Mode::Edit {
            self.apply(Action::Stop); // в новую программу — только в режиме Edit
        }
        self.settle();
        // Правки, сделанные во время сборки, тоже переезжают.
        self.code.save_all().map_err(EditorError::Other)?;
        let session = std::env::temp_dir().join(format!("piska-editor-session-{}.txt", std::process::id()));
        std::fs::write(&session, self.capture_session().to_text()).map_err(|source| SceneError::Io {
            path: session.display().to_string(),
            source,
        })?;
        let mut command = Command::new(exe);
        command.args(relaunch_args(&session)).env_remove("ENGINE_PLAY").env_remove(SESSION_ENV);
        match command.spawn() {
            Ok(child) => {
                log::info!("starting the rebuilt editor `{}`…", exe.display());
                self.rust.handover = Some(Handover {
                    child,
                    session,
                    started: Instant::now(),
                });
                Ok(())
            }
            Err(err) => {
                let _ = std::fs::remove_file(&session);
                Err(EditorError::Other(format!(
                    "cannot start the rebuilt editor `{}`: {err}; this editor keeps running the previous code",
                    exe.display()
                )))
            }
        }
    }

    /// Восстановить состояние из файла сеанса, который оставил прежний процесс
    /// (`--editor-session <файл>`), и удалить файл — это знак старому процессу, что
    /// можно закрываться. Если восстановить не удалось, файл переименовывается в
    /// `*.failed` (в нём несохранённая сцена) и всё равно освобождает старый процесс.
    pub fn resume_session(&mut self, path: &Path) -> Result<(), EditorError> {
        let outcome = std::fs::read_to_string(path)
            .map_err(|err| EditorError::Other(format!("cannot read the editor session `{}`: {err}", path.display())))
            .and_then(|text| Session::from_text(&text).map_err(EditorError::Other))
            .and_then(|session| self.restore_session(&session));
        match &outcome {
            Ok(()) => {
                let _ = std::fs::remove_file(path);
            }
            Err(err) => {
                let failed = path.with_extension("failed");
                let _ = std::fs::rename(path, &failed);
                log::error!(
                    "the editor was rebuilt but its previous state could not be restored ({err}); the \
                     unsaved scene is in `{}`",
                    failed.display()
                );
            }
        }
        outcome
    }

    /// Применить сеанс к этому редактору.
    pub(crate) fn restore_session(&mut self, session: &Session) -> Result<(), EditorError> {
        engine_core::missing::keep_missing_scripts(true);
        if let Some(scene) = &session.scene {
            self.app.world.restore(&SceneSnapshot {
                name: scene.name.clone(),
                objects: scene.objects.clone(),
            })?;
        }
        self.scene_path = session.scene_path.clone();
        self.reset_history();
        self.dirty = session.dirty;
        let scene = &self.app.world.scene;
        self.selection = session.selection.as_ref().and_then(|path| id_at(scene, path));
        self.scene_3d = session.scene_3d;
        let [x, y, z, yaw, pitch, distance] = session.orbit;
        if distance > 0.0 {
            self.orbit.target = Vec3::new(x, y, z);
            self.orbit.yaw = yaw;
            self.orbit.pitch = pitch;
            self.orbit.distance = distance;
        }
        let [cx, cy, half] = session.camera_2d;
        if half > 0.0 {
            self.cam_center = Vec2::new(cx, cy);
            self.cam_half_height = half;
        }
        self.script_rust = session.script_rust;
        for document in &session.documents {
            if let Err(err) = self.code.open(document) {
                log::warn!("{err}");
            }
        }
        if let Some(active) = session.documents.get(session.active_document) {
            let _ = self.code.open(active);
            self.code.goto_line(active, session.cursor_line.max(1));
        }
        self.view_tab = match session.view_tab.as_deref() {
            Some("Code") => ViewTab::Code,
            _ => ViewTab::Scene, // Play не переживает перезапуск: Game → Scene
        };

        // Что изменилось в типах компонентов.
        let before: BTreeSet<&str> = session.registered.iter().map(String::as_str).collect();
        let now: BTreeSet<&str> = self.app.world.registry.names().collect();
        let mut changes: Vec<String> = now.difference(&before).map(|name| format!("+{name}")).collect();
        changes.extend(before.difference(&now).map(|name| format!("-{name}")));
        if session.registered.is_empty() || changes.is_empty() {
            log::info!("reloaded scripts: no component types added or removed");
        } else {
            log::info!("reloaded scripts: {}", changes.join(", "));
        }
        if session.build_started > 0.0 && session.relaunched > 0.0 {
            let now = unix_now();
            log::info!(
                "editor reloaded with the new code in {:.1} s (build {:.1} s, restart {:.1} s)",
                now - session.build_started,
                session.relaunched - session.build_started,
                now - session.relaunched
            );
        }

        // Скрипты, созданные до перезапуска, — на свои объекты.
        for (kind, path) in &session.pending {
            match id_at(&self.app.world.scene, path) {
                Some(object) if self.app.world.registry.contains(kind) => {
                    log::info!("script `{kind}` is built and attached");
                    self.apply(Action::AddComponent { object, kind: kind.clone() });
                }
                Some(object) => {
                    log::warn!(
                        "script `{kind}` is still not registered after the rebuild: check that it is declared \
                         with `component!` as `pub struct {kind}` and that the game calls `scripts::register(&mut app)`"
                    );
                    self.pending_scripts.push((kind.clone(), object));
                }
                None => log::warn!("the object script `{kind}` was meant for is gone"),
            }
        }
        self.report_missing_scripts();
        Ok(())
    }

    /// Сообщить в консоль о компонентах сцены, типов которых нет в этой программе.
    pub(crate) fn report_missing_scripts(&self) {
        let scene = &self.app.world.scene;
        let mut missing: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for id in scene.ids() {
            if let Some(object) = scene.get(id) {
                for component in object.components() {
                    if component.as_any().is::<MissingScript>() {
                        missing.entry(component.type_name()).or_default().push(object.name.clone());
                    }
                }
            }
        }
        for (kind, objects) in missing {
            log::warn!(
                "script `{kind}` is missing (not in this build) on {}: its data is kept and saved unchanged. \
                 Restore the type or remove the component",
                objects.join(", ")
            );
        }
    }

    /// Исходники на Rust изменились после сборки этой программы (или скрипт создан,
    /// но не собран). Проверка файлов — не чаще раза в 2 секунды.
    pub(crate) fn rust_sources_changed(&mut self) -> bool {
        if self.config.relaunch.is_none() {
            return false;
        }
        if self.rust.build_needed {
            return true;
        }
        if let Some((checked, stale)) = self.rust.stale_checked {
            if checked.elapsed() < Duration::from_secs(2) {
                return stale;
            }
        }
        let built = std::env::current_exe()
            .and_then(std::fs::metadata)
            .and_then(|m| m.modified())
            .ok();
        let mut dirs: Vec<PathBuf> = self.config.code_dirs.clone();
        dirs.extend(self.config.rust_script_dir.clone());
        let stale = built.is_some_and(|built| dirs.iter().any(|dir| newer_rust_file(dir, built, 0)));
        self.rust.stale_checked = Some((Instant::now(), stale));
        stale
    }

    /// Сохранили файл `.rs`: один раз подсказать, что код заработает после Build.
    pub(crate) fn hint_rust_saved(&mut self, path: &Path) {
        let is_rust = path.extension().is_some_and(|e| e == "rs");
        if is_rust && self.config.relaunch.is_some() {
            self.rust.stale_checked = None;
            if !std::mem::replace(&mut self.rust.hint_shown, true) {
                log::info!(
                    "Rust code is compiled into the editor: press Build (Ctrl+B) to rebuild the game and \
                     reload the editor with it — the scene, selection and open files are kept"
                );
            }
        }
    }

    /// Переименовать файл скрипта (в том же каталоге); `mod.rs` каталога скриптов
    /// на Rust пишется заново.
    pub(crate) fn rename_script(&mut self, from: &Path, name: &str) -> Result<(), EditorError> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\']) || name == "mod.rs" {
            return Err(EditorError::Other(format!("`{name}` is not a valid file name")));
        }
        let to = from.with_file_name(name);
        if to.exists() {
            return Err(EditorError::Other(format!("`{}` already exists; pick another name", to.display())));
        }
        std::fs::rename(from, &to).map_err(|source| SceneError::Io {
            path: from.display().to_string(),
            source,
        })?;
        log::info!("renamed `{}` to `{name}`", from.display());
        self.code.rename_document(from, &to);
        self.after_script_file_change(from);
        Ok(())
    }

    /// Удалить файл скрипта с диска.
    pub(crate) fn delete_script(&mut self, path: &Path) -> Result<(), EditorError> {
        std::fs::remove_file(path).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })?;
        log::info!("deleted `{}`", path.display());
        self.code.close_path(path);
        self.after_script_file_change(path);
        Ok(())
    }

    fn after_script_file_change(&mut self, path: &Path) {
        self.code.scan(&self.config.code_dirs);
        let in_rust_dir = match (&self.config.rust_script_dir, path.parent()) {
            (Some(dir), Some(parent)) => same_dir(dir, parent),
            _ => false,
        };
        if in_rust_dir {
            if let Some(dir) = self.config.rust_script_dir.clone() {
                if let Err(err) = regenerate_mod(&dir) {
                    log::error!("{err}");
                }
            }
            self.rust.build_needed = true;
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Есть ли в каталоге (до 4 уровней) файл `.rs` новее `than`.
fn newer_rust_file(dir: &Path, than: SystemTime, depth: usize) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth < 4 && !matches!(name.as_str(), "target" | "build" | "bin" | ".git") && !name.starts_with('.')
                && newer_rust_file(&path, than, depth + 1)
            {
                return true;
            }
        } else if path.extension().is_some_and(|e| e == "rs")
            && entry.metadata().and_then(|m| m.modified()).is_ok_and(|modified| modified > than)
        {
            return true;
        }
    }
    false
}

// ----------------------------------------------------------------------
// Интерфейс
// ----------------------------------------------------------------------

/// Выбор языка в окне «New Script».
pub(crate) fn language_switch(ui: &mut egui::Ui, rust: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Language").color(theme::TEXT_MUTED));
        ui.add_space(theme::SP_M);
        ui.selectable_value(rust, true, "Rust");
        ui.selectable_value(rust, false, "C++");
    });
    ui.add_space(theme::SP_M);
}

/// Пояснение в окне «New Script»: что произойдёт, или что настроить.
pub(crate) fn script_dialog_text(rust: bool, config: &EditorConfig) -> String {
    if rust {
        match (&config.rust_script_dir, &config.relaunch) {
            (None, _) => "This editor does not know where Rust scripts live. Start it with \
                          `EditorConfig::rust_scripts(\"src/scripts\")` and `.relaunch(…)`, or open the game \
                          with `engine::editor_main!` (see docs/api/editor.md)."
                .to_string(),
            (Some(_), None) => "The type name (UpperCamelCase, e.g. EnemyPatrol) becomes the component name. \
                                No relaunch command is configured (`EditorConfig::relaunch`), so build the game \
                                and restart the editor yourself."
                .to_string(),
            (Some(_), Some(_)) => "The type name (UpperCamelCase, e.g. EnemyPatrol) becomes the component name. \
                                   The file opens in the Code tab and the game is rebuilt; the editor then \
                                   reloads itself with the new code and attaches the script to the selected object."
                .to_string(),
        }
    } else if config.script_dir.is_none() {
        "This editor does not know where C++ scripts live. Start it with \
         `EditorConfig::cpp_scripts(dir)`."
            .to_string()
    } else {
        "The class name becomes the component name. The plugin is rebuilt and the script is attached to \
         the selected object."
            .to_string()
    }
}

/// Полоса над текстом во вкладке Code: исходники на Rust новее программы — собрать.
pub(crate) fn code_banner(editor: &mut Editor, ui: &mut egui::Ui) {
    if editor.config.relaunch.is_none() {
        return;
    }
    let running = editor.code.build_status() == BuildStatus::Running;
    let (icon, color, text, button) = if editor.rust.handover.is_some() {
        (Icon::Build, theme::OK, "Starting the rebuilt editor… it opens with this scene and these files.", false)
    } else if running && editor.rust.result.is_some() {
        (Icon::Build, theme::WARN, "Building… the editor reloads itself with the new code when the build succeeds.", false)
    } else if editor.rust_sources_changed() {
        (
            Icon::Warn,
            theme::WARN,
            "Rust code changed since this editor was built. Build compiles it and reloads the editor; \
             the scene, selection and open files are kept.",
            !running,
        )
    } else {
        return;
    };
    let mut build = false;
    egui::Frame::none()
        .fill(theme::mix(theme::BG_PANEL, color, 0.16))
        .inner_margin(egui::Margin::symmetric(theme::SP_L, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::inline(ui, icon, color);
                ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT)).wrap());
                if button {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        build = theme::primary_button(ui, "Build and Reload")
                            .on_hover_text(crate::shortcuts::label(ui.ctx(), "build-code"))
                            .clicked();
                    });
                }
            });
        });
    if build {
        editor.actions.push(Action::Build);
    }
}

/// Тело карточки потерянного скрипта в инспекторе: предупреждение и поля только
/// для чтения.
pub(crate) fn missing_script_body(ui: &mut egui::Ui, missing: &MissingScript) {
    ui.horizontal_wrapped(|ui| {
        icons::inline(ui, Icon::Warn, theme::WARN);
        ui.label(
            egui::RichText::new(format!(
                "Missing script: `{}` is not in this build. Its data is kept and saved unchanged; \
                 restore the type and build, or remove the component.",
                missing.kind()
            ))
            .color(theme::WARN),
        );
    });
    ui.add_space(theme::SP_S);
    for (field, value) in missing.fields() {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(120.0, 20.0), egui::Label::new(egui::RichText::new(theme::pretty_name(field)).color(theme::TEXT_MUTED)));
            ui.label(egui::RichText::new(format!("{value:?}")).font(theme::mono()).color(theme::TEXT_MUTED));
        });
    }
}

/// Окно подтверждения переименования / удаления файла скрипта.
pub(crate) fn script_file_dialog(editor: &mut Editor, ctx: &egui::Context) {
    let Some(dialog) = editor.rust.file_dialog.as_mut() else {
        return;
    };
    let mut confirm = false;
    let mut cancel = false;
    let frame = theme::dialog_frame(&ctx.style());
    let response = egui::Modal::new(egui::Id::new("editor_script_file_dialog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(400.0);
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let (title, text, label, danger) = match dialog {
                FileDialog::Rename { path, name } => {
                    let title = "Rename Script";
                    let text = format!(
                        "New file name for “{}”. Rust scripts must keep a snake_case name ending in `.rs`; the \
                         editor rewrites `mod.rs` for you.",
                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    );
                    ui.label(egui::RichText::new(title).font(theme::strong(theme::FONT_HEADING)).color(theme::TEXT_STRONG));
                    ui.add_space(theme::SP_M);
                    ui.label(egui::RichText::new(text).color(theme::TEXT_MUTED));
                    ui.add_space(theme::SP_L);
                    let edit = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    if edit.lost_focus() && enter {
                        confirm = true;
                    }
                    ("", String::new(), "Rename", false)
                }
                FileDialog::Delete { path } => (
                    "Delete script from disk?",
                    format!(
                        "“{}” is deleted permanently (there is no undo). Objects that use its components keep \
                         their data as missing scripts until you remove them.",
                        path.display()
                    ),
                    "Delete",
                    true,
                ),
            };
            if !title.is_empty() {
                ui.label(egui::RichText::new(title).font(theme::strong(theme::FONT_HEADING)).color(theme::TEXT_STRONG));
                ui.add_space(theme::SP_M);
                ui.label(egui::RichText::new(text).color(theme::TEXT_MUTED));
            }
            ui.add_space(theme::SP_L);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pressed = if danger { theme::danger_button(ui, label) } else { theme::primary_button(ui, label) };
                confirm |= pressed.clicked();
                cancel |= theme::secondary_button(ui, "Cancel").clicked();
            });
        });
    if response.should_close() && !confirm {
        cancel = true;
    }
    if confirm {
        match editor.rust.file_dialog.take() {
            Some(FileDialog::Rename { path, name }) => editor.apply(Action::RenameScript { from: path, to: name }),
            Some(FileDialog::Delete { path }) => editor.apply(Action::DeleteScript(path)),
            None => {}
        }
    } else if cancel {
        editor.rust.file_dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piska_rust_scripts_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn script_names_are_upper_camel_case_and_map_to_snake_case_files() {
        for good in ["EnemyPatrol", "Player2D", "A", "HTTPServer"] {
            assert!(validate_rust_name(good).is_ok(), "{good}");
        }
        for bad in ["enemy", "My Script", "9Lives", "", "Enemy_Patrol", "Ёж", "Self", "Type", "Match"] {
            assert!(validate_rust_name(bad).is_err(), "{bad}");
        }
        assert!(validate_rust_name("Type").unwrap_err().contains("keyword"));
        assert_eq!(snake_case("EnemyPatrol"), "enemy_patrol");
        assert_eq!(snake_case("HTTPServer"), "http_server");
        assert_eq!(snake_case("Player2D"), "player2_d");
        assert_eq!(snake_case("A"), "a");
        assert_eq!(snake_case("SpinAndBob"), "spin_and_bob");
    }

    #[test]
    fn the_template_declares_a_component_with_hooks_and_docs() {
        let text = rust_template("EnemyPatrol");
        assert!(text.contains("use engine::prelude::*;"));
        assert!(text.contains("component! {"));
        assert!(text.contains("pub struct EnemyPatrol {"));
        assert!(text.contains("pub speed: f32 = 90.0,"));
        assert!(text.contains("impl Component for EnemyPatrol {"));
        assert!(text.contains("fn on_start(&mut self, _ctx: &mut Ctx)"));
        assert!(text.contains("fn on_update(&mut self, ctx: &mut Ctx, dt: f32)"));
        assert!(text.contains("on_fixed_update") && text.contains("docs/api/gameplay.md"));
        assert!(!text.contains("{Name}"));
        // Разборщик видит в шаблоне ровно этот компонент.
        assert_eq!(
            declared_components(&text).unwrap(),
            [Declared { name: "EnemyPatrol".to_string(), public: true }]
        );
    }

    #[test]
    fn the_parser_finds_components_and_ignores_comments_strings_and_other_items() {
        let source = r##"
            // component! { pub struct InComment {} }
            /* component! { pub struct InBlock { /* nested */ } } */
            const TEXT: &str = "component! { pub struct InString {} }";
            const RAW: &str = r#"component! { pub struct InRaw { x: "}" } }"#;
            fn lifetime<'a>(x: &'a str) -> char { let _ = x; '}' }
            struct NotAComponent;

            component! {
                /// Doc with braces { and } and a `struct Fake` mention.
                pub struct First {
                    pub speed: f32 = 1.0,
                    pub offset: Vec3 = Vec3::new(0.0, { 1.0 }, 0.0),
                }
            }

            engine::prelude::component! {
                pub(crate) struct Second {}
            }

            component! {
                struct Hidden { pub x: i32 = 0 }
            }
        "##;
        let found = declared_components(source).unwrap();
        assert_eq!(
            found,
            [
                Declared { name: "First".to_string(), public: true },
                Declared { name: "Second".to_string(), public: true },
                Declared { name: "Hidden".to_string(), public: false },
            ]
        );
        assert!(declared_components("component! { pub struct Broken { pub x: f32 = 1.0, }").is_err());
        assert!(declared_components("fn x() { \"unterminated }").is_err());
        assert!(declared_components("fn x() { /* never closed }").is_err());
        assert!(declared_components("fn x() ) {").is_err());
        assert_eq!(declared_components("fn main() {}").unwrap(), []);
    }

    #[test]
    fn mod_rs_lists_every_script_and_skips_broken_files() {
        let dir = temp_dir("mod");
        std::fs::write(dir.join("orbit.rs"), rust_template("Orbit")).unwrap();
        std::fs::write(
            dir.join("pickups.rs"),
            "use engine::prelude::*;\ncomponent! { pub struct Coin { pub value: i32 = 1 } }\n\
             component! { pub struct Gem {} }\nimpl Component for Coin {}\nimpl Component for Gem {}\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken.rs"), "component! { pub struct Broken {").unwrap();
        std::fs::write(dir.join("Bad-Name.rs"), rust_template("BadName")).unwrap();
        std::fs::write(dir.join("helpers.rs"), "pub fn helper() {}\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not code").unwrap();
        std::fs::write(dir.join("mod.rs"), "// old\n").unwrap();

        let folder = regenerate_mod(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("mod.rs")).unwrap();
        assert!(text.contains("ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР"), "{text}");
        assert!(text.contains("pub mod orbit;\n") && text.contains("pub mod pickups;\n") && text.contains("pub mod helpers;\n"));
        assert!(!text.contains("broken") && !text.contains("Bad"), "{text}");
        assert!(text.contains("pub fn register(app: &mut engine::prelude::App) {"));
        assert!(text.contains("    app.register::<orbit::Orbit>();\n"));
        assert!(text.contains("    app.register::<pickups::Coin>();\n    app.register::<pickups::Gem>();\n"));
        assert_eq!(folder.components().collect::<Vec<_>>(), ["Orbit", "Coin", "Gem"]);
        assert_eq!(folder.warnings.len(), 2, "{:?}", folder.warnings);
        assert!(folder.warnings.iter().any(|w| w.contains("broken.rs")));
        assert!(folder.warnings.iter().any(|w| w.contains("Bad-Name.rs")));
        // Битый файл не удаляется.
        assert!(dir.join("broken.rs").is_file());

        // Пустой каталог: `register` ничего не делает, но компилируется без предупреждений.
        let empty = temp_dir("mod_empty");
        regenerate_mod(&empty).unwrap();
        let text = std::fs::read_to_string(empty.join("mod.rs")).unwrap();
        assert!(text.contains("    let _ = app;\n") && !text.contains("pub mod"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(empty);
    }

    #[test]
    fn the_session_file_round_trips() {
        let session = Session {
            scene_path: Some("scenes/demo3d.scene".to_string()),
            dirty: true,
            scene: Some(SceneData { name: "Demo".to_string(), objects: Vec::new() }),
            selection: Some(vec![2, 0, 1]),
            view_tab: Some("Code".to_string()),
            scene_3d: true,
            orbit: [1.0, 2.0, 3.0, 0.5, 0.25, 9.0],
            camera_2d: [4.0, -1.5, 6.0],
            documents: vec![PathBuf::from("src/scripts/orbit.rs"), PathBuf::from("C:/x y/=odd.rs")],
            active_document: 1,
            cursor_line: 17,
            pending: vec![("EnemyPatrol".to_string(), vec![0, 3])],
            registered: vec!["Camera".to_string(), "Orbit".to_string()],
            script_rust: true,
            build_started: 1000.5,
            relaunched: 1030.25,
        };
        let text = session.to_text();
        assert_eq!(Session::from_text(&text).unwrap(), session);
        assert_eq!(Session::from_text(&text.replace('\n', "\r\n")).unwrap(), session);
        assert!(Session::from_text("hello").is_err());
    }

    #[test]
    fn cargo_json_lines_give_the_executable() {
        let line = r#"{"reason":"compiler-artifact","target":{"name":"demo3d"},"executable":"C:\\t\\debug\\examples\\demo3d.exe","fresh":false}"#;
        assert_eq!(json_executable(line), Some(PathBuf::from(r"C:\t\debug\examples\demo3d.exe")));
        assert_eq!(json_executable(r#"{"reason":"compiler-artifact","executable":null}"#), None);
        assert_eq!(json_executable(r#"{"executable":"/a/\u0062"}"#), Some(PathBuf::from("/a/b")));
    }

    #[test]
    fn the_running_program_is_moved_aside_and_restored_when_the_build_fails() {
        let root = temp_dir("aside");
        let debug = root.join("debug");
        let examples = debug.join("examples");
        let deps = debug.join("deps");
        std::fs::create_dir_all(&examples).unwrap();
        std::fs::create_dir_all(&deps).unwrap();
        // cargo: deps/<crate>-<hash>.exe и его «жёсткая ссылка» examples/<name>.exe.
        let exe = examples.join("demo3d.exe");
        std::fs::write(deps.join("demo3d-abc123.exe"), b"program").unwrap();
        std::fs::copy(deps.join("demo3d-abc123.exe"), &exe).unwrap();
        let mtime = std::fs::metadata(deps.join("demo3d-abc123.exe")).unwrap().modified().unwrap();
        std::fs::File::options().write(true).open(&exe).unwrap().set_modified(mtime).unwrap();
        std::fs::write(examples.join("demo3d.pdb"), b"symbols").unwrap();
        std::fs::write(deps.join("demo3d-other.exe"), b"another build").unwrap();
        std::fs::write(deps.join("unrelated-abc.exe"), b"program").unwrap();

        // Чужое имя — ничего не трогаем.
        assert!(set_aside(&exe, "something_else", 7).moved.is_empty());
        // Копия вне каталога cargo — тоже.
        let copy_dir = root.join("run");
        std::fs::create_dir_all(&copy_dir).unwrap();
        std::fs::write(copy_dir.join("demo3d.exe"), b"program").unwrap();
        assert!(set_aside(&copy_dir.join("demo3d.exe"), "demo3d", 7).moved.is_empty());

        let aside = set_aside(&exe, "demo3d", 7);
        let moved: Vec<String> = aside
            .moved
            .iter()
            .map(|(_, to)| to.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(moved, ["demo3d.old-7.exe", "demo3d.old-7.pdb", "demo3d-abc123.old-7.exe"]);
        assert!(!exe.exists() && !deps.join("demo3d-abc123.exe").exists());
        assert!(deps.join("demo3d-other.exe").exists(), "a different build is not ours");
        assert!(deps.join("unrelated-abc.exe").exists());

        // Сборка не удалась: имена возвращаются.
        aside.restore();
        assert!(exe.is_file() && examples.join("demo3d.pdb").is_file() && deps.join("demo3d-abc123.exe").is_file());
        assert!(!examples.join("demo3d.old-7.exe").exists());

        // Второй перенос в том же процессе не затирает первый.
        std::fs::write(examples.join("demo3d.old-7.exe"), b"older").unwrap();
        assert_eq!(aside_name(&exe, 7).file_name().unwrap(), "demo3d.old-7-1.exe");

        // Программа пакета: `debug/<bin>.exe` и `debug/deps/<crate>.exe` — одна жёсткая ссылка.
        let bin = debug.join("template-editor.exe");
        std::fs::write(deps.join("template_editor.exe"), b"editor").unwrap();
        std::fs::hard_link(deps.join("template_editor.exe"), &bin).unwrap();
        std::fs::write(debug.join("template_editor.pdb"), b"symbols").unwrap();
        let aside = set_aside(&bin, "template-editor", 9);
        let moved: Vec<String> = aside
            .moved
            .iter()
            .map(|(_, to)| to.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(moved, ["template-editor.old-9.exe", "template_editor.old-9.pdb", "template_editor.old-9.exe"]);
        assert!(!bin.exists() && !deps.join("template_editor.exe").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn relaunch_commands_build_the_right_target() {
        let example = Relaunch::example("engine", "demo3d").dir("ws");
        assert_eq!(example.target, "demo3d");
        assert_eq!(example.build.program, "cargo");
        assert_eq!(&example.build.args[..5], ["build", "-p", "engine", "--example", "demo3d"]);
        assert_eq!(example.build.dir.as_deref(), Some(Path::new("ws")));
        let bin = Relaunch::bin("template", "template-editor", &["editor"]);
        assert_eq!(&bin.build.args[..7], ["build", "-p", "template", "--bin", "template-editor", "--features", "editor"]);
        assert_eq!(bin.target, "template-editor");
    }
}
