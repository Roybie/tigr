//! Compile-time and runtime errors.

use std::fmt;

use crate::vm::source_map::{SourceId, SourceMap};
use crate::vm::token::{Span, Token};

// ---------------- Lex ----------------

#[derive(Debug)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
    pub source: SourceId,
}

impl LexError {
    pub fn new(kind: LexErrorKind, span: Span) -> Self {
        LexError { kind, span, source: SourceId::UNKNOWN }
    }
}

#[derive(Debug)]
pub enum LexErrorKind {
    InvalidChar(char),
    UnterminatedString,
    NumberOutOfRange(String),
    MalformedNumber(String),
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LexErrorKind::InvalidChar(c) => write!(f, "unexpected character '{}'", c),
            LexErrorKind::UnterminatedString => f.write_str("unterminated string literal"),
            LexErrorKind::NumberOutOfRange(lex) => {
                write!(f, "number literal out of range: {}", lex)
            }
            LexErrorKind::MalformedNumber(lex) => {
                write!(f, "malformed number literal: {}", lex)
            }
        }
    }
}

// ---------------- Parse ----------------

#[derive(Debug)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
    pub source: SourceId,
}

impl ParseError {
    pub fn new(kind: ParseErrorKind, span: Span) -> Self {
        ParseError { kind, span, source: SourceId::UNKNOWN }
    }
}

#[derive(Debug)]
pub enum ParseErrorKind {
    Expected { expected: Token, found: Token },
    UnexpectedToken(Token),
    InvalidAssignTarget,
    ExpectedEof(Token),
    InterpolationError(String),
    InvalidPattern(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ParseErrorKind::Expected { expected, found } => {
                write!(f, "expected `{}`, found `{}`", expected, found)
            }
            ParseErrorKind::UnexpectedToken(t) => write!(f, "unexpected token `{}`", t),
            ParseErrorKind::InvalidAssignTarget => f.write_str("invalid assignment target"),
            ParseErrorKind::ExpectedEof(t) => write!(f, "expected end of input, found `{}`", t),
            ParseErrorKind::InterpolationError(m) => write!(f, "in interpolation: {m}"),
            ParseErrorKind::InvalidPattern(m) => write!(f, "invalid pattern: {m}"),
        }
    }
}

// ---------------- Compile ----------------

#[derive(Debug)]
pub struct CompileError {
    pub kind: CompileErrorKind,
    pub span: Span,
    pub source: SourceId,
}

impl CompileError {
    pub fn new(kind: CompileErrorKind, span: Span) -> Self {
        CompileError { kind, span, source: SourceId::UNKNOWN }
    }
}

#[derive(Debug)]
pub enum CompileErrorKind {
    UndeclaredVariable(String),
    UndeclaredAssign(String),
    DuplicateDeclaration(String),
    AssignToBuiltin(String),
    TooManyConstants,
    TooManyLocals,
    TooManyUpvalues,
    JumpTooFar,
    BreakOutsideLoop,
    SpreadInInvalidPosition,
    InvalidMatchPattern(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            CompileErrorKind::UndeclaredVariable(n) => {
                write!(f, "undeclared variable `{}`", n)
            }
            CompileErrorKind::UndeclaredAssign(n) => {
                write!(f, "assignment to undeclared variable `{}` (use `:=` to declare)", n)
            }
            CompileErrorKind::DuplicateDeclaration(n) => {
                write!(f, "`{}` is already declared in this scope", n)
            }
            CompileErrorKind::AssignToBuiltin(n) => {
                write!(f, "cannot assign to built-in `{}` (use `:=` to shadow)", n)
            }
            CompileErrorKind::TooManyConstants => f.write_str("too many constants in chunk"),
            CompileErrorKind::TooManyLocals => f.write_str("too many local variables"),
            CompileErrorKind::TooManyUpvalues => f.write_str("too many captured variables"),
            CompileErrorKind::JumpTooFar => f.write_str("jump distance exceeds 64KiB"),
            CompileErrorKind::BreakOutsideLoop => f.write_str("`break` outside of any loop"),
            CompileErrorKind::SpreadInInvalidPosition => f.write_str(
                "spread `...` is only allowed in array literals, call args, or object literals"
            ),
            CompileErrorKind::InvalidMatchPattern(msg) => {
                write!(f, "invalid match pattern: {}", msg)
            }
        }
    }
}

