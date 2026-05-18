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

use crate::vm::error::RuntimeError;
use crate::vm::gc::{GcRef, GreenHandleKind, UpvalueKind};
use crate::vm::value::Value;
use crate::vm::vm::CallFrame;

/// How a parked coroutine resumes once it is unblocked. A `yield` or a
/// `join` resumes with a plain value pushed onto its stack; an
/// offloaded blocking call that *failed* resumes by raising — the error
/// has to surface against the coroutine's own frames and `try` blocks,
/// not the one that happened to be running when the worker finished.
pub enum ResumeOutcome {
    Value(Value),
    Raise(RuntimeError),
}

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
    /// `Some(outcome)` when parked at a `Yield`, a cooperative `join`,
    /// or an offloaded blocking call: the outcome is delivered on
    /// resume — a `Value` pushed onto the stack, or a `Raise` that
    /// surfaces as an error. `None` for a coroutine that has not
    /// started running yet, or one parked with no result to deliver.
    pub(crate) parked_resume: Option<ResumeOutcome>,
    /// The `${...}`-less handle a `go` minted for this coroutine —
    /// where its return value is recorded so a `join` can read it.
    /// `None` for the actor's main coroutine (#0), which has no handle.
    pub(crate) handle: Option<GcRef<GreenHandleKind>>,
}

/// The result side of a `go` — the value a green-thread handle hands
/// back to a `join`. Phase 4: `go` evaluates to a `Value::GreenHandle`
/// wrapping one of these on the GC heap.
pub struct GreenHandle {
    /// The green thread's coroutine id — what a `join` waits on and
    /// what a finishing coroutine wakes its joiners by.
    pub(crate) id: u32,
    /// `None` while the coroutine is still running; `Some(v)` once it
    /// has returned, holding `v` — the value its body evaluated to.
    pub(crate) result: Option<Value>,
}

/// A coroutine parked in a cooperative `join`, waiting for another
/// green thread to finish before it can resume.
struct BlockedThread {
    /// The coroutine id this thread is joining on.
    awaiting: u32,
    thread: GreenThread,
}

/// A coroutine parked on an in-flight offloaded blocking call, waiting
/// for the worker pool to post that job's completion.
struct IoBlockedThread {
    /// The offload job id this thread is waiting on.
    job_id: u64,
    thread: GreenThread,
}

/// Lifecycle of a generator coroutine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenStatus {
    /// Parked — `GeneratorState`'s fields hold the coroutine, ready to
    /// resume. Covers both "not yet started" and "paused at a yield".
    Suspended,
    /// Resumed: the coroutine's state is live in the `Vm`, or it is an
    /// ancestor on the resume chain. Resuming it again is an error.
    Running,
    /// The body returned. `next()` reports `${ done: true }` forever.
    Done,
}

/// A generator coroutine: a suspended slice of VM execution state,
/// resumed on demand by the `${ next: fn() }` iterator object that
/// wraps it. Unlike a [`GreenThread`] it is not scheduled round-robin
/// — it runs only when something pulls its `next()`, and `yield`
/// inside it hands a value back to that puller. Lives on the GC heap
/// (`GeneratorKind`); `Trace` keeps the parked state's roots alive.
pub struct GeneratorState {
    /// Scheduler coroutine id — its own slot in the owner-tagged
    /// open-upvalue scheme, so a closure created inside the generator
    /// body resolves against the right value stack.
    pub(crate) id: u32,
    pub(crate) status: GenStatus,
    /// The parked coroutine. Populated iff `status == Suspended`;
    /// emptied while the generator runs (its state is in the `Vm`) and
    /// once it is `Done`.
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) stack: Vec<Value>,
    pub(crate) open_upvalues: Vec<GcRef<UpvalueKind>>,
}

