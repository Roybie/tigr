//! `import 'IO'` — file and stdio operations.
//!
//! Fallible IO (`read_file`, `write_file`, `append_file`, `read_line`)
//! raises a `Raised(String)` error on failure — catchable via `try`.
//! Predicate-style entries (`exists`) never raise. Output entries
//! (`eprint`) match `print` semantics: space-separated args + newline.

use std::io::Write;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("read_file",   native("read_file",   Arity::Exact(1), read_file)),
        ("write_file",  native("write_file",  Arity::Exact(2), write_file)),
        ("append_file", native("append_file", Arity::Exact(2), append_file)),
        ("exists",      native("exists",      Arity::Exact(1), exists)),
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
