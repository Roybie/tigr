//! `import 'Path'` — filesystem path manipulation.
//!
//! Pure path computation; none of these entries touch the filesystem.
//! They raise only on a non-String argument.
//!
//! Paths are POSIX-style on every platform: `/` is the one separator and
//! a leading `/` means absolute. This is deliberate — a tigr program
//! (and a game built on it) writes one set of logical paths that behave
//! the same on Linux, macOS, Windows, and the browser, and the native
//! filesystem accepts `/` on Windows too. So these operate on `/`
//! directly rather than going through `std::path`, whose separators and
//! absolute-path rules vary by host.

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

/// Join path segments with `/`. An absolute segment (one starting with
/// `/`) resets the accumulated path, matching the usual `path/push`
/// semantics. Empty segments are skipped.
fn join(args: &[Value]) -> Result<Value, RuntimeError> {
    let mut out = String::new();
    for a in args {
        let seg = expect_string(a, "join")?;
        if seg.is_empty() {
            continue;
        }
        if seg.starts_with('/') {
            out = seg.to_string();
        } else if out.is_empty() || out.ends_with('/') {
            out.push_str(seg);
        } else {
            out.push('/');
            out.push_str(seg);
        }
    }
    Ok(Value::Str(out.into()))
}

/// Everything before the final `/` segment: `dirname('a/b/c.txt')` is
/// `'a/b'`. A path with no `/` has an empty parent; the root is its own.
fn dirname(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "dirname")?;
    let trimmed = strip_trailing_slashes(path);
    let parent = match trimmed.rfind('/') {
        None => "",
        Some(0) => "/",
        Some(i) => &trimmed[..i],
    };
    Ok(Value::Str(parent.into()))
}

/// The final `/` segment: `basename('a/b/c.txt')` is `'c.txt'`.
fn basename(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "basename")?;
    Ok(Value::Str(base(path).into()))
}

/// The extension after the final `.` of the basename, without the dot:
/// `ext('a/b/c.txt')` is `'txt'`. A name with no dot, or a leading-dot
/// dotfile with no other dot (`.bashrc`), has no extension.
fn ext(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "ext")?;
    let name = base(path);
    let e = match name.rfind('.') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => "",
    };
    Ok(Value::Str(e.into()))
}

/// Whether the path is rooted: POSIX-style, a leading `/`.
fn is_absolute(args: &[Value]) -> Result<Value, RuntimeError> {
    let path = expect_string(&args[0], "is_absolute")?;
    Ok(Value::Bool(path.starts_with('/')))
}

/// Drop trailing `/`, but never collapse the root itself to empty.
fn strip_trailing_slashes(path: &str) -> &str {
    let t = path.trim_end_matches('/');
    if t.is_empty() && path.starts_with('/') {
        "/"
    } else {
        t
    }
}

/// The final path segment, used by `basename` and `ext`.
fn base(path: &str) -> &str {
    let t = strip_trailing_slashes(path);
    match t.rfind('/') {
        Some(i) => &t[i + 1..],
        None => t,
    }
}
