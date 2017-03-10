package main

import (
    "fmt"

    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/parser"
)

func main() {
    ex := "(-5 == 6) & 1 + 2 * #{ 5 + 7; someident}"
    e, err := parser.ParseExpression("test", ex)
    fmt.Println(ex)
    ast.Print(e)
    if err != nil {
        fmt.Println(err)
    }
}
