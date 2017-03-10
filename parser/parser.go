package parser

import (
    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/lexer"
    "github.com/roybie/tigr/token"
)

func ParseExpression(name, src string) (ast.Expr, error) {
    var p parser

    fset := token.NewFileSet()
    file := fset.Add(name, src)

    p.init(file, name, string(src), nil)
    node := p.ParseExpr()

    if p.errors.Count() > 0{
        return nil, p.errors
    }
    return node, nil
}

type parser struct {
    file *token.File
    errors token.ErrorList
    lexer lexer.Lexer

    currentScope *ast.Scope

    pos token.Pos
    tok token.Token
    lit string
}

func (p *parser) addError(args ...interface{}) {
    p.errors.Add(p.file.Position(p.pos), args...)
}

func (p *parser) expect(tok token.Token) token.Pos {
    pos := p.pos
    if !p.accept(tok) {
        p.addError("Expected '" + tok.String() + "' got '" + p.lit + "'")
    }
    return pos
}

func (p *parser) accept(tokens ...token.Token) bool {
    for _, t := range tokens {
        if p.tok == t {
            p.next()
            return true
        }
    }
    return false
}

func (p *parser) init(file *token.File, fname, src string, s *ast.Scope) {
    if s == nil {
        s = ast.NewScope(nil)
    }
    p.file = file
    p.lexer.Init(p.file, src)
    p.currentScope = s
    p.next()
}

func (p *parser) next() {
    p.lit, p.tok, p.pos = p.lexer.Scan()
}

func (p *parser) openScope() {
    p.currentScope = ast.NewScope(p.currentScope)
}

func (p *parser) closeScope() {
    p.currentScope = p.currentScope.Parent
}

func (p *parser) ParseExpr() ast.Expr {
    //return ParseAssignExpr()
    var e ast.Expr
    e = p.ParseCompExpr()
    return e
}

func (p *parser) ParseAssignExpr() ast.Expr {
    if p.tok.IsAssign() {
        pos := p.pos
        op := p.tok
        p.next()

        return &ast.BinaryExpr{
            Op: op,
            Pos: pos,
        }
    }
    return p.ParseCompExpr()
}

func (p *parser) ParseCompExpr() ast.Expr {
    e := p.ParseBitExpr()
    pos := p.pos
    op := p.tok

    if p.accept(token.AND, token.OR) {
        return &ast.BinaryExpr{
            Op: op,
            Pos: pos,
            Lhs: e,
            Rhs: p.ParseCompExpr(),
        }
    }
    return e
}

func (p *parser) ParseBitExpr() ast.Expr {
    e := p.ParseEqualExpr()
    pos := p.pos
    op := p.tok

    if p.accept(token.BITAND, token.BITOR, token.BITXOR) {
        return &ast.BinaryExpr{
            Op: op,
            Pos: pos,
            Lhs: e,
            Rhs: p.ParseBitExpr(),
        }
    }
    return e
}

func (p *parser) ParseEqualExpr() ast.Expr {
    e := p.ParseSumExpr()
    pos := p.pos
    op := p.tok

    if p.accept(
        token.EQUAL,
        token.NOTEQUAL,
        token.LTHAN,
        token.GTHAN,
        token.LEQUAL,
        token.GEQUAL) {
        return &ast.BinaryExpr{
            Op: op,
            Pos: pos,
            Lhs: e,
            Rhs: p.ParseEqualExpr(),
        }
    }
    return e
}

func (p *parser) ParseSumExpr() ast.Expr {
    e := p.ParseProdExpr()
    pos := p.pos
    op := p.tok

    if p.accept(token.ADD, token.SUB) {
        return &ast.BinaryExpr{
            Op: op,
            Pos: pos,
            Lhs: e,
            Rhs: p.ParseSumExpr(),
        }
    }
    return e
}

func (p *parser) ParseProdExpr() ast.Expr {
    e := p.ParseUnaryExpr()
    pos := p.pos
    op := p.tok

    if p.accept(token.MUL, token.DIV, token.MOD) {
        return &ast.BinaryExpr{
            Op: op,
            Pos: pos,
            Lhs: e,
            Rhs: p.ParseProdExpr(),
        }
    }
    return e
}

func (p *parser) ParseUnaryExpr() ast.Expr {
    pos := p.pos
    tok := p.tok

    if p.accept(token.NOT, token.LENGTH, token.SUB, token.ADD) {
        return &ast.UnaryExpr{
            Pos: pos,
            Op: tok,
            Value: p.ParseAtomExpr(),
        }
    }
    return p.ParseAtomExpr()
}

func (p *parser) ParseAtomExpr() ast.Expr {
    pos, tok, lit := p.pos, p.tok, p.lit
    p.next()

    switch tok {
    case token.IDENT:
        return &ast.Ident{
            Pos: pos,
            Name: lit,
        }
    case token.NUMBER:
        return &ast.BasicLit{
            Pos: pos,
            Kind: tok,
            Lit: lit,
        }
    case token.LPAREN:
        defer p.expect(token.RPAREN)
        return p.ParseExprList()
    case token.LBRACE:
        defer p.expect(token.RBRACE)
        return p.ParseScope()
    default:
        p.addError("Unexpected '" + p.lit + "'")
        p.next()
    }
    return nil
}

func (p *parser) ParseExprList() ast.Expr {
    return p.ParseExpr()
}

func (p *parser) ParseScope() ast.Expr {
    pos := p.pos
    el := make([]ast.Expr, 0)

    el = append(el, p.ParseExpr())

    for p.accept(token.SEMICOLON) {
        el = append(el, p.ParseExpr())
    }

    return &ast.ScopeExpr{
        Pos: pos,
        List: el,
    }
}
