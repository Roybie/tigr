package token

import (
    "fmt"
)

type Error struct {
    position Position
    message string
}

func (e Error) Error() string {
    return fmt.Sprint(e.position, " ", e.message)
}

type ErrorHandler func(Pos, ...interface{})

type ErrorList []*Error

func (el ErrorList) Count() int {
    return len(el)
}

func (el *ErrorList) Add(p Position, args ...interface{}) {
    *el = append(*el, &Error{position: p, message: fmt.Sprint(args...)})
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
