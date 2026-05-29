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

/// The stdlib catalog as a JSON string, sent once to the playground so
/// CodeMirror can offer member completion (`String.` → its methods) and
/// identifier completion (module names, builtins, keywords) without a
/// per-keystroke round-trip. Shape:
///
/// ```json
/// { "modules": { "String": { "description": "...",
///       "members": [ { "name": "split", "signature": "...",
///                      "doc": "...", "constant": false } ] } },
///   "builtins": [ { "name": "print", "signature": "...", "doc": "..." } ],
///   "keywords": [ "fn", "if", ... ] }
/// ```
///
/// This is the same catalog the language server uses, so the playground's
/// suggestions match an editor's LSP. Hand-serialized (rather than via
/// serde) to keep the crate's dependency set unchanged.
#[wasm_bindgen]
pub fn catalog_json() -> String {
    use crate::catalog::Catalog;
    let cat = Catalog::load();
    let mut s = String::from("{\"modules\":{");

    let mut modules: Vec<_> = cat.modules().collect();
    modules.sort_by(|a, b| a.0.cmp(b.0));
    for (mi, (name, module)) in modules.iter().enumerate() {
        if mi > 0 {
            s.push(',');
        }
        s.push_str(&json_str(name));
        s.push_str(":{\"description\":");
        s.push_str(&json_str(&module.description));
        s.push_str(",\"members\":[");
        let mut members: Vec<_> = module.members.iter().collect();
        members.sort_by(|a, b| a.0.cmp(b.0));
        for (i, (mname, member)) in members.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"name\":");
            s.push_str(&json_str(mname));
            s.push_str(",\"signature\":");
            s.push_str(&json_str(&member.signature));
            s.push_str(",\"doc\":");
            s.push_str(&json_str(&member.doc));
            s.push_str(",\"constant\":");
            s.push_str(if member.is_constant() { "true" } else { "false" });
            s.push('}');
        }
        s.push_str("]}");
    }
    s.push_str("},\"builtins\":[");
    let mut builtins: Vec<_> = cat.builtins().collect();
    builtins.sort_by(|a, b| a.0.cmp(b.0));
    for (i, (name, member)) in builtins.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"name\":");
        s.push_str(&json_str(name));
        s.push_str(",\"signature\":");
        s.push_str(&json_str(&member.signature));
        s.push_str(",\"doc\":");
        s.push_str(&json_str(&member.doc));
        s.push('}');
    }
    s.push_str("],\"keywords\":[");
    let mut keywords: Vec<_> = cat.keywords().map(|(k, _)| k).collect();
    keywords.sort_unstable();
    for (i, k) in keywords.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&json_str(k));
    }
    s.push_str("]}");
    s
}

/// Every lex / parse / compile error in `source`, as a JSON array of
/// `{ from, to, message }` with UTF-16 offsets (CodeMirror's position
/// unit). Reuses `check_source`, the same recovering checker the language
/// server runs, so the playground reports every error at once, not just
/// the first. Runtime errors are excluded — they don't arise from a
/// static check.
#[wasm_bindgen]
pub fn diagnostics(source: &str) -> String {
    use crate::vm::check_source;
    use crate::vm::source_map::SourceId;

    let errors = check_source(source, None, SourceId::UNKNOWN);
    let mut s = String::from("[");
    let mut first = true;
    for err in &errors {
        if let Some((from, to, message)) = diag_span(source, err) {
            if !first {
                s.push(',');
            }
            first = false;
            s.push_str("{\"from\":");
            s.push_str(&from.to_string());
            s.push_str(",\"to\":");
            s.push_str(&to.to_string());
            s.push_str(",\"message\":");
            s.push_str(&json_str(&message));
            s.push('}');
        }
    }
    s.push(']');
    s
}

/// `(from, to, message)` in UTF-16 offsets for a frontend error, or
/// `None` for a runtime error (which has only a line and can't come from
/// a static check). Mirrors the language server's `error_span`.
fn diag_span(source: &str, err: &crate::vm::error::Error) -> Option<(usize, usize, String)> {
    use crate::vm::error::Error as E;
    let (span, message) = match err {
        E::Lex(e) => (e.span, e.to_string()),
        E::Parse(e) => (e.span, e.to_string()),
        E::Compile(e) => (e.span, e.to_string()),
        E::Runtime(_) => return None,
    };
    // Guarantee a non-empty range so a zero-width span (e.g. at EOF) still
    // shows a squiggle.
    let start = utf16_offset(source, span.start);
    let end = utf16_offset(source, span.end.max(span.start + 1));
    Some((start, end, message))
}

/// Convert a byte offset into `source` to a UTF-16 code-unit offset,
/// clamping to a char boundary and to the string's length so a stray span
/// can never panic the slice.
fn utf16_offset(source: &str, byte: usize) -> usize {
    let mut b = byte.min(source.len());
    while b > 0 && !source.is_char_boundary(b) {
        b -= 1;
    }
    source[..b].encode_utf16().count()
}

/// Escape a string as a JSON string literal (including the surrounding
/// quotes). Handles the control characters that appear in docstrings
/// (newlines, tabs) and the two mandatory escapes.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
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
