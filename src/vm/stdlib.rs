//! Built-in functions exposed as ordinary bindings (spec §13).
//!
//! Adding a new built-in: define the
//! `fn(&[Value]) -> Result<Value, RuntimeError>`, append a `Spec`
//! entry to `BUILTINS`. The compiler pre-declares each name as a
//! resolvable global; the VM populates the matching `Value::NativeFn`
//! instances at startup.

use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;

use num_traits::ToPrimitive;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::offload::{BlockingJob, OffloadOk};
use crate::vm::rng;
use crate::vm::task::JoinOutcome;
use crate::vm::transfer::{decode, TransferError};
use crate::vm::value::{bigint_to_f64, Arity, NativeFn, NativeKind, Value};

/// Returns the ordered list of built-in names. The compiler uses this
/// to assign slot indices; the VM uses [`builtins`] to populate the
/// stack at start of execution.
pub fn names() -> &'static [&'static str] {
    &BUILTIN_NAMES
}

/// Build the `Value::NativeFn` instances in the same order as `names`.
pub fn builtins() -> Vec<Value> {
    BUILTINS
        .iter()
        .map(|nf| Value::NativeFn(Rc::new(NativeFn {
            name: nf.name,
            arity: nf.arity,
            kind: nf.kind,
        })))
        .collect()
}

struct Spec {
    name: &'static str,
    arity: Arity,
    kind: NativeKind,
}

/// Shorthand for an ordinary inline built-in.
const fn pure(f: fn(&[Value]) -> Result<Value, RuntimeError>) -> NativeKind {
    NativeKind::Pure(f)
}

const BUILTINS: &[Spec] = &[
    Spec { name: "print", arity: Arity::Variadic, kind: pure(native_print) },
    Spec { name: "str",   arity: Arity::Range(1, 3), kind: pure(native_str) },
    Spec { name: "num",   arity: Arity::Exact(1), kind: pure(native_num) },
    Spec { name: "int",   arity: Arity::Exact(1), kind: pure(native_int) },
    Spec { name: "float", arity: Arity::Exact(1), kind: pure(native_float) },
    Spec { name: "bool",  arity: Arity::Exact(1), kind: pure(native_bool) },
    Spec { name: "floor", arity: Arity::Exact(1), kind: pure(native_floor) },
    Spec { name: "ceil",  arity: Arity::Exact(1), kind: pure(native_ceil) },
    Spec { name: "rand",  arity: Arity::Exact(0), kind: pure(native_rand) },
    Spec { name: "type",  arity: Arity::Exact(1), kind: pure(native_type) },
    Spec { name: "gc",    arity: Arity::Exact(0), kind: pure(native_gc) },
    // v0.14 — concurrency. `join` waits for a `spawn`ed actor; it is
    // also the desugar target for `parallel[]`. `__select` backs the
    // `select` block — internal: a user writes `select`, never it.
    // Both wait, so they are `Blocking`: inside a green thread they
    // offload to the worker pool instead of freezing the actor.
    Spec {
        name: "__select",
        arity: Arity::Exact(2),
        kind: NativeKind::Blocking(native_select),
    },
    Spec {
        name: "join",
        arity: Arity::Exact(1),
        kind: NativeKind::Blocking(native_join),
    },
    // Cooperative green-thread timing for a host frame loop. Both are
    // intercepted by the VM's Call/TailCall dispatch (like `join`) to
    // park the running coroutine on the host clock; the `Pure` bodies
    // here are fallbacks that raise when *not* intercepted — `wait` with
    // a non-numeric argument, or either called where there is no host
    // drive (the main program, `update`/`draw`, or a plain `tigr run`).
    Spec { name: "wait", arity: Arity::Exact(1), kind: pure(native_wait) },
    Spec { name: "wait_frame", arity: Arity::Exact(0), kind: pure(native_wait_frame) },
];

const BUILTIN_NAMES: [&str; 15] = [
    "print", "str", "num", "int", "float", "bool", "floor", "ceil", "rand",
    "type", "gc", "__select", "join", "wait", "wait_frame",
];

