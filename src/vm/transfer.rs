//! Transfer encoding — the `Send`-able representation of a value that
//! crosses an actor / channel heap boundary (v0.14 concurrency).
//!
//! A [`Value`] cannot cross threads: it carries [`GcRef`] handles into
//! a *thread-local* heap and `Rc` pointers. [`encode`] walks a value on
//! the sender's thread (where dereferencing its handles is valid) into
//! an owned, `Send` [`Transfer`]; [`decode`] rebuilds that into the
//! receiver thread's heap, allocating fresh handles.
//!
//! Sendable, deep-copied: the primitives, `Str`, `Bytes`, `Range`,
//! `BigInt`, and the four collections. A closure is sendable iff every
//! captured upvalue is itself sendable — its compiled code rides along
//! as a shared `Arc<Function>`. Not sendable: an iterator, a native
//! function, or a closure with still-open captures — these raise a
//! catchable `not_sendable`. A cyclic collection raises `cycle`.

// The encode/decode API and `Transfer` types are consumed by the
// `Channel` module (Phase 3) and the `spawn` opcode (Phase 4). Until
// then they are dead from the binary's view but exercised by tests.
#![allow(dead_code)]

use std::rc::Rc;
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use num_bigint::BigInt;

use crate::vm::channel::ChannelHandle;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::file_handle::FileHandle;
use crate::vm::socket::SocketHandle;
use crate::vm::task::TaskHandle;
use crate::vm::gc::{
    self, ArrayKind, ClosureKind, GcRef, MapKind, ObjectKind, UpvalueKind,
};
use crate::vm::value::{Closure, Function, MapKey, RangeData, Upvalue, Value};

/// A primitive `Map`/`Set` key in owned form (no `Rc`).
#[derive(Clone, Debug, PartialEq)]
pub enum TransferKey {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
}

/// The `Send`-able mirror of a [`Value`]. Built by [`encode`], consumed
/// by [`decode`]. Carries no `GcRef` and no heap-bound `Rc`, so it is
/// safe to move between threads.
pub enum Transfer {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    BigInt(Box<BigInt>),
    Bytes(Vec<u8>),
    Range { from: i64, to: i64, step: i64, inclusive: bool },
    Array(Vec<Transfer>),
    Object(Vec<(String, Transfer)>),
    Map(Vec<(TransferKey, Transfer)>),
    Set(Vec<TransferKey>),
    /// A sendable closure: shared compiled code plus each captured
    /// upvalue, itself transfer-encoded.
    Closure { function: Arc<Function>, upvalues: Vec<Transfer> },
    /// A channel handle — `Arc`-backed and `Send`, so it crosses the
    /// boundary by handle clone, not deep-copy.
    Channel(ChannelHandle),
    /// A task handle — likewise `Arc`-backed; crosses by clone.
    Task(TaskHandle),
    /// A socket handle — `Arc`-backed and `Send`; crosses by clone, so
    /// a `spawn`ed connection handler can capture the accepted socket.
    Socket(SocketHandle),
    /// A file handle (`IO.open`) — `Arc`-backed and `Send`; crosses by
    /// clone, so a `spawn`ed worker can read a file the parent opened.
    File(FileHandle),
}

/// A worker actor's error, rendered to `Send`-able form so it can cross
/// back to the parent — the worker's `SourceMap` is `Rc` and cannot
/// itself be sent. Used by `join` / `parallel[]` (v0.14 Phase 4).
pub struct TransferError {
    /// Stable snake-case tag (`RuntimeErrorKind::kind_tag`).
    pub kind_tag: String,
    /// One-line error message.
    pub message: String,
    /// The worker's full rendered stack trace + snippet.
    pub rendered_trace: String,
    /// If the worker error was `raise <value>`, that value encoded;
    /// `None` for a built-in error.
    pub raised: Option<Transfer>,
}

impl std::fmt::Debug for Transfer {
    /// Variant-name only — `Function` is not `Debug`, and this exists
    /// just so `Result<Transfer, _>::unwrap_err` works in tests.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Transfer::Null => "Null",
            Transfer::Bool(_) => "Bool",
            Transfer::Int(_) => "Int",
            Transfer::Float(_) => "Float",
            Transfer::Str(_) => "Str",
            Transfer::BigInt(_) => "BigInt",
            Transfer::Bytes(_) => "Bytes",
            Transfer::Range { .. } => "Range",
            Transfer::Array(_) => "Array",
            Transfer::Object(_) => "Object",
            Transfer::Map(_) => "Map",
            Transfer::Set(_) => "Set",
            Transfer::Closure { .. } => "Closure",
            Transfer::Channel(_) => "Channel",
            Transfer::Task(_) => "Task",
            Transfer::Socket(_) => "Socket",
            Transfer::File(_) => "File",
        };
        write!(f, "Transfer::{name}")
    }
}

