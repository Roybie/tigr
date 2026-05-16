//! `import '_NativeMath'` — Rust math primitives.
//!
//! Backend for `stdlib/Math.tg`. Each entry takes a single Number
//! (Int promoted to Float) and returns a Float. Errors raise.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("sqrt",  native("sqrt",  Arity::Exact(1), m_sqrt)),
        ("log",   native("log",   Arity::Exact(1), m_log)),
        ("log2",  native("log2",  Arity::Exact(1), m_log2)),
        ("log10", native("log10", Arity::Exact(1), m_log10)),
        ("exp",   native("exp",   Arity::Exact(1), m_exp)),
        ("sin",   native("sin",   Arity::Exact(1), m_sin)),
        ("cos",   native("cos",   Arity::Exact(1), m_cos)),
        ("tan",   native("tan",   Arity::Exact(1), m_tan)),
        ("pow",   native("pow",   Arity::Exact(2), m_pow)),
    ])
}

fn as_float(v: &Value, label: &str) -> Result<f64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n as f64),
        Value::Float(x) => Ok(*x),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(format!(
                "Math.{label}: expected Number, got {}",
                other.type_name()
            ).into())),
            0,
        )),
    }
}

macro_rules! unary {
    ($fn_name:ident, $label:literal, $rust_fn:ident) => {
        fn $fn_name(args: &[Value]) -> Result<Value, RuntimeError> {
            let x = as_float(&args[0], $label)?;
            Ok(Value::Float(x.$rust_fn()))
        }
    };
}

unary!(m_sqrt,  "sqrt",  sqrt);
unary!(m_log,   "log",   ln);
unary!(m_log2,  "log2",  log2);
unary!(m_log10, "log10", log10);
unary!(m_exp,   "exp",   exp);
unary!(m_sin,   "sin",   sin);
unary!(m_cos,   "cos",   cos);
unary!(m_tan,   "tan",   tan);

fn m_pow(args: &[Value]) -> Result<Value, RuntimeError> {
    let base = as_float(&args[0], "pow")?;
    let exp_ = as_float(&args[1], "pow")?;
    Ok(Value::Float(base.powf(exp_)))
}
