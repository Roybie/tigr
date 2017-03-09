package token

import (
    "fmt"
)

type Pos uint

var NoPos = Pos(0)

func (p Pos) Valid() bool {
    return p != NoPos
}

type Position struct {
    Filename    string
    Col, Row    int
}

func (p Position) String() string {
    if p.Filename == "" {
        return fmt.Sprintf("%d:%d", p.Row, p.Col)
    }

    return fmt.Sprintf("%d:%d:%d", p.Filename, p.Row, p.Col)
}
