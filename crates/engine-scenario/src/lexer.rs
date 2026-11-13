//! Лексер: исходный текст → токены. Понимает комментарии обоих языков
//! (`//`, `/* */`, `--`, `--[[ ]]`), строки в кавычках трёх видов и шаблоны
//! `` `текст ${выражение}` ``.

use std::rc::Rc;

use crate::error::{ErrorKind, Pos, ScriptError};

/// Часть шаблонной строки.
#[derive(Clone, Debug, PartialEq)]
pub enum TPart {
    Lit(Rc<str>),
    /// Исходник выражения внутри `${…}` и позиция его первого символа.
    Expr(Rc<str>, Pos),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(f64),
    Str(Rc<str>),
    Template(Rc<[TPart]>),
    Name(Rc<str>),
    // Ключевые слова.
    Let,
    Const,
    Var,
    Local,
    Function,
    Return,
    If,
    Then,
    Else,
    Elseif,
    End,
    While,
    Do,
    For,
    In,
    Of,
    Repeat,
    Until,
    Break,
    Continue,
    True,
    False,
    Nil,
    And,
    Or,
    Not,
    Typeof,
    Try,
    Catch,
    Throw,
    // Знаки.
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Dot,
    DotDot,
    Ellipsis,
    Colon,
    Question,
    QQ,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    StarStar,
    Hash,
    Assign,
    EqEq,
    EqEqEq,
    BangEq,
    BangEqEq,
    TildeEq,
    Lt,
    Le,
    Gt,
    Ge,
    Bang,
    AndAnd,
    OrOr,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    DotDotEq,
    PlusPlus,
    Eof,
}

impl Tok {
    /// Как токен выглядит в сообщении об ошибке.
    pub fn describe(&self) -> String {
        match self {
            Tok::Num(n) => format!("number `{}`", crate::value::fmt_num(*n)),
            Tok::Str(_) => "a string".into(),
            Tok::Template(_) => "a template string".into(),
            Tok::Name(n) => format!("`{n}`"),
            Tok::Eof => "the end of the file".into(),
            other => format!("`{}`", other.text()),
        }
    }

    pub fn text(&self) -> &'static str {
        match self {
            Tok::Let => "let",
            Tok::Const => "const",
            Tok::Var => "var",
            Tok::Local => "local",
            Tok::Function => "function",
            Tok::Return => "return",
            Tok::If => "if",
            Tok::Then => "then",
            Tok::Else => "else",
            Tok::Elseif => "elseif",
            Tok::End => "end",
            Tok::While => "while",
            Tok::Do => "do",
            Tok::For => "for",
            Tok::In => "in",
            Tok::Of => "of",
            Tok::Repeat => "repeat",
            Tok::Until => "until",
            Tok::Break => "break",
            Tok::Continue => "continue",
            Tok::True => "true",
            Tok::False => "false",
            Tok::Nil => "nil",
            Tok::And => "and",
            Tok::Or => "or",
            Tok::Not => "not",
            Tok::Typeof => "typeof",
            Tok::Try => "try",
            Tok::Catch => "catch",
            Tok::Throw => "throw",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Semi => ";",
            Tok::Dot => ".",
            Tok::DotDot => "..",
            Tok::Ellipsis => "...",
            Tok::Colon => ":",
            Tok::Question => "?",
            Tok::QQ => "??",
            Tok::Arrow => "=>",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Caret => "^",
            Tok::StarStar => "**",
            Tok::Hash => "#",
            Tok::Assign => "=",
            Tok::EqEq => "==",
            Tok::EqEqEq => "===",
            Tok::BangEq => "!=",
            Tok::BangEqEq => "!==",
            Tok::TildeEq => "~=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::Bang => "!",
            Tok::AndAnd => "&&",
            Tok::OrOr => "||",
            Tok::PlusEq => "+=",
            Tok::MinusEq => "-=",
            Tok::StarEq => "*=",
            Tok::SlashEq => "/=",
            Tok::PercentEq => "%=",
            Tok::DotDotEq => "..=",
            Tok::PlusPlus => "++",
            Tok::Num(_) | Tok::Str(_) | Tok::Template(_) | Tok::Name(_) | Tok::Eof => "",
        }
    }
}

/// Все ключевые слова (для подсветки и проверки имён).
pub const KEYWORDS: &[&str] = &[
    "let", "const", "var", "local", "function", "return", "if", "then", "else", "elseif", "end", "while", "do",
    "for", "in", "of", "repeat", "until", "break", "continue", "true", "false", "nil", "null", "and", "or", "not",
    "typeof", "try", "catch", "throw",
];

