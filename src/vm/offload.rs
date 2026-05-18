//! Blocking-IO thread-pool offload for green threads.
//!
//! A green thread doing a blocking call (a child process, file or
//! network IO) would otherwise freeze every other coroutine in the
//! actor — they all share one OS thread. This module moves the
//! blocking work onto a process-wide worker pool and lets the calling
//! coroutine cooperatively park (reusing the scheduler's park/wake
//! machinery) until the worker posts a completion.
//!
//! The split is strict: a worker thread only ever touches plain `Send`
//! data. The GC heap is thread-local per actor ([`crate::vm::gc`]), so
//! a worker must never see a `Value`/`GcRef`. A blocking native
//! extracts POD from its arguments *on the actor thread*, hands the
//! pool a `Send` closure ([`BlockingJob`]) returning POD
//! ([`OffloadResult`]), and [`decode`] rebuilds a `Value` back on the
//! actor thread once the result returns.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::socket::SocketHandle;
use crate::vm::value::Value;

/// The closure a `Blocking` native hands to the worker pool. Runs on a
/// pool thread; captures only `Send` POD — never a `Value`/`GcRef`.
pub type BlockingJob = Box<dyn FnOnce() -> OffloadResult + Send>;

/// What a worker produces — either a raw success payload or a POD form
/// of the error the native would have raised inline. Both sides are
/// `Send`; neither holds a heap reference.
pub type OffloadResult = Result<OffloadOk, OffloadErr>;

/// A `Send` thunk that reconstructs an offloaded call's result on the
/// actor thread. It captures only POD; running it may allocate into
/// the thread-local heap (which a worker thread must never touch). See
/// [`OffloadOk::Deferred`].
pub type DeferredDecode =
    Box<dyn FnOnce() -> Result<Value, RuntimeError> + Send>;

/// The raw, `Send` success payload of an offloaded blocking call.
/// [`decode`] turns each variant into a `Value` on the actor thread.
/// Grows one variant at a time as more natives are converted.
pub enum OffloadOk {
    /// A call with no meaningful result — decodes to `null`.
    Unit,
    /// An integer result (a byte count).
    Int(i64),
    /// A string result (file contents, working directory, ...).
    Str(String),
    /// A byte-buffer result.
    Bytes(Vec<u8>),
    /// An optional byte buffer — `None` decodes to `null` (end-of-stream).
    BytesOrNull(Option<Vec<u8>>),
    /// A list of strings (directory entries).
    StrList(Vec<String>),
    /// An optional string — `None` decodes to `null` (EOF / end-of-stream).
    StrOrNull(Option<String>),
    /// A socket result (`Net.accept` / `connect` / `connect_tls`).
    Socket(SocketHandle),
    /// `Net.recv_from` — one UDP datagram plus its sender's address.
    RecvFrom { data: Vec<u8>, host: String, port: u16 },
    /// `Os.run` — child-process exit code plus captured output.
    Run { code: i64, stdout: String, stderr: String },
    /// An escape hatch for blocking calls whose result needs
    /// module-specific reconstruction on the actor thread — channel
    /// receive, `join`, `select`, where rebuilding the value (or the
    /// error) means decoding a `Transfer` into the heap. The worker
    /// captures only POD into the thunk; the actor thread runs it.
    Deferred(DeferredDecode),
}

/// The `Send` error payload — a POD form of the structured error a
/// blocking native raises today. `kind` is `Some` for a structured
/// `${kind, message}` error (the `Net` module) and `None` for a plain
/// string-valued `raise` (`IO` / `Os`).
pub struct OffloadErr {
    pub kind: Option<String>,
    pub message: String,
}

impl OffloadOk {
    /// Wrap an actor-thread reconstruction thunk as a finished
    /// [`OffloadResult`]. Convenience for blocking natives whose result
    /// needs the heap to rebuild (channel receive, `join`, `select`) —
    /// see [`OffloadOk::Deferred`].
    pub fn deferred(
        thunk: impl FnOnce() -> Result<Value, RuntimeError> + Send + 'static,
    ) -> OffloadResult {
        Ok(OffloadOk::Deferred(Box::new(thunk)))
    }
}

/// Turn a worker's raw result back into a `Value` (allocating into this
/// actor's thread-local heap) or the `RuntimeError` the native would
/// have raised. Runs only on the actor thread.
pub fn decode(result: OffloadResult) -> Result<Value, RuntimeError> {
    match result {
        // A `Deferred` payload carries its own actor-thread thunk —
        // it decides value-vs-error itself, so run it directly.
        Ok(OffloadOk::Deferred(thunk)) => thunk(),
        Ok(ok) => Ok(decode_ok(ok)),
        Err(e) => Err(decode_err(e)),
    }
}

fn decode_ok(ok: OffloadOk) -> Value {
    match ok {
        // Handled by `decode` before reaching here.
        OffloadOk::Deferred(_) => {
            unreachable!("decode handles OffloadOk::Deferred directly")
        }
        OffloadOk::Unit => Value::Null,
        OffloadOk::Int(n) => Value::Int(n),
        OffloadOk::Str(s) => Value::Str(s.into()),
        OffloadOk::Bytes(b) => Value::Bytes(gc::alloc_bytes(b)),
        OffloadOk::BytesOrNull(o) => match o {
            Some(b) => Value::Bytes(gc::alloc_bytes(b)),
            None => Value::Null,
        },
        OffloadOk::StrList(items) => {
            let arr: Vec<Value> =
                items.into_iter().map(|s| Value::Str(s.into())).collect();
            Value::Array(gc::alloc_array(arr))
        }
        OffloadOk::StrOrNull(o) => match o {
            Some(s) => Value::Str(s.into()),
            None => Value::Null,
        },
        OffloadOk::Socket(h) => Value::Socket(h),
        OffloadOk::RecvFrom { data, host, port } => {
            crate::vm::native_modules::object(&[
                ("data", Value::Bytes(gc::alloc_bytes(data))),
                ("host", Value::Str(host.into())),
                ("port", Value::Int(port as i64)),
            ])
        }
        OffloadOk::Run { code, stdout, stderr } => {
            crate::vm::native_modules::object(&[
                ("code", Value::Int(code)),
                ("stdout", Value::Str(stdout.into())),
                ("stderr", Value::Str(stderr.into())),
            ])
        }
    }
}