fn native_print(args: &[Value]) -> Result<Value, RuntimeError> {
    // When an embedder (the browser playground) has installed a capture
    // buffer, the line goes there; otherwise straight to stdout.
    if crate::vm::io_capture::is_capturing() {
        let mut line = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(&arg.to_string());
        }
        line.push('\n');
        crate::vm::io_capture::push(&line);
    } else {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            print!("{arg}");
        }
        println!();
    }
    // Returns the last arg (or null), mirroring block-tail semantics.
    // Lets `compute(print('val:', x))` log and pass `x` through.
    Ok(args.last().cloned().unwrap_or(Value::Null))
}

/// Fallback body for `wait` — reached only when the VM did *not*
/// intercept the call (`Vm::wait_target` matches `wait` only with a
/// single numeric argument). A non-numeric argument is a type error; a
/// numeric one reaching here means `wait` was invoked outside the
/// Call/TailCall dispatch (e.g. a host `call_function("wait", ...)`),
/// where there is no coroutine to park. Both raise catchably.
fn native_wait(args: &[Value]) -> Result<Value, RuntimeError> {
    match args.first() {
        Some(Value::Int(_)) | Some(Value::Float(_)) => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(
                "wait is only valid inside a host-driven green thread".into(),
            )),
            0,
        )),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(
                format!(
                    "wait expects a number of seconds, got {}",
                    other.map(|v| v.type_name()).unwrap_or("nothing"),
                )
                .into(),
            )),
            0,
        )),
    }
}

/// Fallback body for `wait_frame` — see [`native_wait`]. Reached only
/// outside the Call/TailCall dispatch (the no-arg form is otherwise
/// always intercepted).
fn native_wait_frame(_args: &[Value]) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        RuntimeErrorKind::Raised(Value::Str(
            "wait_frame is only valid inside a host-driven green thread".into(),
        )),
        0,
    ))
}

/// `str(x)` — canonical string form. `str(n, radix)` /
/// `str(n, radix, prefix)` — render an Int in `radix` (2..=36, lowercase
/// digits); with `prefix == true` prepend the `0b`/`0o`/`0x` literal
/// marker (only radix 2/8/16 have one).
fn native_str(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() == 1 {
        return Ok(Value::Str(format!("{}", args[0]).into()));
    }

    let n = match &args[0] {
        Value::Int(n) => *n,
        other => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "str() with a radix expects an Int, got {}",
                    other.type_name()
                )),
                0,
            ))
        }
    };
    let radix = match &args[1] {
        Value::Int(r) if (2..=36).contains(r) => *r as u32,
        Value::Int(r) => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "str() radix must be in 2..=36, got {r}"
                )),
                0,
            ))
        }
        other => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "str() radix must be an Int, got {}",
                    other.type_name()
                )),
                0,
            ))
        }
    };
    let prefix = match args.get(2) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "str() prefix flag must be a Bool, got {}",
                    other.type_name()
                )),
                0,
            ))
        }
    };
    let prefix_str = if prefix {
        match radix {
            2 => "0b",
            8 => "0o",
            16 => "0x",
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::TypeMismatch(format!(
                        "str() has no literal prefix for radix {radix} \
                         (prefix is defined only for radix 2, 8, 16)"
                    )),
                    0,
                ))
            }
        }
    } else {
        ""
    };
    let sign = if n < 0 { "-" } else { "" };
    let digits = int_to_radix(n.unsigned_abs(), radix);
    Ok(Value::Str(format!("{sign}{prefix_str}{digits}").into()))
}

/// Render a non-negative magnitude in `radix` (2..=36), lowercase
/// digits. Zero renders as `"0"`.
pub(crate) fn int_to_radix(mut n: u64, radix: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let r = radix as u64;
    let mut buf: Vec<u8> = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % r) as usize]);
        n /= r;
    }
    buf.reverse();
    String::from_utf8(buf).expect("radix digits are ASCII")
}

/// Name the runtime type of a value. `Function` and `NativeFn` both
/// report `"function"` — the user-facing question is "is it callable",
/// not how the callable is implemented.
fn native_type(args: &[Value]) -> Result<Value, RuntimeError> {
    let name = match &args[0] {
        Value::NativeFn(_) => "function",
        other => other.type_name(),
    };
    Ok(Value::Str(name.into()))
}

