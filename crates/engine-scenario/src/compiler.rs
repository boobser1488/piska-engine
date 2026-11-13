//! Компилятор: дерево → байткод.
//!
//! Переменные верхнего уровня файла — глобальные переменные экземпляра скрипта (слоты
//! модуля), всё остальное — слоты кадра. Переменные, которые могут быть захвачены
//! вложенными функциями, хранятся в ячейках (оценка консервативная: по имени).
//! Объявления `function` поднимаются в начало своего блока (как в JavaScript).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::*;
use crate::bytecode::{Op, Proto, UpSrc};
use crate::error::{ErrorKind, Pos, ScriptError};
use crate::value::Value;

/// Скомпилированный скрипт: можно запускать много раз и в разных ВМ.
pub struct Script {
    pub name: Rc<str>,
    pub source: Rc<str>,
    pub(crate) main: Rc<Proto>,
    pub(crate) globals: Rc<[Rc<str>]>,
    pub(crate) global_info: Vec<GlobalInfo>,
}

#[derive(Clone, Debug)]
pub(crate) struct GlobalInfo {
    pub declared: bool,
    pub first_use: Pos,
}

impl Script {
    /// Компилирует исходник. `name` — имя файла для сообщений (`scenarios/intro.scenario`).
    pub fn compile(name: &str, source: &str) -> Result<Rc<Script>, ScriptError> {
        compile_script(name, source, false)
    }

    /// Имена переменных верхнего уровня, объявленных в скрипте.
    pub fn declared_globals(&self) -> Vec<&str> {
        self.globals
            .iter()
            .zip(&self.global_info)
            .filter(|(_, i)| i.declared)
            .map(|(n, _)| &**n)
            .collect()
    }

    /// Имена, которые скрипт берёт снаружи (встроенные функции и функции хоста).
    pub fn free_names(&self) -> Vec<&str> {
        self.globals
            .iter()
            .zip(&self.global_info)
            .filter(|(_, i)| !i.declared)
            .map(|(n, _)| &**n)
            .collect()
    }

    /// Число инструкций байткода (для отладки и тестов).
    pub fn code_size(&self) -> usize {
        fn count(p: &Proto) -> usize {
            p.code.len() + p.protos.iter().map(|c| count(c)).sum::<usize>()
        }
        count(&self.main)
    }
}

pub(crate) fn compile_script(name: &str, source: &str, internal: bool) -> Result<Rc<Script>, ScriptError> {
    let file: Rc<str> = name.into();
    let src: Rc<str> = source.into();
    let ast = crate::parser::parse(source, &file)?;
    let mut c = Compiler {
        file: file.clone(),
        source: src.clone(),
        funcs: Vec::new(),
        globals: Vec::new(),
        global_index: HashMap::new(),
        global_info: Vec::new(),
        global_const: HashSet::new(),
        internal,
    };
    // Объявления верхнего уровня — глобальные переменные экземпляра.
    for stmt in &ast {
        match &stmt.kind {
            StmtKind::Let { decl, constant, .. } => {
                for (n, p) in &decl.names {
                    c.declare_global(n, *p, *constant)?;
                }
            }
            StmtKind::Function { name, func } => c.declare_global(name, func.pos, false)?,
            _ => {}
        }
    }
    let main_def = FuncDef { name: "<main>".into(), params: Vec::new(), rest: false, body: ast, pos: Pos::new(1, 1) };
    let main = c.function(&main_def, None, true)?;
    Ok(Rc::new(Script {
        name: file,
        source: src,
        main,
        globals: c.globals.into(),
        global_info: c.global_info,
    }))
}

#[derive(Clone)]
struct Local {
    name: Rc<str>,
    slot: u16,
    cell: bool,
    constant: bool,
    initialized: bool,
    decl: Pos,
}

#[derive(Default)]
struct LoopCtx {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    try_depth: usize,
}

struct FuncState {
    code: Vec<Op>,
    pos: Vec<Pos>,
    consts: Vec<Value>,
    const_strs: HashMap<Rc<str>, u32>,
    const_nums: HashMap<u64, u32>,
    protos: Vec<Rc<Proto>>,
    upvals: Vec<(UpSrc, Rc<str>, bool)>,
    notes: HashMap<u32, Rc<str>>,
    scopes: Vec<Vec<Local>>,
    next_slot: u16,
    nslots: u16,
    captured: HashSet<Rc<str>>,
    loops: Vec<LoopCtx>,
    try_depth: usize,
    is_main: bool,
}

enum Var {
    Local { slot: u16, cell: bool, constant: bool },
    Up { index: u16, constant: bool },
    Global(u32),
}

/// Куда записать изменённый вектор (`a.pos.x = 1` → обратно в `a.pos`).
enum WriteBack {
    None,
    Var(Rc<str>, Pos),
    Field(Rc<str>),
    Index,
}

struct Compiler {
    file: Rc<str>,
    source: Rc<str>,
    funcs: Vec<FuncState>,
    globals: Vec<Rc<str>>,
    global_index: HashMap<Rc<str>, u32>,
    global_info: Vec<GlobalInfo>,
    global_const: HashSet<Rc<str>>,
    internal: bool,
}

type R<T> = Result<T, ScriptError>;

impl Compiler {
    fn err(&self, pos: Pos, msg: impl Into<String>) -> ScriptError {
        ScriptError::new(ErrorKind::Syntax, msg, &self.file, &self.source, pos)
    }

    fn fs(&mut self) -> &mut FuncState {
        self.funcs.last_mut().expect("function state")
    }

