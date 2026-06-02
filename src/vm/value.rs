//! Runtime values.
//!
//! The mutable / potentially-cyclic types — `Array`, `Object`, `Map`,
//! `Set`, `Iter`, `Closure`, and upvalue cells — are managed by the
//! v0.10 tracing collector ([`crate::vm::gc`]): a `Value` carries a
//! small `Copy` [`GcRef`] handle into the thread-local heap rather than
//! the data itself. `Str`, `Range`, `NativeFn`, and `BigInt` are
//! immutable and acyclic, so they stay plain `Rc` — `Rc` reclaims
//! acyclic data fine and the collector skips them.

use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use num_bigint::BigInt as BigIntData;

use crate::vm::channel::ChannelHandle;
use crate::vm::chunk::Chunk;
use crate::vm::file_handle::FileHandle;
use crate::vm::socket::SocketHandle;
use crate::vm::task::TaskHandle;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc::{
    ArrayKind, BytesKind, ClosureKind, GcRef, GeneratorKind, GreenHandleKind,
    IterKind, LocalChannelKind, MapKind, ObjectKind, SetKind, UpvalueKind,
};

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// `Arc<str>` (not `Rc`) so a string can be pooled in a `Const`
    /// and loaded across actor threads without re-allocating per load.
    Str(Arc<str>),

    // Phase 3+ — GC-managed (see `crate::vm::gc`).
    Array(GcRef<ArrayKind>),
    Object(GcRef<ObjectKind>),

    // v0.9 — arbitrary-keyed dictionary / set. Keys are restricted to
    // hashable primitives (see `MapKey`); insertion-ordered like Object.
    Map(GcRef<MapKind>),
    Set(GcRef<SetKind>),

    // v0.13 — a mutable byte buffer (`Vec<u8>`). GC-managed like the
    // other mutable collections; indexable, `#`-length, `for`-iterable,
    // sliceable. Backs binary IO and future networking.
    Bytes(GcRef<BytesKind>),

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

    // v0.13 — arbitrary-precision integer. Immutable and acyclic, so
    // Rc-managed like Str/Range/NativeFn — the collector skips it.
    // Created explicitly via `BigInt.new(...)`; an overflowing `Int`
    // still raises `overflow` (v0.8) rather than promoting here.
    BigInt(Rc<BigIntData>),

    // v0.14 — a message-passing channel between actors. `Arc`-backed
    // and `Send`; lives outside any heap, so the collector skips it
    // (a GC leaf, like NativeFn). Equality is handle identity.
    Channel(ChannelHandle),

    // v0.14 — a handle to a spawned actor's eventual result. Like
    // `Channel`: `Arc`-backed, `Send`, a GC leaf, identity equality.
    Task(TaskHandle),

    // v0.15 — a network socket (TCP / UDP / TLS). Like `Channel`:
    // `Arc`-backed, `Send`, a GC leaf, identity equality — so it can
    // cross an actor boundary into a `spawn`ed connection handler.
    Socket(SocketHandle),

    // Streaming file handle (`IO.open`). Like `Socket`: `Arc`-backed,
    // `Send`, a GC leaf, identity equality. Crosses actor boundaries
    // so a `spawn`ed worker can read/write a file the parent opened.
    File(FileHandle),

    // green threads — a paused generator coroutine. Produced by calling
    // a `gen fn`; never directly visible to tigr code, which only sees
    // the `${ next: fn() }` iterator object wrapping it. GC-managed:
    // the coroutine's frames/stack are reclaimed when the wrapping
    // object becomes unreachable, even if the generator is undrained.
    Generator(GcRef<GeneratorKind>),

    // green threads (Phase 4) — a handle to a coroutine spawned with
    // `go`. `join(handle)` cooperatively waits for the coroutine and
    // yields its return value. GC-managed: the handle holds the
    // recorded result. Identity equality.
    GreenHandle(GcRef<GreenHandleKind>),

    // green threads (Phase 4) — an intra-actor channel. Unlike
    // `Channel` it never leaves this heap, so it carries `Value`s
    // directly (no transfer-encoding). GC-managed; identity equality.
    LocalChannel(GcRef<LocalChannelKind>),
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
    Str(Arc<str>),
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
    Bytes {
        bytes: GcRef<BytesKind>,
        index: usize,
    },
    String {
        string: Arc<str>,
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
            IterState::Bytes { bytes, index } => {
                let b = bytes.borrow();
                if *index >= b.len() {
                    return None;
                }
                let value = Value::Int(b[*index] as i64);
                let counter = Value::Int(*index as i64);
                *index += 1;
                Some((counter, value))
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
    /// `true` for a `gen fn`. Calling it does not run `chunk`; the VM
    /// builds a paused coroutine and returns a `${ next: fn() }`
    /// iterator object instead (see [`crate::vm::scheduler`]).
    pub is_generator: bool,
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
/// `function` is an `Arc` (not `Rc`) so the immutable compiled code can
/// be shared with actor worker threads (v0.14 concurrency).
pub struct Closure {
    pub function: Arc<Function>,
    pub upvalues: Vec<GcRef<UpvalueKind>>,
}

/// A captured variable. `Open` means "still on a value stack"; `Closed`
/// means "lifted to the heap" (the local has gone out of scope).
///
/// `Open` records both the slot index and the id of the green thread
/// (coroutine) whose stack that slot lives in. With one private value
/// stack per coroutine, a closure captured into a `go` block may refer
/// to a slot on a *different* coroutine's stack; carrying the owner id
/// lets the VM resolve the read/write against the right stack and
/// keeps shared mutation working across coroutines. Coroutine #0 is
/// the actor's main program, so `owner: 0` is the common case.
#[derive(Clone)]
pub enum Upvalue {
    Open { owner: u32, slot: usize },
    Closed(Value),
}

/// A built-in function. Invoked via `Call n` once `n` args are on the
/// stack with the `NativeFn` value just below them.
pub struct NativeFn {
    pub name: &'static str,
    pub arity: Arity,
    pub kind: NativeKind,
}

/// How a cooperative `wait` parks the running green thread: for a fixed
/// number of seconds on the clock, or until the very next host frame.
/// Returned by a [`NativeKind::Park`] native; the VM's call dispatch
/// reads it and parks instead of pushing a value.
#[derive(Clone, Copy)]
pub enum WaitKind {
    /// Resume once the clock advances by `secs` from now. The standalone
    /// run loop advances the clock itself; under a host frame drive the
    /// host does. Works in any program with green threads.
    Secs(f64),
    /// Resume on the next host frame — only meaningful under a host frame
    /// drive, so it raises outside one. Backs purr's `GameTime.wait_frame`.
    NextFrame,
}

/// How a [`NativeFn`] is run. Most natives are `Pure` — fast,
/// non-blocking, run inline on the actor thread exactly as before.
/// `Blocking` natives wrap a call that may wait (a child process, file
/// or network IO); the VM can offload them to a worker pool so a green
/// thread doing IO does not stall its siblings (see
/// [`crate::vm::offload`]). `Park` natives cooperatively suspend the
/// running green thread on the clock (`wait`, `GameTime.wait_frame`).
#[derive(Clone, Copy)]
pub enum NativeKind {
    /// Runs inline on the actor thread — the historical behaviour.
    Pure(fn(&[Value]) -> Result<Value, crate::vm::error::RuntimeError>),
    /// A blocking call. The `fn` runs on the actor thread: it validates
    /// arguments and extracts `Send` POD from them, returning a closure
    /// the worker pool runs off-thread. Bad arguments raise immediately,
    /// before any offload.
    Blocking(
        fn(&[Value]) -> Result<
            crate::vm::offload::BlockingJob,
            crate::vm::error::RuntimeError,
        >,
    ),
    /// A steady-state socket call (`Net` stream / datagram I/O). The
    /// `fn` runs on the actor thread to validate arguments and build a
    /// declarative [`crate::vm::socket::ReactorOp`]; the VM then drives
    /// it on the async-IO reactor so thousands of parked socket ops
    /// cost one poll thread, not one worker thread each. See
    /// [`crate::vm::reactor`].
    Socket(
        fn(&[Value]) -> Result<
            crate::vm::socket::ReactorOp,
            crate::vm::error::RuntimeError,
        >,
    ),
    /// A cooperative park. The `fn` runs on the actor thread to validate
    /// arguments and decide *how* to park ([`WaitKind`]); the VM's call
    /// dispatch then suspends the running green thread instead of pushing
    /// a return value. Reachable only at a coroutine-switch point (a
    /// `Call`/`TailCall`/`CallSpread`); called anywhere else (e.g. a host
    /// `call_function` entry) it raises, since there is no green thread to
    /// park. Backs `wait` and purr's `GameTime.wait_frame`.
    Park(
        fn(&[Value]) -> Result<
            WaitKind,
            crate::vm::error::RuntimeError,
        >,
    ),
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
            Value::Bytes(_) => "bytes",
            Value::Range(_) => "range",
            Value::Iter(_) => "iterator",
            Value::Function(_) => "function",
            Value::NativeFn(_) => "native function",
            Value::BigInt(_) => "bigint",
            Value::Channel(_) => "channel",
            Value::Task(_) => "task",
            Value::Socket(_) => "socket",
            Value::File(_) => "file",
            Value::Generator(_) => "generator",
            Value::GreenHandle(_) => "green_thread",
            Value::LocalChannel(_) => "local_channel",
        }
    }

    /// Read a field of an `Object` value by key, cloning the value out.
    /// Returns `None` if `self` is not an object or the key is absent. A
    /// convenience for host/embedder code reading config-style objects
    /// (e.g. a `${ title: ..., width: ... }` window declaration) without
    /// reaching into the GC arena.
    pub fn get_field(&self, key: &str) -> Option<Value> {
        match self {
            Value::Object(o) => o.borrow().get(key).cloned(),
            _ => None,
        }
    }

    /// The `(key, value)` pairs of an `Object` value, in insertion order,
    /// or `None` for any non-object. The iteration complement of
    /// [`get_field`](Value::get_field): host/embedder code that takes a
    /// caller-built object whose keys it does not know ahead of time (a
    /// `${ name: 'type', ... }` declaration map) can read the whole set in
    /// one call. Each value is cloned out (a handle clone for collections)
    /// and each key copied to an owned `String`, so nothing borrows the GC
    /// arena past the call.
    pub fn fields(&self) -> Option<Vec<(String, Value)>> {
        match self {
            Value::Object(o) => Some(o.borrow().iter().map(|(k, v)| (k.to_string(), v.clone())).collect()),
            _ => None,
        }
    }

    /// Set a field of an `Object` value by key, inserting it if absent and
    /// updating in place (keeping its position) if present. Returns `false`,
    /// touching nothing, if `self` is not an object. The write complement of
    /// [`get_field`](Value::get_field): host/embedder code can update a
    /// caller-owned object in place (e.g. a `${ x, y }` rewritten every frame),
    /// so a hot per-entity native need not allocate a fresh object to return.
    /// Objects are reference values, so the caller observes the change.
    pub fn set_field(&self, key: &str, value: Value) -> bool {
        match self {
            Value::Object(o) => {
                o.borrow_mut().insert(key.into(), value);
                true
            }
            _ => false,
        }
    }

    /// Run `f` against the bytes of a `Bytes` value, borrowing the block in
    /// place rather than cloning it. Returns `Some(f(..))` for a `Bytes`
    /// value and `None` for anything else. The read complement of
    /// [`bytes`](crate::embed::bytes): host/embedder code consumes a byte
    /// argument (a pixel buffer's RGBA, a blob to decode) without copying
    /// the whole block out of the GC arena first, so a multi-megabyte
    /// buffer crosses the boundary by reference. The borrow is held only
    /// for the duration of `f`, so do the copy or parse inside the closure.
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        match self {
            Value::Bytes(b) => Some(f(&b.borrow())),
            _ => None,
        }
    }

    /// Truthiness per spec §5. Lua-style: only `null` and `false` are
    /// falsy. Everything else — including `0`, `0.0`, `''`, `[]`, `${}`,
    /// empty ranges/maps/sets — is truthy. Test emptiness with `#x`.
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Null | Value::Bool(false))
    }
}

