# Tigr Language Specification

Version 0.2 (draft) — written as the target for a bytecode VM implementation.

This spec describes Tigr after the eight design changes agreed in the design
discussion. Where the existing 0.1 implementation differs, this document is
authoritative.

---

## 1. Philosophy

- **Everything is an expression.** No statements. Every construct produces a
  value (possibly `null`).
- **Loops come in pairs.** Plain forms (`for`, `while`) yield the value of the
  last iteration. Array forms (`for[]`, `while[]`) yield an array of the
  per-iteration values, with `null` values filtered out.
- **Concise but consistent.** Prefer one short syntax over two. Promote
  patterns into syntax only when they recur in real programs.
- **Dynamic and small.** Closures, first-class functions, dynamic typing. No
  type annotations, no class system, no inheritance.

---

## 2. Lexical structure

### 2.1 Comments

```
// single-line comment
/* multi-line
   comment */
```

### 2.2 Identifiers

Start with a letter or `_`, continue with letters, digits, or `_`. Case
sensitive. Keywords are reserved and cannot be used as identifiers.

### 2.3 Keywords

```
fn  if  else  for  while  break  return  import  try  catch  raise
match  null  true  false
```

Note: `floor`, `ceil`, `rand`, `for[]`, `while[]` are no longer keywords — see
§13. The `[]` suffix on `for`/`while` is now a separate token.

### 2.4 Operators and punctuation

Arithmetic: `+ - * / % ^^`   (`^^` is exponentiation)
Bitwise:    `& | ^ ~ << >>`  (`^` is XOR; all are Int-only)
Comparison: `== != < > <= >=`
Logical:    `&& || !`
Assignment: `= := += -= *= /= %=`
Pipe:       `|>`
Range:      `.. ..=`
Spread:     `...`
Length:     `#`
Member:     `.`
Match arm:  `=>`
Other:      `( ) { } [ ] , ; : $`

(Change from v0.4: `^` was exponentiation; it is now bitwise XOR, and
exponentiation moved to `^^`.)

### 2.5 Number literals

```
42        // Int (decimal)
3.14      // Float
0         // Int
0.0       // Float

0xFF      // Int (hex; case-insensitive prefix)
0b1010    // Int (binary)
0o755     // Int (octal)

1_000_000 // Underscore separators between digits
3.141_592 // Allowed in fractional part too
0xFF_FF   // …and inside hex/bin/oct

1e6       // Scientific — always Float
2.5e-3    // …with optional sign and fractional mantissa
.5        // Leading-dot float (≡ 0.5)
```

Underscores are allowed only **between** digits — `_5`, `5_`, `5__5`,
and `0x_FF` are all rejected. A trailing dot like `5.` is **not** a
float literal; it lexes as `Int(5)` followed by `Dot`, which is what
makes `5.method()` style member access work.

Numeric literals that don't fit in `i64` (e.g. `0xFFFFFFFFFFFFFFFF`)
are a lex error.

### 2.6 String literals

Single-quoted, with `{expr}` interpolation. See §8.

```
'hello'
'count: {n}'
'literal brace: \{'
```

---

## 3. Types

| Type      | Examples                                 | Notes                              |
|-----------|------------------------------------------|------------------------------------|
| `Int`     | `0`, `42`, `-7`                          | 64-bit signed                      |
| `Float`   | `3.14`, `0.0`                            | 64-bit IEEE-754                    |
| `String`  | `'hello'`                                | Immutable; UTF-8                   |
| `Bool`    | `true`, `false`                          |                                    |
| `Null`    | `null`                                   |                                    |
| `Array`   | `[1, 'two', true]`                       | Heterogeneous, **reference type**  |
| `Object`  | `${ name: 'a', age: 1 }`                 | String keys, **reference type**    |
| `Range`   | `0..10`, `0..=10`, `0..10:2`             | First-class iterable               |
| `Function`| `fn(x) { x * 2 }`                        | Closures over lexical env          |

`Int` and `Float` are jointly referred to as **Number**. Mixed-arithmetic
between them follows §6.2.

`Array` and `Object` are **reference types**: passing them to a function or
binding them to a new name does not copy. This is a change from 0.1.

---

## 4. Bindings and scope

### 4.1 Declaration vs assignment

```
foo := 10        // declare 'foo' in the current scope
foo = 20         // assign to the existing 'foo'
bar = 5          // ERROR: 'bar' is not declared
```

- `:=` introduces a new binding in the **current scope**, shadowing any
  outer binding of the same name.
- `=` assigns to the nearest enclosing binding of that name. It is an error
  if no such binding exists.
- Compound assignments (`+=`, `-=`, `*=`, `/=`, `%=`) require an existing
  binding (same rule as `=`).

Both `:=` and `=` are expressions and evaluate to the assigned value.

### 4.2 Implicit declarations

These positions implicitly declare a new binding in their inner scope:

- Function parameters
- `for` iteration variables (the index and/or element variable)
- Names introduced by destructuring patterns on the LHS of `:=`

### 4.3 Block and scope

A **block** is a `;`-separated sequence of expressions, optionally with a
trailing `;`. Its value is the last expression's value, or `null` if the
sequence ends in `;`.

```
(a := 1; b := a + 1; b * 2)   // == 4
(a := 1; b := 2;)             // == null
```