/// Parse a number from a string; pass through if already a number.
/// Returns `null` on unparseable strings (so callers can chain with
/// `||` defaults).
fn native_num(args: &[Value]) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(x) => Ok(Value::Float(*x)),
        // A BigInt is already numeric — pass it through unchanged.
        Value::BigInt(_) => Ok(args[0].clone()),
        Value::Str(s) => {
            let t = s.trim();
            if let Ok(n) = t.parse::<i64>() {
                Ok(Value::Int(n))
            } else if let Ok(x) = t.parse::<f64>() {
                Ok(Value::Float(x))
            } else {
                Ok(Value::Null)
            }
        }
        Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "num() cannot convert {}", other.type_name()
            )),
            0,
        )),
    }
}

/// Truncate-toward-zero to Int.
fn native_int(args: &[Value]) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(x) => Ok(Value::Int(x.trunc() as i64)),
        Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
        // Narrow a BigInt back to Int — raises `overflow` if it does
        // not fit i64, the same catchable error v0.8 arithmetic uses.
        Value::BigInt(n) => match n.to_i64() {
            Some(i) => Ok(Value::Int(i)),
            None => Err(RuntimeError::new(RuntimeErrorKind::Overflow, 0)),
        },
        Value::Str(s) => {
            let t = s.trim();
            if let Ok(n) = t.parse::<i64>() {
                Ok(Value::Int(n))
            } else if let Ok(x) = t.parse::<f64>() {
                Ok(Value::Int(x.trunc() as i64))
            } else {
                Err(RuntimeError::new(
                    RuntimeErrorKind::TypeMismatch(format!(
                        "int() cannot parse {:?}", s
                    )),
                    0,
                ))
            }
        }
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "int() cannot convert {}", other.type_name()
            )),
            0,
        )),
    }
}

fn native_float(args: &[Value]) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Int(n) => Ok(Value::Float(*n as f64)),
        Value::Float(x) => Ok(Value::Float(*x)),
        Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
        // Lossy — saturates to ±inf beyond the float range.
        Value::BigInt(n) => Ok(Value::Float(bigint_to_f64(n))),
        Value::Str(s) => match s.trim().parse::<f64>() {
            Ok(x) => Ok(Value::Float(x)),
            Err(_) => Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "float() cannot parse {:?}", s
                )),
                0,
            )),
        },
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "float() cannot convert {}", other.type_name()
            )),
            0,
        )),
    }
}

fn native_bool(args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Bool(args[0].is_truthy()))
}

fn native_floor(args: &[Value]) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(x) => Ok(Value::Int(x.floor() as i64)),
        // A BigInt is already integral — return it unchanged.
        Value::BigInt(_) => Ok(args[0].clone()),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "floor() expects a number, got {}", other.type_name()
            )),
            0,
        )),
    }
}

fn native_ceil(args: &[Value]) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(x) => Ok(Value::Int(x.ceil() as i64)),
        // A BigInt is already integral — return it unchanged.
        Value::BigInt(_) => Ok(args[0].clone()),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "ceil() expects a number, got {}", other.type_name()
            )),
            0,
        )),
    }
}

// -- rand -----------------------------------------------------------
//
// `rand()` draws from the shared per-thread PRNG in [`crate::vm::rng`].
// That same stream backs the `Random` module, so `Random.seed(n)`
// makes `rand()` reproducible too.

fn native_rand(_args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Float(rng::next_f64()))
}

// -- gc -------------------------------------------------------------
//
// `gc()` reports the tracing collector's counters as an object —
// `${live, collections, allocated, freed}`. Collection itself is
// automatic (it runs at VM safepoints once the heap crosses a size
// threshold); `gc()` is a read-only window for tests and tuning.

// -- __select -------------------------------------------------------
//
// The runtime backing the `select { ... }` block (v0.14). The parser
// desugars `select` to a `match` over `__select(channels, has_else)`,
// which blocks until one channel has a message and returns
// `${index, value}` (or `${index: -1}` for an `else` arm).

