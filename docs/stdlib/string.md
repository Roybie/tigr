# `String`

> Pure-tigr source module, `stdlib/String.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#string)

The `String` module is the text-manipulation toolkit. Most entries are thin re-exports of native primitives; a few (`join`, `pad_start`, `pad_end`, `is_blank`, `printf`) are pure tigr. It is ambient, so a bare module name works without an `import`.

Indices and lengths are byte offsets, consistent with `#s` counting bytes. A `String` is iterable with `for (ch, s) { ... }`, and `s[i]` indexes it. Every function raises on a non-`String` argument.

```tigr
print(String.join(String.split('a,b,c', ','), '-'));   // => a-b-c
```

## Functions

| Function | Summary |
|----------|---------|
| [`split(s, sep) -> Array`](#splits-sep---array) | Splits `s` into pieces on every occurrence of `sep`. |
| [`join(parts, sep) -> String`](#joinparts-sep---string) | Joins an array of values into one string, calling `str` on each one, placing `sep` between them. |
| [`replace(s, from, to) -> String`](#replaces-from-to---string) | Replaces every occurrence of `from` with `to`. |
| [`replace_first(s, from, to) -> String`](#replace_firsts-from-to---string) | Replaces only the first occurrence of `from` with `to`. |
| [`contains(s, needle) -> Bool`](#containss-needle---bool) | Tests whether `s` contains `needle`. |
| [`index_of(s, needle) -> Int`](#index_ofs-needle---int) | Finds the byte index of the first `needle`. |
| [`lower(s) -> String`](#lowers---string) | Converts `s` to lowercase. |
| [`upper(s) -> String`](#uppers---string) | Converts `s` to uppercase. |
| [`starts_with(s, prefix) -> Bool`](#starts_withs-prefix---bool) | Tests whether `s` begins with `prefix`. |
| [`ends_with(s, suffix) -> Bool`](#ends_withs-suffix---bool) | Tests whether `s` ends with `suffix`. |
| [`trim(s) -> String`](#trims---string) | Removes whitespace from both ends of `s`. |
| [`trim_start(s) -> String`](#trim_starts---string) | Removes leading whitespace from `s`. |
| [`trim_end(s) -> String`](#trim_ends---string) | Removes trailing whitespace from `s`. |
| [`repeat(s, n) -> String`](#repeats-n---string) | Concatenates `n` copies of `s`. |
| [`chars(s) -> Array`](#charss---array) | Splits `s` into one-character strings, one per Unicode character. |
| [`reverse(s) -> String`](#reverses---string) | Reverses the characters of `s`. |
| [`capitalize(s) -> String`](#capitalizes---string) | Uppercases the first character of `s` and leaves the rest unchanged. |
| [`pad_start(s, len, ch) -> String`](#pad_starts-len-ch---string) | Pads `s` on the left with `ch` until it is at least `len` characters wide. |
| [`pad_end(s, len, ch) -> String`](#pad_ends-len-ch---string) | Pads `s` on the right with `ch` until it is at least `len` characters wide. |
| [`is_blank(s) -> Bool`](#is_blanks---bool) | Tests whether `s` is empty or contains only whitespace. |
| [`words(s) -> Array`](#wordss---array) | Splits `s` on runs of whitespace, dropping empty fields. |
| [`lines(s) -> Array`](#liness---array) | Splits `s` into lines on `\n` or `\r\n`. |
| [`split_any(s, delims) -> Array`](#split_anys-delims---array) | Splits `s` on any character that appears in `delims`. |
| [`find_all(s, needle) -> Array`](#find_alls-needle---array) | Finds the byte offset of every non-overlapping occurrence of `needle`. |
| [`count(s, needle) -> Int`](#counts-needle---int) | Counts the non-overlapping occurrences of `needle` in `s`. |
| [`strip_prefix(s, prefix) -> String`](#strip_prefixs-prefix---string) | Removes `prefix` from the start of `s` if it is there, otherwise returns `s` unchanged. |
| [`strip_suffix(s, suffix) -> String`](#strip_suffixs-suffix---string) | Removes `suffix` from the end of `s` if it is there, otherwise returns `s` unchanged. |
| [`matches_glob(s, pattern) -> Bool`](#matches_globs-pattern---bool) | Tests `s` against a shell-style glob pattern. |
| [`format(value, spec) -> String`](#formatvalue-spec---string) | Renders `value` through the format spec mini-language. |
| [`printf(template, args?) -> String`](#printftemplate-args---string) | Renders `template`, replacing each `%(SPEC)` placeholder with `format(next arg, SPEC)`. |


### `split(s, sep) -> Array`

Splits `s` into pieces on every occurrence of `sep`. An empty `sep` splits `s` into its characters.

- `s` *(String)*: the string to split.
- `sep` *(String)*: the separator.

**Returns:** an `Array` of `String` pieces.

```tigr
print(String.split('a,b,c', ','));   // => [a, b, c]
```

### `join(parts, sep) -> String`

Joins an array of values into one string, calling `str` on each one, placing `sep` between them.

- `parts` *(Array)*: the values to join.
- `sep` *(String)*: the separator.

**Returns:** the joined `String`, or `''` for an empty array.

```tigr
print(String.join(['a', 'b', 'c'], '-'));   // => a-b-c
```

### `replace(s, from, to) -> String`

Replaces every occurrence of `from` with `to`. An empty `from` returns `s` unchanged.

- `s` *(String)*: the string to search.
- `from` *(String)*: the substring to replace.
- `to` *(String)*: the replacement.

**Returns:** a new `String` with the replacements applied.

```tigr
print(String.replace('a-b-c', '-', '+'));   // => a+b+c
```

### `replace_first(s, from, to) -> String`

Replaces only the first occurrence of `from` with `to`. An empty `from` returns `s` unchanged.

- `s` *(String)*: the string to search.
- `from` *(String)*: the substring to replace.
- `to` *(String)*: the replacement.

**Returns:** a new `String` with the first match replaced.

```tigr
print(String.replace_first('a-b-c', '-', '+'));   // => a+b-c
```

### `contains(s, needle) -> Bool`

Tests whether `s` contains `needle`.

- `s` *(String)*: the string to search.
- `needle` *(String)*: the substring to look for.

**Returns:** `true` if `needle` is present, otherwise `false`.

```tigr
print(String.contains('hello world', 'world'));   // => true
```

### `index_of(s, needle) -> Int`

Finds the byte index of the first `needle`.

- `s` *(String)*: the string to search.
- `needle` *(String)*: the substring to look for.

**Returns:** the byte index of the first match, or `-1` if absent.

```tigr
print(String.index_of('hello', 'l'));   // => 2
```

### `lower(s) -> String`

Converts `s` to lowercase.

- `s` *(String)*: the string to convert.

**Returns:** the lowercased `String`.

```tigr
print(String.lower('Hello World'));   // => hello world
```

### `upper(s) -> String`

Converts `s` to uppercase.

- `s` *(String)*: the string to convert.

**Returns:** the uppercased `String`.

```tigr
print(String.upper('Hello World'));   // => HELLO WORLD
```

### `starts_with(s, prefix) -> Bool`

Tests whether `s` begins with `prefix`.

- `s` *(String)*: the string to test.
- `prefix` *(String)*: the prefix to look for.

**Returns:** `true` if `s` starts with `prefix`, otherwise `false`.

```tigr
print(String.starts_with('readme.txt', 'readme'));   // => true
```

### `ends_with(s, suffix) -> Bool`

Tests whether `s` ends with `suffix`.

- `s` *(String)*: the string to test.
- `suffix` *(String)*: the suffix to look for.

**Returns:** `true` if `s` ends with `suffix`, otherwise `false`.

```tigr
print(String.ends_with('readme.txt', '.txt'));   // => true
```

### `trim(s) -> String`

Removes whitespace from both ends of `s`.

- `s` *(String)*: the string to trim.

**Returns:** the trimmed `String`.

```tigr
print(String.trim('  hi  '));   // => hi
```

### `trim_start(s) -> String`

Removes leading whitespace from `s`.

- `s` *(String)*: the string to trim.

**Returns:** the trimmed `String`.

```tigr
print(String.trim_start('  hi  ') + '|');   // => hi  |
```

### `trim_end(s) -> String`

Removes trailing whitespace from `s`.

- `s` *(String)*: the string to trim.

**Returns:** the trimmed `String`.

```tigr
print('|' + String.trim_end('  hi  '));   // => |  hi
```

### `repeat(s, n) -> String`

Concatenates `n` copies of `s`.

- `s` *(String)*: the string to repeat.
- `n` *(Int)*: how many copies.

**Returns:** the repeated `String`.
**Raises:** an error if `n` is negative.

```tigr
print(String.repeat('ab', 3));   // => ababab
```

### `chars(s) -> Array`

Splits `s` into one-character strings, one per Unicode character.

- `s` *(String)*: the string to split.

**Returns:** an `Array` of single-character `String` values.

```tigr
print(String.chars('hi!'));   // => [h, i, !]
```

### `reverse(s) -> String`

Reverses the characters of `s`.

- `s` *(String)*: the string to reverse.

**Returns:** the reversed `String`.

```tigr
print(String.reverse('hello'));   // => olleh
```

### `capitalize(s) -> String`

Uppercases the first character of `s` and leaves the rest unchanged.

- `s` *(String)*: the string to capitalize.

**Returns:** the capitalized `String`.

```tigr
print(String.capitalize('hello world'));   // => Hello world
```

### `pad_start(s, len, ch) -> String`

Pads `s` on the left with `ch` until it is at least `len` characters wide.

- `s` *(String)*: the string to pad.
- `len` *(Int)*: the target width.
- `ch` *(String)*: the single-character pad string.

**Returns:** the left-padded `String`.

```tigr
print(String.pad_start('42', 5, '0'));   // => 00042
```

### `pad_end(s, len, ch) -> String`

Pads `s` on the right with `ch` until it is at least `len` characters wide.

- `s` *(String)*: the string to pad.
- `len` *(Int)*: the target width.
- `ch` *(String)*: the single-character pad string.

**Returns:** the right-padded `String`.

```tigr
print(String.pad_end('42', 5, '.') + '|');   // => 42...|
```

### `is_blank(s) -> Bool`

Tests whether `s` is empty or contains only whitespace.

- `s` *(String)*: the string to test.

**Returns:** `true` if `s` is empty or all whitespace, otherwise `false`.

```tigr
print(String.is_blank('   '));   // => true
print(String.is_blank(' x '));   // => false
```

### `words(s) -> Array`

Splits `s` on runs of whitespace, dropping empty fields.

- `s` *(String)*: the string to split.

**Returns:** an `Array` of non-empty `String` words.

```tigr
print(String.words('  the  quick fox '));   // => [the, quick, fox]
```

### `lines(s) -> Array`

Splits `s` into lines on `\n` or `\r\n`. A trailing newline adds no final empty line.

- `s` *(String)*: the string to split.

**Returns:** an `Array` of `String` lines.

```tigr
print(String.lines('one\ntwo\nthree'));   // => [one, two, three]
```

### `split_any(s, delims) -> Array`

Splits `s` on any character that appears in `delims`. An empty `delims` yields `s` unsplit.

- `s` *(String)*: the string to split.
- `delims` *(String)*: the set of delimiter characters.

**Returns:** an `Array` of `String` pieces.

```tigr
print(String.split_any('a,b;c d', ',; '));   // => [a, b, c, d]
```

### `find_all(s, needle) -> Array`

Finds the byte offset of every non-overlapping occurrence of `needle`.

- `s` *(String)*: the string to search.
- `needle` *(String)*: the substring to look for.

**Returns:** an `Array` of `Int` byte offsets, empty for an empty `needle`.

```tigr
print(String.find_all('abababa', 'aba'));   // => [0, 4]
```

### `count(s, needle) -> Int`

Counts the non-overlapping occurrences of `needle` in `s`.

- `s` *(String)*: the string to search.
- `needle` *(String)*: the substring to count.

**Returns:** the count as an `Int`, `0` for an empty `needle`.

```tigr
print(String.count('abababa', 'aba'));   // => 2
```

### `strip_prefix(s, prefix) -> String`

Removes `prefix` from the start of `s` if it is there, otherwise returns `s` unchanged.

- `s` *(String)*: the string to strip.
- `prefix` *(String)*: the prefix to remove.

**Returns:** `s` without the prefix, or `s` unchanged.

```tigr
print(String.strip_prefix('img_03.png', 'img_'));   // => 03.png
print(String.strip_prefix('readme', 'img_'));       // => readme
```

### `strip_suffix(s, suffix) -> String`

Removes `suffix` from the end of `s` if it is there, otherwise returns `s` unchanged.

- `s` *(String)*: the string to strip.
- `suffix` *(String)*: the suffix to remove.

**Returns:** `s` without the suffix, or `s` unchanged.

```tigr
print(String.strip_suffix('readme.txt', '.txt'));   // => readme
```

### `matches_glob(s, pattern) -> Bool`

Tests `s` against a shell-style glob pattern. The whole string must match. The pattern language supports `*` (any run of characters), `?` (exactly one character), `[abc]` and `[a-z]` (one character from a set or range), `[!abc]` (one character not in the set), and `\` to escape a metacharacter.

- `s` *(String)*: the string to test.
- `pattern` *(String)*: the glob pattern.

**Returns:** `true` if the whole string matches, otherwise `false`.
**Raises:** an error on a malformed pattern, such as an unterminated `[` or a dangling `\`.

```tigr
print(String.matches_glob('readme.txt', '*.txt'));            // => true
print(String.matches_glob('img_03.png', 'img_[0-9][0-9].png'));   // => true
```

### `format(value, spec) -> String`

Renders `value` through the format spec mini-language. The spec controls fill, alignment, sign, width, grouping, precision, and the rendered type. The full grammar is `[[fill]align][sign]['#'][width][','][.precision][type]`, each part optional. Type codes include `s` (string), `d` (decimal), `f` (fixed-point float), `e` and `E` (scientific), `x` and `X` (hex), `b` (binary), and `o` (octal). See the spec for the complete grammar.

- `value` *(value)*: the value to render.
- `spec` *(String)*: the format spec.

**Returns:** the formatted `String`.
**Raises:** an error on a mismatched type code, an unparsable spec, or a value the spec cannot render (such as a fractional float with a `d` type).

```tigr
print(String.format(42, '05'));        // => 00042
print(String.format(3.14159, '.2f'));  // => 3.14
print(String.format(255, '#x'));       // => 0xff
print(String.format(1234567, ',d'));   // => 1,234,567
```

### `printf(template, args?) -> String`

Renders `template`, replacing each `%(SPEC)` placeholder with `format(next arg, SPEC)`. A `%%` is a literal percent. Strict on arity: passing too few or too many `args`, or a malformed placeholder, raises.

- `template` *(String)*: the template with `%(SPEC)` placeholders.
- `args` *(Array, optional)*: the values to fill placeholders with, defaulting to `[]`.

**Returns:** the rendered `String`.
**Raises:** an error on an arity mismatch, a stray `%`, or an unterminated placeholder.

```tigr
print(String.printf('%(<6)%(>6.2f)', ['tea', 1.5]));   // => tea     1.50
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#string): the authoritative spec for `String`, including the full format and glob grammars
- [Array](array.md): `split` returns an array, and `join` consumes one
- [Bytes](../../LANGUAGE.md#bytes-v013): for raw byte buffers rather than text
