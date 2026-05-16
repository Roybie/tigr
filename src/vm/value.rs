//! Runtime values.
//!
//! The mutable / potentially-cyclic types — `Array`, `Object`, `Map`,
//! `Set`, `Iter`, `Closure`, and upvalue cells — are managed by the
//! v0.10 tracing collector ([`crate::vm::gc`]): a `Value` carries a
//! small `Copy` [`GcRef`] handle into the thread-local heap rather than
//! the data itself. `Str`, `Range`, and `NativeFn` are immutable and
//! acyclic, so they stay plain `Rc` — `Rc` reclaims acyclic data fine
//! and the collector skips them.

use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;

use crate::vm::chunk::Chunk;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc::{
    ArrayKind, ClosureKind, GcRef, IterKind, MapKind, ObjectKind, SetKind,
    UpvalueKind,
};

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),

    // Phase 3+ — GC-managed (see `crate::vm::gc`).
    Array(GcRef<ArrayKind>),
    Object(GcRef<ObjectKind>),

    // v0.9 — arbitrary-keyed dictionary / set. Keys are restricted to
    // hashable primitives (see `MapKey`); insertion-ordered like Object.
    Map(GcRef<MapKind>),
    Set(GcRef<SetKind>),

    // Phase 5+
    Range(Rc<RangeData>),
    /// Internal iterator state for `for`. GC-managed so the position
    /// advances in place while the value lives on the stack. Never
    /// observable from tigr code.
    Iter(GcRef<IterKind>),

    // Phase 4+ — runtime callable. Plain functions with no captured
    // variables are still represented as a Closure with an empty
    // upvalues vec.
    Function(GcRef<ClosureKind>),

    // Phase 6+
    NativeFn(Rc<NativeFn>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeData {
    pub from: i64,
    pub to: i64,
    pub step: i64,
    pub inclusive: bool,
}

impl RangeData {
    /// Number of elements yielded by iteration / spread / `#`.
    pub fn length(&self) -> i64 {
        if self.step == 0 {
            return 0;
        }
        let going_up = self.step > 0;
        let direction_matches = if going_up {
            self.from < self.to || (self.inclusive && self.from == self.to)
        } else {
            self.from > self.to || (self.inclusive && self.from == self.to)
        };
        if !direction_matches {
            return 0;
        }
        let span = if going_up { self.to - self.from } else { self.from - self.to };
        let abs_step = self.step.abs();
        if self.inclusive {
            span / abs_step + 1
        } else if span == 0 {
            0
        } else {
            (span - 1) / abs_step + 1
        }
    }

    /// Element at index `i` (0-based). Caller is responsible for bounds.
    pub fn nth(&self, i: i64) -> i64 {
        self.from + i * self.step
    }
}

/// A `Map`/`Set` key. Restricted to hashable primitives so the backing
/// `IndexMap`/`IndexSet` can derive `Hash`/`Eq`: `Float` is excluded
/// (NaN/`-0.0` hazards) and the mutable collection types are excluded
/// (a key mutated after insertion would be lost). `Str` hashes by
/// content, matching `Object` key behaviour.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    Null,
    Bool(bool),
    Int(i64),
    Str(Rc<str>),
}

impl MapKey {
    /// Convert a runtime `Value` into a key, or raise `InvalidKeyType`
    /// for an un-hashable type. `line` is stamped on the error (native
    /// callers pass 0 — the VM backfills the call site).
    pub fn from_value(v: &Value, line: u32) -> Result<MapKey, RuntimeError> {
        match v {
            Value::Null => Ok(MapKey::Null),
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::Int(n) => Ok(MapKey::Int(*n)),
            Value::Str(s) => Ok(MapKey::Str(s.clone())),
            other => Err(RuntimeError::new(
                RuntimeErrorKind::InvalidKeyType(other.type_name().into()),
                line,
            )),
        }
    }
}

/// Rehydrate a key back into a `Value` — used by iteration and the
/// `keys`/`entries`/`items` accessors.
impl From<MapKey> for Value {
    fn from(k: MapKey) -> Value {
        match k {
            MapKey::Null => Value::Null,
            MapKey::Bool(b) => Value::Bool(b),
            MapKey::Int(n) => Value::Int(n),
            MapKey::Str(s) => Value::Str(s),
        }
    }
}

