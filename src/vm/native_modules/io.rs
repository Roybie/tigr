//! `import 'IO'` — file and stdio operations.
//!
//! Two flavours of file IO live in this module:
//!
//! * Whole-file path-based ops (`read_file`, `write_file`, `append_file`,
//!   `read_bytes`, `write_bytes`, `append_bytes`) — convenient for small
//!   files. Errors are plain string values.
//! * Streaming handle-based ops (`open`, `read`, `read_line`, `read_until`,
//!   `read_exact`, `read_all`, `write`, `seek`, `tell`, `close`) — the
//!   only way to process a file larger than memory. Errors are structured
//!   `${kind, message}` like `Net` — `catch e { e.kind == 'eof' }`.
//!
//! Predicate-style entries (`exists`, `is_dir`, `is_file`) never raise.
//! Output entries (`eprint`) match `print` semantics: space-separated
//! args + newline.
//!
//! The genuinely-waiting calls are *blocking* natives: inside a green
//! thread they are offloaded to a worker pool so IO does not stall the
//! actor's other coroutines (see [`crate::vm::offload`]). The fast
//! metadata-only ops (`exists`, `is_dir`, `is_file`, `stat`, plus
//! `seek` / `tell` / `close` on a handle) stay inline — offloading a
//! microsecond syscall would cost more than it saves.

use std::io::Write;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::file_handle::{FileError, FileHandle, FileInner, FileMode};
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
        // Streaming file handles. `read_line` accepts 0 args (stdin)
        // or 1 arg (a FileHandle).
        ("open",        native_blocking("open",        Arity::Exact(2), open)),
        ("read",        native_blocking("read",        Arity::Exact(2), read)),
        ("read_exact",  native_blocking("read_exact",  Arity::Exact(2), read_exact)),
        ("read_line",   native_blocking("read_line",   Arity::Range(0, 1), read_line)),
        ("read_until",  native_blocking("read_until",  Arity::Exact(2), read_until)),
        ("read_all",    native_blocking("read_all",    Arity::Exact(1), read_all)),
        ("write",       native_blocking("write",       Arity::Exact(2), write)),
        ("seek",        native("seek",        Arity::Exact(2), seek)),
        ("tell",        native("tell",        Arity::Exact(1), tell)),
        ("close",       native("close",       Arity::Exact(1), close)),
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

// ---------------------------------------------------------------------
// Streaming-handle error helpers (structured, like `Net`)
// ---------------------------------------------------------------------

/// A catchable structured error `${kind, message}` for the streaming
/// file API — matches `Net`'s convention so a `catch` block can
/// dispatch on `.kind` (`io`, `eof`, `closed`, `mode`, `invalid_mode`,
/// `decode`).
fn file_err(kind: &str, msg: String) -> RuntimeError {
    let obj = object(&[
        ("kind", Value::Str(kind.into())),
        ("message", Value::Str(msg.into())),
    ]);
    RuntimeError::new(RuntimeErrorKind::Raised(obj), 0)
}

/// Classify a [`FileError`] into a structured error kind + message.
fn classify_file(label: &str, e: FileError) -> (&'static str, String) {
    match e {
        FileError::Closed => ("closed", format!("IO.{label}: file is closed")),
        FileError::WrongMode(msg) => ("mode", format!("IO.{label}: {msg}")),
        FileError::Eof(msg) => ("eof", format!("IO.{label}: {msg}")),
        FileError::Decode(msg) => ("decode", format!("IO.{label}: {msg}")),
        FileError::InvalidMode(msg) => {
            ("invalid_mode", format!("IO.{label}: {msg}"))
        }
        FileError::Io(io_err) => ("io", format!("IO.{label}: {io_err}")),
    }
}

/// Map a [`FileError`] to a tigr `RuntimeError` for inline natives.
fn map_file_err(label: &str, e: FileError) -> RuntimeError {
    let (kind, msg) = classify_file(label, e);
    file_err(kind, msg)
}

/// Map a [`FileError`] to an [`OffloadErr`] — the POD error a worker
/// thread posts back; [`crate::vm::offload::decode`] rebuilds the same
/// structured error the inline call would raise.
fn offload_file_err(label: &str, e: FileError) -> OffloadErr {
    let (kind, message) = classify_file(label, e);
    OffloadErr { kind: Some(kind.to_string()), message }
}

/// Extract an owned file-handle argument. A `FileHandle` is an `Arc`,
/// so the clone is a refcount bump.
fn take_file(v: &Value, label: &str) -> Result<FileHandle, RuntimeError> {
    match v {
        Value::File(h) => Ok(h.clone()),
        other => Err(file_err(
            "mode",
            format!(
                "IO.{label}: expected a file handle, got {}",
                other.type_name()
            ),
        )),
    }
}

/// A non-negative byte-count argument.
fn expect_count(v: &Value, label: &str) -> Result<usize, RuntimeError> {
    match v {
        Value::Int(n) if *n >= 0 => Ok(*n as usize),
        Value::Int(n) => Err(file_err(
            "io",
            format!("IO.{label}: negative count {n}"),
        )),
        other => Err(file_err(
            "io",
            format!(
                "IO.{label}: expected Int, got {}",
                other.type_name()
            ),
        )),
    }
}

