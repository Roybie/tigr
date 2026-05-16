//! `import 'Os'` — process / environment access.
//!
//! `args` is a *value* (an Array snapshotted at module-build time),
//! not a function — command-line args don't change mid-run, and this
//! lets users index directly: `Os.args[1]`.
//!
//! `exit(code)` calls `std::process::exit` — bypasses `try`. It's a
//! real process exit, not a recoverable error.
//!
//! `run(cmd, ...args)` spawns a child process and captures its output.
//! A non-zero exit is a normal result (reported in `.code`), not an
//! error; it raises only when the process cannot be spawned at all.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    let args = build_args();
    object(&[
        ("args", args),
        ("env",  native("env",  Arity::Exact(1),   env)),
        ("cwd",  native("cwd",  Arity::Exact(0),   cwd)),
        ("run",  native("run",  Arity::AtLeast(1), run)),
        ("exit", native("exit", Arity::Exact(1),   exit)),
    ])
}

fn build_args() -> Value {
    let v: Vec<Value> = std::env::args()
        .map(|s| Value::Str(s.into()))
        .collect();
    Value::Array(gc::alloc_array(v))
}

fn raise(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
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

fn run(args: &[Value]) -> Result<Value, RuntimeError> {
    let cmd = match &args[0] {
        Value::Str(s) => s,
        other => return Err(raise(format!(
            "Os.run: expected String command, got {}", other.type_name()
        ))),
    };
    let mut command = std::process::Command::new(&**cmd);
    for (i, a) in args[1..].iter().enumerate() {
        match a {
            Value::Str(s) => { command.arg(&**s); }
            other => return Err(raise(format!(
                "Os.run: argument {} is not a String, got {}",
                i + 1, other.type_name()
            ))),
        }
    }
    let output = command
        .output()
        .map_err(|e| raise(format!("Os.run({cmd:?}): {e}")))?;
    // A terminating signal yields no exit code; report -1 in that case.
    let code = output.status.code().unwrap_or(-1) as i64;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(object(&[
        ("code",   Value::Int(code)),
        ("stdout", Value::Str(stdout.into())),
        ("stderr", Value::Str(stderr.into())),
    ]))
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
