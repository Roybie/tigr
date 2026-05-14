//! Tigr v0.2 AST.
//!
//! Phase 1 covers literals, identifiers, binary/unary arithmetic,
//! blocks, declarations (`x := expr`) and assignments (`x = expr`).
//! Later phases extend `Expr` with additional variants; this enum will
//! grow but existing variants stay stable.

use crate::vm::token::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct SpannedExpr {
    pub expr: Expr,
    pub span: Span,
}

impl SpannedExpr {
    pub fn new(expr: Expr, span: Span) -> Self {
        SpannedExpr { expr, span }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<SpannedExpr>,
    /// `None` if the block ends with `;` (or is empty); the block's
    /// value is `null` in that case.
    pub tail: Option<Box<SpannedExpr>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,

    // Variable reference
    Ident(String),

    // Operators
    BinOp(BinOp, Box<SpannedExpr>, Box<SpannedExpr>),
    UnOp(UnOp, Box<SpannedExpr>),

    // Block of `;`-separated expressions. Used for the top-level
    // program and for parenthesised blocks `(a; b; c)`.
    Block(Block),

    // `x := expr` — declares a new binding in the current scope.
    Decl(String, Box<SpannedExpr>),

    // `x = expr` — assigns to an existing binding (error if absent).
    Assign(String, Box<SpannedExpr>),
}

#[allow(dead_code)] // Eq..Or land in Phase 2
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    // Comparison (Phase 2+)
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical (Phase 2+)
    And,
    Or,
}

#[allow(dead_code)] // Not/Len land in Phase 2/3
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    Len,
}