    fn declare_global(&mut self, name: &Rc<str>, pos: Pos, constant: bool) -> R<()> {
        if let Some(&i) = self.global_index.get(name) {
            if self.global_info[i as usize].declared {
                return Err(self.err(
                    pos,
                    format!(
                        "`{name}` is already declared on line {} — use `{name} = …` to change it",
                        self.global_info[i as usize].first_use.line
                    ),
                ));
            }
        }
        let i = self.global_slot(name, pos);
        self.global_info[i as usize].declared = true;
        self.global_info[i as usize].first_use = pos;
        if constant {
            self.global_const.insert(name.clone());
        }
        Ok(())
    }

    fn global_slot(&mut self, name: &Rc<str>, pos: Pos) -> u32 {
        if let Some(&i) = self.global_index.get(name) {
            return i;
        }
        let i = self.globals.len() as u32;
        self.globals.push(name.clone());
        self.global_index.insert(name.clone(), i);
        self.global_info.push(GlobalInfo { declared: false, first_use: pos });
        i
    }

    // ---- выдача кода ----

    fn emit(&mut self, op: Op, pos: Pos) -> usize {
        let fs = self.fs();
        fs.code.push(op);
        fs.pos.push(pos);
        fs.code.len() - 1
    }

    fn note(&mut self, at: usize, desc: Option<String>) {
        if let Some(d) = desc {
            self.fs().notes.insert(at as u32, d.into());
        }
    }

    fn here(&mut self) -> usize {
        self.fs().code.len()
    }

    /// Ставит цель перехода инструкции `at` на текущее место.
    fn patch(&mut self, at: usize) {
        let target = self.here();
        self.patch_to(at, target);
    }

    fn patch_to(&mut self, at: usize, target: usize) {
        let off = target as i32 - (at as i32 + 1);
        let fs = self.fs();
        fs.code[at] = match fs.code[at] {
            Op::Jump(_) => Op::Jump(off),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(off),
            Op::AndJump(_) => Op::AndJump(off),
            Op::OrJump(_) => Op::OrJump(off),
            Op::NilJump(_) => Op::NilJump(off),
            Op::TryBegin(_) => Op::TryBegin(off),
            other => other,
        };
    }

    fn jump_back(&mut self, target: usize, pos: Pos) {
        let at = self.here();
        self.emit(Op::Jump(target as i32 - (at as i32 + 1)), pos);
    }

    fn const_str(&mut self, s: &Rc<str>) -> u32 {
        let fs = self.fs();
        if let Some(&i) = fs.const_strs.get(s) {
            return i;
        }
        let i = fs.consts.len() as u32;
        fs.consts.push(Value::Str(s.clone()));
        fs.const_strs.insert(s.clone(), i);
        i
    }

    fn const_num(&mut self, n: f64) -> u32 {
        let fs = self.fs();
        if let Some(&i) = fs.const_nums.get(&n.to_bits()) {
            return i;
        }
        let i = fs.consts.len() as u32;
        fs.consts.push(Value::Num(n));
        fs.const_nums.insert(n.to_bits(), i);
        i
    }

    fn push_num(&mut self, n: f64, pos: Pos) {
        if n == n.trunc() && n.abs() < 1e9 && !(n == 0.0 && n.is_sign_negative()) {
            self.emit(Op::Int(n as i32), pos);
        } else {
            let k = self.const_num(n);
            self.emit(Op::Const(k), pos);
        }
    }

    // ---- функции и области видимости ----

    fn function(&mut self, def: &FuncDef, name_hint: Option<&Rc<str>>, is_main: bool) -> R<Rc<Proto>> {
        let mut captured = HashSet::new();
        for s in &def.body {
            names_in_nested_stmt(s, false, &mut captured);
        }
        let name = match name_hint {
            Some(n) if def.name.starts_with('<') && !is_main => n.clone(),
            _ => def.name.clone(),
        };
        self.funcs.push(FuncState {
            code: Vec::new(),
            pos: Vec::new(),
            consts: Vec::new(),
            const_strs: HashMap::new(),
            const_nums: HashMap::new(),
            protos: Vec::new(),
            upvals: Vec::new(),
            notes: HashMap::new(),
            scopes: vec![Vec::new()],
            next_slot: 0,
            nslots: 0,
            captured,
            loops: Vec::new(),
            try_depth: 0,
            is_main,
        });
        for p in &def.params {
            let slot = self.add_local(p.clone(), def.pos, false)?;
            let cell = self.fs().scopes.last().and_then(|s| s.last()).map(|l| l.cell).unwrap_or(false);
            if let Some(l) = self.fs().scopes.last_mut().and_then(|s| s.last_mut()) {
                l.initialized = true;
            }
            if cell {
                self.emit(Op::GetLocal(slot), def.pos);
                self.emit(Op::NewCell(slot), def.pos);
                self.emit(Op::SetCell(slot), def.pos);
            }
        }
        let result = self.block_body(&def.body, !is_main);
        if let Err(e) = result {
            self.funcs.pop();
            return Err(e);
        }
        let end = Pos::new(def.body.last().map(|s| s.pos.line).unwrap_or(def.pos.line), 1);
        self.emit(Op::ReturnNil, end);
        let fs = self.funcs.pop().expect("function state");
        Ok(Rc::new(Proto {
            name,
            file: self.file.clone(),
            source: self.source.clone(),
            code: fs.code,
            pos: fs.pos,
            consts: fs.consts,
            protos: fs.protos,
            nparams: def.params.len() as u16,
            rest: def.rest,
            nslots: fs.nslots,
            upvals: fs.upvals.iter().map(|u| u.0).collect(),
            internal: self.internal,
            notes: fs.notes,
            line: def.pos.line,
        }))
    }

    /// Верхний уровень главной функции (одна область, слоты-глобальные)?
    fn at_top(&self) -> bool {
        let fs = self.funcs.last().expect("function state");
        fs.is_main && fs.scopes.len() == 1
    }

