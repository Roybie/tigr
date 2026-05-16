# tigr

A small dynamic language where **everything is an expression**. Tigr is built around the idea that every construct — assignments, blocks, conditionals, loops, even `break`, `return`, and `raise` — produces a value. There are no statements.

This README documents **v0.10**: the v0.9 standard-library release plus a tracing **garbage collector** that replaces the reference-counted memory model, so reference cycles are reclaimed instead of leaked. The complete language spec lives in [`LANGUAGE.md`](LANGUAGE.md); this is the friendlier tour. See [Status](#status) for the per-release history.

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
./target/release/tigr test                             # discover and run tests
./target/release/tigr test path/                       # tests under a path
```

When a script finishes, its final value is printed. So `1 + 1` as a one-line file produces `2`. With no argument, tigr drops into a REPL — see [REPL](#repl) below.

`tigr test` discovers test files — any `*_test.tg` file, plus every `.tg` file under a `tests/` directory — runs them, and reports pass/fail counts. See the [`Test`](#test-v09) module for writing tests.

There are working examples under [`examples/v02/`](examples/v02/) organised by build phase, plus Project Euler solutions in [`examples/v02/euler/`](examples/v02/euler/). v0.3 demos are in [`examples/v03/`](examples/v03/), v0.4 demos in [`examples/v04/`](examples/v04/), v0.5 demos in [`examples/v05/`](examples/v05/), v0.7 demos in [`examples/v07/`](examples/v07/), v0.8 demos in [`examples/v08/`](examples/v08/), v0.9 demos in [`examples/v09/`](examples/v09/), and v0.10 demos in [`examples/v10/`](examples/v10/).

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
| `Map`    | `Map.new()` *(v0.9)*                          | Arbitrary-keyed dictionary, reference type |
| `Set`    | `Set.new([1, 2, 3])` *(v0.9)*                 | Collection of unique values, reference type |
| `Range`  | `0..10`, `0..=10`, `10..0:-1`                 | First-class lazy iterable                |
| `Function` | `fn(x) { x * 2 }`                           | Closures over lexical environment        |

Underscores are allowed only between digits — `_5`, `5_`, `5__5`, and `0x_FF` are all rejected. A trailing `5.` lexes as `Int(5)` followed by `Dot` so `5.method` style member access still works.

`Array`, `Object`, `Map`, and `Set` are **reference types** — passing them around shares the same underlying value.

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
arr[0] = 99;                         // mutates in place

arr + 4;                             // a NEW array [99, 2, 3, 4]   (append element)
arr + [4, 5];                        // a NEW array, the two concatenated
```

`Array + Array` concatenates, `Array + value` appends a single element — and `+` always builds a **fresh** array, leaving its operands untouched.

`+=` instead grows the array **in place**. It mirrors `+` (an array right-hand side extends; anything else appends one element), but mutates rather than rebinds — so every alias of the array sees the change, consistent with how `arr[i] = v` already works:

```
a := [1, 2, 3];
b := a;
a += 4;                              // a AND b are now [1, 2, 3, 4]
a += [5, 6];                         // extends in place → [1, 2, 3, 4, 5, 6]
```

To append an array as a *single* element rather than extend, use `Array.push(arr, [1, 2])`. The `Array` module also has `extend` for in-place bulk append; both are O(1)-amortized, where building an array with repeated `arr = arr + x` is O(n²).

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

Iterates a Range, Array, Object, Map, Set, String, or iterator object. One-variable or two-variable form:

| Iterable | One-var          | Two-var                              |
|----------|------------------|--------------------------------------|
| Range    | `for (i, 0..10)` | `for (n, i, 0..10)`   (`n` = 0,1,2…) |
| Array    | `for (x, arr)`   | `for (i, x, arr)`                    |
| Object   | `for (v, obj)`   | `for (k, v, obj)`                    |
| Map      | `for (v, map)`   | `for (k, v, map)`                    |
| Set      | `for (x, set)`   | `for (i, x, set)`     (`i` = 0,1,2…) |
| String   | `for (ch, str)`  | `for (i, ch, str)`                   |
| Iterator | `for (v, it)`    | `for (i, v, it)`      (`i` = 0,1,2…) |

```
last := for (x, [10, 20, 30]) { x };       // 30
all  := for[] (i, 1..=5) { i * i };        // [1, 4, 9, 16, 25]
```

An **iterator object** (an object with a callable `next` field — the `Iter` protocol) is driven by calling `next()`; a `for` can consume an `Iter` pipeline directly, no `Iter.collect()` needed (v0.8). The same applies to array and call spread: `[...it]` and `f(...it)` expand an iterator. An object *without* a callable `next` still iterates as key/value entries.

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

Recoverable errors. `raise expr` aborts the current evaluation, carrying `expr`'s value — **any** value, not just a string. `try expr` evaluates `expr` and yields its value on success, or `null` on a raised/runtime error. `try expr catch (e) { handler }` runs the handler with the raised value bound to `e`. All three are expressions.

`catch` binds **exactly what was raised** — `raise 'msg'` gives `e` a string, `raise ${...}` gives `e` that object. A **built-in** runtime error (division by zero, type mismatch, calling a non-function, a failed import...) is instead reified into an object `${kind, message, line}`, so a handler can `match` on `e.kind`:

```
content := try IO.read_file('config.tg') catch (e) {
    print('warning:', e);
    ''
};

n := try int(input) || 0;       // null on parse failure → 0

result := try risky() catch (e) {
    match e.kind {
        'div_by_zero'   => 0,
        'type_mismatch' => raise e,        // not ours — re-raise it
        _               => null,
    }
};

raise ${kind: 'db_down', detail: 'connection lost'}
```

`e.kind` is a stable snake-case string — one of `div_by_zero`, `type_mismatch`, `index_out_of_bounds`, `arity_mismatch`, `not_callable`, `invalid_index_type`, `invalid_key_type`, `immutable_target`, `import_failed`, `overflow`, `stack_overflow`, `stack_underflow`, `cycle`. `e.message` is the human-readable text an uncaught error would print, and `e.line` is the source line. Native stdlib modules (`Math`, `IO`, `JSON`, ...) raise plain **string** messages, so `catch` binds those as strings — except `JSON.stringify` of a circular structure, which raises a structured `cycle` error. An uncaught raised value is rendered via `str()` in the error report.

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

- **Bare names** (no `/`, `\`, or `.`): resolved against the bundled stdlib and native-module registry. `Array`, `Iter`, `String`, `Math`, `Object`, `Map`, `Set`, `Test` are tigr-source modules; `IO`, `Os`, `Time` are native. Unknown names raise.
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
| `rand`  | `rand() -> Float`          | Uniform in `[0, 1)`; seed it via `Random.seed` |
| `type`  | `type(x) -> String`        | Name of the value's type (`'int'`, `'array'`, `'function'`, ...) |
| `gc`    | `gc() -> Object` *(v0.10)* | Garbage-collector counters: `${live, collections, allocated, freed}` |

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

Bundled modules, imported with `import 'Name'`. Each entry below gives its full signature, return value, and whether it raises. `Array`, `Iter`, `String`, `Math`, `Object`, `Map`, `Set`, and `Test` are tigr-source modules; `IO`, `Path`, `Os`, `Time`, `DateTime`, `JSON`, and `Random` are native (Rust-backed). All `raise`d errors are catchable with `try` / `catch`.

### `Array`

A tigr-source module. Several functions take a **callback** — a function value you supply, which the module calls for you. These are the parameters named `func`, `pred`, and `key` in the table below. Unless a row's description says otherwise, the callback is invoked as `callback(element, index, whole_array)`; since tigr drops extra arguments, declare it with only the parameters you need — `fn(x)`, `fn(x, i)`, or `fn(x, i, arr)` all work. (`reduce`, `create`, and `sort_by` use different callback signatures — see their rows.) Most of these are pure tigr and raise only when an operation they perform does (e.g. `sum` on non-numbers); the in-place mutators (`push`, `extend`, `pop`, `shift`, `unshift`, `insert`, `remove`, `clear`) are native-backed and raise on a non-array argument.

| Function | Returns | Behavior |
|---|---|---|
| `push(arr, value)` | `Array` | Append `value` to `arr` **in place** (O(1) amortized); returns `arr` |
| `extend(arr, other)` | `Array` | Append every element of `other` to `arr` **in place**; returns `arr` |
| `pop(arr)` | value `\| null` | Remove and return the last element **in place**; `null` if empty |
| `shift(arr)` | value `\| null` | Remove and return the first element **in place**; `null` if empty |
| `unshift(arr, value)` | `Array` | Prepend `value` to `arr` **in place**; returns `arr` |
| `insert(arr, index, value)` | `Array` | Insert `value` at `index` **in place** (negative counts from the end, clamped to `0..#arr`); returns `arr` |
| `remove(arr, index)` | value `\| null` | Remove and return one element at `index` **in place** (negative counts from the end); `null` if out of range |
| `remove(arr, start, count)` | `Array` | Remove and return `count` elements from `start` **in place**, as a new array |
| `clear(arr)` | `Array` | Remove every element **in place**; returns `arr` |
| `create(len, func)` | `Array` | Length-`len` array; element `i` is `func(i)` |
| `concat(a, b)` | `Array` | A fresh array — `a` and `b` concatenated, neither mutated |
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
| `head(arr, n)` | `Array` | First `n` elements; a negative `n` counts from the end (`-1` ⇒ all but the last) |
| `tail(arr, n)` | `Array` | Last `n` elements; a negative `n` counts from the start (`-1` ⇒ all but the first) |
| `take(arr, n)` | `Array` | First `n` elements (`n` clamped to `0..#arr`) |
| `drop(arr, n)` | `Array` | All but the first `n` elements |
| `slice(arr, start, end)` | `Array` | Elements `[start, end)`; out-of-range bounds clamp |
| `sum(arr)` | `Number` | Sum of elements (`0` if empty) |
| `max_of(arr)` | value `\| null` | Largest element, or `null` if empty |
| `min_of(arr)` | value `\| null` | Smallest element, or `null` if empty |
| `uniq(arr)` | `Array` | First-seen unique elements, order preserved |
| `zip(a, b)` | `Array` | Pairwise `[a[i], b[i]]`; length is `min(#a, #b)` |
| `join(arr, sep)` | `String` | `str()` each element, joined by `sep` |
| `group_by(arr, key)` | `Map` | Group elements into a `Map` keyed by `key(elem)`; each value is the array of matching elements |
| `chunk(arr, size)` | `Array` | Split into consecutive `size`-long sub-arrays (last may be shorter) |
| `windows(arr, size)` | `Array` | Every contiguous `size`-long sub-array (sliding window) |
| `partition(arr, pred)` | `Array` | `[matching, non_matching]` — elements split by `pred` |
| `flat_map(arr, func)` | `Array` | `map` by `func`, then flatten one level |
| `count_of(arr, pred)` | `Int` | Count of elements where `pred` is truthy |
| `sort(arr)` | `Array` | Ascending order, comparing elements directly (insertion sort) |
| `sort_by(arr, key)` | `Array` | Ascending order, but comparing `key(element)` instead of the elements themselves. `key` is a callback applied to each element to derive the value to sort on — use it to sort by a field or a computed property |

```
Array := import 'Array';
Array.sum(Array.filter([1, 2, 3, 4, 5], fn(x) { x % 2 == 0 }))   // 6

// sort_by: the `key` callback maps each element to the value to sort on.
people := [${name: 'Cy', age: 30}, ${name: 'Al', age: 25}];
Array.sort_by(people, fn(p) { p.age })
// [${name: 'Al', age: 25}, ${name: 'Cy', age: 30}]   — ordered by age

// sort a list of words by length
Array.sort_by(['ccc', 'a', 'bb'], fn(w) { #w })       // ['a', 'bb', 'ccc']
```

### `Iter` (v0.7)

A tigr-source module of **lazy, pull-based iterators**. Where `Array.map` followed by `Array.filter` builds a complete intermediate array at every step, an `Iter` pipeline carries one element through the whole chain at a time and never materializes the in-between arrays — which also makes infinite sequences and short-circuiting possible.

An iterator is an object `${next: fn()}`; each `next()` call returns `${done: true}` or `${done: false, value: v}`. The **adapters** create an iterator, the **combinators** wrap one lazily (no work runs until the result is pulled), and the **consumers** drive the pulling and force evaluation. Callback parameters (`func`, `pred`) are invoked as `callback(value)`.

Since v0.8, `for` and spread (`[...it]`, `f(...it)`) consume an iterator object directly — `collect` is only needed when you specifically want an `Array` value.

`count` and `repeat` are infinite — only pair them with a bounding combinator (`take`) or a short-circuiting consumer (`find` / `nth`).

| Function | Returns | Behavior |
|---|---|---|
| `from(iterable)` | `Iterator` | Wrap an Array / Range / String as an iterator |
| `count(start)` | `Iterator` | Infinite — `start, start+1, start+2, ...` |
| `repeat(value)` | `Iterator` | Infinite — `value` forever |
| `map(it, func)` | `Iterator` | Lazily apply `func` to each element |
| `filter(it, pred)` | `Iterator` | Lazily keep elements where `pred` is truthy |
| `take(it, n)` | `Iterator` | Yield at most the first `n` elements, then stop |
| `drop(it, n)` | `Iterator` | Skip the first `n` elements, yield the rest |
| `enumerate(it)` | `Iterator` | Yield `[index, value]` pairs, index from `0` |
| `zip(a, b)` | `Iterator` | Yield `[a_elem, b_elem]` pairs; done when either side is |
| `chain(a, b)` | `Iterator` | Yield every element of `a`, then every element of `b` |
| `collect(it)` | `Array` | Drain the iterator into a fresh array |
| `reduce(it, func, seed)` | value | Left fold — `func(acc, elem)` over each element, from `seed` |
| `for_each(it, func)` | `null` | Call `func(elem)` on each element, for side effects |
| `count_of(it)` | `Int` | Number of elements the iterator yields |
| `find(it, pred)` | value `\| null` | First element where `pred` is truthy, or `null` |
| `nth(it, n)` | value `\| null` | The 0-indexed `n`th element, or `null` if shorter |

```
Iter := import 'Iter';

// A lazy pipeline — no intermediate array is ever built:
[1, 2, 3, 4, 5]
  |> Iter.from()
  |> Iter.map(fn(n) { n * n })
  |> Iter.filter(fn(n) { n > 4 })
  |> Iter.collect()                  // [9, 16, 25]

// An infinite sequence, bounded by take:
0 |> Iter.count() |> Iter.map(fn(n) { n * n }) |> Iter.take(5) |> Iter.collect()
// [0, 1, 4, 9, 16]

// Short-circuits — stops pulling at the first match:
0 |> Iter.count() |> Iter.find(fn(n) { n > 100 })   // 101
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
| `format(value, spec)` *(v0.9)* | `String` | Render `value` through the spec mini-language (see below) |
| `printf(template, args)` *(v0.9)* | `String` | Fill `%(SPEC)` placeholders in `template` from `args` |

#### The format spec mini-language

`format` and `printf` (v0.9) share one spec mini-language. Every part is
optional; they must appear in this order:

```
spec := [[fill]align][sign]['#'][width][','][.precision][type]
```

**`fill`** — the character used to pad a value out to `width`. Defaults
to a space. A character is only read as `fill` when an `align` character
immediately follows it, so `*>8` fills with `*` but `>8` fills with
spaces.

**`align`** — which side the value sits on inside a `width` field:

- `<` — left-align (the value sits at the left, padding on the right)
- `>` — right-align (padding on the left)
- `^` — centre (padding split both sides; an odd remainder goes right)

Without `align`, numbers default to right-align and everything else to
left-align.

**`sign`** — only `+` is accepted. It forces a leading `+` on positive
numbers; negatives always show `-`, with or without this flag.

**`#`** — alternate form. Adds the base prefix `0x`, `0o`, or `0b` in
front of an `x`/`X`, `o`, or `b` value. Ignored for other types.

**`width`** — the minimum field width, in characters. A shorter value is
padded to this width; a longer value is never truncated by `width`. A
*bare leading `0`* (with no explicit `fill`+`align`) means zero-pad — the
zeros go between the sign/base-prefix and the digits, e.g. `format(-7,
'05')` is `'-0007'`.

**`,`** — group the integer part into thousands with commas, e.g.
`1234567` becomes `1,234,567`. Ignored for non-decimal types.

**`.precision`** — a `.` followed by digits. For floats it sets the
number of decimal places (`f` and `e` default to 6); for strings it
truncates to that many characters; for integers it is ignored.

**`type`** — how to interpret and render the value. If omitted, the
value is rendered in its natural form (what `str()` would give) while
still honouring the rest of the spec.

- `s` — string. Requires a `String` value; raises otherwise.
- `d` — decimal integer.
- `f` — fixed-point float, e.g. `3.14`.
- `e` — float in scientific notation with a lowercase `e`, e.g. `1.2e3`.
- `E` — scientific notation with an uppercase `E`.
- `x` — hexadecimal, lowercase digits (`ff`).
- `X` — hexadecimal, uppercase digits (`FF`).
- `b` — binary (`1010`).
- `o` — octal (`17`).

The numeric types (`d f e E x X b o`) require a number. `d`, `x`, `X`,
`b`, and `o` need an integer — an integral float like `3.0` is accepted,
but a fractional one like `3.5` raises. A mismatched type code or an
unparsable spec always raises.

`printf` placeholders are `%(SPEC)` — the marker is `%(...)`, not `{}`,
because `{}` is already string interpolation. Each placeholder consumes
the next element of `args`; `%%` is a literal percent; passing too few
or too many `args` raises.

```
S := import 'String';
S.split('a,b,c', ',') |> S.join('-')      // 'a-b-c'
S.format(42, '05')                        // '00042'   (zero-pad)
S.format(3.14159, '.2f')                  // '3.14'    (fixed-point)
S.format('hi', '^8')                      // '   hi   '(centre)
S.format(255, '#x')                       // '0xff'    (hex, prefixed)
S.format(1234567, ',d')                   // '1,234,567'
S.printf('%(<6)%(>6.2f)', ['tea', 1.5])   // 'tea     1.50'
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

As of v0.9 `has` is O(1) (it was an O(n) key scan), and `keys` / `values` / `entries` build their result in O(n) rather than O(n²).

### `Map` (v0.9)

A tigr-source module (backed by native `_NativeMap` primitives) exposing the `Map` type — an arbitrary-keyed, insertion-ordered dictionary. Unlike `Object`, whose keys are strings only, a `Map` key may be any `null` / `bool` / `int` / `string` value; a `Float` or collection key raises `invalid_key_type`. Read and write entries with `m[key]` / `m[key] = value`, take the count with `#m`, and iterate with `for (k, v, m)`. `type(m)` is `'map'`. A `Map` is not JSON-serializable.

| Function | Returns | Behavior |
|---|---|---|
| `new(source?)` | `Map` | Empty map; or copy an `Object`'s entries; or build from an array of `[key, value]` pairs |
| `get(m, key)` | value | The entry's value, or `null` if absent (same as `m[key]`) |
| `set(m, key, value)` | `Map` | Insert / overwrite in place; returns `m` (same as `m[key] = value`) |
| `has(m, key)` | `Bool` | True if `key` is present — O(1), distinguishes a missing key from a `null` value |
| `delete(m, key)` | `Bool` | Remove `key`; true if it was present |
| `keys(m)` / `values(m)` / `entries(m)` | `Array` | Keys / values / `[key, value]` pairs in insertion order |
| `size(m)` | `Int` | Entry count (same as `#m`) |
| `clear(m)` | `Map` | Remove every entry in place; returns `m` |

```
Map := import 'Map';
m := Map.new();
m[1] = 'one';                  // int key
m['1'] = 'string';             // distinct string key
[m[1], m['1'], Map.has(m, 2)]  // ['one', 'string', false]
```

### `Set` (v0.9)

A tigr-source module (backed by native `_NativeSet` primitives) exposing the `Set` type — an insertion-ordered collection of unique values. Elements share `Map`'s key restriction (`null` / `bool` / `int` / `string`). Test membership with `s[x]` (writing `s[x] = ...` is an error); take the count with `#s`; iterate with `for (x, s)`. `type(s)` is `'set'`. Not JSON-serializable.

| Function | Returns | Behavior |
|---|---|---|
| `new(array?)` | `Set` | Empty set; or build from an array, collapsing duplicates |
| `add(s, x)` | `Set` | Insert `x` in place; returns `s` |
| `has(s, x)` | `Bool` | True if `x` is a member (same as `s[x]`) |
| `delete(s, x)` | `Bool` | Remove `x`; true if it was present |
| `items(s)` | `Array` | Elements in insertion order |
| `size(s)` | `Int` | Element count (same as `#s`) |
| `clear(s)` | `Set` | Remove every element in place; returns `s` |
| `union(a, b)` | `Set` | Fresh set: every element of either |
| `intersection(a, b)` | `Set` | Fresh set: elements in both |
| `difference(a, b)` | `Set` | Fresh set: `a`'s elements not in `b` |

```
Set := import 'Set';
a := Set.new([1, 2, 3]);
b := Set.new([2, 3, 4]);
Set.items(Set.intersection(a, b))   // [2, 3]
```

### `Test` (v0.9)

A tigr-source module — a small test framework written in tigr itself. The **assertions** `raise` on failure (so they work standalone, anywhere), and `case` / `suite` group tests as plain data. `assert_eq` compares with `==`, which is structural for arrays and objects. `assert_raises` runs `thunk` and fails unless it raised; pass a `kind` to also require a specific error — a reified built-in error's `kind` field, or the raised value itself otherwise — and it returns the caught error.

| Function | Returns | Behavior |
|---|---|---|
| `assert(cond, msg?)` | `true` | Raise `msg` (default `'assertion failed'`) unless `cond` is truthy |
| `assert_eq(actual, expected, msg?)` | `true` | Raise unless `actual == expected`; the message shows both |
| `assert_ne(a, b, msg?)` | `true` | Raise unless `a != b` |
| `assert_raises(thunk, kind?)` | the caught error | Run `thunk`; raise unless it raised. With `kind`, the raised error must match |
| `fail(msg?)` | — | Raise unconditionally (default `'explicit failure'`) |
| `case(name, func)` | `Object` | Package an unrun test — `${name, func}` |
| `suite(name, cases)` | `Object` | Run an array of `case`s; print PASS/FAIL lines + a tally; return `${name, passed, failed, total, failures}` |

```
Test := import 'Test';

Test.suite('arithmetic', [
    Test.case('adds', fn() { Test.assert_eq(1 + 1, 2) }),
    Test.case('div zero raises', fn() {
        Test.assert_raises(fn() { 1 / 0 }, 'div_by_zero')
    }),
])
```

Run this file directly, or via `tigr test` — which discovers every `*_test.tg` file (and every `.tg` file under a `tests/` directory), runs each, sums the `suite` results a file's final expression yields, and exits non-zero if any test failed.

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

JSON's number model is "all numbers are IEEE 754 doubles", so `JSON.parse(JSON.stringify(123))` returns `Float(123.0)`, not `Int(123)`. On the way out, `Int` writes plain digits and an integer-valued `Float` keeps a `.0` suffix. `JSON.stringify` of a circular structure raises a catchable `cycle` error; a non-cyclic shared subtree still serializes fine.

### `Random` (v0.9)

A native module for seedable pseudo-random numbers. `Random` and the bare `rand()` built-in draw from a single per-thread PRNG stream, so `Random.seed(n)` makes `rand()` reproducible too — pin a seed in a test and the draws are deterministic. Until `seed` is called the stream is auto-seeded from the wall clock.

| Function | Returns | Behavior |
|---|---|---|
| `seed(n)` | `null` | Pin the stream to the `Int` `n` (any value works, `0` included) |
| `float()` | `Float` | Uniform in `[0, 1)` |
| `int(lo, hi)` | `Int` | Uniform in the **inclusive** range `[lo, hi]`; raises if `lo > hi` |
| `bool()` | `Bool` | `true` or `false`, each with probability ½ |
| `choice(arr)` | value | A uniformly random element of a non-empty array; raises if empty |
| `range(r)` | `Int` | A uniformly random element of a non-empty range, honouring its step |
| `shuffle(arr)` | `Array` | A **new** array with `arr`'s elements reordered; the input is untouched |

```
Random := import 'Random';
Random.seed(42);
roll := fn() { Random.int(1, 6) };
[roll(), roll(), Random.choice(['a', 'b', 'c']), Random.range(0..=8:2)]
```

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

Since v0.8, an error that escapes every `try` also prints a **stack trace** beneath the snippet — each active call frame, innermost first:

```
$ tigr examples/v08/stack_trace.tg
error[runtime]: integer overflow
 --> examples/v08/stack_trace.tg:6
  |
6 |     n * n * n * n * n * n * n * n * n * n  // overflows i64 for big n
  |
stack trace (most recent call first):
  inner at examples/v08/stack_trace.tg:6
  compute at examples/v08/stack_trace.tg:10
  <main> at examples/v08/stack_trace.tg:13
```

Frame names are inferred from the binding (`f := fn(){}` → `f`), with `<anonymous>` for an unbound `fn` and `<main>` for the top-level program. Tail calls reuse their frame, so a tail-recursive function appears once; a single-frame error prints no trace.

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

## Garbage collection

The mutable, potentially-cyclic value types — `Array`, `Object`, `Map`,
`Set`, iterators, and the cells closures capture — live on a heap
managed by a tracing **mark-sweep** garbage collector (v0.10). Earlier
releases reference-counted these, which leaks any structure that points
back at itself; a tracing collector reclaims cycles like any other
garbage:

```
node := ${value: 1, next: null};
node.next = node;            // a cycle — node points at itself
// `node` going out of scope is enough; the collector reclaims it.
```

Collection is automatic. It runs at safe points between bytecode
instructions, once the live-object count crosses a growing threshold —
you never call it by hand. The `gc()` built-in is a read-only window on
the collector for tests and tuning:

```
stats := gc();
print(stats.live, stats.collections, stats.allocated, stats.freed);
```

`Str`, `Range`, and functions' immutable templates stay reference-counted
— they are acyclic, so a count reclaims them correctly and the collector
need not trace them.

---

## Status

**v0.10 is feature-complete.** 542 tests pass. v0.10 replaces the reference-counted memory model with a tracing **garbage collector**:

- **Tracing mark-sweep GC** — the mutable, potentially-cyclic value types (`Array`, `Object`, `Map`, `Set`, iterators, and closure upvalue cells) are managed by a mark-sweep collector over a per-thread heap; a `Value` holds a small generation-tagged handle into it. Reference cycles — a self-referential object (`o.link = o`), two closures capturing each other — are now **reclaimed** rather than leaked forever, which a reference count could never do. Collection is automatic, running at VM safepoints once the heap crosses a size threshold; the `gc()` built-in returns the collector's counters as `${live, collections, allocated, freed}`.

Earlier releases:

- **v0.9**: a `Test` framework + `tigr test` runner (written in tigr itself), the `Map` / `Set` collection types, the seedable `Random` module, more `Array` combinators (`group_by`/`chunk`/`windows`/`partition`/`flat_map`, in-place removal), and `String` formatting (`format` / `printf` with a width/precision/alignment spec mini-language).

- **v0.8**: integer-overflow checks (a catchable `overflow` error), tail calls + bounded recursion, stack traces on uncaught errors, `JSON.stringify` cycle detection, and `for` / spread consuming iterator objects directly.
- **v0.7 / v0.7b**: lazy `Iter` iterators (pipelines that never materialize intermediate arrays), in-place array growth (`Array.push` / `extend`, and a mutating `+=`), and structured errors — `catch` binds the exact raised value, and built-in errors reify to a `${kind, message, line}` object.
- **v0.6**: `continue` keyword, default parameter values (`fn(a, b = 10)`), and a wider standard library — `IO` filesystem ops, a `Path` module, `Os.run` subprocesses, an `Object` module, and a UTC `DateTime` module.
- **v0.5**: `type()` built-in, bitwise operators (`& | ^ ~ << >>`; `^` became XOR, `^^` is power), `match` expression with refutable patterns.
- **v0.4**: rendered errors with source snippets, extended number literals (`0xFF`/`1e6`/`.5`/`_`), patterns on `=` + mid-expression decls, `JSON` module.
- **v0.3**: `try`/`catch`/`raise`, module caching + bare-name dispatch, native modules (`IO`/`Os`/`Time`), source-stdlib (`Array`/`String`/`Math`), interactive REPL.
- **v0.2**: bytecode VM, closures with Lox-style upvalues, first-class ranges, destructuring patterns, pipe `|>`, spread `...`, string interpolation.

See [`LANGUAGE.md`](LANGUAGE.md) for the authoritative spec. The v0.1 tree-walking interpreter source lives under `src/v01/` for reference; it's not currently wired into the build.

Known limitations / candidates for later:

- Runtime error spans are line-only (no column information). Lex/parse/compile errors carry full spans.
- No regex; `String.contains` / `split` / `replace` cover the 80%.
