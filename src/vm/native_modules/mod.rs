//! Native (Rust-implemented) modules exposed via `import 'Name'`.
//!
//! The compiler emits bare-name `import 'X'` (no path separators or
//! `.`) as a raw constant; at runtime the Import opcode consults
//! [`resolve`] before falling back to the filesystem. Each call to a
//! module builder rebuilds its Object — the Vm-side cache in
//! `module_cache` ensures a given module is built at most once per
//! `Vm` run.

pub mod array;
pub mod bigint;
pub mod bytes;
pub mod channel;
pub mod datetime;
pub mod deferred;
pub mod io;
pub mod json;
pub mod local_channel;
pub mod map;
pub mod math;
// `Net` (sockets/TLS) builds on every native target: it depends on the
// real `socket`/`reactor` API, stubbed only on `wasm32` (no browser
// sockets). `Os` (processes/env) below is likewise portable.
#[cfg(not(target_arch = "wasm32"))]
pub mod net;
pub mod object;
#[cfg(not(target_arch = "wasm32"))]
pub mod os;
pub mod path;
pub mod random;
pub mod set;
pub mod string;
pub mod time;
// The browser `WebSocket` backend for `import 'WS'`. Built only for a
// plain-wasm host (purr): on `wasm32` there is no `Net` for the
// pure-tigr `WS.tg` to use, so the same `WS` API is served by this
// native module over host `env` imports. Excluded from the
// `wasm-bindgen` playground, whose loader cannot supply those imports.
#[cfg(all(target_arch = "wasm32", not(feature = "playground")))]
pub mod ws_web;

use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::vm::error::RuntimeError;
use crate::vm::gc;
use crate::vm::offload::BlockingJob;
use crate::vm::socket::ReactorOp;
use crate::vm::value::{Arity, NativeFn, NativeKind, Value, WaitKind};

/// The bare names of the *public* native modules, in a stable order.
/// Used to seed the ambient global namespace (usable without an
/// explicit `import`). The `_Native*` backends are intentionally
/// excluded — they stay import-only internals. `Os`/`Net` are listed
/// unconditionally so the ambient name set (and thus global indices)
/// is platform-independent; on targets where they are unavailable,
/// [`resolve`] returns `None` and a reference fails at runtime with a
/// clean "no module of that name" error, exactly as `import` does.
pub fn names() -> &'static [&'static str] {
    &[
        "IO", "Path", "Time", "DateTime", "JSON", "Random", "Bytes",
        "BigInt", "Os", "Net", "Deferred",
    ]
}

/// Look up a bare-name module. Returns `None` if no native module of
/// that name exists — callers should then fall back to filesystem
/// resolution or surface an "module not found" error.
pub fn resolve(name: &str) -> Option<Value> {
    match name {
        "IO" => Some(io::module()),
        "Path" => Some(path::module()),
        "Time" => Some(time::module()),
        "DateTime" => Some(datetime::module()),
        "JSON" => Some(json::module()),
        "Random" => Some(random::module()),
        "Bytes" => Some(bytes::module()),
        "BigInt" => Some(bigint::module()),
        // First-class deferred results. Pure VM machinery (GC + the
        // cooperative scheduler), so available on every target including
        // `wasm32` — no threads, sockets or processes involved.
        "Deferred" => Some(deferred::module()),
        // `Os` (processes/env) and the cross-actor `_NativeChannel` use
        // `std::process` and OS threads — portable to every native
        // target, so they stay enabled on Windows. Only `wasm32` (no
        // threads/processes in the playground) drops them.
        #[cfg(not(target_arch = "wasm32"))]
        "Os" => Some(os::module()),
        #[cfg(not(target_arch = "wasm32"))]
        "_NativeChannel" => Some(channel::module()),
        // `Net` (sockets/TLS) needs the async reactor, unavailable on
        // `wasm32`. Unregistered there so `import 'Net'` fails with a
        // clean, catchable "no module of that name" error rather than
        // panicking.
        #[cfg(not(target_arch = "wasm32"))]
        "Net" => Some(net::module()),
        // `WS` on a plain-wasm host: no `Net` exists for the source
        // `WS.tg`, so the browser-`WebSocket` backend serves the same
        // API. On native, `WS` resolves earlier via `source_stdlib`
        // (`WS.tg`); on the playground it stays unavailable.
        #[cfg(all(target_arch = "wasm32", not(feature = "playground")))]
        "WS" => Some(ws_web::module()),
        // Underscore-prefixed names are backends for source stdlibs
        // (Math.tg / String.tg wrap these). User code can also import
        // them directly if it wants the raw primitives.
        "_NativeLocalChannel" => Some(local_channel::module()),
        "_NativeArray" => Some(array::module()),
        "_NativeMap" => Some(map::module()),
        "_NativeMath" => Some(math::module()),
        "_NativeObject" => Some(object::module()),
        "_NativeSet" => Some(set::module()),
        "_NativeString" => Some(string::module()),
        _ => None,
    }
}

