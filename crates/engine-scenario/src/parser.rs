//! Парсер: токены → [`ast`](crate::ast). Рекурсивный спуск, выражения — по приоритетам
//! (Pratt). Обе формы блоков: `{ … }` (JavaScript) и `then/do … end` (Lua).
//!
//! Правило перевода строки: инструкция заканчивается переводом строки или `;`, но
//! выражение продолжается, если строка оборвалась на операторе. `(`, `[` и шаблон в начале
//! новой строки **не** приклеиваются к выражению на предыдущей строке (не становятся
//! вызовом или индексом) — это новая инструкция.

use std::rc::Rc;

use crate::ast::*;
use crate::error::{ErrorKind, Pos, ScriptError};
use crate::lexer::{tokenize_in, TPart, Tok, Token};

/// Предел вложенности выражений и блоков: глубже — ошибка, а не переполнение стека.
const MAX_DEPTH: u32 = 120;

pub struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    file: &'a Rc<str>,
    source: &'a str,
    depth: u32,
    loops: u32,
    /// Внутри средней ветки `?:` двоеточие — разделитель, а не вызов метода `a:b()`.
    ternary: u32,
}

/// Разбирает весь файл.
pub fn parse(source: &str, file: &Rc<str>) -> Result<Vec<Stmt>, ScriptError> {
    let toks = tokenize_in(source, source, file, Pos::new(1, 1))?;
    let mut p = Parser { toks, i: 0, file, source, depth: 0, loops: 0, ternary: 0 };
    let mut out = Vec::new();
    while !p.check(&Tok::Eof) {
        p.statement(&mut out)?;
        p.after_statement()?;
    }
    Ok(out)
}

enum LuaEnd {
    End,
    Else,
    Elseif,
    Until,
    Catch,
}

