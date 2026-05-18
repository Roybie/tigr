//! Interactive REPL.
//!
//! Mechanic (see plan §Phase 5): a single long-lived `Vm` frame holds
//! the session state. Each line is compiled with knowledge of the
//! locals declared by prior lines and runs through `Vm::run_repl_line`
//! which preserves the frame across `Halt`. Uncaught raises print and
//! the session continues with state intact (the REPL frame is a
//! `try_catch` wall).
//!
//! Input is read via `rustyline` so arrow keys, history, Ctrl+A/E
//! cursor moves, etc. all just work. Each accepted line is appended
//! to history (single-line entries — multi-line submissions show as
//! a stack of related lines on ↑, which works fine for short blocks).
//!
//! Multi-line input: if the lexer trips on an unterminated string OR
//! the parser hits EOF mid-expression, we prompt for more lines and
//! re-try the whole accumulated buffer.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::vm::compiler::Compiler;
use crate::vm::error::{Error, LexErrorKind, ParseErrorKind};
use crate::vm::lexer::Lexer;
use crate::vm::parser;
use crate::vm::source_map::SourceMap;
use crate::vm::token::Token;
use crate::vm::value::{Closure, Value};
use crate::vm::vm::Vm;

pub struct Repl {
    vm: Vm,
    /// Names + slots of bindings the user has declared across lines.
    /// The compiler pre-declares these for each new line so name
    /// resolution emits the right `LoadLocal` slot.
    locals: Vec<(String, u8)>,
    /// Shared with the Vm so import-time source registration shows up
    /// here too. Each REPL line is registered as `<repl line N>`.
    sources: Rc<RefCell<SourceMap>>,
    line_no: u32,
}

impl Repl {
    pub fn new() -> Self {
        let sources = Rc::new(RefCell::new(SourceMap::new()));
        let mut vm = Vm::with_source_map(sources.clone());
        vm.start_repl();
        Repl { vm, locals: Vec::new(), sources, line_no: 0 }
    }

    /// Evaluate one line of source. The caller is responsible for
    /// accumulating multi-line input.
    pub fn eval(&mut self, source: &str) -> Result<Value, Error> {
        self.line_no += 1;
        let sid = self
            .sources
            .borrow_mut()
            .add(format!("<repl:{}>", self.line_no), source);

        let tokens = Lexer::new(source).tokenize().map_err(|mut e| {
            e.source = sid;
            Error::from(e)
        })?;
        let program = parser::parse(tokens).map_err(|mut e| {
            e.source = sid;
            Error::from(e)
        })?;
        let (main, new_locals) =
            Compiler::compile_repl_with_source(&program, &self.locals, sid)?;

        let closure = crate::vm::gc::alloc_closure(Closure {
            function: Arc::new(main),
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

    /// Borrow the source map for rendering an error returned from
    /// [`eval`].
    #[allow(dead_code)]
    pub fn sources(&self) -> std::cell::Ref<'_, SourceMap> {
        self.sources.borrow()
    }

    /// Top-level loop. Reads via rustyline (arrow keys, history,
    /// line editing), prints values to stdout, errors to stderr.
    /// Returns on `:quit`/`:q`, Ctrl+D, or a rustyline failure.
    pub fn run(&mut self) -> Result<(), ReadlineError> {
        let mut rl = DefaultEditor::new()?;
        let history_path = history_file();
        if let Some(p) = &history_path {
            // Missing history file is fine on first run; ignore.
            let _ = rl.load_history(p);
        }

        let mut buf = String::new();
        loop {
            let prompt = if buf.is_empty() { "tigr> " } else { "..> " };
            match rl.readline(prompt) {
                Ok(line) => {
                    // Commands only count outside multi-line mode.
                    if buf.is_empty() {
                        let trimmed = line.trim();
                        if trimmed == ":quit" || trimmed == ":q" {
                            break;
                        }
                        if trimmed.is_empty() {
                            continue;
                        }
                    }

                    // Each accepted line goes into history individually
                    // so ↑ walks line-by-line (Python-style).
                    let _ = rl.add_history_entry(line.as_str());

                    buf.push_str(&line);
                    // rustyline strips the trailing newline; put one
                    // back so the lexer's line counter is accurate
                    // across multi-line input.
                    buf.push('\n');

                    match self.eval(&buf) {
                        Ok(v) => {
                            println!("{v}");
                            buf.clear();
                        }
                        Err(e) if is_incomplete(&e) => {
                            // Stay in multi-line mode; loop reads more.
                        }
                        Err(e) => {
                            eprintln!("{}", e.render(&self.sources.borrow()));
                            buf.clear();
                        }
                    }
                }
                // Ctrl+C: abandon any partial input, reprompt fresh.
                Err(ReadlineError::Interrupted) => {
                    if !buf.is_empty() {
                        buf.clear();
                        continue;
                    }
                    break;
                }
                // Ctrl+D on an empty line: exit. On a non-empty buf,
                // also exit (we can't sensibly resume).
                Err(ReadlineError::Eof) => break,
                Err(e) => return Err(e),
            }
        }

        if let Some(p) = &history_path {
            let _ = rl.save_history(p);
        }
        Ok(())
    }
}

/// Path to the persistent history file (`~/.tigr_history` on Unix-like
/// platforms; `%USERPROFILE%\.tigr_history` on Windows). `None` if no
/// home directory can be resolved — history just won't persist.
fn history_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))?;
    let mut p = PathBuf::from(home);
    p.push(".tigr_history");
    Some(p)
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
