//! Runtime values.
//!
//! Phase 1 only constructs `Null`, `Bool`, `Int`, `Float`, `Str`. The
//! enum already lists the full v0.2 type set so opcodes that reference
//! `Value` don't need refactoring as later phases land.
//!
//! Collection types use `Rc<RefCell<...>>` for reference semantics. The
//! `Rc` cycle leak is acknowledged for v0.2 (spec §15.1) — a tracing GC
//! is a v0.3 concern.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

#[allow(dead_code)] // collection / fn / range variants arrive Phase 3+
#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),

    // Phase 3+
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<IndexMap<Rc<str>, Value>>>),

    // Phase 5+
    Range(Rc<RangeData>),

    // Phase 4+
    Function(Rc<Closure>),

    // Phase 6+
    NativeFn(Rc<NativeFn>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeData {
    pub from: i64,
    pub to: i64,
    pub step: i64,
    pub inclusive: bool,
}

/// Placeholder for Phase 4. Real definition comes when functions land.
pub struct Closure {
    pub _phase: u8,
}

/// Placeholder for Phase 6.
pub struct NativeFn {
    pub _phase: u8,
}

#[allow(dead_code)] // is_truthy used Phase 2+
impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Range(_) => "range",
            Value::Function(_) => "function",
            Value::NativeFn(_) => "native function",
        }
    }

    /// Truthiness per spec §5.
    /// (Empty array/object/string falsy. Phase 1 only sees primitives.)
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Array(a) => !a.borrow().is_empty(),
            Value::Object(o) => !o.borrow().is_empty(),
            Value::Range(_) => true, // ranges with empty extent are still truthy values
            Value::Function(_) => true,
            Value::NativeFn(_) => true,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Int(a), Float(b)) | (Float(b), Int(a)) => (*a as f64) == *b,
            (Str(a), Str(b)) => a == b,
            (Array(a), Array(b)) => Rc::ptr_eq(a, b) || *a.borrow() == *b.borrow(),
            (Object(a), Object(b)) => Rc::ptr_eq(a, b) || *a.borrow() == *b.borrow(),
            (Range(a), Range(b)) => a == b,
            (Function(a), Function(b)) => Rc::ptr_eq(a, b),
            (NativeFn(a), NativeFn(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a.partial_cmp(b),
            (Float(a), Float(b)) => a.partial_cmp(b),
            (Int(a), Float(b)) => (*a as f64).partial_cmp(b),
            (Float(a), Int(b)) => a.partial_cmp(&(*b as f64)),
            (Str(a), Str(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

/// Display in the canonical Tigr form used by the `str()` built-in.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(x) => {
                // always show a decimal point so the reader can tell Int from Float
                if x.is_finite() && x.fract() == 0.0 {
                    write!(f, "{x:.1}")
                } else {
                    write!(f, "{x}")
                }
            }
            Value::Str(s) => f.write_str(s),
            Value::Array(a) => {
                f.write_str("[")?;
                let arr = a.borrow();
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 { f.write_str(", ")?; }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Value::Object(o) => {
                f.write_str("${")?;
                let obj = o.borrow();
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 { f.write_str(", ")?; }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("}")
            }
            Value::Range(r) => {
                let dots = if r.inclusive { "..=" } else { ".." };
                if r.step.abs() == 1 {
                    write!(f, "{}{}{}", r.from, dots, r.to)
                } else {
                    write!(f, "{}{}{}:{}", r.from, dots, r.to, r.step)
                }
            }
            Value::Function(_) => f.write_str("fn(...)"),
            Value::NativeFn(_) => f.write_str("<native fn>"),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "'{s}'"),
            other => write!(f, "{other}"),
        }
    }
}
