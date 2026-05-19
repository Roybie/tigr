//! `wasm32` stub for [`crate::vm::reactor`].
//!
//! The async-IO reactor drives socket ops on an `epoll`/`kqueue` poll
//! thread — neither sockets nor that thread exist in the browser. The
//! `Net` module is unregistered on `wasm32`, so no `ReactorOp` is ever
//! produced and these entry points are unreachable. They exist only so
//! `vm.rs`, which calls them unconditionally on the socket dispatch
//! path, compiles unchanged.

use std::sync::Arc;

use crate::vm::offload::{CompletionMailbox, OffloadResult};
use crate::vm::socket::ReactorOp;

/// Unreachable on `wasm32` — no `ReactorOp` is ever constructed.
#[allow(unused_variables)]
pub fn run_blocking(rop: ReactorOp) -> OffloadResult {
    unreachable!("reactor::run_blocking: no socket ops exist on wasm32")
}

/// Unreachable on `wasm32` — no `ReactorOp` is ever constructed.
#[allow(unused_variables)]
pub fn submit(job_id: u64, mailbox: Arc<CompletionMailbox>, rop: ReactorOp) {
    unreachable!("reactor::submit: no socket ops exist on wasm32")
}

/// No-op on `wasm32` — there is no reactor and no socket to cancel.
#[allow(unused_variables)]
pub fn cancel(socket_id: u64) {}
