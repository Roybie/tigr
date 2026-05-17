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
  last iteration. Array forms (`for[]`, `while[]`) yield an array of every
  per-iteration value; `continue` omits an item.
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
fn  if  else  for  while  break  continue  return  import  try  catch
raise  match  null  true  false
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
  binding (same rule as `=`). `+=` on an array target mutates the array
  in place rather than rebinding the name — see §7.1.

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

Only two values are **falsy**:

- `false`
- `null`

**Everything else is truthy** — including `0`, `0.0`, `''` (empty string),
`[]` (empty array), `${}` (empty object), empty ranges, empty maps/sets,
and all functions. Truthiness tests exactly one thing: "is this value
present and not `false`". To test whether a collection or string is
*empty*, compare its length: `#arr == 0`. To test for a number being
zero, compare it: `n == 0`.

`!x` and boolean contexts (`if`, `while`, `&&`, `||`) use this rule.

(Change from v0.2–v0.10, where `0`, `0.0`, `''`, and empty
collections were also falsy. The Lua-style rule keeps *absence*
(`null`) distinct from a legitimate zero/empty value, and makes the
`x || default` idiom default only on `null`/`false`.)

`&&` and `||` short-circuit and return the **value** that decided the result
(not coerced to bool):

```
0 || 'fallback'      // == 0          — 0 is truthy
null || 'fallback'   // == 'fallback' — null is falsy
'a' && 'b'           // == 'b'
false && 'b'         // == false
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

### 6.2b Integer overflow (v0.8)

`Int` is a signed 64-bit value (range `-2^63 .. 2^63-1`). Integer
arithmetic — `+`, `-`, `*`, and unary `-` — is *checked*: a result
that falls outside the `Int` range raises a catchable runtime error
with `kind: 'overflow'` (§9.6) rather than wrapping silently. `^^`
(power) always yields `Float` and so has no integer-overflow path.
`Float` arithmetic is unchecked IEEE-754 and may produce `inf`.

```
try (9223372036854775807 + 1) catch (e) { e.kind }   // 'overflow'
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

Array-literal and function-call spread accept an Array, Range, String,
or — since v0.8 — an iterator object (§7.4); object-literal spread
requires an Object. In a binding pattern, `...` is the **rest** form;
see §11.

---

## 7. Collections

### 7.1 Arrays

```
arr := [1, 2, 3];
arr[0];                     // 1
#arr;                       // 3
arr + 4;                    // a fresh [1, 2, 3, 4]   (append element)
arr + [5, 6];               // a fresh [1, 2, 3, 4, 5, 6]   (concatenate)
arr[0] = 99;                // arr is now [99, 2, 3]   (in place)
```

`Array + Array` concatenates. `Array + value` appends. `Array + Array` does
*not* nest; to append an array as a single element, write `arr + [[...]]`
or `Array.push(arr, [...])`. `+` always produces a fresh array — its
operands are never mutated.

`+=` grows an array **in place** (v0.7). It applies the same
array-vs-value rule as `+` — an array right-hand side extends, any other
value appends one element — but mutates the existing array rather than
rebinding the name, so every alias observes the change. This matches the
reference semantics of indexed assignment.

```
a := [1, 2, 3];
b := a;
a += 4;                     // a and b are both [1, 2, 3, 4]
a += [5, 6];                // a and b are both [1, 2, 3, 4, 5, 6]
```

Indexed assignment mutates the array in place (reference semantics).

(Change from 0.1, where `arr += [5, 6]` produced `[..., [5, 6]]`. Change
from v0.2–v0.6, where `+=` rebuilt a fresh array instead of mutating in
place.)

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
String, or iterator object:

| Iterable | One-var form          | Two-var form                          |
|----------|-----------------------|---------------------------------------|
| Range    | `for (i, 0..10)`      | `for (n, i, 0..10)` (n = 0,1,2,...)   |
| Array    | `for (x, arr)`        | `for (i, x, arr)`                     |
| Object   | `for (v, obj)`        | `for (k, v, obj)`                     |
| String   | `for (ch, str)`       | `for (i, ch, str)`                    |
| Iterator | `for (v, it)`         | `for (i, v, it)` (i = 0,1,2,...)      |

(Change from 0.1: previously `for` only iterated ranges, written as
`for (en?, it?, range)` with a special sub-syntax. The range form is
preserved for back-compat; the new collection forms are added.)

**Iterator objects (v0.8).** An *iterator object* is an Object whose
`next` field is a function — the pull-based protocol of the `Iter`
module (§13.3): `next()` returns `${done: true}` or `${done: false,
value: v}`. `for` drives such an object by calling `next()` until it
reports `done`; the two-var form supplies a synthetic `0, 1, 2, …`
counter. So an `Iter` pipeline can be consumed directly, without
`Iter.collect()`:

```
for (sq, 0..=4 |> Array.from() |> Iter.from() |> Iter.map(fn(n){n*n})) {
    print(sq)
}
```

