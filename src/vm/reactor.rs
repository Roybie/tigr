//! Async-IO reactor for green-thread socket I/O (Phase B).
//!
//! The blocking-IO offload ([`crate::vm::offload`]) moved every parked
//! socket call onto a worker thread. That removed the "a blocking call
//! freezes its sibling coroutines" seam, but it does not *scale*: a
//! server with ten thousand idle keep-alive connections, each with a
//! coroutine parked in `Net.read`, would need ten thousand worker
//! threads. Idle waiting on a socket should cost a table entry, not an
//! OS thread.
//!
//! This module is that fix — one process-wide **reactor thread** running
//! `epoll` / `kqueue` / IOCP-AFD (via the cross-platform [`polling`]
//! crate) that waits on thousands of socket handles at once. It is a
//! second offload *backend*: like the worker pool it ends each op by
//! posting `(job_id, OffloadResult)` to the owning actor's
//! [`CompletionMailbox`], so the scheduler / VM side
//! ([`crate::vm::scheduler`]'s `io_blocked`, `decode`, `park_io` /
//! `wake_io`) is reused unchanged.
//!
//! ## Readiness, not completion
//!
//! `polling` is a *readiness* interface. To keep the actor side
//! identical to the worker pool, the reactor thread itself performs the
//! non-blocking syscall once a handle is ready and posts the *finished*
//! result — not a bare "handle is ready" hint. It therefore owns each
//! op's small state machine (see [`advance`]): a multi-step op like
//! `read_exact` resumes across several readiness events, accumulating
//! into its [`SocketOp`] buffer. Like a worker thread it only ever
//! touches `Send` POD — never the GC heap.
//!
//! ## Oneshot re-arm
//!
//! `polling` delivers one event per arm (the only mode portable to
//! Windows AFD), so an op that is still `Pending` after a readiness
//! event is re-armed with [`Poller::modify`]; a completed op is removed
//! with [`Poller::delete`]. This is the one semantic difference from a
//! level-triggered `epoll`: a missed re-arm hangs the op, so the
//! re-arm path is exercised by the reactor test suite on every OS.
//!
//! ## Two executors
//!
//! A [`SocketOp`] runs two ways. [`run_blocking`] is the inline
//! executor: a thin wrapper over the blocking `socket.rs` methods, used
//! when the actor is idle (so blocking its thread stalls nobody) and
//! for a re-entrant native call. [`advance`] is the reactor executor:
//! the non-blocking state machine driven by readiness events. Both
//! cover every socket kind, TLS included — a TLS op is just a plain op
//! whose `nb_read` / `nb_write` hand-drive `rustls` (see `socket.rs`).

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

#[cfg(unix)]
use std::os::fd::BorrowedFd;
#[cfg(windows)]
use std::os::windows::io::BorrowedSocket;

use polling::{Event, Events, Poller};

use crate::vm::native_modules::net;
use crate::vm::offload::{CompletionMailbox, OffloadOk, OffloadResult};
use crate::vm::socket::{
    NetError, RawHandle, ReactorOp, SocketHandle, SocketInner, SocketOp, CHUNK,
    MAX_DIRECT_READ,
};

/// Borrow a stored raw handle as a `polling` source for `add` /
/// `modify` / `delete`. The borrow is used transiently and never
/// outlives the [`SocketHandle`] that keeps the underlying socket open.
///
/// # Safety
/// The caller must hold the [`SocketHandle`] (and thus keep the handle
/// open) for the duration of the returned borrow — every call site does,
/// since the op's `PendingOp` owns the socket.
#[cfg(unix)]
unsafe fn borrow_source(handle: RawHandle) -> BorrowedFd<'static> {
    BorrowedFd::borrow_raw(handle)
}
#[cfg(windows)]
unsafe fn borrow_source(handle: RawHandle) -> BorrowedSocket<'static> {
    BorrowedSocket::borrow_raw(handle)
}

