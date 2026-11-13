//! Байткод: стековая машина. Локальные переменные — слоты кадра; захваченные
//! замыканиями — «ячейки» (`Rc<RefCell<Value>>`) в слотах.

use std::collections::HashMap;
use std::rc::Rc;

use crate::error::Pos;
use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Nil,
    True,
    False,
    Const(u32),
    Int(i32),
    Pop,
    Dup,
    /// `a b` → `a b a b`.
    Dup2,
    GetLocal(u16),
    SetLocal(u16),
    /// Слот получает новую ячейку со значением `nil`.
    NewCell(u16),
    GetCell(u16),
    SetCell(u16),
    GetUp(u16),
    SetUp(u16),
    GetGlobal(u32),
    SetGlobal(u32),
    /// Список из `n` значений со стека.
    NewList(u32),
    /// `list v` → `list` (v добавлен).
    ListPush,
    /// `list other` → `list` (элементы other добавлены).
    ListExtend,
    /// Словарь из `n` пар `ключ значение`.
    NewMap(u32),
    GetIndex,
    GetField(u32),
    /// `obj key value` → (ничего). Если `obj` — значение-вектор, новое значение
    /// кладётся на стек и выполняется следующая за этой инструкцией обратная запись;
    /// иначе пропускается `skip` инструкций.
    SetIndex(u8),
    SetField(u32, u8),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Neg,
    Not,
    Len,
    Typeof,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Jump(i32),
    /// Снимает значение; переход, если оно ложно.
    JumpIfFalse(i32),
    /// `a and b`: если вершина ложна — переход (значение остаётся), иначе снять.
    AndJump(i32),
    /// `a or b`: если вершина истинна — переход (значение остаётся), иначе снять.
    OrJump(i32),
    /// `a ?? b`: если вершина не `nil` — переход (значение остаётся), иначе снять.
    NilJump(i32),
    /// `f a1..an` → результат.
    Call(u8),
    /// `f argslist` → результат.
    CallList,
    /// `obj a1..an` → результат вызова метода `name`.
    Method(u32, u8, bool),
    /// `obj argslist` → результат.
    MethodList(u32, bool),
    Closure(u32),
    Return,
    ReturnNil,
    /// Готовит цикл `for`: коллекция → состояние итерации (режим: 0 — of, 1 — in).
    IterPrep(u8),
    /// Следующий шаг: кладёт `n` значений и пропускает следующую инструкцию
    /// (`Jump` на выход), либо выполняет её, если элементы кончились.
    IterNext(u16, u8),
    /// Числовой for: слоты `s` (счётчик), `s+1` (предел), `s+2` (шаг). Продолжать —
    /// пропустить следующую инструкцию (`Jump` на выход).
    NumForCheck(u16),
    NumForStep(u16),
    /// Склеивает `n` значений в строку.
    Template(u32),
    /// Значение → `n` значений (список раскладывается, остальное — первым значением).
    Unpack(u16),
    /// Начало `try`: при ошибке — переход на смещение с ошибкой на стеке.
    TryBegin(i32),
    TryEnd,
    Throw,
    /// Ошибка с текстом из константы.
    Fail(u32),
}

/// Откуда замыкание берёт захваченную переменную.
#[derive(Clone, Copy, Debug)]
pub enum UpSrc {
    /// Ячейка в слоте объемлющей функции.
    Local(u16),
    /// Захваченная переменная объемлющей функции.
    Up(u16),
}

/// Скомпилированная функция.
pub struct Proto {
    pub name: Rc<str>,
    pub file: Rc<str>,
    pub source: Rc<str>,
    pub code: Vec<Op>,
    pub pos: Vec<Pos>,
    pub consts: Vec<Value>,
    pub protos: Vec<Rc<Proto>>,
    pub nparams: u16,
    pub rest: bool,
    pub nslots: u16,
    pub upvals: Vec<UpSrc>,
    /// Служебная функция стандартной библиотеки: в стеке ошибок не показывается.
    pub internal: bool,
    /// Описание вызываемого/индексируемого выражения по номеру инструкции (`player.hp`).
    pub notes: HashMap<u32, Rc<str>>,
    pub line: u32,
}

impl Proto {
    pub fn pos_at(&self, ip: usize) -> Pos {
        self.pos.get(ip).copied().unwrap_or_else(|| Pos::new(self.line, 1))
    }
}
