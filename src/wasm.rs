//! `wasm-bindgen` entry points for the browser playground.
//!
//! Two surfaces, matching the two playground tabs:
//!
//! * [`WasmRepl`] — a persistent REPL session for the console tab.
//!   Each [`WasmRepl::eval`] runs one submission with session state
//!   (bindings declared by earlier lines) intact.
//! * [`run_program`] — a one-shot run for the editor tab: a fresh
//!   session evaluates the whole source.
//!
//! Both return an [`EvalResult`]. `print` / `eprint` output is captured
//! (see [`crate::vm::io_capture`]) and returned in `output` rather than
//! lost to an absent console; errors arrive pre-rendered with the same
//! caret-underlined formatting the native CLI prints.

use wasm_bindgen::prelude::*;

use crate::repl::{is_incomplete, Repl};
use crate::vm::io_capture;

/// The outcome of one `eval` / `run_program` call, read field-by-field
/// from JavaScript.
#[wasm_bindgen]
pub struct EvalResult {
    ok: bool,
    incomplete: bool,
    value: String,
    output: String,
    error: String,
}

#[wasm_bindgen]
impl EvalResult {
    /// True if the submission ran to completion without raising.
    #[wasm_bindgen(getter)]
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// True if the submission failed only because it was unfinished —
    /// the console tab should keep a continuation prompt open and not
    /// surface `error` to the user yet.
    #[wasm_bindgen(getter)]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// The submission's final value, formatted as the REPL prints it.
    /// Empty when `ok` is false.
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> String {
        self.value.clone()
    }

    /// Everything `print` / `eprint` wrote during the run, in order.
    #[wasm_bindgen(getter)]
    pub fn output(&self) -> String {
        self.output.clone()
    }

    /// The rendered error (caret-underlined source, stack trace) when
    /// `ok` is false. Empty when `ok` is true.
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> String {
        self.error.clone()
    }
}

/// A persistent REPL session — one per console tab.
#[wasm_bindgen]
pub struct WasmRepl {
    repl: Repl,
}

#[wasm_bindgen]
impl WasmRepl {
    /// Start a fresh session with no bindings.
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmRepl {
        WasmRepl { repl: Repl::new() }
    }

    /// Evaluate one submission, carrying session state forward.
    pub fn eval(&mut self, source: &str) -> EvalResult {
        let (result, output) = io_capture::with_capture(|| self.repl.eval(source));
        shape(result, output, &self.repl)
    }
}

impl Default for WasmRepl {
    fn default() -> Self {
        Self::new()
    }
}

/// The crate version (`CARGO_PKG_VERSION`), so the playground UI shows
/// the same version the wasm was built from — one source of truth, no
/// hand-edited version string in the HTML to drift on a release bump.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Evaluate `source` in a throwaway session — the editor tab's "Run".
#[wasm_bindgen]
pub fn run_program(source: &str) -> EvalResult {
    let mut repl = Repl::new();
    let (result, output) = io_capture::with_capture(|| repl.eval(source));
    shape(result, output, &repl)
}

/// Turn an `eval` outcome plus its captured output into an
/// [`EvalResult`]. `repl` supplies the source map for error rendering.
fn shape(
    result: Result<crate::vm::value::Value, crate::vm::error::Error>,
    output: String,
    repl: &Repl,
) -> EvalResult {
    match result {
        Ok(value) => EvalResult {
            ok: true,
            incomplete: false,
            value: value.to_string(),
            output,
            error: String::new(),
        },
        Err(err) => EvalResult {
            ok: false,
            incomplete: is_incomplete(&err),
            value: String::new(),
            output,
            error: err.render(&repl.sources()),
        },
    }
}