fn cycle() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Cycle, 0)
}

fn not_sendable(what: &str) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::NotSendable(what.to_string()), 0)
}

/// Ancestor-path sets for cycle detection, one per managed kind a cycle
/// can route through. `Set` is excluded — its keys are primitives, so a
/// set can never contain a collection and never forms a cycle.
#[derive(Default)]
struct CycleGuard {
    arrays: Vec<GcRef<ArrayKind>>,
    objects: Vec<GcRef<ObjectKind>>,
    maps: Vec<GcRef<MapKind>>,
    closures: Vec<GcRef<ClosureKind>>,
}

/// Encode a [`Value`] into a `Send`-able [`Transfer`]. Runs on the
/// owning thread. Raises `cycle` on a self-referential collection and
/// `not_sendable` on a value that cannot be deep-copied.
pub fn encode(v: &Value) -> Result<Transfer, RuntimeError> {
    encode_inner(v, &mut CycleGuard::default())
}

fn key_to_transfer(k: &MapKey) -> TransferKey {
    match k {
        MapKey::Null => TransferKey::Null,
        MapKey::Bool(b) => TransferKey::Bool(*b),
        MapKey::Int(n) => TransferKey::Int(*n),
        MapKey::Str(s) => TransferKey::Str(s.to_string()),
    }
}

fn encode_inner(v: &Value, g: &mut CycleGuard) -> Result<Transfer, RuntimeError> {
    Ok(match v {
        Value::Null => Transfer::Null,
        Value::Bool(b) => Transfer::Bool(*b),
        Value::Int(n) => Transfer::Int(*n),
        Value::Float(x) => Transfer::Float(*x),
        Value::Str(s) => Transfer::Str(s.to_string()),
        Value::BigInt(n) => Transfer::BigInt(Box::new((**n).clone())),
        Value::Range(r) => Transfer::Range {
            from: r.from,
            to: r.to,
            step: r.step,
            inclusive: r.inclusive,
        },
        Value::Bytes(b) => Transfer::Bytes(b.borrow().clone()),
        Value::Array(a) => {
            if g.arrays.contains(a) {
                return Err(cycle());
            }
            g.arrays.push(*a);
            // Snapshot then release the borrow before recursing, so a
            // child's own heap access never races this guard.
            let items: Vec<Value> = a.borrow().clone();
            let mut out = Vec::with_capacity(items.len());
            for it in &items {
                out.push(encode_inner(it, g)?);
            }
            g.arrays.pop();
            Transfer::Array(out)
        }
        Value::Object(o) => {
            if g.objects.contains(o) {
                return Err(cycle());
            }
            g.objects.push(*o);
            let pairs: Vec<(Arc<str>, Value)> = o
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut out = Vec::with_capacity(pairs.len());
            for (k, v) in &pairs {
                out.push((k.to_string(), encode_inner(v, g)?));
            }
            g.objects.pop();
            Transfer::Object(out)
        }
        Value::Map(m) => {
            if g.maps.contains(m) {
                return Err(cycle());
            }
            g.maps.push(*m);
            let pairs: Vec<(MapKey, Value)> = m
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut out = Vec::with_capacity(pairs.len());
            for (k, v) in &pairs {
                out.push((key_to_transfer(k), encode_inner(v, g)?));
            }
            g.maps.pop();
            Transfer::Map(out)
        }
        Value::Set(s) => {
            let keys: Vec<TransferKey> =
                s.borrow().iter().map(key_to_transfer).collect();
            Transfer::Set(keys)
        }
        Value::Function(c) => {
            if g.closures.contains(c) {
                return Err(cycle());
            }
            g.closures.push(*c);
            let (function, cells) = {
                let cl = c.borrow();
                (cl.function.clone(), cl.upvalues.clone())
            };
            let mut ups = Vec::with_capacity(cells.len());
            for cell in &cells {
                let captured = match &*cell.borrow() {
                    Upvalue::Closed(val) => val.clone(),
                    Upvalue::Open { .. } => {
                        return Err(not_sendable(
                            "a function with live captured variables",
                        ));
                    }
                };
                ups.push(encode_inner(&captured, g)?);
            }
            g.closures.pop();
            Transfer::Closure { function, upvalues: ups }
        }
        Value::Channel(h) => Transfer::Channel(h.clone()),
        Value::Task(h) => Transfer::Task(h.clone()),
        Value::Socket(h) => Transfer::Socket(h.clone()),
        Value::File(h) => Transfer::File(h.clone()),
        Value::Iter(_) => return Err(not_sendable("an iterator")),
        Value::Generator(_) => return Err(not_sendable("a generator")),
        Value::GreenHandle(_) => return Err(not_sendable("a green thread")),
        Value::LocalChannel(_) => {
            return Err(not_sendable("a local channel"))
        }
        Value::NativeFn(_) => return Err(not_sendable("a native function")),
    })
}