// ---------------------------------------------------------------------
// Inline executor — the blocking path
// ---------------------------------------------------------------------

/// Run a [`ReactorOp`] synchronously on the calling thread, blocking
/// until it completes. Used on the inline fast path (the actor is idle)
/// and for a re-entrant native call. A thin wrapper over the blocking
/// `socket.rs` methods.
pub fn run_blocking(rop: ReactorOp) -> OffloadResult {
    let ReactorOp { socket, op, label } = rop;
    // A prior reactor op may have left the handle non-blocking; the
    // blocking methods below need it blocking again.
    if let Err(e) = socket.set_nonblocking_mode(false) {
        return Err(net::offload_err(label, NetError::Io(e)));
    }
    match op {
        SocketOp::ReadChunk(n) => match socket.read_chunk(n) {
            Ok(data) => Ok(OffloadOk::Bytes(data)),
            Err(e) => Err(net::offload_err(label, e)),
        },
        SocketOp::ReadExact { need, mut got } => {
            while got.len() < need {
                match socket.read_chunk(need - got.len()) {
                    Ok(c) if c.is_empty() => {
                        return Err(net::offload_net_err(
                            "eof",
                            format!(
                                "Net.read_exact: stream ended after {} \
                                 of {need} bytes",
                                got.len()
                            ),
                        ));
                    }
                    Ok(c) => got.extend(c),
                    Err(e) => return Err(net::offload_err(label, e)),
                }
            }
            Ok(OffloadOk::Bytes(got))
        }
        SocketOp::ReadLine => match socket.read_until(b'\n') {
            Ok(opt) => net::finish_line(opt),
            Err(e) => Err(net::offload_err(label, e)),
        },
        SocketOp::ReadUntil(delim) => match socket.read_until(delim) {
            Ok(opt) => Ok(OffloadOk::BytesOrNull(opt)),
            Err(e) => Err(net::offload_err(label, e)),
        },
        SocketOp::ReadAll(mut got) => {
            loop {
                match socket.read_chunk(MAX_DIRECT_READ) {
                    Ok(c) if c.is_empty() => break,
                    Ok(c) => got.extend(c),
                    Err(e) => return Err(net::offload_err(label, e)),
                }
            }
            Ok(OffloadOk::Bytes(got))
        }
        SocketOp::WriteAll { data, .. } => match socket.write_all(&data) {
            Ok(()) => Ok(OffloadOk::Int(data.len() as i64)),
            Err(e) => Err(net::offload_err(label, e)),
        },
        SocketOp::Accept => match socket.accept() {
            Ok(conn) => Ok(OffloadOk::Socket(conn)),
            Err(e) => Err(net::offload_err(label, e)),
        },
        SocketOp::RecvFrom(n) => match socket.recv_from(n) {
            Ok((data, addr)) => Ok(OffloadOk::RecvFrom {
                data,
                host: addr.ip().to_string(),
                port: addr.port(),
            }),
            Err(e) => Err(net::offload_err(label, e)),
        },
    }
}

// ---------------------------------------------------------------------
// Reactor executor — the non-blocking state machine
// ---------------------------------------------------------------------

/// Whether one byte of progress is `WouldBlock` — the handle is not ready.
fn would_block(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock
}

/// The result of one [`advance`] pass over an op.
enum Advance {
    /// The op finished — post this result, deregister the handle.
    Done(OffloadResult),
    /// The handle would block; stay registered and wait for the next event.
    Pending,
}

