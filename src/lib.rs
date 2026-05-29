//! Tigr language library — the bytecode VM, compiler, and REPL.
//!
//! This crate also ships the `tigr` binary (`src/main.rs`). The library
//! target exists so the VM can be embedded elsewhere — in particular
//! the client-side WebAssembly playground build, whose `wasm-bindgen`
//! entry points live in [`wasm`].

pub mod repl;
pub mod vm;

/// A catalog of the language's named entities (builtins, stdlib module
/// members, keywords) with signatures and docstrings, parsed from the
/// committed `docs/stdlib/*.md`. The language server uses it for hover,
/// completion, and signature help; the wasm playground reuses it for the
/// same, so the two stay in lockstep.
pub mod catalog;

/// `wasm-bindgen` entry points for the browser playground. Only built
/// for the `wasm32` target; the native binary never sees it.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

#[cfg(test)]
mod tests;
