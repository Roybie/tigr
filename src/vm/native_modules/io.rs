//! `import 'IO'` — file and stdio operations.
//!
//! Fallible IO (`read_file`, `write_file`, `append_file`, `read_line`,
//! `list_dir`, `mkdir`, `remove`, `stat`) raises a string-valued error
//! on failure — catchable via `try`. Predicate-style entries (`exists`,
//! `is_dir`, `is_file`) never raise. Output entries (`eprint`) match
//! `print` semantics: space-separated args + newline.
//!
//! The genuinely-waiting calls (file reads/writes, directory ops,
//! `read_line`) are *blocking* natives: inside a green thread they are
//! offloaded to a worker pool so IO does not stall the actor's other
//! coroutines (see [`crate::vm::offload`]). The fast metadata-only ops
//! (`exists`, `is_dir`, `is_file`, `stat`) stay inline — offloading a
//! microsecond `stat` would cost more than it saves.

use std::io::Write;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::offload::{BlockingJob, OffloadErr, OffloadOk};
use crate::vm::value::{Arity, Value};

use super::{native, native_blocking, object};

pub fn module() -> Value {
    object(&[
        ("read_file",   native_blocking("read_file",   Arity::Exact(1), read_file)),
        ("write_file",  native_blocking("write_file",  Arity::Exact(2), write_file)),
        ("append_file", native_blocking("append_file", Arity::Exact(2), append_file)),
        ("read_bytes",   native_blocking("read_bytes",   Arity::Exact(1), read_bytes)),
        ("write_bytes",  native_blocking("write_bytes",  Arity::Exact(2), write_bytes)),
        ("append_bytes", native_blocking("append_bytes", Arity::Exact(2), append_bytes)),
        ("exists",      native("exists",      Arity::Exact(1), exists)),
        ("list_dir",    native_blocking("list_dir",    Arity::Exact(1), list_dir)),
        ("mkdir",       native_blocking("mkdir",       Arity::Exact(1), mkdir)),
        ("remove",      native_blocking("remove",      Arity::Exact(1), remove)),
        ("is_dir",      native("is_dir",      Arity::Exact(1), is_dir)),
        ("is_file",     native("is_file",     Arity::Exact(1), is_file)),
        ("stat",        native("stat",        Arity::Exact(1), stat)),
        ("read_line",   native_blocking("read_line",   Arity::Exact(0), read_line)),
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

/// Extract an owned `String` argument for a blocking native — the
/// worker closure outlives the borrowed `Value`.
fn take_string(v: &Value, label: &str) -> Result<String, RuntimeError> {
    Ok(expect_string(v, label)?.to_string())
}

/// Extract an owned `Vec<u8>` argument for a blocking native.
fn take_bytes(v: &Value, label: &str) -> Result<Vec<u8>, RuntimeError> {
    match v {
        Value::Bytes(b) => Ok(b.borrow().clone()),
        other => Err(raise(format!(
            "IO.{label}: expected Bytes, got {}",
            other.type_name()
        ))),
    }
}

fn raise(msg: String) -> RuntimeError {
    // Line is filled in by the VM dispatch site (it knows the
    // calling opcode's line).
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

/// A worker-side IO failure, as the string-valued error the inline
/// version would have raised.
fn io_err(msg: String) -> OffloadErr {
    OffloadErr { kind: None, message: msg }
}

fn read_file(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "read_file")?;
    Ok(Box::new(move || {
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(OffloadOk::Str(s)),
            Err(e) => Err(io_err(format!("read_file({path:?}): {e}"))),
        }
    }))
}

fn write_file(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "write_file")?;
    let contents = take_string(&args[1], "write_file")?;
    Ok(Box::new(move || {
        match std::fs::write(&path, &contents) {
            Ok(()) => Ok(OffloadOk::Unit),
            Err(e) => Err(io_err(format!("write_file({path:?}): {e}"))),
        }
    }))
}

fn append_file(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "append_file")?;
    let contents = take_string(&args[1], "append_file")?;
    Ok(Box::new(move || {
        use std::fs::OpenOptions;
        let r = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(contents.as_bytes()));
        match r {
            Ok(()) => Ok(OffloadOk::Unit),
            Err(e) => Err(io_err(format!("append_file({path:?}): {e}"))),
        }
    }))
}

fn read_bytes(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "read_bytes")?;
    Ok(Box::new(move || {
        match std::fs::read(&path) {
            Ok(b) => Ok(OffloadOk::Bytes(b)),
            Err(e) => Err(io_err(format!("read_bytes({path:?}): {e}"))),
        }
    }))
}

fn write_bytes(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "write_bytes")?;
    let data = take_bytes(&args[1], "write_bytes")?;
    Ok(Box::new(move || {
        match std::fs::write(&path, &data) {
            Ok(()) => Ok(OffloadOk::Unit),
            Err(e) => Err(io_err(format!("write_bytes({path:?}): {e}"))),
        }
    }))
}

fn append_bytes(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "append_bytes")?;
    let data = take_bytes(&args[1], "append_bytes")?;
    Ok(Box::new(move || {
        use std::fs::OpenOptions;
        let r = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(&data));
        match r {
            Ok(()) => Ok(OffloadOk::Unit),
            Err(e) => Err(io_err(format!("append_bytes({path:?}): {e}"))),
        }
    }))
}

fn exists(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "exists")?;
    Ok(Value::Bool(std::path::Path::new(path).exists()))
}

fn list_dir(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "list_dir")?;
    Ok(Box::new(move || {
        // Collect plain `String`s on the worker; the decoder builds
        // the `Value` array back on the actor thread.
        let entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(e) => return Err(io_err(format!("list_dir({path:?}): {e}"))),
        };
        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => names.push(
                    entry.file_name().to_string_lossy().into_owned(),
                ),
                Err(e) => {
                    return Err(io_err(format!("list_dir({path:?}): {e}")))
                }
            }
        }
        Ok(OffloadOk::StrList(names))
    }))
}

fn mkdir(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "mkdir")?;
    Ok(Box::new(move || {
        match std::fs::create_dir_all(&path) {
            Ok(()) => Ok(OffloadOk::Unit),
            Err(e) => Err(io_err(format!("mkdir({path:?}): {e}"))),
        }
    }))
}

fn remove(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "remove")?;
    Ok(Box::new(move || {
        let p = std::path::Path::new(&path);
        let result = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        match result {
            Ok(()) => Ok(OffloadOk::Unit),
            Err(e) => Err(io_err(format!("remove({path:?}): {e}"))),
        }
    }))
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

fn read_line(_args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    Ok(Box::new(|| {
        let mut buf = String::new();
        match std::io::stdin().read_line(&mut buf) {
            Ok(0) => Ok(OffloadOk::StrOrNull(None)), // EOF
            Ok(_) => {
                // Strip trailing \n (and \r on Windows-style input).
                if buf.ends_with('\n') {
                    buf.pop();
                    if buf.ends_with('\r') {
                        buf.pop();
                    }
                }
                Ok(OffloadOk::StrOrNull(Some(buf)))
            }
            Err(e) => Err(io_err(format!("read_line: {e}"))),
        }
    }))
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