A **scope** is a block surrounded by `{ }`. It opens a new lexical scope:
bindings introduced inside (`:=`, parameters, for-vars) are not visible
outside. Mutations to outer bindings persist.

```
a := 9;
b := { c := 20; c * (a = a + 1) };   // a == 10, b == 200, c is undefined here
```

### 4.4 Function scope and closures

Functions capture the lexical environment of their definition site. Captured
variables are by reference: mutating them inside a closure updates the
enclosing binding.

```
make_counter := fn() {
    n := 0;
    fn() { n = n + 1; n }
};
c := make_counter();
c();   // 1
c();   // 2
```

---

## 5. Truthiness

The following values are **falsy**:

- `false`
- `null`
- `0`
- `0.0`
- `''` (empty string)
- `[]` (empty array)
- `${}` (empty object)

Everything else is **truthy**, including all functions and all non-empty
ranges. (Change from 0.1: empty arrays and objects are now falsy.)

`!x` and boolean contexts (`if`, `while`, `&&`, `||`) use this rule.

`&&` and `||` short-circuit and return the **value** that decided the result
(not coerced to bool):

```
0 || 'fallback'    // == 'fallback'
'a' && 'b'         // == 'b'
null || []         // == []
```

(Change from 0.1: `&&`/`||` previously returned a `Bool`; they now return one
of their operands, like Lua/JavaScript.)

---

## 6. Expressions

### 6.1 Precedence (low to high)

| Level | Operators                                   | Assoc |
|-------|---------------------------------------------|-------|
| 1     | `=` `:=` `+=` `-=` `*=` `/=` `%=`           | right |
| 2     | `\|\|`                                      | left  |
| 3     | `&&`                                        | left  |
| 4     | `==` `!=` `<` `>` `<=` `>=`                 | left  |
| 5     | `\|`  (bitwise OR)                          | left  |
| 6     | `^`   (bitwise XOR)                         | left  |
| 7     | `&`   (bitwise AND)                         | left  |
| 8     | `\|>`                                       | left  |
| 9     | `..` `..=`  (with optional `:step`)         | n/a   |
| 10    | `<<` `>>`                                   | left  |
| 11    | `+` `-`                                     | left  |
| 12    | `*` `/` `%`                                 | left  |
| 13    | `^^`  (exponentiation)                      | right |
| 14    | unary `-` `!` `#` `~`                       | n/a   |
| 15    | call `f(...)`, index `a[i]`, member `a.b`   | left  |

### 6.2 Numeric arithmetic

- `Int op Int` → `Int`, except division: `n / m` is `Int` if it divides
  evenly, else `Float`.
- Any `Float` operand → `Float` result.
- `^^` (power) always produces `Float`.
- `%` follows the sign of the dividend.

### 6.2a Bitwise operators (v0.5)

`& | ^ << >>` (binary) and `~` (unary) operate only on `Int` — any
other operand type raises a catchable runtime error. `^` is bitwise
XOR; exponentiation is the separate `^^` operator. `>>` is an
arithmetic (sign-preserving) shift. A shift amount outside `0..64`
raises rather than wrapping. Precedence follows the table in §6.1
(Rust-style: `<< >>` looser than `+ -`, and `& ^ |` looser than the
comparison operators).

```
0b1100 & 0b1010    // 8
0b1100 | 0b1010    // 14
0b1100 ^ 0b1010    // 6
~0                 // -1
1 << 8             // 256
-16 >> 2           // -4
```

### 6.3 Equality

- `==` and `!=` work between any two values. Different types are unequal,
  except `Int` and `Float` are compared numerically.
- Arrays and objects compare **structurally** (element-wise / key-wise).
- Functions compare by identity.
- `null == null` is `true`. `null == 0` is `false`.

### 6.4 Pipe `|>`

`x |> rhs` evaluates `x`, then:

- If `rhs` is a call expression `f(args...)`, transform to `f(x, args...)`.
- Otherwise, evaluate `rhs` (it must produce a function) and call it with
  `x` as the sole argument.

```
arr |> Array.map(double) |> Array.reverse()
//  ==  Array.reverse(Array.map(arr, double))

5 |> double                 // == double(5)
5 |> double()               // == double(5)
0..10 |> Array.from()       // == Array.from(0..10)
```

Pipe is left-associative. Evaluation order is strictly left-to-right.

### 6.5 Indexing and member access

```
arr[0]
arr[i + 1]
obj['key']
obj.key                     // sugar for obj['key']
'hello'[1]                  // == 'e'  (strings are indexable)
```

Out-of-range numeric index → `null`. Missing object key → `null`.
Negative array indices count from the end: `arr[-1]` is the last element.

`obj.key` is exactly equivalent to `obj['key']` and may appear on the LHS of
any assignment operator.

### 6.6 Spread `...`

The spread operator unpacks an iterable into its containing context:

```
[1, ...other, 5]            // array literal
${...defaults, color: 'r'}  // object literal (later keys win)
f(x, ...args, y)            // function call
```

In a binding pattern, `...` is the **rest** form; see §11.

---

## 7. Collections

### 7.1 Arrays

