package token

type File struct {
    base    int
    name    string
    lines   []int
    size    int
}

func (f *File) AddLine(offset int) {
    if offset >= f.base-1 && offset < f.base+f.size {
        f.lines = append(f.lines, offset)
    }
}

func (f *File) Base() int {
    return f.base
}

func (f *File) Pos(offset int) Pos {
    if offset < 0 || offset >= f.size {
        panic("illegal file offset")
    }
    return Pos(f.base + offset)
}

func (f *File) Position(p Pos) Position {
    col, row := int(p)-f.Base()+1, 1

    for i, nl := range f.lines {
        if p > f.Pos(nl) {
            col, row = int(p-f.Pos(nl))-f.Base()+1, i+1
        }
    }

    return Position{Filename: f.name, Col: col, Row: row}
}

func (f *File) Size() int {
    return f.size
}

func NewFile(name string, base, size int) *File {
    return &File{
        base:  base,
        name:  name,
        lines: make([]int, 0, 16),
        size:  size,
    }
}
