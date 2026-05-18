//! `import '_NativeLocalChannel'` — the primitives backing the
//! ergonomic `stdlib/LocalChannel.tg` wrapper (Phase 4 green threads).
//!
//! A `LocalChannel` carries messages between *green threads* of one
//! actor. Every coroutine that touches it shares the actor's heap, so
//! a message moves by value — no transfer-encoding, no deep copy.
//!
//! Every primitive here is non-blocking: `new`/`send`/`try_recv`/
//! `close` never suspend. The blocking `recv` is built in
//! `LocalChannel.tg` by `yield`-looping on `try_recv` — cooperative
//! waiting belongs in tigr, where `yield` exists.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc::{self, GcRef, LocalChannelKind};
use crate::vm::local_channel::LocalChannel;
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("new", native("new", Arity::Exact(0), lc_new)),
        ("send", native("send", Arity::Exact(2), lc_send)),
        ("try_recv", native("try_recv", Arity::Exact(1), lc_try_recv)),
        ("close", native("close", Arity::Exact(1), lc_close)),
    ])
}

/// Extract a local-channel handle from an argument, or raise a type
/// error.
fn as_local_channel(v: &Value) -> Result<GcRef<LocalChannelKind>, RuntimeError> {
    match v {
        Value::LocalChannel(h) => Ok(*h),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "expected a local channel, got {}",
                other.type_name()
            )),
            0,
        )),
    }
}

/// Build a single-field result object (`${key: val}`).
fn tagged(key: &str, val: Value) -> Value {
    let mut m: IndexMap<Arc<str>, Value> = IndexMap::with_capacity(1);
    m.insert(Arc::from(key), val);
    Value::Object(gc::alloc_object(m))
}

/// `new()` — an empty, unbounded intra-actor channel.
fn lc_new(_args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::LocalChannel(gc::alloc_local_channel(LocalChannel::new())))
}

/// `send(channel, message)` — enqueues `message` by value (no copy).
/// Raises `channel_closed` on a closed channel. Returns `null`.
fn lc_send(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_local_channel(&args[0])?;
    let mut c = ch.borrow_mut();
    if c.closed {
        return Err(RuntimeError::new(RuntimeErrorKind::ChannelClosed, 0));
    }
    c.queue.push_back(args[1].clone());
    Ok(Value::Null)
}

/// `try_recv(channel)` — never blocks. Returns `${value: v}`,
/// `${closed: true}`, or `${empty: true}` if nothing is ready. The
/// `LocalChannel.tg` `recv` wrapper `yield`-loops on the `empty` case.
fn lc_try_recv(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_local_channel(&args[0])?;
    let mut c = ch.borrow_mut();
    if let Some(msg) = c.queue.pop_front() {
        Ok(tagged("value", msg))
    } else if c.closed {
        Ok(tagged("closed", Value::Bool(true)))
    } else {
        Ok(tagged("empty", Value::Bool(true)))
    }
}

/// `close(channel)` — marks the channel closed: `send` then raises and
/// `recv` drains the buffer, then reports `${closed: true}`. Returns
/// `null`.
fn lc_close(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_local_channel(&args[0])?;
    ch.borrow_mut().closed = true;
    Ok(Value::Null)
}