The presence of a **callable `next` field** is what distinguishes an
iterator object from a plain object. An object that has a `next` field
that is *not* a function (or no `next` at all) iterates as key/value
entries, exactly as before. A `next()` that returns a non-object, or an
object with no `done` field, raises a catchable error.

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
while[] cond scope          // array of every iteration's body value
```

### 9.3 for / for[]

See §7.4 for the iteration forms. `for[]` collects values; `for` returns
the last.

```
squares := for[] (i, 1..=10) { i * i };
last := for (x, arr) { x };
```

A `for[]` / `while[]` collects **every** iteration's body value
verbatim, including `null`. The only way to omit an item is `continue`
(§9.4a) — skipping is control flow, not a filtered value.

```
for[] (i, 0..5) { if i % 2 == 0 { i } }   // [0, null, 2, null, 4]
for[] (i, 0..5) { if i % 2 != 0 { continue }; i }   // [0, 2, 4]
```

### 9.4 break

`break` exits the innermost loop, optionally with a value:

```
break                       // exit loop, loop value is null
break 5                     // exit loop, loop value is 5
break (x + y)               // expression form requires parens
```

In a `for[]` / `while[]`, `break <value>` appends the value to the result
array — verbatim, even if it is `null`. A bare `break` (no value) appends
nothing, exiting the loop without contributing a final item.

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

### 9.4a continue (v0.6)

`continue` skips the rest of the current loop iteration and proceeds to
the next. In a `for[]` / `while[]` the skipped iteration contributes
**nothing** to the result array — `continue` is the only way to omit an
item. In a plain `for` / `while` that iteration's value becomes `null`.
Unlike `break`, `continue` carries no value. `continue` outside any loop
is a compile-time error.

```
evens := for[] (n, 0..10) {
    if n % 2 != 0 { continue };
    n
};                                  // [0, 2, 4, 6, 8]
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

Errors are values. `raise expr` aborts the current evaluation, carrying
`expr` — **any** value, stored verbatim with no coercion. `try expr`
evaluates `expr`, producing its value on success or — on a raised or
built-in runtime error — `null`. `try expr catch (e) { handler }`
instead evaluates the handler with the error bound to `e`. Both `try`
and `raise` are expressions.

`catch` binds **exactly what was raised** — `raise 'msg'` binds a
string, `raise ${...}` binds that object. A **built-in** runtime error
is instead reified into an object `${kind, message, line}` so a handler
can `match` on it (v0.7b):

- `kind` — a stable snake-case tag: `type_mismatch`, `div_by_zero`,
  `index_out_of_bounds`, `arity_mismatch`, `not_callable`,
  `invalid_index_type`, `immutable_target`, `import_failed`,
  `overflow`, `stack_overflow`, `stack_underflow`, `cycle`,
  `no_match`.
- `message` — the human-readable text an uncaught error would show
  (what `RuntimeError::Display` produces, e.g. `"division by zero"`).
- `line` — the source line the error occurred on.

```
content := try IO.read_file('config.tg') catch (e) {
    print('warning:', e);
    ''
};

count := try num(input) || 0;             // null on parse failure → 0

result := try risky() catch (e) {
    match e.kind {
        'div_by_zero' => 0,
        _             => raise e,          // not ours — re-raise
    }
};

raise ${kind: 'db_down', detail: 'connection lost'}
```

The body of `try` parses at `&&` precedence, so `try f(x) || default`
binds as `(try f(x)) || default`. Wrap in parens to include `||` inside
the try body.

Native stdlib modules (`Math`, `IO`, `JSON`, `Path`, ...) raise plain
**string** messages, so `catch` binds those as strings. An uncaught
raised value is rendered via `str()` in the error report. One
exception: `JSON.stringify` of a circular structure raises a
structured built-in error (`kind: 'cycle'`), reified like the others
above.

`raise` does not require a string; non-string values stringify via the
same rules as `str()`. The error value handlers see is always a string.