fn keyword(word: &str) -> Option<Tok> {
    Some(match word {
        "let" => Tok::Let,
        "const" => Tok::Const,
        "var" => Tok::Var,
        "local" => Tok::Local,
        "function" => Tok::Function,
        "return" => Tok::Return,
        "if" => Tok::If,
        "then" => Tok::Then,
        "else" => Tok::Else,
        "elseif" => Tok::Elseif,
        "end" => Tok::End,
        "while" => Tok::While,
        "do" => Tok::Do,
        "for" => Tok::For,
        "in" => Tok::In,
        "of" => Tok::Of,
        "repeat" => Tok::Repeat,
        "until" => Tok::Until,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "true" => Tok::True,
        "false" => Tok::False,
        "nil" | "null" => Tok::Nil,
        "and" => Tok::And,
        "or" => Tok::Or,
        "not" => Tok::Not,
        "typeof" => Tok::Typeof,
        "try" => Tok::Try,
        "catch" => Tok::Catch,
        "throw" => Tok::Throw,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
    /// Перед токеном был перевод строки.
    pub nl_before: bool,
    /// Сразу за токеном (без пробела) начинался комментарий `--`: вероятно, имелся в виду `x--`.
    pub dashes_after: bool,
}

struct Lexer<'a> {
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
    file: &'a Rc<str>,
    source: &'a str,
}

/// Разбивает `text` на токены. `start` — позиция его первого символа (для `${…}` в
/// шаблонах), выдержки для ошибок берутся из `full` (весь файл).
pub fn tokenize_in(text: &str, full: &str, file: &Rc<str>, start: Pos) -> Result<Vec<Token>, ScriptError> {
    let mut lx = Lexer { chars: text.chars().collect(), i: 0, line: start.line, col: start.col, file, source: full };
    lx.run()
}

impl Lexer<'_> {
    fn peek(&self) -> char {
        self.chars.get(self.i).copied().unwrap_or('\0')
    }

    fn peek_at(&self, k: usize) -> char {
        self.chars.get(self.i + k).copied().unwrap_or('\0')
    }

    fn at_end(&self) -> bool {
        self.i >= self.chars.len()
    }

    fn bump(&mut self) -> char {
        let c = self.peek();
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        c
    }

    fn pos(&self) -> Pos {
        Pos::new(self.line, self.col)
    }

    fn err(&self, pos: Pos, msg: impl Into<String>) -> ScriptError {
        ScriptError::new(ErrorKind::Syntax, msg, self.file, self.source, pos)
    }

    fn run(&mut self) -> Result<Vec<Token>, ScriptError> {
        let mut out: Vec<Token> = Vec::new();
        let mut nl = true;
        loop {
            // Пробелы и комментарии.
            let mut glued = true;
            loop {
                let c = self.peek();
                if self.at_end() {
                    break;
                }
                if c == '\n' {
                    nl = true;
                    glued = false;
                    self.bump();
                } else if c.is_whitespace() {
                    glued = false;
                    self.bump();
                } else if c == '/' && self.peek_at(1) == '/' {
                    while !self.at_end() && self.peek() != '\n' {
                        self.bump();
                    }
                } else if c == '/' && self.peek_at(1) == '*' {
                    let start = self.pos();
                    self.bump();
                    self.bump();
                    loop {
                        if self.at_end() {
                            return Err(self.err(start, "this `/*` comment is never closed with `*/`"));
                        }
                        if self.peek() == '*' && self.peek_at(1) == '/' {
                            self.bump();
                            self.bump();
                            break;
                        }
                        if self.bump() == '\n' {
                            nl = true;
                        }
                    }
                } else if c == '-' && self.peek_at(1) == '-' {
                    if glued {
                        if let Some(last) = out.last_mut() {
                            if matches!(last.tok, Tok::Name(_) | Tok::RParen | Tok::RBracket) {
                                last.dashes_after = true;
                            }
                        }
                    }
                    let start = self.pos();
                    self.bump();
                    self.bump();
                    if self.peek() == '[' && self.peek_at(1) == '[' {
                        self.bump();
                        self.bump();
                        loop {
                            if self.at_end() {
                                return Err(self.err(start, "this `--[[` comment is never closed with `]]`"));
                            }
                            if self.peek() == ']' && self.peek_at(1) == ']' {
                                self.bump();
                                self.bump();
                                break;
                            }
                            if self.bump() == '\n' {
                                nl = true;
                            }
                        }
                    } else {
                        while !self.at_end() && self.peek() != '\n' {
                            self.bump();
                        }
                    }
                } else {
                    break;
                }
            }
            let pos = self.pos();
            if self.at_end() {
                out.push(Token { tok: Tok::Eof, pos, nl_before: true, dashes_after: false });
                return Ok(out);
            }
            let tok = self.token()?;
            out.push(Token { tok, pos, nl_before: nl, dashes_after: false });
            nl = false;
        }
    }

    fn token(&mut self) -> Result<Tok, ScriptError> {
        let pos = self.pos();
        let c = self.peek();
        if c.is_ascii_digit() || (c == '.' && self.peek_at(1).is_ascii_digit()) {
            return self.number();
        }
        if c == '_' || c.is_alphabetic() {
            let mut word = String::new();
            while self.peek() == '_' || self.peek().is_alphanumeric() {
                word.push(self.bump());
            }
            return Ok(keyword(&word).unwrap_or_else(|| Tok::Name(word.into())));
        }
        if c == '"' || c == '\'' {
            return self.string(c);
        }
        if c == '`' {
            return self.template();
        }
        self.bump();
        let n = self.peek();
        let tok = match c {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            ',' => Tok::Comma,
            ';' => Tok::Semi,
            ':' => Tok::Colon,
            '#' => Tok::Hash,
            '^' => Tok::Caret,
            '.' => {
                if n == '.' {
                    self.bump();
                    if self.peek() == '.' {
                        self.bump();
                        Tok::Ellipsis
                    } else if self.peek() == '=' {
                        self.bump();
                        Tok::DotDotEq
                    } else {
                        Tok::DotDot
                    }
                } else {
                    Tok::Dot
                }
            }
            '?' => {
                if n == '?' {
                    self.bump();
                    Tok::QQ
                } else {
                    Tok::Question
                }
            }
            '=' => {
                if n == '>' {
                    self.bump();
                    Tok::Arrow
                } else if n == '=' {
                    self.bump();
                    if self.peek() == '=' {
                        self.bump();
                        Tok::EqEqEq
                    } else {
                        Tok::EqEq
                    }
                } else {
                    Tok::Assign
                }
            }
            '!' => {
                if n == '=' {
                    self.bump();
                    if self.peek() == '=' {
                        self.bump();
                        Tok::BangEqEq
                    } else {
                        Tok::BangEq
                    }
                } else {
                    Tok::Bang
                }
            }
            '~' => {
                if n == '=' {
                    self.bump();
                    Tok::TildeEq
                } else {
                    return Err(self.err(pos, "unexpected `~` — did you mean `~=` (not equal)?"));
                }
            }
            '<' => self.with_eq(Tok::Lt, Tok::Le),
            '>' => self.with_eq(Tok::Gt, Tok::Ge),
            '+' => {
                if n == '+' {
                    self.bump();
                    Tok::PlusPlus
                } else {
                    self.with_eq(Tok::Plus, Tok::PlusEq)
                }
            }
            '-' => self.with_eq(Tok::Minus, Tok::MinusEq),
            '*' => {
                if n == '*' {
                    self.bump();
                    Tok::StarStar
                } else {
                    self.with_eq(Tok::Star, Tok::StarEq)
                }
            }
            '/' => self.with_eq(Tok::Slash, Tok::SlashEq),
            '%' => self.with_eq(Tok::Percent, Tok::PercentEq),
            '&' => {
                if n == '&' {
                    self.bump();
                    Tok::AndAnd
                } else {
                    return Err(self.err(pos, "unexpected `&` — use `&&` or `and`"));
                }
            }
            '|' => {
                if n == '|' {
                    self.bump();
                    Tok::OrOr
                } else {
                    return Err(self.err(pos, "unexpected `|` — use `||` or `or`"));
                }
            }
            other => {
                return Err(self.err(pos, format!("unexpected character `{other}`")));
            }
        };
        Ok(tok)
    }

    fn with_eq(&mut self, plain: Tok, eq: Tok) -> Tok {
        if self.peek() == '=' {
            self.bump();
            eq
        } else {
            plain
        }
    }

    fn number(&mut self) -> Result<Tok, ScriptError> {
        let pos = self.pos();
        let mut text = String::new();
        if self.peek() == '0' && matches!(self.peek_at(1), 'x' | 'X') {
            self.bump();
            self.bump();
            while self.peek().is_ascii_hexdigit() || self.peek() == '_' {
                let c = self.bump();
                if c != '_' {
                    text.push(c);
                }
            }
            if self.peek().is_alphanumeric() || text.is_empty() {
                return Err(self.err(pos, "malformed hexadecimal number"));
            }
            return u64::from_str_radix(&text, 16)
                .map(|v| Tok::Num(v as f64))
                .map_err(|_| self.err(pos, "hexadecimal number is too large"));
        }
        while self.peek().is_ascii_digit() || self.peek() == '_' {
            let c = self.bump();
            if c != '_' {
                text.push(c);
            }
        }
        if self.peek() == '.' && self.peek_at(1).is_ascii_digit() {
            text.push(self.bump());
            while self.peek().is_ascii_digit() || self.peek() == '_' {
                let c = self.bump();
                if c != '_' {
                    text.push(c);
                }
            }
        }
        if matches!(self.peek(), 'e' | 'E')
            && (self.peek_at(1).is_ascii_digit()
                || (matches!(self.peek_at(1), '+' | '-') && self.peek_at(2).is_ascii_digit()))
        {
            text.push(self.bump());
            if matches!(self.peek(), '+' | '-') {
                text.push(self.bump());
            }
            while self.peek().is_ascii_digit() {
                text.push(self.bump());
            }
        }
        if self.peek() == '_' || self.peek().is_alphabetic() {
            return Err(self.err(pos, "malformed number: a name can't start with a digit"));
        }
        text.parse::<f64>().map(Tok::Num).map_err(|_| self.err(pos, "malformed number"))
    }

    /// Разбирает `\`-последовательность (обратная косая черта уже съедена).
    fn escape(&mut self, out: &mut String) -> Result<(), ScriptError> {
        let pos = self.pos();
        if self.at_end() {
            return Err(self.err(pos, "unfinished escape sequence at the end of the file"));
        }
        let c = self.bump();
        match c {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            '\\' | '"' | '\'' | '`' | '$' | '{' | '}' => out.push(c),
            '\n' => {}
            'x' => {
                let mut code = 0u32;
                for _ in 0..2 {
                    let d = self.peek().to_digit(16).ok_or_else(|| self.err(pos, "`\\x` needs two hex digits"))?;
                    self.bump();
                    code = code * 16 + d;
                }
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            'u' => {
                if self.peek() != '{' {
                    return Err(self.err(pos, "write unicode escapes as `\\u{1F600}`"));
                }
                self.bump();
                let mut code = 0u32;
                let mut digits = 0;
                while self.peek() != '}' {
                    let d = self.peek().to_digit(16).ok_or_else(|| self.err(pos, "bad `\\u{…}` escape"))?;
                    self.bump();
                    code = code.saturating_mul(16).saturating_add(d);
                    digits += 1;
                    if digits > 6 {
                        return Err(self.err(pos, "bad `\\u{…}` escape"));
                    }
                }
                self.bump();
                out.push(char::from_u32(code).ok_or_else(|| self.err(pos, "`\\u{…}` is not a valid character"))?);
            }
            other => return Err(self.err(pos, format!("unknown escape `\\{other}` — use `\\\\` for a backslash"))),
        }
        Ok(())
    }

    fn string(&mut self, quote: char) -> Result<Tok, ScriptError> {
        let pos = self.pos();
        self.bump();
        let mut out = String::new();
        loop {
            if self.at_end() || self.peek() == '\n' {
                return Err(self.err(
                    pos,
                    format!("this string is never closed with {quote} (use `\\n` or a `template` string for line breaks)"),
                ));
            }
            let c = self.bump();
            if c == quote {
                break;
            }
            if c == '\\' {
                self.escape(&mut out)?;
            } else {
                out.push(c);
            }
        }
        Ok(Tok::Str(out.into()))
    }

    fn template(&mut self) -> Result<Tok, ScriptError> {
        let pos = self.pos();
        self.bump();
        let mut parts = Vec::new();
        let mut lit = String::new();
        loop {
            if self.at_end() {
                return Err(self.err(pos, "this template string is never closed with `"));
            }
            let c = self.peek();
            if c == '`' {
                self.bump();
                break;
            }
            if c == '\\' {
                self.bump();
                self.escape(&mut lit)?;
                continue;
            }
            if c == '$' && self.peek_at(1) == '{' {
                self.bump();
                self.bump();
                if !lit.is_empty() {
                    parts.push(TPart::Lit(std::mem::take(&mut lit).into()));
                }
                let expr_pos = self.pos();
                let mut depth = 0usize;
                let mut src = String::new();
                loop {
                    if self.at_end() {
                        return Err(self.err(expr_pos, "`${` in this template string is never closed with `}`"));
                    }
                    let c = self.peek();
                    if c == '}' && depth == 0 {
                        self.bump();
                        break;
                    }
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        '"' | '\'' => {
                            // Строка внутри выражения: копируем целиком, чтобы `}` в ней не закрыл `${`.
                            src.push(self.bump());
                            while !self.at_end() && self.peek() != c && self.peek() != '\n' {
                                let d = self.bump();
                                src.push(d);
                                if d == '\\' && !self.at_end() {
                                    src.push(self.bump());
                                }
                            }
                            if !self.at_end() {
                                src.push(self.bump());
                            }
                            continue;
                        }
                        _ => {}
                    }
                    src.push(self.bump());
                }
                if src.trim().is_empty() {
                    return Err(self.err(expr_pos, "empty `${}` in a template string"));
                }
                parts.push(TPart::Expr(src.into(), expr_pos));
                continue;
            }
            lit.push(self.bump());
        }
        if !lit.is_empty() || parts.is_empty() {
            parts.push(TPart::Lit(lit.into()));
        }
        Ok(Tok::Template(parts.into()))
    }
}
