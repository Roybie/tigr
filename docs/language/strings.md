# Strings

Spec: [LANGUAGE.md §8](../../LANGUAGE.md#8-strings)

A `String` is an immutable sequence of Unicode characters. Tigr has two string literals. They produce the same `String` type and differ on exactly one axis: whether the lexer interpolates.

## Single-quoted: interpolated

A single-quoted `'…'` string supports `{expr}` interpolation. Each `{expr}` is replaced by the result of running `str` on a single Tigr expression. Backslash escapes (`\n`, `\t`, `\r`, `\\`, `\'`, `\{`) are processed; use `\{` for a literal brace.

```tigr
name := 'tigr';
print('hello, {name}!');     // => hello, tigr!
print('sum: {2 + 3}');       // => sum: 5

arr := [10, 20, 30];
print('first: {arr[0]}, count: {#arr}');   // => first: 10, count: 3

print('a literal \{ brace');   // => a literal { brace
```

Interpolations can hold any expression, including another string, so they nest:

```tigr
ok := true;
print('{ if ok { 'yes' } else { 'no' } }');   // => yes
```

## Double-quoted: raw

A double-quoted `"…"` string is fully raw. There is no interpolation, and backslash is a literal character with no escapes at all. Everything between the quotes is taken verbatim. This is the form for text that genuinely contains braces or backslashes: JSON or code templates, glob and format specs, Windows paths.

```tigr
name := 'tigr';
print("hello {name} world");   // => hello {name} world
print("*.{rs,tg}");            // => *.{rs,tg}
print("C:\Users\me");          // => C:\Users\me
```

Because there are no escapes, a `"` cannot appear inside a `"…"` string. When the text needs a double quote, use `'…'` with `\'` or interpolation instead. Both forms share every operator and the same UTF-8 character semantics, and they compare equal when they hold the same characters:

```tigr
print("ab" == 'ab');   // => true
```

## String operators

```tigr
print('abc' + 'def');   // => abcdef    concatenation
print(#'hello');        // => 5         character count
print('hello'[1]);      // => e         index returns a one-character string
print('hello'[1..4]);   // => ell       Range index slices by character
```

`+` between a string and a non-string is an error; reach for interpolation when you need to splice a non-string into text. An out-of-range index returns `null`. A `Range` index slices the string by character and is O(n), the same cost as a single character index.

Strings are immutable: there is no in-place mutation, and every operation that "changes" a string returns a fresh one.

## The String module

The [`String`](../stdlib/string.md) module adds the bulk of the text toolkit: `split`, `join`, `trim`, `replace`, `chars`, `pad_start`, and many more.

Two pieces are worth flagging here. `String.format` and `String.printf` share a format-spec mini-language for padding, alignment, sign, numeric base, and precision. `String.matches_glob` tests a string against a shell-style glob pattern.

```tigr
print(String.format(42, '08'));   // => 00000042
print(String.matches_glob('foo.tg', '*.tg'));   // => true
```

See the [`String` module reference](../stdlib/string.md) for the full API and the format-spec grammar.

## See also

- [Expressions](expressions.md): indexing, slicing, and the `+` operator in general
- [Overview](overview.md): where `String` sits among the value types
- [`String` module](../stdlib/string.md): the standard-library text functions and format mini-language
- [LANGUAGE.md §8](../../LANGUAGE.md#8-strings): the authoritative spec