    fn add_local(&mut self, name: Rc<str>, pos: Pos, constant: bool) -> R<u16> {
        let fs = self.funcs.last().expect("function state");
        if let Some(prev) = fs.scopes.last().and_then(|s| s.iter().find(|l| l.name == name)) {
            let line = prev.decl.line;
            return Err(self.err(pos, format!("`{name}` is already declared on line {line} in this block")));
        }
        let fs = self.fs();
        let slot = fs.next_slot;
        if slot == u16::MAX {
            return Err(self.err(pos, "too many local variables in one function"));
        }
        fs.next_slot += 1;
        fs.nslots = fs.nslots.max(fs.next_slot);
        let cell = fs.captured.contains(&name);
        fs.scopes.last_mut().expect("scope").push(Local { name, slot, cell, constant, initialized: false, decl: pos });
        Ok(slot)
    }

    fn mark_initialized(&mut self, name: &Rc<str>) {
        let fs = self.fs();
        if let Some(l) = fs.scopes.last_mut().and_then(|s| s.iter_mut().rev().find(|l| &l.name == name)) {
            l.initialized = true;
        }
    }

    fn push_scope(&mut self) {
        self.fs().scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        let fs = self.fs();
        if let Some(scope) = fs.scopes.pop() {
            if let Some(first) = scope.first() {
                fs.next_slot = first.slot;
            }
        }
    }

    /// Блок с подъёмом объявлений. `new_scope == false` — для тела функции (область уже есть).
    fn block_body(&mut self, stmts: &[Stmt], own_scope_is_function: bool) -> R<()> {
        let _ = own_scope_is_function;
        self.predeclare(stmts)?;
        for s in stmts {
            self.statement(s)?;
        }
        Ok(())
    }

    fn block(&mut self, stmts: &[Stmt]) -> R<()> {
        self.push_scope();
        let r = self.block_body(stmts, false);
        self.pop_scope();
        r
    }

