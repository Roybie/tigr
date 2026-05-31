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
pub mod io;
pub mod json;
pub mod local_channel;
pub mod map;
pub mod math;
// `Net` (sockets/TLS) builds only on reactor-capable targets: it
// depends on the real `socket`/`reactor` API, which is stubbed on both
// `wasm32` (no browser sockets) and `windows` (IOCP gap, Tier 1). `Os`
// (processes/env) below stays on every native target — it is portable.
#[cfg(all(not(target_arch = "wasm32"), not(windows)))]
pub mod net;
pub mod object;
#[cfg(not(target_arch = "wasm32"))]
pub mod os;
pub mod path;
pub mod random;
pub mod set;
pub mod string;
pub mod time;

use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::vm::error::RuntimeError;
use crate::vm::gc;
use crate::vm::offload::BlockingJob;
use crate::vm::socket::ReactorOp;
use crate::vm::value::{Arity, NativeFn, NativeKind, Value, WaitKind};

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
        // `Os` (processes/env) and the cross-actor `_NativeChannel` use
        // `std::process` and OS threads — portable to every native
        // target, so they stay enabled on Windows. Only `wasm32` (no
        // threads/processes in the playground) drops them.
        #[cfg(not(target_arch = "wasm32"))]
        "Os" => Some(os::module()),
        #[cfg(not(target_arch = "wasm32"))]
        "_NativeChannel" => Some(channel::module()),
        // `Net` (sockets/TLS) needs the async reactor, unavailable on
        // `wasm32` and on Windows Tier 1. Unregistered there so `import
        // 'Net'` fails with a clean, catchable "no module of that name"
        // error rather than panicking.
        #[cfg(all(not(target_arch = "wasm32"), not(windows)))]
        "Net" => Some(net::module()),
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
/// Only the `Net` module builds socket entries, so on targets where
/// `Net` is unregistered (`wasm32` and `windows`) this helper is
/// unreferenced — kept for a uniform API.
#[cfg_attr(any(target_arch = "wasm32", windows), allow(dead_code))]
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
