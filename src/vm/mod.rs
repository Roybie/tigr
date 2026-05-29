//! Tigr v0.2 bytecode VM.
//!
//! Pipeline: source → [`lexer`] → [`token`]s → [`parser`] → [`ast`] →
//! [`compiler`] → [`chunk`] of [`opcode`]s → [`vm`] → [`value`].

pub mod ast;
pub mod channel;
pub mod chunk;
pub mod compiler;
pub mod error;
pub mod file_handle;
pub mod fold;
pub mod gc;
pub mod io_capture;
pub mod lexer;
pub mod local_channel;
pub mod native_modules;
pub mod offload;
pub mod opcode;
pub mod parser;
/// The async-IO reactor. The real `epoll`/`kqueue`-backed implementation
/// is native-only; the `wasm32` build swaps in an inert stub since the
/// browser has no sockets (see `reactor_stub.rs`).
#[cfg(not(target_arch = "wasm32"))]
pub mod reactor;
#[cfg(target_arch = "wasm32")]
#[path = "reactor_stub.rs"]
pub mod reactor;
pub mod rng;
pub mod scheduler;
/// Network sockets. Native-only; the `wasm32` build swaps in a
/// type-only stub (the `Net` module is unregistered there).
#[cfg(not(target_arch = "wasm32"))]
pub mod socket;
#[cfg(target_arch = "wasm32")]
#[path = "socket_stub.rs"]
pub mod socket;
pub mod source_map;
pub mod source_stdlib;
pub mod stdlib;
pub mod task;
pub mod token;
pub mod transfer;
pub mod value;
pub mod vm;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use self::compiler::Compiler;
use self::error::{Error, RuntimeError, RuntimeErrorKind};
use self::lexer::Lexer;
use self::source_map::{SourceId, SourceMap};
use self::value::Value;
use self::vm::Vm;

/// Run a source file end-to-end. Returns the program's final value.
/// Imports declared inside the source are resolved relative to this
/// file's parent directory (spec §12). Errors render bare; use
/// [`run_file_with_map`] when you want the source map back for
/// snippet rendering.
#[allow(dead_code)]
pub fn run_file(path: &Path) -> Result<Value, Error> {
    let sources = Rc::new(RefCell::new(SourceMap::new()));
    run_file_with_map(path, sources).map(|(v, _)| v)
}

/// Run a file but also surface the [`SourceMap`] so the caller can
/// render any returned error against it. Used by the CLI driver.
pub fn run_file_with_map(
    path: &Path,
    sources: Rc<RefCell<SourceMap>>,
) -> Result<(Value, Rc<RefCell<SourceMap>>), Error> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Err(Error::Runtime(RuntimeError::new(
                RuntimeErrorKind::ImportFailed(
                    path.display().to_string(),
                    e.to_string(),
                ),
                0,
            )));
        }
    };
    let path_buf = PathBuf::from(path);
    let sid = sources.borrow_mut().add_path(&path_buf, source.clone());
    let base_dir = path.parent().map(PathBuf::from);
    let value = run_source_inner(&source, base_dir, sid, sources.clone())?;
    Ok((value, sources))
}

/// Compile and execute a string of Tigr source. Returns the final
/// value. With no base directory, relative imports resolve against
/// the process cwd at runtime (rarely what you want — prefer
/// `run_file` when possible).
#[allow(dead_code)]
pub fn run_source(source: &str) -> Result<Value, Error> {
    let sources = Rc::new(RefCell::new(SourceMap::new()));
    let sid = sources.borrow_mut().add("<string>", source);
    run_source_inner(source, None, sid, sources)
}

/// Same as [`run_source`] but also returns the populated [`SourceMap`]
/// so callers (mainly tests) can render an error.
#[allow(dead_code)]
pub fn run_source_with_map(
    source: &str,
) -> (Result<Value, Error>, Rc<RefCell<SourceMap>>) {
    let sources = Rc::new(RefCell::new(SourceMap::new()));
    let sid = sources.borrow_mut().add("<string>", source);
    let result = run_source_inner(source, None, sid, sources.clone());
    (result, sources)
}