fn decode_err(e: OffloadErr) -> RuntimeError {
    let value = match e.kind {
        Some(kind) => crate::vm::native_modules::object(&[
            ("kind", Value::Str(kind.into())),
            ("message", Value::Str(e.message.into())),
        ]),
        None => Value::Str(e.message.into()),
    };
    RuntimeError::new(RuntimeErrorKind::Raised(value), 0)
}

/// Per-actor inbox for offload completions. A worker thread posts a
/// finished job here; the owning actor thread drains it at a
/// coroutine-switch point. Holds only POD — never a GC root, never
/// traced.
pub struct CompletionMailbox {
    done: Mutex<Vec<(u64, OffloadResult)>>,
    /// Signalled whenever a completion is posted, so an actor that has
    /// nothing else to run can sleep instead of spinning.
    wake: Condvar,
}

impl CompletionMailbox {
    pub fn new() -> Arc<Self> {
        Arc::new(CompletionMailbox {
            done: Mutex::new(Vec::new()),
            wake: Condvar::new(),
        })
    }

    /// Take every completion posted so far without blocking. Returns an
    /// empty vec if none are ready.
    pub fn drain(&self) -> Vec<(u64, OffloadResult)> {
        std::mem::take(&mut *self.done.lock().unwrap())
    }

    /// Block the calling (actor) thread until at least one completion
    /// is posted, then take every completion that is ready.
    pub fn wait_drain(&self) -> Vec<(u64, OffloadResult)> {
        let mut done = self.done.lock().unwrap();
        while done.is_empty() {
            done = self.wake.wait(done).unwrap();
        }
        std::mem::take(&mut *done)
    }

    /// Post a finished job. Called by a worker thread and by the
    /// async-IO reactor thread ([`crate::vm::reactor`]) — both are
    /// completion producers for the same actor mailbox.
    pub(crate) fn post(&self, id: u64, result: OffloadResult) {
        let mut done = self.done.lock().unwrap();
        done.push((id, result));
        self.wake.notify_one();
    }
}

/// One unit of blocking work plus the mailbox to post its completion
/// to. The `mailbox` `Arc` routes the result back to the right actor.
struct OffloadJob {
    id: u64,
    mailbox: Arc<CompletionMailbox>,
    work: BlockingJob,
}

/// Soft ceiling on live worker threads. The pool grows on demand and
/// never shrinks; the cap only guards against a runaway program
/// spawning unbounded blocking calls. A blocking-call dependency cycle
/// could in principle exhaust the pool, but at this size that is a
/// pathological program, not a realistic one.
const WORKER_SOFT_CAP: usize = 512;

/// Process-wide worker pool. Grows on demand: a submit reuses an idle
/// parked worker or spawns a new one; idle workers are kept, never
/// killed.
struct Pool {
    inner: Mutex<PoolInner>,
    /// Signalled when a job is pushed, waking a parked worker.
    work_ready: Condvar,
}

struct PoolInner {
    queue: VecDeque<OffloadJob>,
    /// Workers currently parked waiting for a job.
    idle: usize,
    /// Workers alive (idle or running).
    total: usize,
}

impl Pool {
    fn new() -> Pool {
        Pool {
            inner: Mutex::new(PoolInner {
                queue: VecDeque::new(),
                idle: 0,
                total: 0,
            }),
            work_ready: Condvar::new(),
        }
    }

    fn submit(&'static self, job: OffloadJob) {
        let mut inner = self.inner.lock().unwrap();
        inner.queue.push_back(job);
        // Spawn a fresh worker whenever there are more queued jobs than
        // parked workers to take them (and the soft cap leaves room).
        //
        // Comparing counts is race-free where testing `idle == 0` is
        // not: a worker decrements `idle` only after it wakes from
        // `wait` and reacquires this lock, so a burst of `submit`s that
        // outruns the workers waking would all see the same stale
        // `idle` and decline to spawn — leaving one worker to run the
        // whole burst serially. A notified-but-not-yet-woken worker is
        // still counted in `idle`, and the job it will take is still
        // counted in `queue.len()`, so the comparison stays balanced;
        // only a genuine surplus of jobs triggers a new worker.
        if inner.queue.len() > inner.idle && inner.total < WORKER_SOFT_CAP {
            inner.total += 1;
            thread::spawn(move || self.worker());
        }
        drop(inner);
        self.work_ready.notify_one();
    }

    fn worker(&'static self) {
        loop {
            let job = {
                let mut inner = self.inner.lock().unwrap();
                loop {
                    if let Some(j) = inner.queue.pop_front() {
                        break j;
                    }
                    inner.idle += 1;
                    inner = self.work_ready.wait(inner).unwrap();
                    inner.idle -= 1;
                }
            };
            let result = (job.work)();
            job.mailbox.post(job.id, result);
        }
    }
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(Pool::new)
}

/// Hand a blocking job to the worker pool. The completion is posted to
/// `mailbox` tagged with `id`; the caller parks the running coroutine
/// in the scheduler under the same `id` and resumes it when the
/// completion arrives.
pub fn submit(id: u64, mailbox: Arc<CompletionMailbox>, work: BlockingJob) {
    pool().submit(OffloadJob { id, mailbox, work });
}