/// Live iterator over one of the four iterable types. The compiler
/// emits `OpCode::MakeIter` to wrap an iterable in an `Iter` value, and
/// `OpCode::IterNext`/`IterNext2` to advance it inside a `for` loop.
#[derive(Clone)]
pub enum IterState {
    Range {
        current: i64,
        to: i64,
        step: i64,
        inclusive: bool,
        index: i64,
    },
    Array {
        array: GcRef<ArrayKind>,
        index: usize,
    },
    Object {
        object: GcRef<ObjectKind>,
        index: usize,
    },
    Map {
        map: GcRef<MapKind>,
        index: usize,
    },
    Set {
        set: GcRef<SetKind>,
        index: usize,
    },
    String {
        string: Rc<str>,
        char_index: usize,
        byte_index: usize,
    },
    /// An iterator object — `${ next: fn() }`. Unlike the other
    /// variants this cannot be advanced by `next()` (advancing it means
    /// calling a tigr closure); the VM drives it directly. `index` is a
    /// synthetic counter for the two-var `for` form; `done` is sticky
    /// once the object has reported exhaustion.
    IterObject {
        object: GcRef<ObjectKind>,
        index: i64,
        done: bool,
    },
}

impl IterState {
    /// Advance and yield `(counter_or_key, value)`. Returns `None` when
    /// exhausted. The compiler decides whether to use the counter via
    /// `IterNext` (one-var) vs `IterNext2` (two-var).
    pub fn next(&mut self) -> Option<(Value, Value)> {
        match self {
            IterState::Range { current, to, step, inclusive, index } => {
                let has_more = if *step > 0 {
                    if *inclusive { *current <= *to } else { *current < *to }
                } else if *step < 0 {
                    if *inclusive { *current >= *to } else { *current > *to }
                } else {
                    false
                };
                if !has_more {
                    return None;
                }
                let value = Value::Int(*current);
                let counter = Value::Int(*index);
                *current += *step;
                *index += 1;
                Some((counter, value))
            }
            IterState::Array { array, index } => {
                let arr = array.borrow();
                if *index >= arr.len() {
                    return None;
                }
                let v = arr[*index].clone();
                let counter = Value::Int(*index as i64);
                *index += 1;
                Some((counter, v))
            }
            IterState::Object { object, index } => {
                let obj = object.borrow();
                if *index >= obj.len() {
                    return None;
                }
                let (k, v) = obj.get_index(*index).unwrap();
                let key = Value::Str(k.clone());
                let value = v.clone();
                *index += 1;
                Some((key, value))
            }
            IterState::Map { map, index } => {
                let m = map.borrow();
                if *index >= m.len() {
                    return None;
                }
                let (k, v) = m.get_index(*index).unwrap();
                let key = Value::from(k.clone());
                let value = v.clone();
                *index += 1;
                Some((key, value))
            }
            IterState::Set { set, index } => {
                let s = set.borrow();
                if *index >= s.len() {
                    return None;
                }
                let elem = Value::from(s.get_index(*index).unwrap().clone());
                let counter = Value::Int(*index as i64);
                *index += 1;
                Some((counter, elem))
            }
            IterState::String { string, char_index, byte_index } => {
                let rest = &string[*byte_index..];
                let mut iter = rest.chars();
                let Some(c) = iter.next() else { return None; };
                let counter = Value::Int(*char_index as i64);
                let value = Value::Str(c.to_string().into());
                *byte_index += c.len_utf8();
                *char_index += 1;
                Some((counter, value))
            }
            IterState::IterObject { .. } => {
                unreachable!("IterObject is advanced by the VM, not IterState::next()")
            }
        }
    }
}

/// Compile-time function template. Lives in the enclosing chunk's
/// `functions` table; instances become runtime [`Closure`]s via the
/// `OpCode::Closure` opcode.
pub struct Function {
    /// Number of fixed positional parameters (excluding rest).
    pub arity: usize,
    /// `true` if the function declared a `...rest` parameter.
    /// Extra args land in an Array at slot `arity + 1`; if fewer than
    /// `arity` args were passed, rest is an empty Array.
    pub has_rest: bool,
    pub chunk: Chunk,
    /// Capture instructions: for each upvalue, where to source it from
    /// when the enclosing function constructs a closure.
    pub upvalues: Vec<UpvalueInfo>,
    pub name: Option<String>,
}

/// One per upvalue in a [`Function`]. `is_local = true` means "capture
/// the slot at `index` of the enclosing function's frame"; `false`
/// means "reuse the upvalue at `index` of the enclosing function's
/// closure".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpvalueInfo {
    pub is_local: bool,
    pub index: u8,
}

/// Runtime callable: a function template + its captured upvalue cells.
pub struct Closure {
    pub function: Rc<Function>,
    pub upvalues: Vec<GcRef<UpvalueKind>>,
}

/// A captured variable. `Open` means "still on the value stack at this
/// slot index"; `Closed` means "lifted to the heap" (the local has gone
/// out of scope).
#[derive(Clone)]
pub enum Upvalue {
    Open(usize),
    Closed(Value),
}

/// A built-in function. Invoked via `Call n` once `n` args are on the
/// stack with the `NativeFn` value just below them.
pub struct NativeFn {
    pub name: &'static str,
    pub arity: Arity,
    pub func: fn(&[Value]) -> Result<Value, crate::vm::error::RuntimeError>,
}

