//! `import '_NativeMap'` — Rust `Map` primitives.
//!
//! Backend for `stdlib/Map.tg`. `Map` is an arbitrary-keyed dictionary
//! backed by an insertion-ordered `IndexMap<MapKey, Value>`. Keys are
//! restricted to hashable primitives (null, bool, int, string); a
//! `Float` or collection key raises `InvalidKeyType`. Every operation
//! needs direct `RefCell` access, so the whole module is native.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, MapKey, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("new",     native("new",     Arity::Range(0, 1), m_new)),
        ("get",     native("get",     Arity::Exact(2),    m_get)),
        ("set",     native("set",     Arity::Exact(3),    m_set)),
        ("has",     native("has",     Arity::Exact(2),    m_has)),
        ("delete",  native("delete",  Arity::Exact(2),    m_delete)),
        ("keys",    native("keys",    Arity::Exact(1),    m_keys)),
        ("values",  native("values",  Arity::Exact(1),    m_values)),
        ("entries", native("entries", Arity::Exact(1),    m_entries)),
        ("size",    native("size",    Arity::Exact(1),    m_size)),
        ("clear",   native("clear",   Arity::Exact(1),    m_clear)),
    ])
}

fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

fn expect_map(
    v: &Value,
    label: &str,
) -> Result<Rc<RefCell<IndexMap<MapKey, Value>>>, RuntimeError> {
    match v {
        Value::Map(m) => Ok(m.clone()),
        other => Err(err(format!(
            "Map.{label}: expected Map, got {}", other.type_name()
        ))),
    }
}

fn new_map(entries: IndexMap<MapKey, Value>) -> Value {
    Value::Map(Rc::new(RefCell::new(entries)))
}

/// `new()` → empty Map. `new(obj)` copies an Object's entries.
/// `new(pairs)` builds from an Array of `[key, value]` pairs.
fn m_new(args: &[Value]) -> Result<Value, RuntimeError> {
    let mut map: IndexMap<MapKey, Value> = IndexMap::new();
    match args.first() {
        None => {}
        Some(Value::Object(o)) => {
            for (k, v) in o.borrow().iter() {
                map.insert(MapKey::Str(k.clone()), v.clone());
            }
        }
        Some(Value::Array(a)) => {
            for pair in a.borrow().iter() {
                let Value::Array(p) = pair else {
                    return Err(err(format!(
                        "Map.new: expected [key, value] pair, got {}",
                        pair.type_name()
                    )));
                };
                let p = p.borrow();
                if p.len() != 2 {
                    return Err(err(format!(
                        "Map.new: expected [key, value] pair of length 2, got length {}",
                        p.len()
                    )));
                }
                map.insert(MapKey::from_value(&p[0], 0)?, p[1].clone());
            }
        }
        Some(other) => {
            return Err(err(format!(
                "Map.new: expected an object or array of pairs, got {}",
                other.type_name()
            )));
        }
    }
    Ok(new_map(map))
}

/// `get(m, key)` → the value, or `null` if absent.
fn m_get(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "get")?;
    let key = MapKey::from_value(&args[1], 0)?;
    let value = map.borrow().get(&key).cloned().unwrap_or(Value::Null);
    Ok(value)
}

/// `set(m, key, value)` → inserts in place, returns `m`.
fn m_set(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "set")?;
    let key = MapKey::from_value(&args[1], 0)?;
    map.borrow_mut().insert(key, args[2].clone());
    Ok(args[0].clone())
}

/// `has(m, key)` → `true` if the key is present. O(1).
fn m_has(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "has")?;
    let key = MapKey::from_value(&args[1], 0)?;
    let present = map.borrow().contains_key(&key);
    Ok(Value::Bool(present))
}

/// `delete(m, key)` → removes the key, returns `true` if it was
/// present. `shift_remove` preserves the insertion order of the rest.
fn m_delete(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "delete")?;
    let key = MapKey::from_value(&args[1], 0)?;
    let removed = map.borrow_mut().shift_remove(&key).is_some();
    Ok(Value::Bool(removed))
}

fn array(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}

/// `keys(m)` → Array of keys in insertion order.
fn m_keys(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "keys")?;
    let keys = map.borrow().keys().map(|k| Value::from(k.clone())).collect();
    Ok(array(keys))
}

/// `values(m)` → Array of values in insertion order.
fn m_values(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "values")?;
    let values = map.borrow().values().cloned().collect();
    Ok(array(values))
}

/// `entries(m)` → Array of `[key, value]` pairs in insertion order.
fn m_entries(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "entries")?;
    let entries = map
        .borrow()
        .iter()
        .map(|(k, v)| array(vec![Value::from(k.clone()), v.clone()]))
        .collect();
    Ok(array(entries))
}

/// `size(m)` → entry count.
fn m_size(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "size")?;
    let n = map.borrow().len() as i64;
    Ok(Value::Int(n))
}

/// `clear(m)` → empties `m` in place, returns `m`.
fn m_clear(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = expect_map(&args[0], "clear")?;
    map.borrow_mut().clear();
    Ok(args[0].clone())
}
