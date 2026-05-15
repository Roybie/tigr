//! `import 'Os'` — process / environment access.
//!
//! `args` is a *value* (an Array snapshotted at module-build time),
//! not a function — command-line args don't change mid-run, and this
//! lets users index directly: `Os.args[1]`.
//!
//! `exit(code)` calls `std::process::exit` — bypasses `try`. It's a
//! real process exit, not a recoverable error.

use std::cell::RefCell;
use std::rc::Rc;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    let args = build_args();
    object(&[
        ("args", args),
        ("env",  native("env",  Arity::Exact(1), env)),
        ("cwd",  native("cwd",  Arity::Exact(0), cwd)),
        ("exit", native("exit", Arity::Exact(1), exit)),
    ])
}

fn build_args() -> Value {
    let v: Vec<Value> = std::env::args()
        .map(|s| Value::Str(s.into()))
        .collect();
    Value::Array(Rc::new(RefCell::new(v)))
}

fn raise(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(msg), 0)
}

fn env(args: &[Value]) -> Result<Value, RuntimeError> {
    let name = match &args[0] {
        Value::Str(s) => s,
        other => return Err(raise(format!(
            "Os.env: expected String, got {}", other.type_name()
        ))),
    };
    match std::env::var(&**name) {
        Ok(v) => Ok(Value::Str(v.into())),
        Err(_) => Ok(Value::Null),
    }
}

fn cwd(_args: &[Value]) -> Result<Value, RuntimeError> {
    std::env::current_dir()
        .map(|p| Value::Str(p.to_string_lossy().to_string().into()))
        .map_err(|e| raise(format!("Os.cwd: {e}")))
}

fn exit(args: &[Value]) -> Result<Value, RuntimeError> {
    let code = match &args[0] {
        Value::Int(n) => *n as i32,
        other => return Err(raise(format!(
            "Os.exit: expected Int, got {}", other.type_name()
        ))),
    };
    std::process::exit(code);
}