#[allow(dead_code)] // AtLeast not used until later phases (e.g. fold)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arity {
    Exact(usize),
    Variadic,
    AtLeast(usize),
    /// `Range(min, max)` — accepts `min..=max` arguments. Used by
    /// `JSON.stringify(value [, indent])`.
    Range(usize, usize),
}

impl Arity {
    pub fn check(self, n: usize) -> bool {
        match self {
            Arity::Exact(k) => n == k,
            Arity::Variadic => true,
            Arity::AtLeast(k) => n >= k,
            Arity::Range(min, max) => n >= min && n <= max,
        }
    }

    pub fn describe(self) -> String {
        match self {
            Arity::Exact(k) => format!("exactly {k}"),
            Arity::Variadic => "any number of".to_string(),
            Arity::AtLeast(k) => format!("at least {k}"),
            Arity::Range(min, max) => format!("between {min} and {max}"),
        }
    }
}

#[allow(dead_code)] // is_truthy used Phase 2+
impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Range(_) => "range",
            Value::Iter(_) => "iterator",
            Value::Function(_) => "function",
            Value::NativeFn(_) => "native function",
        }
    }

    /// Truthiness per spec §5.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Array(a) => !a.borrow().is_empty(),
            Value::Object(o) => !o.borrow().is_empty(),
            Value::Map(m) => !m.borrow().is_empty(),
            Value::Set(s) => !s.borrow().is_empty(),
            // §5: "all non-empty ranges" are truthy → empty range falsy.
            Value::Range(r) => r.length() > 0,
            Value::Iter(_) => true,
            Value::Function(_) => true,
            Value::NativeFn(_) => true,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Int(a), Float(b)) | (Float(b), Int(a)) => (*a as f64) == *b,
            (Str(a), Str(b)) => a == b,
            // `a == b` is GcRef identity (slot + generation) — the role
            // `Rc::ptr_eq` played before — and short-circuits the deep
            // structural compare for the same-object case.
            (Array(a), Array(b)) => a == b || *a.borrow() == *b.borrow(),
            (Object(a), Object(b)) => a == b || *a.borrow() == *b.borrow(),
            (Map(a), Map(b)) => a == b || *a.borrow() == *b.borrow(),
            (Set(a), Set(b)) => a == b || *a.borrow() == *b.borrow(),
            (Range(a), Range(b)) => a == b,
            (Iter(a), Iter(b)) => a == b,
            (Function(a), Function(b)) => a == b,
            (NativeFn(a), NativeFn(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a.partial_cmp(b),
            (Float(a), Float(b)) => a.partial_cmp(b),
            (Int(a), Float(b)) => (*a as f64).partial_cmp(b),
            (Float(a), Int(b)) => a.partial_cmp(&(*b as f64)),
            (Str(a), Str(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

/// Display in the canonical Tigr form used by the `str()` built-in.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(x) => {
                // always show a decimal point so the reader can tell Int from Float
                if x.is_finite() && x.fract() == 0.0 {
                    write!(f, "{x:.1}")
                } else {
                    write!(f, "{x}")
                }
            }
            Value::Str(s) => f.write_str(s),
            Value::Array(a) => {
                f.write_str("[")?;
                let arr = a.borrow();
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 { f.write_str(", ")?; }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Value::Object(o) => {
                f.write_str("${")?;
                let obj = o.borrow();
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 { f.write_str(", ")?; }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("}")
            }
            Value::Map(m) => {
                f.write_str("Map{")?;
                let map = m.borrow();
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 { f.write_str(", ")?; }
                    write!(f, "{}: {v}", Value::from(k.clone()))?;
                }
                f.write_str("}")
            }
            Value::Set(s) => {
                f.write_str("Set{")?;
                let set = s.borrow();
                for (i, k) in set.iter().enumerate() {
                    if i > 0 { f.write_str(", ")?; }
                    write!(f, "{}", Value::from(k.clone()))?;
                }
                f.write_str("}")
            }
            Value::Range(r) => {
                let dots = if r.inclusive { "..=" } else { ".." };
                if r.step.abs() == 1 {
                    write!(f, "{}{}{}", r.from, dots, r.to)
                } else {
                    write!(f, "{}{}{}:{}", r.from, dots, r.to, r.step)
                }
            }
            Value::Iter(_) => f.write_str("<iterator>"),
            Value::Function(c) => {
                let closure = c.borrow();
                match &closure.function.name {
                    Some(n) => write!(f, "<fn {n}>"),
                    None => f.write_str("<fn>"),
                }
            }
            Value::NativeFn(n) => write!(f, "<native fn {}>", n.name),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "'{s}'"),
            other => write!(f, "{other}"),
        }
    }
}
