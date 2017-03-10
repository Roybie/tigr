package lexer

import (
    "unicode"
    "github.com/roybie/tigr/token"
)

type Lexer struct {
    char    rune
    offset  int
    roffset int
    source  string
    file    *token.File
}

func (l *Lexer) Init(file *token.File, src string) {
    l.file = file
    l.offset, l.roffset = 0, 0
    l.source = src
    l.file.AddLine(l.offset)

    l.next()
}

func (l *Lexer) Scan() (string, token.Token, token.Pos) {
    l.skipWhitespace()

    if unicode.IsLetter(l.char) {
        return l.scanIdentifier()
    }

    if unicode.IsDigit(l.char) {
        return l.scanNumber()
    }

    if l.char == '"' {
        return l.scanString()
    }

    var tok token.Token
    char := l.char
    lit, pos := string(l.char), l.file.Pos(l.offset)
    l.next()

    //TODO make this nicer than a switch/case for each op
    switch char {
        //LPAREN
        case '(':
            tok = token.LPAREN
        //RPAREN
        case ')':
            tok = token.RPAREN
        //LBRACE
        case '{':
            tok = token.LBRACE
        //RBRACE
        case '}':
            tok = token.RBRACE
        //LBRACK
        case '[':
            tok = token.LBRACK
        //RBRACK
        case ']':
            tok = token.RBRACK
        //COMMA
        case ',':
            tok = token.COMMA
        //COLON
        case ':':
            tok = l.multiCharOp('=', token.DECLARE, token.COLON)
        //SEMICOLON
        case ';':
            tok = token.SEMICOLON
        //FULLSTOP
        case '.':
            tok = l.multiCharOp('.', token.SPREAD, token.FULLSTOP)
        //ADD
        case '+':
            tok = l.multiCharOp('=', token.ADDASSIGN, token.ADD)
        //SUB
        case '-':
            tok = l.multiCharOp('=', token.SUBASSIGN, token.SUB)
        //MUL
        case '*':
            tok = l.multiCharOp('=', token.MULASSIGN, token.MUL)
        //DIV
        case '/':
            //special case for comments
            if l.char == '/' {
                l.skipComment()
                l.next()
                return l.Scan()
            }
            tok = l.multiCharOp('=', token.DIVASSIGN, token.DIV)
        //MOD
        case '%':
            tok = l.multiCharOp('=', token.MODASSIGN, token.MOD)
        //POW
        case '^':
            tok = token.BITXOR
        //BITAND
        case '&':
            tok = l.multiCharOp('&', token.AND, token.BITAND)
        //BITOR
        case '|':
            tok = l.multiCharOp('|', token.OR, token.BITOR)
        //NOT
        case '!':
            tok = l.multiCharOp('=', token.NOTEQUAL, token.NOT)
        //ASSIGN
        case '=':
            tok = l.multiCharOp('=', token.EQUAL, token.ASSIGN)
        //GTHAN
        case '>':
            tok = l.multiCharOp('=', token.GEQUAL, token.GTHAN)
        //LTHAN
        case '<':
            tok = l.multiCharOp('=', token.LEQUAL, token.LTHAN)
        //LENGTH
        case '#':
            tok = token.LENGTH
        default:
            if l.offset >= len(l.source)-1 {
                tok = token.EOF
            } else {
                tok = token.ILLEGAL
            }
    }

    return lit, tok, pos
}

func (l *Lexer) next() {
    l.char = rune(0)
    if l.roffset < len(l.source) {
        l.offset = l.roffset
        l.char = rune(l.source[l.offset])
        if l.char == '\n' {
            l.file.AddLine(l.offset)
        }
        l.roffset++
    }
}

func (l *Lexer) peek() rune {
    if l.roffset < len(l.source) {
        return rune(l.source[l.roffset])
    }
    return rune(0)
}

func (l *Lexer) scanString() (string, token.Token, token.Pos) {
    start := l.offset

    for {
        if l.peek() == '"' {
            if l.char != '\\' {
                break
            }
        }
        l.next()
    }
    l.next()
    offset := l.offset
    if l.char == rune(0) {
        offset++
    }

    return l.source[start:offset], token.STRING, l.file.Pos(start)
}

func (l *Lexer) scanNumber() (string, token.Token, token.Pos) {
    start := l.offset

    for unicode.IsDigit(l.char) {
        l.next()
    }

    if l.char == '.' {
        if unicode.IsDigit(l.peek()) {
            l.next()
            for unicode.IsDigit(l.char) {
                l.next()
            }
        }
    }

    offset := l.offset
    if l.char == rune(0) {
        offset++
    }

    return l.source[start:offset], token.NUMBER, l.file.Pos(start)
}

func (l *Lexer) scanIdentifier() (string, token.Token, token.Pos) {
    start := l.offset

    for unicode.IsLetter(l.char) || unicode.IsDigit(l.char) {
        l.next()
    }

    offset := l.offset
    if l.char == rune(0) {
        offset++
    }
    lit := l.source[start:offset]

    return lit, token.Lookup(lit), l.file.Pos(start)
}

func (l *Lexer) multiCharOp(r rune, a, b token.Token) token.Token {
    if l.char == r {
        l.next()
        return a
    }
    return b
}

func (l *Lexer) skipComment() {
    for l.char != '\n' && l.offset < len(l.source) - 1 {
        l.next()
    }
}

func (l *Lexer) skipWhitespace() {
    for unicode.IsSpace(l.char) {
        l.next()
    }
}