```
arr := [1, 2, 3];
arr[0];                     // 1
#arr;                       // 3
arr + 4;                    // [1, 2, 3, 4]   (append element)
arr + [5, 6];               // [1, 2, 3, 4, 5, 6]   (concatenate)
arr += 7;                   // arr is now [1, 2, 3, 4, 5, 6, 7]
arr[0] = 99;                // arr is now [99, ...]
```

`Array + Array` concatenates. `Array + value` appends. `Array + Array` does
*not* nest; to append an array as a single element, write `arr + [[...]]`.
(Change from 0.1, where `arr += [5,6]` produced `[..., [5, 6]]`.)

Indexed assignment mutates the array in place (reference semantics).

### 7.2 Objects

```
obj := ${
    name: 'a',
    'with space': 1,
    nested: ${ inner: true },
};
obj.name;                   // 'a'
obj['with space'];          // 1
obj.color = 'red';          // adds a new key
#obj;                       // 3 (or 4 after the assign above)
```

Keys are always strings. Identifier keys (`name:`) are sugar for the
quoted form (`'name':`). `#obj` is the number of keys.

### 7.3 Ranges

Ranges are first-class values:

```
r := 0..10;                 // [0, 10)  — exclusive
r := 0..=10;                // [0, 10]  — inclusive
r := 0..10:2;               // step 2
r := 10..0:-1;              // descending
r := 10..0;                 // descending; step auto-flipped to -1
```

A range with `from`, `to`, `step` where `step` does not move `from` toward
`to` is empty.

Ranges support:

- Iteration in `for`
- Spread in array literals: `[...0..5]` → `[0, 1, 2, 3, 4]`
- Length: `#(0..10)` → 10
- Indexing: `(0..10:2)[1]` → 2

Ranges are **lazy**: they do not materialize their elements unless spread or
indexed.

### 7.4 Iteration

`for (vars, iterable) scope` — the iterable can be a Range, Array, Object,
or String:

| Iterable | One-var form          | Two-var form                          |
|----------|-----------------------|---------------------------------------|
| Range    | `for (i, 0..10)`      | `for (n, i, 0..10)` (n = 0,1,2,...)   |
| Array    | `for (x, arr)`        | `for (i, x, arr)`                     |
| Object   | `for (v, obj)`        | `for (k, v, obj)`                     |
| String   | `for (ch, str)`       | `for (i, ch, str)`                    |

(Change from 0.1: previously `for` only iterated ranges, written as
`for (en?, it?, range)` with a special sub-syntax. The range form is
preserved for back-compat; the new collection forms are added.)

Iteration variables are scoped to the loop body and not visible after.

---

## 8. Strings

Strings are immutable sequences of Unicode characters.

### 8.1 Operators

```
'abc' + 'def'               // 'abcdef'  — concatenation
#'hello'                    // 5         — character count
'hello'[0]                  // 'h'       — character at index (1-char string)
'hello' == 'hello'          // true
```

`+` between a string and a non-string is an error; use interpolation.

### 8.2 Interpolation

Inside any single-quoted string, `{expr}` is replaced by the result of
`str(expr)` (see §13). Use `\{` for a literal `{`. The interpolation grammar
matches a single tigr expression.

```
name := 'world';
'hello, {name}!'            // 'hello, world!'
'sum: {a + b}'              // 'sum: 7'
'first: {arr[0]}'           // 'first: 1'
```

Nested strings inside interpolation are allowed:

```
'{ if cond { 'yes' } else { 'no' } }'
```

(Open question: do we want a non-interpolating string form, e.g. `r'...'`?
Recommendation: no, until a real use case appears.)

---

## 9. Control flow

### 9.1 if / else

```
if cond scope
if cond scope else scope
if cond scope else if cond scope else scope
```

Resolves to the value of the chosen branch's scope, or `null` if no branch
matches.

### 9.2 while / while[]

```
while cond scope            // value of last iteration, or null
while[] cond scope          // array of per-iteration values (nulls filtered)
```

### 9.3 for / for[]

See §7.4 for the iteration forms. `for[]` collects values; `for` returns
the last.

```
squares := for[] (i, 1..=10) { i * i };
last := for (x, arr) { x };
```

### 9.4 break

`break` exits the innermost loop, optionally with a value:

```
break                       // exit loop, loop value is null
break 5                     // exit loop, loop value is 5
break (x + y)               // expression form requires parens
```

In a `for[]` / `while[]`, the value supplied to `break` is appended to the
result array (unless `null`, which is filtered like any iteration value).

`break` itself is an expression that evaluates to a "break value" — passing
it to another `break` propagates the exit one level up:

```
for (i, 0..10) {
    for (j, 0..10) {
        if i * j == 25 {
            break (break [i, j])    // break out of both loops with [5, 5]
        }
    }
}
```

### 9.5 return

`return` exits the innermost function, optionally with a value:

```
return
return value
return (expr)
```

Like `break`, `return value` is itself an expression (yielding a return
value), so it can be passed to outer `break`/`return` if needed.

### 9.6 try / catch / raise (v0.3)

Errors are values. `raise expr` aborts the current evaluation with the
given message (coerced to string). `try expr` evaluates `expr`,
producing its value on success or — on a raised or built-in runtime
error — `null`. `try expr catch (e) { handler }` instead evaluates the
handler with the error message bound to `e`. Both `try` and `raise` are
expressions.