fn native_select(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    use crate::vm::channel::{select, SelectResult};

    let chans = match &args[0] {
        Value::Array(a) => {
            let arr = a.borrow();
            let mut v = Vec::with_capacity(arr.len());
            for item in arr.iter() {
                match item {
                    Value::Channel(h) => v.push(h.clone()),
                    other => {
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "select expects channels, got {}",
                                other.type_name()
                            )),
                            0,
                        ));
                    }
                }
            }
            v
        }
        other => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "select expects an array of channels, got {}",
                    other.type_name()
                )),
                0,
            ));
        }
    };
    let has_else = matches!(&args[1], Value::Bool(true));

    Ok(Box::new(move || {
        let result = select(&chans, has_else);
        OffloadOk::deferred(move || match result {
            SelectResult::Fired { index, message } => {
                let mut m: IndexMap<Arc<str>, Value> =
                    IndexMap::with_capacity(2);
                m.insert(Arc::from("index"), Value::Int(index as i64));
                m.insert(Arc::from("value"), decode(message));
                Ok(Value::Object(gc::alloc_object(m)))
            }
            SelectResult::ElseReady => {
                let mut m: IndexMap<Arc<str>, Value> =
                    IndexMap::with_capacity(1);
                m.insert(Arc::from("index"), Value::Int(-1));
                Ok(Value::Object(gc::alloc_object(m)))
            }
            SelectResult::AllClosed => {
                Err(RuntimeError::new(RuntimeErrorKind::ChannelClosed, 0))
            }
        })
    }))
}

fn native_gc(_args: &[Value]) -> Result<Value, RuntimeError> {
    let s = gc::stats();
    let mut m: IndexMap<Arc<str>, Value> = IndexMap::with_capacity(4);
    m.insert(Arc::from("live"), Value::Int(s.live as i64));
    m.insert(Arc::from("collections"), Value::Int(s.collections as i64));
    m.insert(Arc::from("allocated"), Value::Int(s.total_allocated as i64));
    m.insert(Arc::from("freed"), Value::Int(s.total_freed as i64));
    Ok(Value::Object(gc::alloc_object(m)))
}

// -- join (v0.14 concurrency) ---------------------------------------
//
// `join(task)` blocks for a `spawn`ed actor's result. It either decodes
// the actor's return value into the caller's heap, or re-raises the
// actor's error so the caller can `try`/`catch` it.

/// `join(task)` — block for the actor's result. Returns its value, or
/// raises: the actor's own `raise`d value verbatim, or — for a
/// built-in actor error — an object `${kind, message, trace, worker}`.
///
/// A blocking native: the Condvar wait for the worker actor runs on
/// the offload pool, so a green thread joining a `spawn`ed actor does
/// not freeze its siblings. (`join` on a *green-thread* handle never
/// reaches here — the VM intercepts that case for a cooperative
/// `coop_join` before the native is invoked.)
fn native_join(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let task = match &args[0] {
        Value::Task(t) => t.clone(),
        other => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "join() expects a task, got {}",
                    other.type_name()
                )),
                0,
            ));
        }
    };
    Ok(Box::new(move || {
        let outcome = task.join();
        OffloadOk::deferred(move || match outcome {
            JoinOutcome::Outcome(Ok(transfer)) => Ok(decode(transfer)),
            JoinOutcome::Outcome(Err(te)) => Err(actor_error(te)),
            JoinOutcome::AlreadyJoined => Err(RuntimeError::new(
                RuntimeErrorKind::Raised(Value::Str(
                    "task has already been joined".into(),
                )),
                0,
            )),
        })
    }))
}

/// Reconstruct a catchable error in the joining actor from a worker's
/// `TransferError`.
fn actor_error(te: TransferError) -> RuntimeError {
    // A `raise <value>` in the worker re-raises that exact value, so
    // the parent's `catch` binds what the worker raised.
    if let Some(raised) = te.raised {
        return RuntimeError::new(RuntimeErrorKind::Raised(decode(raised)), 0);
    }
    // A built-in worker error surfaces as an object carrying the
    // worker's kind, message, and rendered trace.
    let mut m: IndexMap<Arc<str>, Value> = IndexMap::with_capacity(4);
    m.insert(Arc::from("kind"), Value::Str(te.kind_tag.into()));
    m.insert(Arc::from("message"), Value::Str(te.message.into()));
    m.insert(Arc::from("trace"), Value::Str(te.rendered_trace.into()));
    m.insert(Arc::from("worker"), Value::Bool(true));
    RuntimeError::new(
        RuntimeErrorKind::Raised(Value::Object(gc::alloc_object(m))),
        0,
    )
}
