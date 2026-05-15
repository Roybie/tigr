//! Compile-time and runtime errors.

use std::fmt;

use crate::vm::token::{Span, Token};

// ---------------- Lex ----------------

#[derive(Debug)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

impl LexError {
    pub fn new(kind: LexErrorKind, span: Span) -> Self {
        LexError { kind, span }
    }
}

#[derive(Debug)]
pub enum LexErrorKind {
    InvalidChar(char),
    UnterminatedString,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LexErrorKind::InvalidChar(c) => write!(f, "unexpected character '{}'", c),
            LexErrorKind::UnterminatedString => f.write_str("unterminated string literal"),
        }
    }
}

// ---------------- Parse ----------------

#[derive(Debug)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

impl ParseError {
    pub fn new(kind: ParseErrorKind, span: Span) -> Self {
        ParseError { kind, span }
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
}

impl CompileError {
    pub fn new(kind: CompileErrorKind, span: Span) -> Self {
        CompileError { kind, span }
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
        }
    }
}

// ---------------- Runtime ----------------

#[derive(Debug)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub line: u32,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, line: u32) -> Self {
        RuntimeError { kind, line }
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
