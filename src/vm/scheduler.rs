//! Cooperative green-thread scheduling within a single actor.
//!
//! A *green thread* (coroutine) is a suspended slice of VM execution
//! state — its own call-frame stack, value stack and open-upvalue
//! list. Every green thread in one actor shares that actor's
//! thread-local heap; the scheduler multiplexes them cooperatively
//! onto the single OS thread, switching only at `yield` points and
//! when a coroutine returns. There is no preemption: a coroutine that
//! never yields starves the others.
//!
//! The *running* coroutine's execution state lives directly in the
//! `Vm`'s own fields. Only *parked* coroutines are stored here as
//! [`GreenThread`]s. Coroutine #0 is always the actor's main program.

use std::collections::VecDeque;

use crate::vm::gc::{GcRef, UpvalueKind};
use crate::vm::value::Value;
use crate::vm::vm::CallFrame;

/// A suspended green thread: everything needed to resume it later.
pub struct GreenThread {
    pub(crate) id: u32,
    /// True for coroutine #0 — the actor's main program. When main
    /// returns the actor finishes; a returning non-main coroutine just
    /// hands control to the next ready one.
    pub(crate) is_main: bool,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) stack: Vec<Value>,
    pub(crate) open_upvalues: Vec<GcRef<UpvalueKind>>,
    /// `Some(v)` when parked at a `Yield`: `v` is pushed onto the stack
    /// on resume so the `yield` expression evaluates to it. `None` for
    /// a coroutine that has not started running yet.
    pub(crate) parked_resume: Option<Value>,
}

/// Per-actor cooperative scheduler: a FIFO run-queue of ready,
/// not-currently-running coroutines plus bookkeeping for the running
/// one.
pub struct Scheduler {
    queue: VecDeque<GreenThread>,
    next_id: u32,
    current_id: u32,
    current_is_main: bool,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            queue: VecDeque::new(),
            next_id: 1,
            current_id: 0,
            current_is_main: true,
        }
    }

    /// Reset to a single running main coroutine (#0). Called when a
    /// `Vm` (re)starts a top-level program or an actor closure.
    pub fn reset(&mut self) {
        self.queue.clear();
        self.next_id = 1;
        self.current_id = 0;
        self.current_is_main = true;
    }

    /// Is the currently-running coroutine the actor's main program?
    pub fn current_is_main(&self) -> bool {
        self.current_is_main
    }

    /// The running coroutine's `(id, is_main)`.
    pub fn current(&self) -> (u32, bool) {
        (self.current_id, self.current_is_main)
    }

    /// Allocate the next coroutine id.
    pub fn fresh_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Append a ready coroutine to the back of the run-queue.
    pub fn enqueue(&mut self, gt: GreenThread) {
        self.queue.push_back(gt);
    }

    /// Take the next ready coroutine, if any (round-robin: front).
    pub fn take_next(&mut self) -> Option<GreenThread> {
        self.queue.pop_front()
    }

    /// Record which coroutine is now running. Called on every switch.
    pub fn set_current(&mut self, id: u32, is_main: bool) {
        self.current_id = id;
        self.current_is_main = is_main;
    }

    /// All parked coroutines — for GC root tracing.
    pub fn queued(&self) -> impl Iterator<Item = &GreenThread> {
        self.queue.iter()
    }

    /// Borrow a parked coroutine's value stack by id — used to resolve
    /// an open upvalue that a `go` block captured from another
    /// coroutine. Returns `None` if no parked coroutine has that id.
    pub fn stack_of(&self, id: u32) -> Option<&Vec<Value>> {
        self.queue.iter().find(|gt| gt.id == id).map(|gt| &gt.stack)
    }

    /// Mutable counterpart of [`stack_of`].
    pub fn stack_of_mut(&mut self, id: u32) -> Option<&mut Vec<Value>> {
        self.queue
            .iter_mut()
            .find(|gt| gt.id == id)
            .map(|gt| &mut gt.stack)
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
