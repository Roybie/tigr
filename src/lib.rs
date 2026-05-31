//! Tigr language library — the bytecode VM, compiler, and REPL.
//!
//! This crate also ships the `tigr` binary (`src/main.rs`). The library
//! target exists so the VM can be embedded elsewhere — in particular
//! the client-side WebAssembly playground build, whose `wasm-bindgen`
//! entry points live in [`wasm`].

pub mod repl;
pub mod vm;

/// Embedding API: drive a persistent VM from a Rust host (register
/// native modules, load a program, call its top-level functions each
/// frame). Gated behind the `embed` feature so it is absent from the
/// default build and the wasm playground.
#[cfg(feature = "embed")]
pub mod embed;

/// A catalog of the language's named entities (builtins, stdlib module
/// members, keywords) with signatures and docstrings, parsed from the
/// committed `docs/stdlib/*.md`. The language server uses it for hover,
/// completion, and signature help; the wasm playground reuses it for the
/// same, so the two stay in lockstep.
pub mod catalog;

/// `wasm-bindgen` entry points for the browser playground. Only built
/// for the `wasm32` target *and* the `playground` feature (on by
/// default): the native binary never sees it, and an embedder building
/// for a non-`wasm-bindgen` web host (e.g. macroquad's plain-wasm
/// loader) turns the feature off so this glue — and its `wasm-bindgen`
/// imports — stays out of the module.
#[cfg(all(target_arch = "wasm32", feature = "playground"))]
pub mod wasm;

#[cfg(test)]
mod tests;