/// Drive `op` against `socket` as far as a non-blocking syscall allows.
/// Called once at submit time (data may already be buffered) and again
/// on every readiness event for the op's handle.
fn advance(op: &mut SocketOp, socket: &SocketInner, label: &'static str) -> Advance {
    // A concurrent `close` beats any pending readiness — without this a
    // `shutdown`-induced readable event would resolve as a clean EOF
    // rather than the catchable `closed` the spec promises.
    if socket.is_closed() {
        return Advance::Done(Err(net::offload_err(label, NetError::Closed)));
    }
    match op {
        SocketOp::Accept => match socket.nb_accept() {
            Ok(conn) => Advance::Done(Ok(OffloadOk::Socket(conn))),
            Err(NetError::Io(e)) if would_block(&e) => Advance::Pending,
            Err(e) => Advance::Done(Err(net::offload_err(label, e))),
        },
        SocketOp::RecvFrom(n) => match socket.recv_from(*n) {
            Ok((data, addr)) => Advance::Done(Ok(OffloadOk::RecvFrom {
                data,
                host: addr.ip().to_string(),
                port: addr.port(),
            })),
            Err(NetError::Io(e)) if would_block(&e) => Advance::Pending,
            Err(e) => Advance::Done(Err(net::offload_err(label, e))),
        },
        SocketOp::ReadChunk(n) => {
            let n = *n;
            if n == 0 {
                return Advance::Done(Ok(OffloadOk::Bytes(Vec::new())));
            }
            let buffered = socket.take_buffered(n);
            if !buffered.is_empty() {
                return Advance::Done(Ok(OffloadOk::Bytes(buffered)));
            }
            let mut tmp = vec![0u8; n.min(MAX_DIRECT_READ)];
            match socket.nb_read(&mut tmp) {
                Ok(got) => {
                    tmp.truncate(got);
                    Advance::Done(Ok(OffloadOk::Bytes(tmp)))
                }
                Err(e) if would_block(&e) => Advance::Pending,
                Err(e) => {
                    Advance::Done(Err(net::offload_err(label, NetError::Io(e))))
                }
            }
        }
        SocketOp::ReadExact { need, got } => loop {
            if got.len() >= *need {
                return Advance::Done(Ok(OffloadOk::Bytes(std::mem::take(got))));
            }
            let want = *need - got.len();
            let buffered = socket.take_buffered(want);
            if !buffered.is_empty() {
                got.extend(buffered);
                continue;
            }
            let mut tmp = vec![0u8; want.min(CHUNK)];
            match socket.nb_read(&mut tmp) {
                Ok(0) => {
                    return Advance::Done(Err(net::offload_net_err(
                        "eof",
                        format!(
                            "Net.read_exact: stream ended after {} of {} \
                             bytes",
                            got.len(),
                            need
                        ),
                    )));
                }
                Ok(g) => got.extend_from_slice(&tmp[..g]),
                Err(e) if would_block(&e) => return Advance::Pending,
                Err(e) => {
                    return Advance::Done(Err(net::offload_err(
                        label,
                        NetError::Io(e),
                    )));
                }
            }
        },
        SocketOp::ReadUntil(delim) => match read_until_step(socket, *delim, label) {
            Step::Done(opt) => Advance::Done(Ok(OffloadOk::BytesOrNull(opt))),
            Step::Pending => Advance::Pending,
            Step::Err(result) => Advance::Done(Err(result)),
        },
        SocketOp::ReadLine => match read_until_step(socket, b'\n', label) {
            Step::Done(opt) => Advance::Done(net::finish_line(opt)),
            Step::Pending => Advance::Pending,
            Step::Err(result) => Advance::Done(Err(result)),
        },
        SocketOp::ReadAll(got) => loop {
            got.extend(socket.take_buffered_all());
            let mut tmp = vec![0u8; CHUNK];
            match socket.nb_read(&mut tmp) {
                Ok(0) => {
                    return Advance::Done(Ok(OffloadOk::Bytes(std::mem::take(
                        got,
                    ))));
                }
                Ok(g) => got.extend_from_slice(&tmp[..g]),
                Err(e) if would_block(&e) => return Advance::Pending,
                Err(e) => {
                    return Advance::Done(Err(net::offload_err(
                        label,
                        NetError::Io(e),
                    )));
                }
            }
        },
        SocketOp::WriteAll { data, sent } => loop {
            if *sent >= data.len() {
                // Every byte handed off — but a TLS socket may still
                // hold encrypted records that have not reached the
                // wire. Drain them before the op completes.
                return match socket.nb_flush() {
                    Ok(()) => {
                        Advance::Done(Ok(OffloadOk::Int(data.len() as i64)))
                    }
                    Err(e) if would_block(&e) => Advance::Pending,
                    Err(e) => Advance::Done(Err(net::offload_err(
                        label,
                        NetError::Io(e),
                    ))),
                };
            }
            match socket.nb_write(&data[*sent..]) {
                Ok(0) => {
                    return Advance::Done(Err(net::offload_err(
                        label,
                        NetError::Io(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "write returned 0",
                        )),
                    )));
                }
                Ok(g) => *sent += g,
                Err(e) if would_block(&e) => return Advance::Pending,
                Err(e) => {
                    return Advance::Done(Err(net::offload_err(
                        label,
                        NetError::Io(e),
                    )));
                }
            }
        },
    }
}

