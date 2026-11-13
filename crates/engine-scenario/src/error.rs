//! Ошибки Scenario: позиция в исходнике, строка-выдержка с кареткой, подсказка и
//! короткий стек вызовов. Ни одна ошибка скрипта не паникует движок.

use std::fmt;
use std::rc::Rc;

/// Позиция в исходнике: строка и столбец (с единицы, столбец — в символах).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub const fn new(line: u32, col: u32) -> Self {
        Self { line, col }
    }
}

/// Вид ошибки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// Ошибка разбора: лексер, парсер, компилятор.
    Syntax,
    /// Ошибка связывания: неизвестное имя.
    Link,
    /// Ошибка во время выполнения.
    Runtime,
    /// Скрипт упёрся в ограничение (бесконечный цикл, память, глубина вызовов).
    Limit,
}

/// Кадр стека вызовов в сообщении об ошибке.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackFrame {
    pub function: String,
    pub pos: Pos,
}

/// Ошибка скрипта с местом: `file:line:col`.
#[derive(Clone, Debug)]
pub struct ScriptError {
    pub kind: ErrorKind,
    pub message: String,
    pub file: Rc<str>,
    pub pos: Pos,
    /// Строка исходника, на которую указывает ошибка (без перевода строки).
    pub excerpt: String,
    pub hint: Option<String>,
    /// Стек вызовов, от самого глубокого кадра к внешнему (только для ошибок выполнения).
    pub stack: Vec<StackFrame>,
}

impl ScriptError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>, file: &Rc<str>, source: &str, pos: Pos) -> Self {
        Self {
            kind,
            message: message.into(),
            file: file.clone(),
            pos,
            excerpt: line_of(source, pos.line),
            hint: None,
            stack: Vec::new(),
        }
    }

    pub(crate) fn with_hint(mut self, hint: Option<String>) -> Self {
        self.hint = hint;
        self
    }

    /// `file:line:col` — так Console делает место кликабельным.
    pub fn location(&self) -> String {
        format!("{}:{}:{}", self.file, self.pos.line, self.pos.col)
    }

    /// Одна строка: `file:line:col: message`.
    pub fn short(&self) -> String {
        format!("{}: {}", self.location(), self.message)
    }

    /// Полный текст: место, сообщение, выдержка с кареткой, подсказка и стек.
    pub fn report(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location(), self.message)?;
        if !self.excerpt.is_empty() {
            let number = self.pos.line.to_string();
            let pad = " ".repeat(number.len());
            let excerpt: String = self.excerpt.chars().map(|c| if c == '\t' { ' ' } else { c }).collect();
            let caret = " ".repeat((self.pos.col.max(1) - 1) as usize);
            write!(f, "\n {number} | {excerpt}\n {pad} | {caret}^")?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n hint: {hint}")?;
        }
        for frame in &self.stack {
            write!(f, "\n   at {} ({}:{}:{})", frame.function, self.file, frame.pos.line, frame.pos.col)?;
        }
        Ok(())
    }
}

impl std::error::Error for ScriptError {}

/// Строка `line` (с единицы) исходника.
pub(crate) fn line_of(source: &str, line: u32) -> String {
    source.lines().nth(line.saturating_sub(1) as usize).unwrap_or("").trim_end_matches('\r').to_string()
}

/// Расстояние Дамерау–Левенштейна (перестановка соседних букв — одна правка) — для
/// подсказок «did you mean …?».
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().map(|c| c.to_ascii_lowercase()).collect();
    let b: Vec<char> = b.chars().map(|c| c.to_ascii_lowercase()).collect();
    let w = b.len() + 1;
    let mut d = vec![0usize; (a.len() + 1) * w];
    for i in 0..=a.len() {
        d[i * w] = i;
    }
    for (j, cell) in d.iter_mut().enumerate().take(w) {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[(i - 1) * w + j] + 1).min(d[i * w + j - 1] + 1).min(d[(i - 1) * w + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * w + j - 2] + 1);
            }
            d[i * w + j] = v;
        }
    }
    d[a.len() * w + b.len()]
}

/// Ближайшее к `name` имя из `candidates`, если оно достаточно похоже.
pub(crate) fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let limit = (name.chars().count() / 3).max(1);
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        if candidate == name || candidate.starts_with("__") {
            continue;
        }
        let d = edit_distance(name, candidate);
        let better = match best {
            None => true,
            Some((bd, bn)) => d < bd || (d == bd && candidate < bn),
        };
        if d <= limit && better {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, n)| n.to_string())
}
