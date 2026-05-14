//! Tokens emitted by the lexer.
//!
//! The full v0.2 token set is defined here even though earlier phases
//! only emit a subset. Adding a new variant later is cheap.

use std::fmt;

/// A byte-offset span into the source. Line/column can be recovered
/// lazily for error messages by walking the source up to `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32) -> Self {
        Span { start, end, line }
    }

    pub fn join(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),

    // Keywords
    Null,
    True,
    False,
    Fn,
    If,
    Else,
    For,
    While,
    Break,
    Return,
    Import,

    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,

    // Comparison
    EqEq,
    BangEq,
    Lt,
    Gt,
    LtEq,
    GtEq,

    // Logical
    AmpAmp,
    PipePipe,
    Bang,

    // Assignment
    Eq,
    ColonEq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,

    // Pipe
    PipeGt,

    // Range
    DotDot,
    DotDotEq,

    // Spread
    Ellipsis,

    // Misc
    Hash,
    Dot,
    Comma,
    Colon,
    Semicolon,
    Dollar,

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBrack,
    RBrack,

    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Token::*;
        match self {
            Int(n) => write!(f, "{n}"),
            Float(x) => write!(f, "{x}"),
            Str(s) => write!(f, "'{s}'"),
            Ident(s) => write!(f, "{s}"),
            Null => f.write_str("null"),
            True => f.write_str("true"),
            False => f.write_str("false"),
            Fn => f.write_str("fn"),
            If => f.write_str("if"),
            Else => f.write_str("else"),
            For => f.write_str("for"),
            While => f.write_str("while"),
            Break => f.write_str("break"),
            Return => f.write_str("return"),
            Import => f.write_str("import"),
            Plus => f.write_str("+"),
            Minus => f.write_str("-"),
            Star => f.write_str("*"),
            Slash => f.write_str("/"),
            Percent => f.write_str("%"),
            Caret => f.write_str("^"),
            EqEq => f.write_str("=="),
            BangEq => f.write_str("!="),
            Lt => f.write_str("<"),
            Gt => f.write_str(">"),
            LtEq => f.write_str("<="),
            GtEq => f.write_str(">="),
            AmpAmp => f.write_str("&&"),
            PipePipe => f.write_str("||"),
            Bang => f.write_str("!"),
            Eq => f.write_str("="),
            ColonEq => f.write_str(":="),
            PlusEq => f.write_str("+="),
            MinusEq => f.write_str("-="),
            StarEq => f.write_str("*="),
            SlashEq => f.write_str("/="),
            PercentEq => f.write_str("%="),
            PipeGt => f.write_str("|>"),
            DotDot => f.write_str(".."),
            DotDotEq => f.write_str("..="),
            Ellipsis => f.write_str("..."),
            Hash => f.write_str("#"),
            Dot => f.write_str("."),
            Comma => f.write_str(","),
            Colon => f.write_str(":"),
            Semicolon => f.write_str(";"),
            Dollar => f.write_str("$"),
            LParen => f.write_str("("),
            RParen => f.write_str(")"),
            LBrace => f.write_str("{"),
            RBrace => f.write_str("}"),
            LBrack => f.write_str("["),
            RBrack => f.write_str("]"),
            Eof => f.write_str("<eof>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

impl SpannedToken {
    pub fn new(token: Token, span: Span) -> Self {
        SpannedToken { token, span }
    }
}
