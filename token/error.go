package token

import (
    "fmt"
    "strings"
)

type Error struct {
    file    *File
    position Position
    message string
}

func (e Error) Error() string {
    return fmt.Sprint(e.file.GetLine(e.position.Row - 1), "\n", strings.Repeat(" ", e.position.Col-1), "^\n", e.position, " ", e.message, "\n")
}

func (e Error) SamePos(p Position) bool {
    return e.position.Col == p.Col && e.position.Row == p.Row
}

type ErrorHandler func(Pos, ...interface{})

type ErrorList []*Error

func (el ErrorList) Count() int {
    return len(el)
}

func (el *ErrorList) Add(f *File, p Position, args ...interface{}) {
    if el.Count() > 0 && (*el)[el.Count() - 1].SamePos(p) {
        return
    }
    *el = append(*el, &Error{file: f, position: p, message: fmt.Sprint(args...)})
}

func (el ErrorList) Error() string {
    var output string
    for i, err := range el {
        if i >= 10 {
           output += fmt.Sprint("More than 10 errors,", len(el)-10, "hidden")
            break
        }
        output += fmt.Sprintln(err)
    }

    return output
}

func (el ErrorList) Print() {
    for _, err := range el {
        fmt.Println(err)
    }
}
