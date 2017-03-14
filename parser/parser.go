package parser

import (
    "fmt"
    "strings"
    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/lexer"
    "github.com/roybie/tigr/token"
)

func ParseExpression(name, src string) (ast.Expr, map[string]bool, error) {
    var p parser

    fset := token.NewFileSet()
    file := fset.Add(name, src)

    p.init(file, name, string(src), nil)
    node := p.ParseFile()

    if p.errors.Count() > 0 {
        return nil, nil, p.errors
    }
    return node, p.goimports, nil
}

type parser struct {
    file *token.File
    errors token.ErrorList
    lexer lexer.Lexer
    goimports map[string]bool

    currentScope *ast.Scope

    pos token.Pos
    tok token.Token
    lit string
}

func (p *parser) addError(args ...interface{}) {
    p.errors.Add(p.file, p.file.Position(p.pos), args...)
}

func (p *parser) addGoImport(imp string) {
    p.goimports[imp] = true
}

func (p *parser) expect(tokens ...token.Token) token.Pos {
    pos := p.pos
    if !p.accept(tokens...) {
        toks := make([]string, 0)
        for _, t := range tokens {
            toks = append(toks, t.String())
        }
        exp := strings.Join(toks, " or ")
        errorString := "Expected '" + exp + "' got '" + p.lit + "'"
        if exp == "}" {
            errorString += "\nDid you forget a ';' at the end of the previous expression?"
        }
        p.addError(errorString)
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
    p.goimports = make(map[string]bool, 0)
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
        e = p.ParseIdent()
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
    case token.GO:
        e = p.ParseGoExpr()
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

func (p *parser) ParseIdent() ast.Expr {
    lit := p.lit
    return &ast.Ident{
        Pos: p.expect(token.IDENT),
        Name: lit,
    }
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

    el := p.ParseFuncParamList()

    typ := p.ParseTypeExpr()
    body := p.ParseScope()

    return &ast.FunctionExpr{
        Pos: pos,
        Args: el,
        Type: typ,
        Body: body,
    }
}

func (p *parser) ParseFuncParamList() []ast.Expr {

    el := make([]ast.Expr, 0)

    p.expect(token.LPAREN)

    if !p.accept(token.RPAREN) {
        el = append(el, p.ParseParamExpr())
        id, idok := el[len(el) - 1].(*ast.ParamExpr).Name.(*ast.Ident)
        if !idok {
            p.addError("Invalid Argument " + p.tok.String() + ": '" + p.lit + "'")
        } else {
            prev := p.currentScope.Insert(&ast.Object{
                Pos: p.pos,
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
            el = append(el, p.ParseParamExpr())
            id, idok := el[len(el) - 1].(*ast.ParamExpr).Name.(*ast.Ident)
            if !idok {
                p.addError("Invalid Argument " + p.tok.String() + ": '" + p.lit + "'")
            } else {
                prev := p.currentScope.Insert(&ast.Object{
                    Pos: p.pos,
                    Name: id.Name,
                })

                if prev != nil {
                    p.addError( "Parameter '", prev.Name, "' already declared at ", p.file.Position(prev.Pos))
                }
            }
        }
        p.expect(token.RPAREN)
    }

    return el
}

func (p *parser) ParseParamExpr() ast.Expr {
    pos := p.pos
    name := p.ParseIdent()
    typ := p.ParseTypeExpr()

    return &ast.ParamExpr{
        Pos: pos,
        Name: name,
        Type: typ,
    }
}

func (p *parser) ParseGoExpr() ast.Expr {
    pos := p.expect(token.GO)
    p.expect(token.LBRACE)
    i := p.ParseExpr()
    p.expect(token.COMMA)
    f := p.ParseExpr()
    p.expect(token.COMMA)
    t := p.ParseExpr()
    p.expect(token.RBRACE)

    imp, iOk := i.(*ast.BasicLit)
    fun, fOk := f.(*ast.BasicLit)
    typ, tOk := t.(*ast.BasicLit)

    if iOk && fOk && tOk && imp.Kind == token.STRING && fun.Kind == token.STRING && typ.Kind == token.STRING {
        //ok!
        if len(imp.Lit) > 0 {
            p.addGoImport(imp.Lit)
        }
    } else {
        p.addError("Go function and import must be string")
    }
    return &ast.GoExpr{
        Pos: pos,
        Function: fun.Lit,
        Type: typ.Lit,
    }
}

func (p *parser) ParseTypeExpr() string {
    lit, tok := p.lit, p.tok
    if !p.tok.IsType() {
        p.addError("Invalid type " + lit)
    }

    p.next()
    if tok == token.ARRAYTYPE {
        t := p.ParseTypeExpr()
        return "[]" + t
    }
    if tok == token.OBJECTTYPE {
        return "map[string]interface{}"
    }
    if tok == token.FUNCTYPE {
        pl := p.ParseTypeList()
        t := p.ParseTypeExpr()
        return "func(" + strings.Join(pl, ", ") + ") " + t + " "
    }

    return lit
}

func (p *parser) ParseTypeList() []string {
    p.expect(token.LPAREN)

    pl := make([]string, 0)

    if !p.accept(token.LPAREN) {
        pl = append(pl, p.ParseTypeExpr())

        for p.accept(token.COMMA) {
            pl = append(pl, p.ParseTypeExpr())
        }
        p.expect(token.RPAREN)
    }
    return pl
}