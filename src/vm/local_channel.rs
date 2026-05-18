//! Intra-actor channel — the message-passing conduit *between green
//! threads* of one actor (Phase 4 of the green-threads work).
//!
//! Unlike a cross-actor [`Channel`](crate::vm::channel), a
//! `LocalChannel` never crosses an OS-thread boundary: every coroutine
//! that touches it shares the one actor's heap. So it carries plain
//! [`Value`]s directly — no transfer-encoding, no deep copy. It is
//! GC-managed (a `LocalChannelKind` arena slot), since its buffered
//! messages are ordinary heap roots.
//!
//! `send`/`close` are non-blocking; a `recv` on an empty channel
//! cooperatively `yield`s the coroutine and retries — see
//! `stdlib/LocalChannel.tg`.

use std::collections::VecDeque;

use crate::vm::value::Value;

/// A buffered, unbounded intra-actor channel. Messages move by value
/// (no copy — coroutines share a heap); `send` never blocks.
pub struct LocalChannel {
    /// Buffered messages waiting for a receiver, oldest first.
    pub(crate) queue: VecDeque<Value>,
    /// Once closed, `send` raises and `recv` drains then reports
    /// `${closed: true}`.
    pub(crate) closed: bool,
}

impl LocalChannel {
    pub fn new() -> Self {
        LocalChannel { queue: VecDeque::new(), closed: false }
    }
}
