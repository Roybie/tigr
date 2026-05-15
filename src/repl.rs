//! Interactive REPL.
//!
//! Mechanic (see plan §Phase 5): a single long-lived `Vm` frame holds
//! the session state. Each line is compiled with knowledge of the
//! locals declared by prior lines and runs through `Vm::run_repl_line`
//! which preserves the frame across `Halt`. Uncaught raises print and
//! the session continues with state intact (the REPL frame is a
//! `try_catch` wall).
//!
//! Multi-line input: if the lexer trips on an unterminated string OR
//! the parser hits EOF mid-expression, we prompt for more lines and
//! re-try the whole accumulated buffer.

use std::io::{self, BufRead, Write};
use std::rc::Rc;

use crate::vm::compiler::Compiler;
use crate::vm::error::{Error, LexErrorKind, ParseErrorKind};
use crate::vm::lexer::Lexer;
use crate::vm::parser;
use crate::vm::token::Token;
use crate::vm::value::{Closure, Value};
use crate::vm::vm::Vm;

pub struct Repl {
    vm: Vm,
    /// Names + slots of bindings the user has declared across lines.
    /// The compiler pre-declares these for each new line so name
    /// resolution emits the right `LoadLocal` slot.
    locals: Vec<(String, u8)>,
}

impl Repl {
    pub fn new() -> Self {
        let mut vm = Vm::new();
        vm.start_repl();
        Repl { vm, locals: Vec::new() }
    }

    /// Evaluate one line of source. The caller is responsible for
    /// accumulating multi-line input.
    pub fn eval(&mut self, source: &str) -> Result<Value, Error> {
        let tokens = Lexer::new(source).tokenize()?;
        let program = parser::parse(tokens)?;
        let (main, new_locals) = Compiler::compile_repl(&program, &self.locals)?;

        let closure = Rc::new(Closure {
            function: Rc::new(main),
            upvalues: Vec::new(),
        });

        // Snapshot the stack length BEFORE the new locals materialise.
        // On uncaught raise, the Vm truncates to this length and the
        // new locals are discarded — the user's session sees no
        // half-introduced bindings.
        let snapshot_len = 1 + self.locals.len();

        match self.vm.run_repl_line(closure, snapshot_len) {
            Ok(v) => {
                // Line succeeded — commit the new locals.
                self.locals.extend(new_locals);
                Ok(v)
            }
            Err(e) => Err(Error::Runtime(e)),
        }
    }

    /// Top-level loop. Reads from stdin, prints values to stdout,
    /// errors to stderr. Returns on `:quit`/`:q` or EOF.
    pub fn run(&mut self) -> io::Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();

        let mut buf = String::new();
        loop {
            // Prompt.
            let prompt = if buf.is_empty() { "tigr> " } else { "..> " };
            write!(output, "{prompt}")?;
            output.flush()?;

            // Read one line. EOF → exit.
            let mut line = String::new();
            let n = input.read_line(&mut line)?;
            if n == 0 {
                writeln!(output)?;
                return Ok(());
            }

            // Handle REPL commands ONLY when not mid-multiline.
            if buf.is_empty() {
                let trimmed = line.trim();
                if trimmed == ":quit" || trimmed == ":q" {
                    return Ok(());
                }
                if trimmed.is_empty() {
                    continue;
                }
            }

            buf.push_str(&line);

            // Try to evaluate. If the input looks incomplete, prompt
            // for more; otherwise either print the value or the error
            // and reset the buffer.
            match self.eval(&buf) {
                Ok(v) => {
                    writeln!(output, "{v}")?;
                    buf.clear();
                }
                Err(e) if is_incomplete(&e) => {
                    // Stay in multi-line mode; loop reads more.
                }
                Err(e) => {
                    let mut err = io::stderr();
                    writeln!(err, "{e}")?;
                    buf.clear();
                }
            }
        }
    }
}

/// True iff the error suggests the user just needs to type more.
/// Conservative — we only continue on the two unambiguous signals:
/// unterminated string literals and parsers expecting more after EOF.
fn is_incomplete(err: &Error) -> bool {
    match err {
        Error::Lex(e) => matches!(e.kind, LexErrorKind::UnterminatedString),
        Error::Parse(e) => match &e.kind {
            ParseErrorKind::Expected { found, .. } => matches!(found, Token::Eof),
            ParseErrorKind::UnexpectedToken(t) => matches!(t, Token::Eof),
            _ => false,
        },
        _ => false,
    }
}
