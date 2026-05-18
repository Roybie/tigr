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
//! This module is that fix — one process-wide **reactor thread**
//! running `epoll` / `kqueue` (via `mio`) that waits on thousands of
//! socket fds at once. It is a second offload *backend*: like the
//! worker pool it ends each op by posting `(job_id, OffloadResult)` to
//! the owning actor's [`CompletionMailbox`], so the scheduler / VM side
//! ([`crate::vm::scheduler`]'s `io_blocked`, `decode`, `park_io` /
//! `wake_io`) is reused unchanged.
//!
//! ## Readiness, not completion
//!
//! `epoll` is a *readiness* interface. To keep the actor side identical
//! to the worker pool, the reactor thread itself performs the
//! non-blocking syscall once an fd is ready and posts the *finished*
//! result — not a bare "fd is ready" hint. It therefore owns each op's
//! small state machine (see [`advance`]): a multi-step op like
//! `read_exact` resumes across several readiness events, accumulating
//! into its [`SocketOp`] buffer. Like a worker thread it only ever
//! touches `Send` POD — never the GC heap.
//!
//! ## Two executors
//!
//! A [`SocketOp`] runs two ways. [`run_blocking`] is the inline
//! executor: a thin wrapper over the blocking `socket.rs` methods, used
//! when the actor is idle (so blocking its thread stalls nobody) and as
//! the worker-pool fallback for socket kinds the reactor cannot drive
//! yet (TLS, in sub-phase B1). [`advance`] is the reactor executor: the
//! non-blocking state machine driven by readiness events.

use std::collections::HashMap;
use std::io;
use std::os::unix::io::RawFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token, Waker};

use crate::vm::native_modules::net;
use crate::vm::offload::{CompletionMailbox, OffloadOk, OffloadResult};
use crate::vm::socket::{
    NetError, ReactorOp, SocketHandle, SocketInner, SocketOp, CHUNK,
    MAX_DIRECT_READ,
};

/// Token reserved for the registration `Waker` — never an op token.
const WAKE: Token = Token(0);

// ---------------------------------------------------------------------
// Inline executor — the blocking path
// ---------------------------------------------------------------------

/// Run a [`ReactorOp`] synchronously on the calling thread, blocking
/// until it completes. Used on the inline fast path (the actor is idle)
/// and as the worker-pool fallback for non-reactor sockets. A thin
/// wrapper over the blocking `socket.rs` methods.
pub fn run_blocking(rop: ReactorOp) -> OffloadResult {
    let ReactorOp { socket, op, label } = rop;
    // A prior reactor op may have left the fd non-blocking; the
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
    }
}

// ---------------------------------------------------------------------
// Reactor executor — the non-blocking state machine
// ---------------------------------------------------------------------

/// Whether one byte of progress is `WouldBlock` — the fd is not ready.
fn would_block(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock
}

/// The result of one [`advance`] pass over an op.
enum Advance {
    /// The op finished — post this result, deregister the fd.
    Done(OffloadResult),
    /// The fd would block; stay registered and wait for the next event.
    Pending,
}

