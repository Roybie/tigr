# Control flow

Spec: [LANGUAGE.md §9](../../LANGUAGE.md#9-control-flow)

Every control-flow construct in Tigr is an expression. `if`, `while`, `for`, `match`, and even `break` and `return` all produce a value, so they can sit anywhere an expression can: on the right of `:=`, inside an argument list, as the last line of a block.

## `if` / `else`

```
if cond { ... }
if cond { ... } else { ... }
if cond1 { ... } else if cond2 { ... } else { ... }
```

An `if` evaluates to the value of whichever branch runs. If no branch matches and there is no `else`, it evaluates to `null`.

```tigr
score := 85;
label := if score > 90 { 'A' } else if score > 80 { 'B' } else { 'C' };
print(label);   // => B
```

## `while` and `while[]`

A plain `while` loops while its condition holds and evaluates to the value of the last iteration's body, or `null` if the body never ran.

```tigr
i := 0;
last := while i < 5 { i = i + 1; i * 10 };
print(last);    // => 50
```

The `while[]` form collects the body value of every iteration into an array.

```tigr
i := 0;
squares := while[] i < 4 { i = i + 1; i * i };
print(squares); // => [1, 4, 9, 16]
```

## `for` and `for[]`

A `for` loop iterates a Range, Array, Object, Map, Set, Bytes, String, or iterator object. Each iterable has a one-variable and a two-variable form:

| Iterable | One variable     | Two variables                              |
|----------|------------------|--------------------------------------------|
| Range    | `for (i, 0..10)` | `for (n, i, 0..10)` where `n` is 0, 1, 2, … |
| Array    | `for (x, arr)`   | `for (i, x, arr)`                           |
| Object   | `for (v, obj)`   | `for (k, v, obj)`                           |
| Map      | `for (v, map)`   | `for (k, v, map)`                           |
| Set      | `for (x, set)`   | `for (i, x, set)` where `i` is 0, 1, 2, …   |
| Bytes    | `for (b, buf)`   | `for (i, b, buf)` where `b` is a byte Int   |
| String   | `for (ch, str)`  | `for (i, ch, str)`                          |
| Iterator | `for (v, it)`    | `for (i, v, it)` where `i` is 0, 1, 2, …    |

Like `while`, a plain `for` evaluates to the last body value, and `for[]` collects every body value into an array.

```tigr
last := for (x, [10, 20, 30]) { x };
print(last);                        // => 30

all := for[] (i, 1..=5) { i * i };
print(all);                         // => [1, 4, 9, 16, 25]
```

A `for[]` or `while[]` collects every body value verbatim, `null` included. The only way to omit an item is `continue`.

An iterator object is any object with a callable `next` field, the shape produced by the [`Iter`](../stdlib/iter.md) module. A `for` can consume an `Iter` pipeline directly, with no `Iter.collect()` in between. The same applies to spread: `[...it]` and `f(...it)` expand an iterator. An object without a callable `next` iterates as key/value entries instead.

Each iteration opens a fresh scope for the loop variables, so a closure created inside the loop captures that iteration's value independently:

```tigr
adders := for[] (i, 0..3) { fn(x) { x + i } };
print(adders[0](10));   // => 10
print(adders[1](10));   // => 11
print(adders[2](10));   // => 12
```

## `break`

`break` exits the innermost loop. On its own it produces `null`; with a value, it produces that value. An expression after `break` needs parentheses.

```
break              // null
break 5            // 5
break (x + y)      // expression form needs parens
```

In a `for[]` or `while[]`, `break <value>` appends the value to the result array, even if that value is `null`, while a bare `break` appends nothing.

`break` is itself an expression, so you can pass one `break` to another to bail out of nested loops:

```tigr
found := for (i, 0..10) {
    for (j, 0..10) {
        if i * j == 25 {
            break (break [i, j])
        }
    }
};
print(found);   // => [5, 5]
```

## `continue`

`continue` skips the rest of the current iteration and moves to the next. In a `for[]` or `while[]`, the skipped iteration adds nothing to the result array, which makes `continue` the way to filter while building. In a plain `for` or `while`, the skipped iteration's value becomes `null`. Unlike `break`, `continue` carries no value, and using it outside a loop is a compile-time error.

```tigr
evens := for[] (n, 0..10) {
    if n % 2 != 0 { continue };
    n
};
print(evens);   // => [0, 2, 4, 6, 8]
```

## `return`

`return` exits the innermost function. Like `break`, it is an expression and can be chained.

```tigr
find := fn(arr, target) {
    for (i, 0..#arr) {
        if arr[i] == target { return i }
    };
    null
};
print(find([7, 8, 9], 8));   // => 1
```

## `match`

`match` evaluates its subject once, then tries each comma-separated arm from top to bottom. It yields the body of the first arm whose pattern matches, taking an optional `if` guard into account. If no arm matches it raises a catchable `no_match` error, so a `_` wildcard arm at the end makes the `match` total. It is an expression.

```tigr
score := 75;
grade := match score {
    90..=100 => 'A',
    80..=89  => 'B',
    70..=79  => 'C',
    _        => 'F',
};
print(grade);   // => C
```

Match patterns are refutable: a pattern can fail and fall through to the next arm. That is the opposite of the destructuring patterns covered in [Destructuring](destructuring.md), which must match. The pattern kinds are:

- **Literal**: `0`, `'hi'`, `true`, `null`, `-1`. Matches if the subject `==` the literal.
- **Binding**: a bare name. Matches anything and binds it for the arm.
- **Wildcard**: `_`. Matches anything and binds nothing.
- **Range**: `0..10` or `0..=9`. Matches a number in range; a non-number fails.
- **Array**: `[a, b]` for an exact length, or `[head, ...rest]` for a length of at least one.
- **Object**: `${kind: 'circle', r}`. Sub-pattern fields must match, shorthand fields bind, and a missing key binds `null`.
- **Or-pattern**: `1 | 2 | 3`. Matches any alternative. Alternatives may not bind variables.

```tigr
area := fn(shape) {
    match shape {
        ${kind: 'circle', r}  => 3.14159 * r ^^ 2,
        ${kind: 'rect', w, h} => w * h,
        _                     => raise 'unknown shape',
    }
};
print(area(${kind: 'rect', w: 3, h: 4}));   // => 12

sum := fn(xs) {
    match xs {
        []            => 0,
        [head, ...tl] => head + sum(tl),
    }
};
print(sum([1, 2, 3, 4]));   // => 10
```

A guard is an `if` expression after the pattern. It can see the names the pattern binds:

```tigr
n := -3;
classify := match n {
    x if x < 0 => 'negative',
    0          => 'zero',
    _          => 'positive',
};
print(classify);   // => negative
```

Each arm body runs in its own scope, and the pattern bindings and guard see those names.

## See also

- [Errors](errors.md): `try`, `catch`, and `raise`, the recoverable-error constructs
- [Destructuring](destructuring.md): the irrefutable patterns used in bindings
- [Functions](functions.md): how `return` interacts with calls and tail calls
- [LANGUAGE.md §9](../../LANGUAGE.md#9-control-flow): the authoritative spec
