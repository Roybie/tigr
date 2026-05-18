//! Runtime task handle — the result of `spawn` (v0.14 concurrency).
//!
//! A `spawn` starts an actor on its own OS thread and immediately
//! yields a `Task`. `Task.join` blocks until the actor finishes and
//! then produces its result — decoded into the joining thread's heap,
//! or re-raised as the actor's error. The handle is `Arc`-backed and
//! `Send`, so it can itself be passed between actors.

use std::sync::{Arc, Condvar, Mutex};

use crate::vm::transfer::{Transfer, TransferError};

/// A shared handle to a spawned actor's eventual result.
pub type TaskHandle = Arc<TaskInner>;

/// The actor's outcome: its return value transfer-encoded, or its
/// uncaught error rendered to `Send`-able form.
pub type ActorOutcome = Result<Transfer, TransferError>;

enum TaskState {
    /// The actor is still running.
    Pending,
    /// The actor finished; its outcome has not been collected yet.
    Ready(ActorOutcome),
    /// The outcome has already been taken by a `join`.
    Taken,
}

pub struct TaskInner {
    state: Mutex<TaskState>,
    done: Condvar,
}

/// The result of [`TaskInner::join`].
pub enum JoinOutcome {
    /// The actor's outcome (first `join` only).
    Outcome(ActorOutcome),
    /// A second `join` on a task whose result was already taken.
    AlreadyJoined,
}

impl TaskInner {
    pub fn new() -> TaskHandle {
        Arc::new(TaskInner {
            state: Mutex::new(TaskState::Pending),
            done: Condvar::new(),
        })
    }

    /// Called by the worker thread when the actor finishes.
    pub fn complete(&self, outcome: ActorOutcome) {
        let mut g = self.state.lock().unwrap();
        *g = TaskState::Ready(outcome);
        drop(g);
        self.done.notify_all();
    }

    /// Block until the actor finishes, then hand back its outcome.
    /// A second join reports [`JoinOutcome::AlreadyJoined`].
    pub fn join(&self) -> JoinOutcome {
        let mut g = self.state.lock().unwrap();
        loop {
            match &*g {
                TaskState::Pending => {
                    g = self.done.wait(g).unwrap();
                }
                TaskState::Taken => return JoinOutcome::AlreadyJoined,
                TaskState::Ready(_) => {
                    match std::mem::replace(&mut *g, TaskState::Taken) {
                        TaskState::Ready(o) => return JoinOutcome::Outcome(o),
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
}