    /// Объявляет все `let` и `function` блока заранее и создаёт поднятые функции.
    fn predeclare(&mut self, stmts: &[Stmt]) -> R<()> {
        let top = self.at_top();
        if !top {
            for s in stmts {
                match &s.kind {
                    StmtKind::Let { decl, constant, .. } => {
                        for (n, p) in &decl.names {
                            let slot = self.add_local(n.clone(), *p, *constant)?;
                            if self.is_cell(slot) {
                                self.emit(Op::NewCell(slot), *p);
                            }
                        }
                    }
                    StmtKind::Function { name, func } => {
                        let slot = self.add_local(name.clone(), func.pos, false)?;
                        if self.is_cell(slot) {
                            self.emit(Op::NewCell(slot), func.pos);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Поднятые функции создаются до остального кода блока.
        for s in stmts {
            if let StmtKind::Function { name, func } = &s.kind {
                self.mark_initialized(name);
                self.closure(func, Some(name), func.pos)?;
                self.store_decl(name, func.pos)?;
            }
        }
        Ok(())
    }

    fn is_cell(&self, slot: u16) -> bool {
        let fs = self.funcs.last().expect("function state");
        fs.scopes.iter().flatten().any(|l| l.slot == slot && l.cell)
    }

    fn closure(&mut self, def: &FuncDef, hint: Option<&Rc<str>>, pos: Pos) -> R<()> {
        let proto = self.function(def, hint, false)?;
        let fs = self.fs();
        let i = fs.protos.len() as u32;
        fs.protos.push(proto);
        self.emit(Op::Closure(i), pos);
        Ok(())
    }

    // ---- переменные ----

    fn find_local(fs: &FuncState, name: &str) -> Option<Local> {
        for scope in fs.scopes.iter().rev() {
            if let Some(l) = scope.iter().rev().find(|l| &*l.name == name) {
                return Some(l.clone());
            }
        }
        None
    }

    fn resolve_up(&mut self, depth: usize, name: &Rc<str>) -> Option<(u16, bool)> {
        if depth == 0 {
            return None;
        }
        let parent = depth - 1;
        if let Some(l) = Self::find_local(&self.funcs[parent], name) {
            if !l.cell {
                // Консервативный анализ гарантирует ячейку; на всякий случай — глобальная.
                return None;
            }
            return Some((self.add_upval(depth, UpSrc::Local(l.slot), name, l.constant), l.constant));
        }
        let (idx, constant) = self.resolve_up(parent, name)?;
        Some((self.add_upval(depth, UpSrc::Up(idx), name, constant), constant))
    }

    fn add_upval(&mut self, depth: usize, src: UpSrc, name: &Rc<str>, constant: bool) -> u16 {
        let fs = &mut self.funcs[depth];
        if let Some(i) = fs.upvals.iter().position(|(_, n, _)| n == name) {
            return i as u16;
        }
        fs.upvals.push((src, name.clone(), constant));
        (fs.upvals.len() - 1) as u16
    }

    fn resolve(&mut self, name: &Rc<str>, pos: Pos, reading: bool) -> R<Var> {
        let depth = self.funcs.len() - 1;
        if let Some(l) = Self::find_local(&self.funcs[depth], name) {
            if reading && !l.initialized {
                return Err(self.err(
                    pos,
                    format!("`{name}` is used before its declaration on line {}", l.decl.line),
                ));
            }
            return Ok(Var::Local { slot: l.slot, cell: l.cell, constant: l.constant });
        }
        if let Some((index, constant)) = self.resolve_up(depth, name) {
            return Ok(Var::Up { index, constant });
        }
        Ok(Var::Global(self.global_slot(name, pos)))
    }

    fn load_var(&mut self, name: &Rc<str>, pos: Pos) -> R<()> {
        match self.resolve(name, pos, true)? {
            Var::Local { slot, cell: false, .. } => self.emit(Op::GetLocal(slot), pos),
            Var::Local { slot, cell: true, .. } => self.emit(Op::GetCell(slot), pos),
            Var::Up { index, .. } => self.emit(Op::GetUp(index), pos),
            Var::Global(i) => self.emit(Op::GetGlobal(i), pos),
        };
        Ok(())
    }

    /// Запись в объявляемую переменную (`let`): константность не проверяется.
    fn store_decl(&mut self, name: &Rc<str>, pos: Pos) -> R<()> {
        match self.resolve(name, pos, false)? {
            Var::Local { slot, cell: false, .. } => self.emit(Op::SetLocal(slot), pos),
            Var::Local { slot, cell: true, .. } => self.emit(Op::SetCell(slot), pos),
            Var::Up { index, .. } => self.emit(Op::SetUp(index), pos),
            Var::Global(i) => self.emit(Op::SetGlobal(i), pos),
        };
        self.mark_initialized(name);
        Ok(())
    }

    /// Обычное присваивание: проверяет `const` и объявленность.
    fn store_var(&mut self, name: &Rc<str>, pos: Pos) -> R<()> {
        let const_err = |c: &Compiler| {
            c.err(pos, format!("`{name}` is a const and can't be changed — declare it with `let` instead"))
        };
        match self.resolve(name, pos, false)? {
            Var::Local { constant: true, .. } | Var::Up { constant: true, .. } => return Err(const_err(self)),
            Var::Local { slot, cell: false, .. } => {
                self.emit(Op::SetLocal(slot), pos);
            }
            Var::Local { slot, cell: true, .. } => {
                self.emit(Op::SetCell(slot), pos);
            }
            Var::Up { index, .. } => {
                self.emit(Op::SetUp(index), pos);
            }
            Var::Global(i) => {
                if !self.global_info[i as usize].declared {
                    let hint = crate::error::suggest(
                        name,
                        self.globals
                            .iter()
                            .zip(&self.global_info)
                            .filter(|(_, g)| g.declared)
                            .map(|(n, _)| &**n),
                    )
                    .map(|s| format!("did you mean `{s}`?"));
                    return Err(self
                        .err(pos, format!("`{name}` is not declared — write `let {name} = …` (or `local {name} = …`) first"))
                        .with_hint(hint));
                }
                if self.global_const.contains(name) {
                    return Err(const_err(self));
                }
                self.emit(Op::SetGlobal(i), pos);
            }
        }
        Ok(())
    }

    // ---- инструкции ----

    fn statement(&mut self, s: &Stmt) -> R<()> {
        let pos = s.pos;
        match &s.kind {
            StmtKind::Let { decl, values, .. } => self.let_stmt(decl, values, pos)?,
            StmtKind::Function { .. } => {} // уже поднята
            StmtKind::FieldFunction { target, func } => {
                let name = match &target.kind {
                    ExprKind::Field(_, n) => Some(n.clone()),
                    _ => None,
                };
                self.assign_to(target, &mut |c: &mut Compiler| c.closure(func, name.as_ref(), func.pos))?;
            }
            StmtKind::Assign { targets, values } => self.assign_stmt(targets, values, pos)?,
            StmtKind::Compound { target, op, value } => self.compound(target, *op, value)?,
            StmtKind::Expr(e) => {
                self.expr(e)?;
                self.emit(Op::Pop, pos);
            }
            StmtKind::If { arms, otherwise } => {
                let mut ends = Vec::new();
                for (cond, body) in arms {
                    self.expr(cond)?;
                    let skip = self.emit(Op::JumpIfFalse(0), cond.pos);
                    self.block(body)?;
                    ends.push(self.emit(Op::Jump(0), pos));
                    self.patch(skip);
                }
                if let Some(body) = otherwise {
                    self.block(body)?;
                }
                for e in ends {
                    self.patch(e);
                }
            }
            StmtKind::While { cond, body } => {
                let start = self.here();
                self.expr(cond)?;
                let exit = self.emit(Op::JumpIfFalse(0), cond.pos);
                self.loop_body(body, None)?;
                let ctx = self.fs().loops.pop().unwrap_or_default();
                for c in ctx.continues {
                    self.patch_to(c, start);
                }
                self.jump_back(start, pos);
                self.patch(exit);
                for b in ctx.breaks {
                    self.patch(b);
                }
            }
            StmtKind::DoWhile { body, cond, until } => {
                let start = self.here();
                let try_depth = self.fs().try_depth;
                self.fs().loops.push(LoopCtx { try_depth, ..Default::default() });
                self.push_scope();
                self.block_body(body, false)?;
                let ctx_cont = std::mem::take(&mut self.fs().loops.last_mut().expect("loop").continues);
                for c in ctx_cont {
                    self.patch(c);
                }
                self.expr(cond)?;
                if !*until {
                    self.emit(Op::Not, cond.pos);
                }
                let back = self.here();
                self.emit(Op::JumpIfFalse(start as i32 - (back as i32 + 1)), cond.pos);
                self.pop_scope();
                let ctx = self.fs().loops.pop().unwrap_or_default();
                for b in ctx.breaks {
                    self.patch(b);
                }
            }
            StmtKind::NumFor { var, start, end, step, body } => {
                self.push_scope();
                let base = self.add_local("(for)".into(), pos, false)?;
                self.add_local("(limit)".into(), pos, false)?;
                self.add_local("(step)".into(), pos, false)?;
                self.expr(start)?;
                self.emit(Op::SetLocal(base), pos);
                self.expr(end)?;
                self.emit(Op::SetLocal(base + 1), pos);
                match step {
                    Some(s) => self.expr(s)?,
                    None => {
                        self.emit(Op::Int(1), pos);
                    }
                }
                self.emit(Op::SetLocal(base + 2), pos);
                let check = self.emit(Op::NumForCheck(base), pos);
                let exit = self.emit(Op::Jump(0), pos);
                // Переменная цикла — своя на каждой итерации.
                self.push_scope();
                let slot = self.add_local(var.0.clone(), var.1, false)?;
                if self.is_cell(slot) {
                    self.emit(Op::NewCell(slot), var.1);
                }
                self.emit(Op::GetLocal(base), var.1);
                self.store_decl(&var.0, var.1)?;
                self.loop_body(body, None)?;
                self.pop_scope();
                let ctx = self.fs().loops.pop().unwrap_or_default();
                for c in ctx.continues {
                    self.patch(c);
                }
                self.emit(Op::NumForStep(base), pos);
                self.jump_back(check, pos);
                self.patch(exit);
                for b in ctx.breaks {
                    self.patch(b);
                }
                self.pop_scope();
            }
            StmtKind::CFor { init, cond, step, body } => {
                self.push_scope();
                if let Some(init) = init {
                    self.predeclare(std::slice::from_ref(init))?;
                    self.statement(init)?;
                }
                let start = self.here();
                let exit = match cond {
                    Some(c) => {
                        self.expr(c)?;
                        Some(self.emit(Op::JumpIfFalse(0), c.pos))
                    }
                    None => None,
                };
                self.loop_body(body, None)?;
                let ctx = self.fs().loops.pop().unwrap_or_default();
                for c in ctx.continues {
                    self.patch(c);
                }
                if let Some(step) = step {
                    self.statement(step)?;
                }
                self.jump_back(start, pos);
                if let Some(e) = exit {
                    self.patch(e);
                }
                for b in ctx.breaks {
                    self.patch(b);
                }
                self.pop_scope();
            }
            StmtKind::ForIn { decl, iter, mode, body } => {
                self.push_scope();
                let hidden = self.add_local("(iter)".into(), pos, false)?;
                self.expr(iter)?;
                self.emit(Op::IterPrep(if *mode == IterMode::Of { 0 } else { 1 }), iter.pos);
                self.emit(Op::SetLocal(hidden), pos);
                let start = self.here();
                let n = decl.names.len();
                let pushed = if decl.pattern || *mode == IterMode::Of { 1 } else { n.min(2) };
                if !decl.pattern && *mode == IterMode::In && n > 2 {
                    return Err(self.err(pos, "`for … in` gives at most two values: the key (index) and the value"));
                }
                self.emit(Op::IterNext(hidden, pushed as u8), pos);
                let exit = self.emit(Op::Jump(0), pos);
                self.push_scope();
                if pushed == 1 && n > 1 {
                    self.emit(Op::Unpack(n as u16), pos);
                }
                for (name, p) in &decl.names {
                    let slot = self.add_local(name.clone(), *p, false)?;
                    if self.is_cell(slot) {
                        // Ячейку создаём до записи значений.
                        let at = self.here();
                        self.emit(Op::NewCell(slot), *p);
                        let _ = at;
                    }
                }
                for (name, p) in decl.names.iter().rev() {
                    self.store_decl(name, *p)?;
                }
                self.loop_body(body, None)?;
                self.pop_scope();
                let ctx = self.fs().loops.pop().unwrap_or_default();
                for c in ctx.continues {
                    self.patch_to(c, start);
                }
                self.jump_back(start, pos);
                self.patch(exit);
                for b in ctx.breaks {
                    self.patch(b);
                }
                self.pop_scope();
            }
            StmtKind::Block(body) => self.block(body)?,
            StmtKind::Return(value) => match value {
                Some(v) => {
                    self.expr(v)?;
                    self.emit(Op::Return, pos);
                }
                None => {
                    self.emit(Op::ReturnNil, pos);
                }
            },
            StmtKind::Break | StmtKind::Continue => {
                let fs = self.funcs.last().expect("function state");
                let Some(lp) = fs.loops.last() else {
                    return Err(self.err(pos, "`break`/`continue` outside of a loop"));
                };
                let pops = fs.try_depth - lp.try_depth;
                for _ in 0..pops {
                    self.emit(Op::TryEnd, pos);
                }
                let j = self.emit(Op::Jump(0), pos);
                let lp = self.fs().loops.last_mut().expect("loop");
                if matches!(s.kind, StmtKind::Break) {
                    lp.breaks.push(j);
                } else {
                    lp.continues.push(j);
                }
            }
            StmtKind::Try { body, name, handler } => {
                let begin = self.emit(Op::TryBegin(0), pos);
                self.fs().try_depth += 1;
                let r = self.block(body);
                self.fs().try_depth -= 1;
                r?;
                self.emit(Op::TryEnd, pos);
                let end = self.emit(Op::Jump(0), pos);
                self.patch(begin);
                self.push_scope();
                match name {
                    Some((n, p)) => {
                        let slot = self.add_local(n.clone(), *p, false)?;
                        if self.is_cell(slot) {
                            self.emit(Op::NewCell(slot), *p);
                        }
                        self.store_decl(n, *p)?;
                    }
                    None => {
                        self.emit(Op::Pop, pos);
                    }
                }
                self.block_body(handler, false)?;
                self.pop_scope();
                self.patch(end);
            }
            StmtKind::Throw(e) => {
                self.expr(e)?;
                self.emit(Op::Throw, pos);
            }
        }
        Ok(())
    }

    /// Тело цикла: открывает контекст цикла (закрывает его вызывающий).
    fn loop_body(&mut self, body: &[Stmt], _label: Option<()>) -> R<()> {
        let try_depth = self.fs().try_depth;
        self.fs().loops.push(LoopCtx { try_depth, ..Default::default() });
        self.block(body)
    }

    fn let_stmt(&mut self, decl: &Decl, values: &[Expr], pos: Pos) -> R<()> {
        let names = &decl.names;
        let hint = if names.len() == 1 { Some(&names[0].0) } else { None };
        if decl.pattern || (names.len() > 1 && values.len() == 1) {
            match values.first() {
                Some(v) => self.expr_named(v, None)?,
                None => {
                    self.emit(Op::Nil, pos);
                }
            }
            self.emit(Op::Unpack(names.len() as u16), pos);
        } else {
            for i in 0..names.len().max(values.len()) {
                match values.get(i) {
                    Some(v) => self.expr_named(v, if i < names.len() { hint } else { None })?,
                    None => {
                        self.emit(Op::Nil, pos);
                    }
                }
                if i >= names.len() {
                    self.emit(Op::Pop, pos);
                }
            }
        }
        for (n, p) in names.iter().rev() {
            self.store_decl(n, *p)?;
        }
        Ok(())
    }

    fn assign_stmt(&mut self, targets: &[Expr], values: &[Expr], pos: Pos) -> R<()> {
        if targets.len() == 1 {
            let hint = match &targets[0].kind {
                ExprKind::Name(n) | ExprKind::Field(_, n) => Some(n.clone()),
                _ => None,
            };
            let first = &values[0];
            let rest = &values[1..];
            self.assign_to(&targets[0], &mut |c: &mut Compiler| {
                c.expr_named(first, hint.as_ref())?;
                for extra in rest {
                    c.expr(extra)?;
                    c.emit(Op::Pop, extra.pos);
                }
                Ok(())
            })?;
            return Ok(());
        }
        // Множественное присваивание: значения во временные слоты, потом по местам.
        self.push_scope();
        let mut temps = Vec::new();
        for i in 0..targets.len() {
            temps.push(self.add_local(format!("(tmp{i})").into(), pos, false)?);
        }
        if values.len() == 1 {
            self.expr(&values[0])?;
            self.emit(Op::Unpack(targets.len() as u16), pos);
        } else {
            for i in 0..targets.len().max(values.len()) {
                match values.get(i) {
                    Some(v) => self.expr(v)?,
                    None => {
                        self.emit(Op::Nil, pos);
                    }
                }
                if i >= targets.len() {
                    self.emit(Op::Pop, pos);
                }
            }
        }
        for &t in temps.iter().rev() {
            self.emit(Op::SetLocal(t), pos);
        }
        for (t, &slot) in targets.iter().zip(&temps) {
            self.assign_to(t, &mut |c: &mut Compiler| {
                c.emit(Op::GetLocal(slot), pos);
                Ok(())
            })?;
        }
        self.pop_scope();
        Ok(())
    }

    /// Готовит основание для записи `base.x = …`: кладёт его на стек и сообщает,
    /// как записать изменённое значение-вектор обратно.
    fn prepare_base(&mut self, base: &Expr) -> R<WriteBack> {
        Ok(match &base.kind {
            ExprKind::Name(n) => {
                self.load_var(n, base.pos)?;
                WriteBack::Var(n.clone(), base.pos)
            }
            ExprKind::Field(obj, f) => {
                self.expr(obj)?;
                self.emit(Op::Dup, base.pos);
                let k = self.const_str(f);
                let at = self.emit(Op::GetField(k), base.pos);
                self.note(at, obj.describe());
                WriteBack::Field(f.clone())
            }
            ExprKind::Index(obj, key) => {
                self.expr(obj)?;
                self.expr(key)?;
                self.emit(Op::Dup2, base.pos);
                let at = self.emit(Op::GetIndex, base.pos);
                self.note(at, obj.describe());
                WriteBack::Index
            }
            _ => {
                self.expr(base)?;
                WriteBack::None
            }
        })
    }

    /// Выдаёт `SetIndex`/`SetField` и код обратной записи.
    fn finish_store(&mut self, set: Op, wb: WriteBack, pos: Pos, desc: Option<String>) -> R<()> {
        let at = self.emit(set, pos);
        self.note(at, desc);
        let (skip, extras): (u8, usize) = match wb {
            WriteBack::None => (0, 0),
            WriteBack::Var(name, p) => {
                let before = self.here();
                // Константу-вектор менять нельзя.
                let is_const = match self.resolve(&name, p, false)? {
                    Var::Local { constant, .. } | Var::Up { constant, .. } => constant,
                    Var::Global(_) => self.global_const.contains(&name),
                };
                if is_const {
                    let msg: Rc<str> = format!(
                        "`{name}` is a const; vectors and colors are values, so changing a part of one changes `{name}` itself — declare it with `let`"
                    )
                    .into();
                    let k = self.const_str(&msg);
                    self.emit(Op::Fail(k), p);
                } else {
                    self.store_var(&name, p)?;
                }
                ((self.here() - before) as u8, 0)
            }
            WriteBack::Field(f) => {
                let k = self.const_str(&f);
                self.emit(Op::SetField(k, 0), pos);
                self.emit(Op::Jump(1), pos);
                (2, 1)
            }
            WriteBack::Index => {
                self.emit(Op::SetIndex(0), pos);
                self.emit(Op::Jump(2), pos);
                (2, 2)
            }
        };
        let fs = self.fs();
        fs.code[at] = match fs.code[at] {
            Op::SetIndex(_) => Op::SetIndex(skip),
            Op::SetField(k, _) => Op::SetField(k, skip),
            other => other,
        };
        for _ in 0..extras {
            self.emit(Op::Pop, pos);
        }
        Ok(())
    }

    /// Присваивание в цель; `value` кладёт значение на стек.
    fn assign_to(&mut self, target: &Expr, value: &mut dyn FnMut(&mut Compiler) -> R<()>) -> R<()> {
        match &target.kind {
            ExprKind::Name(n) => {
                value(self)?;
                self.store_var(n, target.pos)
            }
            ExprKind::Field(obj, f) => {
                let wb = self.prepare_base(obj)?;
                value(self)?;
                let k = self.const_str(f);
                self.finish_store(Op::SetField(k, 0), wb, target.pos, obj.describe())
            }
            ExprKind::Index(obj, key) => {
                let wb = self.prepare_base(obj)?;
                self.expr(key)?;
                value(self)?;
                self.finish_store(Op::SetIndex(0), wb, target.pos, obj.describe())
            }
            _ => Err(self.err(target.pos, "can't assign to this — only to a variable, `a.field` or `a[index]`")),
        }
    }

    fn compound(&mut self, target: &Expr, op: BinOp, value: &Expr) -> R<()> {
        let bin = binop_code(op);
        match &target.kind {
            ExprKind::Name(n) => {
                self.load_var(n, target.pos)?;
                self.expr(value)?;
                self.emit(bin, value.pos);
                self.store_var(n, target.pos)
            }
            ExprKind::Field(obj, f) => {
                let wb = self.prepare_base(obj)?;
                self.emit(Op::Dup, target.pos);
                let k = self.const_str(f);
                let at = self.emit(Op::GetField(k), target.pos);
                self.note(at, obj.describe());
                self.expr(value)?;
                self.emit(bin, value.pos);
                self.finish_store(Op::SetField(k, 0), wb, target.pos, obj.describe())
            }
            ExprKind::Index(obj, key) => {
                let wb = self.prepare_base(obj)?;
                self.expr(key)?;
                self.emit(Op::Dup2, target.pos);
                let at = self.emit(Op::GetIndex, target.pos);
                self.note(at, obj.describe());
                self.expr(value)?;
                self.emit(bin, value.pos);
                self.finish_store(Op::SetIndex(0), wb, target.pos, obj.describe())
            }
            _ => Err(self.err(target.pos, "can't assign to this")),
        }
    }

    // ---- выражения ----

    fn expr(&mut self, e: &Expr) -> R<()> {
        self.expr_named(e, None)
    }

    fn args(&mut self, args: &[Arg], pos: Pos) -> R<Option<u8>> {
        if args.iter().any(|a| a.spread) {
            self.emit(Op::NewList(0), pos);
            for a in args {
                self.expr(&a.expr)?;
                self.emit(if a.spread { Op::ListExtend } else { Op::ListPush }, a.expr.pos);
            }
            return Ok(None);
        }
        if args.len() > 250 {
            return Err(self.err(pos, "too many arguments in one call (at most 250)"));
        }
        for a in args {
            self.expr(&a.expr)?;
        }
        Ok(Some(args.len() as u8))
    }

    fn expr_named(&mut self, e: &Expr, hint: Option<&Rc<str>>) -> R<()> {
        let pos = e.pos;
        match &e.kind {
            ExprKind::Nil => {
                self.emit(Op::Nil, pos);
            }
            ExprKind::Bool(true) => {
                self.emit(Op::True, pos);
            }
            ExprKind::Bool(false) => {
                self.emit(Op::False, pos);
            }
            ExprKind::Num(n) => self.push_num(*n, pos),
            ExprKind::Str(s) => {
                let k = self.const_str(s);
                self.emit(Op::Const(k), pos);
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    match p {
                        TemplatePiece::Lit(s) => {
                            let k = self.const_str(s);
                            self.emit(Op::Const(k), pos);
                        }
                        TemplatePiece::Expr(x) => self.expr(x)?,
                    }
                }
                self.emit(Op::Template(parts.len() as u32), pos);
            }
            ExprKind::Name(n) => self.load_var(n, pos)?,
            ExprKind::List(items) => {
                if items.iter().any(|a| a.spread) {
                    self.args(items, pos)?;
                } else {
                    for a in items {
                        self.expr(&a.expr)?;
                    }
                    self.emit(Op::NewList(items.len() as u32), pos);
                }
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    match k {
                        MapKey::Name(n) => {
                            let c = self.const_str(n);
                            self.emit(Op::Const(c), pos);
                            self.expr_named(v, Some(n))?;
                        }
                        MapKey::Expr(x) => {
                            self.expr(x)?;
                            self.expr(v)?;
                        }
                    }
                }
                self.emit(Op::NewMap(entries.len() as u32), pos);
            }
            ExprKind::Func(def) => self.closure(def, hint, pos)?,
            ExprKind::Index(obj, key) => {
                self.expr(obj)?;
                self.expr(key)?;
                let at = self.emit(Op::GetIndex, pos);
                self.note(at, obj.describe());
            }
            ExprKind::Field(obj, name) => {
                self.expr(obj)?;
                let k = self.const_str(name);
                let at = self.emit(Op::GetField(k), pos);
                self.note(at, obj.describe());
            }
            ExprKind::Call(callee, args) => {
                self.expr(callee)?;
                let at = match self.args(args, pos)? {
                    Some(n) => self.emit(Op::Call(n), pos),
                    None => self.emit(Op::CallList, pos),
                };
                self.note(at, callee.describe());
            }
            ExprKind::Method { obj, name, args, colon } => {
                self.expr(obj)?;
                let k = self.const_str(name);
                let at = match self.args(args, pos)? {
                    Some(n) => self.emit(Op::Method(k, n, *colon), pos),
                    None => self.emit(Op::MethodList(k, *colon), pos),
                };
                self.note(at, obj.describe());
            }
            ExprKind::Unary(op, x) => {
                self.expr(x)?;
                let code = match op {
                    UnOp::Neg => Op::Neg,
                    UnOp::Not => Op::Not,
                    UnOp::Len => Op::Len,
                    UnOp::Typeof => Op::Typeof,
                };
                let at = self.emit(code, pos);
                self.note(at, x.describe());
            }
            ExprKind::Binary(op, a, b) => {
                self.expr(a)?;
                self.expr(b)?;
                self.emit(binop_code(*op), pos);
            }
            ExprKind::And(a, b) | ExprKind::Or(a, b) | ExprKind::Nullish(a, b) => {
                self.expr(a)?;
                let j = self.emit(
                    match &e.kind {
                        ExprKind::And(..) => Op::AndJump(0),
                        ExprKind::Or(..) => Op::OrJump(0),
                        _ => Op::NilJump(0),
                    },
                    pos,
                );
                self.expr(b)?;
                self.patch(j);
            }
            ExprKind::Ternary(c, a, b) => {
                self.expr(c)?;
                let skip = self.emit(Op::JumpIfFalse(0), pos);
                self.expr_named(a, hint)?;
                let end = self.emit(Op::Jump(0), pos);
                self.patch(skip);
                self.expr_named(b, hint)?;
                self.patch(end);
            }
        }
        Ok(())
    }
}

fn binop_code(op: BinOp) -> Op {
    match op {
        BinOp::Add => Op::Add,
        BinOp::Sub => Op::Sub,
        BinOp::Mul => Op::Mul,
        BinOp::Div => Op::Div,
        BinOp::Mod => Op::Mod,
        BinOp::Pow => Op::Pow,
        BinOp::Concat => Op::Concat,
        BinOp::Eq => Op::Eq,
        BinOp::Ne => Op::Ne,
        BinOp::Lt => Op::Lt,
        BinOp::Le => Op::Le,
        BinOp::Gt => Op::Gt,
        BinOp::Ge => Op::Ge,
    }
}

// ---- анализ захвата: все имена, упомянутые во вложенных функциях ----

fn names_in_nested_stmt(s: &Stmt, inside: bool, out: &mut HashSet<Rc<str>>) {
    let e = |x: &Expr, out: &mut HashSet<Rc<str>>| names_in_nested_expr(x, inside, out);
    let body = |b: &[Stmt], out: &mut HashSet<Rc<str>>| {
        for s in b {
            names_in_nested_stmt(s, inside, out);
        }
    };
    let decl = |d: &Decl, out: &mut HashSet<Rc<str>>| {
        if inside {
            for (n, _) in &d.names {
                out.insert(n.clone());
            }
        }
    };
    match &s.kind {
        StmtKind::Let { decl: d, values, .. } => {
            decl(d, out);
            for v in values {
                e(v, out);
            }
        }
        StmtKind::Function { name, func } => {
            // Сама функция — вложенная: имя функции нужно ей для рекурсии.
            out.insert(name.clone());
            func_names(func, out);
        }
        StmtKind::FieldFunction { target, func } => {
            e(target, out);
            func_names(func, out);
        }
        StmtKind::Assign { targets, values } => {
            for t in targets {
                e(t, out);
            }
            for v in values {
                e(v, out);
            }
        }
        StmtKind::Compound { target, value, .. } => {
            e(target, out);
            e(value, out);
        }
        StmtKind::Expr(x) | StmtKind::Throw(x) => e(x, out),
        StmtKind::Return(x) => {
            if let Some(x) = x {
                e(x, out);
            }
        }
        StmtKind::If { arms, otherwise } => {
            for (c, b) in arms {
                e(c, out);
                body(b, out);
            }
            if let Some(b) = otherwise {
                body(b, out);
            }
        }
        StmtKind::While { cond, body: b } | StmtKind::DoWhile { body: b, cond, .. } => {
            e(cond, out);
            body(b, out);
        }
        StmtKind::NumFor { var, start, end, step, body: b } => {
            if inside {
                out.insert(var.0.clone());
            }
            e(start, out);
            e(end, out);
            if let Some(s) = step {
                e(s, out);
            }
            body(b, out);
        }
        StmtKind::CFor { init, cond, step, body: b } => {
            if let Some(i) = init {
                names_in_nested_stmt(i, inside, out);
            }
            if let Some(c) = cond {
                e(c, out);
            }
            if let Some(s) = step {
                names_in_nested_stmt(s, inside, out);
            }
            body(b, out);
        }
        StmtKind::ForIn { decl: d, iter, body: b, .. } => {
            decl(d, out);
            e(iter, out);
            body(b, out);
        }
        StmtKind::Block(b) => body(b, out),
        StmtKind::Try { body: b, name, handler } => {
            body(b, out);
            if let (true, Some((n, _))) = (inside, name) {
                out.insert(n.clone());
            }
            body(handler, out);
        }
        StmtKind::Break | StmtKind::Continue => {}
    }
}

fn func_names(f: &FuncDef, out: &mut HashSet<Rc<str>>) {
    for p in &f.params {
        out.insert(p.clone());
    }
    for s in &f.body {
        names_in_nested_stmt(s, true, out);
    }
}

fn names_in_nested_expr(x: &Expr, inside: bool, out: &mut HashSet<Rc<str>>) {
    let mut go = |y: &Expr| names_in_nested_expr(y, inside, out);
    match &x.kind {
        ExprKind::Name(n) => {
            if inside {
                out.insert(n.clone());
            }
        }
        ExprKind::Func(f) => func_names(f, out),
        ExprKind::Template(parts) => {
            for p in parts {
                if let TemplatePiece::Expr(y) = p {
                    go(y);
                }
            }
        }
        ExprKind::List(items) => {
            for a in items {
                go(&a.expr);
            }
        }
        ExprKind::Map(entries) => {
            for (k, v) in entries {
                if let MapKey::Expr(k) = k {
                    go(k);
                }
                go(v);
            }
        }
        ExprKind::Index(a, b)
        | ExprKind::Binary(_, a, b)
        | ExprKind::And(a, b)
        | ExprKind::Or(a, b)
        | ExprKind::Nullish(a, b) => {
            go(a);
            go(b);
        }
        ExprKind::Field(a, _) | ExprKind::Unary(_, a) => go(a),
        ExprKind::Call(f, args) => {
            go(f);
            for a in args {
                go(&a.expr);
            }
        }
        ExprKind::Method { obj, args, .. } => {
            go(obj);
            for a in args {
                go(&a.expr);
            }
        }
        ExprKind::Ternary(a, b, c) => {
            go(a);
            go(b);
            go(c);
        }
        ExprKind::Nil | ExprKind::Bool(_) | ExprKind::Num(_) | ExprKind::Str(_) => {}
    }
}
