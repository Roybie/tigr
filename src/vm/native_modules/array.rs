//! `import '_NativeArray'` — Rust array primitives.
//!
//! Backend for `stdlib/Array.tg`. Provides in-place mutation that the
//! pure-tigr `+` / spread forms cannot express without copying the
//! whole array: `arr + x` and `[...arr, x]` both clone `arr`, so
//! building an array by repeated append is O(n^2). `push`/`extend`
//! mutate the array behind its `Rc<RefCell<..>>` directly, the same
//! way the `for[]` collecting opcode does — O(1) amortized / O(m).

use std::cell::RefCell;
use std::rc::Rc;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("push",   native("push",   Arity::Exact(2), a_push)),
        ("extend", native("extend", Arity::Exact(2), a_extend)),
    ])
}

fn expect_array(
    v: &Value,
    label: &str,
) -> Result<Rc<RefCell<Vec<Value>>>, RuntimeError> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(format!(
                "Array.{label}: expected Array, got {}",
                other.type_name()
            ).into())),
            0,
        )),
    }
}

/// `push(arr, value)` — append `value` to `arr` in place. Returns
/// `arr` (the same reference) so it reads as an expression.
fn a_push(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "push")?;
    arr.borrow_mut().push(args[1].clone());
    Ok(args[0].clone())
}

/// `extend(arr, other)` — append every element of `other` to `arr` in
/// place. Returns `arr`. `other`'s contents are snapshotted first so a
/// self-extend (`extend(a, a)`) doesn't double-borrow the cell.
fn a_extend(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "extend")?;
    let other = expect_array(&args[1], "extend")?;
    let items: Vec<Value> = other.borrow().clone();
    arr.borrow_mut().extend(items);
    Ok(args[0].clone())
}