/// One non-blocking step of a delimiter scan, shared by `ReadUntil` and
/// `ReadLine`. The over-read surplus lives in the socket's own read
/// buffer, so a later op sees it.
enum Step {
    /// Found the delimiter, or hit end-of-stream — `None` is a clean
    /// EOF with nothing buffered.
    Done(Option<Vec<u8>>),
    Pending,
    Err(crate::vm::offload::OffloadErr),
}

fn read_until_step(socket: &SocketInner, delim: u8, label: &'static str) -> Step {
    loop {
        if let Some(line) = socket.take_buffered_until(delim) {
            return Step::Done(Some(line));
        }
        let mut tmp = vec![0u8; CHUNK];
        match socket.nb_read(&mut tmp) {
            Ok(0) => {
                let rest = socket.take_buffered_all();
                return Step::Done(if rest.is_empty() {
                    None
                } else {
                    Some(rest)
                });
            }
            Ok(g) => socket.push_buffered(&tmp[..g]),
            Err(e) if would_block(&e) => return Step::Pending,
            Err(e) => {
                return Step::Err(net::offload_err(label, NetError::Io(e)));
            }
        }
    }
}

// ---------------------------------------------------------------------
// The reactor thread
// ---------------------------------------------------------------------

/// A message from an actor thread to the reactor thread, delivered over
/// the registration channel and announced with [`Poller::notify`].
enum Msg {
    /// Register a new op and start driving it.
    Submit {
        job_id: u64,
        mailbox: Arc<CompletionMailbox>,
        rop: ReactorOp,
    },
    /// `Net.close(sock)` — fail every op on that socket with `closed`.
    Cancel { socket_id: u64 },
}

/// An op the reactor is currently driving, keyed by its registration
/// key in the op table. Holds only `Send` POD — never a GC root.
struct PendingOp {
    socket: SocketHandle,
    op: SocketOp,
    job_id: u64,
    mailbox: Arc<CompletionMailbox>,
    label: &'static str,
    /// The registered handle, kept so the op can be re-armed / removed.
    handle: RawHandle,
    /// The interests this op waits on, re-applied on each oneshot re-arm.
    readable: bool,
    writable: bool,
}

/// The process-wide reactor handle. Actors talk to the reactor thread
/// through `tx` (the registration channel) and `poller.notify()` (which
/// pulls the thread out of `wait`).
struct Reactor {
    tx: Mutex<Sender<Msg>>,
    poller: Arc<Poller>,
}

static REACTOR: OnceLock<Reactor> = OnceLock::new();

/// The reactor handle, starting the reactor thread on first use.
fn reactor() -> &'static Reactor {
    REACTOR.get_or_init(|| {
        let poller = Arc::new(Poller::new().expect("reactor: create poller"));
        let (tx, rx) = mpsc::channel();
        let loop_poller = Arc::clone(&poller);
        thread::Builder::new()
            .name("tigr-reactor".into())
            .spawn(move || reactor_loop(loop_poller, rx))
            .expect("reactor: spawn thread");
        Reactor { tx: Mutex::new(tx), poller }
    })
}

