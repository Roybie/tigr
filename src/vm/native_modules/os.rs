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
use crate::vm::offload::{BlockingJob, OffloadErr, OffloadOk};
use crate::vm::value::{Arity, Value};

use super::{native, native_blocking, object};

pub fn module() -> Value {
    let args = build_args();
    object(&[
        ("args", args),
        ("env",  native("env",  Arity::Exact(1),   env)),
        ("cwd",  native_blocking("cwd", Arity::Exact(0), cwd)),
        // `run` spawns a child process and waits for it — a blocking
        // call, so it is offloaded to the worker pool when other green
        // threads are live (see `crate::vm::offload`).
        ("run",  native_blocking("run", Arity::AtLeast(1), run)),
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

fn cwd(_args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    Ok(Box::new(|| {
        match std::env::current_dir() {
            Ok(p) => Ok(OffloadOk::Str(p.to_string_lossy().into_owned())),
            Err(e) => Err(OffloadErr {
                kind: None,
                message: format!("Os.cwd: {e}"),
            }),
        }
    }))
}

/// `run(cmd, ...args)` — a blocking native. The actor-thread half here
/// validates the arguments and extracts owned `String`s; the worker
/// closure it returns spawns the child process and captures its output
/// off-thread.
fn run(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let cmd: String = match &args[0] {
        Value::Str(s) => s.to_string(),
        other => return Err(raise(format!(
            "Os.run: expected String command, got {}", other.type_name()
        ))),
    };
    let mut arg_strs: Vec<String> = Vec::with_capacity(args.len() - 1);
    for (i, a) in args[1..].iter().enumerate() {
        match a {
            Value::Str(s) => arg_strs.push(s.to_string()),
            other => return Err(raise(format!(
                "Os.run: argument {} is not a String, got {}",
                i + 1, other.type_name()
            ))),
        }
    }
    Ok(Box::new(move || {
        let mut command = std::process::Command::new(&cmd);
        for a in &arg_strs {
            command.arg(a);
        }
        match command.output() {
            Ok(output) => {
                // A terminating signal yields no exit code; report -1.
                let code = output.status.code().unwrap_or(-1) as i64;
                let stdout =
                    String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr =
                    String::from_utf8_lossy(&output.stderr).into_owned();
                Ok(OffloadOk::Run { code, stdout, stderr })
            }
            Err(e) => Err(OffloadErr {
                kind: None,
                message: format!("Os.run({cmd:?}): {e}"),
            }),
        }
    }))
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
