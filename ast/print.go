package ast

import (
    "github.com/roybie/tigr/token"
)

func Print(node Expr) string {
    if node == nil {
        return ""
    }
    switch n := node.(type) {
    case *BinaryExpr:
        return(Print(n.Lhs) + " " + n.Op.String() + " " + Print(n.Rhs))
    case *UnaryExpr:
        return(n.Op.String() + Print(n.Value))
    case *Ident:
        return(n.Name)
    case *BasicLit:
        str := ""
        if n.Kind == token.STRING {
            str += "\""
        }
        str += n.Lit
        if n.Kind == token.STRING {
            str += "\""
        }
        return(str)
    case *ScopeExpr:
        str := "{"
        for i, v := range n.List {
            str += Print(v)
            if i < len(n.List) - 1 {
                str += "; "
            }
        }
        str += " }"
        return(str)
    case *IfExpr:
        str := "if "
        for i, v := range n.Cond {
            str += Print(v)
            if i < len(n.Cond) - 1 {
                str += "; "
            }
        }
        str += " "
        str += Print(n.Then)
        if _, ok := (n.Else).(*ScopeExpr); ok {
            str += " else "
            str += Print(n.Else)
        }
        return str
    case *ForExpr:
        str := "for "
        for i, v := range n.Cond {
            str += Print(v)
            if i < len(n.Cond) - 1 {
                str += "; "
            }
        }
        str += " "
        str += Print(n.Body)
        return(str)
    case *ForAExpr:
        str := "for[] "
        for i, v := range n.Cond {
            str += Print(v)
            if i < len(n.Cond) - 1 {
                str += "; "
            }
        }
        str += " "
        str += Print(n.Body)
        return(str)
    case *WhileExpr:
        str := "while "
        for i, v := range n.Cond {
            str += Print(v)
            if i < len(n.Cond) - 1 {
                str += "; "
            }
        }
        str += " "
        str += Print(n.Body)
        return(str)
    case *WhileAExpr:
        str := "while[] "
        for i, v := range n.Cond {
            str += Print(v)
            if i < len(n.Cond) - 1 {
                str += "; "
            }
        }
        str += " "
        str += Print(n.Body)
        return(str)
    case *IndexedExpr:
        return(Print(n.Item) + "[" + Print(n.Index) + "]")
    case *CallExpr:
        str := Print(n.Callee)
        str += "("
        for i, v := range n.Args {
            str += Print(v)
            if i < len(n.Args) - 1 {
                str += ", "
            }
        }
        str += ")"
        return str
    case *BreakExpr:
        return("break " + Print(n.Value))
    case *ReturnExpr:
        return("return " + Print(n.Value))
    case *ImportExpr:
        return("import " + Print(n.Import))
    case *ArrayExpr:
        str := "["
        for i, v := range n.Elements {
            str += Print(v)
            if i < len(n.Elements) - 1 {
                str += ", "
            }
        }
        str += "]"
        return str
    case *ObjectExpr:
        str := "{"
        for i, v := range n.Elements {
            str += Print(v)
            if i < len(n.Elements) - 1 {
                str += ", "
            }
        }
        str += "}"
        return str
    case *ObjectMemberExpr:
        return(Print(n.Index) + ": " + Print(n.Value))
    case *FunctionExpr:
        str := "fn ("
        for i, v := range n.Args {
            str += Print(v)
            if i < len(n.Args) - 1 {
                str += ", "
            }
        }
        str += ")"
        str += Print(n.Body)
        return str
    default:
        return ""
    }
}
