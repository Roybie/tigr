//! Built-in functions exposed as ordinary bindings (spec §13).
//!
//! Adding a new built-in: define the
//! `fn(&[Value]) -> Result<Value, RuntimeError>`, append a `Spec`
//! entry to `BUILTINS`. The compiler pre-declares each name as a
//! resolvable global; the VM populates the matching `Value::NativeFn`
//! instances at startup.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, NativeFn, Value};

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
            func: nf.func,
        })))
        .collect()
}

struct Spec {
    name: &'static str,
    arity: Arity,
    func: fn(&[Value]) -> Result<Value, RuntimeError>,
}

const BUILTINS: &[Spec] = &[
    Spec { name: "print", arity: Arity::Variadic, func: native_print },
    Spec { name: "str",   arity: Arity::Exact(1), func: native_str },
    Spec { name: "num",   arity: Arity::Exact(1), func: native_num },
    Spec { name: "int",   arity: Arity::Exact(1), func: native_int },
    Spec { name: "float", arity: Arity::Exact(1), func: native_float },
    Spec { name: "bool",  arity: Arity::Exact(1), func: native_bool },
    Spec { name: "floor", arity: Arity::Exact(1), func: native_floor },
    Spec { name: "ceil",  arity: Arity::Exact(1), func: native_ceil },
    Spec { name: "rand",  arity: Arity::Exact(0), func: native_rand },
    Spec { name: "type",  arity: Arity::Exact(1), func: native_type },
];

const BUILTIN_NAMES: [&str; 10] = [
    "print", "str", "num", "int", "float", "bool", "floor", "ceil", "rand",
    "type",
];

fn native_print(args: &[Value]) -> Result<Value, RuntimeError> {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            print!(" ");
        }
        print!("{arg}");
    }
    println!();
    // Returns the last arg (or null), mirroring block-tail semantics.
    // Lets `compute(print('val:', x))` log and pass `x` through.
    Ok(args.last().cloned().unwrap_or(Value::Null))
}

fn native_str(args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::Str(format!("{}", args[0]).into()))
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
// Small xorshift64 PRNG seeded from the wall clock on first use. We
// don't pull in the `rand` crate — a hobby PRNG is plenty for a
// hobby language. Distribution: `next_f64()` is uniform on [0, 1)
// using the top 53 bits of the xorshift output.

thread_local! {
    static RNG_STATE: Cell<u64> = Cell::new(0);
}

fn next_rand_u64() -> u64 {
    RNG_STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdeadbeef);
            // mix the seed so two near-simultaneous starts diverge
            x = nanos ^ 0x9E3779B97F4A7C15;
            if x == 0 { x = 0xdeadbeef; }
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

fn native_rand(_args: &[Value]) -> Result<Value, RuntimeError> {
    // top 53 bits → [0, 2^53) → divide by 2^53 → [0, 1)
    let bits = next_rand_u64() >> 11;
    Ok(Value::Float((bits as f64) / ((1u64 << 53) as f64)))
}
