package parser

import (
    "fmt"
    "strings"
    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/lexer"
    "github.com/roybie/tigr/token"
)

func ParseExpression(name, src string) (ast.Expr, error) {
    var p parser

    fset := token.NewFileSet()
    file := fset.Add(name, src)

    p.init(file, name, string(src), nil)
    node := p.ParseFile()

    if p.errors.Count() > 0 {
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

func (p *parser) expect(tokens ...token.Token) token.Pos {
    pos := p.pos
    if !p.accept(tokens...) {
        toks := make([]string, 0)
        for _, t := range tokens {
            toks = append(toks, t.String())
        }
        exp := strings.Join(toks, " or ")
        p.addError("Expected '" + exp + "' got '" + p.lit + "'")
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

func (p *parser) ParseFile() ast.Expr {
    var e ast.Expr
    e = p.ParseExpr()
    if p.tok != token.EOF {
        p.addError("Unexpected " + p.tok.String() + ": " + p.lit)
    }
    return e
}

func (p *parser) ParseExpr() ast.Expr {
    return p.ParseAssignExpr()
}

func (p *parser) ParseAssignExpr() ast.Expr {
    e := p.ParseCompExpr()
    pos := p.pos
    op := p.tok

    id, idok := e.(*ast.Ident)
    id2, indok := e.(*ast.IndexedExpr)
    ind_is_id := false
    var id3 *ast.Ident
    if indok {
        id3, ind_is_id = id2.Item.(*ast.Ident)
    }
    if p.tok.IsAssign() {

        if (idok || ind_is_id) {
            if idok {
                if op == token.DECLARE {
                    prev := p.currentScope.Insert(&ast.Object{
                        Pos: pos,
                        Name: id.Name,
                    })

                    if prev != nil {
                        p.addError( "Variable '", prev.Name, "' already declared in current scope at ", p.file.Position(prev.Pos))
                    } else {
                        //SHADOWING warn or not?
                        prev = p.currentScope.Parent.Lookup(id.Name)
                        if prev != nil {
                            //p.addError( "Warning: Variable '", prev.Name, "' already declared in parent scope at ", p.file.Position(prev.Pos))
                        }
                    }
                } else {
                    if v := p.currentScope.Lookup(id.Name); v == nil {
                        p.addError("Cannot assign to undeclared variable '", id.Name, "'")
                    }
                }
            }
            if indok {
                if op != token.DECLARE {
                    if v := p.currentScope.Lookup(id3.Name); v == nil {
                        p.addError("Cannot assign to undeclared variable '", id3.Name, "'")
                    }
                }
            }
            p.next()

            return &ast.BinaryExpr{
                Op: op,
                Pos: pos,
                Lhs: e,
                Rhs: p.ParseAssignExpr(),
            }
        } else {
            p.addError(fmt.Sprintf("Invalid assignment to %s", ast.Print(e)))
        }
    }
    return e
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
    //lit := p.lit

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
    var e ast.Expr
    tok, lit := p.tok, p.lit

    switch tok {
    case token.IDENT:
        e = &ast.Ident{
            Pos: p.expect(token.IDENT),
            Name: lit,
        }
    case token.NUMBER:
        e = &ast.BasicLit{
            Pos: p.expect(token.NUMBER),
            Kind: tok,
            Lit: lit,
        }
    case token.STRING:
        e = &ast.BasicLit{
            Pos: p.expect(token.STRING),
            Kind: tok,
            Lit: lit,
        }
    case token.BOOL:
        e = &ast.BasicLit{
            Pos: p.expect(token.BOOL),
            Kind: tok,
            Lit: lit,
        }
    case token.LPAREN:
        p.next()
        e = p.ParseExpr()
        p.expect(token.RPAREN)
    case token.LBRACE:
        e = p.ParseScope()
    case token.IF:
        e = p.ParseIf()
    case token.FOR:
        e = p.ParseFor()
    case token.WHILE:
        e = p.ParseWhile()
    case token.BREAK:
        e = p.ParseBreakExpr()
    case token.RETURN:
        e = p.ParseReturnExpr()
    case token.IMPORT:
        e = p.ParseImportExpr()
    case token.LBRACK:
        e = p.ParseArrayDec()
    case token.DOLLAR:
        e = p.ParseObjectDec()
    case token.FUNC:
        e = p.ParseFunctionDec()
    default:
        p.addError("Unexpected " + tok.String() + ": " + lit)
    }
    //check for indexed
    for p.accept(token.LBRACK) {
        e = p.ParseIndexed(e)
    }
    for p.accept(token.FULLSTOP) {
        e = p.ParseIndexedObject(e)
    }
    //check for function call
    for p.accept(token.LPAREN) {
        e = p.ParseFunctionCall(e)
    }
    return e
}

func (p *parser) ParseExprList() []ast.Expr {
    el := make([]ast.Expr, 0)
    el = append(el, p.ParseExpr())

    for p.accept(token.SEMICOLON) {
        el = append(el, p.ParseExpr())
    }

    return el
}

func (p *parser) ParseScope() ast.Expr {
    p.openScope()
    defer p.closeScope()
    pos := p.expect(token.LBRACE)

    el := p.ParseExprList()

    p.expect(token.RBRACE)

    return &ast.ScopeExpr{
        Pos: pos,
        List: el,
    }
}

func (p *parser) ParseIf() ast.Expr {
    p.openScope()
    defer p.closeScope()
    pos := p.expect(token.IF)

    var els ast.Expr
    cond := p.ParseExprList()
    then := p.ParseScope()

    if p.accept(token.ELSE) {
        els = p.ParseScope()
    }

    return &ast.IfExpr{
        Pos: pos,
        Cond: cond,
        Then: then,
        Else: els,
    }
}

func (p *parser) ParseFor() ast.Expr {
    p.openScope()
    defer p.closeScope()
    pos := p.expect(token.FOR)

    arr := false

    if p.accept(token.LBRACK) {
        //for[]
        p.expect(token.RBRACK)
        arr = true
    }

    cond := p.ParseExprList()
    body := p.ParseScope()

    if arr {
        return &ast.ForAExpr{
            Pos: pos,
            Cond: cond,
            Body: body,
        }
    } else {
        return &ast.ForExpr{
            Pos: pos,
            Cond: cond,
            Body: body,
        }
    }
}

func (p *parser) ParseWhile() ast.Expr {
    p.openScope()
    defer p.closeScope()
    pos := p.expect(token.WHILE)

    arr := false

    if p.accept(token.LBRACK) {
        //for[]
        p.expect(token.RBRACK)
        arr = true
    }

    cond := p.ParseExprList()
    body := p.ParseScope()

    if arr {
        return &ast.WhileAExpr{
            Pos: pos,
            Cond: cond,
            Body: body,
        }
    } else {
        return &ast.WhileExpr{
            Pos: pos,
            Cond: cond,
            Body: body,
        }
    }
}

func (p *parser) ParseIndexed(e ast.Expr) ast.Expr {
    pos := p.pos

    defer p.expect(token.RBRACK)
    return &ast.IndexedExpr{
        Pos: pos,
        Item: e,
        Index: p.ParseExpr(),
    }

}

func (p *parser) ParseIndexedObject(e ast.Expr) ast.Expr {
    var ex ast.Expr
    pos, tok, lit := p.pos, p.tok, p.lit

    if tok != token.IDENT {
        p.addError("Invalid Object Index " + "'" + p.lit + "'")
    } else {
        p.next()
        ex = &ast.IndexedExpr{
            Pos: pos,
            Item: e,
            Index: &ast.BasicLit{
                Pos: pos,
                Kind: token.STRING,
                Lit: lit,
            },
        }
    }
    return ex
}

func (p *parser) ParseFunctionCall(e ast.Expr) ast.Expr {
    var args []ast.Expr
    pos := p.pos

    //Pos Callee Args
    args = p.ParseCallArgs()

    return &ast.CallExpr{
        Pos: pos,
        Callee: e,
        Args: args,
    }
}

func (p *parser) ParseCallArgs() []ast.Expr {

    el := make([]ast.Expr, 0)

    if p.accept(token.RPAREN) {
        return el
    }
    el = append(el, p.ParseExpr())

    for p.accept(token.COMMA) {
        el = append(el, p.ParseExpr())
    }

    p.expect(token.RPAREN)

    return el
}

func (p *parser) ParseBreakExpr() ast.Expr {
    return &ast.BreakExpr{
        Pos: p.expect(token.BREAK),
        Value: p.ParseExpr(),
    }
}

func (p *parser) ParseReturnExpr() ast.Expr {
    return &ast.ReturnExpr{
        Pos: p.expect(token.RETURN),
        Value: p.ParseExpr(),
    }
}

func (p *parser) ParseImportExpr() ast.Expr {
    return &ast.ImportExpr{
        Pos: p.expect(token.IMPORT),
        Import: p.ParseExpr(),
    }
}

func (p *parser) ParseArrayDec() ast.Expr {
    pos := p.expect(token.LBRACK)

    el := make([]ast.Expr, 0)

    if !p.accept(token.RBRACK) {
        el = append(el, p.ParseExpr())

        for p.accept(token.COMMA) {
            el = append(el, p.ParseExpr())
        }
        p.expect(token.RBRACK)
    }
    return &ast.ArrayExpr{
        Pos: pos,
        Elements: el,
    }
}

func (p *parser) ParseObjectDec() ast.Expr {
    pos := p.expect(token.DOLLAR)
    p.expect(token.LBRACE)
    el := make([]ast.Expr, 0)

    if !p.accept(token.RBRACE) {
        el = append(el, p.ParseObjectMemberExpr())

        for p.accept(token.COMMA) {
            el = append(el, p.ParseObjectMemberExpr())
        }
        p.expect(token.RBRACE)
    }
    return &ast.ObjectExpr{
        Pos: pos,
        Elements: el,
    }
}

func (p *parser) ParseObjectMemberExpr() ast.Expr {
    pos, tok, lit := p.pos, p.tok, p.lit

    if tok != token.IDENT {
        p.addError("Invalid Object Index " + "'" + lit + "'")
    }
    index := p.ParseExpr()
    p.expect(token.COLON)
    value := p.ParseExpr()
    return &ast.ObjectMemberExpr{
        Pos: pos,
        Index: index,
        Value: value,
    }
}

func (p *parser) ParseFunctionDec() ast.Expr {
    p.openScope()
    defer p.closeScope()
    pos := p.expect(token.FUNC)

    el := make([]ast.Expr, 0)

    p.expect(token.LPAREN)

    if !p.accept(token.RPAREN) {
        el = append(el, p.ParseExpr())
        id, idok := el[len(el) - 1].(*ast.Ident)
        if !idok {
            p.addError("Invalid Argument " + p.tok.String() + ": '" + p.lit + "'")
        } else {
            prev := p.currentScope.Insert(&ast.Object{
                Pos: pos,
                Name: id.Name,
            })

            if prev != nil {
                p.addError( "Parameter '", prev.Name, "' already declared at ", p.file.Position(prev.Pos))
            }
        }

        for p.accept(token.COMMA) {
            if p.tok != token.IDENT {
                p.addError("Invalid Argument " + p.tok.String() + ": '" + p.lit + "'")
            }
            el = append(el, p.ParseExpr())
            id, idok := el[len(el) - 1].(*ast.Ident)
            if !idok {
                p.addError("Invalid Argument " + p.tok.String() + ": '" + p.lit + "'")
            } else {
                prev := p.currentScope.Insert(&ast.Object{
                    Pos: pos,
                    Name: id.Name,
                })

                if prev != nil {
                    p.addError( "Parameter '", prev.Name, "' already declared at ", p.file.Position(prev.Pos))
                }
            }
        }
        p.expect(token.RPAREN)
    }
    body := p.ParseScope()

    return &ast.FunctionExpr{
        Pos: pos,
        Args: el,
        Body: body,
    }
}