/// Per-actor cooperative scheduler: a FIFO run-queue of ready,
/// not-currently-running coroutines plus bookkeeping for the running
/// one.
pub struct Scheduler {
    queue: VecDeque<GreenThread>,
    /// Coroutines parked in a cooperative `join` — not ready to run
    /// until the green thread each awaits finishes. Kept off `queue`
    /// so the round-robin never schedules a thread that would only
    /// re-block immediately.
    blocked: Vec<BlockedThread>,
    /// Coroutines parked on an in-flight offloaded blocking call — not
    /// ready to run until the worker pool posts their job's completion.
    /// Kept off `queue` for the same reason as `blocked`.
    io_blocked: Vec<IoBlockedThread>,
    next_id: u32,
    current_id: u32,
    current_is_main: bool,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            queue: VecDeque::new(),
            blocked: Vec::new(),
            io_blocked: Vec::new(),
            next_id: 1,
            current_id: 0,
            current_is_main: true,
        }
    }

    /// Reset to a single running main coroutine (#0). Called when a
    /// `Vm` (re)starts a top-level program or an actor closure.
    pub fn reset(&mut self) {
        self.queue.clear();
        self.blocked.clear();
        self.io_blocked.clear();
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

    /// Park `thread` until green thread `awaiting` finishes. Used by a
    /// cooperative `join`: the joiner leaves the run-queue entirely and
    /// is re-enqueued by [`wake_joiners`] when its target returns.
    pub fn block(&mut self, awaiting: u32, thread: GreenThread) {
        self.blocked.push(BlockedThread { awaiting, thread });
    }

    /// Green thread `finished` returned with `result`: move every
    /// coroutine that was `join`-blocked on it back onto the run-queue,
    /// delivering `result` as the value its `join` expression yields.
    pub fn wake_joiners(&mut self, finished: u32, result: &Value) {
        let mut i = 0;
        while i < self.blocked.len() {
            if self.blocked[i].awaiting == finished {
                let mut bt = self.blocked.swap_remove(i);
                bt.thread.parked_resume =
                    Some(ResumeOutcome::Value(result.clone()));
                self.queue.push_back(bt.thread);
            } else {
                i += 1;
            }
        }
    }

    /// Park `thread` on the in-flight offload job `job_id`. Used when a
    /// coroutine calls a blocking native that has been offloaded to the
    /// worker pool; [`wake_io`] re-enqueues it once the job completes.
    pub fn park_io(&mut self, job_id: u64, thread: GreenThread) {
        self.io_blocked.push(IoBlockedThread { job_id, thread });
    }

    /// Offload job `job_id` completed: move the coroutine parked on it
    /// back onto the run-queue, delivering `outcome` (its decoded value,
    /// or a raise if the blocking call failed). Returns `false` if no
    /// coroutine was parked on that id — defensive; should not happen.
    pub fn wake_io(&mut self, job_id: u64, outcome: ResumeOutcome) -> bool {
        if let Some(pos) =
            self.io_blocked.iter().position(|t| t.job_id == job_id)
        {
            let mut t = self.io_blocked.swap_remove(pos);
            t.thread.parked_resume = Some(outcome);
            self.queue.push_back(t.thread);
            true
        } else {
            false
        }
    }

    /// Is any coroutine parked on an in-flight offload job?
    pub fn has_io_blocked(&self) -> bool {
        !self.io_blocked.is_empty()
    }

    /// No coroutine other than the running one is ready, `join`-blocked
    /// or IO-blocked — so a blocking call can run inline on the actor
    /// thread without stalling anyone.
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty()
            && self.blocked.is_empty()
            && self.io_blocked.is_empty()
    }

    /// Could the actor make progress if the running coroutine parked
    /// itself? True when another coroutine is ready, or an offload job
    /// is outstanding (its completion will make one ready).
    pub fn can_make_progress(&self) -> bool {
        !self.queue.is_empty() || !self.io_blocked.is_empty()
    }

    /// Record which coroutine is now running. Called on every switch.
    pub fn set_current(&mut self, id: u32, is_main: bool) {
        self.current_id = id;
        self.current_is_main = is_main;
    }

    /// Every parked coroutine — ready, `join`-blocked *and*
    /// IO-blocked — for GC root tracing.
    pub fn queued(&self) -> impl Iterator<Item = &GreenThread> {
        self.queue
            .iter()
            .chain(self.blocked.iter().map(|bt| &bt.thread))
            .chain(self.io_blocked.iter().map(|t| &t.thread))
    }

    /// Borrow a parked coroutine's value stack by id — used to resolve
    /// an open upvalue that a `go` block captured from another
    /// coroutine. Searches both the run-queue and the `join`-blocked
    /// set. Returns `None` if no parked coroutine has that id.
    pub fn stack_of(&self, id: u32) -> Option<&Vec<Value>> {
        self.queue
            .iter()
            .chain(self.blocked.iter().map(|bt| &bt.thread))
            .chain(self.io_blocked.iter().map(|t| &t.thread))
            .find(|gt| gt.id == id)
            .map(|gt| &gt.stack)
    }

    /// Mutable counterpart of [`stack_of`].
    pub fn stack_of_mut(&mut self, id: u32) -> Option<&mut Vec<Value>> {
        self.queue
            .iter_mut()
            .chain(self.blocked.iter_mut().map(|bt| &mut bt.thread))
            .chain(self.io_blocked.iter_mut().map(|t| &mut t.thread))
            .find(|gt| gt.id == id)
            .map(|gt| &mut gt.stack)
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