Unmatched `raise` exits the program with the message at the line of the
`raise` (same shape as today's runtime panics).

An uncaught error — a `raise` or a built-in runtime error that escapes
every `try` — is rendered with a source snippet (Appendix C item 17)
followed by a **stack trace** (v0.8): each active call frame, innermost
first, as `<name> at <file>:<line>`. Function names come from the
binding (`f := fn(){}` → `f`), falling back to `<anonymous>`; the
top-level program shows as `<main>`. Because tail calls reuse their
frame (§10.5), a tail-recursive function appears once. The trace is
omitted for a single-frame error.

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
`match` raises a catchable `no_match` runtime error (§9.6) rather than
yielding a value — a fall-through is almost always a bug. To make a
`match` total, end it with a `_` wildcard (or a bare-binding) arm; an
unguarded one is provably exhaustive and never raises. `match` is an
expression. Arms are comma-separated; a trailing comma is allowed.
Each arm body runs in its own scope.

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
- **Default values (v0.6)**: a parameter may be given a default with `=`,
  e.g. `fn(a, b = 10) { ... }`. The default is evaluated and bound when
  that argument slot is `null` — whether the argument was omitted *or*
  explicitly passed as `null`. Defaults may reference earlier parameters
  (`fn(a, b = a + 1)`), are evaluated left-to-right, and run only when
  needed. A default is permitted only on a plain identifier parameter —
  not a destructuring pattern, and not the rest parameter.

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

### 10.5 Recursion and tail calls (v0.8)

A function may call itself — a `fn` initialiser sees its own binding
name, so `fact := fn(n) { if n <= 1 { 1 } else { n * fact(n - 1) } }`
works directly. Mutual recursion uses the forward-declaration idiom:
declare the name first (`g := null`), then assign the function once the
other is in scope.

A call in **tail position** reuses the current call frame instead of
pushing a new one, so a tail-recursive function runs in constant frame
space, to any depth. A call is in tail position when its result is
*directly* the result of the enclosing function — including through the
branches of an `if`, the arms of a `match`, and the tail expression of
a block, when those are themselves the function's result.

```
sum := fn(n, acc) {
    if n <= 0 { acc } else { sum(n - 1, acc + n) }   // tail call
};
sum(1000000, 0)                                      // runs in O(1) frames
```

A call is **not** in tail position if its result is used further — e.g.
`n * fact(n - 1)` (the call feeds `*`) or `1 + sum(n - 1)` (feeds `+`).
Such a call still pushes a frame. To make deep recursion of that shape
work, rewrite it in the accumulator style above. Calls inside a `try`
body, a `&&`/`||` operand, or a loop body are likewise never tail
calls.

Call depth is bounded: recursion that genuinely nests past the VM's
limit raises a catchable `stack_overflow` error (§9.6) rather than
crashing the process.

```
try deepNonTailRecursion() catch (e) { e.kind }      // 'stack_overflow'
```

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
    filter: fn(arr, f) { for[] (x, arr) { if !f(x) { continue }; x } },
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
| `str`     | `str(x [, radix [, prefix]])` | String form; radix form for Ints  |
| `num`     | `num(x) -> Number\|null` | Parse string or pass through number    |
| `int`     | `int(x) -> Int`          | Truncate toward zero                   |
| `float`   | `float(x) -> Float`      | Coerce Int → Float; parse strings      |
| `bool`    | `bool(x) -> Bool`        | Truthiness rule from §5                |
| `floor`   | `floor(x) -> Int`        | Round down                             |
| `ceil`    | `ceil(x) -> Int`         | Round up                               |
| `rand`    | `rand() -> Float`        | Uniform in [0, 1); seedable via `Random.seed` (§13.2) |
| `type`    | `type(x) -> String`      | Name of the value's type (v0.5)        |
| `gc`      | `gc() -> Object`         | Garbage-collector counters (v0.10): `${live, collections, allocated, freed}` |

`gc()` returns a read-only snapshot of the tracing collector's state
(§15.1): `live` is the current managed-object count, `collections` the
number of collections run so far, and `allocated` / `freed` the lifetime
totals. Collection is automatic — `gc()` only observes it.

`type(x)` returns one of `'int'`, `'float'`, `'string'`, `'bool'`,
`'null'`, `'array'`, `'object'`, `'range'`, `'function'`. Both user
closures and native built-ins report `'function'`.

`str` takes an optional **radix** and **prefix** (v0.5). `str(x)` is
the canonical form. `str(n, radix)` renders an `Int` `n` in `radix`
(an `Int` in `2..=36`, lowercase digits); a non-`Int` value or an
out-of-range radix raises. `str(n, radix, prefix)` with `prefix` a
`Bool` prepends the literal marker — `0b` / `0o` / `0x` for radix
2 / 8 / 16 (any other radix with `prefix == true` raises). A negative
number's `-` precedes the prefix.

```
str(255, 16)         // 'ff'
str(255, 16, true)   // '0xff'
str(10, 2, true)     // '0b1010'
str(-10, 16, true)   // '-0xa'
```

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
| `list_dir`    | `list_dir(path) -> Array<String>`  | Names of the directory's entries; raises on error (v0.6) |
| `mkdir`       | `mkdir(path) -> null`              | Create directory and any missing parents; raises on error (v0.6) |
| `remove`      | `remove(path) -> null`             | Delete a file, or a directory recursively; raises on error (v0.6) |
| `is_dir`      | `is_dir(path) -> Bool`             | True if the path is a directory; never raises (v0.6) |
| `is_file`     | `is_file(path) -> Bool`            | True if the path is a regular file; never raises (v0.6) |
| `stat`        | `stat(path) -> Object`             | `${size, is_dir, is_file, modified_ms}`; raises if the path is missing (v0.6) |
| `read_line`   | `read_line() -> String\|null`      | One line from stdin (without trailing `\n`); null on EOF |
| `eprint`      | `eprint(...args) -> last_arg`      | Like `print` but to stderr                        |

#### `Os`

| Entry   | Signature                  | Behavior                                              |
|---------|----------------------------|-------------------------------------------------------|
| `args`  | `Array<String>` (value)    | `[interpreter, script, user_arg1, user_arg2, ...]`    |
| `env`   | `env(name) -> String\|null`| Read environment variable; null if unset              |
| `cwd`   | `cwd() -> String`          | Current working directory                             |
| `run`   | `run(cmd, ...args) -> Object` | Run a subprocess, capturing output (v0.6). See below  |
| `exit`  | `exit(code) -> never`      | Exit the process; bypasses `try` (real process exit)  |

`Os.run(cmd, ...args)` spawns `cmd` with the given string arguments,
waits for it, and returns `${code, stdout, stderr}` — `code` is the
exit status (`-1` if the process was killed by a signal), `stdout` /
`stderr` are the captured streams as Strings. A non-zero exit is a
normal result, **not** an error; `run` raises only when the process
cannot be spawned at all (e.g. command not found).

#### `Path` (v0.6)

Pure path-string manipulation backed by the host's path rules; nothing
here touches the filesystem.

| Entry         | Signature                          | Behavior                                          |
|---------------|------------------------------------|---------------------------------------------------|
| `join`        | `join(...parts) -> String`         | Join path segments with the platform separator    |
| `dirname`     | `dirname(path) -> String`          | The parent directory (`''` if none)               |
| `basename`    | `basename(path) -> String`         | The final component (`''` if none)                |
| `ext`         | `ext(path) -> String`              | File extension without the dot (`''` if none)     |
| `is_absolute` | `is_absolute(path) -> Bool`        | True if the path is absolute                      |

Every `Path` entry raises on a non-String argument.

#### `Time`

| Entry      | Signature                | Behavior                                |
|------------|--------------------------|-----------------------------------------|
| `now_ms`   | `now_ms() -> Int`        | Milliseconds since UNIX epoch           |
| `now_ns`   | `now_ns() -> Int`        | Nanoseconds since UNIX epoch            |
| `sleep_ms` | `sleep_ms(n) -> null`    | Block the thread for `n` ms             |

#### `DateTime` (v0.6)

Calendar date/time, **UTC only**. A *components object* is
`${year, month, day, hour, minute, second, ms, weekday, yearday}` —
`month` is 1–12, `weekday` is 0=Sunday, `yearday` is the 1-based day of
the year.

| Entry     | Signature                       | Behavior                                          |
|-----------|---------------------------------|---------------------------------------------------|
| `now`     | `now() -> Object`               | The current UTC time as a components object       |
| `from_ms` | `from_ms(ms) -> Object`         | Convert epoch-milliseconds to a components object |
| `to_ms`   | `to_ms(obj) -> Int`             | Convert a components object to epoch-milliseconds; missing fields default (year 1970, month/day 1, rest 0) |
| `format`  | `format(ms, fmt) -> String`     | Render epoch-ms `ms` per `fmt`. Directives: `%Y %m %d %H %M %S %j %%`; other text is literal |
| `parse`   | `parse(str) -> Int`             | Parse ISO-8601 `YYYY-MM-DD[(T\| )HH:MM:SS[.fff]]` to epoch-ms; raises on malformed input |

`format`'s first argument is epoch-**milliseconds**, not a components
object — pass a `Time.now_ms()` or `to_ms(...)` result:

```
DateTime := import 'DateTime';
DateTime.format(DateTime.to_ms(DateTime.now()), '%Y-%m-%d')   // '2026-05-15'
```

#### `Random` (v0.9)

Seedable pseudo-random numbers. Every entry — and the bare `rand()`
built-in (§13.1) — draws from a single per-thread stream, so
`Random.seed(n)` makes `rand()` reproducible too. Until `seed` is
called the stream is auto-seeded from the wall clock.

| Entry     | Signature                       | Behavior                                                        |
|-----------|---------------------------------|-----------------------------------------------------------------|
| `seed`    | `seed(n) -> null`               | Pin the stream to Int `n` (any value, `0` included)             |
| `float`   | `float() -> Float`              | Uniform Float in `[0, 1)`                                       |
| `int`     | `int(lo, hi) -> Int`            | Uniform Int in the **inclusive** range `[lo, hi]`; raises if `lo > hi` |
| `bool`    | `bool() -> Bool`                | `true` or `false`, each with probability ½                      |
| `choice`  | `choice(arr) -> value`          | A uniformly random element of a non-empty Array; raises if empty |
| `range`   | `range(r) -> Int`               | A uniformly random element of a non-empty Range, honouring its step (`range(0..=8:2)` → one of `0,2,4,6,8`) |
| `shuffle` | `shuffle(arr) -> Array`         | A **new** array with `arr`'s elements in random order; the input is left untouched |

```
Random := import 'Random';
Random.seed(42);
Random.int(1, 6)          // a dice roll, reproducible after the seed
```

### 13.3 Source-stdlib modules (v0.3)

These ship as tigr `.tg` files embedded in the interpreter. `import`
returns an Object of functions; signatures are the same as any
user-defined module.

#### `Array`

`push`, `extend`, `pop`, `shift`, `unshift`, `insert`, `remove`,
`clear`, `create`, `concat`, `map`, `filter`, `reduce`, `flatten`,
`reverse`, `index`, `find`, `find_index`, `any`, `all`, `head`, `tail`,
`take`, `drop`, `slice`, `sum`, `max_of`, `min_of`, `uniq`, `zip`,
`join`, `group_by`, `chunk`, `windows`, `partition`, `flat_map`,
`count_of`, `sort`, `sort_by`. Callbacks receive
`(elem, index, whole_array)`; unused trailing args are dropped per
spec §10.3.

The eight in-place mutators are backed by the native `_NativeArray`
module — pure tigr can grow an array (`+`/spread) but cannot shrink
one. `push(arr, v)` / `extend(arr, other)` append (O(1) amortized /
O(#other)); `pop` / `shift` remove and return the last / first element
(`null` on an empty array); `unshift(arr, v)` prepends; `insert(arr,
i, v)` inserts at `i`; `remove(arr, i)` removes and returns one element
(`null` if out of range), while `remove(arr, start, count)` removes and
returns a `count`-long sub-array; `clear` empties in place. All return
`arr` except `pop`/`shift`/`remove`. Negative indices count from the
end. Contrast `concat`, which builds a fresh array.

`head`/`tail` accept a negative `n` (Python-slice style):
`head(arr, -1)` is all but the last element, `tail(arr, -1)` all but
the first — whereas `take`/`drop` clamp a negative `n` to 0. `group_by`
returns a `Map` (so non-string keys work); the other combinators build
fresh arrays.

#### `Iter` (v0.7)

Lazy, pull-based iterators. An iterator is an object `${next: fn()}`
whose `next()` yields `${done: true}` or `${done: false, value}`.
Adapters `from`, `count`, `repeat`; lazy combinators `map`, `filter`,
`take`, `drop`, `enumerate`, `zip`, `chain`; consumers `collect`,
`reduce`, `for_each`, `count_of`, `find`, `nth`. A combinator does no
work until a consumer pulls from it, so a pipeline never materializes
an intermediate array. `count` / `repeat` are infinite and must be
bounded by `take` (or a short-circuiting `find` / `nth`). Pure tigr —
closures capture the source iterator; no VM support is required.

#### `Object` (v0.6)

`keys`, `values`, `entries`, `from_entries`, `has`, `merge`, `map`,
`filter`. `keys` / `values` / `entries` return arrays in insertion
order (`entries` → `[key, value]` pairs; `from_entries` is its
inverse). `merge` / `map` / `filter` return fresh objects — inputs are
never mutated. Callbacks receive `(value, key, whole_object)`.

As of v0.9, `has` is O(1) (backed by native `_NativeObject`) and tells
a missing key from a present `null` value, which `obj[key]` cannot.
`keys` / `values` / `entries` append in place (O(n) total) rather than
copying the accumulator each step.

#### `Map` (v0.9)

An arbitrary-keyed, insertion-ordered dictionary. Unlike `Object`
(string keys only), a `Map` key may be any **null / bool / int /
string** value; a `Float` or collection key raises `invalid_key_type`.
It is a distinct runtime type — `type(m)` is `"map"` — backed by the
native `_NativeMap` module.

`m[key]` reads an entry (`null` when absent) and `m[key] = value`
writes one. `#m` is the entry count; `for (k, v, m) { ... }` iterates
entries in insertion order. Functions: `new`, `get`, `set`, `has`,
`delete`, `keys`, `values`, `entries`, `size`, `clear`. `new()` builds
an empty map; `new(obj)` copies an Object's entries; `new(pairs)`
builds from an array of `[key, value]` pairs. `has` is O(1) and tells
a missing key from a present `null` value. A `Map` is not
JSON-serializable (`JSON.stringify` raises).

```
Map := import 'Map';
m := Map.new();
m[1] = 'one';      // int key
m['1'] = 'string'; // distinct string key — no collision
Map.has(m, 1)      // → true
```

#### `Set` (v0.9)

An insertion-ordered collection of unique values. Elements share
`Map`'s key restriction (null / bool / int / string). `type(s)` is
`"set"`; backed by the native `_NativeSet` module.

`s[x]` tests membership (`true` / `false`); `s[x] = ...` is an error
(`immutable_target`) — mutate with `add` / `delete`. `#s` is the
element count; `for (x, s) { ... }` iterates in insertion order.
Functions: `new`, `add`, `has`, `delete`, `items`, `size`, `clear`,
and the algebra `union`, `intersection`, `difference` (each returns a
fresh set, inputs untouched). `new(array)` builds a set from an array,
collapsing duplicates. Like `Map`, a `Set` is not JSON-serializable.

#### `String`

`split`, `join`, `replace`, `contains`, `index_of`, `lower`, `upper`,
`starts_with`, `ends_with`, `trim`, `trim_start`, `trim_end`,
`repeat`, `chars`, `pad_start`, `pad_end`, `format`, `printf`.

`format(value, spec)` (v0.9) renders one value through a spec
mini-language and `printf(template, args)` (v0.9) fills a template;
both share the same spec:

```
spec := [[fill]align][sign]['#'][width][','][.precision][type]
```

| Field       | Meaning                                                          |
|-------------|------------------------------------------------------------------|
| `fill`      | Any char — only a fill char when immediately followed by `align` |
| `align`     | `<` left, `>` right, `^` centre                                  |
| `sign`      | `+` shows a sign on positive numbers (`-` is always shown)        |
| `#`         | Alternate form — adds the `0x`/`0o`/`0b` prefix for `x`/`X`/`o`/`b` |
| `width`     | Minimum field width; a *bare* leading `0` means zero-pad         |
| `,`         | Thousands grouping of the integer part                           |
| `.precision`| Float decimal places; truncates strings                         |
| `type`      | `s d f e E x X b o` — absent renders by the value's natural type |

Numbers default to right-align, everything else to left-align. `f`/`e`
default to 6 decimals. A `f`/`e` type on a non-number, an integer type
on a non-integral float, an `s` on a non-string, or an unparsable spec
all raise. `printf` placeholders are `%(SPEC)` — each consumes the next
element of `args` and `%%` is a literal percent; too few or too many
arguments both raise. (`%(...)`, not `{}`, because tigr interpolates
`{}` in every string literal.) Example:

```tigr
String.format(1234567, ',d')                   // "1,234,567"
String.format(3.14159, '.2f')                  // "3.14"
String.format('hi', '^8')                      // "   hi   "
String.printf('%(<8)%(>6.2f)', ['tea', 1.5])   // "tea       1.50"
```

#### `Math`

Constants `PI`, `E`. Functions `sqrt`, `log`, `log2`, `log10`, `exp`,
`sin`, `cos`, `tan`, `pow`, `abs`, `sign`, `min`, `max`, `clamp`.

The trig/log/exp functions are backed by the native `_NativeMath`
module (also importable directly). Source `Math.tg` re-exports them
alongside pure-tigr helpers — this gives users a single point to
shadow / extend without touching the interpreter.

#### `Test` (v0.9)

A small test framework, itself written in tigr. Assertions —
`assert(cond, msg?)`, `assert_eq(actual, expected, msg?)`,
`assert_ne(a, b, msg?)`, `assert_raises(thunk, kind?)`,
`fail(msg?)` — `raise` on failure, so they work standalone. `assert_eq`
uses `==`, which is structural for arrays and objects (§6.3).
`assert_raises` runs `thunk` and fails unless it raised; with a `kind`
argument the raised value must match — a reified built-in error's
`kind` field, or the raised value itself otherwise — and the caught
error is returned.

Tests are plain data: `case(name, fn)` packages an unrun test, and
`suite(name, cases)` runs an array of them, printing a `PASS`/`FAIL`
line per case and a tally, then returning a result object
`${name, passed, failed, total, failures}` (`failures` being an array
of `${name, error}`).

```
Test := import 'Test';

Test.suite('arithmetic', [
    Test.case('adds', fn() { Test.assert_eq(1 + 1, 2) }),
    Test.case('div zero raises', fn() {
        Test.assert_raises(fn() { 1 / 0 }, 'div_by_zero')
    }),
])
```

The `tigr test [path]` CLI subcommand discovers test files —
`*_test.tg` anywhere, plus every `.tg` file under a `tests/`
directory — runs each, and sums the `passed`/`failed` fields of the
`suite` result(s) a file's final expression yields (a lone result
object, or an array of them). A file that raises an uncaught error
counts as a failure. The process exits non-zero if any test failed.

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

A circular structure passed to `JSON.stringify` (an array or object
reachable from itself) raises a catchable `cycle` error (v0.8) rather
than overflowing the call stack. A non-cyclic shared subtree — the
same array referenced from two places — still serializes fine.

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
              | 'continue'
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
Param       ::= '...' Identifier | Pattern | Identifier '=' Expr

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

As of v0.10 the reference implementation no longer uses `Rc<RefCell<...>>`
for the mutable, potentially-cyclic types. `Array`, `Object`, `Map`,
`Set`, iterators, and closure upvalue cells live on a per-thread arena
heap managed by a tracing mark-sweep collector; a `Value` carries a
small generation-tagged handle into that heap. `Str`, `Range`, and the
immutable `Function` template stay `Rc` — they are acyclic, so a
reference count reclaims them correctly. See Appendix J.

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
3. Only `false` and `null` are falsy (Lua-style); `0`, `''`, `[]`,
   `${}` are truthy.
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
    `'function'`. `str` also gains optional `radix` / `prefix`
    arguments for rendering an `Int` in base 2..=36.
23. **Bitwise operators** (§6.2a) — `& | ^ ~ << >>`, all `Int`-only.
    `>>` is an arithmetic shift; a shift amount outside `0..64`
    raises. Precedence is Rust-style (see §6.1).
24. **`match` expression** (§9.7) — refutable pattern matching with
    literal, binding, wildcard, range, array, object, and or-patterns,
    optional `if` guards, comma-separated arms. Non-exhaustive: a
    `match` with no matching arm evaluates to `null`. Or-pattern
    alternatives may not bind variables.

## Appendix E — Changes in v0.6

All additions; no breaking changes.

25. **`continue`** (§9.4a) — a loop-control expression that skips the
    rest of the current iteration. The iteration contributes `null`;
    `continue` carries no value. `continue` outside a loop is a
    compile-time error. `continue` is now a reserved keyword (§2.3).
26. **Default parameter values** (§10.3) — `fn(a, b = 10) { ... }`. The
    default is bound when the argument slot is `null` (omitted or
    explicitly `null`). Identifier parameters only.
27. **`IO` filesystem entries** (§13.2) — `list_dir`, `mkdir`,
    `remove`, `is_dir`, `is_file`, `stat`.
28. **`Path` native module** (§13.2) — `join`, `dirname`, `basename`,
    `ext`, `is_absolute`.
29. **`Os.run`** (§13.2) — run a subprocess, capturing
    `${code, stdout, stderr}`.
30. **`DateTime` native module** (§13.2) — UTC calendar date/time:
    `now`, `from_ms`, `to_ms`, `format`, `parse`.
31. **`Object` source-stdlib module** (§13.3) — `keys`, `values`,
    `entries`, `from_entries`, `has`, `merge`, `map`, `filter`.

## Appendix F — Changes in v0.7

User-visible breaking change:

32. **`+=` on an array mutates in place** (§7.1, §4.1). Through v0.6,
    `arr += x` rebuilt a fresh array and rebound the name; it now
    mutates the existing array, so aliases observe the change. The
    array-vs-value rule is unchanged — an array right-hand side
    extends, any other value appends one element. Plain `arr + x` is
    unaffected and still yields a fresh array.

Additions (non-breaking):

33. **`Iter` source-stdlib module** (§13.3) — lazy, pull-based
    iterators. Adapters `from`/`count`/`repeat`, lazy combinators
    `map`/`filter`/`take`/`drop`/`enumerate`/`zip`/`chain`, consumers
    `collect`/`reduce`/`for_each`/`count_of`/`find`/`nth`. A pipeline
    carries one element through the whole chain at a time, never
    materializing an intermediate array; `count` / `repeat` are
    infinite sequences.
34. **`Array.push` / `Array.extend`** (§13.3) — in-place array append
    (O(1) amortized) and bulk append, each returning the array.
    Backed by the native `_NativeArray` module.

The structured-error work (`catch` binding the raised value; built-in
errors as `${kind, message, line}` objects) landed separately — see
Appendix G.

## Appendix G — Changes in v0.7b

User-visible breaking change:

35. **Errors are structured values** (§9.6). `raise expr` no longer
    coerces `expr` to a string — `catch (e)` binds the exact value
    raised, whatever its type. A built-in runtime error, when caught,
    is reified into a `${kind, message, line}` object instead of a
    string, so handlers can `match e.kind`. Handlers that did string
    operations on a caught built-in error must adapt (read
    `e.message`, or branch on `e.kind`). `raise` of a string, and the
    string messages raised by native stdlib modules, are unaffected.

## Appendix H — Changes in v0.8

User-visible breaking change:

36. **`for` and spread consume iterator objects** (§7.4, §6.6). An
    Object whose `next` field is callable is now treated as an
    *iterator object* — `for` drives its `next()` protocol, and
    array-literal / function-call spread (`[...it]`, `f(...it)`)
    expand it. This unifies the v0.7 `Iter` module with the language:
    an `Iter` pipeline can be consumed directly, without
    `Iter.collect()`. **Breaking** only for code that iterated a plain
    object which happened to have a callable `next` field — it now
    follows the iterator protocol instead of yielding key/value
    entries. Objects without a callable `next` are unaffected.
    Object-literal spread (`${...x}`) is unchanged and still requires
    an Object.

37. **Integer overflow raises a catchable error** (§6.2b). `Int`
    arithmetic — `+`, `-`, `*`, and unary `-` — is now *checked*: a
    result outside the signed 64-bit range raises a runtime error with
    `kind: 'overflow'` instead of wrapping (debug builds previously
    panicked; release builds wrapped). Caught, it reifies to
    `${kind: 'overflow', message: 'integer overflow', line}` like every
    other built-in error. `^^` is unaffected — it always yields
    `Float`. **Breaking** only for code that relied on silent
    two's-complement wraparound, expected to be effectively no existing
    Tigr programs.

Additive changes:

38. **Tail calls and bounded recursion** (§10.5). A call in tail
    position now reuses the current call frame instead of pushing a new
    one, so tail-recursive functions — including mutually-recursive
    ones — run in constant frame space, to any depth. Tail position
    propagates through `if`/`else` branches, `match` arms, and block
    tail expressions. Independently, call depth is now bounded:
    recursion that genuinely nests past the limit raises a catchable
    `stack_overflow` error (reified as
    `${kind: 'stack_overflow', message: 'call stack depth exceeded',
    line}`) instead of crashing the process.

39. **Stack traces on uncaught errors** (§9.6). When a runtime error
    escapes every `try` handler, the rendered report now prints a
    `stack trace` block beneath the source snippet, listing each active
    call frame innermost-first as `<name> at <file>:<line>`. Function
    names are inferred from the binding (`f := fn(){}` → `f`), with
    `<anonymous>` for an unbound `fn` and `<main>` for the top-level
    program. Tail calls reuse their frame (item 38), so a tail-recursive
    function appears once. The trace is omitted when there is a single
    frame (it would only repeat the snippet) and for *caught* errors —
    a value bound by `catch` still carries only `kind`/`message`/`line`.

40. **`JSON.stringify` cycle detection** (§13.4, §9.6). `JSON.stringify`
    of a circular structure — an array or object reachable from itself —
    now raises a catchable error with `kind: 'cycle'` (reified as
    `${kind: 'cycle', message: 'circular reference', line}`) instead of
    recursing until the host call stack overflows and crashes the
    process. A non-cyclic shared subtree (the same array referenced from
    two places) still serializes normally. This is the one native
    stdlib error that is a structured built-in error rather than a plain
    string message.

## Appendix I — Changes in v0.9

Additive changes:

41. **`Test` module and `tigr test`** (§13.3). A new source-stdlib
    module, `import 'Test'`, provides a small test framework written in
    tigr: assertions (`assert`, `assert_eq`, `assert_ne`,
    `assert_raises`, `fail`) that `raise` on failure, and `case` /
    `suite` for grouping tests as plain data. `suite` runs an array of
    `case`s, prints a `PASS`/`FAIL` line per case, and returns a result
    object `${name, passed, failed, total, failures}`. A new CLI
    subcommand, `tigr test [path]`, discovers test files (`*_test.tg`
    anywhere, plus every `.tg` file under a `tests/` directory), runs
    each, sums the `suite` results, and exits non-zero if any test
    failed.

42. **`Map` and `Set` types** (§13.3). Two new native value types.
    `Map` is an arbitrary-keyed, insertion-ordered dictionary — keys
    may be any null / bool / int / string value, unlike `Object`'s
    string-only keys. `Set` is an insertion-ordered collection of
    unique values with `union` / `intersection` / `difference`. Both
    support `m[k]` / `s[x]` indexing, `#` length, and `for` iteration;
    a `Float` or collection key/element raises a new `invalid_key_type`
    error; neither is JSON-serializable. `type()` reports `"map"` /
    `"set"`. Imported as `import 'Map'` / `import 'Set'`, backed by the
    native `_NativeMap` / `_NativeSet` modules. The roadmap's "stringify
    keys internally" option was dropped — distinct native types give
    true O(1) operations and keep `1` and `'1'` distinct keys.

43. **`Object.has` is O(1); `keys`/`values`/`entries` are O(n)**
    (§13.3). `Object.has` now uses a native `contains_key` (the new
    `_NativeObject` module) instead of an O(n) key scan, and still
    distinguishes a missing key from a present `null` value.
    `Object.keys` / `values` / `entries` append in place rather than
    rebuilding the accumulator array each step, dropping their cost
    from O(n²) to O(n). Behaviour is unchanged — purely a speed fix.

44. **`Random` module** (§13.2). A new native module, `import 'Random'`,
    for seedable pseudo-random numbers: `seed`, `float`, `int(lo, hi)`
    (inclusive both ends), `bool`, `choice`, `range`, and `shuffle`
    (non-destructive). `Random` and the bare `rand()` built-in now share
    a single per-thread PRNG stream, so `Random.seed(n)` makes `rand()`
    reproducible too — previously `rand()` was unseedable. Behaviour of
    `rand()` is otherwise unchanged.

45. **String formatting** (§13.3). Two new `String` functions sharing
    one spec mini-language. `String.format(value, spec)` formats a
    single value — width, alignment, fill, sign, precision, thousands
    grouping, and the type codes `s d f e E x X b o`. `String.printf(
    template, args)` substitutes `%(SPEC)` placeholders, each SPEC being
    the `format` mini-language and `%%` a literal percent. The template
    marker is `%(...)` rather than `{}` because `{}` is already string
    interpolation. Previously interpolation only did bare `str(expr)` —
    no width, precision, or alignment.

## Appendix J — Changes in v0.10

46. **Tracing garbage collector** (§15.1). The reference implementation
    replaces the `Rc<RefCell<...>>` representation of the mutable,
    potentially-cyclic value types — `Array`, `Object`, `Map`, `Set`,
    iterators, and closure upvalue cells — with a hand-written
    mark-sweep collector over a per-thread arena heap; a `Value` now
    carries a small generation-tagged handle into that heap. Reference
    cycles (a self-referential object, two closures that capture each
    other) are reclaimed instead of leaking forever. Collection is
    automatic — it runs at VM dispatch-loop safepoints once the
    live-object count crosses a growing threshold — and has no effect
    observable from tigr code beyond reclaiming memory. `Str`, `Range`,
    and the immutable `Function` template stay reference-counted
    (acyclic, so a count suffices). This is an implementation change;
    the language is unaffected.

47. **`gc()` built-in** (§13). A new zero-argument built-in returning
    the garbage collector's counters as an object,
    `${live, collections, allocated, freed}` — `live` is the current
    managed-object count, `collections` the number of collections run,
    and `allocated` / `freed` the lifetime totals. Read-only: collection
    itself is automatic and cannot be forced from tigr code. Intended
    for tests and for observing memory behaviour.
