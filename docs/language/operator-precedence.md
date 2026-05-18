# Operator precedence

Spec: [LANGUAGE.md §6](../../LANGUAGE.md#6-expressions)

This table lists every operator from lowest binding strength to highest, with associativity. A higher level binds tighter, so it groups before a lower one.

| Level | Operators                                       | Assoc |
|-------|-------------------------------------------------|-------|
| 1     | `=` `:=` `+=` `-=` `*=` `/=` `%=`               | right |
| 2     | `\|\|`                                          | left  |
| 3     | `&&`                                            | left  |
| 4     | `==` `!=` `<` `>` `<=` `>=`                      | left  |
| 5     | `\|`  (bitwise OR)                              | left  |
| 6     | `^`   (bitwise XOR)                             | left  |
| 7     | `&`   (bitwise AND)                             | left  |
| 8     | `\|>` (pipe)                                    | left  |
| 9     | `..` `..=`  (with optional `:step`)             | n/a   |
| 10    | `<<` `>>`                                       | left  |
| 11    | `+` `-`                                         | left  |
| 12    | `*` `/` `%`                                     | left  |
| 13    | `^^`  (exponentiation)                          | right |
| 14    | unary `-` `!` `#` `~`                           | n/a   |
| 15    | call `f(…)`, index `a[i]`, member `a.b`         | left  |

A few consequences worth keeping in mind:

- Assignment is the loosest operator and is right-associative, so `a := b := 0` declares both.
- `*` `/` `%` bind tighter than `+` `-`, the usual arithmetic rule, so `1 + 2 * 3` is `7`.
- The bitwise operators are Rust-style: `<<` and `>>` bind looser than `+` and `-`, and `& ^ |` bind looser than the comparison operators. That differs from C, so parenthesize bitwise expressions mixed with arithmetic or comparison.
- Exponentiation `^^` is right-associative, so `2 ^^ 3 ^^ 2` is `2 ^^ (3 ^^ 2)`, which is `512.0`.
- Unary operators (level 14) bind tighter than `^^`, so `-2 ^^ 2` groups as `(-2) ^^ 2`, which is `4.0`.
- Call, index, and member access bind tightest, so `#arr[0]` takes the length of `arr[0]`, not of `arr`.

A short program that exercises the table:

```tigr
print(1 + 2 * 3);     // => 7        * before +
print(2 ^^ 3 ^^ 2);   // => 512.0    ^^ is right-associative
print(-2 ^^ 2);       // => 4.0      unary - binds tighter than ^^
print(#'abc' + 1);    // => 4        # binds tighter than +
```

## See also

- [Expressions](expressions.md): what each operator does
- [Overview](overview.md): the `&&` / `||` value-returning rule
- [LANGUAGE.md §6](../../LANGUAGE.md#6-expressions): the authoritative spec
