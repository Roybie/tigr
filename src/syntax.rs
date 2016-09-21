#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Token <'a> {
    String(&'a str),
    Id(&'a str),

    Integer(&'a str),
    Float(&'a str),
    Bool(&'a str),
    Null,

    KeyFor,
    KeyWhile,
    KeyIf,
    KeyElse,

    // Brackets
    OpLparen,
    OpRparen,
    OpLbrace,
    OpRbrace,
    OpLbrack,
    OpRbrack,

    // Arithmetic
    OpMinus,
    OpPlus,
    OpDivide,
    OpMult,
    OpMod,
    OpPower,
    OpLand,
    OpLor,
    OpMinusEq,
    OpPlusEq,
    OpDivideEq,
    OpMultEq,
    OpModEq,

    // Assignment
    OpEqual,

    // Equality
    OpEquiv,
    OpNotEquiv,
    OpGreater,
    OpLess,
    OpGreatEqual,
    OpLessEqual,

    OpAnd,
    OpOr,

    // Misc
    OpNot,
    OpComma,
    OpColon,
    OpSemicolon,
    OpDot,
    OpRange,
    OpLength,
}

pub const KEYWORDS : [(&'static str, Token<'static>); 5] = [
    ("null",    Token::Null),
    ("for",     Token::KeyFor),
    ("while",   Token::KeyWhile),
    ("if",      Token::KeyIf),
    ("else",    Token::KeyElse),
];

pub const OPERATORS : [(&'static str, Token<'static>); 24] = [
    ("-",        Token::OpMinus),
    ("+",        Token::OpPlus),
    ("/",        Token::OpDivide),
    ("*",        Token::OpMult),
    ("%",        Token::OpMod),
    ("^",        Token::OpPower),
    ("&",        Token::OpLand),
    ("|",        Token::OpLor),
    ("-=",       Token::OpMinusEq),
    ("+=",       Token::OpPlusEq),
    ("/=",       Token::OpDivideEq),
    ("*=",       Token::OpMultEq),
    ("%=",       Token::OpModEq),
    ("=",        Token::OpEqual),
    ("==",       Token::OpEquiv),
    ("!",        Token::OpNot),
    ("!=",       Token::OpNotEquiv),
    (">",        Token::OpGreater),
    ("<",        Token::OpLess),
    (">=",       Token::OpGreatEqual),
    ("<=",       Token::OpLessEqual),
    ("&&",       Token::OpAnd),
    ("||",       Token::OpOr),
    ("#",        Token::OpLength),
];

pub const TOKENS : [(&'static str,Token<'static>); 11] = [
    ("(",        Token::OpLparen),
    (")",        Token::OpRparen),
    ("{",        Token::OpLbrace),
    ("}",        Token::OpRbrace),
    ("[",        Token::OpLbrack),
    ("]",        Token::OpRbrack),
    (",",        Token::OpComma),
    (":",        Token::OpColon),
    (";",        Token::OpSemicolon),
    (".",        Token::OpDot),
    ("..",       Token::OpRange),
];
