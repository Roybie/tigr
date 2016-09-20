use std::fmt::{Debug, Formatter, Error};

#[derive(Clone, PartialEq)]
pub enum Expr {
    Type(Type),
    UnOp(UnOpCode, Box<Expr>),
    BinOp(Box<Expr>, BinOpCode, Box<Expr>),
    Block(Vec<Box<Expr>>, Box<Expr>),
    Scope(Box<Expr>),
    Args(Vec<Box<Expr>>),
    Spread(Box<Expr>),
    Range(Box<Expr>, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    For(Box<Expr>, Box<Expr>),
    ForA(Box<Expr>, Box<Expr>),
}

#[derive(Clone, PartialEq)]
pub enum Type {
    Id(String),
    Number(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Array(Vec<Box<Expr>>),
    Null,
}

#[derive(Copy, Clone, PartialEq)]
pub enum BinOpCode {
    Ass,
    Mul,
    Div,
    Add,
    Sub,
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
}

#[allow(unused_must_use)]
impl Debug for Expr {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::Expr::*;
        match *self {
            UnOp(op, ref r) => write!(fmt, "{:?}{:?}", op, r),
            BinOp(ref l, op, ref r) => write!(fmt, "({:?} {:?} {:?})", l, op, r),
            Type(ref t) => write!(fmt, "{:?}", t),
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
            For(ref f, ref e) => write!(fmt, "for {:?} {:?}", f, e),
            ForA(ref f, ref e) => write!(fmt, "for[] {:?} {:?}", f, e),
        }
    }
}

impl Debug for BinOpCode {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        use self::BinOpCode::*;
        match *self {
            Ass => write!(fmt, "="),
            Mul => write!(fmt, "*"),
            Div => write!(fmt, "/"),
            Add => write!(fmt, "+"),
            Sub => write!(fmt, "-"),
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
            Array(ref a) => {
                write!(fmt, "Arr[");
                for (i, e) in a.iter().enumerate() {
                    write!(fmt, "{:?}", e);
                    if i < a.len() - 1 { write!(fmt, ", "); }
                }
                write!(fmt, "]")
            },
            Null => write!(fmt, "null"),
        }
    }
}