/// Hand a socket op to the reactor. The completion is posted to
/// `mailbox` tagged with `job_id`; the caller parks the running
/// coroutine under the same id (exactly as for a worker-pool offload).
pub fn submit(job_id: u64, mailbox: Arc<CompletionMailbox>, rop: ReactorOp) {
    let r = reactor();
    r.tx
        .lock()
        .unwrap()
        .send(Msg::Submit { job_id, mailbox, rop })
        .expect("reactor thread alive");
    r.poller.notify().expect("reactor: notify");
}

/// Cancel every reactor op on socket `socket_id` — drives `Net.close`.
/// A no-op if the reactor has never been started (no socket op was
/// ever offloaded).
pub fn cancel(socket_id: u64) {
    if let Some(r) = REACTOR.get() {
        if r.tx.lock().unwrap().send(Msg::Cancel { socket_id }).is_ok() {
            let _ = r.poller.notify();
        }
    }
}

/// The reactor thread's event loop. Owns the [`Poller`], the op table
/// and the key counter; never returns.
fn reactor_loop(poller: Arc<Poller>, rx: Receiver<Msg>) {
    let mut events = Events::new();
    let mut ops: HashMap<usize, PendingOp> = HashMap::new();
    // Op keys start at 1; `notify` reports `usize::MAX`, never a key.
    let mut next_key: usize = 1;
    loop {
        events.clear();
        if let Err(e) = poller.wait(&mut events, None) {
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            panic!("reactor: wait failed: {e}");
        }
        // A `notify` (new submit / cancel) or any op event wakes us.
        // Drain the registration channel first, so a `Cancel` is
        // observed before a stale readiness event for the same socket.
        drain_messages(&rx, &poller, &mut ops, &mut next_key);
        for event in events.iter() {
            advance_token(event.key, &poller, &mut ops);
        }
    }
}

/// Drain every queued registration / cancellation message.
fn drain_messages(
    rx: &Receiver<Msg>,
    poller: &Poller,
    ops: &mut HashMap<usize, PendingOp>,
    next_key: &mut usize,
) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Msg::Submit { job_id, mailbox, rop } => {
                start_op(job_id, mailbox, rop, poller, ops, next_key);
            }
            Msg::Cancel { socket_id } => {
                cancel_socket(socket_id, poller, ops);
            }
        }
    }
}

/// Register a freshly-submitted op and drive it once — data may already
/// be buffered or the handle already ready, in which case it completes
/// here and is never registered.
fn start_op(
    job_id: u64,
    mailbox: Arc<CompletionMailbox>,
    rop: ReactorOp,
    poller: &Poller,
    ops: &mut HashMap<usize, PendingOp>,
    next_key: &mut usize,
) {
    let ReactorOp { socket, mut op, label } = rop;
    // Read ops watch the read half, writes the write half — distinct
    // `dup`'d handles, so a concurrent read + write register
    // independently. A TLS connection has one handle and oscillates
    // between read and write readiness as rustls drives its records, so
    // it is registered for both interests and re-driven on either.
    let writes = matches!(op, SocketOp::WriteAll { .. });
    let handle = if writes {
        socket.write_raw_handle()
    } else {
        socket.read_raw_handle()
    };
    let (readable, writable) = if socket.is_tls() {
        (true, true)
    } else if writes {
        (false, true)
    } else {
        (true, false)
    };
    let Some(handle) = handle else {
        // Not reactor-eligible — `dispatch_socket` routes such sockets
        // to the worker pool, so this should be unreachable. Be safe.
        mailbox.post(
            job_id,
            Err(net::offload_err(
                label,
                NetError::WrongKind("reactor: not a connected stream".into()),
            )),
        );
        return;
    };
    if let Err(e) = socket.set_nonblocking_mode(true) {
        mailbox.post(job_id, Err(net::offload_err(label, NetError::Io(e))));
        return;
    }
    // Try once: buffered data has no readiness to wait on, and the
    // handle may already be ready.
    if let Advance::Done(result) = advance(&mut op, &socket, label) {
        mailbox.post(job_id, result);
        return;
    }
    let key = *next_key;
    *next_key = next_key.wrapping_add(1).max(1);
    // SAFETY: borrowing the raw handle is sound because the `PendingOp`
    // inserted below owns `socket`, keeping the handle open until the op
    // is `delete`d (on completion, cancellation, or a re-arm failure).
    let added = unsafe {
        let source = borrow_source(handle);
        poller.add(&source, Event::new(key, readable, writable))
    };
    if let Err(e) = added {
        mailbox.post(job_id, Err(net::offload_err(label, NetError::Io(e))));
        return;
    }
    ops.insert(
        key,
        PendingOp {
            socket,
            op,
            job_id,
            mailbox,
            label,
            handle,
            readable,
            writable,
        },
    );
}

