//! `import 'WS'` on the browser (`wasm32`) — WebSocket over the host's
//! native `WebSocket`.
//!
//! On a native target `WS` is the pure-tigr `stdlib/WS.tg`, layered on
//! `Net`. A browser has no raw TCP, only `WebSocket`, so on `wasm32` the
//! same `WS` API is backed by this module instead (see
//! [`crate::vm::native_modules::resolve`]). It is *not* built for tigr's
//! own `wasm-bindgen` playground (the `playground` feature), whose
//! loader cannot supply the raw `env` imports below; a plain-wasm host
//! (purr's miniquad loader) fills them with a small JS plugin. The
//! reference implementation is `web/tigr_ws.js`, and the ABI is
//! documented in `docs/stdlib/ws.md`.
//!
//! The surface matches `WS.tg` exactly — `connect` / `send` / `poll` /
//! `drain` / `state` / `close` — so the same tigr program runs unchanged
//! native and on web. The handle here is an opaque integer id (an object
//! on native); either way a program only passes it back to the other
//! calls.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::value::{Arity, Value};

use super::{bytes, native, object};

// The host-provided bridge to the browser `WebSocket`. Each function is
// imported from the wasm `env` module; a plain-wasm host (purr)
// implements them in JS over linear memory, the same mechanism its
// audio / gamepad plugins use. See `web/tigr_ws.js`.
extern "C" {
    /// Open a connection to `url` (UTF-8 at `ptr`/`len`). Returns a
    /// non-negative handle id; the connection completes asynchronously,
    /// so this never blocks and reports failure later via
    /// `tigr_ws_state`. A negative return is a synchronous reject (e.g.
    /// a malformed url).
    fn tigr_ws_connect(ptr: *const u8, len: usize) -> i32;
    /// Send a message on handle `id`. `is_binary` selects a binary (`1`)
    /// or text (`0`) frame; `ptr`/`len` is the payload in linear memory.
    fn tigr_ws_send(id: i32, ptr: *const u8, len: usize, is_binary: i32);
    /// Dequeue the next inbound message on `id` into `out`/`cap`. The
    /// first written byte is a kind tag (`0` text, `1` binary); the rest
    /// is the payload. Returns the *framed* length (tag + payload):
    ///   `0`                no message is waiting;
    ///   `n` (`0 < n <= cap`) `n` bytes were written and the message
    ///                        was consumed;
    ///   `n` (`n > cap`)    the buffer is too small and nothing was
    ///                        consumed — retry with an `n`-byte buffer;
    ///   `< 0`              the connection is closed and the queue empty.
    fn tigr_ws_poll(id: i32, out: *mut u8, cap: usize) -> i32;
    /// The connection state: `0` connecting, `1` open, `2` closed.
    fn tigr_ws_state(id: i32) -> i32;
    /// Close handle `id`. Idempotent.
    fn tigr_ws_close(id: i32);
}

pub fn module() -> Value {
    object(&[
        ("connect", native("connect", Arity::Exact(1), ws_connect)),
        ("send",    native("send",    Arity::Exact(2), ws_send)),
        ("poll",    native("poll",    Arity::Exact(1), ws_poll)),
        ("drain",   native("drain",   Arity::Exact(1), ws_drain)),
        ("state",   native("state",   Arity::Exact(1), ws_state)),
        ("close",   native("close",   Arity::Exact(1), ws_close)),
    ])
}

/// A catchable structured error `${kind, message}`, mirroring `WS.tg`.
fn raise(kind: &str, message: String) -> RuntimeError {
    let obj = object(&[
        ("kind", Value::Str(kind.into())),
        ("message", Value::Str(message.into())),
    ]);
    RuntimeError::new(RuntimeErrorKind::Raised(obj), 0)
}

/// Extract a handle id (an `Int`, as `connect` returns).
fn expect_id(v: &Value, label: &str) -> Result<i32, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n as i32),
        other => Err(raise(
            "type",
            format!("WS.{label}: expected a handle, got {}", other.type_name()),
        )),
    }
}

fn ws_connect(args: &[Value]) -> Result<Value, RuntimeError> {
    let url = match &args[0] {
        Value::Str(s) => s,
        other => {
            return Err(raise(
                "type",
                format!("WS.connect: expected a String url, got {}", other.type_name()),
            ));
        }
    };
    let id = unsafe { tigr_ws_connect(url.as_ptr(), url.len()) };
    if id < 0 {
        return Err(raise(
            "connect",
            format!("WS.connect: host rejected url {url:?}"),
        ));
    }
    Ok(Value::Int(id as i64))
}

fn ws_send(args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_id(&args[0], "send")?;
    match &args[1] {
        Value::Str(s) => unsafe {
            tigr_ws_send(id, s.as_ptr(), s.len(), 0);
        },
        Value::Bytes(b) => {
            let data = b.borrow();
            unsafe {
                tigr_ws_send(id, data.as_ptr(), data.len(), 1);
            }
        }
        other => {
            return Err(raise(
                "type",
                format!("WS.send: expected String or Bytes, got {}", other.type_name()),
            ));
        }
    }
    Ok(Value::Null)
}

/// Pull one inbound message from the host queue, growing the transfer
/// buffer if a message does not fit. `None` means the queue is empty
/// (or the connection is closed).
fn poll_one(id: i32) -> Option<Value> {
    let mut cap: usize = 1024;
    loop {
        let mut buf = vec![0u8; cap];
        let n = unsafe { tigr_ws_poll(id, buf.as_mut_ptr(), cap) };
        if n <= 0 {
            return None; // nothing waiting, or closed and drained
        }
        let n = n as usize;
        if n > cap {
            cap = n; // too small; the host kept the message — retry
            continue;
        }
        buf.truncate(n);
        let payload = buf[1..].to_vec();
        return Some(if buf[0] == 0 {
            Value::Str(String::from_utf8_lossy(&payload).into_owned().into())
        } else {
            bytes(payload)
        });
    }
}

fn ws_poll(args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_id(&args[0], "poll")?;
    Ok(poll_one(id).unwrap_or(Value::Null))
}

fn ws_drain(args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_id(&args[0], "drain")?;
    let mut msgs: Vec<Value> = Vec::new();
    while let Some(m) = poll_one(id) {
        msgs.push(m);
    }
    Ok(Value::Array(gc::alloc_array(msgs)))
}

fn ws_state(args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_id(&args[0], "state")?;
    let name = match unsafe { tigr_ws_state(id) } {
        0 => "connecting",
        1 => "open",
        _ => "closed",
    };
    Ok(Value::Str(name.into()))
}

fn ws_close(args: &[Value]) -> Result<Value, RuntimeError> {
    let id = expect_id(&args[0], "close")?;
    unsafe {
        tigr_ws_close(id);
    }
    Ok(Value::Null)
}
