//! `wasm32` stub for [`crate::vm::socket`].
//!
//! Sockets have no browser equivalent, and the `Net` module is not
//! registered on `wasm32` (see `native_modules::resolve`), so no
//! `SocketHandle` or `ReactorOp` is ever constructed at runtime. These
//! types exist only so the shared `value`, `offload`, `vm`, and
//! `transfer` modules — which name them unconditionally — compile
//! unchanged for the browser build.

use std::sync::Arc;

/// Stub socket handle. Never constructed on `wasm32`.
pub type SocketHandle = Arc<SocketInner>;

/// Opaque socket payload. Has no constructor, so a `SocketHandle` can
/// never come into existence on `wasm32`.
pub struct SocketInner {
    _private: (),
}

impl SocketInner {
    /// A real socket's monotonic id. Unreachable on `wasm32`.
    pub fn id(&self) -> u64 {
        0
    }
}

/// Stub reactor op — the unit a `NativeKind::Socket` native would
/// produce. Has no constructor; never built on `wasm32`.
pub struct ReactorOp {
    _private: (),
}
