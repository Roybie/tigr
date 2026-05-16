//! `import '_NativeString'` — Rust string primitives.
//!
//! Backend for `stdlib/String.tg`. Pure-tigr versions of these would
//! be O(n) per character (every `s[i]` walks UTF-8 from the start),
//! so we expose Rust implementations that are linear over bytes.

use std::cell::RefCell;
use std::rc::Rc;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("split",       native("split",       Arity::Exact(2), s_split)),
        ("replace",     native("replace",     Arity::Exact(3), s_replace)),
        ("contains",    native("contains",    Arity::Exact(2), s_contains)),
        ("index_of",    native("index_of",    Arity::Exact(2), s_index_of)),
        ("lower",       native("lower",       Arity::Exact(1), s_lower)),
        ("upper",       native("upper",       Arity::Exact(1), s_upper)),
        ("starts_with", native("starts_with", Arity::Exact(2), s_starts_with)),
        ("ends_with",   native("ends_with",   Arity::Exact(2), s_ends_with)),
        ("trim",        native("trim",        Arity::Exact(1), s_trim)),
        ("trim_start",  native("trim_start",  Arity::Exact(1), s_trim_start)),
        ("trim_end",    native("trim_end",    Arity::Exact(1), s_trim_end)),
        ("repeat",      native("repeat",      Arity::Exact(2), s_repeat)),
        ("chars",       native("chars",       Arity::Exact(1), s_chars)),
    ])
}

fn as_str<'a>(v: &'a Value, label: &str) -> Result<&'a str, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(format!(
                "String.{label}: expected String, got {}",
                other.type_name()
            ).into())),
            0,
        )),
    }
}

fn s_split(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "split")?;
    let sep = as_str(&args[1], "split")?;
    let parts: Vec<Value> = if sep.is_empty() {
        // Empty separator → one-char strings (matches `chars`).
        s.chars().map(|c| Value::Str(c.to_string().into())).collect()
    } else {
        s.split(sep).map(|p| Value::Str(p.into())).collect()
    };
    Ok(Value::Array(Rc::new(RefCell::new(parts))))
}

fn s_replace(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "replace")?;
    let from = as_str(&args[1], "replace")?;
    let to = as_str(&args[2], "replace")?;
    if from.is_empty() {
        // Match Rust's behavior would be to insert `to` between every
        // char; that's surprising. Return the source unchanged.
        return Ok(Value::Str(s.into()));
    }
    Ok(Value::Str(s.replace(from, to).into()))
}

fn s_contains(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "contains")?;
    let needle = as_str(&args[1], "contains")?;
    Ok(Value::Bool(s.contains(needle)))
}

/// Byte index of the first occurrence, or -1 if not found.
/// (Bytes, not chars — fine for ASCII; consistent with `len`-via-`#`
/// where `#'café' == 5` because tigr counts bytes.)
fn s_index_of(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "index_of")?;
    let needle = as_str(&args[1], "index_of")?;
    match s.find(needle) {
        Some(i) => Ok(Value::Int(i as i64)),
        None => Ok(Value::Int(-1)),
    }
}

fn s_lower(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "lower")?;
    Ok(Value::Str(s.to_lowercase().into()))
}

fn s_upper(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "upper")?;
    Ok(Value::Str(s.to_uppercase().into()))
}

fn s_starts_with(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "starts_with")?;
    let prefix = as_str(&args[1], "starts_with")?;
    Ok(Value::Bool(s.starts_with(prefix)))
}

fn s_ends_with(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "ends_with")?;
    let suffix = as_str(&args[1], "ends_with")?;
    Ok(Value::Bool(s.ends_with(suffix)))
}

fn s_trim(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "trim")?;
    Ok(Value::Str(s.trim().into()))
}

fn s_trim_start(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "trim_start")?;
    Ok(Value::Str(s.trim_start().into()))
}

fn s_trim_end(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "trim_end")?;
    Ok(Value::Str(s.trim_end().into()))
}

fn s_repeat(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "repeat")?;
    let n = match &args[1] {
        Value::Int(n) if *n >= 0 => *n as usize,
        Value::Int(_) => return Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(
                "String.repeat: negative count".into())),
            0,
        )),
        other => return Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(format!(
                "String.repeat: count must be Int, got {}", other.type_name()
            ).into())),
            0,
        )),
    };
    Ok(Value::Str(s.repeat(n).into()))
}

fn s_chars(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "chars")?;
    let parts: Vec<Value> = s
        .chars()
        .map(|c| Value::Str(c.to_string().into()))
        .collect();
    Ok(Value::Array(Rc::new(RefCell::new(parts))))
}
