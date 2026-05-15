# tigr

A small dynamic language where **everything is an expression**. Tigr is built around the idea that every construct — assignments, blocks, conditionals, loops, even `break` and `return` — produces a value. There are no statements.

This README documents the **v0.2** bytecode VM implementation. The complete language spec lives in [`LANGUAGE.md`](LANGUAGE.md); this is the friendlier tour.

```
double := fn(x) { x * 2 };
squares := for[] (i, 1..=10) { i * i };
print('first square doubled:', double(squares[0]));   // 'first square doubled: 2'
```

---

## Running tigr

```bash
cargo build --release
./target/release/tigr path/to/program.tg
```

When the program finishes, its final value is printed. So `1 + 1` as a one-line file produces `2`.

There are working examples under [`examples/v02/`](examples/v02/) organised by build phase, plus a few Project Euler solutions in [`examples/v02/euler/`](examples/v02/euler/).

---

## Core idea: everything is an expression

Every line of tigr produces a value. The value of a block is its last expression. The value of `if` is the value of the chosen branch. The value of a function is whatever the body ends with. There's no `return` needed for the common case.

```
x := if 5 > 3 { 'big' } else { 'small' };   // x == 'big'

total := for (n, 1..=10) { n };              // total == 10 (last n)
all   := for[] (n, 1..=10) { n };            // all   == [1,2,3,4,5,6,7,8,9,10]

sum := fn(a, b) { a + b };                   // body's last expression IS the return value
sum(2, 3);                                   // 5
```

Because everything is an expression, you can compose freely:

```
greeting := 'Hello, ' + (if loud { 'WORLD' } else { 'world' }) + '!';
```

---

## Types

| Type     | Examples                                      | Notes                                    |
|----------|-----------------------------------------------|------------------------------------------|
| `Int`    | `42`, `-7`, `0`                               | 64-bit signed                            |
| `Float`  | `3.14`, `0.0`                                 | 64-bit IEEE-754                          |
| `String` | `'hello'`, `'name = {n}'`                     | Single-quoted, UTF-8, interpolated       |
| `Bool`   | `true`, `false`                               |                                          |
| `Null`   | `null`                                        |                                          |
| `Array`  | `[1, 'two', true]`                            | Heterogeneous, reference type            |
| `Object` | `${name: 'a', age: 1}`                        | String keys, reference type              |
| `Range`  | `0..10`, `0..=10`, `10..0:-1`                 | First-class lazy iterable                |
| `Function` | `fn(x) { x * 2 }`                           | Closures over lexical environment        |

`Array` and `Object` are **reference types** — passing them around shares the same underlying value.

### Truthiness

The following are **falsy**: `false`, `null`, `0`, `0.0`, `''`, `[]`, `${}`. Everything else (including non-empty ranges and all functions) is truthy.

`&&` and `||` short-circuit and return **the value that decided the result** (not coerced to bool):

```
0 || 'fallback'      // 'fallback'
'a' && 'b'           // 'b'
null || []           // []
```

---

## Bindings: `:=` vs `=`

There are two distinct operators:

