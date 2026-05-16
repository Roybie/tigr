//! `import '_NativeArray'` — Rust array primitives.
//!
//! Backend for `stdlib/Array.tg`. Provides in-place mutation that the
//! pure-tigr `+` / spread forms cannot express without copying the
//! whole array: `arr + x` and `[...arr, x]` both clone `arr`, so
//! building an array by repeated append is O(n^2). `push`/`extend`
//! mutate the array behind its `Rc<RefCell<..>>` directly, the same
//! way the `for[]` collecting opcode does — O(1) amortized / O(m).
//!
//! Element *removal* lives here for the same reason: pure tigr can
//! grow an array but has no way to shrink one. `pop`/`shift`/`remove`/
//! `clear` (plus the front/middle inserts `unshift`/`insert`) all
//! mutate the backing `Vec` in place.

use std::cell::RefCell;
use std::rc::Rc;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("push",    native("push",    Arity::Exact(2),    a_push)),
        ("extend",  native("extend",  Arity::Exact(2),    a_extend)),
        ("pop",     native("pop",     Arity::Exact(1),    a_pop)),
        ("shift",   native("shift",   Arity::Exact(1),    a_shift)),
        ("unshift", native("unshift", Arity::Exact(2),    a_unshift)),
        ("insert",  native("insert",  Arity::Exact(3),    a_insert)),
        ("remove",  native("remove",  Arity::Range(2, 3), a_remove)),
        ("clear",   native("clear",   Arity::Exact(1),    a_clear)),
    ])
}

fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

fn expect_array(
    v: &Value,
    label: &str,
) -> Result<Rc<RefCell<Vec<Value>>>, RuntimeError> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        other => Err(err(format!(
            "Array.{label}: expected Array, got {}",
            other.type_name()
        ))),
    }
}

fn expect_int(v: &Value, label: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(err(format!(
            "Array.{label}: expected Int, got {}",
            other.type_name()
        ))),
    }
}

/// Resolve a possibly-negative index against `len`. A negative index
/// counts back from the end (`-1` ⇒ last). The result may still be
/// out of `[0, len]` — callers decide how to clamp or reject it.
fn resolve_index(idx: i64, len: usize) -> i64 {
    if idx < 0 { idx + len as i64 } else { idx }
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

/// `pop(arr)` — remove and return the last element. `null` if `arr`
/// is empty.
fn a_pop(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "pop")?;
    let popped = arr.borrow_mut().pop();
    Ok(popped.unwrap_or(Value::Null))
}

/// `shift(arr)` — remove and return the first element. `null` if
/// `arr` is empty.
fn a_shift(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "shift")?;
    let mut arr = arr.borrow_mut();
    if arr.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(arr.remove(0))
    }
}

/// `unshift(arr, value)` — prepend `value`. Returns `arr`.
fn a_unshift(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "unshift")?;
    arr.borrow_mut().insert(0, args[1].clone());
    Ok(args[0].clone())
}

/// `insert(arr, index, value)` — insert `value` at `index`. A negative
/// `index` counts from the end; the resolved index is clamped to
/// `[0, #arr]`. Returns `arr`.
fn a_insert(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "insert")?;
    let idx = expect_int(&args[1], "insert")?;
    let mut arr = arr.borrow_mut();
    let len = arr.len();
    let at = resolve_index(idx, len).clamp(0, len as i64) as usize;
    arr.insert(at, args[2].clone());
    Ok(args[0].clone())
}

/// `remove(arr, index)` — remove and return the single element at
/// `index`; `null` if `index` is out of range. A negative `index`
/// counts from the end.
///
/// `remove(arr, start, count)` — remove and return `count` elements
/// starting at `start`, as a new array. A negative `start` counts from
/// the end; `start` is clamped to `[0, #arr]` and `count` to
/// `[0, #arr - start]`.
fn a_remove(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "remove")?;
    let mut arr = arr.borrow_mut();
    let len = arr.len();

    if args.len() == 2 {
        let idx = expect_int(&args[1], "remove")?;
        let at = resolve_index(idx, len);
        if at < 0 || at >= len as i64 {
            Ok(Value::Null)
        } else {
            Ok(arr.remove(at as usize))
        }
    } else {
        let start = expect_int(&args[1], "remove")?;
        let count = expect_int(&args[2], "remove")?;
        let start = resolve_index(start, len).clamp(0, len as i64) as usize;
        let count = count.clamp(0, (len - start) as i64) as usize;
        let removed: Vec<Value> = arr.drain(start..start + count).collect();
        Ok(Value::Array(Rc::new(RefCell::new(removed))))
    }
}

/// `clear(arr)` — remove every element in place. Returns `arr`.
fn a_clear(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "clear")?;
    arr.borrow_mut().clear();
    Ok(args[0].clone())
}
