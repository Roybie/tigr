//! `import 'Time'` — monotonic-ish wall-clock access for scripts.
//!
//! `now_ms` / `now_ns` are good enough for "how long did this take",
//! the common ask in a hobby language. Both fit in `i64`: ms doesn't
//! overflow until year 292 million; ns doesn't overflow until 2262.

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("now_ms",   native("now_ms",   Arity::Exact(0), now_ms)),
        ("now_ns",   native("now_ns",   Arity::Exact(0), now_ns)),
        ("sleep_ms", native("sleep_ms", Arity::Exact(1), sleep_ms)),
    ])
}

fn raise(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(msg), 0)
}

fn now_ms(_args: &[Value]) -> Result<Value, RuntimeError> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| raise(format!("Time.now_ms: {e}")))?;
    Ok(Value::Int(d.as_millis() as i64))
}

fn now_ns(_args: &[Value]) -> Result<Value, RuntimeError> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| raise(format!("Time.now_ns: {e}")))?;
    Ok(Value::Int(d.as_nanos() as i64))
}

fn sleep_ms(args: &[Value]) -> Result<Value, RuntimeError> {
    let ms = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Int(_) => return Err(raise("Time.sleep_ms: negative duration".into())),
        other => return Err(raise(format!(
            "Time.sleep_ms: expected Int, got {}", other.type_name()
        ))),
    };
    thread::sleep(Duration::from_millis(ms));
    Ok(Value::Null)
}
