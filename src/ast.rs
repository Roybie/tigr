use std::fmt::{Debug, Formatter, Error};
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use interpreter::Env;

#[derive(Clone, PartialEq)]
pub enum Expr {
    Type(Type),
    Index(Box<Expr>, Box<Expr>),
    UnOp(UnOpCode, Box<Expr>),
    BinOp(Box<Expr>, BinOpCode, Box<Expr>),
    Block(Vec<Box<Expr>>, Box<Expr>),
    Scope(Box<Expr>),
    Args(Vec<Box<Expr>>),
    Spread(Box<Expr>),
    Range(Box<Expr>, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Import(String),
    While(Box<Expr>, Box<Expr>),
    WhileA(Box<Expr>, Box<Expr>),
    For(Box<Expr>, Box<Expr>),
    ForA(Box<Expr>, Box<Expr>),
    FuncCall(Box<Expr>, Box<Expr>),
    NatFun(NatFunction, Box<Expr>),
}

#[derive(Clone, PartialEq)]
pub enum Type {
    Id(String),
    Number(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Function(Box<Expr>, Box<Expr>, Rc<RefCell<Env>>),
    Array(Vec<Box<Expr>>),
    Object(HashMap<String, Type>),
    Break(Box<Expr>),
    Return(Box<Expr>),
    Null,
}

#[derive(Copy, Clone, PartialEq)]
pub enum BinOpCode {
    Pow,
    Ass,
    Mul,
    Div,
    Add,
    Sub,
    Mod,
    MulEq,
    DivEq,
    AddEq,
    SubEq,
    ModEq,
    And,
    Or,
    Equ,
    Neq,
    Lt,
    LEt,
    Gt,
    GEt,
}

#[derive(Copy, Clone, PartialEq)]
pub enum UnOpCode {
    Neg,
    Not,
    Len,
}

#[derive(Copy, Clone, PartialEq)]
pub enum NatFunction {
    Floor,
    Ceil,
    Rand,
}

#[allow(unused_must_use)]
impl Debug for Expr {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::Expr::*;
        match *self {
            UnOp(op, ref r) => write!(fmt, "{:?}{:?}", op, r),
            BinOp(ref l, op, ref r) => write!(fmt, "({:?} {:?} {:?})", l, op, r),
            Type(ref t) => write!(fmt, "{:?}", t),
            Index(ref a, ref i) => write!(fmt, "{:?}[{:?}]", a, i),
            Block(ref e, ref r) => {
                for expr in e {
                    write!(fmt, "{:?}; ", expr);
                };
                write!(fmt, "{:?}", r)
            },
            Args(ref e) => {
                write!(fmt, "(");
                for (i, expr) in e.iter().enumerate() {
                    write!(fmt, "{:?}", expr);
                    if i < e.len() - 1 { write!(fmt, ", "); }
                }
                write!(fmt, ")")
            },
            Spread(ref e) => write!(fmt, "{:?}", e),
            Scope(ref e) => write!(fmt, "{{ {:?} }}", e),
            Range(ref from, ref to, ref step) => write!(fmt, "Range({:?} to {:?} by {:?})", from, to, step),
            If(ref check, ref if_branch, ref else_branch) => write!(fmt, "if({:?}) {:?} else {:?}", check, if_branch, else_branch),
            Import(ref s) => write!(fmt, "import {:?}", s),
            While(ref check, ref branch) => write!(fmt, "while {:?} {:?}", check, branch),
            WhileA(ref check, ref branch) => write!(fmt, "while[] {:?} {:?}", check, branch),
            For(ref f, ref e) => write!(fmt, "for {:?} {:?}", f, e),
            ForA(ref f, ref e) => write!(fmt, "for[] {:?} {:?}", f, e),
            FuncCall(ref f, ref a) => write!(fmt, "{:?}{:?}", f, a),
            NatFun(op, ref e) => write!(fmt, "{:?}{:?}", op, e),
        }
    }
}

impl Debug for BinOpCode {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::BinOpCode::*;
        match *self {
            Pow => write!(fmt, "^"),
            Ass => write!(fmt, "="),
            Mul => write!(fmt, "*"),
            Div => write!(fmt, "/"),
            Add => write!(fmt, "+"),
            Sub => write!(fmt, "-"),
            Mod => write!(fmt, "%"),
            MulEq => write!(fmt, "*="),
            DivEq => write!(fmt, "/="),
            AddEq => write!(fmt, "+="),
            SubEq => write!(fmt, "-="),
            ModEq => write!(fmt, "%="),
            And => write!(fmt, "&&"),
            Or => write!(fmt, "||"),
            Equ => write!(fmt, "=="),
            Neq => write!(fmt, "!="),
            Lt => write!(fmt, "<"),
            Gt => write!(fmt, ">"),
            LEt => write!(fmt, "<="),
            GEt => write!(fmt, ">="),
        }
    }
}

impl Debug for UnOpCode {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::UnOpCode::*;
        match *self {
            Neg => write!(fmt, "-"),
            Not => write!(fmt, "Not"),
            Len => write!(fmt, "#"),
        }
    }
}

impl Debug for NatFunction {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::NatFunction::*;
        match *self {
            Floor => write!(fmt, "floor"),
            Ceil => write!(fmt, "ceil"),
            Rand => write!(fmt, "rand"),
        }
    }
}

#[allow(unused_must_use)]
impl Debug for Type {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::Type::*;
        match *self {
            Id(ref i) => write!(fmt, "Var[{:?}]", i),
            Number(n) => write!(fmt, "{}", n),
            Float(f) => write!(fmt, "{}", f),
            String(ref s) => write!(fmt, "'{}'", s),
            Bool(b) => write!(fmt, "{}", b),
            Function(ref a, ref s, _) => write!(fmt, "fn{:?} {:?}", a, s),
            Array(ref a) => {
                write!(fmt, "Arr[");
                for (i, e) in a.iter().enumerate() {
                    write!(fmt, "{:?}", e);
                    if i < a.len() - 1 { write!(fmt, ", "); }
                }
                write!(fmt, "]")
            },
            Object(ref h) => {
                write!(fmt, "${{");
                for (i, (key, value)) in h.iter().enumerate() {
                    write!(fmt, "{:?} : {:?}", key, value);
                    if i < h.len() - 1 { write!(fmt, ", "); }
                }
                write!(fmt, "}}")
            },
            Break(ref t) => write!(fmt, "break: {:?}", t),
            Return(ref t) => write!(fmt, "return: {:?}", t),
            Null => write!(fmt, "null"),
        }
    }
}
