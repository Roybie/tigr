//! Built-in functions exposed as ordinary bindings (spec §13).
//!
//! Adding a new built-in: define the
//! `fn(&[Value]) -> Result<Value, RuntimeError>`, append a `Spec`
//! entry to `BUILTINS`. The compiler pre-declares each name as a
//! resolvable global; the VM populates the matching `Value::NativeFn`
//! instances at startup.

use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::rng;
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
    Spec { name: "str",   arity: Arity::Range(1, 3), func: native_str },
    Spec { name: "num",   arity: Arity::Exact(1), func: native_num },
    Spec { name: "int",   arity: Arity::Exact(1), func: native_int },
    Spec { name: "float", arity: Arity::Exact(1), func: native_float },
    Spec { name: "bool",  arity: Arity::Exact(1), func: native_bool },
    Spec { name: "floor", arity: Arity::Exact(1), func: native_floor },
    Spec { name: "ceil",  arity: Arity::Exact(1), func: native_ceil },
    Spec { name: "rand",  arity: Arity::Exact(0), func: native_rand },
    Spec { name: "type",  arity: Arity::Exact(1), func: native_type },
    Spec { name: "gc",    arity: Arity::Exact(0), func: native_gc },
];

const BUILTIN_NAMES: [&str; 11] = [
    "print", "str", "num", "int", "float", "bool", "floor", "ceil", "rand",
    "type", "gc",
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

fn native_gc(_args: &[Value]) -> Result<Value, RuntimeError> {
    let s = gc::stats();
    let mut m: IndexMap<Rc<str>, Value> = IndexMap::with_capacity(4);
    m.insert(Rc::from("live"), Value::Int(s.live as i64));
    m.insert(Rc::from("collections"), Value::Int(s.collections as i64));
    m.insert(Rc::from("allocated"), Value::Int(s.total_allocated as i64));
    m.insert(Rc::from("freed"), Value::Int(s.total_freed as i64));
    Ok(Value::Object(gc::alloc_object(m)))
}
