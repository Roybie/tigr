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

    type_start

    NUMTYPE
    INTTYPE
    FLOATTYPE
    STRTYPE
    BOOLTYPE
    FUNCTYPE
    ARRAYTYPE
    OBJECTTYPE
    ANYTYPE

    type_end

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
    DOLLAR

    ADD
    SUB
    MUL
    DIV
    MOD

    BITXOR
    BITAND
    BITOR
    NOT

    assign_start

    ASSIGN
    DECLARE
    ADDASSIGN
    SUBASSIGN
    MULASSIGN
    DIVASSIGN
    MODASSIGN

    assign_end

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

var tok_strings = map[Token]string {
    EOF:        "EOF",
    ILLEGAL:    "Illegal",
    NUMBER:     "Number",
    STRING:     "String",
    BOOL:       "Boolean",
    IDENT:      "Identifier",
    NUMTYPE:    "number",
    INTTYPE:    "int",
    FLOATTYPE:  "float",
    BOOLTYPE:   "boolean",
    STRTYPE:    "string",
    FUNCTYPE:   "function",
    ARRAYTYPE:  "array",
    OBJECTTYPE: "object",
    ANYTYPE:    "any",
    VAR:        "var",
    CONST:      "const",
    FOR:        "for",
    RANGE:      "range",
    WHILE:      "while",
    IF:         "if",
    ELSE:       "else",
    BREAK:      "break",
    CONTINUE:   "continue",
    RETURN:     "return",
    FUNC:       "fn",
    IMPORT:     "import",
    GO:         "go",
    LPAREN:     "(",
    RPAREN:     ")",
    LBRACE:     "{",
    RBRACE:     "}",
    LBRACK:     "[",
    RBRACK:     "]",
    COMMA:      ",",
    COLON:      ":",
    SEMICOLON:  ";",
    FULLSTOP:   ".",
    DOLLAR:     "$",
    ADD:        "+",
    SUB:        "-",
    MUL:        "*",
    DIV:        "/",
    MOD:        "%",
    BITXOR:     "^",
    BITAND:     "&",
    BITOR:      "|",
    NOT:        "!",
    ASSIGN:     "=",
    DECLARE:    ":=",
    ADDASSIGN:  "+=",
    SUBASSIGN:  "-=",
    MULASSIGN:  "*=",
    DIVASSIGN:  "/=",
    MODASSIGN:  "%=",
    EQUAL:      "==",
    NOTEQUAL:   "!=",
    GTHAN:      ">",
    LTHAN:      "<",
    GEQUAL:     ">=",
    LEQUAL:     "<=",
    AND:        "&&",
    OR:         "||",
    LENGTH:     "#",
    SPREAD:     "..",
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

func (t Token) IsAssign() bool {
    return t > assign_start && t < assign_end
}

func (t Token) IsType() bool {
    return t > type_start && t < type_end
}

func (t Token) String() string {
    return tok_strings[t]
}

func (t Token) Valid() bool {
    return t > token_start && t < token_end
}

func Lookup(str string) Token {
    if str == "true" || str == "false" {
        return BOOL
    }
    for t, s := range tok_strings {
        if s == str {
            return t
        }
    }
    return IDENT
}