/// A readiness event fired for `key` — drive that op forward, then
/// either remove it (done) or re-arm it (oneshot, still pending).
fn advance_token(key: usize, poller: &Poller, ops: &mut HashMap<usize, PendingOp>) {
    // Driving the op borrows the table mutably; the re-arm / removal
    // that follows borrows it again, so resolve the outcome first.
    enum Outcome {
        Done(OffloadResult),
        Rearm,
        Gone,
    }
    let outcome = match ops.get_mut(&key) {
        Some(pending) => {
            match advance(&mut pending.op, &pending.socket, pending.label) {
                Advance::Done(result) => Outcome::Done(result),
                Advance::Pending => Outcome::Rearm,
            }
        }
        // The op already completed or was cancelled — a stale event.
        None => Outcome::Gone,
    };
    match outcome {
        Outcome::Gone => {}
        Outcome::Done(result) => {
            let pending = ops.remove(&key).unwrap();
            // SAFETY: `pending` still owns the socket here.
            let source = unsafe { borrow_source(pending.handle) };
            let _ = poller.delete(&source);
            pending.mailbox.post(pending.job_id, result);
        }
        Outcome::Rearm => {
            // `polling` is oneshot — re-arm for the next event.
            let (handle, readable, writable) = {
                let p = ops.get(&key).unwrap();
                (p.handle, p.readable, p.writable)
            };
            // SAFETY: the op still owns the socket (still in `ops`).
            let source = unsafe { borrow_source(handle) };
            let event = Event::new(key, readable, writable);
            if let Err(e) = poller.modify(&source, event) {
                let _ = poller.delete(&source);
                let pending = ops.remove(&key).unwrap();
                pending.mailbox.post(
                    pending.job_id,
                    Err(net::offload_err(pending.label, NetError::Io(e))),
                );
            }
        }
    }
}

/// Fail every op on socket `socket_id` with `closed`, deregistering
/// each handle. The socket's `closed` flag is already set (by `close`),
/// so the woken coroutine sees the same `closed` an inline op would
/// raise.
fn cancel_socket(socket_id: u64, poller: &Poller, ops: &mut HashMap<usize, PendingOp>) {
    let keys: Vec<usize> = ops
        .iter()
        .filter(|(_, p)| p.socket.id() == socket_id)
        .map(|(k, _)| *k)
        .collect();
    for key in keys {
        let pending = ops.remove(&key).unwrap();
        // SAFETY: `pending` still owns the socket here.
        let source = unsafe { borrow_source(pending.handle) };
        let _ = poller.delete(&source);
        pending.mailbox.post(
            pending.job_id,
            Err(net::offload_err(pending.label, NetError::Closed)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything the reactor thread holds crosses a thread boundary —
    /// the registration message and the op-table entry must be `Send`.
    #[test]
    fn reactor_state_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Msg>();
        assert_send::<PendingOp>();
    }
}
