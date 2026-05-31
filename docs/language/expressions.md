# Expressions

Spec: [LANGUAGE.md §6](../../LANGUAGE.md#6-expressions)

Every operator in Tigr builds an expression, and expressions nest freely. This page covers arithmetic, comparison, the bitwise operators, indexing and member access, spread, and the pipe operator. For the full precedence table see [Operator precedence](operator-precedence.md).

## Arithmetic

The arithmetic operators are `+ - * / % ^^`. `^^` is exponentiation.

```tigr
print(6 / 2);     // => 3      divides evenly, stays Int
print(7 / 2);     // => 3.5    does not divide evenly, becomes Float
print(2 + 3.0);   // => 5.0    any Float operand makes the result Float
print(7 % 3);     // => 1
print(-7 % 3);    // => -1     % follows the sign of the dividend
print(2 ^^ 10);   // => 1024.0 ^^ always produces a Float
```

The rules in one place:

- `Int op Int` produces an `Int`, except division: `n / m` stays `Int` only when it divides evenly, otherwise it becomes `Float`.
- Any `Float` operand makes the whole result a `Float`.
- `^^` (power) always produces a `Float`.
- `%` takes the sign of the dividend.

`Int` arithmetic for `+`, `-`, `*`, and unary `-` is checked. A result outside the signed 64-bit range raises a catchable `overflow` error rather than wrapping silently. `Float` arithmetic is unchecked IEEE-754 and may produce `inf`.

## Comparison and equality

The comparison operators are `== != < > <= >=`.

`==` and `!=` work between any two values. Values of different types are unequal, with the single exception that `Int` and `Float` compare numerically.

```tigr
print(1 == 1.0);          // => true
print([1, 2] == [1, 2]);  // => true   arrays compare element-wise
```

Arrays and objects compare structurally. Functions compare by identity. `null == null` is `true`, and `null == 0` is `false`.

## Bitwise operators

`& | ^ << >>` (binary) and `~` (unary) operate on `Int` only. Any other operand type raises a catchable error. `^` is bitwise XOR; exponentiation is the separate `^^` operator. `>>` is an arithmetic, sign-preserving shift, and a shift amount outside `0..64` raises rather than wrapping.

```tigr
print(0b1100 & 0b1010);   // => 8
print(0b1100 | 0b1010);   // => 14
print(0b1100 ^ 0b1010);   // => 6     bitwise XOR
print(~0);                // => -1
print(1 << 8);            // => 256
print(-16 >> 2);          // => -4    arithmetic shift keeps the sign
```

Precedence is Rust-style: `<<` and `>>` bind looser than `+` and `-`, and `& ^ |` bind looser than the comparison operators. Parenthesize when in doubt.

## Indexing and member access

Index a collection with `[]` and read object fields with `.`:

```
arr[0]
arr[i + 1]
obj['key']
obj.key          // sugar for obj['key']
```

An out-of-range numeric index returns `null`, and a missing object key returns `null`. Negative array indices count from the end, so `arr[-1]` is the last element. `obj.key` is exactly `obj['key']` and may sit on the left of any assignment operator.

Indexing an `Array`, `Bytes`, or `String` with a `Range` instead of an `Int` slices it. The result is a fresh sub-collection of the same type. Out-of-range endpoints clamp, and the range's direction carries through, so a descending range reverses. See [Collections](collections.md) for worked slice examples.

## Spread

The spread operator `...` unpacks an iterable into the context around it:

```
[1, ...other, 5]            // into an array literal
${...defaults, color: 'r'}  // into an object literal (later keys win)
f(x, ...args, y)            // into a function call
```

Array-literal and call spread accept an `Array`, `Range`, `String`, or iterator object. Object-literal spread requires an `Object`. (On the left of a binding, `...` is the rest pattern instead; see [Destructuring](destructuring.md).)

## Pipe `|>`

`x |> rhs` evaluates `x`, then feeds it to `rhs`:

- If `rhs` is a call `f(args…)`, it becomes `f(x, args…)`. The piped value goes in as the first argument.
- Otherwise `rhs` is evaluated, must produce a function, and is called with `x` as its only argument.

```tigr
double := fn(x) { x * 2 };

print(5 |> double);     // => 10   no call on the right, so double(5)
print(5 |> double());   // => 10   same thing, written as a call

arr := [1, 2, 3];
print(arr |> Array.map(double) |> Array.reverse());
// => [6, 4, 2]   equivalent to Array.reverse(Array.map(arr, double))
```

Pipe is left-associative and evaluation runs strictly left to right, so a chain reads top to bottom in source order.

## See also

- [Operator precedence](operator-precedence.md): the full precedence and associativity table
- [Overview](overview.md): truthiness and the `&&` / `||` value rule
- [Collections](collections.md): indexing, slicing, and spread on arrays, objects, and ranges
- [Strings](strings.md): string concatenation and character indexing
- [LANGUAGE.md §6](../../LANGUAGE.md#6-expressions): the authoritative spec
