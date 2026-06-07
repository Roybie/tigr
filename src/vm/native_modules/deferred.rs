//! `Deferred` — first-class deferred results.
//!
//! `Deferred.new()` mints a write-once result; `join(d)` waits on it;
//! `Deferred.resolve(d, v)` / `Deferred.reject(d, e)` settle it, waking
//! every awaiter (a deferred may have many — `resolve`/`reject`
//! broadcast). Both return `true` if they settled it, `false` if it was
//! already settled.
//!
//! Only `new` runs here: it allocates a fresh `Deferred` on the heap.
//! `resolve`/`reject`/`join` need the scheduler (to wake or park
//! coroutines), so the VM intercepts those before these entries run (see
//! `deferred_resolve_target` / `deferred_join_target` in `vm.rs`). The
//! `resolve`/`reject` bodies below are reached only on a type error — a
//! non-deferred first argument, which the interception does not match —
//! and exist so the module entries carry the right name and arity.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::scheduler::Deferred;
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("new", native("new", Arity::Exact(0), d_new)),
        ("resolve", native("resolve", Arity::Exact(2), d_resolve)),
        ("reject", native("reject", Arity::Exact(2), d_reject)),
    ])
}

/// `Deferred.new()` — mint an unsettled deferred on the managed heap.
fn d_new(_args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Deferred(gc::alloc_deferred(Deferred { result: None })))
}

fn not_a_deferred(name: &str, got: &Value) -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::TypeMismatch(format!(
            "Deferred.{name} expects a deferred, got {}",
            got.type_name()
        )),
        0,
    )
}

/// Reached only when `resolve`'s first argument is not a deferred — the
/// VM intercepts the real case (a `Value::Deferred` first arg).
fn d_resolve(args: &[Value]) -> Result<Value, RuntimeError> {
    Err(not_a_deferred("resolve", &args[0]))
}

/// Reached only when `reject`'s first argument is not a deferred.
fn d_reject(args: &[Value]) -> Result<Value, RuntimeError> {
    Err(not_a_deferred("reject", &args[0]))
}
