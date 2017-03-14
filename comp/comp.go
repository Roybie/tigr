package main

import (
    "fmt"

    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/parser"
)

func main() {
    ex := `{
        handler := fn(res func(string) int, req object) int {
            res("success")
        }
}`
    e, imp, err := parser.ParseExpression("test", ex)
    fmt.Println(ex)
    if len(imp) > 0 {
        fmt.Print("imports {")
        for i := range imp {
            fmt.Printf(" \"%s\" ", i)
        }
        fmt.Println("}")
    }
    fmt.Println(ast.Print(e))
    if err != nil {
        fmt.Println(err)
    }
}
