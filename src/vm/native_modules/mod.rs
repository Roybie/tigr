//! Native (Rust-implemented) modules exposed via `import 'Name'`.
//!
//! The compiler emits bare-name `import 'X'` (no path separators or
//! `.`) as a raw constant; at runtime the Import opcode consults
//! [`resolve`] before falling back to the filesystem. Each call to a
//! module builder rebuilds its Object — the Vm-side cache in
//! `module_cache` ensures a given module is built at most once per
//! `Vm` run.

pub mod io;
pub mod json;
pub mod math;
pub mod os;
pub mod string;
pub mod time;

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::RuntimeError;
use crate::vm::value::{Arity, NativeFn, Value};

/// Look up a bare-name module. Returns `None` if no native module of
/// that name exists — callers should then fall back to filesystem
/// resolution or surface an "module not found" error.
pub fn resolve(name: &str) -> Option<Value> {
    match name {
        "IO" => Some(io::module()),
        "Os" => Some(os::module()),
        "Time" => Some(time::module()),
        "JSON" => Some(json::module()),
        // Underscore-prefixed names are backends for source stdlibs
        // (Math.tg / String.tg wrap these). User code can also import
        // them directly if it wants the raw primitives.
        "_NativeMath" => Some(math::module()),
        "_NativeString" => Some(string::module()),
        _ => None,
    }
}

/// Build a `Value::NativeFn` for a module entry.
pub(crate) fn native(
    name: &'static str,
    arity: Arity,
    func: fn(&[Value]) -> Result<Value, RuntimeError>,
) -> Value {
    Value::NativeFn(Rc::new(NativeFn { name, arity, func }))
}

/// Build a `Value::Object` from a list of (key, value) pairs in source
/// order. Keys are static `&'static str` since module entry names are
/// fixed; the IndexMap uses `Rc<str>` internally.
pub(crate) fn object(entries: &[(&'static str, Value)]) -> Value {
    let mut m: IndexMap<Rc<str>, Value> = IndexMap::with_capacity(entries.len());
    for (k, v) in entries {
        m.insert(Rc::from(*k), v.clone());
    }
    Value::Object(Rc::new(RefCell::new(m)))
}
