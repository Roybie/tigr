//! Capturable process output for `print` / `eprint`.
//!
//! The native CLI writes straight to stdout / stderr. Embedders — the
//! browser playground in particular, where there is no terminal —
//! install a capture buffer for the duration of a run via
//! [`with_capture`]. While one is installed, `print` and `eprint`
//! append to it ([`push`]) instead of writing to an absent console;
//! the embedder reads the captured text back when the run returns.
//!
//! The buffer is thread-local, which is exactly right: each actor
//! thread captures its own output, and the single-threaded wasm build
//! has just the one.

use std::cell::RefCell;

thread_local! {
    static SINK: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// True while a capture buffer is installed on this thread.
pub fn is_capturing() -> bool {
    SINK.with(|s| s.borrow().is_some())
}

/// Append `text` to the active capture buffer. A no-op when nothing is
/// capturing — callers test [`is_capturing`] first to decide whether to
/// write to the terminal instead.
pub fn push(text: &str) {
    SINK.with(|s| {
        if let Some(buf) = s.borrow_mut().as_mut() {
            buf.push_str(text);
        }
    });
}

/// Run `f` with a fresh capture buffer installed, returning its result
/// alongside everything `print` / `eprint` wrote during the call. A
/// previously installed buffer is saved and restored, so captures may
/// nest without losing the outer one.
pub fn with_capture<R>(f: impl FnOnce() -> R) -> (R, String) {
    let prev = SINK.with(|s| s.borrow_mut().replace(String::new()));
    let result = f();
    let captured = SINK.with(|s| {
        std::mem::replace(&mut *s.borrow_mut(), prev).unwrap_or_default()
    });
    (result, captured)
}
