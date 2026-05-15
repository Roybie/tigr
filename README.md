# tigr

A small dynamic language where **everything is an expression**. Tigr is built around the idea that every construct — assignments, blocks, conditionals, loops, even `break`, `return`, and `raise` — produces a value. There are no statements.

This README documents **v0.6**: the v0.5 release plus a `continue` keyword, default parameter values, and a wider standard library (`Path`, `Object`, `DateTime`, subprocess execution, and filesystem operations). The complete language spec lives in [`LANGUAGE.md`](LANGUAGE.md); this is the friendlier tour.

```
double := fn(x) { x * 2 };
squares := for[] (i, 1..=10) { i * i };
print('first square doubled:', double(squares[0]));   // 'first square doubled: 2'
```

---

## Running tigr

```bash
cargo build --release
./target/release/tigr path/to/program.tg              # run a script
./target/release/tigr path/to/program.tg arg1 arg2    # script + args (Os.args)
./target/release/tigr                                  # interactive REPL
```

When a script finishes, its final value is printed. So `1 + 1` as a one-line file produces `2`. With no argument, tigr drops into a REPL — see [REPL](#repl) below.

There are working examples under [`examples/v02/`](examples/v02/) organised by build phase, plus Project Euler solutions in [`examples/v02/euler/`](examples/v02/euler/). v0.3 demos are in [`examples/v03/`](examples/v03/), v0.4 demos in [`examples/v04/`](examples/v04/), and v0.5 demos in [`examples/v05/`](examples/v05/).

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
| `Int`    | `42`, `0xFF`, `0b1010`, `0o755`, `1_000_000`  | 64-bit signed; hex/bin/oct + `_` separators |
| `Float`  | `3.14`, `.5`, `1e6`, `2.5e-3`                 | 64-bit IEEE-754; scientific is always `Float` |
| `String` | `'hello'`, `'name = {n}'`                     | Single-quoted, UTF-8, interpolated       |
| `Bool`   | `true`, `false`                               |                                          |
| `Null`   | `null`                                        |                                          |
| `Array`  | `[1, 'two', true]`                            | Heterogeneous, reference type            |
| `Object` | `${name: 'a', age: 1}`                        | String keys, reference type              |
| `Range`  | `0..10`, `0..=10`, `10..0:-1`                 | First-class lazy iterable                |
| `Function` | `fn(x) { x * 2 }`                           | Closures over lexical environment        |

Underscores are allowed only between digits — `_5`, `5_`, `5__5`, and `0x_FF` are all rejected. A trailing `5.` lexes as `Int(5)` followed by `Dot` so `5.method` style member access still works.

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

`+ - * / % ^^` (`^^` is power, always returns `Float`).

Integer division stays `Int` when it divides evenly, otherwise becomes `Float`: `6 / 2 == 3` but `7 / 2 == 3.5`.

Mixed `Int`/`Float` arithmetic returns `Float`. `%` follows the sign of the dividend.

Comparison: `== != < > <= >=`. Equality across types is always false except `Int`/`Float` compare numerically. Arrays and objects compare structurally (element-/key-wise).

## Bitwise operators

`& | ^ ~ << >>` operate on `Int` only — any other operand raises. `^` is bitwise XOR (exponentiation is the separate `^^`). `>>` is an arithmetic, sign-preserving shift; a shift amount outside `0..64` raises.

```
0b1100 & 0b1010      // 8
0b1100 | 0b1010      // 14
0b1100 ^ 0b1010      // 6      bitwise XOR
~0                   // -1
1 << 8               // 256
-16 >> 2             // -4     arithmetic shift
```

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

### `continue`

`continue` skips the rest of the current loop iteration and moves to the next. The skipped iteration contributes `null` — so in a `for[]` / `while[]` nothing is appended, and in a plain `for` / `while` that iteration's value becomes `null`. Unlike `break`, `continue` carries no value. Using it outside a loop is a compile-time error.

```
evens := for[] (n, 0..10) {
    if n % 2 != 0 { continue };
    n
};                                   // [0, 2, 4, 6, 8]
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

### `try` / `catch` / `raise`

Recoverable errors. `raise expr` aborts the current evaluation with a string message; `try expr` evaluates `expr` and yields its value on success, or `null` on a raised/runtime error. `try expr catch (e) { handler }` runs the handler with the error message bound to `e`. All three are expressions.

```
content := try IO.read_file('config.tg') catch (e) {
    print('warning:', e);
    ''
};

n := try int(input) || 0;       // null on parse failure → 0

raise 'database connection lost'
```

Built-in runtime errors (division by zero, type mismatch, out-of-bounds, missing file...) are catchable — the catch handler sees the same message that an uncaught error would print.

The body of `try` binds tighter than `||` so `try f(x) || default` is the natural fallback idiom; wrap in parens if you want the `||` inside the try body.

### `match`

`match` evaluates a subject once and tries each comma-separated arm top-to-bottom, yielding the body of the first arm whose pattern (and optional `if` guard) matches. With no matching arm it evaluates to `null` — non-exhaustive, like an `if` with no `else`. It's an expression.

```
grade := match score {
    90..=100 => 'A',
    80..=89  => 'B',
    70..=79  => 'C',
    _        => 'F',
};
```

Match patterns are *refutable* — they can fail and fall through (unlike the destructuring patterns of the previous section). The pattern kinds:

- **Literal** — `0`, `'hi'`, `true`, `null`, `-1` — matches if the subject `==` it.
- **Binding** — a bare name; matches anything and binds it for the arm.
- **Wildcard** — `_`; matches anything, binds nothing.
- **Range** — `0..10` / `0..=9`; matches a number in range (a non-number just fails).
- **Array** — `[a, b]` (exact length) or `[head, ...rest]` (length ≥ 1).
- **Object** — `${kind: 'circle', r}`; sub-pattern fields must match, shorthand fields bind (missing key → `null`).
- **Or-pattern** — `1 | 2 | 3`; matches any alternative. Alternatives may not bind variables.

```
area := fn(shape) {
    match shape {
        ${kind: 'circle', r}  => 3.14159 * r ^^ 2,
        ${kind: 'rect', w, h} => w * h,
        _                     => raise 'unknown shape',
    }
};

sum := fn(xs) {
    match xs {
        []            => 0,
        [head, ...tl] => head + sum(tl),
    }
};

classify := match n {
    x if x < 0 => 'negative',
    0          => 'zero',
    _          => 'positive',
};
```

Each arm body runs in its own scope; pattern bindings and a guard see those names.

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
- **Defaults**: a parameter can have a default — `fn(a, b = 10)`. The default fills in when that argument slot is `null` (omitted *or* explicitly passed `null`).

```
length := fn(...args) { #args };
length();                            // 0
length(1, 2, 3);                     // 3

greet := fn(${name, age}) { 'hi {name}, {age}!' };
greet(${name: 'tigr', age: 0});      // 'hi tigr, 0!'

scale := fn(x, factor = 2) { x * factor };
scale(10);                           // 20  — default used
scale(10, 5);                        // 50
scale(10, null);                     // 20  — explicit null also triggers it
```

A default is only allowed on a plain identifier parameter (not a destructuring pattern, not the rest parameter). Defaults may reference earlier parameters (`fn(a, b = a + 1)`), evaluate left-to-right, and run only when needed. Note a falsy-but-not-null value like `0` does **not** trigger the default — only `null` does.

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

### Reassigning with `=`

Patterns also work on the LHS of plain `=`. Every leaf must already be declared, otherwise it's a compile-time error.

```
a := 1; b := 2;
[b, a] = [a, b];                     // swap
${x, y} = ${x: 10, y: 20};           // bulk reassign
```

Compound forms like `+=` are not allowed with patterns.

### Mid-expression decls

A pattern `:=` inside a larger expression hoists each leaf to a stable slot at scope entry, so the surrounding op can't clobber it. The expression evaluates to the source rhs:

```
arr := ([a, b] := [3, 4]);           // arr=[3,4]; a=3; b=4
n   := 5 + ([c, d] := [10, 20])[0];  // n=15; c=10; d=20
```

The same applies inside `for` iter expressions, function-call args, etc.

---

## Modules / imports

```
Array := import 'Array';       // bare name → bundled stdlib / native module
local := import './lib/util';  // path → user file
```

There are two flavors:

- **Bare names** (no `/`, `\`, or `.`): resolved against the bundled stdlib and native-module registry. `Array`, `String`, `Math` are tigr-source modules; `IO`, `Os`, `Time` are native. Unknown names raise.
- **Path-shaped**: resolved relative to the importing file. The `.tg` extension is added automatically if absent.

A user module is typically just an object literal:

```
// lib/util.tg
${
    map: fn(arr, f) { for[] (x, arr) { f(x) } },
    filter: fn(arr, f) { for[] (x, arr) { if f(x) { x } } },
    // ...
}
```

Each path is evaluated **at most once per session** — subsequent imports of the same path return the cached value, so two `import 'X'` calls return the same underlying object. Circular imports raise a catchable error.

---

## Built-ins

These are ordinary bindings in the root environment. They can be shadowed, passed around, or stored.

| Name    | Signature                  | Behavior                                   |
|---------|----------------------------|--------------------------------------------|
| `print` | `print(...args)`           | Write each arg via `str`, space-separated, plus newline. Returns last arg. |
| `str`   | `str(x [, radix [, prefix]])` | Canonical string form; radix form renders an Int in base 2..=36 |
| `num`   | `num(x) -> Number\|null`   | Parse string or pass through a number      |
| `int`   | `int(x) -> Int`            | Truncate toward zero                       |
| `float` | `float(x) -> Float`        | Coerce/parse to Float                      |
| `bool`  | `bool(x) -> Bool`          | Apply the truthiness rule                  |
| `floor` | `floor(x) -> Int`          | Round down                                 |
| `ceil`  | `ceil(x) -> Int`           | Round up                                   |
| `rand`  | `rand() -> Float`          | Uniform in `[0, 1)`                        |
| `type`  | `type(x) -> String`        | Name of the value's type (`'int'`, `'array'`, `'function'`, ...) |

`str` rules (in brief): `null` → `'null'`, numbers → decimal (Int has no point, Float always does), strings unchanged, arrays/objects bracketed with elements `str`-ed, ranges as `'a..b'` (or `'a..=b'`, with `:step` if non-default), functions as `'fn(...)'`.

`str` also takes an optional **radix** and **prefix** for rendering an `Int` in another base:

```
str(255, 16)         // 'ff'
str(255, 16, true)   // '0xff'   — prefix: 0b / 0o / 0x for radix 2 / 8 / 16
str(10, 2, true)     // '0b1010'
str(-10, 16, true)   // '-0xa'
```

The radix is an `Int` in `2..=36` (lowercase digits). A non-`Int` value, an out-of-range radix, or `prefix == true` for a radix without a literal marker all raise.

---

## Standard library reference

Bundled modules, imported with `import 'Name'`. Each entry below gives its full signature, return value, and whether it raises. `Array`, `String`, `Math`, and `Object` are tigr-source modules; `IO`, `Path`, `Os`, `Time`, `DateTime`, and `JSON` are native (Rust-backed). All `raise`d errors are catchable with `try` / `catch`.

### `Array`

A tigr-source module. Several functions take a **callback** — a function value you supply, which the module calls for you. These are the parameters named `func`, `pred`, and `key` in the table below. Unless a row's description says otherwise, the callback is invoked as `callback(element, index, whole_array)`; since tigr drops extra arguments, declare it with only the parameters you need — `fn(x)`, `fn(x, i)`, or `fn(x, i, arr)` all work. (`reduce`, `create`, and `sort_by` use different callback signatures — see their rows.) Pure tigr: these raise only when an operation they perform does (e.g. `sum` on non-numbers).

| Function | Returns | Behavior |
|---|---|---|
| `create(len, func)` | `Array` | Length-`len` array; element `i` is `func(i)` |
| `concat(a, b)` | `Array` | Concatenate arrays `a` and `b` |
| `map(arr, func)` | `Array` | Apply `func` to each element |
| `filter(arr, pred)` | `Array` | Keep elements where `pred` is truthy |
| `reduce(arr, func, seed)` | value | Left fold; `func(acc, elem, index, arr)` starting from `seed` |
| `flatten(arr)` | `Array` | Concatenate one level of nested arrays |
| `reverse(arr)` | `Array` | Elements in reverse order |
| `index(arr, elem)` | `Int \| null` | First index `==` to `elem`, or `null` |
| `find(arr, pred)` | value `\| null` | First element where `pred` is truthy, or `null` |
| `find_index(arr, pred)` | `Int` | Index of the first match, or `-1` |
| `any(arr, pred)` | `Bool` | True if `pred` holds for at least one element |
| `all(arr, pred)` | `Bool` | True if `pred` holds for every element |
| `head(arr, n)` | `Array` | First `n` elements (clamped to length) |
| `tail(arr, n)` | `Array` | Last `n` elements (clamped to length) |
| `take(arr, n)` | `Array` | First `n` elements (`n` clamped to `0..#arr`) |
| `drop(arr, n)` | `Array` | All but the first `n` elements |
| `slice(arr, start, end)` | `Array` | Elements `[start, end)`; out-of-range bounds clamp |
| `sum(arr)` | `Number` | Sum of elements (`0` if empty) |
| `max_of(arr)` | value `\| null` | Largest element, or `null` if empty |
| `min_of(arr)` | value `\| null` | Smallest element, or `null` if empty |
| `uniq(arr)` | `Array` | First-seen unique elements, order preserved |
| `zip(a, b)` | `Array` | Pairwise `[a[i], b[i]]`; length is `min(#a, #b)` |
| `join(arr, sep)` | `String` | `str()` each element, joined by `sep` |
| `sort(arr)` | `Array` | Ascending order (insertion sort) |
| `sort_by(arr, key)` | `Array` | Ascending by `key(element)` |

```
Array := import 'Array';
Array.sum(Array.filter([1, 2, 3, 4, 5], fn(x) { x % 2 == 0 }))   // 6
```

### `String`

A tigr-source module wrapping native primitives. Every entry raises on a non-`String` argument. Indices and lengths are **byte** offsets, consistent with `#` counting bytes.

| Function | Returns | Behavior |
|---|---|---|
| `split(s, sep)` | `Array<String>` | Split `s` on `sep`; an empty `sep` splits into characters |
| `join(parts, sep)` | `String` | `str()` each of `parts`, joined by `sep` |
| `replace(s, from, to)` | `String` | Replace every `from` with `to`; an empty `from` returns `s` unchanged |
| `contains(s, needle)` | `Bool` | True if `s` contains `needle` |
| `index_of(s, needle)` | `Int` | Byte index of the first `needle`, or `-1` |
| `lower(s)` | `String` | Lowercased |
| `upper(s)` | `String` | Uppercased |
| `starts_with(s, prefix)` | `Bool` | True if `s` starts with `prefix` |
| `ends_with(s, suffix)` | `Bool` | True if `s` ends with `suffix` |
| `trim(s)` | `String` | Whitespace removed from both ends |
| `trim_start(s)` | `String` | Leading whitespace removed |
| `trim_end(s)` | `String` | Trailing whitespace removed |
| `repeat(s, n)` | `String` | `s` repeated `n` times; a negative `n` raises |
| `chars(s)` | `Array<String>` | One-character strings, one per Unicode character |
| `pad_start(s, len, ch)` | `String` | Left-pad `s` with `ch` until it reaches length `len` |
| `pad_end(s, len, ch)` | `String` | Right-pad `s` with `ch` until it reaches length `len` |

```
S := import 'String';
S.split('a,b,c', ',') |> S.join('-')   // 'a-b-c'
```

### `Math`

A tigr-source module; trig / log / exp are backed by native code. Numeric functions raise on a non-`Number` argument.

| Name | Returns | Behavior |
|---|---|---|
| `PI` | `Float` | `3.141592653589793` — a value, not a function |
| `E` | `Float` | `2.718281828459045` — a value |
| `sqrt(x)` | `Float` | Square root |
| `log(x)` | `Float` | Natural logarithm |
| `log2(x)` | `Float` | Base-2 logarithm |
| `log10(x)` | `Float` | Base-10 logarithm |
| `exp(x)` | `Float` | `e` raised to `x` |
| `sin(x)`, `cos(x)`, `tan(x)` | `Float` | Trigonometric functions (radians) |
| `pow(x, y)` | `Float` | `x` raised to `y` |
| `abs(x)` | `Number` | Absolute value |
| `sign(x)` | `Int` | `-1`, `0`, or `1` |
| `min(a, b)` | value | The smaller of `a` and `b` |
| `max(a, b)` | value | The larger of `a` and `b` |
| `clamp(x, lo, hi)` | value | `x` constrained to `[lo, hi]` |

### `Object` (v0.6)

A tigr-source module. `map` and `filter` take a **callback** as their second argument — the parameters named `func` and `pred` in the table below. A callback is a function value the module calls for you; here it is invoked as `callback(value, key, whole_object)`, so declare it with as many parameters as you need (extras are dropped). `merge` / `map` / `filter` return fresh objects — they never mutate their input.

| Function | Returns | Behavior |
|---|---|---|
| `keys(obj)` | `Array<String>` | Keys in insertion order |
| `values(obj)` | `Array` | Values in insertion order |
| `entries(obj)` | `Array` | `[key, value]` pairs in insertion order |
| `from_entries(pairs)` | `Object` | Build an object from `[key, value]` pairs; later pairs win |
| `has(obj, key)` | `Bool` | True if `key` is present — distinguishes a missing key from a `null` value |
| `merge(a, b)` | `Object` | Shallow merge into a fresh object; `b` wins on collisions |
| `map(obj, func)` | `Object` | Fresh object, each value replaced by `func(value, key, obj)` |
| `filter(obj, pred)` | `Object` | Fresh object keeping entries where `pred(value, key, obj)` is truthy |

```
Object := import 'Object';
Object.entries(${a: 1, b: 2})                  // [['a', 1], ['b', 2]]
Object.map(${a: 1, b: 2}, fn(v) { v * 10 })    // ${a: 10, b: 20}  — fn is the callback
```

### `IO`

A native module. File operations raise a catchable error on failure; the predicate entries never raise.

| Function | Returns | Behavior |
|---|---|---|
| `read_file(path)` | `String` | Entire file contents as UTF-8; raises on error |
| `write_file(path, text)` | `null` | Overwrite the file with `text`; raises on error |
| `append_file(path, text)` | `null` | Append `text`, creating the file if missing; raises on error |
| `exists(path)` | `Bool` | True if the path exists; never raises |
| `list_dir(path)` | `Array<String>` | Names of the directory's entries; raises on error *(v0.6)* |
| `mkdir(path)` | `null` | Create the directory and any missing parents; raises on error *(v0.6)* |
| `remove(path)` | `null` | Delete a file, or a directory and its contents; raises on error *(v0.6)* |
| `is_dir(path)` | `Bool` | True if the path is a directory; never raises *(v0.6)* |
| `is_file(path)` | `Bool` | True if the path is a regular file; never raises *(v0.6)* |
| `stat(path)` | `Object` | `${size, is_dir, is_file, modified_ms}`; raises if the path is missing *(v0.6)* |
| `read_line()` | `String \| null` | One line from stdin without the trailing newline; `null` on EOF |
| `eprint(...args)` | last arg | Like `print`, but writes to stderr |

### `Path` (v0.6)

A native module for path-string manipulation — nothing here touches the filesystem. Every entry raises on a non-`String` argument.

| Function | Returns | Behavior |
|---|---|---|
| `join(...parts)` | `String` | Join segments with the platform separator |
| `dirname(path)` | `String` | The parent directory, or `''` if none |
| `basename(path)` | `String` | The final component, or `''` if none |
| `ext(path)` | `String` | File extension without the dot, or `''` if none |
| `is_absolute(path)` | `Bool` | True if `path` is absolute |

### `Os`

A native module for process and environment access.

| Name | Returns | Behavior |
|---|---|---|
| `args` | `Array<String>` | Command-line arguments `[interpreter, script, user_args...]` — a value, not a function |
| `env(name)` | `String \| null` | Value of environment variable `name`, or `null` if unset |
| `cwd()` | `String` | Current working directory; raises on error |
| `run(cmd, ...args)` | `Object` | Run a subprocess *(v0.6)* — see below |
| `exit(code)` | never returns | Exit the process immediately with `code`; bypasses `try` |

`Os.run(cmd, ...args)` runs `cmd` with the given string arguments and returns `${code, stdout, stderr}` — `code` is the exit status (`-1` if the process was killed by a signal), `stdout` / `stderr` are the captured output streams as strings. A non-zero exit is a normal result, not an error; `run` raises only when the process cannot be spawned (e.g. command not found).

```
Os := import 'Os';
String := import 'String';
r := Os.run('git', 'rev-parse', '--short', 'HEAD');
if r.code == 0 { print('at', String.trim(r.stdout)) }
```

### `Time`

A native module for wall-clock access.

| Function | Returns | Behavior |
|---|---|---|
| `now_ms()` | `Int` | Milliseconds since the UNIX epoch |
| `now_ns()` | `Int` | Nanoseconds since the UNIX epoch |
| `sleep_ms(n)` | `null` | Block the thread for `n` milliseconds; a negative `n` raises |

### `DateTime` (v0.6)

A native module for calendar date/time, **UTC only** (no timezone support). A *components object* has the fields `${year, month, day, hour, minute, second, ms, weekday, yearday}` — `month` is 1–12, `weekday` is 0=Sunday, `yearday` is the 1-based day of the year.

| Function | Returns | Behavior |
|---|---|---|
| `now()` | `Object` | The current UTC time as a components object |
| `from_ms(ms)` | `Object` | Convert epoch-milliseconds to a components object |
| `to_ms(obj)` | `Int` | Convert a components object to epoch-milliseconds; missing fields default (year 1970, month/day 1, others 0) |
| `format(ms, fmt)` | `String` | Render epoch-milliseconds `ms` per `fmt`. Directives: `%Y %m %d %H %M %S %j %%`; other text is literal |
| `parse(str)` | `Int` | Parse ISO-8601 `YYYY-MM-DD`, optionally `(T or space)HH:MM:SS[.fff]`, to epoch-milliseconds; raises on malformed input |

`format` takes epoch-**milliseconds**, not a components object:

```
DateTime := import 'DateTime';
DateTime.format(DateTime.to_ms(DateTime.now()), '%Y-%m-%d')   // '2026-05-15'
```

### `JSON` (v0.4)

A native module.

| Function | Returns | Behavior |
|---|---|---|
| `parse(str)` | value | Parse a JSON string. Numbers always come back as `Float`. Raises on malformed input |
| `stringify(value)` | `String` | Compact JSON text. Raises on `Function` / `Range` / `Iter` / `NativeFn` / `NaN` / `Infinity` |
| `stringify(value, indent)` | `String` | Pretty-printed; `indent` is an `Int` (number of spaces) or a `String` (literal indent unit) |

```
JSON := import 'JSON';
JSON.stringify(${name: 'tigr', v: 0.6}, 2)
// '{\n  "name": "tigr",\n  "v": 0.6\n}'
```

JSON's number model is "all numbers are IEEE 754 doubles", so `JSON.parse(JSON.stringify(123))` returns `Float(123.0)`, not `Int(123)`. On the way out, `Int` writes plain digits and an integer-valued `Float` keeps a `.0` suffix. Cycles in arrays/objects aren't detected and will overflow the call stack — same posture as the wider Rc-cycle story.

---

## Error rendering (v0.4)

When an error escapes the program (or a REPL line), tigr prints a rustc-style block: filename, source line, and a caret/underline pointing at the offending span:

```
$ tigr examples/v04/errors.tg
error[runtime]: division by zero
 --> examples/v04/errors.tg:6
  |
6 | result := x / y;
  |
```

Lex / parse / compile errors carry a span and get an underlined caret matching the span's width:

```
error[parse]: unexpected token `:=`
 --> /tmp/p.tg:2:6
  |
2 | y := := 7;
  |      ^^
```

Errors inside an imported file render against THAT file's source — the import dispatcher registers each imported source so the renderer can find it. REPL lines register as `<repl:N>` so the same machinery works at the prompt.

---

## REPL

Running `tigr` with no script enters an interactive session:

```
$ tigr
tigr> x := 5
5
tigr> make_counter := fn() { n := 0; fn() { n = n + 1; n } }
<fn>
tigr> c := make_counter()
<fn>
tigr> c()
1
tigr> c()
2
tigr> raise 'oops'
error[runtime]: oops
 --> <repl:6>:1
  |
1 | raise 'oops'
  |
tigr> c()
3
tigr> :q
```

Bindings persist across lines. Closures share upvalue cells, so mutating an outer name is visible through closures defined either earlier or later. An uncaught raise prints the error but the session continues with state intact. Multi-line input is supported when the parser sees `{`/`(`/`[`/`'` left open. `:quit` / `:q` (or Ctrl+D) exits; Ctrl+C abandons the current line.

The REPL uses [`rustyline`](https://github.com/kkawakam/rustyline) for input, so `←`/`→` move the cursor, `↑`/`↓` walk history (one entry per accepted line), and the usual Emacs-style edit keys (Ctrl+A, Ctrl+E, Ctrl+W, ...) work. History is persisted to `~/.tigr_history` across sessions.

---

## Worked examples

A real v0.3 script — count word frequencies in a file:

```
IO := import 'IO';
Os := import 'Os';
Array := import 'Array';
String := import 'String';

path := Os.args[2];
text := try IO.read_file(path) catch (e) {
    IO.eprint('error:', e);
    Os.exit(1)
};

words := text
    |> String.trim()
    |> String.lower()
    |> String.split(' ')
    |> Array.filter(fn(w) { #w > 0 });

counts := ${};
for (w, words) {
    counts[w] = (counts[w] || 0) + 1
};
counts
```

And Project Euler #4 — the "everything is an expression" showpiece, largest palindrome made from the product of two 3-digit numbers:

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

More examples in [`examples/v02/`](examples/v02/) (v0.2 features) and [`examples/v03/`](examples/v03/) (errors, modules, stdlib).

---

## Operator precedence

Low to high, with associativity:

| Level | Operators                                       | Assoc |
|-------|-------------------------------------------------|-------|
| 1     | `=` `:=` `+=` `-=` `*=` `/=` `%=`               | right |
| 2     | `\|\|`                                          | left  |
| 3     | `&&`                                            | left  |
| 4     | `==` `!=` `<` `>` `<=` `>=`                     | left  |
| 5     | `\|` (bitwise OR)                               | left  |
| 6     | `^` (bitwise XOR)                               | left  |
| 7     | `&` (bitwise AND)                               | left  |
| 8     | `\|>`                                           | left  |
| 9     | `..` `..=` (with optional `:step`)              | n/a   |
| 10    | `<<` `>>`                                       | left  |
| 11    | `+` `-`                                         | left  |
| 12    | `*` `/` `%`                                     | left  |
| 13    | `^^` (exponentiation)                           | right |
| 14    | unary `-` `!` `#` `~`                           | n/a   |
| 15    | call `f(...)`, index `a[i]`, member `a.b`       | left  |

---

## Status

**v0.6 is feature-complete.** 351 tests pass. On top of v0.5, v0.6 adds:

1. **`continue`** — skip the rest of a loop iteration; the iteration contributes `null`. Now a reserved keyword.
2. **Default parameter values** — `fn(a, b = 10)`; the default fills in when the argument slot is `null`.
3. **Wider standard library** — filesystem operations on `IO` (`list_dir`, `mkdir`, `remove`, `is_dir`, `is_file`, `stat`); a new `Path` module; subprocess execution via `Os.run`; an `Object` source-stdlib module; and a `DateTime` module (UTC calendar date/time).

Earlier releases:

- **v0.5**: `type()` built-in, bitwise operators (`& | ^ ~ << >>`; `^` became XOR, `^^` is power), `match` expression with refutable patterns.
- **v0.4**: rendered errors with source snippets, extended number literals (`0xFF`/`1e6`/`.5`/`_`), patterns on `=` + mid-expression decls, `JSON` module.
- **v0.3**: `try`/`catch`/`raise`, module caching + bare-name dispatch, native modules (`IO`/`Os`/`Time`), source-stdlib (`Array`/`String`/`Math`), interactive REPL.
- **v0.2**: bytecode VM, closures with Lox-style upvalues, first-class ranges, destructuring patterns, pipe `|>`, spread `...`, string interpolation.

See [`LANGUAGE.md`](LANGUAGE.md) for the authoritative spec. The v0.1 tree-walking interpreter source lives under `src/v01/` for reference; it's not currently wired into the build.

Known limitations / candidates for later:

- No tracing GC — collections use `Rc<RefCell<...>>`, so reference cycles leak. Acceptable for hobby-scale data.
- `JSON.stringify` doesn't detect cyclic arrays/objects — they overflow the call stack.
- Runtime error spans are line-only (no column information). Lex/parse/compile errors carry full spans.
- No regex; `String.contains` / `split` / `replace` cover the 80%.
