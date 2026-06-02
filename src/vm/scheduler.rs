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
#[derive(Clone)]
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
    /// `None` while the coroutine is still running. Once it finishes,
    /// `Some(ResumeOutcome::Value(v))` holds the value its body
    /// evaluated to, or `Some(ResumeOutcome::Raise(e))` records an
    /// uncaught error so a later `join` re-raises it. A coroutine
    /// cancelled while parked finishes with `Value(${cancelled: true})`.
    pub(crate) result: Option<ResumeOutcome>,
    /// Set by `cancel(handle)`. Consulted at every resume-from-park
    /// (`Vm::load_green`): a set flag turns the resume into a catchable
    /// `cancelled` raise at the park call site instead of delivering the
    /// park's value, and is cleared as it fires (edge-triggered, so a
    /// `catch` may clean up and even park again without re-cancelling).
    /// Setting it on an already-finished coroutine (`result.is_some()`)
    /// is a harmless no-op.
    pub(crate) cancel_requested: bool,
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

/// A coroutine parked in a cooperative `wait` / `wait_frame`, waiting
/// for the host clock to reach `wake_time`. Unlike `io_blocked` and
/// `blocked`, nothing inside the actor can wake it — only the host
/// advancing time via [`Scheduler::wake_timers`] does, which is why
/// timer parks must stay out of [`Scheduler::can_make_progress`].
struct TimerBlockedThread {
    /// Host-clock time at which the coroutine becomes ready again. A
    /// `wait(secs)` sets `frame_now + secs`; a `wait_frame()` uses
    /// `f64::NEG_INFINITY` so it is due on the very next tick.
    wake_time: f64,
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
    /// Coroutines parked in a cooperative `wait` / `wait_frame` — not
    /// ready until the host clock reaches each one's `wake_time`. Kept
    /// off `queue`, and deliberately excluded from `can_make_progress`
    /// (only the host wakes these, never another coroutine).
    timer_blocked: Vec<TimerBlockedThread>,
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
            timer_blocked: Vec::new(),
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
        self.timer_blocked.clear();
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