thread_local! {
    /// Recursion depth of the structural [`Value`] equality compare.
    /// Heap collections can form cycles, so comparing two *distinct*
    /// cyclic structures would recurse until the native stack
    /// overflows. Past [`EQ_DEPTH_LIMIT`] the compare gives up and
    /// reports unequal — a cyclic structure equals nothing but itself,
    /// and that case is caught by the `GcRef`-identity short-circuit
    /// without ever recursing.
    static EQ_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Native-stack-recursion ceiling for [`Value`] equality — far past any
/// realistic nesting, well short of overflowing the stack.
const EQ_DEPTH_LIMIT: u32 = 1000;

/// Run one structural-equality step under the [`EQ_DEPTH`] guard.
/// Returns `false` once the depth limit is hit, which breaks an
/// otherwise unbounded recursion through a cyclic collection.
fn eq_guarded(step: impl FnOnce() -> bool) -> bool {
    EQ_DEPTH.with(|d| {
        let depth = d.get();
        if depth >= EQ_DEPTH_LIMIT {
            return false;
        }
        d.set(depth + 1);
        let equal = step();
        d.set(depth);
        equal
    })
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
            (Array(a), Array(b)) => {
                a == b || eq_guarded(|| *a.borrow() == *b.borrow())
            }
            (Object(a), Object(b)) => {
                a == b || eq_guarded(|| *a.borrow() == *b.borrow())
            }
            (Map(a), Map(b)) => {
                a == b || eq_guarded(|| *a.borrow() == *b.borrow())
            }
            (Set(a), Set(b)) => a == b || *a.borrow() == *b.borrow(),
            (Bytes(a), Bytes(b)) => a == b || *a.borrow() == *b.borrow(),
            (Range(a), Range(b)) => a == b,
            (Iter(a), Iter(b)) => a == b,
            (Function(a), Function(b)) => a == b,
            (NativeFn(a), NativeFn(b)) => Rc::ptr_eq(a, b),
            (Channel(a), Channel(b)) => Arc::ptr_eq(a, b),
            (Task(a), Task(b)) => Arc::ptr_eq(a, b),
            (Socket(a), Socket(b)) => Arc::ptr_eq(a, b),
            (File(a), File(b)) => Arc::ptr_eq(a, b),
            (Generator(a), Generator(b)) => a == b,
            (GreenHandle(a), GreenHandle(b)) => a == b,
            (LocalChannel(a), LocalChannel(b)) => a == b,
            (BigInt(a), BigInt(b)) => a == b,
            // A `BigInt` and an `Int` of equal value compare equal,
            // mirroring `Int`/`Float` cross-type equality above.
            (BigInt(a), Int(b)) | (Int(b), BigInt(a)) => {
                **a == BigIntData::from(*b)
            }
            // `BigInt`/`Float` are deliberately never `==`: a BigInt
            // outside f64's exact range could spuriously match.
            _ => false,
        }
    }
}

