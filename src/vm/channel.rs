//! Runtime channel — the message-passing conduit between actors
//! (v0.14 concurrency).
//!
//! A `Channel` is an `Arc`-shared, mutex-guarded queue. It is the one
//! reference type that legitimately crosses OS threads: it lives
//! outside any single heap, and the messages it carries are stored in
//! [`Transfer`] form — the sender encodes on its thread, the receiver
//! decodes on its own. `ChannelInner` is `Send + Sync`, so a
//! `ChannelHandle` rides through `Transfer::Channel` to a spawned
//! actor.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::vm::transfer::Transfer;

/// A shared handle to a channel. Cloning bumps the `Arc` refcount;
/// every holder may both send and receive (channels are bidirectional
/// — see the v0.14 plan's "single bidirectional handle" decision).
pub type ChannelHandle = Arc<ChannelInner>;

pub struct ChannelInner {
    queue: Mutex<VecDeque<Transfer>>,
    not_empty: Condvar,
    not_full: Condvar,
    /// `None` for an unbounded channel; `Some(n)` caps the buffer at
    /// `n` and makes `send` block (backpressure) while full.
    capacity: Option<usize>,
    closed: AtomicBool,
}

/// The outcome of a [`ChannelInner::recv`].
pub enum RecvOutcome {
    /// A message, still transfer-encoded — the caller decodes it into
    /// its own heap.
    Message(Transfer),
    /// The channel is closed and the buffer is drained.
    Closed,
}

impl ChannelInner {
    /// Create a channel. `capacity = None` is unbounded; `Some(n)`
    /// bounds the buffer at `n`.
    pub fn new(capacity: Option<usize>) -> ChannelHandle {
        Arc::new(ChannelInner {
            queue: Mutex::new(VecDeque::new()),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
            capacity,
            closed: AtomicBool::new(false),
        })
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Enqueue a message. On a bounded channel, blocks while the
    /// buffer is full. Returns `Err(())` if the channel is closed.
    pub fn send(&self, msg: Transfer) -> Result<(), ()> {
        let mut q = self.queue.lock().unwrap();
        if let Some(cap) = self.capacity {
            while q.len() >= cap && !self.is_closed() {
                q = self.not_full.wait(q).unwrap();
            }
        }
        if self.is_closed() {
            return Err(());
        }
        q.push_back(msg);
        drop(q);
        self.not_empty.notify_one();
        wake_selectors();
        Ok(())
    }

    /// Dequeue a message, blocking while the buffer is empty and the
    /// channel is still open.
    pub fn recv(&self) -> RecvOutcome {
        let mut q = self.queue.lock().unwrap();
        loop {
            if let Some(msg) = q.pop_front() {
                drop(q);
                self.not_full.notify_one();
                return RecvOutcome::Message(msg);
            }
            if self.is_closed() {
                return RecvOutcome::Closed;
            }
            q = self.not_empty.wait(q).unwrap();
        }
    }

    /// Non-blocking receive. `None` means nothing is available right
    /// now and the channel is still open.
    pub fn try_recv(&self) -> Option<RecvOutcome> {
        let mut q = self.queue.lock().unwrap();
        if let Some(msg) = q.pop_front() {
            drop(q);
            self.not_full.notify_one();
            return Some(RecvOutcome::Message(msg));
        }
        if self.is_closed() {
            Some(RecvOutcome::Closed)
        } else {
            None
        }
    }

    /// Mark the channel closed and wake every blocked sender/receiver
    /// so they observe it.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.not_empty.notify_all();
        self.not_full.notify_all();
        wake_selectors();
    }
}

// ---- select: waiting on several channels at once -------------------

/// Process-wide parker for `select`. A thread blocked in [`select`]
/// waits on `wake`; every `send`/`close` calls [`wake_selectors`].
/// `selectors` keeps the common no-select path off the global lock.
struct SelectPark {
    lock: Mutex<()>,
    wake: Condvar,
    selectors: AtomicUsize,
}

fn select_park() -> &'static SelectPark {
    static PARK: OnceLock<SelectPark> = OnceLock::new();
    PARK.get_or_init(|| SelectPark {
        lock: Mutex::new(()),
        wake: Condvar::new(),
        selectors: AtomicUsize::new(0),
    })
}

/// Wake every thread parked in `select`. A no-op (one atomic load)
/// when no `select` is in progress.
fn wake_selectors() {
    let p = select_park();
    if p.selectors.load(Ordering::Acquire) == 0 {
        return;
    }
    let _g = p.lock.lock().unwrap();
    p.wake.notify_all();
}

/// The outcome of a [`select`].
pub enum SelectResult {
    /// Channel `index` produced a message (still transfer-encoded).
    Fired { index: usize, message: Transfer },
    /// Nothing was ready and the caller permitted a non-blocking
    /// return (an `else` arm).
    ElseReady,
    /// Every channel is closed and drained — a blocking `select` has
    /// nothing left to wait for.
    AllClosed,
}

/// Block until one of `chans` has a message; return which fired. A
/// closed-and-drained channel is skipped; if all are closed, returns
/// `AllClosed`. With `has_else`, returns `ElseReady` instead of
/// blocking when nothing is ready.
///
/// The parker lock is held across the channel scan and the wait. This
/// is deadlock-free: `send`/`close` acquire the parker lock only
/// *after* releasing their channel lock, so no thread ever holds a
/// channel lock while waiting on the parker lock.
pub fn select(chans: &[ChannelHandle], has_else: bool) -> SelectResult {
    if chans.is_empty() {
        return if has_else { SelectResult::ElseReady } else { SelectResult::AllClosed };
    }
    let p = select_park();
    p.selectors.fetch_add(1, Ordering::AcqRel);
    let mut guard = p.lock.lock().unwrap();
    let result = loop {
        let mut hit = None;
        let mut all_closed = true;
        for (i, ch) in chans.iter().enumerate() {
            match ch.try_recv() {
                Some(RecvOutcome::Message(t)) => {
                    hit = Some((i, t));
                    break;
                }
                Some(RecvOutcome::Closed) => {} // channel dead — skip it
                None => all_closed = false,
            }
        }
        if let Some((index, message)) = hit {
            break SelectResult::Fired { index, message };
        }
        if all_closed {
            break SelectResult::AllClosed;
        }
        if has_else {
            break SelectResult::ElseReady;
        }
        guard = p.wake.wait(guard).unwrap();
    };
    drop(guard);
    p.selectors.fetch_sub(1, Ordering::AcqRel);
    result
}