fn key_from_transfer(k: TransferKey) -> MapKey {
    match k {
        TransferKey::Null => MapKey::Null,
        TransferKey::Bool(b) => MapKey::Bool(b),
        TransferKey::Int(n) => MapKey::Int(n),
        TransferKey::Str(s) => MapKey::Str(s.into()),
    }
}

/// Rebuild a [`Value`] from a [`Transfer`] into the *current* thread's
/// heap. Allocates fresh `GcRef` handles for every collection.
pub fn decode(t: Transfer) -> Value {
    match t {
        Transfer::Null => Value::Null,
        Transfer::Bool(b) => Value::Bool(b),
        Transfer::Int(n) => Value::Int(n),
        Transfer::Float(x) => Value::Float(x),
        Transfer::Str(s) => Value::Str(s.into()),
        Transfer::BigInt(n) => Value::BigInt(Rc::new(*n)),
        Transfer::Bytes(b) => Value::Bytes(gc::alloc_bytes(b)),
        Transfer::Range { from, to, step, inclusive } => {
            Value::Range(Rc::new(RangeData { from, to, step, inclusive }))
        }
        Transfer::Array(items) => {
            let v: Vec<Value> = items.into_iter().map(decode).collect();
            Value::Array(gc::alloc_array(v))
        }
        Transfer::Object(pairs) => {
            let mut m: IndexMap<Arc<str>, Value> = IndexMap::new();
            for (k, v) in pairs {
                m.insert(Arc::from(k.as_str()), decode(v));
            }
            Value::Object(gc::alloc_object(m))
        }
        Transfer::Map(pairs) => {
            let mut m: IndexMap<MapKey, Value> = IndexMap::new();
            for (k, v) in pairs {
                m.insert(key_from_transfer(k), decode(v));
            }
            Value::Map(gc::alloc_map(m))
        }
        Transfer::Set(keys) => {
            let mut s: IndexSet<MapKey> = IndexSet::new();
            for k in keys {
                s.insert(key_from_transfer(k));
            }
            Value::Set(gc::alloc_set(s))
        }
        Transfer::Closure { function, upvalues } => {
            let cells: Vec<GcRef<UpvalueKind>> = upvalues
                .into_iter()
                .map(|u| gc::alloc_upvalue(Upvalue::Closed(decode(u))))
                .collect();
            Value::Function(gc::alloc_closure(Closure { function, upvalues: cells }))
        }
        Transfer::Channel(h) => Value::Channel(h),
        Transfer::Task(h) => Value::Task(h),
        Transfer::Socket(h) => Value::Socket(h),
        Transfer::File(h) => Value::File(h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::chunk::Chunk;
    use crate::vm::value::{Arity, IterState, NativeFn, NativeKind};

    /// `Transfer` must be `Send` — it is the payload that crosses
    /// actor threads. A compile-time check.
    #[test]
    fn transfer_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Transfer>();
        assert_send::<TransferError>();
    }

    fn roundtrip(v: Value) -> Value {
        decode(encode(&v).expect("should encode"))
    }

    #[test]
    fn primitives_roundtrip() {
        for v in [
            Value::Null,
            Value::Bool(true),
            Value::Int(-7),
            Value::Float(3.5),
            Value::Str("héllo".into()),
        ] {
            assert_eq!(roundtrip(v.clone()), v);
        }
    }

    #[test]
    fn bigint_and_range_roundtrip() {
        let big = Value::BigInt(Rc::new(BigInt::from(1_i64) << 200));
        assert_eq!(roundtrip(big.clone()), big);
        let r = Value::Range(Rc::new(RangeData {
            from: 0,
            to: 10,
            step: 2,
            inclusive: true,
        }));
        assert_eq!(roundtrip(r.clone()), r);
    }

    #[test]
    fn collections_roundtrip() {
        let bytes = Value::Bytes(gc::alloc_bytes(vec![1, 2, 3]));
        assert_eq!(roundtrip(bytes.clone()), bytes);

        let arr = Value::Array(gc::alloc_array(vec![
            Value::Int(1),
            Value::Str("two".into()),
            Value::Array(gc::alloc_array(vec![Value::Bool(false)])),
        ]));
        assert_eq!(roundtrip(arr.clone()), arr);

        let mut obj = IndexMap::new();
        obj.insert(Arc::from("k"), Value::Int(9));
        let obj = Value::Object(gc::alloc_object(obj));
        assert_eq!(roundtrip(obj.clone()), obj);

        let mut map = IndexMap::new();
        map.insert(MapKey::Int(1), Value::Str("a".into()));
        let map = Value::Map(gc::alloc_map(map));
        assert_eq!(roundtrip(map.clone()), map);

        let mut set = IndexSet::new();
        set.insert(MapKey::Str("x".into()));
        let set = Value::Set(gc::alloc_set(set));
        assert_eq!(roundtrip(set.clone()), set);
    }

    #[test]
    fn decode_allocates_a_distinct_handle() {
        // A decoded array must be a fresh heap object, not the original.
        let orig = gc::alloc_array(vec![Value::Int(1)]);
        let copy = decode(encode(&Value::Array(orig)).unwrap());
        match copy {
            Value::Array(c) => assert!(c != orig, "decode must deep-copy"),
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn closure_with_closed_upvalues_roundtrips() {
        let f = Arc::new(Function {
            arity: 0,
            has_rest: false,
            chunk: Chunk::new(),
            upvalues: Vec::new(),
            is_generator: false,
            name: Some("worker".into()),
        });
        let up = gc::alloc_upvalue(Upvalue::Closed(Value::Int(42)));
        let cl = gc::alloc_closure(Closure { function: f, upvalues: vec![up] });
        let decoded = decode(encode(&Value::Function(cl)).unwrap());
        match decoded {
            Value::Function(c) => {
                let c = c.borrow();
                assert_eq!(c.upvalues.len(), 1);
                match &*c.upvalues[0].borrow() {
                    Upvalue::Closed(Value::Int(n)) => assert_eq!(*n, 42),
                    _ => panic!("expected closed upvalue 42"),
                }
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn closure_with_open_upvalue_is_not_sendable() {
        let f = Arc::new(Function {
            arity: 0,
            has_rest: false,
            chunk: Chunk::new(),
            upvalues: Vec::new(),
            is_generator: false,
            name: None,
        });
        let up = gc::alloc_upvalue(Upvalue::Open { owner: 0, slot: 0 });
        let cl = gc::alloc_closure(Closure { function: f, upvalues: vec![up] });
        let err = encode(&Value::Function(cl)).unwrap_err();
        assert_eq!(err.kind.kind_tag(), "not_sendable");
    }

    #[test]
    fn iterator_and_native_fn_are_not_sendable() {
        let it = Value::Iter(gc::alloc_iter(IterState::Range {
            current: 0,
            to: 3,
            step: 1,
            inclusive: false,
            index: 0,
        }));
        assert_eq!(
            encode(&it).unwrap_err().kind.kind_tag(),
            "not_sendable"
        );

        fn noop(_: &[Value]) -> Result<Value, RuntimeError> {
            Ok(Value::Null)
        }
        let nf = Value::NativeFn(Rc::new(NativeFn {
            name: "noop",
            arity: Arity::Exact(0),
            kind: NativeKind::Pure(noop),
        }));
        assert_eq!(
            encode(&nf).unwrap_err().kind.kind_tag(),
            "not_sendable"
        );
    }

    #[test]
    fn cyclic_array_raises_cycle() {
        let a = gc::alloc_array(vec![Value::Int(1)]);
        a.borrow_mut().push(Value::Array(a)); // a contains itself
        let err = encode(&Value::Array(a)).unwrap_err();
        assert_eq!(err.kind.kind_tag(), "cycle");
    }
}