/// Drive `op` against `socket` as far as a non-blocking syscall allows.
/// Called once at submit time (data may already be buffered) and again
/// on every readiness event for the op's fd.
fn advance(op: &mut SocketOp, socket: &SocketInner, label: &'static str) -> Advance {
    // A concurrent `close` beats any pending readiness — without this a
    // `shutdown`-induced readable event would resolve as a clean EOF
    // rather than the catchable `closed` the spec promises.
    if socket.is_closed() {
        return Advance::Done(Err(net::offload_err(label, NetError::Closed)));
    }
    match op {
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
                return Advance::Done(Ok(OffloadOk::Int(data.len() as i64)));
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
/// the registration channel and announced with the [`Waker`].
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

/// An op the reactor is currently driving, keyed by its [`Token`] in
/// the op table. Holds only `Send` POD — never a GC root.
struct PendingOp {
    socket: SocketHandle,
    op: SocketOp,
    job_id: u64,
    mailbox: Arc<CompletionMailbox>,
    label: &'static str,
    /// The registered fd, kept so the op can be deregistered.
    fd: RawFd,
}

/// The process-wide reactor handle. Actors talk to the reactor thread
/// through `tx` (the registration channel) and `waker` (which pulls the
/// thread out of `poll`).
struct Reactor {
    tx: Mutex<Sender<Msg>>,
    waker: Waker,
}

static REACTOR: OnceLock<Reactor> = OnceLock::new();

/// The reactor handle, starting the reactor thread on first use.
fn reactor() -> &'static Reactor {
    REACTOR.get_or_init(|| {
        let poll = Poll::new().expect("reactor: create mio Poll");
        let waker = Waker::new(poll.registry(), WAKE)
            .expect("reactor: create mio Waker");
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("tigr-reactor".into())
            .spawn(move || reactor_loop(poll, rx))
            .expect("reactor: spawn thread");
        Reactor { tx: Mutex::new(tx), waker }
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
    r.waker.wake().expect("reactor: wake");
}

/// Cancel every reactor op on socket `socket_id` — drives `Net.close`.
/// A no-op if the reactor has never been started (no socket op was
/// ever offloaded).
pub fn cancel(socket_id: u64) {
    if let Some(r) = REACTOR.get() {
        if r.tx.lock().unwrap().send(Msg::Cancel { socket_id }).is_ok() {
            let _ = r.waker.wake();
        }
    }
}

/// The reactor thread's event loop. Owns the `Poll`, the op table and
/// the token counter; never returns.
fn reactor_loop(mut poll: Poll, rx: Receiver<Msg>) {
    let mut events = Events::with_capacity(256);
    let mut ops: HashMap<Token, PendingOp> = HashMap::new();
    // Op tokens start at 1 — `Token(0)` is the registration waker.
    let mut next_token: usize = 1;
    loop {
        if let Err(e) = poll.poll(&mut events, None) {
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            panic!("reactor: poll failed: {e}");
        }
        for event in events.iter() {
            if event.token() == WAKE {
                drain_messages(&rx, &poll, &mut ops, &mut next_token);
            } else {
                advance_token(event.token(), &poll, &mut ops);
            }
        }
    }
}

/// Drain every queued registration / cancellation message.
fn drain_messages(
    rx: &Receiver<Msg>,
    poll: &Poll,
    ops: &mut HashMap<Token, PendingOp>,
    next_token: &mut usize,
) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Msg::Submit { job_id, mailbox, rop } => {
                start_op(job_id, mailbox, rop, poll, ops, next_token);
            }
            Msg::Cancel { socket_id } => {
                cancel_socket(socket_id, poll, ops);
            }
        }
    }
}

/// Register a freshly-submitted op and drive it once — data may already
/// be buffered or the fd already ready, in which case it completes here
/// and is never registered.
fn start_op(
    job_id: u64,
    mailbox: Arc<CompletionMailbox>,
    rop: ReactorOp,
    poll: &Poll,
    ops: &mut HashMap<Token, PendingOp>,
    next_token: &mut usize,
) {
    let ReactorOp { socket, mut op, label } = rop;
    // Read ops watch the read half, writes the write half — distinct
    // `dup`'d fds, so a concurrent read + write register independently.
    let writes = matches!(op, SocketOp::WriteAll { .. });
    let (fd, interest) = if writes {
        (socket.write_raw_fd(), Interest::WRITABLE)
    } else {
        (socket.read_raw_fd(), Interest::READABLE)
    };
    let Some(fd) = fd else {
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
    // Try once: buffered data has no fd readiness to wait on, and the
    // fd may already be ready.
    if let Advance::Done(result) = advance(&mut op, &socket, label) {
        mailbox.post(job_id, result);
        return;
    }
    let token = Token(*next_token);
    *next_token = next_token.wrapping_add(1).max(1);
    let mut source = SourceFd(&fd);
    if let Err(e) = poll.registry().register(&mut source, token, interest) {
        mailbox.post(job_id, Err(net::offload_err(label, NetError::Io(e))));
        return;
    }
    ops.insert(token, PendingOp { socket, op, job_id, mailbox, label, fd });
}

/// A readiness event fired for `token` — drive that op forward.
fn advance_token(token: Token, poll: &Poll, ops: &mut HashMap<Token, PendingOp>) {
    let result = match ops.get_mut(&token) {
        Some(pending) => {
            match advance(&mut pending.op, &pending.socket, pending.label) {
                Advance::Done(result) => result,
                Advance::Pending => return,
            }
        }
        // The op already completed or was cancelled — a stale event.
        None => return,
    };
    let pending = ops.remove(&token).unwrap();
    let mut source = SourceFd(&pending.fd);
    let _ = poll.registry().deregister(&mut source);
    pending.mailbox.post(pending.job_id, result);
}

/// Fail every op on socket `socket_id` with `closed`, deregistering
/// each fd. The socket's `closed` flag is already set (by `close`), so
/// the woken coroutine sees the same `closed` an inline op would raise.
fn cancel_socket(socket_id: u64, poll: &Poll, ops: &mut HashMap<Token, PendingOp>) {
    let tokens: Vec<Token> = ops
        .iter()
        .filter(|(_, p)| p.socket.id() == socket_id)
        .map(|(t, _)| *t)
        .collect();
    for token in tokens {
        let pending = ops.remove(&token).unwrap();
        let mut source = SourceFd(&pending.fd);
        let _ = poll.registry().deregister(&mut source);
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
