//! Tigr v0.2 bytecode VM.
//!
//! Pipeline: source → [`lexer`] → [`token`]s → [`parser`] → [`ast`] →
//! [`compiler`] → [`chunk`] of [`opcode`]s → [`vm`] → [`value`].

pub mod ast;
pub mod chunk;
pub mod compiler;
pub mod error;
pub mod lexer;
pub mod opcode;
pub mod parser;
pub mod stdlib;
pub mod token;
pub mod value;
pub mod vm;

use std::path::{Path, PathBuf};

use self::compiler::Compiler;
use self::error::{Error, RuntimeError, RuntimeErrorKind};
use self::lexer::Lexer;
use self::value::Value;
use self::vm::Vm;

/// Run a source file end-to-end. Returns the program's final value.
/// Imports declared inside the source are resolved relative to this
/// file's parent directory (spec §12).
pub fn run_file(path: &Path) -> Result<Value, Error> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        Error::Runtime(RuntimeError::new(
            RuntimeErrorKind::ImportFailed(
                path.display().to_string(),
                e.to_string(),
            ),
            0,
        ))
    })?;
    let base_dir = path.parent().map(PathBuf::from);
    run_source_with_dir(&source, base_dir)
}

/// Compile and execute a string of Tigr source. Returns the final
/// value. With no base directory, relative imports resolve against
/// the process cwd at runtime (rarely what you want — prefer
/// `run_file` when possible).
#[allow(dead_code)]
pub fn run_source(source: &str) -> Result<Value, Error> {
    run_source_with_dir(source, None)
}

fn run_source_with_dir(
    source: &str,
    base_dir: Option<PathBuf>,
) -> Result<Value, Error> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = parser::parse(tokens)?;
    let main = Compiler::compile_with_dir(&program, base_dir)?;
    let mut vm = Vm::new();
    let value = vm.run(main)?;
    Ok(value)
}
