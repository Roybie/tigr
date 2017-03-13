package main

import (
    "fmt"

    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/parser"
)

func main() {
    ex := `{
    //Terms of sequence to print out
    terms := go{"fmt", "fmt.Println", "int"}("Hello World");
    //Variable for holding sequence
    v := [0,1];
    // for [] returns an array of expressions in the loop
    v += for[] i:=0;i<terms-#v { i+=1;v[1] = v[0] + (v[0] = v[1]) };
    terms = b := 3;
    add := fn (a int, b int) function { a + b }
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
