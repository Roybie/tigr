#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Token <'a> {
    String(&'a str),
    Id(&'a str),

    Integer(&'a str),
    Float(&'a str),
    Bool(&'a str),
    Null,

    KeyFor,
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
}

pub const KEYWORDS : [(&'static str, Token<'static>); 4] = [
    ("null",    Token::Null),
    ("for",     Token::KeyFor),
    ("if",      Token::KeyIf),
    ("else",    Token::KeyElse),
];

pub const OPERATORS : [(&'static str,Token<'static>); 29] = [
    ("(",        Token::OpLparen),
    (")",        Token::OpRparen),
    ("{",        Token::OpLbrace),
    ("}",        Token::OpRbrace),
    ("[",        Token::OpLbrack),
    ("]",        Token::OpRbrack),
    ("-",        Token::OpMinus),
    ("+",        Token::OpPlus),
    ("/",        Token::OpDivide),
    ("*",        Token::OpMult),
    ("%",        Token::OpMod),
    ("^",        Token::OpPower),
    ("&",        Token::OpLand),
    ("|",        Token::OpLor),
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
    (",",        Token::OpComma),
    (":",        Token::OpColon),
    (";",        Token::OpSemicolon),
    (".",        Token::OpDot),
    ("..",       Token::OpRange),
];
