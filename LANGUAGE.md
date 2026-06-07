# Tigr Language Specification

Version 0.17 (draft) — written as the target for a bytecode VM implementation.

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
spawn  select  parallel  go  yield  gen
```

Note: `floor`, `ceil`, `rand`, `for[]`, `while[]` are no longer keywords — see
§13. The `[]` suffix on `for`/`while` is now a separate token. The
concurrency keywords on the third line are `spawn`/`select`/`parallel`
(actors, Appendix L) and `go`/`yield`/`gen` (green threads and
generators, Appendix P).

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

Two forms, both producing the same `String` type (see §8):

- **Single-quoted** `'…'` — with `{expr}` interpolation and backslash
  escapes.
- **Double-quoted** `"…"` — a fully raw literal: no interpolation and
  no escapes. Every character between the quotes is literal, so `{`,
  `}`, and `\` need no escaping. A `"` cannot appear inside.

```
'hello'
'count: {n}'
'literal brace: \{'
"raw: { } and \ need no escaping"
"C:\Users\me"
```

---

## 3. Types

| Type           | Examples                          | `type()`         | Notes                                            |
|----------------|-----------------------------------|------------------|--------------------------------------------------|
| `Int`          | `0`, `42`, `-7`                   | `'int'`          | 64-bit signed                                    |
| `Float`        | `3.14`, `0.0`                     | `'float'`        | 64-bit IEEE-754                                  |
| `String`       | `'hello'`                         | `'string'`       | Immutable; UTF-8                                 |
| `Bool`         | `true`, `false`                   | `'bool'`         |                                                  |
| `Null`         | `null`                            | `'null'`         |                                                  |
| `Array`        | `[1, 'two', true]`                | `'array'`        | Heterogeneous, **reference type**                |
| `Object`       | `${ name: 'a', age: 1 }`          | `'object'`       | String keys, **reference type**                  |
| `Range`        | `0..10`, `0..=10`, `0..10:2`      | `'range'`        | First-class iterable                             |
| `Function`     | `fn(x) { x * 2 }`                 | `'function'`     | Closures over lexical env; native built-ins too  |
| `Map`          | `Map.new()`                       | `'map'`          | Arbitrary keys, **reference type** (v0.9)        |
| `Set`          | `Set.new()`                       | `'set'`          | Unique values, **reference type** (v0.9)         |
| `Bytes`        | `Bytes.new(4)`                    | `'bytes'`        | Mutable byte buffer, **reference type** (v0.13)  |
| `BigInt`       | `BigInt.new(2)`                   | `'bigint'`       | Arbitrary-precision integer; immutable (v0.13)   |
| `Channel`      | `Channel.new()`                   | `'channel'`      | Cross-actor message conduit (v0.14)              |
| `Task`         | `spawn fn() { }`                  | `'task'`         | Handle to a spawned actor (v0.14)                |
| `Socket`       | `Net.listen('0.0.0.0', 0)`        | `'socket'`       | Network socket (v0.15)                           |
| `GreenThread`  | `go fn() { }`                     | `'green_thread'` | Green-thread (coroutine) handle (Appendix P)     |
| `LocalChannel` | `LocalChannel.new()`              | `'local_channel'`| Intra-actor message conduit (Appendix P)         |

`Int` and `Float` are jointly referred to as **Number**. Mixed-arithmetic
between them follows §6.2. The `type()` built-in (§13.1) reports the
tag in the third column.

