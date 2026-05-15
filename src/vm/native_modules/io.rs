//! `import 'IO'` — file and stdio operations.
//!
//! Fallible IO (`read_file`, `write_file`, `append_file`, `read_line`,
//! `list_dir`, `mkdir`, `remove`, `stat`) raises a `Raised(String)` error
//! on failure — catchable via `try`. Predicate-style entries (`exists`,
//! `is_dir`, `is_file`) never raise. Output entries (`eprint`) match
//! `print` semantics: space-separated args + newline.

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("read_file",   native("read_file",   Arity::Exact(1), read_file)),
        ("write_file",  native("write_file",  Arity::Exact(2), write_file)),
        ("append_file", native("append_file", Arity::Exact(2), append_file)),
        ("exists",      native("exists",      Arity::Exact(1), exists)),
        ("list_dir",    native("list_dir",    Arity::Exact(1), list_dir)),
        ("mkdir",       native("mkdir",       Arity::Exact(1), mkdir)),
        ("remove",      native("remove",      Arity::Exact(1), remove)),
        ("is_dir",      native("is_dir",      Arity::Exact(1), is_dir)),
        ("is_file",     native("is_file",     Arity::Exact(1), is_file)),
        ("stat",        native("stat",        Arity::Exact(1), stat)),
        ("read_line",   native("read_line",   Arity::Exact(0), read_line)),
        ("eprint",      native("eprint",      Arity::Variadic, eprint)),
    ])
}

fn expect_string<'a>(v: &'a Value, label: &str) -> Result<&'a str, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(raise(format!(
            "IO.{label}: expected String, got {}",
            other.type_name()
        ))),
    }
}

fn raise(msg: String) -> RuntimeError {
    // Line is filled in by the VM dispatch site (it knows the
    // calling opcode's line).
    RuntimeError::new(RuntimeErrorKind::Raised(msg), 0)
}

fn read_file(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "read_file")?;
    std::fs::read_to_string(path)
        .map(|s| Value::Str(s.into()))
        .map_err(|e| raise(format!("read_file({path:?}): {e}")))
}

fn write_file(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "write_file")?;
    let contents = expect_string(&args[1], "write_file")?;
    std::fs::write(path, contents)
        .map(|_| Value::Null)
        .map_err(|e| raise(format!("write_file({path:?}): {e}")))
}

fn append_file(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "append_file")?;
    let contents = expect_string(&args[1], "append_file")?;
    use std::fs::OpenOptions;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(contents.as_bytes()))
        .map(|_| Value::Null)
        .map_err(|e| raise(format!("append_file({path:?}): {e}")))
}

fn exists(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "exists")?;
    Ok(Value::Bool(std::path::Path::new(path).exists()))
}

fn list_dir(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "list_dir")?;
    let entries =
        std::fs::read_dir(path).map_err(|e| raise(format!("list_dir({path:?}): {e}")))?;
    let mut names: Vec<Value> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| raise(format!("list_dir({path:?}): {e}")))?;
        names.push(Value::Str(entry.file_name().to_string_lossy().into_owned().into()));
    }
    Ok(Value::Array(Rc::new(RefCell::new(names))))
}

fn mkdir(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "mkdir")?;
    std::fs::create_dir_all(path)
        .map(|_| Value::Null)
        .map_err(|e| raise(format!("mkdir({path:?}): {e}")))
}

fn remove(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "remove")?;
    let p = std::path::Path::new(path);
    let result = if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    };
    result
        .map(|_| Value::Null)
        .map_err(|e| raise(format!("remove({path:?}): {e}")))
}

fn is_dir(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "is_dir")?;
    Ok(Value::Bool(std::path::Path::new(path).is_dir()))
}

fn is_file(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "is_file")?;
    Ok(Value::Bool(std::path::Path::new(path).is_file()))
}

fn stat(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "stat")?;
    let meta = std::fs::metadata(path).map_err(|e| raise(format!("stat({path:?}): {e}")))?;
    // `modified()` is unsupported on a few exotic platforms; fall back
    // to `null` rather than failing the whole call.
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| Value::Int(d.as_millis() as i64))
        .unwrap_or(Value::Null);
    Ok(object(&[
        ("size", Value::Int(meta.len() as i64)),
        ("is_dir", Value::Bool(meta.is_dir())),
        ("is_file", Value::Bool(meta.is_file())),
        ("modified_ms", modified_ms),
    ]))
}

fn read_line(_args: &[Value]) -> Result<Value, RuntimeError> {
    let mut buf = String::new();
    match std::io::stdin().read_line(&mut buf) {
        Ok(0) => Ok(Value::Null), // EOF
        Ok(_) => {
            // Strip trailing \n (and \r on Windows-style input).
            if buf.ends_with('\n') {
                buf.pop();
                if buf.ends_with('\r') {
                    buf.pop();
                }
            }
            Ok(Value::Str(buf.into()))
        }
        Err(e) => Err(raise(format!("read_line: {e}"))),
    }
}

fn eprint(args: &[Value]) -> Result<Value, RuntimeError> {
    let stderr = std::io::stderr();
    let mut h = stderr.lock();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            let _ = write!(h, " ");
        }
        let _ = write!(h, "{arg}");
    }
    let _ = writeln!(h);
    // Mirror `print`: return the last arg (or null) so eprint composes.
    Ok(args.last().cloned().unwrap_or(Value::Null))
}
