#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Token <'a> {
    String(&'a str),
    Id(&'a str),

    Integer(&'a str),
    Float(&'a str),
    Bool(&'a str),
    Null,

    KeyFor,
    KeyForA,
    KeyWhile,
    KeyWhileA,
    KeyIf,
    KeyElse,
    KeyBreak,

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

    //lexing error
    OpUnexpected(char),

    //Ignored by lexer
    IgnoreComment,
    IgnoreWhitespace,

}

pub const KEYWORDS : [(&'static str, Token<'static>); 8] = [
    ("null",    Token::Null),
    ("for",     Token::KeyFor),
    ("for[]",   Token::KeyForA),
    ("while",   Token::KeyWhile),
    ("while[]", Token::KeyWhileA),
    ("if",      Token::KeyIf),
    ("else",    Token::KeyElse),
    ("break",   Token::KeyBreak),
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
