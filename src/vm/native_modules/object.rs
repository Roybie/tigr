//! `import '_NativeObject'` — Rust `Object` primitives.
//!
//! Backend for `stdlib/Object.tg`. Provides only `has`: a membership
//! test that distinguishes a missing key from a present `null` value
//! in O(1), which indexing (`obj[k]` returns `null` for both) cannot.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("has", native("has", Arity::Exact(2), o_has)),
    ])
}

fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

/// `has(obj, key)` → `true` if `obj` contains `key`. O(1) — unlike the
/// pure-tigr key scan it replaces.
fn o_has(args: &[Value]) -> Result<Value, RuntimeError> {
    let Value::Object(o) = &args[0] else {
        return Err(err(format!(
            "Object.has: expected Object, got {}", args[0].type_name()
        )));
    };
    let Value::Str(key) = &args[1] else {
        return Err(err(format!(
            "Object.has: expected a string key, got {}", args[1].type_name()
        )));
    };
    Ok(Value::Bool(o.borrow().contains_key(key)))
}