`Array`, `Object`, `Map`, `Set`, and `Bytes` are **reference types**:
passing one to a function or binding it to a new name does not copy
(a change from 0.1). `Channel`, `Task`, `Socket`, `GreenThread`, and
`LocalChannel` are opaque handles, likewise shared rather than copied.
Every other type — including `BigInt`, which is heap-backed but
immutable — behaves as a plain value.

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
0..10 |> Array.sum()        // == Array.sum(0..10)
```

Pipe is left-associative. Evaluation order is strictly left-to-right.

### 6.5 Indexing and member access

```
arr[0]
arr[i + 1]
obj['key']
obj.key                     // sugar for obj['key']
'hello'[1]                  // == 'e'  (strings are indexable)
arr[1..3]                   // == [arr[1], arr[2]]  (slice with a Range)
```

Out-of-range numeric index → `null`. Missing object key → `null`.
Negative array indices count from the end: `arr[-1]` is the last element.

Indexing an `Array`, `Bytes`, or `String` with a **`Range`** key slices it,
returning a fresh sub-collection of the same type (a copy — like
`Array.slice` / `Bytes.slice`). `coll[Int]` yields one element; `coll[Range]`
yields a sub-collection. Negative endpoints count from the end and
out-of-range endpoints clamp, so `arr[0..1000]` is the whole array. The
range's step and direction carry through: `arr[0..#arr:2]` takes every other
element and a descending range reverses (`arr[#arr-1..=0]`). Because a range
literal fixes its direction from the written endpoints, an end-relative slice
that must stay ascending uses `#`, not a negative end — `arr[1..#arr-1]`, not
`arr[1..-1]` (the latter is a descending range). A `String` slice is
character-indexed and therefore O(n), like `s[i]`.

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
- Indexing into a range: `(0..10:2)[1]` → 2
- Indexing a collection: `[10, 20, 30, 40][1..3]` → `[20, 30]` (see §6.5)

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
for (sq, 0..=4 |> Iter.from() |> Iter.map(fn(n){n*n})) {
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

### 8.2 String forms — interpolated and raw

Tigr has two string literals. They produce the same `String` type and
differ on exactly one axis — whether the lexer interpolates.

**Single-quoted `'…'` — interpolated.** `{expr}` is replaced by the
result of `str(expr)` (see §13). Use `\{` for a literal `{`. The
interpolation grammar matches a single tigr expression. Backslash
escapes (`\n`, `\t`, `\r`, `\\`, `\'`, `\{`) are processed.

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

**Double-quoted `"…"` — raw, non-interpolating.** A `{` is an ordinary
character, and backslash is literal — there are *no* escapes at all.
Everything between the quotes is taken verbatim. This is the form for
text that genuinely contains braces or backslashes — JSON or code
templates, glob/format specs, Windows paths.

```
"hello {name} world"        // 'hello {name} world' — no interpolation
"*.{rs,tg}"                 // brace pattern, verbatim
"C:\Users\me"               // backslashes are literal
```

Because there are no escapes, a `"` cannot appear inside a `"…"`
string — reach for `'…'` (with `\'` or interpolation) when the text
contains a double quote. Both forms share every operator, the same
UTF-8 character semantics, and indexing (§8.1); they compare equal
when they hold the same characters (`"ab" == 'ab'`).

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

- `kind` — a stable snake-case tag, one of: `type_mismatch`,
  `div_by_zero`, `index_out_of_bounds`, `arity_mismatch`,
  `not_callable`, `invalid_index_type`, `invalid_key_type`,
  `immutable_target`, `import_failed`, `overflow`, `stack_overflow`,
  `stack_underflow`, `cycle`, `no_match`, `not_sendable`,
  `channel_closed`. (`invalid_key_type` arrived with `Map`/`Set` in
  v0.9; `not_sendable` and `channel_closed` with actors in v0.14.)
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
mod   := import './plugins/{name}';   // any expression
```

`import` takes an **arbitrary expression**, evaluates it, and expects
the result to be a string path. The whole expression up to the end of
the statement is consumed, so concatenation needs no parentheses
(`import base + '/' + name`). Path resolution and the string check
happen at runtime; a path that does not evaluate to a string raises a
catchable `type_mismatch` error.

The resolved string has two flavors:

- **Bare names** (no `/`, `\`, or `.`) — resolved against the
  native-module registry built into the interpreter (e.g. `IO`, `Os`,
  `Time` in v0.3 Phase 3+). An unknown bare name raises a catchable
  error.
- **Path-shaped strings** — resolved against the importing file's
  directory (per spec §12). `.tg` is appended automatically if absent.
  A missing file raises a catchable `import_failed` error.

`import` returns the imported module's final expression value.

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

> The navigable reference for every module below, with full signatures and runnable examples, lives under [`docs/stdlib/`](docs/stdlib/README.md). This section is the normative contract.

Built-ins are ordinary bindings in the root environment. They can be
shadowed, passed around, and stored.

### 13.1 Required built-ins for v0.2

> Navigable reference: [`docs/stdlib/builtins.md`](docs/stdlib/builtins.md).

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
| `join`    | `join(task) -> value`    | Block for a `spawn`ed actor's result (v0.14, Appendix L) |
| `wait`    | `wait(seconds) -> null`  | Cooperatively pause the running coroutine for `seconds`, letting siblings run (Appendix P) |

`gc()` returns a read-only snapshot of the tracing collector's state
(§15.1): `live` is the current managed-object count, `collections` the
number of collections run so far, and `allocated` / `freed` the lifetime
totals. Collection is automatic — `gc()` only observes it.

`type(x)` returns the value's type as a lowercase string. The complete
set, with the version each was introduced:

- core (v0.2): `'null'`, `'bool'`, `'int'`, `'float'`, `'string'`,
  `'array'`, `'object'`, `'range'`, `'function'`
- `'map'`, `'set'` (v0.9)
- `'bytes'`, `'bigint'` (v0.13)
- `'channel'`, `'task'` (v0.14)
- `'socket'` (v0.15)
- `'green_thread'`, `'local_channel'` (green threads, Appendix P)

Both user closures and native built-ins report `'function'` — `type`
deliberately collapses the two. A `gen fn` literal is itself an
ordinary function value (`'function'`); *calling* it yields a plain
`${next: fn()}` object, so a generator instance and an `Iter`-module
iterator both report `'object'`.

`str` takes an optional **radix** and **prefix** (v0.5). `str(x)` is
the canonical form. `str(n, radix)` renders an `Int` `n` in `radix`
(an `Int` in `2..=36`, lowercase digits); a non-`Int` value or an
out-of-range radix raises. `str(n, radix, prefix)` with `prefix` a
`Bool` prepends the literal marker — `0b` / `0o` / `0x` for radix
2 / 8 / 16 (any other radix with `prefix == true` raises). A negative
number's `-` precedes the prefix.

### 13.2 Native modules (v0.3)

Imported via `import 'Name'` (no path separators). Each native module
returns an object whose entries are ordinary tigr values; users can
destructure or pass them like any other binding.

#### `IO`

> Navigable reference: [`docs/stdlib/io.md`](docs/stdlib/io.md).

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
| `read_bytes`  | `read_bytes(path) -> Bytes`        | Read entire file as raw bytes; raises on error (v0.13) |
| `write_bytes` | `write_bytes(path, bytes) -> null` | Overwrite file with raw bytes; raises on error (v0.13) |
| `append_bytes`| `append_bytes(path, bytes) -> null`| Append raw bytes; creates if missing; raises on error (v0.13) |

#### `Os`

> Navigable reference: [`docs/stdlib/os.md`](docs/stdlib/os.md).

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

> Navigable reference: [`docs/stdlib/path.md`](docs/stdlib/path.md).

Pure path-string manipulation; nothing here touches the filesystem.
Paths are POSIX-style on every platform (`/` separators, a leading `/`
is absolute), so the same logical paths behave identically on Linux,
macOS, Windows, and the browser.

| Entry         | Signature                          | Behavior                                          |
|---------------|------------------------------------|---------------------------------------------------|
| `join`        | `join(...parts) -> String`         | Join path segments with `/` (an absolute segment resets) |
| `dirname`     | `dirname(path) -> String`          | The parent directory (`''` if none)               |
| `basename`    | `basename(path) -> String`         | The final component (`''` if none)                |
| `ext`         | `ext(path) -> String`              | File extension without the dot (`''` if none)     |
| `is_absolute` | `is_absolute(path) -> Bool`        | True if the path is absolute                      |

Every `Path` entry raises on a non-String argument.

#### `Time`

> Navigable reference: [`docs/stdlib/time.md`](docs/stdlib/time.md).

| Entry      | Signature                | Behavior                                |
|------------|--------------------------|-----------------------------------------|
| `now_ms`   | `now_ms() -> Int`        | Milliseconds since UNIX epoch           |
| `now_ns`   | `now_ns() -> Int`        | Nanoseconds since UNIX epoch            |
| `sleep_ms` | `sleep_ms(n) -> null`    | Block the thread for `n` ms             |

#### `DateTime` (v0.6)

> Navigable reference: [`docs/stdlib/datetime.md`](docs/stdlib/datetime.md).

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
object — pass a `Time.now_ms()` or `to_ms(...)` result.

#### `Random` (v0.9)

> Navigable reference: [`docs/stdlib/random.md`](docs/stdlib/random.md).

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

#### `Bytes` (v0.13)

> Navigable reference: [`docs/stdlib/bytes.md`](docs/stdlib/bytes.md).

`Bytes` is a value type as well as a module — a **mutable byte buffer**
(`Vec<u8>`), the binary counterpart to the UTF-8-only `String`. It is
GC-managed like `Array`/`Map`/`Set`, and supports the collection
operators directly:

- `b[i]` reads the byte at `i` as an `Int` 0–255; a negative `i` counts
  from the end; an out-of-range `i` yields `null`.
- `b[i] = n` writes a byte in place; `n` must be an `Int` 0–255.
- `#b` is the byte count.
- `for (i, byte, b)` iterates `(index, byte-as-Int)`.
- `[...b]` spreads the buffer into an Array of `Int`s, and array
  destructuring works — `[first, ...rest] := b` binds `first` to an
  `Int` and `rest` to a new `Bytes`.
- `a + b` concatenates two buffers into a new one; `b += other` extends
  `b` in place. Both operands must be `Bytes`.
- `==` compares buffers by content. There is no ordering (`<`, `>`).
- `type(b)` is `'bytes'`; `str(b)` is a hex view, `Bytes[de ad be ef]`,
  truncated for large buffers.
- A `Bytes` cannot be a `Map`/`Set` key (it is mutable) and is not
  JSON-serializable.

The module supplies construction, conversion, growth, and a named
family of fixed-width integer readers/writers for binary protocols.

| Entry          | Signature                                  | Behavior                                                       |
|----------------|--------------------------------------------|----------------------------------------------------------------|
| `new`          | `new(n [, fill]) -> Bytes`                 | `n` bytes, zero- or `fill`-filled; raises if `n < 0`           |
| `from_array`   | `from_array(arr) -> Bytes`                 | Pack an `[Int]` (each 0–255); raises otherwise                 |
| `from_string`  | `from_string(s) -> Bytes`                  | The UTF-8 encoding of `s`                                      |
| `from_hex`     | `from_hex(s) -> Bytes`                     | Decode a hex string (whitespace ignored); raises `decode` on bad input |
| `from_base64`  | `from_base64(s) -> Bytes`                  | Decode standard base64; raises `decode` on bad input           |
| `to_array`     | `to_array(b) -> Array<Int>`                | The buffer as one `Int` per byte                               |
| `to_string`    | `to_string(b) -> String`                   | Decode as UTF-8; raises a catchable `decode` error if invalid  |
| `to_hex`       | `to_hex(b) -> String`                      | Lower-case hex, two digits per byte                            |
| `to_base64`    | `to_base64(b) -> String`                   | Standard-alphabet base64 with `=` padding                      |
| `push`         | `push(b, byte) -> Bytes`                   | Append one byte in place; returns `b`                          |
| `extend`       | `extend(b, other) -> Bytes`                | Append every byte of `other` in place; returns `b`             |
| `slice`        | `slice(b, start, end) -> Bytes`            | A new buffer of `b[start..end]`; negative indices count from the end, bounds are clamped |
| `concat`       | `concat(a, b) -> Bytes`                    | A new buffer of `a` followed by `b`                            |
| `read_u8` …    | `read_<type>(b, offset) -> Int`            | Read a fixed-width integer at `offset` (see below)             |
| `write_u8` …   | `write_<type>(b, offset, value) -> Bytes`  | Write a fixed-width integer at `offset`, in place; returns `b` |

The integer family is named, not parameterized: `<type>` is `u8`/`i8`
(no endianness), or one of `u16`/`i16`/`u32`/`i32`/`u64`/`i64` followed
by `_be` (big-endian) or `_le` (little-endian) — e.g. `read_u32_be`,
`write_i16_le`. A read or write whose `offset + width` falls outside the
buffer raises a catchable error. `write_*` raises if `value` does not
fit the field (an unsigned writer also rejects a negative `value`). An
unsigned 64-bit *read* of a value above the `Int` (`i64`) range raises a
catchable `overflow` — the same error class as v0.8 arithmetic overflow.

#### `BigInt` (v0.13)

> Navigable reference: [`docs/stdlib/bigint.md`](docs/stdlib/bigint.md).

`BigInt` is a value type as well as a module — an **arbitrary-precision
integer**, the complement to the fixed-width `Int`. Where an `Int`
operation that exceeds the 64-bit range raises a catchable `overflow`
(§6.2), a `BigInt` simply grows. It is immutable, so — unlike `Bytes` —
it is an ordinary *value* type, not a reference type.

A `BigInt` is created **explicitly**; an overflowing `Int` is *not*
promoted automatically (that would silently change a value's type and
defeat the `overflow` error). Once created it works with the ordinary
operators:

- `+ - * ^^`, unary `-`, and `%` behave as for `Int`, but never
  overflow. An `Int` operand is promoted to `BigInt`, so `b + 1` works;
  a `Float` operand promotes the `BigInt` to `Float` (the result is a
  `Float`), as with `Int`/`Float` mixing.
- `^^` with a non-negative integer exponent stays exact and yields a
  `BigInt`; a negative or fractional exponent falls back to `Float`.
- `/` is **exact-or-raise**: `a / b` yields a `BigInt` only when the
  division leaves no remainder; otherwise it raises a catchable
  `inexact_division` error, and `a / 0` raises `div_by_zero`. This keeps
  every `BigInt` operator closed over exact integers — it never silently
  produces a lossy `Float`. Use `BigInt.divmod` / `BigInt.div` for
  integer (truncating) division.
- `==` / `!=` and the ordering operators compare `BigInt`s, and compare
  a `BigInt` against an `Int` by value (`BigInt.new(5) == 5`). A
  `BigInt` *orders* against a `Float` but is never `==` to one (a value
  beyond the float's exact range could compare spuriously equal).
- The bitwise operators (`& | ^ ~ << >>`) are `Int`-only and raise on a
  `BigInt`.
- `type(b)` is `'bigint'`; `str(b)` is the decimal form. `int(b)`
  narrows back to an `Int` (raising `overflow` if it does not fit);
  `float(b)` converts (lossily). A `BigInt` cannot be a `Map`/`Set` key
  and is not JSON-serializable.

| Entry          | Signature                                  | Behavior                                                              |
|----------------|--------------------------------------------|-----------------------------------------------------------------------|
| `new`          | `new(x) -> BigInt`                         | From an `Int`, a decimal `String` (trimmed, optional sign), or a `BigInt`; a malformed string raises a catchable `parse` error |
| `to_int`       | `to_int(b) -> Int`                         | Narrow to an `Int`; raises `overflow` if outside the `i64` range      |
| `to_float`     | `to_float(b) -> Float`                     | Convert to a `Float` (lossy; saturates to `±inf`)                     |
| `to_str_radix` | `to_str_radix(b, radix) -> String`         | The value in base `radix` (2–36)                                      |
| `divmod`       | `divmod(a, b) -> [BigInt, BigInt]`          | `[quotient, remainder]`, truncating toward zero; raises `div_by_zero` |
| `div`          | `div(a, b) -> BigInt`                      | The truncating integer quotient; raises `div_by_zero`                 |
| `abs`          | `abs(b) -> BigInt`                         | Absolute value                                                       |
| `pow`          | `pow(base, exp) -> BigInt`                 | `base` to a non-negative integer `exp`; a negative `exp` raises       |
| `sign`         | `sign(b) -> Int`                           | `-1`, `0`, or `1`                                                     |
| `is_negative`  | `is_negative(b) -> Bool`                   | `true` for a value below zero                                         |
| `gcd`          | `gcd(a, b) -> BigInt`                      | Greatest common divisor (non-negative)                                |
| `lcm`          | `lcm(a, b) -> BigInt`                      | Least common multiple                                                 |

Every module function that takes a number accepts an `Int` as well as a
`BigInt`.

#### `Net` (v0.15)

> Navigable reference: [`docs/stdlib/net.md`](docs/stdlib/net.md).

`Net` opens **network sockets** — a TCP listener and TCP streams, UDP
datagram sockets, and TLS-encrypted client connections. A socket is a
`Value` in its own right (`type(s)` is `'socket'`): like a channel or a
task it is `Arc`-backed and **sendable**, so it crosses an actor
boundary. That is the idiom for a server — `accept` a connection, then
`spawn` a handler actor that captures the socket.

A socket's `==` is **identity** (handle equality, like a channel); a
socket is not a `Map`/`Set` key and is not JSON-serializable.

Reads come in two layers. The low-level `read(sock, n)` returns up to
`n` bytes as a `Bytes`; an **empty `Bytes` means end-of-stream**. The
helpers `read_exact` / `read_line` / `read_until` / `read_all` build
framed reads on top of it — the socket carries an internal buffer, so a
helper that over-reads keeps the surplus for the next call. `read_line`
and `read_until` return `null` at end-of-stream.

A failed operation raises a catchable **structured error**
`${kind, message}`, so `catch` code can dispatch on `e.kind`. `kind` is
one of `timeout`, `closed`, `eof`, `refused`, `dns`, `tls`,
`addr_in_use`, `decode`, or `io`. By default a read or write blocks
indefinitely; `set_timeout(sock, ms)` bounds them, and a timed-out
operation raises `timeout`. `close` is idempotent and unblocks an actor
stuck mid-`read` on the same socket — or stuck in `accept` on a
listener, which then raises `closed`. `select` is *not* extended to
sockets — to multiplex, bridge a socket to a channel with a reader
actor.

| Entry         | Signature                                  | Behavior                                                              |
|---------------|--------------------------------------------|-----------------------------------------------------------------------|
| `listen`      | `listen(host, port) -> socket`             | A TCP listener bound to `host:port`; port `0` lets the OS assign one  |
| `accept`      | `accept(listener) -> socket`               | Block for the next inbound connection                                 |
| `connect`     | `connect(host, port) -> socket`            | Open a TCP stream to `host:port`                                       |
| `connect_tls` | `connect_tls(host, port, [ca_pem]) -> socket` | Open a TLS stream; `host` is verified against the server certificate; optional `ca_pem` adds trusted roots |
| `listen_tls`  | `listen_tls(host, port, cert_pem, key_pem) -> socket` | A TLS server listener; `accept` yields encrypted server sockets   |
| `bind`        | `bind(host, port) -> socket`               | A UDP datagram socket bound to `host:port`                             |
| `send_to`     | `send_to(sock, bytes, host, port) -> Int`  | Send one UDP datagram; returns the byte count sent                     |
| `recv_from`   | `recv_from(sock, n) -> Object`             | Receive one datagram (≤ `n` bytes) as `${data: Bytes, host, port}`     |
| `read`        | `read(sock, n) -> Bytes`                   | Read up to `n` bytes; an empty `Bytes` is end-of-stream                |
| `write`       | `write(sock, bytes) -> Int`                | Write every byte; returns the count written                           |
| `read_exact`  | `read_exact(sock, n) -> Bytes`             | Read exactly `n` bytes; raises `eof` if the stream ends first          |
| `read_line`   | `read_line(sock) -> String`                | One `\n`-terminated line, trailing `\r\n`/`\n` stripped; `null` at EOF; raises `decode` on invalid UTF-8 |
| `read_until`  | `read_until(sock, byte) -> Bytes`          | Read up to and including `byte`; `null` at end-of-stream               |
| `read_all`    | `read_all(sock) -> Bytes`                  | Every remaining byte to end-of-stream                                  |
| `local_addr`  | `local_addr(sock) -> Object`               | The socket's own address as `${host, port}`                            |
| `peer_addr`   | `peer_addr(sock) -> Object`                | The connected peer's address as `${host, port}`                        |
| `set_timeout` | `set_timeout(sock, ms) -> null`            | Bound reads/writes to `ms` ms; `ms <= 0` clears the timeout            |
| `close`       | `close(sock) -> null`                      | Close the socket; idempotent, unblocks a reader stuck mid-`read` or an actor stuck in `accept` |

### 13.3 Source-stdlib modules (v0.3)

These ship as tigr `.tg` files embedded in the interpreter. `import`
returns an Object of functions; signatures are the same as any
user-defined module.

#### `Array`

> Navigable reference: [`docs/stdlib/array.md`](docs/stdlib/array.md).

Callbacks receive `(elem, index, whole_array)`; unused trailing args
are dropped per spec §10.3.

| Entry        | Signature                             | Behavior                                                       |
|--------------|---------------------------------------|----------------------------------------------------------------|
| `push`       | `push(arr, value) -> Array`           | Append `value` in place (O(1) amortized); returns `arr`        |
| `extend`     | `extend(arr, other) -> Array`         | Append every element of `other` in place; returns `arr`        |
| `pop`        | `pop(arr) -> value`                   | Remove and return the last element; `null` if empty           |
| `shift`      | `shift(arr) -> value`                 | Remove and return the first element; `null` if empty          |
| `unshift`    | `unshift(arr, value) -> Array`        | Prepend `value` in place; returns `arr`                        |
| `insert`     | `insert(arr, index, value) -> Array`  | Insert `value` at `index` in place; returns `arr`              |
| `remove`     | `remove(arr, index, count?) -> value` | Remove and return one element (`null` if out of range), or a `count`-long sub-array |
| `clear`      | `clear(arr) -> Array`                 | Empty the array in place; returns `arr`                        |
| `create`     | `create(len, func) -> Array`          | A fresh `len`-element array of `func(i)`                       |
| `concat`     | `concat(a, b) -> Array`               | A fresh array of `a` followed by `b`                           |
| `map`        | `map(arr, func) -> Array`             | A fresh array of `func` applied to each element                |
| `filter`     | `filter(arr, pred) -> Array`          | A fresh array of the elements for which `pred` is truthy       |
| `reduce`     | `reduce(arr, func, seed) -> value`    | Fold left-to-right from `seed`                                 |
| `flatten`    | `flatten(arr) -> Array`               | A fresh array with one level of nesting removed                |
| `reverse`    | `reverse(arr) -> Array`               | A fresh array in reverse order                                 |
| `index`      | `index(arr, elem) -> Int`             | First index of `elem` (by `==`), or `-1`                       |
| `find`       | `find(arr, pred) -> value`            | First element for which `pred` holds, or `null`                |
| `find_index` | `find_index(arr, pred) -> Int`        | Index of the first such element, or `-1`                       |
| `any`        | `any(arr, pred) -> Bool`              | True if `pred` holds for some element                          |
| `all`        | `all(arr, pred) -> Bool`              | True if `pred` holds for every element                         |
| `head`       | `head(arr, n) -> Array`               | First `n` elements; a negative `n` drops `n` from the end      |
| `tail`       | `tail(arr, n) -> Array`               | Last `n` elements; a negative `n` drops `n` from the start     |
| `take`       | `take(arr, n) -> Array`               | First `n` elements; a negative `n` clamps to 0                 |
| `drop`       | `drop(arr, n) -> Array`               | All but the first `n`; a negative `n` clamps to 0              |
| `slice`      | `slice(arr, start, end) -> Array`     | A fresh `arr[start..end]`; negative indices count from the end |
| `sum`        | `sum(arr) -> Number`                  | Sum of the elements                                            |
| `max_of`     | `max_of(arr) -> value`                | Largest element                                                |
| `min_of`     | `min_of(arr) -> value`                | Smallest element                                               |
| `uniq`       | `uniq(arr) -> Array`                  | A fresh array with duplicates dropped, first occurrence kept   |
| `zip`        | `zip(a, b) -> Array`                  | A fresh array of `[a[i], b[i]]` pairs, length of the shorter   |
| `join`       | `join(arr, sep) -> String`            | The elements (via `str`) joined by `sep`                       |
| `group_by`   | `group_by(arr, key) -> Map`           | Group elements into a `Map` keyed by `key(elem)`               |
| `chunk`      | `chunk(arr, size) -> Array`           | Split into consecutive `size`-long sub-arrays                  |
| `windows`    | `windows(arr, size) -> Array`         | Every overlapping `size`-long sub-array                        |
| `partition`  | `partition(arr, pred) -> Array`       | `[matching, non_matching]`                                     |
| `flat_map`   | `flat_map(arr, func) -> Array`        | `map` then `flatten` one level                                 |
| `count_of`   | `count_of(arr, pred) -> Int`          | How many elements satisfy `pred`                               |
| `sort`       | `sort(arr) -> Array`                  | A fresh array sorted ascending                                 |
| `sort_by`    | `sort_by(arr, key) -> Array`          | A fresh array sorted ascending by `key(elem)`                  |

The eight in-place mutators (`push`, `extend`, `pop`, `shift`,
`unshift`, `insert`, `remove`, `clear`) are backed by the native
`_NativeArray` module — pure tigr can grow an array (`+`/spread) but
cannot shrink one. `pop` / `shift` / `remove` return the removed
element(s); the other five return `arr`. Negative indices count from
the end. Contrast `concat`, which builds a fresh array.

`head`/`tail` accept a negative `n` (Python-slice style):
`head(arr, -1)` is all but the last element, `tail(arr, -1)` all but
the first — whereas `take`/`drop` clamp a negative `n` to 0. `group_by`
returns a `Map` (so non-string keys work); the other combinators build
fresh arrays.

#### `Iter` (v0.7)

> Navigable reference: [`docs/stdlib/iter.md`](docs/stdlib/iter.md).

Lazy, pull-based iterators. An iterator is an object `${next: fn()}`
whose `next()` yields `${done: true}` or `${done: false, value}`.

| Entry        | Signature                          | Behavior                                                  |
|--------------|------------------------------------|-----------------------------------------------------------|
| `from`       | `from(iterable) -> Iterator`       | Wrap any iterable (Array, Range, String, …) as an iterator |
| `count`      | `count(start) -> Iterator`         | The infinite sequence `start, start+1, start+2, …`        |
| `repeat`     | `repeat(value) -> Iterator`        | The infinite sequence of `value`                          |
| `map`        | `map(it, func) -> Iterator`        | Lazily apply `func` to each value                         |
| `filter`     | `filter(it, pred) -> Iterator`     | Lazily keep the values for which `pred` holds             |
| `take`       | `take(it, n) -> Iterator`          | The first `n` values, then stop                           |
| `take_while` | `take_while(it, pred) -> Iterator` | Values up to the first for which `pred` fails             |
| `drop`       | `drop(it, n) -> Iterator`          | Skip the first `n` values                                 |
| `drop_while` | `drop_while(it, pred) -> Iterator` | Skip values up to the first for which `pred` fails        |
| `enumerate`  | `enumerate(it) -> Iterator`        | Pair each value with its 0-based index                    |
| `zip`        | `zip(a, b) -> Iterator`            | Pair values from `a` and `b`; ends with the shorter       |
| `chain`      | `chain(a, b) -> Iterator`          | Every value of `a`, then every value of `b`               |
| `collect`    | `collect(it) -> Array`             | Drain the iterator into a fresh array                     |
| `reduce`     | `reduce(it, func, seed) -> value`  | Fold the iterator from `seed`                             |
| `for_each`   | `for_each(it, func) -> Null`       | Run `func` on each value for effect                       |
| `count_of`   | `count_of(it) -> Int`              | Drain the iterator; return how many values it yielded     |
| `find`       | `find(it, pred) -> value`          | First value for which `pred` holds, or `null`             |
| `nth`        | `nth(it, n) -> value`              | The `n`-th value (0-based), or `null`                     |

A combinator does no work until a consumer pulls from it, so a pipeline
never materializes an intermediate array. `count` / `repeat` are
infinite and must be bounded by `take` (or a short-circuiting `find` /
`nth`). Pure tigr — closures capture the source iterator; no VM support
is required.

#### `Object` (v0.6)

> Navigable reference: [`docs/stdlib/object.md`](docs/stdlib/object.md).

Callbacks receive `(value, key, whole_object)`.

| Entry          | Signature                       | Behavior                                                  |
|----------------|---------------------------------|-----------------------------------------------------------|
| `keys`         | `keys(obj) -> Array`            | The keys, in insertion order                              |
| `values`       | `values(obj) -> Array`          | The values, in insertion order                            |
| `entries`      | `entries(obj) -> Array`         | `[key, value]` pairs, in insertion order                  |
| `from_entries` | `from_entries(pairs) -> Object` | Build an object from `[key, value]` pairs (inverse of `entries`) |
| `has`          | `has(obj, key) -> Bool`         | True if `key` is present (O(1)); a present `null` counts  |
| `merge`        | `merge(a, b) -> Object`         | A fresh object of `a`'s entries overlaid by `b`'s         |
| `map`          | `map(obj, func) -> Object`      | A fresh object, each value replaced by `func(value, key, obj)` |
| `filter`       | `filter(obj, pred) -> Object`   | A fresh object of the entries for which `pred` holds      |

`merge` / `map` / `filter` return fresh objects — inputs are never
mutated. As of v0.9, `has` is O(1) (backed by native `_NativeObject`)
and tells a missing key from a present `null` value, which `obj[key]`
cannot; `keys` / `values` / `entries` append in place (O(n) total)
rather than copying the accumulator each step.

As of v0.9, `has` is O(1) (backed by native `_NativeObject`) and tells
a missing key from a present `null` value, which `obj[key]` cannot.
`keys` / `values` / `entries` append in place (O(n) total) rather than
copying the accumulator each step.

#### `Map` (v0.9)

> Navigable reference: [`docs/stdlib/map.md`](docs/stdlib/map.md).

An arbitrary-keyed, insertion-ordered dictionary. Unlike `Object`
(string keys only), a `Map` key may be any **null / bool / int /
string** value; a `Float` or collection key raises `invalid_key_type`.
It is a distinct runtime type — `type(m)` is `"map"` — backed by the
native `_NativeMap` module.

`m[key]` reads an entry (`null` when absent) and `m[key] = value`
writes one. `#m` is the entry count; `for (k, v, m) { ... }` iterates
entries in insertion order.

| Entry     | Signature                       | Behavior                                                       |
|-----------|---------------------------------|----------------------------------------------------------------|
| `new`     | `new(source?) -> Map`           | An empty map, or one copied from an Object or `[key, value]` pairs |
| `get`     | `get(m, key) -> value`          | The value for `key`, or `null` if absent                       |
| `set`     | `set(m, key, value) -> Map`     | Insert or update in place; returns `m`                         |
| `has`     | `has(m, key) -> Bool`           | True if `key` is present (O(1)); a present `null` counts       |
| `delete`  | `delete(m, key) -> Bool`        | Remove `key`; true if it was present                           |
| `keys`    | `keys(m) -> Array`              | The keys, in insertion order                                   |
| `values`  | `values(m) -> Array`            | The values, in insertion order                                 |
| `entries` | `entries(m) -> Array`           | `[key, value]` pairs, in insertion order                       |
| `size`    | `size(m) -> Int`                | The entry count (same as `#m`)                                 |
| `clear`   | `clear(m) -> Map`               | Empty the map in place; returns `m`                            |

A `Map` is not JSON-serializable (`JSON.stringify` raises).

#### `Set` (v0.9)

> Navigable reference: [`docs/stdlib/set.md`](docs/stdlib/set.md).

An insertion-ordered collection of unique values. Elements share
`Map`'s key restriction (null / bool / int / string). `type(s)` is
`"set"`; backed by the native `_NativeSet` module.

`s[x]` tests membership (`true` / `false`); `s[x] = ...` is an error
(`immutable_target`) — mutate with `add` / `delete`. `#s` is the
element count; `for (x, s) { ... }` iterates in insertion order.

| Entry          | Signature                    | Behavior                                                  |
|----------------|------------------------------|-----------------------------------------------------------|
| `new`          | `new(array?) -> Set`         | An empty set, or one built from an array (duplicates collapsed) |
| `add`          | `add(s, x) -> Set`           | Insert `x` in place; returns `s`                          |
| `has`          | `has(s, x) -> Bool`          | True if `x` is a member (same as `s[x]`)                  |
| `delete`       | `delete(s, x) -> Bool`       | Remove `x`; true if it was present                        |
| `items`        | `items(s) -> Array`          | The elements, in insertion order                          |
| `size`         | `size(s) -> Int`             | The element count (same as `#s`)                          |
| `clear`        | `clear(s) -> Set`            | Empty the set in place; returns `s`                       |
| `union`        | `union(a, b) -> Set`         | A fresh set of the elements in either                     |
| `intersection` | `intersection(a, b) -> Set`  | A fresh set of the elements in both                       |
| `difference`   | `difference(a, b) -> Set`    | A fresh set of `a`'s elements not in `b`                  |

`union` / `intersection` / `difference` leave their inputs untouched.
Like `Map`, a `Set` is not JSON-serializable.

#### `String`

> Navigable reference: [`docs/stdlib/string.md`](docs/stdlib/string.md).

| Entry           | Signature                          | Behavior                                                  |
|-----------------|------------------------------------|-----------------------------------------------------------|
| `split`         | `split(s, sep) -> Array`           | Split on each literal `sep`                               |
| `join`          | `join(parts, sep) -> String`       | Concatenate `parts` separated by `sep`                    |
| `replace`       | `replace(s, from, to) -> String`   | Replace every `from` with `to`                            |
| `replace_first` | `replace_first(s, from, to) -> String` | Replace only the first `from`                         |
| `contains`      | `contains(s, needle) -> Bool`      | True if `needle` occurs in `s`                            |
| `index_of`      | `index_of(s, needle) -> Int`       | Byte offset of the first `needle`, or `-1`                |
| `lower`         | `lower(s) -> String`               | Lower-cased                                               |
| `upper`         | `upper(s) -> String`               | Upper-cased                                               |
| `starts_with`   | `starts_with(s, prefix) -> Bool`   | True if `s` begins with `prefix`                          |
| `ends_with`     | `ends_with(s, suffix) -> Bool`     | True if `s` ends with `suffix`                            |
| `trim`          | `trim(s) -> String`                | Whitespace removed from both ends                         |
| `trim_start`    | `trim_start(s) -> String`          | Leading whitespace removed                                |
| `trim_end`      | `trim_end(s) -> String`            | Trailing whitespace removed                               |
| `repeat`        | `repeat(s, n) -> String`           | `s` concatenated `n` times                                |
| `chars`         | `chars(s) -> Array`                | The characters as single-char strings                     |
| `pad_start`     | `pad_start(s, len, ch) -> String`  | Left-pad with `ch` to width `len`                         |
| `pad_end`       | `pad_end(s, len, ch) -> String`    | Right-pad with `ch` to width `len`                        |
| `words`         | `words(s) -> Array`                | Split on whitespace runs, dropping empties (v0.13)        |
| `lines`         | `lines(s) -> Array`                | Split on `\n` / `\r\n` (v0.13)                            |
| `split_any`     | `split_any(s, delims) -> Array`    | Split on any character in `delims` (v0.13)                |
| `find_all`      | `find_all(s, needle) -> Array`     | Byte offsets of every non-overlapping match (v0.13)       |
| `count`         | `count(s, needle) -> Int`          | Number of non-overlapping matches (v0.13)                 |
| `reverse`       | `reverse(s) -> String`             | Characters in reverse order (v0.13)                       |
| `strip_prefix`  | `strip_prefix(s, prefix) -> String`| `s` with `prefix` removed if present (v0.13)              |
| `strip_suffix`  | `strip_suffix(s, suffix) -> String`| `s` with `suffix` removed if present (v0.13)              |
| `capitalize`    | `capitalize(s) -> String`          | First character upper-cased (v0.13)                       |
| `is_blank`      | `is_blank(s) -> Bool`              | True if empty or all-whitespace (v0.13)                   |
| `matches_glob`  | `matches_glob(s, pattern) -> Bool` | Whole-string shell-style glob match (v0.13)               |
| `format`        | `format(value, spec) -> String`    | Render `value` through the spec mini-language (v0.9)      |
| `printf`        | `printf(template, args?) -> String`| Fill `%(spec)` placeholders in `template` (v0.9)          |

Like `index_of`, the offsets `find_all` returns are byte offsets.
`matches_glob(s, pattern)` is a whole-string shell-style match — `*`
(any run), `?` (one char), `[abc]`/`[a-z]` classes, `[!...]` negation,
`\` to escape a metacharacter — a small slice of pattern matching, not
a full regular expression language; a malformed pattern raises.

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
`{}` in every string literal.)

#### `Math`

> Navigable reference: [`docs/stdlib/math.md`](docs/stdlib/math.md).

| Entry   | Signature                  | Behavior                                       |
|---------|----------------------------|------------------------------------------------|
| `PI`    | `PI` *(Float)*             | The constant π                                 |
| `E`     | `E` *(Float)*              | The constant e                                 |
| `sqrt`  | `sqrt(x) -> Float`         | Square root                                    |
| `log`   | `log(x) -> Float`          | Natural logarithm                              |
| `log2`  | `log2(x) -> Float`         | Base-2 logarithm                               |
| `log10` | `log10(x) -> Float`        | Base-10 logarithm                              |
| `exp`   | `exp(x) -> Float`          | `e` raised to `x`                              |
| `sin`   | `sin(x) -> Float`          | Sine of `x` (radians)                          |
| `cos`   | `cos(x) -> Float`          | Cosine of `x` (radians)                        |
| `tan`   | `tan(x) -> Float`          | Tangent of `x` (radians)                       |
| `pow`   | `pow(x, y) -> Float`       | `x` raised to `y` (same result as `^^`)        |
| `abs`   | `abs(x) -> Number`         | Absolute value                                 |
| `sign`  | `sign(x) -> Int`           | `-1`, `0`, or `1`                              |
| `min`   | `min(a, b) -> value`       | The smaller of `a` and `b`                     |
| `max`   | `max(a, b) -> value`       | The larger of `a` and `b`                      |
| `clamp` | `clamp(x, lo, hi) -> value`| `x` confined to `[lo, hi]`                     |

The trig/log/exp functions are backed by the native `_NativeMath`
module (also importable directly). Source `Math.tg` re-exports them
alongside pure-tigr helpers — this gives users a single point to
shadow / extend without touching the interpreter.

#### `Test` (v0.9)

> Navigable reference: [`docs/stdlib/test.md`](docs/stdlib/test.md).

A small test framework, itself written in tigr.

| Entry           | Signature                                     | Behavior                                                  |
|-----------------|-----------------------------------------------|-----------------------------------------------------------|
| `assert`        | `assert(cond, msg?) -> Bool`                  | Raise unless `cond` is truthy                             |
| `assert_eq`     | `assert_eq(actual, expected, msg?) -> Bool`   | Raise unless `actual == expected`                         |
| `assert_ne`     | `assert_ne(a, b, msg?) -> Bool`               | Raise unless `a != b`                                     |
| `assert_raises` | `assert_raises(thunk, kind?) -> value`        | Raise unless `thunk` raised; returns the caught error     |
| `fail`          | `fail(msg?) -> value`                         | Raise unconditionally                                     |
| `case`          | `case(name, func) -> Object`                  | Package an unrun test as plain data                       |
| `suite`         | `suite(name, cases) -> Object`                | Run an array of cases; print results; return the tally   |

The assertions `raise` on failure, so they work standalone. `assert_eq`
uses `==`, which is structural for arrays and objects (§6.3).
`assert_raises` runs `thunk` and fails unless it raised; with a `kind`
argument the raised value must match — a reified built-in error's
`kind` field, or the raised value itself otherwise. `suite` prints a
`PASS`/`FAIL` line per case and a tally, then returns a result object
`${name, passed, failed, total, failures}` (`failures` being an array
of `${name, error}`).

The `tigr test [path]` CLI subcommand discovers test files —
`*_test.tg` anywhere, plus every `.tg` file under a `tests/`
directory — runs each, and sums the `passed`/`failed` fields of the
`suite` result(s) a file's final expression yields (a lone result
object, or an array of them). A file that raises an uncaught error
counts as a failure. The process exits non-zero if any test failed.

#### `Url` (v0.15)

> Navigable reference: [`docs/stdlib/url.md`](docs/stdlib/url.md).

URL parsing and the percent-codec, layered on `String`/`Bytes`.

| Entry          | Signature                       | Behavior                                                       |
|----------------|---------------------------------|----------------------------------------------------------------|
| `parse`        | `parse(url) -> Object`          | Split an absolute URL into `${scheme, host, port, path, query, fragment}`; a missing scheme raises |
| `build`        | `build(parts) -> String`        | The inverse of `parse`, so `build(parse(u))` round-trips       |
| `encode`       | `encode(s) -> String`           | RFC-3986 percent-encode, byte-wise over UTF-8                  |
| `decode`       | `decode(s) -> String`           | Percent-decode; a malformed `%`-escape raises a `decode` error |
| `parse_query`  | `parse_query(s) -> Object`      | Parse an `a=1&b=x%20y` query string into an Object             |
| `encode_query` | `encode_query(obj) -> String`   | Render an Object as a query string                             |

In `parse`'s result `port` is an `Int` or `null`, `path` defaults to
`'/'`, and `query`/`fragment` are the raw substrings or `null`.
`parse_query` decodes `+` to a space and keeps a duplicate key's last
value. See Appendix N.

#### `Http` (v0.15)

> Navigable reference: [`docs/stdlib/http.md`](docs/stdlib/http.md).

An HTTP/1.1 client and server helper over `Net`.

| Entry            | Signature                              | Behavior                                                  |
|------------------|----------------------------------------|-----------------------------------------------------------|
| `request`        | `request(opts) -> Object`              | Perform a request; returns `${status, status_text, headers, body}` |
| `get`            | `get(url, opts?) -> Object`            | `GET` request                                             |
| `post`           | `post(url, body?, opts?) -> Object`    | `POST` request                                            |
| `put`            | `put(url, body?, opts?) -> Object`     | `PUT` request                                             |
| `delete`         | `delete(url, opts?) -> Object`         | `DELETE` request                                          |
| `head`           | `head(url, opts?) -> Object`           | `HEAD` request                                            |
| `patch`          | `patch(url, body?, opts?) -> Object`   | `PATCH` request                                           |
| `text`           | `text(resp) -> String`                 | Decode a response body as UTF-8 text                      |
| `json`           | `json(resp) -> value`                  | Parse a response body as JSON                             |
| `read_request`   | `read_request(sock) -> Object`         | Server: read a request as `${method, path, query, headers, body}` |
| `write_response` | `write_response(sock, resp) -> Int`    | Server: write a response                                  |
| `serve`          | `serve(listener, handler) -> Null`     | Server: accept loop dispatching to per-connection actors  |

A response `body` is always `Bytes` (`text` / `json` decode it) and
`headers` keys are lowercased (a duplicate header collapses, last
wins). 3xx redirects are followed automatically. v1 has no keep-alive.
See Appendix N.

#### `WS`

> Navigable reference: [`docs/stdlib/ws.md`](docs/stdlib/ws.md).

A WebSocket (RFC 6455) client. On native targets it is pure tigr over
`Net`; in a browser the same API is backed by the host's `WebSocket`.
WebSockets are the one transport shared by every target, so a networked
game writes its messaging against `WS` once.

| Entry     | Signature                  | Behavior                                                   |
|-----------|----------------------------|------------------------------------------------------------|
| `connect` | `connect(url) -> handle`   | Open a `ws://` / `wss://` connection and run the handshake |
| `send`    | `send(ws, data) -> Null`   | Send a `String` as a text frame, `Bytes` as binary         |
| `poll`    | `poll(ws) -> value`        | Next inbound message, or `null` (never blocks)             |
| `drain`   | `drain(ws) -> Array`       | Every message buffered this tick (never blocks)            |
| `state`   | `state(ws) -> String`      | `'connecting'` \| `'open'` \| `'closed'`                   |
| `close`   | `close(ws) -> Null`        | Close the connection                                       |

The API is poll-based, so it drops into a frame loop with no callbacks.
A text message arrives as a `String`, a binary message as `Bytes`. The
client masks every frame, reassembles fragments, and auto-answers pings.
`wss://` runs over TLS. On native, `connect` returns already `open`; in
a browser it may be `'connecting'` first.

### 13.4 `JSON` (v0.4)

> Navigable reference: [`docs/stdlib/json.md`](docs/stdlib/json.md).

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
| string            | `String`                                   |
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
              | 'import' Expr
              | Try | Raise | Match
              | Spawn | Go | Yield | Select | Parallel

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

Spawn       ::= 'spawn' Expr
Go          ::= 'go' Expr
Yield       ::= 'yield' Expr?
Select      ::= 'select' '{' (SelectArm (',' SelectArm)* ','?)? '}'
SelectArm   ::= Identifier ':=' Expr '=>' Expr | 'else' '=>' Expr
Parallel    ::= 'parallel' '[' ']' '(' ForVars ',' Expr ')' Scope

Literal     ::= Integer | Float | String | 'true' | 'false' | 'null'
ArrayLit    ::= '[' (Element (',' Element)* ','?)? ']'
Element     ::= '...' Expr | Expr
ObjectLit   ::= '$' '{' (ObjMember (',' ObjMember)* ','?)? '}'
ObjMember   ::= '...' Expr | Identifier ':' Expr | String ':' Expr | Identifier   // shorthand
FunctionLit ::= 'gen'? 'fn' '(' Params? ')' '{' Block '}'   // 'gen' = generator
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
- `spawn`/`select`/`parallel[]` (actors) and `go`/`yield`/`gen fn`
  (green threads and generators) are the concurrency constructs; see
  Appendices L and P.

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

### 15.8 Optimization (v0.12)

Two optional passes, both semantically invisible — they change neither
the value a program produces nor the errors it raises:

- **Constant folding** — an AST→AST pass between parsing and
  compilation. A `BinOp`/`UnOp` whose operands are all literals is
  replaced by the literal it evaluates to (`2 + 3` → `5`); a
  fully-parenthesised literal expression collapses too. The folder must
  mirror the VM's arithmetic exactly, and — critically — must **decline
  to fold** any operation that would *raise* at runtime: integer
  overflow (§6.2b), divide-by-zero, an out-of-range shift. Leaving
  those unfolded keeps the catchable error and its source line intact.
- **Peephole — jump threading** — a pass over finished bytecode. A
  forward jump whose target is an unconditional jump is retargeted past
  it. Only operand bytes change; code does not move, so the line table
  needs no fixup.

Both are verifiable with the `tigr disasm` listing. Neither is required
for a conforming implementation.

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
    optional `if` guards, comma-separated arms. In v0.5 a `match` with
    no matching arm evaluated to `null`; v0.11's null-conflation
    cleanup changed this to raise a catchable `no_match` error (§9.7).
    Or-pattern alternatives may not bind variables.

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
    `map`/`filter`/`take`/`take_while`/`drop`/`drop_while`/`enumerate`/`zip`/`chain`, consumers
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

## Appendix K — Changes in v0.13

48. **`String` text helpers** (§13.3). Twelve targeted additions to the
    `String` module, all additive: `words`, `lines`, and `split_any`
    cover the splitting cases the literal-separator `split` cannot —
    runs of whitespace, line breaks, and a set of delimiter characters.
    `find_all` returns the byte offsets of every non-overlapping match
    of a substring and `count` returns how many there are. `replace_first`
    replaces a single match where `replace` replaces all. `reverse`,
    `strip_prefix`, `strip_suffix`, and `capitalize` are
    self-explanatory; `is_blank` reports whether a string is empty or
    all-whitespace. `matches_glob(s, pattern)` is a whole-string
    shell-style match — `*` matches any run of characters, `?` exactly
    one, `[abc]` / `[a-z]` a character class, `[!...]` a negated class,
    and `\` escapes a metacharacter. It is a deliberately small slice of
    pattern-as-data matching, not a regular-expression engine; a
    malformed pattern (an unterminated `[`, a dangling `\`) raises a
    catchable error. A full `Regex` module remains deferred — see the
    roadmap.

49. **`Bytes` type + binary IO** (§13.2). A new value type — a mutable,
    GC-managed byte buffer — alongside the `Bytes` module that builds
    and converts it, and three binary `IO` entries (`read_bytes`,
    `write_bytes`, `append_bytes`). `Bytes` is the binary counterpart to
    the UTF-8-only `String`: it is indexable (bytes read as `Int`
    0–255), `#`-measurable, `for`-iterable, spreadable, concatenable
    with `+`/`+=`, and content-compared with `==`. `String` ⇄ `Bytes`
    conversion is explicit and the decode direction (`to_string`) raises
    a catchable `decode` error on invalid UTF-8; `Bytes` ⇄ `[Int]`,
    hex, and base64 conversions round-trip. For binary-protocol work the
    module carries a named family of fixed-width integer readers and
    writers — `read_u32_be`, `write_i16_le`, and so on — so a call site
    states its width and endianness without a magic argument. An
    unsigned 64-bit read above the `Int` range raises a catchable
    `overflow`, consistent with v0.8. `Bytes` is the prerequisite for
    future networking and non-text-file work; streaming IO (file and
    socket handles) remains deferred.

50. **Range-keyed collection slicing** (§6.5, §7.3). Indexing an `Array`,
    `Bytes`, or `String` with a `Range` rather than an `Int` slices it,
    returning a fresh same-type sub-collection — `arr[1..3]`, `b[0..=4]`,
    `s[2..#s]`. No new syntax: `coll[range]` already parsed and compiled to
    the index opcode; this gives the `Range` key a meaning instead of an
    error. The slice copies (like `Array.slice` / `Bytes.slice`, which
    stay); negative endpoints count from the end and out-of-range endpoints
    clamp; the range's step and direction carry through, so a descending
    range yields a reversed slice. A `String` slice is character-indexed.
    Range-keyed *assignment* is out of scope — slicing is read-only.

51. **`BigInt` arbitrary-precision integer** (§13.2). A new value type —
    an immutable, arbitrary-precision integer — alongside the `BigInt`
    module that builds and operates on it. It is the complement to v0.8's
    "integer overflow raises a catchable error": where an `Int`
    computation past the 64-bit range raises `overflow`, a `BigInt`
    grows instead. A `BigInt` is created **explicitly** (`BigInt.new`) —
    an overflowing `Int` is never auto-promoted, since that would
    silently change a value's type. Once created it works with the
    ordinary operators (`+ - * / % ^^`, unary `-`, comparisons): an
    `Int` operand is promoted, a `Float` operand promotes the result to
    `Float`, and cross-type `==`/ordering against an `Int` works by
    value. Division is **exact-or-raise** — `/` yields a `BigInt` only
    when the result is exact, otherwise it raises a catchable
    `inexact_division` error, so a `BigInt` operator never silently
    decays to a lossy `Float`; `BigInt.divmod` / `BigInt.div` give
    integer division. The module also covers conversion (`to_int` raises
    `overflow` if the value will not fit an `Int`; `to_float`,
    `to_str_radix`), `pow`, `abs`, `sign`, `gcd`, and `lcm`. Bitwise
    operators stay `Int`-only; a `BigInt` is not a valid `Map`/`Set` key
    and is not JSON-serializable.

## Appendix L — Changes in v0.14

52. **Actors: `spawn` and `join`.** `spawn fn` runs a
    function as an *actor* — an OS thread with its own heap — and
    evaluates immediately to a `Task` handle. `join(t)`, a global
    built-in, blocks until the actor finishes and yields its result.
    `spawn` and `join` are a symmetric pair; neither needs an import.
    Actors share no
    mutable state: a spawned function is **deep-copied** across the
    heap boundary, so it may capture only *sendable* values
    (primitives, `String`, `Bytes`, `Range`, `BigInt`, the four
    collections, channels, tasks, and functions whose own captures are
    sendable). Capturing an iterator, a native function, or a function
    with a still-open capture raises a catchable `not_sendable`; a
    cyclic collection raises `cycle`. Because a spawned function is
    copied, it cannot see later mutations in the parent and `import`s
    its own modules. An actor's uncaught error surfaces at `join`,
    catchable like any error: a `raise`d value re-raises verbatim, a
    built-in error arrives as a `${kind, message, trace, worker}`
    object. The model is OS-thread actors rather than cooperative
    coroutines — it works with the per-thread v0.10 GC and needs no
    changes to it.

53. **Channels.** `import 'Channel'` is the conduit
    between actors — the one reference type that crosses threads.
    `Channel.new()` is unbounded; `Channel.new(n)` bounds the buffer at
    `n`, so `send` blocks (backpressure) while full. `send(ch, v)`
    deep-copies `v` into the channel; `recv(ch)` blocks and returns
    `${value: v}` for a message or `${closed: true}` once the channel
    is closed and drained; `try_recv(ch)` never blocks, adding
    `${empty: true}`. `close(ch)` wakes every blocked actor. A `send`
    on a closed channel raises the catchable `channel_closed`. Channels
    are bidirectional — any holder may both send and receive.

54. **`select`.** `select { name := ch => body, ... }`
    waits on several channels at once and runs the arm of the first to
    have a message, binding `name` to that value. A trailing
    `else => body` arm makes `select` non-blocking — it runs when no
    channel is ready. A closed channel is skipped; if every channel is
    closed `select` raises `channel_closed`. It is not a new core
    construct — `select` desugars to a `match`.

55. **`parallel[]`.** `parallel[] (v, iter) { body }`
    mirrors `for[]` but runs each iteration's body as its own actor,
    all concurrently, and collects the results into an array **in
    input order**. The body is deep-copied per actor (same sendability
    rule as `spawn`). The first body to raise aborts the block — the
    error propagates out — while sibling actors already started run to
    completion with their results discarded (there is no cancellation
    primitive in v0.14). It is the structured, common-case form built
    on `spawn` + `join`; reach for raw `spawn`/`Channel`/`select` when
    the work is not a simple fan-out.

## Appendix M — Changes in v0.15

56. **Networking: the `Net` module** (§13.2). `import 'Net'` opens
    network sockets — a TCP listener and TCP streams, UDP datagram
    sockets, and TLS-encrypted client connections. A socket is a
    first-class **sendable** `Value` (`type` `'socket'`), `Arc`-backed
    with identity equality like a channel, so an `accept`ed connection
    can cross into a `spawn`ed per-connection handler actor. Reads come
    in two layers: low-level `read(sock, n)` (an empty `Bytes` is
    end-of-stream) and the framed helpers `read_exact` / `read_line` /
    `read_until` / `read_all`, which share an internal per-socket
    buffer so a helper that over-reads keeps the surplus. An operation
    raises a catchable structured `${kind, message}` error — `kind` one
    of `timeout`, `closed`, `eof`, `refused`, `dns`, `tls`,
    `addr_in_use`, `decode`, or `io`. `set_timeout` bounds blocking
    reads/writes; `close` is idempotent and unblocks a reader stuck
    mid-`read` (and an actor stuck in `accept`, which then raises
    `closed`). `connect_tls` verifies the server certificate against
    the host OS trust store (plus an optional extra-CA PEM argument);
    `listen_tls` is the TLS *server* side — its `accept` yields
    encrypted server sockets, so `Http.serve(Net.listen_tls(...))` is
    an HTTPS server. `select` is *not* extended to sockets —
    bridge a socket to a channel with a reader actor to multiplex. The
    `Bytes` buffer (v0.13) is the enabler this was the prerequisite
    for.

## Appendix N — Changes in v0.15 (Http & Url)

57. **`Url` and `Http` source-stdlib modules** (§13.3). Two pure-tigr
    `.tg` modules layered on the native `Net`/`String`/`Bytes`/`JSON`
    primitives — no new core syntax.

    `import 'Url'` parses, builds, and codes URLs. `parse(url)` splits
    an absolute URL into `${scheme, host, port, path, query,
    fragment}` (`port` an `Int` or `null`, `path` defaulting to
    `'/'`, `query`/`fragment` the raw still-encoded substrings or
    `null`); a missing scheme raises. `build(parts)` inverts it —
    `build(parse(u))` round-trips. `encode`/`decode` are RFC-3986
    percent-coding applied byte-wise to the UTF-8 encoding, so
    non-ASCII text survives; the unreserved set `A-Za-z0-9-._~` passes
    through and a malformed `%`-escape raises a structured `decode`
    error. `encode_query(obj)` / `parse_query(str)` convert between an
    Object and an `a=1&b=x%20y` query string — `parse_query` turns
    `+` into a space and form-decodes both sides; on a duplicate key
    the last value wins.

    `import 'Http'` is an HTTP/1.1 client and server helper over
    `Net`. The client `request(opts)` — and the thin
    `get`/`post`/`put`/`delete`/`head`/`patch` wrappers — takes
    `opts = ${url, method, headers, body, max_redirects,
    follow_redirects, timeout}` (only `url` required; `body` a String
    or Bytes) and returns `${status, status_text, headers, body}`.
    `body` is always `Bytes` so binary responses are exact —
    `text(resp)` / `json(resp)` decode it. `headers` is an Object with
    **lowercased keys**; a repeated header collapses, last value wins
    (a documented v1 limitation). 3xx redirects are followed
    automatically (cap 10, opt out with `follow_redirects: false`):
    301/302/303 continue as GET with no body, 307/308 preserve the
    method and body; exceeding the cap raises `too_many_redirects`.
    The body is framed by `Transfer-Encoding: chunked`, then
    `Content-Length`, then — since v1 always sends `Connection:
    close` and so has no keep-alive — read to end-of-stream.

    The server side: `read_request(sock)` returns `${method, path,
    query, headers, body}` (the body read only when a `Content-Length`
    / `Transfer-Encoding` header is present, so a request never blocks
    on a missing EOF); `write_response(sock, ${status, headers,
    body})` writes a response, forcing `Content-Length` and
    `Connection: close`; `serve(listener, handler)` is an accept loop
    that hands each connection to its own `spawn`ed actor. Because a
    spawned closure is deep-copied across the actor boundary, the
    `handler` passed to `serve` **must be sendable** — it must
    `import` any modules it needs inside its own body, never capture
    them (the same rule as `spawn` everywhere). A handler returning a
    String becomes a `200 text/plain` response; an Object is sent
    as-is; a handler that raises yields a best-effort `500`. `serve`
    runs until its `listener` is closed — `close(listener)` from any
    actor stops the accept loop and `serve` returns cleanly, so a
    `serve` actor can be `join`ed after a deliberate shutdown.

## Appendix O — Changes in v0.17 (raw double-quoted strings)

58. **Non-interpolating double-quoted string literal** (§2.6, §8.2).
    `"…"` is a second string literal alongside `'…'`. It is fully
    raw: no `{expr}` interpolation and no backslash escapes — every
    character between the quotes is taken verbatim, so `{`, `}`, and
    `\` need no escaping. The only consequence is that a `"` cannot
    appear inside a `"…"` string; use `'…'` for text containing a
    double quote.

    Both forms produce the same `String` type with identical UTF-8
    semantics, operators, and indexing — they differ on exactly one
    axis, whether the lexer interpolates. The change is **lexer-only**:
    a new string-scanning branch emits the same plain `Str` constant,
    so the parser, compiler, and VM are unchanged. This resolves the
    long-standing §8.2 open question about a non-interpolating string
    form. Intended use: JSON and code templates, glob/format specs,
    and Windows-style paths — text that would otherwise need a `\{`
    on every brace.

## Appendix P — Green threads, generators, and IO offload

The v0.14 actor model (Appendix L) is one concurrency axis: real
parallelism across cores, one OS thread and one heap per actor, messages
deep-copied. This appendix adds the second axis: *green threads*,
lightweight coroutines that share one actor's heap and are scheduled
cooperatively, together with generator functions and a runtime that
keeps a blocking call from freezing an actor's coroutines. (Landed after
v0.17; not separately version-numbered in the roadmap.)

59. **Green threads: `go` and `yield`.** `go fn` runs a
    function as a *coroutine* inside the current actor and evaluates
    immediately to a **green-thread handle**. Unlike `spawn`, which
    gives each actor one OS thread and one heap with deep-copied
    messages, a `go` coroutine shares the actor's heap, so it needs no copying and no
    channels, and it is scheduled cooperatively onto the actor's single
    OS thread. Scheduling has no preemption: a coroutine runs until it
    `yield`s or returns, then the scheduler hands control to the next
    ready one, round-robin. A bare `yield` with nothing else ready
    resumes at once. The actor's main program is itself coroutine zero,
    so the idiom `while (!done) { yield }` pumps the scheduler until a
    coroutine has finished its work; a coroutine that never yields
    starves the rest. The `join` built-in is extended from `Task`s to
    green-thread handles: `join(handle)` cooperatively parks the caller
    (its siblings keep running) until the coroutine returns, then
    evaluates to that return value, and may be called repeatedly. An
    uncaught `raise` in a `go` body does not abort the actor: the
    coroutine ends and its error is recorded on the handle, so a later
    `join` re-raises it; a body that wants the joiner to keep going can
    `catch` internally and return a tagged value instead. The `go_cancel`
    built-in requests cancellation of a `go` coroutine: `go_cancel(handle)`
    is non-blocking and idempotent, returning `true` if the coroutine
    was still live and is now marked or `false` if it had already
    finished. The mark takes effect the next time the coroutine resumes
    from a park (any park: `yield`, `wait`, `join`, a channel receive, a
    blocking IO call, or a host frame wait), where a catchable
    `cancelled` is raised at the park's call site and unwinds the body
    through the ordinary error path. There is no preemption: a body that
    never parks again runs to completion. An uncaught `cancelled` ends
    only that coroutine, recorded so a later `join` returns
    `${cancelled: true}` rather than re-raising, and never aborts the
    actor. The `go_alive(handle)` built-in is the non-destructive query
    that completes the trio: it reads the handle and returns `true` while
    the coroutine is live, `false` once it has finished or been cancelled
    (a pending `go_cancel` reads as not-alive synchronously), without
    blocking like `join` or mutating like `go_cancel`. Both `go_cancel`
    and `go_alive` are green-only (`join` stays unprefixed because it also
    waits on actor `Task`s). Two-level model: `spawn` is parallelism
    across cores with separate heaps and copied messages; `go` is cheap
    concurrency on one core with a shared heap and cooperative hand-off.

60. **Intra-actor channels: `LocalChannel`.**
    `import 'LocalChannel'` is a channel *between green threads* of one
    actor. Because every coroutine shares the actor's heap a message
    moves directly, with no deep copy and no transfer-encoding (contrast the
    cross-actor `Channel` of Appendix L, which copies). `new()` takes no
    capacity: the channel is unbounded and `send` never blocks; `recv`
    on an empty channel `yield`s the coroutine until a value or a close
    arrives. `recv` / `try_recv` return `${value: v}`, `${closed: true}`
    once the channel is closed and drained, or (`try_recv` only)
    `${empty: true}`; `send` on a closed channel raises the catchable
    `channel_closed`. `type` is `'local_channel'`; a `LocalChannel` is
    neither sendable across actors nor JSON-serializable.

61. **Generators: `gen fn`.** `gen fn (params) { body }`
    is a generator-function literal. *Calling* it does not run the body;
    it builds a paused coroutine and returns an iterator object
    `${next: fn()}`. Each `next()` runs the body to the next `yield`,
    which produces a value as `${done: false, value}`; once the body
    returns, `next()` reports `${done: true}` from then on. Because a
    generator speaks the ordinary iterator protocol it drives a `for`
    loop, the spread forms `[...g]` and `f(...g)`, and the whole `Iter`
    module directly. `Iter` is itself built from `gen fn`
    generators, so a generator you write drops straight into an `Iter`
    pipeline. A `gen fn` with `while true` is the natural way to write
    an infinite or streaming sequence: each value is computed only when
    pulled. A `raise` escaping a generator body surfaces at the `next()`
    call site, catchable with an ordinary `try` around the pull. `yield`
    thus serves both coroutine forms: in a `go` body it hands control to
    the scheduler; in a `gen fn` body it produces the next value.

62. **Blocking-call offload: worker pool and async-I/O reactor.**
    A blocking call (a child process, or file or socket
    I/O) made while other coroutines are live no longer freezes the
    actor. The call is moved off the actor thread, the calling coroutine
    cooperatively parks, and its siblings keep running until the result
    is ready; the call then resumes the coroutine exactly as a normal
    return (or `raise`). With nothing else to schedule the call simply
    runs inline on the actor thread, so a program that uses no `go` is
    unaffected. Two backends share the work. A *worker pool* handles
    short blocking work: `Os.run` / `Os.cwd`, the waiting `IO` file and
    directory calls, the calls that may need a blocking name lookup
    (`connect`, `connect_tls`, `send_to`), and the cross-actor waits
    `Channel.send` / `Channel.recv` / `select` / `join` on a `Task`.
    Steady-state socket I/O runs instead on a single *async-I/O reactor*
    thread built on the operating system's `epoll` or `kqueue`:
    `accept`, `read`, `write`, `read_exact`, `read_line`, `read_until`,
    `read_all`, and `recv_from`. The difference shows at scale: a
    coroutine parked in `read` on the reactor costs one table entry, so
    one actor can hold tens of thousands of idle connections open at
    once. Fast non-waiting calls (`IO.exists` / `is_dir` / `is_file` /
    `stat`, `Net.listen` / `bind` / `local_addr` / `peer_addr` /
    `set_timeout` / `close`, `Channel.try_recv` / `close`) stay inline.
    One consequence of cooperative parking: a green thread may
    `Channel.recv` from a sibling green thread in the same actor without
    deadlocking, because the receive parks the coroutine rather than
    sleeping the shared OS thread.

63. **Deferred values: `Deferred`.** A `Deferred` is a first-class,
    write-once result a coroutine can wait on and anything can complete.
    `Deferred.new()` mints one; `join(d)` waits on it (the same `join`
    that waits on a `Task` and a green-thread handle, now extended to a
    deferred); `Deferred.resolve(d, v)` settles it with a value and
    `Deferred.reject(d, e)` settles it with an error. It generalises
    `join`: where `join` waits on a *coroutine's* return, a deferred waits
    on a value *anyone* can supply, so barriers, fan-in, one-shot
    signalling, and first-to-complete are writable in pure tigr with no
    host. It is a latch: `result` is recorded once, so a `join` after a
    settle returns (or re-raises) immediately, and a value resolved before
    anyone waits is still delivered. `resolve`/`reject` broadcast — every
    coroutine parked in `join(d)` wakes, all with the same value.
    `reject` re-raises its value verbatim at each awaiter's `join` site,
    the same `Raised` path a `go` body's uncaught error takes to its
    joiner, so it composes with `try`/`catch`. Settling is once:
    `resolve`/`reject` return `true` if they settled the deferred and
    `false` if it was already settled (mirroring `go_cancel`'s bool), so a
    first-to-complete race needs no guarding. A `join(d)` is a
    cancellation point like any other park, so `go_cancel` reaches a
    deferred awaiter. A deferred that nothing ever resolves leaves its
    awaiters parked, the same accepted tradeoff as a channel receive with
    no sender; a standalone program that would block on a deferred with no
    way to resolve it raises a catchable deadlock rather than hanging.
    `type` is `'deferred'`; a `Deferred` is neither sendable across actors
    nor JSON-serializable. Embedders complete one from outside the worker
    pool through `Vm::resolve_deferred` / `reject_deferred` (and the
    `Session` wrappers), the async-completion seam for a host value that
    arrives later (a GPU readback, an OS event, a dialog result): the host
    hands a coroutine a value from its own loop, not from a blocking
    worker.
