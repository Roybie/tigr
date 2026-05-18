//! `import 'Task'` — joining spawned actors (v0.14 concurrency).
//!
//! `spawn` (a keyword) yields a `Task`; `Task.join` is the only
//! operation on one. It blocks until the actor finishes, then either
//! decodes the actor's return value into the caller's heap, or
//! re-raises the actor's error so the caller can `try`/`catch` it.

use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::task::JoinOutcome;
use crate::vm::transfer::{decode, TransferError};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[("join", native("join", Arity::Exact(1), t_join))])
}

/// `join(task)` — block for the actor's result. Returns its value, or
/// raises: the actor's own `raise`d value verbatim, or — for a
/// built-in actor error — an object `${kind, message, trace, worker}`.
pub(crate) fn t_join(args: &[Value]) -> Result<Value, RuntimeError> {
    let task = match &args[0] {
        Value::Task(t) => t,
        other => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "Task.join expects a task, got {}",
                    other.type_name()
                )),
                0,
            ));
        }
    };
    match task.join() {
        JoinOutcome::Outcome(Ok(transfer)) => Ok(decode(transfer)),
        JoinOutcome::Outcome(Err(te)) => Err(actor_error(te)),
        JoinOutcome::AlreadyJoined => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(
                "task has already been joined".into(),
            )),
            0,
        )),
    }
}

/// Reconstruct a catchable error in the joining actor from a worker's
/// `TransferError`.
fn actor_error(te: TransferError) -> RuntimeError {
    // A `raise <value>` in the worker re-raises that exact value, so
    // the parent's `catch` binds what the worker raised.
    if let Some(raised) = te.raised {
        return RuntimeError::new(RuntimeErrorKind::Raised(decode(raised)), 0);
    }
    // A built-in worker error surfaces as an object carrying the
    // worker's kind, message, and rendered trace.
    let mut m: IndexMap<Rc<str>, Value> = IndexMap::with_capacity(4);
    m.insert(Rc::from("kind"), Value::Str(te.kind_tag.into()));
    m.insert(Rc::from("message"), Value::Str(te.message.into()));
    m.insert(Rc::from("trace"), Value::Str(te.rendered_trace.into()));
    m.insert(Rc::from("worker"), Value::Bool(true));
    RuntimeError::new(
        RuntimeErrorKind::Raised(Value::Object(gc::alloc_object(m))),
        0,
    )
}