/// Lossy `BigInt` → `f64`, saturating to `±∞` when the magnitude
/// exceeds the float range. Used for `BigInt`/`Float` ordering and for
/// arithmetic that has a `Float` operand.
pub(crate) fn bigint_to_f64(n: &BigIntData) -> f64 {
    use num_traits::ToPrimitive;
    n.to_f64().unwrap_or_else(|| {
        if n.sign() == num_bigint::Sign::Minus {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    })
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
            (BigInt(a), BigInt(b)) => a.partial_cmp(b),
            (BigInt(a), Int(b)) => a.as_ref().partial_cmp(&BigIntData::from(*b)),
            (Int(a), BigInt(b)) => BigIntData::from(*a).partial_cmp(b.as_ref()),
            // `BigInt`/`Float` ordering is supported (unlike equality):
            // `<`/`>` is what big-number code needs; promote to f64.
            (BigInt(a), Float(b)) => bigint_to_f64(a).partial_cmp(b),
            (Float(a), BigInt(b)) => a.partial_cmp(&bigint_to_f64(b)),
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
            Value::Bytes(b) => {
                // Space-separated hex, truncated so a large buffer can
                // never blow up a string interpolation or error message.
                const SHOWN: usize = 64;
                let bytes = b.borrow();
                f.write_str("Bytes[")?;
                for (i, byte) in bytes.iter().take(SHOWN).enumerate() {
                    if i > 0 { f.write_str(" ")?; }
                    write!(f, "{byte:02x}")?;
                }
                if bytes.len() > SHOWN {
                    write!(f, " … ({} total)", bytes.len())?;
                }
                f.write_str("]")
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
            Value::BigInt(n) => write!(f, "{n}"),
            Value::Channel(_) => f.write_str("<channel>"),
            Value::Task(_) => f.write_str("<task>"),
            Value::Socket(s) => write!(f, "<socket #{}>", s.id()),
            Value::File(fh) => write!(f, "<file #{}>", fh.id()),
            Value::Generator(_) => f.write_str("<generator>"),
            Value::GreenHandle(_) => f.write_str("<green thread>"),
            Value::LocalChannel(_) => f.write_str("<local channel>"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::chunk::{Chunk, Const};

    /// v0.14: compiled code must be `Send + Sync` so an `Arc<Function>`
    /// can be handed to an actor worker thread. A compile-time check —
    /// the bounds fail to type-check if the invariant ever breaks.
    #[test]
    fn compiled_code_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Const>();
        assert_send_sync::<Chunk>();
        assert_send_sync::<Function>();
        assert_send_sync::<Arc<Function>>();
    }
}
