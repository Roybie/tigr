package token

type Token int

const (
    token_start Token = iota

    EOF
    ILLEGAL

    literal_start

    NUMBER
    STRING
    BOOL
    IDENT

    literal_end

    key_start

    VAR
    CONST
    FOR
    RANGE
    WHILE
    IF
    ELSE
    BREAK
    CONTINUE
    RETURN
    FUNC
    IMPORT
    GO

    key_end

    op_start

    LPAREN
    RPAREN
    LBRACE
    RBRACE
    LBRACK
    RBRACK

    COMMA
    COLON
    SEMICOLON
    FULLSTOP

    ADD
    SUB
    MUL
    DIV
    MOD
    POW

    BITAND
    BITOR
    NOT

    ASSIGN
    ADDASSIGN
    SUBASSIGN
    MULASSIGN
    DIVASSIGN
    MODASSIGN
    POWASSIGN

    EQUAL
    NOTEQUAL
    GTHAN
    LTHAN
    GEQUAL
    LEQUAL

    AND
    OR

    LENGTH
    SPREAD

    op_end

    token_end
)

var strings = map[Token]string {
    EOF:        "EOF",
    ILLEGAL:    "Illegal",
    NUMBER:     "Number",
    STRING:     "String",
    BOOL:       "Boolean",
    IDENT:      "Identifier",
    VAR         "var",
    CONST:      "const",
    FOR:        "for",
    RANGE:      "range",
    WHILE:      "while",
    IF:         "if",
    ELSE:       "else",
    BREAK:      "break",
    CONTINUE:   "continue",
    RETURN      "return",
    FUNC        "fn",
    IMPORT      "import",
    GO          "go",
    LPAREN      "(",
    RPAREN      ")",
    LBRACE      "{",
    RBRACE      "}",
    LBRACK      "[",
    RBRACK      "]",
    COMMA       ",",
    COLON       ":",
    SEMICOLON   ";",
    FULLSTOP    ".",
    ADD         "+",
    SUB         "-",
    MUL         "*",
    DIV         "/",
    MOD         "%",
    POW         "^",
    BITAND      "&",
    BITOR       "|",
    NOT         "!",
    ASSIGN      "=",
    ADDASSIGN   "+=",
    SUBASSIGN   "-=",
    MULASSIGN   "*=",
    DIVASSIGN   "/=",
    MODASSIGN   "%=",
    POWASSIGN   "^=",
    EQUAL       "==",
    NOTEQUAL    "!=",
    GTHAN       ">",
    LTHAN       "<",
    GEQUAL      ">=",
    LEQUAL      "<=",
    AND         "&&",
    OR          "||",
    LENGTH      "#",
    SPREAD      "..",
}

func (t Token) IsLiteral() bool {
    return t > literal_start && t < literal_end
}

func (t Token) IsOperator() bool {
    return t > op_start && t < op_end
}

func (t Token) IsKeyword() bool {
    return t > key_start && t < key_end
}

func (t Token) String() string {
    return strings[t]
}

func (t Token) Valid() bool {
    return t > token_start && t < token_end
}

func Lookup(str string) Token {
    if str == "true" || str == "false" {
        return BOOL
    }
    for t, s := range strings {
        if s == str {
            return t
        }
    }
    return IDENT
}