/// Build a `Value::NativeFn` for an ordinary (non-blocking) module
/// entry — runs inline on the actor thread.
pub fn native(
    name: &'static str,
    arity: Arity,
    func: fn(&[Value]) -> Result<Value, RuntimeError>,
) -> Value {
    Value::NativeFn(Rc::new(NativeFn {
        name,
        arity,
        kind: NativeKind::Pure(func),
    }))
}

/// Build a `Value::NativeFn` for a *blocking* module entry — a call
/// that may wait, which the VM can offload to a worker pool so a green
/// thread doing IO does not stall its siblings. `func` runs on the
/// actor thread to validate arguments and extract `Send` POD; the
/// closure it returns runs on a pool thread. See [`crate::vm::offload`].
pub fn native_blocking(
    name: &'static str,
    arity: Arity,
    func: fn(&[Value]) -> Result<BlockingJob, RuntimeError>,
) -> Value {
    Value::NativeFn(Rc::new(NativeFn {
        name,
        arity,
        kind: NativeKind::Blocking(func),
    }))
}

/// Build a `Value::NativeFn` for a steady-state *socket* entry — a
/// `Net` stream / datagram call the VM drives on the async-IO reactor
/// instead of a worker thread (see [`crate::vm::reactor`]). `func` runs
/// on the actor thread to validate arguments and build a
/// [`ReactorOp`]; the reactor's poll thread carries it out.
///
/// Only the `Net` module builds socket entries, so on `wasm32` (where
/// `Net` is unregistered) this helper is unreferenced — kept for a
/// uniform API.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn native_socket(
    name: &'static str,
    arity: Arity,
    func: fn(&[Value]) -> Result<ReactorOp, RuntimeError>,
) -> Value {
    Value::NativeFn(Rc::new(NativeFn {
        name,
        arity,
        kind: NativeKind::Socket(func),
    }))
}

/// The actor-thread step of a frame-yield `Park` native: take no
/// arguments and always park until the next host frame.
fn park_frame(_args: &[Value]) -> Result<WaitKind, RuntimeError> {
    Ok(WaitKind::NextFrame)
}

/// Build a *frame-wait* module entry for an embedder driving a frame
/// loop (e.g. purr's `GameTime.wait_frame`). Calling it from a green
/// thread cooperatively parks that thread until the host's next
/// [`Vm::drain_ready`](crate::vm::vm::Vm::drain_ready); siblings run
/// meanwhile. It raises if called outside a host frame drive, since
/// there is then no "next frame" to resume on. Hosts that do not drive
/// frames should not register one.
pub fn native_frame_wait(name: &'static str, arity: Arity) -> Value {
    Value::NativeFn(Rc::new(NativeFn {
        name,
        arity,
        kind: NativeKind::Park(park_frame),
    }))
}

/// Build a `Value::Object` from a list of (key, value) pairs in source
/// order. Keys are static `&'static str` since module entry names are
/// fixed; the IndexMap uses `Arc<str>` internally.
pub fn object(entries: &[(&'static str, Value)]) -> Value {
    let mut m: IndexMap<Arc<str>, Value> = IndexMap::with_capacity(entries.len());
    for (k, v) in entries {
        m.insert(Arc::from(*k), v.clone());
    }
    Value::Object(gc::alloc_object(m))
}

/// Build a `Value::Bytes` from an owned byte block, the byte-buffer
/// counterpart of [`object`]. A host native that produces raw bytes (a
/// pixel buffer's RGBA, a decoded blob) returns them with this; read them
/// back with [`Value::with_bytes`]. The `Vec` moves onto the managed heap,
/// so there is no copy beyond the allocation the caller already made.
pub fn bytes(data: Vec<u8>) -> Value {
    Value::Bytes(gc::alloc_bytes(data))
}
