# `JSON`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.4](../../LANGUAGE.md#134-json-v04)

The `JSON` module reads and writes JSON text. It exposes no value type of its own: `parse` returns ordinary tigr values (`null`, `Bool`, `Float`, `String`, `Array`, `Object`) and `stringify` accepts the same. Import it with `JSON := import 'JSON'`. Numbers are always parsed as `Float`, the convention JSON itself follows, so an integer-valued number comes back as `Float`.

JSON source contains `{` and `}`, which a single-quoted tigr string interpolates. Escape them as `\{` and `\}`, or build the source some other way.

```tigr
JSON := import 'JSON';

print(JSON.parse('\{"n": 1, "ok": true\}'));    // => ${n: 1.0, ok: true}
```

## Functions

### `parse(text) -> value`

Parses one JSON value out of a string. Whitespace before and after the value is allowed; any other trailing content is an error.

- `text` *(String)*: the JSON source.

**Returns:** the decoded value. Objects become `Object`, arrays become `Array`, JSON numbers become `Float`, and `null`/`true`/`false`/strings map to their tigr equivalents.
**Raises:** a string error if `text` is not a String, or a string error with a line and column if the JSON is malformed.

```tigr
JSON := import 'JSON';

print(JSON.parse('[1, 2, 3]'));          // => [1.0, 2.0, 3.0]
print(JSON.parse('  null  '));           // => null
print(JSON.parse('"hi\\nthere"'));       // => hi
```

### `stringify(value, indent?) -> String`

Serializes a tigr value to JSON text. With no `indent` the output is compact; with one, the output is pretty-printed.

- `value` *(value)*: the value to encode. `null`, `Bool`, `Int`, `Float`, `String`, `Array`, and `Object` are serializable. An `Int` is written with no decimal point; an integer-valued `Float` keeps a `.0` suffix.
- `indent` *(Int or String, optional)*: an `Int` indents each level by that many spaces; a `String` uses that string as the indent unit; `null` is the same as omitting it.

**Returns:** the JSON text as a `String`.
**Raises:** a `cycle` error if `value` contains a reference cycle. A string error if `value` (or anything inside it) is a type JSON cannot represent (a function, range, map, set, bytes, bigint, channel, task, or socket), or if a `Float` is `NaN` or infinite.

```tigr
JSON := import 'JSON';

print(JSON.stringify(${a: 1, b: [true, null]}));        // => {"a":1,"b":[true,null]}
print(JSON.stringify(3.0));                             // => 3.0
print(JSON.stringify([1, 2], 2));                       // => [
                                                        //      1,
                                                        //      2
                                                        //    ]
```

## See also

- [LANGUAGE.md §13.4](../../LANGUAGE.md#134-json-v04): the authoritative spec for `JSON`
- [Errors](../language/errors.md): catching the `cycle` error and parse failures
- [Object](object.md): the value type `parse` produces for JSON objects