fn run_source_inner(
    source: &str,
    base_dir: Option<PathBuf>,
    sid: SourceId,
    sources: Rc<RefCell<SourceMap>>,
) -> Result<Value, Error> {
    let tokens = Lexer::new(source).tokenize().map_err(|mut e| {
        e.source = sid;
        Error::from(e)
    })?;
    let mut program = parser::parse(tokens).map_err(|mut e| {
        e.source = sid;
        Error::from(e)
    })?;
    fold::fold_program(&mut program);
    let main = Compiler::compile_with_source(&program, base_dir, sid)?;
    let mut vm = Vm::with_source_map(sources);
    let value = vm.run(main)?;
    Ok(value)
}

/// Read and compile a file without running it, registering the file's
/// source in the supplied [`SourceMap`]. Used by the Import opcode to
/// push the imported module's `<main>` as a fresh call frame on the
/// SAME Vm (sharing the cache, see [`vm::Vm`]).
///
/// All error paths fold into `Error` — the caller is expected to
/// re-wrap as `RuntimeError::ImportFailed` so the import can be
/// caught with `try`.
pub fn compile_file_into(
    path: &Path,
    sources: &mut SourceMap,
) -> Result<self::value::Function, Error> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        Error::Runtime(RuntimeError::new(
            RuntimeErrorKind::ImportFailed(
                path.display().to_string(),
                e.to_string(),
            ),
            0,
        ))
    })?;
    let path_buf = PathBuf::from(path);
    let sid = sources.add_path(&path_buf, source.clone());
    let base_dir = path.parent().map(PathBuf::from);
    compile_source_with_id(&source, base_dir, sid)
}

/// Compile pre-loaded source with a known [`SourceId`]. Used by the
/// Import opcode for source-stdlib modules.
pub fn compile_source_with_id(
    source: &str,
    base_dir: Option<PathBuf>,
    sid: SourceId,
) -> Result<self::value::Function, Error> {
    let tokens = Lexer::new(source).tokenize().map_err(|mut e| {
        e.source = sid;
        Error::from(e)
    })?;
    let mut program = parser::parse(tokens).map_err(|mut e| {
        e.source = sid;
        Error::from(e)
    })?;
    fold::fold_program(&mut program);
    let main = Compiler::compile_with_source(&program, base_dir, sid)?;
    Ok(main)
}

/// Parse a source into its (recovered) AST for tooling that walks the
/// tree — go-to-definition, hover. Lexes with recovery, so a stray bad
/// character no longer wipes out the whole tree (the LSP gets the lex
/// errors themselves via [`check_source`]). The tree is deliberately
/// NOT constant-folded, so every identifier and span survives for
/// position lookups.
pub fn parse_tree(source: &str) -> self::ast::Block {
    let (tokens, _lex_errors) = Lexer::new(source).tokenize_recover();
    parser::parse_recover(tokens).0
}

/// Collect every diagnostic for a source without running it. Each stage
/// recovers so it reports *all* of its errors at once: the lexer skips
/// bad characters (multiple lex errors), the parser resyncs at statement
/// boundaries (multiple parse errors), and the compiler accumulates
/// semantic errors (multiple compile errors). Errors are stamped with
/// `sid`. Used by the language server.
///
/// The three kinds are never mixed in one report: each stage's errors
/// are returned alone, earliest non-empty stage first. A partial token
/// stream or partial tree from a failed earlier stage would spawn
/// spurious downstream errors (undeclared variables from dropped
/// declarations, etc.), so we surface one stage's errors and let the
/// next edit reveal the rest.
pub fn check_source(
    source: &str,
    base_dir: Option<PathBuf>,
    sid: SourceId,
) -> Vec<Error> {
    let (tokens, lex_errors) = Lexer::new(source).tokenize_recover();
    if !lex_errors.is_empty() {
        return lex_errors
            .into_iter()
            .map(|mut e| {
                e.source = sid;
                Error::from(e)
            })
            .collect();
    }
    let (mut program, parse_errors) = parser::parse_recover(tokens);
    if !parse_errors.is_empty() {
        return parse_errors
            .into_iter()
            .map(|mut e| {
                e.source = sid;
                Error::from(e)
            })
            .collect();
    }
    fold::fold_program(&mut program);
    Compiler::compile_check(&program, base_dir, sid)
        .into_iter()
        .map(Error::from)
        .collect()
}
