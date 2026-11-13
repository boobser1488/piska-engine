//! Синтаксическое дерево. Обе формы записи (Lua и JavaScript) сводятся к одним и тем же
//! узлам, поэтому компилятор и ВМ о «диалектах» не знают.

use std::rc::Rc;

use crate::error::Pos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Len,
    Typeof,
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Debug)]
pub enum TemplatePiece {
    Lit(Rc<str>),
    Expr(Expr),
}

#[derive(Debug)]
pub struct Arg {
    pub expr: Expr,
    pub spread: bool,
}

#[derive(Debug)]
pub enum MapKey {
    Name(Rc<str>),
    Expr(Expr),
}

#[derive(Debug)]
pub enum ExprKind {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Template(Vec<TemplatePiece>),
    Name(Rc<str>),
    List(Vec<Arg>),
    Map(Vec<(MapKey, Expr)>),
    Func(Rc<FuncDef>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, Rc<str>),
    Call(Box<Expr>, Vec<Arg>),
    Method { obj: Box<Expr>, name: Rc<str>, args: Vec<Arg>, colon: bool },
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Nullish(Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Короткое описание выражения для сообщений: `player.hp`, `greet`.
    pub fn describe(&self) -> Option<String> {
        match &self.kind {
            ExprKind::Name(n) => Some(n.to_string()),
            ExprKind::Field(obj, name) => Some(format!("{}.{}", obj.describe()?, name)),
            ExprKind::Index(obj, key) => match &key.kind {
                ExprKind::Num(n) => Some(format!("{}[{}]", obj.describe()?, crate::value::fmt_num(*n))),
                ExprKind::Str(s) => Some(format!("{}[\"{}\"]", obj.describe()?, s)),
                ExprKind::Name(k) => Some(format!("{}[{}]", obj.describe()?, k)),
                _ => Some(format!("{}[…]", obj.describe()?)),
            },
            ExprKind::Call(callee, _) => Some(format!("{}(…)", callee.describe()?)),
            ExprKind::Method { obj, name, colon, .. } => {
                Some(format!("{}{}{}(…)", obj.describe()?, if *colon { ":" } else { "." }, name))
            }
            _ => None,
        }
    }
}

/// Объявление функции (любая форма: `function`, стрелка, Lua-функция).
#[derive(Debug)]
pub struct FuncDef {
    pub name: Rc<str>,
    pub params: Vec<Rc<str>>,
    /// Последний параметр собирает остальные аргументы в список (`...rest`).
    pub rest: bool,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

/// Что объявляет `let`: имена или шаблон `[a, b]`.
#[derive(Debug)]
pub struct Decl {
    pub names: Vec<(Rc<str>, Pos)>,
    /// `let [a, b] = list` — разбор списка.
    pub pattern: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterMode {
    /// `for (x of list)` — значения.
    Of,
    /// `for k, v in pairs(m)` / `for (k in m)` — сначала ключ (индекс), потом значение.
    In,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
}

#[derive(Debug)]
pub enum StmtKind {
    Let { decl: Decl, values: Vec<Expr>, constant: bool },
    /// `function name() …` / `local function name() …` — объявление с подъёмом (hoisting).
    Function { name: Rc<str>, func: Rc<FuncDef> },
    /// `function a.b() …` / `function a:b() …` — присваивание в поле.
    FieldFunction { target: Expr, func: Rc<FuncDef> },
    Assign { targets: Vec<Expr>, values: Vec<Expr> },
    Compound { target: Expr, op: BinOp, value: Expr },
    Expr(Expr),
    If { arms: Vec<(Expr, Vec<Stmt>)>, otherwise: Option<Vec<Stmt>> },
    While { cond: Expr, body: Vec<Stmt> },
    /// `repeat … until c` (`until == true`) и `do { … } while (c)`.
    DoWhile { body: Vec<Stmt>, cond: Expr, until: bool },
    NumFor { var: (Rc<str>, Pos), start: Expr, end: Expr, step: Option<Expr>, body: Vec<Stmt> },
    CFor { init: Option<Box<Stmt>>, cond: Option<Expr>, step: Option<Box<Stmt>>, body: Vec<Stmt> },
    ForIn { decl: Decl, iter: Expr, mode: IterMode, body: Vec<Stmt> },
    Block(Vec<Stmt>),
    Return(Option<Expr>),
    Break,
    Continue,
    Try { body: Vec<Stmt>, name: Option<(Rc<str>, Pos)>, handler: Vec<Stmt> },
    Throw(Expr),
}
