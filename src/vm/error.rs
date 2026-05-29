//! Compile-time and runtime errors.

use std::fmt;

use crate::vm::source_map::{SourceId, SourceMap};
use crate::vm::token::{Span, Token};
use crate::vm::value::Value;

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
    /// Two expressions ran together with no `;` between them. The span
    /// points at the end of the first expression, where the separator
    /// belongs, rather than at the next expression's first token.
    MissingSemicolon { found: Token },
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
            ParseErrorKind::MissingSemicolon { found } => write!(
                f,
                "missing `;` before `{found}` — did you forget a semicolon?"
            ),
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
    ContinueOutsideLoop,
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
            CompileErrorKind::ContinueOutsideLoop => {
                f.write_str("`continue` outside of any loop")
            }
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

/// One entry in an uncaught error's stack trace: the function that was
/// executing and the source line it was at. Built innermost-first as
/// `try_catch` unwinds frames (see `vm.rs`).
#[derive(Debug, Clone)]
pub struct TraceFrame {
    /// Function name (inferred from the binding), `None` for an unbound
    /// `fn` — rendered as `<anonymous>`.
    pub name: Option<String>,
    pub source: SourceId,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub line: u32,
    pub source: SourceId,
    /// Call-frame stack captured while unwinding an uncaught error,
    /// innermost frame first. Empty until `try_catch` records it; stays
    /// empty for caught errors (the partial trace is simply discarded).
    pub trace: Vec<TraceFrame>,
    /// When set, the renderer prints this snippet instead of the default
    /// `--> file:line` rendering for `self.source`/`self.line`. Used for
    /// `ImportFailed` carrying a compile error in the imported file so
    /// the primary location points at that file, not the import call.
    /// The `trace` is still appended as "imported from:" lines.
    pub rendered: Option<String>,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, line: u32) -> Self {
        RuntimeError {
            kind,
            line,
            source: SourceId::UNKNOWN,
            trace: Vec::new(),
            rendered: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeErrorKind {
    TypeMismatch(String),
    DivisionByZero,
    StackUnderflow,
    ArityMismatch { name: String, expected: String, got: usize },
    NotCallable(String),
    IndexOutOfBounds(i64),
    InvalidIndexType(String),
    /// A `Map`/`Set` key (or `m[k]` index) had an un-hashable type.
    InvalidKeyType(String),
    ImmutableTarget(String),
    ImportFailed(String, String),
    /// Integer arithmetic (`+ - *`, unary `-`) overflowed `i64`.
    Overflow,
    /// Recursion exceeded the VM's configured maximum call depth.
    StackOverflow,
    /// `JSON.stringify` hit a circular reference in the value graph.
    Cycle,
    /// A `match` expression fell through every arm without matching.
    NoMatch,
    /// A value could not cross an actor/channel boundary: it cannot be
    /// deep-copied into another heap. Carries a human description of
    /// the offending value (e.g. a function with live captures, an
    /// iterator, or a native function). v0.14.
    NotSendable(String),
    /// A `send` was attempted on a closed channel. v0.14.
    ChannelClosed,
    /// A value raised by `raise expr`, stored verbatim — never coerced
    /// to a string. `catch` binds exactly this value; an uncaught
    /// raise renders it via `str()` (the `Value` `Display` form).
    /// Built-in error variants are never `Raised`; when caught, those
    /// are reified by the VM into a `${kind, message, line}` object.
    Raised(Value),
}

impl RuntimeErrorKind {
    /// Stable snake-case tag for the `kind` field of the
    /// `${kind, message, line}` object a caught built-in error is
    /// reified into. `Raised` is never reified, so its tag is unused.
    pub fn kind_tag(&self) -> &'static str {
        match self {
            RuntimeErrorKind::TypeMismatch(_) => "type_mismatch",
            RuntimeErrorKind::DivisionByZero => "div_by_zero",
            RuntimeErrorKind::StackUnderflow => "stack_underflow",
            RuntimeErrorKind::ArityMismatch { .. } => "arity_mismatch",
            RuntimeErrorKind::NotCallable(_) => "not_callable",
            RuntimeErrorKind::IndexOutOfBounds(_) => "index_out_of_bounds",
            RuntimeErrorKind::InvalidIndexType(_) => "invalid_index_type",
            RuntimeErrorKind::InvalidKeyType(_) => "invalid_key_type",
            RuntimeErrorKind::ImmutableTarget(_) => "immutable_target",
            RuntimeErrorKind::ImportFailed(..) => "import_failed",
            RuntimeErrorKind::Overflow => "overflow",
            RuntimeErrorKind::StackOverflow => "stack_overflow",
            RuntimeErrorKind::Cycle => "cycle",
            RuntimeErrorKind::NoMatch => "no_match",
            RuntimeErrorKind::NotSendable(_) => "not_sendable",
            RuntimeErrorKind::ChannelClosed => "channel_closed",
            RuntimeErrorKind::Raised(_) => "raised",
        }
    }
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
            RuntimeErrorKind::InvalidKeyType(t) => write!(
                f, "invalid key type: {t} (Map/Set keys must be null, bool, int, or string)"
            ),
            RuntimeErrorKind::ImmutableTarget(t) => write!(f, "{t} is immutable"),
            RuntimeErrorKind::ImportFailed(path, msg) => write!(
                f, "import of {path:?} failed: {msg}"
            ),
            RuntimeErrorKind::Overflow => f.write_str("integer overflow"),
            RuntimeErrorKind::StackOverflow => f.write_str("call stack depth exceeded"),
            RuntimeErrorKind::Cycle => f.write_str("circular reference"),
            RuntimeErrorKind::NoMatch => f.write_str("no matching arm in match expression"),
            RuntimeErrorKind::NotSendable(t) => write!(
                f, "{t} cannot be sent across actors"
            ),
            RuntimeErrorKind::ChannelClosed => {
                f.write_str("send on a closed channel")
            }
            RuntimeErrorKind::Raised(v) => write!(f, "{v}"),
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
        // ImportFailed pre-renders the inner compile error against the
        // imported file's source so its primary `--> file:line` points
        // at the actual error site, not the import call. Append the
        // import chain as "imported from:" lines (from `e.trace`, which
        // try_catch populates with each Import frame as it unwinds).
        if let Error::Runtime(e) = self {
            if let Some(rendered) = &e.rendered {
                let mut out = rendered.clone();
                if !e.trace.is_empty() {
                    out.push_str("imported from:\n");
                    for tf in &e.trace {
                        let loc = sources
                            .get(tf.source)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<unknown>");
                        out.push_str(&format!("  {loc}:{}\n", tf.line));
                    }
                }
                return out;
            }
        }
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
        let mut out =
            render_snippet(label, &file.name, &file.text, line, col_start, span_len, &body);
        // Stack trace beneath the snippet for uncaught runtime errors.
        // Skipped when there is a single frame — that would just repeat
        // the snippet location above.
        if let Error::Runtime(e) = self {
            if e.trace.len() > 1 {
                out.push_str("stack trace (most recent call first):\n");
                for tf in &e.trace {
                    let fname = tf.name.as_deref().unwrap_or("<anonymous>");
                    let loc = sources
                        .get(tf.source)
                        .map(|f| f.name.as_str())
                        .unwrap_or("<unknown>");
                    out.push_str(&format!("  {fname} at {loc}:{}\n", tf.line));
                }
            }
        }
        out
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
