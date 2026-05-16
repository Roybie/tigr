//! `import 'Random'` — seedable pseudo-random numbers.
//!
//! Every entry draws from the shared per-thread PRNG in
//! [`crate::vm::rng`] — the same stream the bare `rand()` builtin
//! uses. `Random.seed(n)` therefore makes both this module *and*
//! `rand()` reproducible, which is the whole point: tests can pin a
//! seed and get deterministic output.

use std::cell::RefCell;
use std::rc::Rc;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::rng;
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("seed",    native("seed",    Arity::Exact(1), r_seed)),
        ("float",   native("float",   Arity::Exact(0), r_float)),
        ("int",     native("int",     Arity::Exact(2), r_int)),
        ("bool",    native("bool",    Arity::Exact(0), r_bool)),
        ("choice",  native("choice",  Arity::Exact(1), r_choice)),
        ("range",   native("range",   Arity::Exact(1), r_range)),
        ("shuffle", native("shuffle", Arity::Exact(1), r_shuffle)),
    ])
}

fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

fn expect_int(v: &Value, label: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(err(format!(
            "Random.{label}: expected Int, got {}", other.type_name()
        ))),
    }
}

/// `seed(n)` — pin the stream to `n`. Any Int works (`seed(0)`
/// included). Returns `null`.
fn r_seed(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = expect_int(&args[0], "seed")?;
    rng::seed(n as u64);
    Ok(Value::Null)
}

/// `float()` — uniform Float in `[0, 1)`.
fn r_float(_args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Float(rng::next_f64()))
}

/// `int(lo, hi)` — uniform Int in the inclusive range `[lo, hi]`.
fn r_int(args: &[Value]) -> Result<Value, RuntimeError> {
    let lo = expect_int(&args[0], "int")?;
    let hi = expect_int(&args[1], "int")?;
    // `i128` so the span never overflows, even for `lo`/`hi` at the
    // `i64` extremes.
    let span = (hi as i128) - (lo as i128) + 1;
    if span <= 0 {
        return Err(err(format!(
            "Random.int: lo ({lo}) must not exceed hi ({hi})"
        )));
    }
    let result = if span > u64::MAX as i128 {
        // The range spans every `i64`; any 64-bit word lands in it.
        rng::next_u64() as i64
    } else {
        let offset = rng::next_below(span as u64) as i128;
        (lo as i128 + offset) as i64
    };
    Ok(Value::Int(result))
}

/// `bool()` — `true` or `false`, each with probability 1/2.
fn r_bool(_args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Bool(rng::next_u64() & 1 == 1))
}

/// `choice(arr)` — a uniformly random element of a non-empty Array.
fn r_choice(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = match &args[0] {
        Value::Array(a) => a,
        other => return Err(err(format!(
            "Random.choice: expected Array, got {}", other.type_name()
        ))),
    };
    let arr = arr.borrow();
    if arr.is_empty() {
        return Err(err("Random.choice: array is empty".into()));
    }
    let i = rng::next_below(arr.len() as u64) as usize;
    Ok(arr[i].clone())
}

/// `range(r)` — a uniformly random element of a non-empty Range,
/// honouring its step (`range(0..=8:2)` ⇒ one of `0,2,4,6,8`).
fn r_range(args: &[Value]) -> Result<Value, RuntimeError> {
    let r = match &args[0] {
        Value::Range(r) => r,
        other => return Err(err(format!(
            "Random.range: expected Range, got {}", other.type_name()
        ))),
    };
    let len = r.length();
    if len <= 0 {
        return Err(err("Random.range: range is empty".into()));
    }
    let i = rng::next_below(len as u64) as i64;
    Ok(Value::Int(r.nth(i)))
}

/// `shuffle(arr)` — a *new* array holding `arr`'s elements in random
/// order. The input is left untouched. Fisher-Yates.
fn r_shuffle(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = match &args[0] {
        Value::Array(a) => a,
        other => return Err(err(format!(
            "Random.shuffle: expected Array, got {}", other.type_name()
        ))),
    };
    let mut out: Vec<Value> = arr.borrow().clone();
    // Walk high → low, swapping each slot with a uniformly random
    // earlier-or-equal slot.
    for i in (1..out.len()).rev() {
        let j = rng::next_below((i + 1) as u64) as usize;
        out.swap(i, j);
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}