/// A single byte (`Int` in `0..=255`).
fn expect_byte(v: &Value, label: &str) -> Result<u8, RuntimeError> {
    match v {
        Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
        Value::Int(n) => Err(file_err(
            "io",
            format!("IO.{label}: byte value {n} out of range 0..=255"),
        )),
        other => Err(file_err(
            "io",
            format!(
                "IO.{label}: expected Int, got {}",
                other.type_name()
            ),
        )),
    }
}

/// `data` for `write` — accepts a `Bytes` buffer or a `String` (which
/// is written as its UTF-8 bytes).
fn take_write_data(v: &Value) -> Result<Vec<u8>, RuntimeError> {
    match v {
        Value::Bytes(b) => Ok(b.borrow().clone()),
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        other => Err(file_err(
            "io",
            format!(
                "IO.write: expected Bytes or String, got {}",
                other.type_name()
            ),
        )),
    }
}

// ---------------------------------------------------------------------
// Whole-file path-based ops (unchanged from earlier IO surface).
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// Streaming file handle ops.
// ---------------------------------------------------------------------

fn open(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let path = take_string(&args[0], "open")?;
    let mode_str = take_string(&args[1], "open")?;
    let Some(mode) = FileMode::parse(&mode_str) else {
        return Err(file_err(
            "invalid_mode",
            format!(
                "IO.open: unknown mode {mode_str:?} (expected 'r', 'w', or 'a')"
            ),
        ));
    };
    Ok(Box::new(move || match FileInner::open(path.clone(), mode) {
        Ok(handle) => Ok(OffloadOk::File(handle)),
        Err(e) => Err(offload_file_err(
            &format!("open({path:?})"),
            e,
        )),
    }))
}

fn read(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let handle = take_file(&args[0], "read")?;
    let n = expect_count(&args[1], "read")?;
    Ok(Box::new(move || match handle.read_chunk(n) {
        Ok(b) => Ok(OffloadOk::Bytes(b)),
        Err(e) => Err(offload_file_err("read", e)),
    }))
}

fn read_exact(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let handle = take_file(&args[0], "read_exact")?;
    let n = expect_count(&args[1], "read_exact")?;
    Ok(Box::new(move || match handle.read_exact(n) {
        Ok(b) => Ok(OffloadOk::Bytes(b)),
        Err(e) => Err(offload_file_err("read_exact", e)),
    }))
}

fn read_line(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    if args.is_empty() {
        // Stdin form, unchanged from earlier.
        return Ok(Box::new(|| {
            let mut buf = String::new();
            match std::io::stdin().read_line(&mut buf) {
                Ok(0) => Ok(OffloadOk::StrOrNull(None)), // EOF
                Ok(_) => {
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
        }));
    }
    let handle = take_file(&args[0], "read_line")?;
    Ok(Box::new(move || match handle.read_line() {
        Ok(line) => Ok(OffloadOk::StrOrNull(line)),
        Err(e) => Err(offload_file_err("read_line", e)),
    }))
}

fn read_until(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let handle = take_file(&args[0], "read_until")?;
    let delim = expect_byte(&args[1], "read_until")?;
    Ok(Box::new(move || match handle.read_until(delim) {
        Ok(b) => Ok(OffloadOk::BytesOrNull(b)),
        Err(e) => Err(offload_file_err("read_until", e)),
    }))
}

fn read_all(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let handle = take_file(&args[0], "read_all")?;
    Ok(Box::new(move || match handle.read_all() {
        Ok(b) => Ok(OffloadOk::Bytes(b)),
        Err(e) => Err(offload_file_err("read_all", e)),
    }))
}

fn write(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let handle = take_file(&args[0], "write")?;
    let data = take_write_data(&args[1])?;
    Ok(Box::new(move || match handle.write_all(&data) {
        Ok(n) => Ok(OffloadOk::Int(n as i64)),
        Err(e) => Err(offload_file_err("write", e)),
    }))
}

fn seek(args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = take_file(&args[0], "seek")?;
    let pos = match &args[1] {
        Value::Int(n) => *n,
        other => {
            return Err(file_err(
                "io",
                format!(
                    "IO.seek: expected Int, got {}",
                    other.type_name()
                ),
            ))
        }
    };
    handle.seek(pos).map_err(|e| map_file_err("seek", e))?;
    Ok(Value::Null)
}

fn tell(args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = take_file(&args[0], "tell")?;
    let pos = handle.tell().map_err(|e| map_file_err("tell", e))?;
    Ok(Value::Int(pos))
}

fn close(args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = take_file(&args[0], "close")?;
    handle.close();
    Ok(Value::Null)
}

fn eprint(args: &[Value]) -> Result<Value, RuntimeError> {
    // When an embedder (the browser playground) has installed a capture
    // buffer, the line goes there; otherwise straight to stderr.
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
        let stderr = std::io::stderr();
        let mut h = stderr.lock();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                let _ = write!(h, " ");
            }
            let _ = write!(h, "{arg}");
        }
        let _ = writeln!(h);
    }
    // Mirror `print`: return the last arg (or null) so eprint composes.
    Ok(args.last().cloned().unwrap_or(Value::Null))
}