    /// Is any coroutine sitting on the ready run-queue right now? Lets a
    /// host frame drain skip the park/restore of the main coroutine when
    /// nothing woke this tick.
    pub fn has_ready(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Park `thread` until green thread `awaiting` finishes. Used by a
    /// cooperative `join`: the joiner leaves the run-queue entirely and
    /// is re-enqueued by [`wake_joiners`] when its target returns.
    pub fn block(&mut self, awaiting: u32, thread: GreenThread) {
        self.blocked.push(BlockedThread { awaiting, thread });
    }

    /// Green thread `finished` ended with `outcome`: move every
    /// coroutine that was `join`-blocked on it back onto the run-queue,
    /// delivering `outcome` — a `Value` its `join` expression yields,
    /// or a `Raise` that re-surfaces the green thread's uncaught error
    /// at the join site.
    pub fn wake_joiners(&mut self, finished: u32, outcome: &ResumeOutcome) {
        let mut i = 0;
        while i < self.blocked.len() {
            if self.blocked[i].awaiting == finished {
                let mut bt = self.blocked.swap_remove(i);
                bt.thread.parked_resume = Some(outcome.clone());
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

    /// Park `thread` in a cooperative `wait` until the host clock reaches
    /// `wake_time` (a `wait_frame()` passes `f64::NEG_INFINITY`, so it is
    /// due on the next tick). Re-enqueued by [`wake_timers`].
    pub fn park_timer(&mut self, wake_time: f64, thread: GreenThread) {
        self.timer_blocked.push(TimerBlockedThread { wake_time, thread });
    }

    /// Move every timer-parked coroutine whose `wake_time <= now` back
    /// onto the run-queue, delivering `Value::Null` (a `wait` expression
    /// evaluates to null on resume). Returns `true` if any woke. Called
    /// once per host tick at the top of [`crate::vm::vm::Vm::drain_ready`].
    pub fn wake_timers(&mut self, now: f64) -> bool {
        let mut woke = false;
        let mut i = 0;
        while i < self.timer_blocked.len() {
            if self.timer_blocked[i].wake_time <= now {
                let mut t = self.timer_blocked.swap_remove(i);
                t.thread.parked_resume =
                    Some(ResumeOutcome::Value(Value::Null));
                self.queue.push_back(t.thread);
                woke = true;
            } else {
                i += 1;
            }
        }
        woke
    }

    /// Is any coroutine parked in a cooperative `wait`?
    pub fn has_timer_blocked(&self) -> bool {
        !self.timer_blocked.is_empty()
    }

    /// The earliest `wake_time` among timer-parked coroutines, if any.
    /// The standalone driver sleeps the actor thread to this point.
    pub fn next_timer_wake(&self) -> Option<f64> {
        self.timer_blocked
            .iter()
            .map(|t| t.wake_time)
            .min_by(|a, b| {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// No coroutine other than the running one is ready, `join`-blocked,
    /// IO-blocked or timer-blocked — so a blocking call can run inline on
    /// the actor thread without stalling anyone. A timer-parked sibling
    /// counts: running inline would delay its wake, so the call should
    /// offload instead.
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty()
            && self.blocked.is_empty()
            && self.io_blocked.is_empty()
            && self.timer_blocked.is_empty()
    }

    /// Could the actor make progress if the running coroutine parked
    /// itself? True when another coroutine is ready, an offload job is
    /// outstanding (its completion will make one ready), or a `wait`
    /// timer is pending (it wakes once its time comes — the host advances
    /// the clock, or the standalone driver sleeps to it).
    pub fn can_make_progress(&self) -> bool {
        !self.queue.is_empty()
            || !self.io_blocked.is_empty()
            || !self.timer_blocked.is_empty()
    }

    /// A `cancel(handle)` marked coroutine `id` for cancellation: if it
    /// is parked in a `join`, an offloaded blocking call, or a `wait`
    /// timer, abandon that wait and move it onto the run-queue so it
    /// resumes promptly — where [`crate::vm::vm::Vm::load_green`] turns
    /// the resume into the `cancelled` raise. Without this, cancelling a
    /// coroutine asleep in `wait(10)` would not take effect until the ten
    /// seconds elapsed. The delivered `Null` is nominal: `load_green`
    /// discards it for the raise. A coroutine that is already on the
    /// run-queue (yield-parked or not yet started), or is the running one
    /// (a self-cancel), is left as-is — the flag alone handles it at its
    /// next resume. Returns `true` if it unparked one. An abandoned IO
    /// job may still complete later; [`wake_io`] then finds no waiter and
    /// is a no-op, so nothing deadlocks.
    pub fn cancel_unpark(&mut self, id: u32) -> bool {
        let ready = Some(ResumeOutcome::Value(Value::Null));
        if let Some(pos) =
            self.blocked.iter().position(|t| t.thread.id == id)
        {
            let mut bt = self.blocked.swap_remove(pos);
            bt.thread.parked_resume = ready;
            self.queue.push_back(bt.thread);
            return true;
        }
        if let Some(pos) =
            self.io_blocked.iter().position(|t| t.thread.id == id)
        {
            let mut t = self.io_blocked.swap_remove(pos);
            t.thread.parked_resume = ready;
            self.queue.push_back(t.thread);
            return true;
        }
        if let Some(pos) =
            self.timer_blocked.iter().position(|t| t.thread.id == id)
        {
            let mut t = self.timer_blocked.swap_remove(pos);
            t.thread.parked_resume = ready;
            self.queue.push_back(t.thread);
            return true;
        }
        false
    }

    /// Record which coroutine is now running. Called on every switch.
    pub fn set_current(&mut self, id: u32, is_main: bool) {
        self.current_id = id;
        self.current_is_main = is_main;
    }

    /// Every parked coroutine — ready, `join`-blocked, IO-blocked *and*
    /// timer-blocked — for GC root tracing.
    pub fn queued(&self) -> impl Iterator<Item = &GreenThread> {
        self.queue
            .iter()
            .chain(self.blocked.iter().map(|bt| &bt.thread))
            .chain(self.io_blocked.iter().map(|t| &t.thread))
            .chain(self.timer_blocked.iter().map(|t| &t.thread))
    }

    /// Borrow a parked coroutine's value stack by id — used to resolve
    /// an open upvalue that a `go` block captured from another
    /// coroutine. Searches the run-queue and every parked set
    /// (`join`-blocked, IO-blocked, timer-blocked). Returns `None` if no
    /// parked coroutine has that id.
    pub fn stack_of(&self, id: u32) -> Option<&Vec<Value>> {
        self.queue
            .iter()
            .chain(self.blocked.iter().map(|bt| &bt.thread))
            .chain(self.io_blocked.iter().map(|t| &t.thread))
            .chain(self.timer_blocked.iter().map(|t| &t.thread))
            .find(|gt| gt.id == id)
            .map(|gt| &gt.stack)
    }

    /// Mutable counterpart of [`stack_of`].
    pub fn stack_of_mut(&mut self, id: u32) -> Option<&mut Vec<Value>> {
        self.queue
            .iter_mut()
            .chain(self.blocked.iter_mut().map(|bt| &mut bt.thread))
            .chain(self.io_blocked.iter_mut().map(|t| &mut t.thread))
            .chain(self.timer_blocked.iter_mut().map(|t| &mut t.thread))
            .find(|gt| gt.id == id)
            .map(|gt| &mut gt.stack)
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
