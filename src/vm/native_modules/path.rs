//! `import 'Path'` — filesystem path manipulation.
//!
//! Pure path computation backed by `std::path`; none of these entries
//! touch the filesystem. They raise only on a non-String argument.

use std::path::{Path, PathBuf};

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("join",        native("join",        Arity::AtLeast(1), join)),
        ("dirname",     native("dirname",     Arity::Exact(1),   dirname)),
        ("basename",    native("basename",    Arity::Exact(1),   basename)),
        ("ext",         native("ext",         Arity::Exact(1),   ext)),
        ("is_absolute", native("is_absolute", Arity::Exact(1),   is_absolute)),
    ])
}

fn raise(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

fn expect_string<'a>(v: &'a Value, label: &str) -> Result<&'a str, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(raise(format!(
            "Path.{label}: expected String, got {}",
            other.type_name()
        ))),
    }
}

fn join(args: &[Value]) -> Result<Value, RuntimeError> {
    let mut buf = PathBuf::new();
    for a in args {
        buf.push(expect_string(a, "join")?);
    }
    Ok(Value::Str(buf.to_string_lossy().into_owned().into()))
}

fn dirname(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "dirname")?;
    let parent = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Value::Str(parent.into()))
}

fn basename(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "basename")?;
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Value::Str(name.into()))
}

fn ext(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "ext")?;
    let e = Path::new(path)
        .extension()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Value::Str(e.into()))
}

fn is_absolute(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "is_absolute")?;
    Ok(Value::Bool(Path::new(path).is_absolute()))
}
