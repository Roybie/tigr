//! `import '_NativeChannel'` — the channel primitives backing the
//! ergonomic `stdlib/Channel.tg` wrapper (v0.14 concurrency).
//!
//! A channel carries messages between actors. `send` transfer-encodes
//! its argument on the calling thread; `recv` decodes into the
//! caller's heap. `recv`/`try_recv` return an object the caller
//! pattern-matches: `${value: v}`, `${closed: true}`, or — `try_recv`
//! only — `${empty: true}`.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::vm::channel::{ChannelHandle, ChannelInner, RecvOutcome};
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::transfer::{decode, encode};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("new", native("new", Arity::Exact(1), c_new)),
        ("send", native("send", Arity::Exact(2), c_send)),
        ("recv", native("recv", Arity::Exact(1), c_recv)),
        ("try_recv", native("try_recv", Arity::Exact(1), c_try_recv)),
        ("close", native("close", Arity::Exact(1), c_close)),
    ])
}

fn raise(msg: &str) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

/// Extract a channel handle from an argument, or raise a type error.
fn as_channel(v: &Value) -> Result<&ChannelHandle, RuntimeError> {
    match v {
        Value::Channel(h) => Ok(h),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "expected a channel, got {}",
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

/// `new(capacity)` — `null` capacity is unbounded; a positive integer
/// bounds the buffer (the `Channel.tg` wrapper defaults it to `null`).
fn c_new(args: &[Value]) -> Result<Value, RuntimeError> {
    let capacity = match &args[0] {
        Value::Null => None,
        Value::Int(n) if *n > 0 => Some(*n as usize),
        Value::Int(_) => {
            return Err(raise("Channel capacity must be a positive integer"));
        }
        other => {
            return Err(raise(&format!(
                "Channel capacity must be an integer or null, got {}",
                other.type_name()
            )));
        }
    };
    Ok(Value::Channel(ChannelInner::new(capacity)))
}

/// `send(channel, message)` — transfer-encodes and enqueues `message`.
/// Raises `not_sendable`/`cycle` for an un-sendable value, or
/// `channel_closed` if the channel is closed. Returns `null`.
fn c_send(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_channel(&args[0])?;
    let msg = encode(&args[1])?;
    match ch.send(msg) {
        Ok(()) => Ok(Value::Null),
        Err(()) => Err(RuntimeError::new(RuntimeErrorKind::ChannelClosed, 0)),
    }
}

/// `recv(channel)` — blocks for a message. Returns `${value: v}`, or
/// `${closed: true}` once the channel is closed and drained.
fn c_recv(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_channel(&args[0])?;
    Ok(match ch.recv() {
        RecvOutcome::Message(t) => tagged("value", decode(t)),
        RecvOutcome::Closed => tagged("closed", Value::Bool(true)),
    })
}

/// `try_recv(channel)` — never blocks. Returns `${value: v}`,
/// `${closed: true}`, or `${empty: true}` if nothing is ready.
fn c_try_recv(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_channel(&args[0])?;
    Ok(match ch.try_recv() {
        Some(RecvOutcome::Message(t)) => tagged("value", decode(t)),
        Some(RecvOutcome::Closed) => tagged("closed", Value::Bool(true)),
        None => tagged("empty", Value::Bool(true)),
    })
}

/// `close(channel)` — closes the channel, waking blocked actors.
/// Returns `null`.
fn c_close(args: &[Value]) -> Result<Value, RuntimeError> {
    let ch = as_channel(&args[0])?;
    ch.close();
    Ok(Value::Null)
}
