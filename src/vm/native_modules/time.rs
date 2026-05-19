//! `import 'Time'` — monotonic-ish wall-clock access for scripts.
//!
//! `now_ms` / `now_ns` are good enough for "how long did this take",
//! the common ask in a hobby language. Both fit in `i64`: ms doesn't
//! overflow until year 292 million; ns doesn't overflow until 2262.
//!
//! The browser playground build has no OS clock or thread; `now_*` are
//! backed by JavaScript's `Date.now()` and `sleep_ms` raises a
//! catchable error (a tab cannot block synchronously).

#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
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
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms(_args: &[Value]) -> Result<Value, RuntimeError> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| raise(format!("Time.now_ms: {e}")))?;
    Ok(Value::Int(d.as_millis() as i64))
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ns(_args: &[Value]) -> Result<Value, RuntimeError> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| raise(format!("Time.now_ns: {e}")))?;
    Ok(Value::Int(d.as_nanos() as i64))
}

#[cfg(not(target_arch = "wasm32"))]
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

/// `Date.now()` — milliseconds since the Unix epoch, as JS sees them.
#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = Date)]
        pub fn now() -> f64;
    }
}

#[cfg(target_arch = "wasm32")]
fn now_ms(_args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Int(js::now() as i64))
}

#[cfg(target_arch = "wasm32")]
fn now_ns(_args: &[Value]) -> Result<Value, RuntimeError> {
    // `Date.now()` has only millisecond resolution; scale to ns so the
    // unit matches the native build even though the low digits are 0.
    Ok(Value::Int((js::now() * 1.0e6) as i64))
}

#[cfg(target_arch = "wasm32")]
fn sleep_ms(_args: &[Value]) -> Result<Value, RuntimeError> {
    Err(raise(
        "Time.sleep_ms is unavailable in the browser playground".into(),
    ))
}