// ---------------- Runtime ----------------

#[derive(Debug)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub line: u32,
    pub source: SourceId,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, line: u32) -> Self {
        RuntimeError { kind, line, source: SourceId::UNKNOWN }
    }
}

#[derive(Debug)]
pub enum RuntimeErrorKind {
    TypeMismatch(String),
    DivisionByZero,
    StackUnderflow,
    ArityMismatch { name: String, expected: String, got: usize },
    NotCallable(String),
    IndexOutOfBounds(i64),
    InvalidIndexType(String),
    ImmutableTarget(String),
    ImportFailed(String, String),
    /// User-raised error (`raise expr`) or a builtin/runtime error that
    /// has been caught and re-thrown as a string. The contained value
    /// is what catch handlers will see and what `Display` prints for an
    /// uncaught raise. Distinguished from other variants so we don't
    /// add a `runtime error:` prefix when stringifying.
    Raised(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            RuntimeErrorKind::TypeMismatch(s) => write!(f, "type mismatch: {}", s),
            RuntimeErrorKind::DivisionByZero => f.write_str("division by zero"),
            RuntimeErrorKind::StackUnderflow => f.write_str("internal: stack underflow"),
            RuntimeErrorKind::ArityMismatch { name, expected, got } => write!(
                f, "{name} expects {expected} arguments; got {got}"
            ),
            RuntimeErrorKind::NotCallable(t) => write!(f, "value of type {t} is not callable"),
            RuntimeErrorKind::IndexOutOfBounds(i) => write!(f, "index {i} out of bounds"),
            RuntimeErrorKind::InvalidIndexType(t) => write!(f, "cannot index with {t}"),
            RuntimeErrorKind::ImmutableTarget(t) => write!(f, "{t} is immutable"),
            RuntimeErrorKind::ImportFailed(path, msg) => write!(
                f, "import of {path:?} failed: {msg}"
            ),
            RuntimeErrorKind::Raised(s) => f.write_str(s),
        }
    }
}

// ---------------- Top-level error ----------------

#[derive(Debug)]
pub enum Error {
    Lex(LexError),
    Parse(ParseError),
    Compile(CompileError),
    Runtime(RuntimeError),
}

impl Error {
    /// Source id this error refers to, if any. UNKNOWN when the error
    /// pre-dates source registration (synthetic / test input).
    pub fn source(&self) -> SourceId {
        match self {
            Error::Lex(e) => e.source,
            Error::Parse(e) => e.source,
            Error::Compile(e) => e.source,
            Error::Runtime(e) => e.source,
        }
    }

    /// Stamp the source id on this error if it doesn't already have
    /// one. Idempotent — won't overwrite a real id with another.
    #[allow(dead_code)]
    pub fn stamp_source(&mut self, id: SourceId) {
        if !self.source().is_unknown() {
            return;
        }
        match self {
            Error::Lex(e) => e.source = id,
            Error::Parse(e) => e.source = id,
            Error::Compile(e) => e.source = id,
            Error::Runtime(e) => e.source = id,
        }
    }

