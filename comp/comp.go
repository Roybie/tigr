package main

import (
    "fmt"

    "github.com/roybie/tigr/ast"
    "github.com/roybie/tigr/parser"
)

func main() {
    ex := `{
    //Terms of sequence to print out
    terms := 20;
    //Variable for holding sequence
    v := [0,1];
    // for [] returns an array of expressions in the loop
    v += for[] i:=0;i<terms-#v { i+=1;v[1] = v[0] + (v[0] = v[1]) }
}`
    e, err := parser.ParseExpression("test", ex)
    fmt.Println(ex)
    fmt.Println(ast.Print(e))
    if err != nil {
        fmt.Println(err)
    }
}