```
content := try IO.read_file('config.tg') catch (e) {
    print('warning:', e);
    ''
};

count := try num(input) || 0;             // null on parse failure → 0

raise 'database connection lost'           // never returns
```

The body of `try` parses at `&&` precedence, so `try f(x) || default`
binds as `(try f(x)) || default`. Wrap in parens to include `||` inside
the try body. Built-in runtime errors (type mismatch, division by zero,
out-of-bounds index, missing import, etc.) are catchable and arrive as
the same string `RuntimeError::Display` produces — e.g.
`"type mismatch: ..."` or `"division by zero"`.

`raise` does not require a string; non-string values stringify via the
same rules as `str()`. The error value handlers see is always a string.

Unmatched `raise` exits the program with the message at the line of the
`raise` (same shape as today's runtime panics).

### 9.7 match (v0.5)

```
match subject {
    pattern => expr,
    pattern if guard => expr,
    _ => expr,
}
```

`match` evaluates `subject` once, then tries each arm top-to-bottom.
The value of the `match` is the body of the first arm whose pattern
matches (and whose guard, if present, is truthy). If no arm matches,
`match` evaluates to `null` — it is **non-exhaustive**, like an `if`
with no `else`. `match` is an expression. Arms are comma-separated; a
trailing comma is allowed. Each arm body runs in its own scope.

**Patterns** in a `match` arm are *refutable* — unlike the irrefutable
destructuring patterns of §11, they can fail and fall through:

- **Literal** — `0`, `'hi'`, `true`, `null`, `-1`. Matches if the
  subject `==` the literal.
- **Binding** — a bare name. Matches anything; binds the subject to
  that name for the arm body and guard.
- **Wildcard** — `_`. Matches anything, binds nothing.
- **Range** — `0..10` / `0..=9`. Matches if the subject is a number
  within the range. A non-number subject simply fails (does not
  raise).
- **Array** — `[p, q]` matches an array of exactly that length;
  `[head, ...rest]` matches length `>= 1`, `rest` collecting the
  remainder. A non-array subject fails without raising.
- **Object** — `${kind: 'circle', r}` matches an object; fields with a
  sub-pattern (`kind: 'circle'`) must match, shorthand fields (`r`)
  bind the value (a missing key binds `null`). `${a, ...rest}` collects
  unconsumed keys.
- **Or-pattern** — `p1 | p2 | p3`. Matches if any alternative matches.
  In v0.5 the alternatives must be literals, ranges, or `_` (no
  bindings, no structural patterns).

Patterns nest. A guard `pattern if cond` is an extra boolean test
evaluated after the pattern binds; a false guard falls through to the
next arm.

```
grade := match score {
    90..=100 => 'A',
    80..=89  => 'B',
    _        => 'F',
};

area := match shape {
    ${kind: 'rect', w, h}      => w * h,
    ${kind: 'square', side: s} => s * s,
    _                          => raise 'unknown shape',
};

sum := fn(xs) {
    match xs {
        []            => 0,
        [head, ...tl] => head + sum(tl),
    }
};
```

---

## 10. Functions

### 10.1 Definition

```
add := fn(a, b) { a + b };
fn() { 0 }                  // anonymous function expression
```

A function expression evaluates to a closure capturing the current
environment.

### 10.2 Call

```
add(1, 2)
fn() { 0 }()
arr.map(double)             // see Pipe §6.4 — but this is index+call, see below
```

`obj.method(args)` is `(obj.method)(args)` — i.e. plain index then call.
Tigr does not pass `this`. To get a method-style call with the receiver as
first arg, use pipe: `obj |> method(args)`.

### 10.3 Parameters

- **Positional**: `fn(a, b, c) { ... }`. Missing args are `null`. Extra args
  are silently dropped.
- **Rest**: `fn(a, ...rest) { ... }` — `rest` is an array of remaining args
  (possibly empty). Only one rest parameter, must be last.
- **Destructuring**: any parameter can be a pattern (§11):
  `fn([a, b], ${name}) { ... }`.

### 10.4 Closures

Functions capture their enclosing scope's bindings by reference. The
captured environment is the lexical scope at the point of `fn`, not at the
point of call.

```
adders := for[] (i, 0..3) { fn(x) { x + i } };
adders[0](10);              // 10
adders[1](10);              // 11
adders[2](10);              // 12
```

(Note: this works because each iteration of `for` opens a fresh scope for
`i`. The closure captures *that* scope's `i`. Implementation must preserve
this — see §15.)

---

## 11. Destructuring

Patterns appear:

- On the LHS of `:=` (declares the names in the pattern)
- On the LHS of `=` (assigns to existing bindings)
- As function parameters (declares as parameter names)

### 11.1 Array patterns

```
[a, b, c] := [1, 2, 3];                // a=1 b=2 c=3
[head, ...rest] := [1, 2, 3, 4];       // head=1 rest=[2,3,4]
[a, _, c] := [1, 2, 3];                // _ skips a position
[a, b] := [1];                         // a=1 b=null
```

### 11.2 Object patterns

```
${name, age} := person;                // shorthand: name := person.name, etc.
${name: n, age: a} := person;          // rename
${name, ...rest} := person;            // rest gets remaining keys as object
```

### 11.3 Nested patterns

```
[${name}, ${age}] := pairs;
${user: ${id, name}} := response;
```

### 11.4 Rules

- Patterns may not appear on the LHS of compound assignments (`+=` etc.).
- Pattern `:=` works in mid-expression position too:
  `arr := ([a, b] := [1, 2])`. The expression's value is the source
  rhs (here, `[1, 2]`); the names `a` and `b` are bound in the
  enclosing scope. Spec-equivalent to declaring them at the start of
  the scope and assigning at the source position.
- A pattern with `...rest` may have at most one rest element, in last
  position.
- Missing values bind to `null`.

---

## 12. Modules / imports

```
Array := import 'Array';
util  := import './lib/util';
```

`import` evaluates the named module and returns its final expression's
value. There are two flavors:

- **Bare names** (no `/`, `\`, or `.` in the string) — resolved against
  the native-module registry built into the interpreter (e.g. `IO`,
  `Os`, `Time` in v0.3 Phase 3+). An unknown bare name raises a
  catchable error.
- **Path-shaped strings** — resolved against the importing file's
  directory (per spec §12). `.tg` is appended automatically if absent.

A module typically returns an object:

```
// Array.tg
${
    map: fn(arr, f) { for[] (x, arr) { f(x) } },
    filter: fn(arr, f) { for[] (x, arr) { if f(x) { x } } },
    // ...
}
```

### 12.1 Caching (v0.3)

Each path is evaluated **at most once per `Vm` run**. The result is
cached and returned for subsequent imports of the same path. Bare-name
modules are similarly cached. As a corollary, two imports of the same
file yield the same underlying Object reference — mutating one is
visible through the other.

A circular import (`a.tg` imports `b.tg` which imports `a.tg`) raises
a catchable `"circular import"` error rather than diverging.

---

## 13. Built-in functions

Built-ins are ordinary bindings in the root environment. They can be
shadowed, passed around, and stored:

```
ops := [floor, ceil];
my_print := print;
```

### 13.1 Required built-ins for v0.2

| Name      | Signature                | Behavior                               |
|-----------|--------------------------|----------------------------------------|
| `print`   | `print(...args)`         | Write each arg (via `str`) + newline   |
| `str`     | `str(x) -> String`       | Canonical string form of any value     |
| `num`     | `num(x) -> Number\|null` | Parse string or pass through number    |
| `int`     | `int(x) -> Int`          | Truncate toward zero                   |
| `float`   | `float(x) -> Float`      | Coerce Int → Float; parse strings      |
| `bool`    | `bool(x) -> Bool`        | Truthiness rule from §5                |
| `floor`   | `floor(x) -> Int`        | Round down                             |
| `ceil`    | `ceil(x) -> Int`         | Round up                               |
| `rand`    | `rand() -> Float`        | Uniform in [0, 1)                      |
| `type`    | `type(x) -> String`      | Name of the value's type (v0.5)        |

`type(x)` returns one of `'int'`, `'float'`, `'string'`, `'bool'`,
`'null'`, `'array'`, `'object'`, `'range'`, `'function'`. Both user
closures and native built-ins report `'function'`.

### 13.2 Native modules (v0.3)

Imported via `import 'Name'` (no path separators). Each native module
returns an object whose entries are ordinary tigr values; users can
destructure or pass them like any other binding.

#### `IO`

| Entry         | Signature                          | Behavior                                          |
|---------------|------------------------------------|---------------------------------------------------|
| `read_file`   | `read_file(path) -> String`        | Read entire file as UTF-8; raises on error        |
| `write_file`  | `write_file(path, str) -> null`    | Overwrite file; raises on error                   |
| `append_file` | `append_file(path, str) -> null`   | Append; creates if missing; raises on error       |
| `exists`      | `exists(path) -> Bool`             | True if the path exists; never raises             |
| `read_line`   | `read_line() -> String\|null`      | One line from stdin (without trailing `\n`); null on EOF |
| `eprint`      | `eprint(...args) -> last_arg`      | Like `print` but to stderr                        |

#### `Os`

| Entry   | Signature                  | Behavior                                              |
|---------|----------------------------|-------------------------------------------------------|
| `args`  | `Array<String>` (value)    | `[interpreter, script, user_arg1, user_arg2, ...]`    |
| `env`   | `env(name) -> String\|null`| Read environment variable; null if unset              |
| `cwd`   | `cwd() -> String`          | Current working directory                             |
| `exit`  | `exit(code) -> never`      | Exit the process; bypasses `try` (real process exit)  |

#### `Time`

| Entry      | Signature                | Behavior                                |
|------------|--------------------------|-----------------------------------------|
| `now_ms`   | `now_ms() -> Int`        | Milliseconds since UNIX epoch           |
| `now_ns`   | `now_ns() -> Int`        | Nanoseconds since UNIX epoch            |
| `sleep_ms` | `sleep_ms(n) -> null`    | Block the thread for `n` ms             |

### 13.3 Source-stdlib modules (v0.3)

These ship as tigr `.tg` files embedded in the interpreter. `import`
returns an Object of functions; signatures are the same as any
user-defined module.

#### `Array`

`create`, `concat`, `map`, `filter`, `reduce`, `flatten`, `reverse`,
`index`, `find`, `find_index`, `any`, `all`, `head`, `tail`, `take`,
`drop`, `slice`, `sum`, `max_of`, `min_of`, `uniq`, `zip`, `join`,
`sort`, `sort_by`. Callbacks receive `(elem, index, whole_array)`;
unused trailing args are dropped per spec §10.3.

#### `String`

`split`, `join`, `replace`, `contains`, `index_of`, `lower`, `upper`,
`starts_with`, `ends_with`, `trim`, `trim_start`, `trim_end`,
`repeat`, `chars`, `pad_start`, `pad_end`.

#### `Math`

Constants `PI`, `E`. Functions `sqrt`, `log`, `log2`, `log10`, `exp`,
`sin`, `cos`, `tan`, `pow`, `abs`, `sign`, `min`, `max`, `clamp`.

The trig/log/exp functions are backed by the native `_NativeMath`
module (also importable directly). Source `Math.tg` re-exports them
alongside pure-tigr helpers — this gives users a single point to
shadow / extend without touching the interpreter.

### 13.4 `JSON` (v0.4)

```
JSON := import 'JSON';

JSON.parse(str) -> value
JSON.stringify(value) -> str            // compact
JSON.stringify(value, indent) -> str    // pretty; indent is Int (spaces) or Str
```

Type mapping:

| JSON              | tigr                                       |
|-------------------|--------------------------------------------|
| `null`            | `null`                                     |
| `true` / `false`  | `Bool`                                     |
| number            | `Float` (always — see below)               |
| string            | `Str`                                      |
| array             | `Array`                                    |
| object            | `Object` (insertion order preserved)       |

`JSON.parse` always parses numbers as `Float` (matching JSON's
"numbers are IEEE 754 doubles" convention; JS, Python's stdlib `json`
both behave this way). So `JSON.parse(JSON.stringify(123))` returns
`Float(123.0)`, not `Int(123)`. `JSON.stringify` writes `Int` values
without a decimal point and `Float` values with a `.0` suffix when
they're integer-valued.

Both calls raise on the failure cases — catchable via `try`:
- `JSON.parse`: malformed syntax, trailing content after the value,
  invalid escape sequences, unmatched surrogate pairs.
- `JSON.stringify`: non-serializable types (`Function`, `Range`,
  `Iter`, `NativeFn`), `NaN`/`Infinity`.

Cycles in arrays/objects are not detected and will overflow the call
stack — same posture as the wider Rc-cycle story (§15.1).

`str` rules:

- `Null` → `'null'`
- `Bool` → `'true'` / `'false'`
- `Number` → decimal form (Int has no decimal point; Float always has one)
- `String` → the string itself (no surrounding quotes)
- `Array` → `'[' + elements joined ', ' + ']'`, each via `str`
- `Object` → `'${' + 'k: v' joined ', ' + '}'`, key-order unspecified
- `Range` → `'a..b'` or `'a..=b'`, with `:s` if step ≠ 1
- `Function` → `'fn(...)'`

---

## 14. Grammar (informal EBNF)

```
Program     ::= Block
Block       ::= (Expr ';')* Expr?

Expr        ::= Assign

Assign      ::= Pattern ':=' Assign
              | LValue AssignOp Assign
              | LogicOr
AssignOp    ::= '=' | '+=' | '-=' | '*=' | '/=' | '%='

LogicOr     ::= LogicAnd ('||' LogicAnd)*
LogicAnd    ::= Equality ('&&' Equality)*
Equality    ::= BitOr (EqOp BitOr)*
BitOr       ::= BitXor ('|' BitXor)*
BitXor      ::= BitAnd ('^' BitAnd)*
BitAnd      ::= Pipe ('&' Pipe)*
Pipe        ::= RangeExpr ('|>' RangeExpr)*
RangeExpr   ::= Shift (('..' | '..=') Shift (':' Shift)?)?
Shift       ::= Additive (('<<' | '>>') Additive)*
Additive    ::= Multiplicative (('+' | '-') Multiplicative)*
Multiplicative ::= Power (('*' | '/' | '%') Power)*
Power       ::= Unary ('^^' Power)?
Unary       ::= ('-' | '!' | '#' | '~') Unary | Postfix
Postfix     ::= Primary (Call | Index | Member)*
Call        ::= '(' (Arg (',' Arg)*)? ')'
Arg         ::= '...' Expr | Expr
Index       ::= '[' Expr ']'
Member      ::= '.' Identifier

Primary     ::= Literal
              | Identifier
              | '(' Block ')'
              | '{' Block '}'                         // scope
              | ArrayLit | ObjectLit | FunctionLit
              | If | While | WhileA | For | ForA
              | 'break' BreakValue?
              | 'return' ReturnValue?
              | 'import' String
              | Try | Raise | Match

Try         ::= 'try' LogicAnd ('catch' '(' Identifier ')' Scope)?
Raise       ::= 'raise' Expr

Match       ::= 'match' Expr '{' (MatchArm (',' MatchArm)* ','?)? '}'
MatchArm    ::= MatchPat ('if' Expr)? '=>' Expr
MatchPat    ::= MatchAlt ('|' MatchAlt)*
MatchAlt    ::= LiteralPat | RangePat | Identifier | '_'
              | MatchArrayPat | MatchObjectPat
LiteralPat  ::= '-'? (Integer | Float) | String | 'true' | 'false' | 'null'
RangePat    ::= ('-'? NumLit) ('..' | '..=') ('-'? NumLit)
MatchArrayPat  ::= '[' (MatchPat (',' MatchPat)* )? ('...' Identifier)? ']'
MatchObjectPat ::= '$' '{' (MatchField (',' MatchField)*)? ('...' Identifier)? '}'
MatchField  ::= Identifier (':' MatchPat)?

Literal     ::= Integer | Float | String | 'true' | 'false' | 'null'
ArrayLit    ::= '[' (Element (',' Element)* ','?)? ']'
Element     ::= '...' Expr | Expr
ObjectLit   ::= '$' '{' (ObjMember (',' ObjMember)* ','?)? '}'
ObjMember   ::= '...' Expr | Identifier ':' Expr | String ':' Expr | Identifier   // shorthand
FunctionLit ::= 'fn' '(' Params? ')' '{' Block '}'
Params      ::= Param (',' Param)*
Param       ::= '...' Identifier | Pattern

Pattern     ::= Identifier | '_' | ArrayPat | ObjectPat
ArrayPat    ::= '[' (PatternElem (',' PatternElem)* ','?)? ']'
PatternElem ::= '...' Identifier | Pattern
ObjectPat   ::= '$' '{' (ObjPat (',' ObjPat)* ','?)? '}'
ObjPat      ::= Identifier (':' Pattern)? | '...' Identifier

LValue      ::= Identifier | Postfix-ending-in-Index-or-Member

If          ::= 'if' Expr Scope ('else' (Scope | If))?
While       ::= 'while' Expr Scope
WhileA      ::= 'while' '[' ']' Expr Scope
For         ::= 'for' '(' ForVars ',' Expr ')' Scope
ForA        ::= 'for' '[' ']' '(' ForVars ',' Expr ')' Scope
ForVars     ::= Identifier (',' Identifier (',' Identifier)?)?
```

Some notes:

- `for[]` and `while[]` are now `for '[' ']'` and `while '[' ']'` —
  whitespace permitted; the `[]` is parsed as separate tokens.
- The `for (vars, iterable)` form supersedes the old range-shaped sub-syntax.
  An iterable that is a `RangeExpr` reproduces the old behavior; an
  iterable that is anything else iterates that collection.
- Object literals and patterns share `${ ... }`; disambiguation is by
  context (LHS of `:=` / parameter position vs. expression position).

---

## 15. Notes for implementers (bytecode VM)

This section is not normative but flags decisions a VM author should make
deliberately.

### 15.1 Value representation

Recommended: a tagged union (`enum`) of:

```
Null
Bool(bool)
Int(i64)
Float(f64)
String(Rc<String>)        // or interned StringId
Array(Rc<RefCell<Vec<Value>>>)
Object(Rc<RefCell<HashMap<String, Value>>>)
Range(Rc<RangeData>)
Function(Rc<Closure>)
```

`Rc<RefCell<...>>` for collections gives you reference semantics cheaply
without a real GC. If you later add cycles (objects referencing themselves),
upgrade to a tracing GC.

### 15.2 Closures and upvalues

Tigr closures capture lexical environment by reference. The standard
bytecode-VM technique is:

- Each function has a list of "upvalue" slots.
- An upvalue points to a local in an enclosing frame, or to a heap-allocated
  cell once that local goes out of scope ("closing" the upvalue).
- See Crafting Interpreters Ch. 25 for a worked design.

The `for`-loop closure example in §10.4 requires that each iteration's loop
variable lives in a fresh slot that closures can capture independently. The
simplest implementation: each iteration opens a new scope, and closing
upvalues at scope-end heap-allocates the captured cell.

### 15.3 Break/return as values

Two viable strategies:

**A. Sentinel values.** `break` and `return` produce special `Value` tags
that propagate up. Loops/functions check for them after each subexpression.
Matches the 0.1 tree-walker. Easy but slow (every block exit is a check).

**B. Dedicated opcodes with unwind targets.** Each loop emits a `BREAK`
opcode with a target PC; each function frame has a `RETURN` opcode. For
break-with-value, the value is pushed on the stack before `BREAK`. For
chained `break (break v)`, the inner `break` pushes its value and emits a
`BREAK` to the inner loop's exit; the outer loop's exit handler sees the
value.

**Recommendation: B.** Sentinels-as-values force every binop and call site
to type-check its operands for the sentinel, which kills perf and is easy
to forget. Opcodes are localized.

The wrinkle is `break (break v)` — the inner `break` is *evaluated as an
expression* whose value is the outer break's argument. One way: compile
`break (break v)` so that the inner break's "value" position emits code
that pushes `v`, then a `BREAK` to the outer loop's exit. The intermediate
result is discarded. This works because `break` never actually produces a
value at runtime — control jumps before any consumer would read it.

### 15.4 String interpolation

Compile `'foo {x} bar {y}'` to:

```
PUSH 'foo '
<expr x>
CALL_BUILTIN str, 1
CONCAT
PUSH ' bar '
CONCAT
<expr y>
CALL_BUILTIN str, 1
CONCAT
```

Or, more efficiently, a single `CONCAT_N` that takes a count.

### 15.5 Pipe lowering

Pipe is pure sugar at the AST level. Lower `x |> f(a, b)` to `f(x, a, b)`
in the parser or a desugar pass. No runtime support needed.

### 15.6 Range iteration

`for (x, range)` should not materialize the range. Compile to a counted
loop with the range's `from`/`to`/`step` baked in. For `for (x, array)`,
emit a length-bounded indexed loop. For `for (k, v, object)`, you need an
iteration order — recommend insertion order (use `IndexMap` or similar).

### 15.7 Open questions for the implementer

These are genuinely undecided; pick when you reach them:

- **String type**: small-string optimization? Interning for short strings?
- **Number tower**: keep Int/Float split, or unify on Float like JS?
  Keeping the split is more "Tigr" but adds dispatch.
- **Equality on collections**: structural vs identity. This spec says
  structural. A VM author may find identity-by-default with a `eq()`
  built-in cheaper.
- **Module caching**: re-evaluate on every `import`, or cache by path?
- **Error handling**: panic-and-die (current 0.1) or recoverable runtime
  errors with `try`? Out of scope for v0.2.

---

## Appendix A — Migration from 0.1

User-visible breaking changes:

1. `=` no longer creates new bindings. Use `:=` to declare.
2. Arrays and objects are now reference types.
3. Empty `[]` and `${}` are now falsy.
4. `&&` / `||` now return one of their operands rather than a `Bool`.
5. `arr += [5, 6]` now concatenates instead of nesting.
6. `floor`, `ceil`, `rand` are no longer keywords; they are bindings.
7. Strings now interpolate: any `{` in a string starts an interpolation.
   Use `\{` for a literal brace.

Additions (non-breaking):

8. `for` now iterates arrays, objects, strings, and ranges directly.
9. Pipe `|>`, spread `...`, `..=` inclusive ranges, destructuring patterns.
10. `print`, `str`, `num`, `int`, `float`, `bool` built-ins.
11. Strings support `+`, `#`, indexing.

## Appendix B — Changes in v0.3

12. `try` / `catch` / `raise` expressions for recoverable errors
    (§9.6). Built-in runtime errors are now catchable; previously they
    aborted the program unconditionally.
13. Module caching (§12.1). Each path now evaluates once per `Vm` run.
    Bare-name imports (e.g. `import 'IO'`) route to a built-in native
    module registry; unknown names raise.
14. Native modules `IO`, `Os`, `Time` (§13.2) — file/stdio,
    process/environment, and clock access. Errors from fallible IO
    are `Raised(String)` and catchable via `try`.
15. Source-stdlib modules `Array`, `String`, `Math` (§13.3) — shipped
    as embedded `.tg` source. Math/String wrap underlying native
    modules (`_NativeMath`, `_NativeString`) for primitives that
    need Rust.
16. **Interactive REPL.** Running `tigr` with no script argument
    starts a session where each line is evaluated against a
    persistent set of bindings. Closures over REPL locals share
    upvalue cells across lines, so mutating an outer name is
    visible through closures defined earlier or later. An uncaught
    raise prints the error and the session continues. Multi-line
    input is supported when the parser indicates incompleteness.
    `:quit` / `:q` exits.

## Appendix C — Changes in v0.4

17. **Source-snippet error rendering.** Lex / parse / compile /
    runtime errors now print with a filename, source line, and a
    caret/underline that matches the offending span (lex/parse/
    compile) or just points at the source line (runtime). REPL lines
    register as `<repl:N>` sources; imports register their file
    paths so errors inside an imported file render against THAT
    file's source.
18. **Number-literal extensions** (§2.5): hex `0xFF`, binary `0b1010`,
    octal `0o755`; underscore digit separators `1_000_000` /
    `0xFF_FF`; scientific `1e6` / `2.5e-3` (always Float); leading-
    dot floats `.5`. Trailing-dot like `5.` continues to lex as
    `Int(5) Dot` so `5.method` member access still works.
19. **Pattern destructuring on `=`** (§11.4) and **mid-expression
    pattern decls.** `[a, b] = [3, 4]` reassigns to existing
    bindings. `arr := ([a, b] := [3, 4])` introduces `a` and `b` in
    the enclosing scope and evaluates to the source rhs.
20. **`JSON` native module** (§13.4) — `parse` and `stringify` with
    an optional `indent` argument. Numbers always parse as `Float`.

## Appendix D — Changes in v0.5

User-visible breaking change:

21. **`^` is now bitwise XOR; exponentiation moved to `^^`.** Any
    existing `a ^ b` power expression must be rewritten `a ^^ b`.

Additions (non-breaking):

22. **`type(x)` built-in** (§13.1) — returns the value's type as a
    string. User closures and native built-ins both report
    `'function'`.
23. **Bitwise operators** (§6.2a) — `& | ^ ~ << >>`, all `Int`-only.
    `>>` is an arithmetic shift; a shift amount outside `0..64`
    raises. Precedence is Rust-style (see §6.1).
24. **`match` expression** (§9.7) — refutable pattern matching with
    literal, binding, wildcard, range, array, object, and or-patterns,
    optional `if` guards, comma-separated arms. Non-exhaustive: a
    `match` with no matching arm evaluates to `null`. Or-pattern
    alternatives may not bind variables.