- `:=` **declares** a new binding in the current scope.
- `=` **assigns** to the nearest enclosing binding of that name (error if it doesn't exist).

```
foo := 10;           // declare
foo = 20;            // assign
bar = 5;             // ERROR — bar isn't declared
```

Compound forms `+=`, `-=`, `*=`, `/=`, `%=` require an existing binding (like `=`).

Both `:=` and `=` are expressions and evaluate to the assigned value:

```
result := (x := 5) + (y := 7);   // x=5, y=7, result=12
```

Mid-expression `:=` declarations work as you'd expect — the local is hoisted to a stable slot at scope entry so the surrounding op can't clobber it.

---

## Blocks and scopes

A **block** is a `;`-separated sequence of expressions. The block's value is the last expression's value (or `null` if the block ends in `;`).

```
(a := 1; b := a + 1; b * 2)        // 4
(a := 1; b := 2;)                  // null  (trailing ;)
```

A **scope** is a block in `{ }` — same rules, plus it opens a fresh lexical scope. Bindings declared with `:=` inside a scope are not visible after the closing `}`. Mutations to outer bindings persist:

```
a := 9;
b := { c := 20; c * (a = a + 1) };
// a == 10, b == 200, c is out of scope here
```

---

## Strings

Single-quoted, with `{expr}` interpolation. Use `\{` for a literal brace.

```
name := 'tigr';
greet := 'hello, {name}!';                // 'hello, tigr!'
math  := 'sum: {2 + 3}';                  // 'sum: 5'
arr   := [10, 20, 30];
desc  := 'first: {arr[0]}, count: {#arr}';
```

Interpolations can nest:

```
'{ if ok { 'yes' } else { 'no' } }'
```

String operators:

```
'abc' + 'def'        // 'abcdef'    concatenation
#'hello'             // 5           character count
'hello'[1]           // 'e'         indexing — out-of-range returns null
```

Strings are immutable.

---

## Arithmetic and comparison

`+ - * / % ^` (`^` is power, always returns `Float`).

Integer division stays `Int` when it divides evenly, otherwise becomes `Float`: `6 / 2 == 3` but `7 / 2 == 3.5`.

Mixed `Int`/`Float` arithmetic returns `Float`. `%` follows the sign of the dividend.

Comparison: `== != < > <= >=`. Equality across types is always false except `Int`/`Float` compare numerically. Arrays and objects compare structurally (element-/key-wise).

---

## Arrays

```
arr := [1, 2, 3];
arr[0];                              // 1
arr[-1];                             // 3   (negative indices count from the end)
#arr;                                // 3
arr + 4;                             // [1, 2, 3, 4]    (append element)
arr + [4, 5, 6];                     // [1, 2, 3, 4, 5, 6]   (concatenate arrays)
arr += 7;                            // arr is now [1, 2, 3, 7]
arr[0] = 99;                         // mutates in place
```

`Array + Array` concatenates, `Array + value` appends. To append an array as a single element, write `arr + [[1,2]]`.

Spread `...` unpacks into a literal:

```
[1, ...other, 9]                     // expanded
```

Out-of-range index returns `null`.

---

## Objects

```
obj := ${
    name: 'tigr',
    'with space': 1,
    nested: ${ inner: true },
};

obj.name;                            // 'tigr'  — `.key` is sugar for ['key']
obj['with space'];                   // 1
obj.color = 'red';                   // add a new key
#obj;                                // number of keys
```

Identifier keys (`name:`) are sugar for the quoted form (`'name':`). Object spread:

```
${...defaults, color: 'red'}         // later keys win
```

Object shorthand: `${name}` is equivalent to `${name: name}`.

Missing keys return `null`. Indexed assignment mutates in place.

---

## Ranges

Ranges are first-class lazy values, not loops:

```
r := 0..10;                          // [0, 10) — exclusive
r := 0..=10;                         // [0, 10] — inclusive
r := 0..10:2;                        // step 2 — 0, 2, 4, 6, 8
r := 10..0:-1;                       // descending — 10, 9, ..., 1
r := 10..0;                          // descending; step auto-flips to -1
```

Operations:

```
#r;                                  // length
r[2];                                // element at index
[...0..5];                           // materialize: [0, 1, 2, 3, 4]
for (i, r) { ... };                  // iterate
```

A range whose `step` doesn't move `from` toward `to` is empty.

---

## Control flow

### `if` / `else`

```
if cond { ... }
if cond { ... } else { ... }
if cond1 { ... } else if cond2 { ... } else { ... }
```

`if` evaluates to the chosen branch's value, or `null` if no branch matches.

```
label := if score > 90 { 'A' } else if score > 80 { 'B' } else { 'C' };
```

### `while` and `while[]`

```
while cond { body }                  // evaluates to last iteration's value (or null)
while[] cond { body }                // collects each body value into an array (nulls filtered)
```

```
i := 0;
last := while i < 5 { i = i + 1; i * 10 };   // last == 50
```

### `for` and `for[]`

Iterates a Range, Array, Object, or String. One-variable or two-variable form:

| Iterable | One-var          | Two-var                              |
|----------|------------------|--------------------------------------|
| Range    | `for (i, 0..10)` | `for (n, i, 0..10)`   (`n` = 0,1,2…) |
| Array    | `for (x, arr)`   | `for (i, x, arr)`                    |
| Object   | `for (v, obj)`   | `for (k, v, obj)`                    |
| String   | `for (ch, str)`  | `for (i, ch, str)`                   |

```
last := for (x, [10, 20, 30]) { x };       // 30
all  := for[] (i, 1..=5) { i * i };        // [1, 4, 9, 16, 25]
```

Each iteration opens a **fresh scope** for the loop variables — closures capture each iteration's `i` independently:

```
adders := for[] (i, 0..3) { fn(x) { x + i } };
adders[0](10);                              // 10
adders[1](10);                              // 11
adders[2](10);                              // 12
```

### `break`

Exits the innermost loop, optionally with a value:

```
break                                // null
break 5                              // 5
break (x + y)                        // expression form needs parens
```

In a `for[]` / `while[]`, the break value is appended to the result array (unless `null`, which is filtered).

`break` is itself an expression — pass it to another `break` to propagate out:

```
for (i, 0..10) {
    for (j, 0..10) {
        if i * j == 25 {
            break (break [i, j])     // bail out of both loops with [5, 5]
        }
    }
}
```

### `return`

Exits the innermost function. Like `break`, it's an expression and can be chained.

```
find := fn(arr, target) {
    for (i, 0..#arr) {
        if arr[i] == target { return i }
    };
    null
};
```

---

## Functions

```
add := fn(a, b) { a + b };
add(2, 3);                           // 5

fn() { 0 }();                        // anonymous, immediately invoked
```

Functions capture their enclosing environment as a closure. Captured variables are by reference:

```
make_counter := fn() {
    n := 0;
    fn() { n += 1 }                  // captures n by reference
};
c := make_counter();
c();                                 // 1
c();                                 // 2
```

### Parameters

- **Positional**: missing args become `null`, extra args are dropped.
- **Rest**: a final `...name` collects the remaining args as an array.
- **Patterns**: any parameter can be a destructuring pattern.

```
length := fn(...args) { #args };
length();                            // 0
length(1, 2, 3);                     // 3

greet := fn(${name, age}) { 'hi {name}, {age}!' };
greet(${name: 'tigr', age: 0});      // 'hi tigr, 0!'
```

### Method-style calls

`obj.method(args)` is `(obj.method)(args)` — plain index then call. Tigr doesn't pass `this`. For receiver-as-first-arg style, use pipe (below).

---

## Pipe `|>`

`x |> f(args)` rewrites to `f(x, args)`. If the right side isn't a call, `|>` calls it with `x` as the sole argument.

```
arr |> Array.map(double) |> Array.reverse()
// equivalent to: Array.reverse(Array.map(arr, double))

5 |> double                          // double(5)
5 |> double()                        // double(5)
0..10 |> Array.from()                // Array.from(0..10)
```

Pipe is left-associative; evaluation is strictly left-to-right.

---

## Destructuring

Patterns appear on the LHS of `:=`, on the LHS of `=`, and as function parameters. Missing values bind to `null`.

### Array patterns

```
[a, b, c] := [1, 2, 3];              // a=1, b=2, c=3
[head, ...rest] := [10, 20, 30, 40]; // head=10, rest=[20,30,40]
[x, _, z] := [1, 2, 3];              // _ skips a position
[m, n] := [99];                      // m=99, n=null
```

### Object patterns

```
${name, age} := person;              // shorthand: name := person.name etc.
${name: n} := person;                // rename
${name, ...others} := person;        // rest collects remaining keys
```

### Nested patterns

```
${user: ${id, name}} := response;
[${name}, ${name: second}] := pair_of_people;
```

---

## Modules / imports

```
Array := import 'Array';
util  := import './lib/util';
```

`import` evaluates the named file as a complete program in a fresh root environment and returns its final value. Paths are resolved relative to the importing file. The `.tg` extension is added automatically if absent.

A module is typically just an object literal:

```
// Array.tg
${
    map: fn(arr, f) { for[] (x, arr) { f(x) } },
    filter: fn(arr, f) { for[] (x, arr) { if f(x) { x } } },
    // ...
}
```

Each `import` re-evaluates the file — there's no caching in v0.2.

---

## Built-ins

These are ordinary bindings in the root environment. They can be shadowed, passed around, or stored.

| Name    | Signature                  | Behavior                                   |
|---------|----------------------------|--------------------------------------------|
| `print` | `print(...args)`           | Write each arg via `str`, space-separated, plus newline. Returns last arg. |
| `str`   | `str(x) -> String`         | Canonical string form of any value         |
| `num`   | `num(x) -> Number\|null`   | Parse string or pass through a number      |
| `int`   | `int(x) -> Int`            | Truncate toward zero                       |
| `float` | `float(x) -> Float`        | Coerce/parse to Float                      |
| `bool`  | `bool(x) -> Bool`          | Apply the truthiness rule                  |
| `floor` | `floor(x) -> Int`          | Round down                                 |
| `ceil`  | `ceil(x) -> Int`           | Round up                                   |
| `rand`  | `rand() -> Float`          | Uniform in `[0, 1)`                        |

`str` rules (in brief): `null` → `'null'`, numbers → decimal (Int has no point, Float always does), strings unchanged, arrays/objects bracketed with elements `str`-ed, ranges as `'a..b'` (or `'a..=b'`, with `:step` if non-default), functions as `'fn(...)'`.

---

## A worked example

Project Euler #4 — largest palindrome made from the product of two 3-digit numbers:

```
for (i, 999..=900) {
  for (j, 999..=900) {
    n := num := i * j;
    r := 0;

    if n == while num > 0 {
              dig := num % 10;
              num = (num - dig) / 10;
              r = r * 10 + dig
            }
    {
      break (break n)
    }
  }
}
```

See [`examples/v02/`](examples/v02/) for many more — destructuring, closures, pipes, imports, the array library, and a handful of Project Euler solutions.

---

## Operator precedence

Low to high, with associativity:

| Level | Operators                                       | Assoc |
|-------|-------------------------------------------------|-------|
| 1     | `=` `:=` `+=` `-=` `*=` `/=` `%=`               | right |
| 2     | `\|\|`                                          | left  |
| 3     | `&&`                                            | left  |
| 4     | `==` `!=` `<` `>` `<=` `>=`                     | left  |
| 5     | `\|>`                                           | left  |
| 6     | `..` `..=` (with optional `:step`)              | n/a   |
| 7     | `+` `-`                                         | left  |
| 8     | `*` `/` `%`                                     | left  |
| 9     | `^`                                             | right |
| 10    | unary `-` `!` `#`                               | n/a   |
| 11    | call `f(...)`, index `a[i]`, member `a.b`       | left  |

---

## Status

v0.2 is feature-complete: the seven build phases of the bytecode VM are shipped and the test suite passes (169 tests). The full spec is in [`LANGUAGE.md`](LANGUAGE.md). The v0.1 tree-walking interpreter source lives under `src/v01/` for reference; it's not currently wired into the build.

Known limitations / v0.3 candidates:

- No module caching — every `import` re-evaluates the file.
- No tracing GC — collections use `Rc<RefCell<...>>`, so cycles leak.
- Array and object destructuring patterns work fine at the top of a statement but aren't yet hoisted when nested mid-expression (Ident destructures are). Workaround: lift the destructure into a statement of its own.
