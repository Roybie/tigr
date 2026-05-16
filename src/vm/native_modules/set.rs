//! `import '_NativeSet'` — Rust `Set` primitives.
//!
//! Backend for `stdlib/Set.tg`. `Set` is an insertion-ordered
//! collection of unique values backed by `IndexSet<MapKey>`. Elements
//! share `Map`'s key restriction: hashable primitives only.

use indexmap::IndexSet;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc::{self, GcRef, SetKind};
use crate::vm::value::{Arity, MapKey, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("new",          native("new",          Arity::Range(0, 1), s_new)),
        ("add",          native("add",          Arity::Exact(2),    s_add)),
        ("has",          native("has",          Arity::Exact(2),    s_has)),
        ("delete",       native("delete",       Arity::Exact(2),    s_delete)),
        ("items",        native("items",        Arity::Exact(1),    s_items)),
        ("size",         native("size",         Arity::Exact(1),    s_size)),
        ("clear",        native("clear",        Arity::Exact(1),    s_clear)),
        ("union",        native("union",        Arity::Exact(2),    s_union)),
        ("intersection", native("intersection", Arity::Exact(2),    s_intersection)),
        ("difference",   native("difference",   Arity::Exact(2),    s_difference)),
    ])
}

fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

fn expect_set(
    v: &Value,
    label: &str,
) -> Result<GcRef<SetKind>, RuntimeError> {
    match v {
        Value::Set(s) => Ok(*s),
        other => Err(err(format!(
            "Set.{label}: expected Set, got {}", other.type_name()
        ))),
    }
}

fn new_set(elems: IndexSet<MapKey>) -> Value {
    Value::Set(gc::alloc_set(elems))
}

/// `new()` → empty Set. `new(array)` builds from an Array's elements.
fn s_new(args: &[Value]) -> Result<Value, RuntimeError> {
    let mut set: IndexSet<MapKey> = IndexSet::new();
    match args.first() {
        None => {}
        Some(Value::Array(a)) => {
            for elem in a.borrow().iter() {
                set.insert(MapKey::from_value(elem, 0)?);
            }
        }
        Some(other) => {
            return Err(err(format!(
                "Set.new: expected an array, got {}", other.type_name()
            )));
        }
    }
    Ok(new_set(set))
}

/// `add(s, x)` → inserts `x` in place, returns `s`.
fn s_add(args: &[Value]) -> Result<Value, RuntimeError> {
    let set = expect_set(&args[0], "add")?;
    let elem = MapKey::from_value(&args[1], 0)?;
    set.borrow_mut().insert(elem);
    Ok(args[0].clone())
}

/// `has(s, x)` → `true` if `x` is a member. O(1).
fn s_has(args: &[Value]) -> Result<Value, RuntimeError> {
    let set = expect_set(&args[0], "has")?;
    let elem = MapKey::from_value(&args[1], 0)?;
    let present = set.borrow().contains(&elem);
    Ok(Value::Bool(present))
}

/// `delete(s, x)` → removes `x`, returns `true` if it was present.
/// `shift_remove` preserves the insertion order of the rest.
fn s_delete(args: &[Value]) -> Result<Value, RuntimeError> {
    let set = expect_set(&args[0], "delete")?;
    let elem = MapKey::from_value(&args[1], 0)?;
    let removed = set.borrow_mut().shift_remove(&elem);
    Ok(Value::Bool(removed))
}

/// `items(s)` → Array of elements in insertion order.
fn s_items(args: &[Value]) -> Result<Value, RuntimeError> {
    let set = expect_set(&args[0], "items")?;
    let items = set.borrow().iter().map(|e| Value::from(e.clone())).collect();
    Ok(Value::Array(gc::alloc_array(items)))
}

/// `size(s)` → element count.
fn s_size(args: &[Value]) -> Result<Value, RuntimeError> {
    let set = expect_set(&args[0], "size")?;
    let n = set.borrow().len() as i64;
    Ok(Value::Int(n))
}

/// `clear(s)` → empties `s` in place, returns `s`.
fn s_clear(args: &[Value]) -> Result<Value, RuntimeError> {
    let set = expect_set(&args[0], "clear")?;
    set.borrow_mut().clear();
    Ok(args[0].clone())
}

/// `union(a, b)` → a fresh Set with every element of both, `a`'s order
/// first.
fn s_union(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = expect_set(&args[0], "union")?;
    let b = expect_set(&args[1], "union")?;
    let out: IndexSet<MapKey> =
        a.borrow().union(&b.borrow()).cloned().collect();
    Ok(new_set(out))
}

/// `intersection(a, b)` → a fresh Set with elements present in both.
fn s_intersection(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = expect_set(&args[0], "intersection")?;
    let b = expect_set(&args[1], "intersection")?;
    let out: IndexSet<MapKey> =
        a.borrow().intersection(&b.borrow()).cloned().collect();
    Ok(new_set(out))
}

/// `difference(a, b)` → a fresh Set with `a`'s elements not in `b`.
fn s_difference(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = expect_set(&args[0], "difference")?;
    let b = expect_set(&args[1], "difference")?;
    let out: IndexSet<MapKey> =
        a.borrow().difference(&b.borrow()).cloned().collect();
    Ok(new_set(out))
}
