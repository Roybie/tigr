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
pub mod token;
pub mod value;
pub mod vm;

use std::path::Path;

use self::compiler::Compiler;
use self::error::Error;
use self::lexer::Lexer;
use self::value::Value;
use self::vm::Vm;

/// Run a source file end-to-end. Returns the program's final value.
pub fn run_file(path: &Path) -> Result<Value, Error> {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("couldn't read {}: {}", path.display(), e));
    run_source(&source)
}

/// Compile and execute a string of Tigr source. Returns the final value.
pub fn run_source(source: &str) -> Result<Value, Error> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = parser::parse(tokens)?;
    let chunk = Compiler::compile(&program)?;
    let mut vm = Vm::new();
    let value = vm.run(&chunk)?;
    Ok(value)
}