    /// Render with a source snippet, caret/underline, and filename
    /// when the source is registered in the supplied [`SourceMap`].
    /// Falls back to the legacy single-line `Display` form when the
    /// source is unknown.
    pub fn render(&self, sources: &SourceMap) -> String {
        let (label, line, col_start, span_len, body) = match self {
            Error::Lex(e) => (
                "lex",
                e.span.line,
                Some(e.span.start),
                Some(e.span.end.saturating_sub(e.span.start).max(1)),
                format!("{e}"),
            ),
            Error::Parse(e) => (
                "parse",
                e.span.line,
                Some(e.span.start),
                Some(e.span.end.saturating_sub(e.span.start).max(1)),
                format!("{e}"),
            ),
            Error::Compile(e) => (
                "compile",
                e.span.line,
                Some(e.span.start),
                Some(e.span.end.saturating_sub(e.span.start).max(1)),
                format!("{e}"),
            ),
            Error::Runtime(e) => (
                "runtime",
                e.line,
                None,
                None,
                format!("{e}"),
            ),
        };
        let Some(file) = sources.get(self.source()) else {
            return format!("{self}");
        };
        render_snippet(label, &file.name, &file.text, line, col_start, span_len, &body)
    }
}

fn render_snippet(
    label: &str,
    filename: &str,
    source: &str,
    line: u32,
    span_start: Option<usize>,
    span_len: Option<usize>,
    body: &str,
) -> String {
    let (line_text, line_byte_start) = nth_line(source, line);
    let col = span_start
        .and_then(|s| s.checked_sub(line_byte_start))
        .map(|n| n + 1);
    let header_loc = match col {
        Some(c) => format!("{filename}:{line}:{c}"),
        None => format!("{filename}:{line}"),
    };
    let gutter_width = line.to_string().len().max(1);
    let pad = " ".repeat(gutter_width);
    let mut out = String::new();
    out.push_str(&format!("error[{label}]: {body}\n"));
    out.push_str(&format!("{pad}--> {header_loc}\n"));
    out.push_str(&format!("{pad} |\n"));
    out.push_str(&format!("{line:>w$} | {line_text}\n", w = gutter_width));
    // Caret line. For runtime errors with no span, omit it (no false precision).
    if let (Some(c), Some(len)) = (col, span_len) {
        let caret_pad = " ".repeat(c.saturating_sub(1));
        let carets = "^".repeat(len.max(1));
        out.push_str(&format!("{pad} | {caret_pad}{carets}\n"));
    } else {
        out.push_str(&format!("{pad} |\n"));
    }
    out
}

/// Return the text of the n-th 1-based line (without the trailing
/// newline) and its byte offset in `source`. If the requested line is
/// beyond the source, returns an empty line at the source's end.
fn nth_line(source: &str, line: u32) -> (&str, usize) {
    if line == 0 {
        return ("", 0);
    }
    let mut start = 0;
    let mut current = 1u32;
    for (i, b) in source.bytes().enumerate() {
        if current == line {
            // find end of this line
            let end = source[i..]
                .find('\n')
                .map(|n| i + n)
                .unwrap_or(source.len());
            return (&source[i..end], i);
        }
        if b == b'\n' {
            current += 1;
            start = i + 1;
        }
    }
    if current == line {
        let end = source[start..]
            .find('\n')
            .map(|n| start + n)
            .unwrap_or(source.len());
        (&source[start..end], start)
    } else {
        ("", source.len())
    }
}

impl From<LexError> for Error {
    fn from(e: LexError) -> Self { Error::Lex(e) }
}
impl From<ParseError> for Error {
    fn from(e: ParseError) -> Self { Error::Parse(e) }
}
impl From<CompileError> for Error {
    fn from(e: CompileError) -> Self { Error::Compile(e) }
}
impl From<RuntimeError> for Error {
    fn from(e: RuntimeError) -> Self { Error::Runtime(e) }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Lex(e) => write!(f, "lex error (line {}): {}", e.span.line, e),
            Error::Parse(e) => write!(f, "parse error (line {}): {}", e.span.line, e),
            Error::Compile(e) => write!(f, "compile error (line {}): {}", e.span.line, e),
            Error::Runtime(e) => write!(f, "runtime error (line {}): {}", e.line, e),
        }
    }
}