impl<'a> Parser<'a> {
    // ---- служебное ----

    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }

    fn peek_at(&self, k: usize) -> &Tok {
        let j = (self.i + k).min(self.toks.len() - 1);
        &self.toks[j].tok
    }

    fn token(&self) -> &Token {
        &self.toks[self.i]
    }

    fn pos(&self) -> Pos {
        self.toks[self.i].pos
    }

    fn check(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.i].clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.check(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn err(&self, pos: Pos, msg: impl Into<String>) -> ScriptError {
        ScriptError::new(ErrorKind::Syntax, msg, self.file, self.source, pos)
    }

    fn unexpected(&self, wanted: &str) -> ScriptError {
        let found = self.peek().describe();
        self.err(self.pos(), format!("expected {wanted}, found {found}"))
    }

    fn expect(&mut self, t: &Tok, wanted: &str) -> Result<Token, ScriptError> {
        if self.check(t) {
            Ok(self.advance())
        } else {
            Err(self.unexpected(wanted))
        }
    }

    fn name(&mut self, what: &str) -> Result<(Rc<str>, Pos), ScriptError> {
        let pos = self.pos();
        match self.peek().clone() {
            Tok::Name(n) => {
                self.advance();
                Ok((n, pos))
            }
            t if !t.text().is_empty() && t.text().chars().all(|c| c.is_alphabetic()) => Err(self.err(
                pos,
                format!("expected {what}, found the keyword `{}` (keywords can't be used as names)", t.text()),
            )),
            _ => Err(self.unexpected(what)),
        }
    }

    /// Имя поля после `.`: можно и ключевое слово (`obj.end`).
    fn field_name(&mut self) -> Result<Rc<str>, ScriptError> {
        match self.peek().clone() {
            Tok::Name(n) => {
                self.advance();
                Ok(n)
            }
            t if !t.text().is_empty() && t.text().chars().all(|c| c.is_alphabetic()) => {
                self.advance();
                Ok(t.text().into())
            }
            _ => Err(self.unexpected("a field name after `.`")),
        }
    }

    fn enter(&mut self) -> Result<(), ScriptError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(self.pos(), format!("code is nested too deeply (more than {MAX_DEPTH} levels)")));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// После инструкции: необязательная `;`.
    fn after_statement(&mut self) -> Result<(), ScriptError> {
        while self.eat(&Tok::Semi) {}
        Ok(())
    }

    // ---- блоки ----

    /// `{ … }` — `{` уже на очереди.
    fn js_block(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        let open = self.expect(&Tok::LBrace, "`{`")?.pos;
        self.enter()?;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                Tok::RBrace => {
                    self.advance();
                    break;
                }
                Tok::Eof => {
                    return Err(self.err(
                        self.pos(),
                        format!("expected `}}` to close the `{{` opened at line {}", open.line),
                    ))
                }
                Tok::End | Tok::Elseif | Tok::Until => {
                    return Err(self.err(
                        self.pos(),
                        format!(
                            "unexpected {} — this block was opened with `{{` at line {}, close it with `}}`",
                            self.peek().describe(),
                            open.line
                        ),
                    ))
                }
                _ => {
                    self.statement(&mut out)?;
                    self.after_statement()?;
                }
            }
        }
        self.leave();
        Ok(out)
    }

    /// Lua-блок до `end` / `else` / `elseif` / `until` / `catch`.
    fn lua_block(&mut self, what: &str, open: Pos, allowed: &[Tok]) -> Result<(Vec<Stmt>, LuaEnd), ScriptError> {
        self.enter()?;
        let mut out = Vec::new();
        let end = loop {
            let t = self.peek().clone();
            let kind = match t {
                Tok::End => Some(LuaEnd::End),
                Tok::Else => Some(LuaEnd::Else),
                Tok::Elseif => Some(LuaEnd::Elseif),
                Tok::Until => Some(LuaEnd::Until),
                Tok::Catch => Some(LuaEnd::Catch),
                _ => None,
            };
            if let Some(kind) = kind {
                if allowed.contains(&t) {
                    self.advance();
                    break kind;
                }
                return Err(self.err(
                    self.pos(),
                    format!(
                        "unexpected {} inside the `{what}` opened at line {}",
                        t.describe(),
                        open.line
                    ),
                ));
            }
            if t == Tok::Eof || t == Tok::RBrace {
                let closer = allowed
                    .iter()
                    .map(|t| format!("`{}`", t.text()))
                    .collect::<Vec<_>>()
                    .join(" or ");
                let found = if t == Tok::Eof { String::new() } else { format!(", found {}", t.describe()) };
                return Err(self.err(
                    self.pos(),
                    format!("expected {closer} to close the `{what}` opened at line {}{found}", open.line),
                ));
            }
            self.statement(&mut out)?;
            self.after_statement()?;
        };
        self.leave();
        Ok((out, end))
    }

    /// Тело цикла/условия JS-формы: `{ … }` или одна инструкция.
    fn js_body(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        if self.check(&Tok::LBrace) {
            self.js_block()
        } else {
            self.enter()?;
            let mut out = Vec::new();
            if matches!(self.peek(), Tok::Eof | Tok::RBrace | Tok::End) {
                return Err(self.unexpected("a statement or `{`"));
            }
            self.statement(&mut out)?;
            self.leave();
            Ok(out)
        }
    }

    /// Тело цикла: `do … end`, `{ … }` или одна инструкция.
    fn loop_body(&mut self, what: &str, open: Pos) -> Result<Vec<Stmt>, ScriptError> {
        self.loops += 1;
        let body = if self.eat(&Tok::Do) {
            self.lua_block(what, open, &[Tok::End]).map(|b| b.0)
        } else {
            self.js_body()
        };
        self.loops -= 1;
        body
    }

    // ---- инструкции ----

    fn statement(&mut self, out: &mut Vec<Stmt>) -> Result<(), ScriptError> {
        let pos = self.pos();
        let stmt = match self.peek().clone() {
            Tok::Let | Tok::Var | Tok::Local | Tok::Const => {
                let constant = self.check(&Tok::Const);
                self.advance();
                if self.check(&Tok::Function) {
                    let fpos = self.advance().pos;
                    let (name, _) = self.name("a function name")?;
                    let func = self.function_rest(name.clone(), fpos, false)?;
                    StmtKind::Function { name, func }
                } else {
                    return self.declaration(out, pos, constant);
                }
            }
            Tok::Function => {
                if !matches!(self.peek_at(1), Tok::Name(_)) {
                    let e = self.expression()?;
                    return Err(self.err(e.pos, "this function is never used — give it a name: `function name() … end`"));
                }
                let fpos = self.advance().pos;
                let (name, npos) = self.name("a function name")?;
                if matches!(self.peek(), Tok::Dot | Tok::Colon) {
                    let mut target = Expr { kind: ExprKind::Name(name.clone()), pos: npos };
                    let mut method = false;
                    let mut last = name;
                    while matches!(self.peek(), Tok::Dot | Tok::Colon) {
                        if method {
                            return Err(self.err(self.pos(), "`:` must be the last part of a method name"));
                        }
                        method = self.advance().tok == Tok::Colon;
                        let field = self.field_name()?;
                        target = Expr { kind: ExprKind::Field(Box::new(target), field.clone()), pos: npos };
                        last = field;
                    }
                    let func = self.function_rest(last, fpos, method)?;
                    StmtKind::FieldFunction { target, func }
                } else {
                    let func = self.function_rest(name.clone(), fpos, false)?;
                    StmtKind::Function { name, func }
                }
            }
            Tok::If => self.if_statement()?,
            Tok::While => {
                self.advance();
                let cond = self.expression()?;
                let body = self.loop_body("while", pos)?;
                StmtKind::While { cond, body }
            }
            Tok::Do => {
                self.advance();
                if self.check(&Tok::LBrace) {
                    self.loops += 1;
                    let body = self.js_block()?;
                    self.loops -= 1;
                    self.expect(&Tok::While, "`while` after `do { … }`")?;
                    let cond = self.expression()?;
                    StmtKind::DoWhile { body, cond, until: false }
                } else {
                    let (body, _) = self.lua_block("do", pos, &[Tok::End])?;
                    StmtKind::Block(body)
                }
            }
            Tok::Repeat => {
                self.advance();
                self.loops += 1;
                let body = if self.check(&Tok::LBrace) {
                    let b = self.js_block()?;
                    self.expect(&Tok::Until, "`until` after `repeat { … }`")?;
                    b
                } else {
                    self.lua_block("repeat", pos, &[Tok::Until])?.0
                };
                self.loops -= 1;
                let cond = self.expression()?;
                StmtKind::DoWhile { body, cond, until: true }
            }
            Tok::For => self.for_statement()?,
            Tok::Return => {
                self.advance();
                let t = self.token();
                let value = if t.nl_before
                    || matches!(
                        t.tok,
                        Tok::Eof | Tok::End | Tok::RBrace | Tok::Semi | Tok::Else | Tok::Elseif | Tok::Until
                    ) {
                    None
                } else {
                    Some(self.expression()?)
                };
                StmtKind::Return(value)
            }
            Tok::Break | Tok::Continue => {
                let t = self.advance();
                if self.loops == 0 {
                    return Err(self.err(pos, format!("`{}` can only be used inside a loop", t.tok.text())));
                }
                if t.tok == Tok::Break {
                    StmtKind::Break
                } else {
                    StmtKind::Continue
                }
            }
            Tok::Try => self.try_statement()?,
            Tok::Throw => {
                self.advance();
                StmtKind::Throw(self.expression()?)
            }
            Tok::LBrace => StmtKind::Block(self.js_block()?),
            Tok::Semi => {
                self.advance();
                return Ok(());
            }
            _ => self.expression_statement()?,
        };
        out.push(Stmt { kind: stmt, pos });
        Ok(())
    }

    fn declaration(&mut self, out: &mut Vec<Stmt>, pos: Pos, constant: bool) -> Result<(), ScriptError> {
        let mut pos = pos;
        loop {
            let decl = self.decl_target()?;
            let mut values = Vec::new();
            let mut more = false;
            if self.eat(&Tok::Assign) {
                loop {
                    values.push(self.expression()?);
                    if self.check(&Tok::Comma) {
                        // `let a = 1, b = 2` — следующий объявляемый, а не ещё одно значение.
                        if matches!(self.peek_at(1), Tok::Name(_)) && matches!(self.peek_at(2), Tok::Assign) {
                            self.advance();
                            more = true;
                            break;
                        }
                        self.advance();
                        continue;
                    }
                    break;
                }
            } else if decl.pattern {
                return Err(self.unexpected("`=` after the `[…]` pattern"));
            } else if constant {
                return Err(self.err(decl.names[0].1, format!("const `{}` needs a value: `const {} = …`", decl.names[0].0, decl.names[0].0)));
            }
            out.push(Stmt { kind: StmtKind::Let { decl, values, constant }, pos });
            if !more {
                return Ok(());
            }
            pos = self.pos();
        }
    }

    /// Имена после `let` / `local`: `a`, `a, b` или `[a, b]`.
    fn decl_target(&mut self) -> Result<Decl, ScriptError> {
        if self.eat(&Tok::LBracket) {
            let mut names = Vec::new();
            loop {
                if self.check(&Tok::RBracket) {
                    break;
                }
                names.push(self.name("a variable name")?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RBracket, "`]` to close the pattern")?;
            if names.is_empty() {
                return Err(self.unexpected("at least one name in `[…]`"));
            }
            return Ok(Decl { names, pattern: true });
        }
        let mut names = vec![self.name("a variable name")?];
        // `let a = 1, b = 2` разбирается в declaration(); здесь только `local a, b`.
        while self.check(&Tok::Comma) && matches!(self.peek_at(1), Tok::Name(_)) {
            self.advance();
            names.push(self.name("a variable name")?);
        }
        Ok(Decl { names, pattern: false })
    }

    fn if_statement(&mut self) -> Result<StmtKind, ScriptError> {
        let open = self.expect(&Tok::If, "`if`")?.pos;
        let mut arms = Vec::new();
        let cond = self.expression()?;
        if self.eat(&Tok::Then) {
            // Lua: if … then … elseif … else … end
            let mut cond = cond;
            loop {
                let (body, end) = self.lua_block("if", open, &[Tok::End, Tok::Else, Tok::Elseif])?;
                arms.push((cond, body));
                match end {
                    LuaEnd::Elseif => {
                        cond = self.expression()?;
                        self.expect(&Tok::Then, "`then` after the `elseif` condition")?;
                    }
                    LuaEnd::Else => {
                        let (other, _) = self.lua_block("if", open, &[Tok::End])?;
                        return Ok(StmtKind::If { arms, otherwise: Some(other) });
                    }
                    _ => return Ok(StmtKind::If { arms, otherwise: None }),
                }
            }
        }
        // JavaScript: if (…) { … } else if (…) { … } else { … }
        let body = self.js_body()?;
        arms.push((cond, body));
        let mut otherwise = None;
        while self.check(&Tok::Else) {
            self.advance();
            if self.check(&Tok::If) {
                self.advance();
                let c = self.expression()?;
                if self.check(&Tok::Then) {
                    return Err(self.err(self.pos(), "`then` after `else if` — in the `{ }` form write `else if (…) { … }`, in the Lua form use `elseif … then`"));
                }
                let b = self.js_body()?;
                arms.push((c, b));
            } else {
                otherwise = Some(self.js_body()?);
                break;
            }
        }
        if self.check(&Tok::Elseif) {
            return Err(self.err(self.pos(), "`elseif` belongs to the Lua form `if … then … end`; after `{ }` write `else if`"));
        }
        Ok(StmtKind::If { arms, otherwise })
    }

    fn for_statement(&mut self) -> Result<StmtKind, ScriptError> {
        let open = self.expect(&Tok::For, "`for`")?.pos;
        if self.check(&Tok::LParen) {
            // JS: for (let x of …) / for (init; cond; step)
            let save = self.i;
            self.advance();
            let had_kw = matches!(self.peek(), Tok::Let | Tok::Const | Tok::Var | Tok::Local);
            if had_kw {
                self.advance();
            }
            if had_kw || matches!(self.peek(), Tok::Name(_)) {
                let try_decl = self.i;
                if let Ok(decl) = self.decl_target() {
                    if matches!(self.peek(), Tok::Of | Tok::In) {
                        let mode = if self.advance().tok == Tok::Of { IterMode::Of } else { IterMode::In };
                        let iter = self.expression()?;
                        self.expect(&Tok::RParen, "`)` after the `for` header")?;
                        let body = self.loop_body("for", open)?;
                        return Ok(StmtKind::ForIn { decl, iter, mode, body });
                    }
                }
                self.i = try_decl;
            }
            // C-подобный for.
            self.i = save;
            self.advance();
            let init = if self.check(&Tok::Semi) {
                None
            } else {
                let mut v = Vec::new();
                self.statement(&mut v)?;
                if v.len() != 1 {
                    return Err(self.err(open, "the first part of `for (…; …; …)` must be a single statement"));
                }
                Some(Box::new(v.pop().unwrap_or(Stmt { kind: StmtKind::Block(Vec::new()), pos: open })))
            };
            self.expect(&Tok::Semi, "`;` after the first part of `for (…; …; …)`")?;
            let cond = if self.check(&Tok::Semi) { None } else { Some(self.expression()?) };
            self.expect(&Tok::Semi, "`;` after the condition of `for (…; …; …)`")?;
            let step = if self.check(&Tok::RParen) {
                None
            } else {
                let mut v = Vec::new();
                self.statement(&mut v)?;
                v.pop().map(Box::new)
            };
            self.expect(&Tok::RParen, "`)` to close the `for (…; …; …)` header")?;
            let body = self.loop_body("for", open)?;
            return Ok(StmtKind::CFor { init, cond, step, body });
        }
        // Lua: for i = a, b[, step] do … end   |   for k, v in pairs(m) do … end
        if matches!(self.peek(), Tok::Name(_)) && matches!(self.peek_at(1), Tok::Assign) {
            let var = self.name("a loop variable")?;
            self.advance();
            let start = self.expression()?;
            self.expect(&Tok::Comma, "`,` between the start and the end of the numeric `for`")?;
            let end = self.expression()?;
            let step = if self.eat(&Tok::Comma) { Some(self.expression()?) } else { None };
            let body = self.loop_body("for", open)?;
            return Ok(StmtKind::NumFor { var, start, end, step, body });
        }
        if matches!(self.peek(), Tok::Let | Tok::Const | Tok::Var | Tok::Local) {
            self.advance();
        }
        let decl = self.decl_target()?;
        let mode = match self.peek() {
            Tok::In => IterMode::In,
            Tok::Of => IterMode::Of,
            _ => return Err(self.unexpected("`in`, `of` or `=` in the `for` header")),
        };
        self.advance();
        let iter = self.expression()?;
        let body = self.loop_body("for", open)?;
        Ok(StmtKind::ForIn { decl, iter, mode, body })
    }

    fn try_statement(&mut self) -> Result<StmtKind, ScriptError> {
        let open = self.expect(&Tok::Try, "`try`")?.pos;
        let lua = !self.check(&Tok::LBrace);
        let body = if lua {
            self.lua_block("try", open, &[Tok::Catch])?.0
        } else {
            let b = self.js_block()?;
            self.expect(&Tok::Catch, "`catch` after the `try` block")?;
            b
        };
        let name = if self.eat(&Tok::LParen) {
            let n = self.name("the error variable name")?;
            self.expect(&Tok::RParen, "`)` after the error variable")?;
            Some(n)
        } else if matches!(self.peek(), Tok::Name(_)) && !self.token().nl_before {
            Some(self.name("the error variable name")?)
        } else {
            None
        };
        let handler = if self.check(&Tok::LBrace) {
            self.js_block()?
        } else {
            self.lua_block("try", open, &[Tok::End])?.0
        };
        Ok(StmtKind::Try { body, name, handler })
    }

    fn expression_statement(&mut self) -> Result<StmtKind, ScriptError> {
        let start = self.pos();
        let first = self.expression()?;
        if matches!(self.peek(), Tok::Comma) {
            let mut targets = vec![first];
            while self.eat(&Tok::Comma) {
                targets.push(self.postfix_expr()?);
            }
            self.expect(&Tok::Assign, "`=` after the list of assignment targets")?;
            for t in &targets {
                self.check_target(t)?;
            }
            let values = self.expression_list()?;
            return Ok(StmtKind::Assign { targets, values });
        }
        let op = match self.peek() {
            Tok::Assign => {
                self.advance();
                self.check_target(&first)?;
                let values = self.expression_list()?;
                return Ok(StmtKind::Assign { targets: vec![first], values });
            }
            Tok::PlusEq => BinOp::Add,
            Tok::MinusEq => BinOp::Sub,
            Tok::StarEq => BinOp::Mul,
            Tok::SlashEq => BinOp::Div,
            Tok::PercentEq => BinOp::Mod,
            Tok::DotDotEq => BinOp::Concat,
            Tok::PlusPlus => {
                let pos = self.advance().pos;
                self.check_target(&first)?;
                return Ok(StmtKind::Compound { target: first, op: BinOp::Add, value: Expr { kind: ExprKind::Num(1.0), pos } });
            }
            _ => {
                if matches!(first.kind, ExprKind::Call(..) | ExprKind::Method { .. }) {
                    return Ok(StmtKind::Expr(first));
                }
                let last = &self.toks[self.i.saturating_sub(1)];
                let hint = if last.dashes_after {
                    Some(format!(
                        "`--` starts a comment in Scenario, so `{0}--` is just `{0}`; write `{0} -= 1`",
                        first.describe().unwrap_or_else(|| "x".into())
                    ))
                } else if let ExprKind::Binary(BinOp::Eq, ..) = first.kind {
                    Some("to assign a value use a single `=`".into())
                } else {
                    None
                };
                return Err(self.err(start, "this line computes a value but does nothing with it").with_hint(hint));
            }
        };
        self.advance();
        self.check_target(&first)?;
        let value = self.expression()?;
        Ok(StmtKind::Compound { target: first, op, value })
    }

    fn check_target(&self, e: &Expr) -> Result<(), ScriptError> {
        match e.kind {
            ExprKind::Name(_) | ExprKind::Field(..) | ExprKind::Index(..) => Ok(()),
            _ => Err(self.err(e.pos, "can't assign to this — only to a variable, `a.field` or `a[index]`")),
        }
    }

    fn expression_list(&mut self) -> Result<Vec<Expr>, ScriptError> {
        let mut v = vec![self.expression()?];
        while self.eat(&Tok::Comma) {
            v.push(self.expression()?);
        }
        Ok(v)
    }

    // ---- функции ----

    /// Параметры и тело после имени функции; `function` уже съеден.
    fn function_rest(&mut self, name: Rc<str>, open: Pos, method: bool) -> Result<Rc<FuncDef>, ScriptError> {
        self.expect(&Tok::LParen, "`(` to start the parameter list")?;
        let (mut params, rest) = self.params()?;
        if method {
            params.insert(0, "self".into());
        }
        let saved_loops = std::mem::replace(&mut self.loops, 0);
        let saved_ternary = std::mem::replace(&mut self.ternary, 0);
        let body = if self.check(&Tok::LBrace) {
            self.js_block()
        } else {
            self.lua_block("function", open, &[Tok::End]).map(|b| b.0)
        };
        self.loops = saved_loops;
        self.ternary = saved_ternary;
        Ok(Rc::new(FuncDef { name, params, rest, body: body?, pos: open }))
    }

    /// Список параметров до `)` включительно.
    fn params(&mut self) -> Result<(Vec<Rc<str>>, bool), ScriptError> {
        let mut params = Vec::new();
        let mut rest = false;
        loop {
            if self.eat(&Tok::RParen) {
                break;
            }
            if self.eat(&Tok::Ellipsis) {
                params.push(self.name("the rest parameter name after `...`")?.0);
                rest = true;
                self.expect(&Tok::RParen, "`)` — the `...rest` parameter must be the last")?;
                break;
            }
            let (n, p) = self.name("a parameter name")?;
            if params.contains(&n) {
                return Err(self.err(p, format!("parameter `{n}` is listed twice")));
            }
            params.push(n);
            if !self.eat(&Tok::Comma) {
                self.expect(&Tok::RParen, "`,` or `)` in the parameter list")?;
                break;
            }
        }
        Ok((params, rest))
    }

    fn arrow_body(&mut self, params: Vec<Rc<str>>, rest: bool, open: Pos) -> Result<Expr, ScriptError> {
        let saved_loops = std::mem::replace(&mut self.loops, 0);
        let saved_ternary = std::mem::replace(&mut self.ternary, 0);
        let body = if self.check(&Tok::LBrace) {
            self.js_block()
        } else {
            self.expression().map(|e| {
                let pos = e.pos;
                vec![Stmt { kind: StmtKind::Return(Some(e)), pos }]
            })
        };
        self.loops = saved_loops;
        self.ternary = saved_ternary;
        let func = FuncDef { name: "<arrow>".into(), params, rest, body: body?, pos: open };
        Ok(Expr { kind: ExprKind::Func(Rc::new(func)), pos: open })
    }

    // ---- выражения ----

    pub fn expression(&mut self) -> Result<Expr, ScriptError> {
        self.enter()?;
        let r = self.ternary_expr();
        self.leave();
        r
    }

    fn ternary_expr(&mut self) -> Result<Expr, ScriptError> {
        let cond = self.binary(1)?;
        if self.check(&Tok::Question) {
            let pos = self.advance().pos;
            self.ternary += 1;
            let a = self.expression();
            self.ternary -= 1;
            let a = a?;
            self.expect(&Tok::Colon, "`:` in `condition ? a : b`")?;
            let b = self.expression()?;
            return Ok(Expr { kind: ExprKind::Ternary(Box::new(cond), Box::new(a), Box::new(b)), pos });
        }
        Ok(cond)
    }

    fn binop(&self) -> Option<(u8, bool)> {
        // (уровень, правоассоциативный)
        Some(match self.peek() {
            Tok::Or | Tok::OrOr | Tok::QQ => (1, false),
            Tok::And | Tok::AndAnd => (2, false),
            Tok::EqEq | Tok::EqEqEq | Tok::BangEq | Tok::BangEqEq | Tok::TildeEq => (3, false),
            Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge => (4, false),
            Tok::DotDot => (5, true),
            Tok::Plus | Tok::Minus => (6, false),
            Tok::Star | Tok::Slash | Tok::Percent => (7, false),
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8) -> Result<Expr, ScriptError> {
        let mut left = self.unary()?;
        while let Some((level, right_assoc)) = self.binop() {
            if level < min {
                break;
            }
            let t = self.advance();
            let next = if right_assoc { level } else { level + 1 };
            self.enter()?;
            let right = self.binary(next);
            self.leave();
            let right = right?;
            let (l, r) = (Box::new(left), Box::new(right));
            let kind = match t.tok {
                Tok::Or | Tok::OrOr => ExprKind::Or(l, r),
                Tok::QQ => ExprKind::Nullish(l, r),
                Tok::And | Tok::AndAnd => ExprKind::And(l, r),
                Tok::EqEq | Tok::EqEqEq => ExprKind::Binary(BinOp::Eq, l, r),
                Tok::BangEq | Tok::BangEqEq | Tok::TildeEq => ExprKind::Binary(BinOp::Ne, l, r),
                Tok::Lt => ExprKind::Binary(BinOp::Lt, l, r),
                Tok::Le => ExprKind::Binary(BinOp::Le, l, r),
                Tok::Gt => ExprKind::Binary(BinOp::Gt, l, r),
                Tok::Ge => ExprKind::Binary(BinOp::Ge, l, r),
                Tok::DotDot => ExprKind::Binary(BinOp::Concat, l, r),
                Tok::Plus => ExprKind::Binary(BinOp::Add, l, r),
                Tok::Minus => ExprKind::Binary(BinOp::Sub, l, r),
                Tok::Star => ExprKind::Binary(BinOp::Mul, l, r),
                Tok::Slash => ExprKind::Binary(BinOp::Div, l, r),
                _ => ExprKind::Binary(BinOp::Mod, l, r),
            };
            left = Expr { kind, pos: t.pos };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ScriptError> {
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Bang | Tok::Not => UnOp::Not,
            Tok::Hash => UnOp::Len,
            Tok::Typeof => UnOp::Typeof,
            Tok::Plus => {
                return Err(self.err(self.pos(), "unary `+` is not supported — use `tonumber(x)` to convert"));
            }
            _ => return self.power(),
        };
        let pos = self.advance().pos;
        self.enter()?;
        let e = self.unary();
        self.leave();
        let e = e?;
        // Свёртка `-5` в число сразу.
        if let (UnOp::Neg, ExprKind::Num(n)) = (op, &e.kind) {
            return Ok(Expr { kind: ExprKind::Num(-n), pos });
        }
        Ok(Expr { kind: ExprKind::Unary(op, Box::new(e)), pos })
    }

    fn power(&mut self) -> Result<Expr, ScriptError> {
        let base = self.postfix_expr()?;
        if matches!(self.peek(), Tok::Caret | Tok::StarStar) {
            let pos = self.advance().pos;
            self.enter()?;
            let exp = self.unary();
            self.leave();
            return Ok(Expr { kind: ExprKind::Binary(BinOp::Pow, Box::new(base), Box::new(exp?)), pos });
        }
        Ok(base)
    }

    fn args(&mut self) -> Result<Vec<Arg>, ScriptError> {
        let open = self.expect(&Tok::LParen, "`(`")?.pos;
        let saved = std::mem::replace(&mut self.ternary, 0);
        let r = self.arg_list(&Tok::RParen, open, "(");
        self.ternary = saved;
        r
    }

    fn arg_list(&mut self, close: &Tok, open: Pos, opener: &str) -> Result<Vec<Arg>, ScriptError> {
        let mut args = Vec::new();
        loop {
            if self.eat(close) {
                break;
            }
            if self.check(&Tok::Eof) {
                return Err(self.err(
                    self.pos(),
                    format!("expected `{}` to close the `{opener}` opened at line {}", close.text(), open.line),
                ));
            }
            let spread = self.eat(&Tok::Ellipsis);
            let expr = self.expression()?;
            args.push(Arg { expr, spread });
            if !self.eat(&Tok::Comma) {
                if self.check(close) {
                    self.advance();
                    break;
                }
                if self.check(&Tok::Eof) {
                    return Err(self.err(
                        self.pos(),
                        format!("expected `{}` to close the `{opener}` opened at line {}", close.text(), open.line),
                    ));
                }
                return Err(self.unexpected(&format!("`,` or `{}`", close.text())));
            }
        }
        Ok(args)
    }

    fn postfix_expr(&mut self) -> Result<Expr, ScriptError> {
        let mut e = self.primary()?;
        loop {
            let t = self.token();
            match &t.tok {
                Tok::Dot => {
                    self.advance();
                    let pos = self.pos();
                    let name = self.field_name()?;
                    if self.check(&Tok::LParen) && !self.token().nl_before {
                        let args = self.args()?;
                        e = Expr { kind: ExprKind::Method { obj: Box::new(e), name, args, colon: false }, pos };
                    } else {
                        e = Expr { kind: ExprKind::Field(Box::new(e), name), pos };
                    }
                }
                Tok::Colon
                    if self.ternary == 0
                        && matches!(self.peek_at(1), Tok::Name(_))
                        && matches!(self.peek_at(2), Tok::LParen) =>
                {
                    self.advance();
                    let pos = self.pos();
                    let name = self.field_name()?;
                    let args = self.args()?;
                    e = Expr { kind: ExprKind::Method { obj: Box::new(e), name, args, colon: true }, pos };
                }
                Tok::LBracket if !t.nl_before => {
                    let open = self.advance().pos;
                    let saved = std::mem::replace(&mut self.ternary, 0);
                    let key = self.expression();
                    self.ternary = saved;
                    let key = key?;
                    if !self.eat(&Tok::RBracket) {
                        return Err(self.err(
                            self.pos(),
                            format!("expected `]` to close the `[` opened at line {}, found {}", open.line, self.peek().describe()),
                        ));
                    }
                    e = Expr { kind: ExprKind::Index(Box::new(e), Box::new(key)), pos: open };
                }
                Tok::LParen if !t.nl_before => {
                    let pos = e.pos;
                    let args = self.args()?;
                    e = Expr { kind: ExprKind::Call(Box::new(e), args), pos };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    /// После `(` на текущей позиции — это параметры стрелочной функции?
    fn is_arrow_params(&self) -> bool {
        let mut depth = 0i32;
        let mut j = self.i;
        while j < self.toks.len() {
            match self.toks[j].tok {
                Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.toks.get(j + 1).map(|t| &t.tok), Some(Tok::Arrow));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            j += 1;
        }
        false
    }

    fn primary(&mut self) -> Result<Expr, ScriptError> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Num(n) => {
                self.advance();
                ExprKind::Num(n)
            }
            Tok::Str(s) => {
                self.advance();
                ExprKind::Str(s)
            }
            Tok::Template(parts) => {
                self.advance();
                let mut pieces = Vec::new();
                for part in parts.iter() {
                    match part {
                        TPart::Lit(s) => pieces.push(TemplatePiece::Lit(s.clone())),
                        TPart::Expr(src, at) => pieces.push(TemplatePiece::Expr(self.sub_expression(src, *at)?)),
                    }
                }
                ExprKind::Template(pieces)
            }
            Tok::True => {
                self.advance();
                ExprKind::Bool(true)
            }
            Tok::False => {
                self.advance();
                ExprKind::Bool(false)
            }
            Tok::Nil => {
                self.advance();
                ExprKind::Nil
            }
            Tok::Name(n) => {
                self.advance();
                if self.check(&Tok::Arrow) {
                    self.advance();
                    return self.arrow_body(vec![n], false, pos);
                }
                ExprKind::Name(n)
            }
            Tok::LParen => {
                if self.is_arrow_params() {
                    self.advance();
                    let (params, rest) = self.params()?;
                    self.expect(&Tok::Arrow, "`=>`")?;
                    return self.arrow_body(params, rest, pos);
                }
                self.advance();
                let saved = std::mem::replace(&mut self.ternary, 0);
                let e = self.expression();
                self.ternary = saved;
                let e = e?;
                if !self.eat(&Tok::RParen) {
                    return Err(self.err(
                        self.pos(),
                        format!("expected `)` to close the `(` opened at line {}, found {}", pos.line, self.peek().describe()),
                    ));
                }
                return Ok(e);
            }
            Tok::LBracket => {
                self.advance();
                let saved = std::mem::replace(&mut self.ternary, 0);
                let items = self.arg_list(&Tok::RBracket, pos, "[");
                self.ternary = saved;
                ExprKind::List(items?)
            }
            Tok::LBrace => {
                let saved = std::mem::replace(&mut self.ternary, 0);
                let r = self.table();
                self.ternary = saved;
                r?
            }
            Tok::Function => {
                self.advance();
                let name: Rc<str> = if let Tok::Name(n) = self.peek().clone() {
                    self.advance();
                    n
                } else {
                    "<anonymous>".into()
                };
                ExprKind::Func(self.function_rest(name, pos, false)?)
            }
            Tok::Ellipsis => {
                return Err(self.err(pos, "`...` can only be used in a call `f(...list)`, a list `[...list]` or a parameter list"))
            }
            _ => return Err(self.unexpected("an expression")),
        };
        Ok(Expr { kind, pos })
    }

    fn sub_expression(&mut self, src: &str, at: Pos) -> Result<Expr, ScriptError> {
        let toks = tokenize_in(src, self.source, self.file, at)?;
        let mut p = Parser { toks, i: 0, file: self.file, source: self.source, depth: self.depth, loops: 0, ternary: 0 };
        let e = p.expression()?;
        if !p.check(&Tok::Eof) {
            return Err(p.unexpected("`}` to end the `${…}` expression"));
        }
        Ok(e)
    }

    /// `{ … }` в выражении: словарь (`{a = 1, b: 2, ["k"] = 3}`) или список (`{1, 2, 3}`).
    fn table(&mut self) -> Result<ExprKind, ScriptError> {
        let open = self.expect(&Tok::LBrace, "`{`")?.pos;
        self.enter()?;
        let mut keyed: Vec<(MapKey, Expr)> = Vec::new();
        let mut items: Vec<Arg> = Vec::new();
        loop {
            if self.eat(&Tok::RBrace) {
                break;
            }
            if self.check(&Tok::Eof) {
                return Err(self.err(self.pos(), format!("expected `}}` to close the `{{` opened at line {}", open.line)));
            }
            let entry_pos = self.pos();
            let key = match self.peek().clone() {
                Tok::Name(n) if matches!(self.peek_at(1), Tok::Assign | Tok::Colon) => {
                    self.advance();
                    self.advance();
                    Some(MapKey::Name(n))
                }
                Tok::Str(s) if matches!(self.peek_at(1), Tok::Assign | Tok::Colon) => {
                    self.advance();
                    self.advance();
                    Some(MapKey::Name(s))
                }
                Tok::Num(n) if matches!(self.peek_at(1), Tok::Assign | Tok::Colon) => {
                    self.advance();
                    self.advance();
                    Some(MapKey::Expr(Expr { kind: ExprKind::Num(n), pos: entry_pos }))
                }
                t if !t.text().is_empty()
                    && t.text().chars().all(|c| c.is_alphabetic())
                    && matches!(self.peek_at(1), Tok::Assign | Tok::Colon) =>
                {
                    self.advance();
                    self.advance();
                    Some(MapKey::Name(t.text().into()))
                }
                Tok::LBracket if self.bracket_key() => {
                    self.advance();
                    let k = self.expression()?;
                    self.expect(&Tok::RBracket, "`]` after the key")?;
                    self.advance(); // `=` или `:`
                    Some(MapKey::Expr(k))
                }
                _ => None,
            };
            match key {
                Some(k) => keyed.push((k, self.expression()?)),
                None => {
                    let spread = self.eat(&Tok::Ellipsis);
                    items.push(Arg { expr: self.expression()?, spread });
                }
            }
            if !keyed.is_empty() && !items.is_empty() {
                self.leave();
                return Err(self.err(
                    entry_pos,
                    "a `{ }` literal can't mix `key = value` entries with plain items — use `[ … ]` for a list",
                ));
            }
            if !(self.eat(&Tok::Comma) || self.eat(&Tok::Semi)) {
                if self.eat(&Tok::RBrace) {
                    break;
                }
                self.leave();
                return Err(self.unexpected("`,` or `}` in the `{ }` literal"));
            }
        }
        self.leave();
        if !items.is_empty() {
            Ok(ExprKind::List(items))
        } else {
            Ok(ExprKind::Map(keyed))
        }
    }

    /// `[` в словаре начинает ключ `[expr] = …`, а не элемент-список?
    fn bracket_key(&self) -> bool {
        let mut depth = 0i32;
        let mut j = self.i;
        while j < self.toks.len() {
            match self.toks[j].tok {
                Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.toks.get(j + 1).map(|t| &t.tok), Some(Tok::Assign | Tok::Colon));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            j += 1;
        }
        false
    }
}
