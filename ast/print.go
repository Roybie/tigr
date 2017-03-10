package ast

import (
    "fmt"
)

func Print(node Expr) {
    if node == nil {
        return
    }

    switch n := node.(type) {
    case *BinaryExpr:
        fmt.Print("( ")
        Print(n.Lhs)
        fmt.Print(" ", n.Op, " ")
        Print(n.Rhs)
        fmt.Print(" )")
    case *UnaryExpr:
        fmt.Print("( ")
        fmt.Print(n.Op, " ")
        Print(n.Value)
        fmt.Print(" )")
    case *Ident:
        fmt.Print("ID:" + n.Name)
    case *BasicLit:
        fmt.Print(n.Lit)
    case *ScopeExpr:
        fmt.Print("{ ")
        for i, v := range n.List {
            Print(v)
            if i < len(n.List) - 1 {
                fmt.Print("; ")
            }
        }
        fmt.Print(" }")
    }
}
