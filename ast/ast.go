package ast

import (
    "github.com/roybie/tigr/token"
)

type Node interface {
    Position() token.Pos
}

type Expr interface {
    Node
    exprNode()
}

type BinaryExpr struct {
    Op  token.Token
    Pos token.Pos
    Lhs Expr
    Rhs Expr
}

type UnaryExpr struct {
    Op token.Token
    Pos token.Pos
    Value Expr
}

type BasicLit struct {
    Pos token.Pos
    Kind token.Token
    Lit  string
}

type Ident struct {
    Pos token.Pos
    Name string
}

type IndexedExpr struct {
    Pos token.Pos
    Item Expr
    Index Expr
}

type ScopeExpr struct {
    Pos token.Pos
    List []Expr
}

type IfExpr struct {
    Pos token.Pos
    Cond []Expr
    Then Expr
    Else Expr
}

type ForExpr struct {
    Pos token.Pos
    Cond []Expr
    Body Expr
}

type ForAExpr struct {
    Pos token.Pos
    Cond []Expr
    Body Expr
}

type WhileExpr struct {
    Pos token.Pos
    Cond []Expr
    Body Expr
}

type WhileAExpr struct {
    Pos token.Pos
    Cond []Expr
    Body Expr
}

type BreakExpr struct {
    Pos token.Pos
    Value Expr
}

type CallExpr struct {
    Pos token.Pos
    Callee Expr
    Args []Expr
}

type ReturnExpr struct {
    Pos token.Pos
    Value Expr
}

type ImportExpr struct {
    Pos token.Pos
    Import Expr
}

type ArrayExpr struct {
    Pos token.Pos
    Elements []Expr
}

type ObjectExpr struct {
    Pos token.Pos
    Elements []Expr
}

type ObjectMemberExpr struct {
    Pos token.Pos
    Index Expr
    Value Expr
}

type FunctionExpr struct {
    Pos token.Pos
    Args []Expr
    Body Expr
}

type Scope struct {
    Parent *Scope
    Table map[string]*Object
}

type Object struct {
    Pos token.Pos
    Name string
}

func (e *BinaryExpr) Position() token.Pos { return e.Pos }
func (e *UnaryExpr) Position() token.Pos { return e.Pos }
func (e *BasicLit) Position() token.Pos { return e.Pos }
func (e *Ident) Position() token.Pos { return e.Pos }
func (e *IndexedExpr) Position() token.Pos { return e.Pos }
func (e *ScopeExpr) Position() token.Pos { return e.Pos }
func (e *IfExpr) Position() token.Pos { return e.Pos }
func (e *ForExpr) Position() token.Pos { return e.Pos }
func (e *ForAExpr) Position() token.Pos { return e.Pos }
func (e *WhileExpr) Position() token.Pos { return e.Pos }
func (e *WhileAExpr) Position() token.Pos { return e.Pos }
func (e *BreakExpr) Position() token.Pos { return e.Pos }
func (e *CallExpr) Position() token.Pos { return e.Pos }
func (e *ReturnExpr) Position() token.Pos { return e.Pos }
func (e *ImportExpr) Position() token.Pos { return e.Pos }
func (e *ArrayExpr) Position() token.Pos { return e.Pos }
func (e *Object) Position() token.Pos { return e.Pos }
func (e *ObjectExpr) Position() token.Pos { return e.Pos }
func (e *ObjectMemberExpr) Position() token.Pos { return e.Pos }
func (e *FunctionExpr) Position() token.Pos { return e.Pos }

func (e *BinaryExpr) exprNode() {}
func (e *UnaryExpr) exprNode() {}
func (e *BasicLit) exprNode() {}
func (e *Ident) exprNode() {}
func (e *IndexedExpr) exprNode() {}
func (e *ScopeExpr) exprNode() {}
func (e *IfExpr) exprNode() {}
func (e *ForExpr) exprNode() {}
func (e *ForAExpr) exprNode() {}
func (e *WhileExpr) exprNode() {}
func (e *WhileAExpr) exprNode() {}
func (e *BreakExpr) exprNode() {}
func (e *CallExpr) exprNode() {}
func (e *ReturnExpr) exprNode() {}
func (e *ImportExpr) exprNode() {}
func (e *ArrayExpr) exprNode() {}
func (e *ObjectExpr) exprNode() {}
func (e *ObjectMemberExpr) exprNode() {}
func (e *FunctionExpr) exprNode() {}

func NewScope(parent *Scope) *Scope {
    return &Scope{Parent: parent, Table: make(map[string]*Object)}
}

func (s *Scope) Insert(ob *Object) *Object {
    if old, ok := s.Table[ob.Name]; ok {
        return old
    }
    s.Table[ob.Name] = ob
    return nil
}

func (s *Scope) Lookup(ident string) *Object {
    if ob, ok := s.Table[ident]; ok || s.Parent == nil {
        return ob
    }
    return s.Parent.Lookup(ident)
}
