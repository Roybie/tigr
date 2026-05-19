# `Bytes`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#bytes-v013)

A `Bytes` value is a mutable byte buffer, a growable sequence of integers each in the range `0..=255`. It is a value type in its own right, so it gets the same syntax as the other collections: `b[i]` indexes a byte, `#b` is the length, `for (x, b) { ... }` iterates the bytes, `b[start:end]` slices a fresh buffer, and `+` / `+=` concatenate. Import the module with `Bytes := import 'Bytes'`. The module supplies what the operators cannot: construction, conversion to and from strings, hex, base64, arrays, in-place growth, and a family of fixed-width integer readers and writers for binary-protocol work.

```tigr
Bytes := import 'Bytes';

b := Bytes.from_string('hi');
print(#b);              // => 2
print(b[0]);            // => 104
```

## Functions

| Function | Summary |
|----------|---------|
| [`new(n, fill?) -> Bytes`](#newn-fill---bytes) | Creates a buffer of `n` bytes. |
| [`from_array(arr) -> Bytes`](#from_arrayarr---bytes) | Packs an array of integers, each in `0..=255`, into a buffer. |
| [`from_string(s) -> Bytes`](#from_strings---bytes) | Encodes a string as its UTF-8 bytes. |
| [`from_hex(s) -> Bytes`](#from_hexs---bytes) | Decodes a hex string. |
| [`from_base64(s) -> Bytes`](#from_base64s---bytes) | Decodes a standard-alphabet base64 string. |
| [`to_array(b) -> Array`](#to_arrayb---array) | Copies the buffer into an array, one `Int` per byte. |
| [`to_string(b) -> String`](#to_stringb---string) | Decodes the buffer as UTF-8 text. |
| [`to_hex(b) -> String`](#to_hexb---string) | Encodes the buffer as lower-case hex, two digits per byte, with no separators. |
| [`to_base64(b) -> String`](#to_base64b---string) | Encodes the buffer as standard-alphabet base64 with `=` padding. |
| [`push(b, byte) -> Bytes`](#pushb-byte---bytes) | Appends one byte to the end of `b`, in place. |
| [`extend(b, other) -> Bytes`](#extendb-other---bytes) | Appends every byte of `other` to `b`, in place. |
| [`slice(b, start, end) -> Bytes`](#sliceb-start-end---bytes) | Copies `b[start..end]` into a new buffer. |
| [`concat(a, b) -> Bytes`](#concata-b---bytes) | Builds a new buffer holding `a` followed by `b`. |


### `new(n, fill?) -> Bytes`

Creates a buffer of `n` bytes. Without `fill` the bytes are all zero.

- `n` *(Int)*: the buffer length, which must not be negative.
- `fill` *(Int, optional)*: the byte value to repeat, in `0..=255`.

**Returns:** a new `Bytes` of length `n`.
**Raises:** a string error if `n` is negative or `fill` is out of range.

```tigr
Bytes := import 'Bytes';

print(Bytes.new(3));            // => Bytes[00 00 00]
print(Bytes.new(2, 255));       // => Bytes[ff ff]
```

### `from_array(arr) -> Bytes`

Packs an array of integers, each in `0..=255`, into a buffer.

- `arr` *(Array)*: the integers to pack, one byte per element.

**Returns:** a new `Bytes` of the same length as `arr`.
**Raises:** a string error if an element is not an `Int`, or is outside `0..=255`.

```tigr
Bytes := import 'Bytes';

print(Bytes.from_array([104, 105]));    // => Bytes[68 69]
```

### `from_string(s) -> Bytes`

Encodes a string as its UTF-8 bytes. This always succeeds.

- `s` *(String)*: the text to encode.

**Returns:** a new `Bytes` holding the UTF-8 encoding of `s`.

```tigr
Bytes := import 'Bytes';

print(Bytes.from_string('hi'));         // => Bytes[68 69]
```

### `from_hex(s) -> Bytes`

Decodes a hex string. ASCII whitespace in `s` is ignored, and both letter cases are accepted.

- `s` *(String)*: the hex digits.

**Returns:** a new `Bytes`, one byte per pair of hex digits.
**Raises:** a structured `decode` error if `s` has an odd number of digits or a non-hex character.

```tigr
Bytes := import 'Bytes';

print(Bytes.from_hex('deadbeef'));      // => Bytes[de ad be ef]
```

### `from_base64(s) -> Bytes`

Decodes a standard-alphabet base64 string. ASCII whitespace is ignored.

- `s` *(String)*: the base64 text.

**Returns:** a new `Bytes`.
**Raises:** a structured `decode` error if `s` is not valid base64.

```tigr
Bytes := import 'Bytes';

print(Bytes.from_base64('Zm9v'));       // => Bytes[66 6f 6f]
```

### `to_array(b) -> Array`

Copies the buffer into an array, one `Int` per byte.

- `b` *(Bytes)*: the buffer to read.

**Returns:** an `Array` of integers in `0..=255`.

```tigr
Bytes := import 'Bytes';

print(Bytes.to_array(Bytes.from_string('hi')));     // => [104, 105]
```

### `to_string(b) -> String`

Decodes the buffer as UTF-8 text.

- `b` *(Bytes)*: the buffer to decode.

**Returns:** the decoded `String`.
**Raises:** a structured `decode` error if the bytes are not valid UTF-8.

```tigr
Bytes := import 'Bytes';

print(Bytes.to_string(Bytes.from_array([104, 105])));   // => hi
```

### `to_hex(b) -> String`

Encodes the buffer as lower-case hex, two digits per byte, with no separators.

- `b` *(Bytes)*: the buffer to encode.

**Returns:** the hex `String`.

```tigr
Bytes := import 'Bytes';

print(Bytes.to_hex(Bytes.from_array([222, 173])));      // => dead
```

### `to_base64(b) -> String`

Encodes the buffer as standard-alphabet base64 with `=` padding.

- `b` *(Bytes)*: the buffer to encode.

**Returns:** the base64 `String`.

```tigr
Bytes := import 'Bytes';

print(Bytes.to_base64(Bytes.from_string('foo')));       // => Zm9v
```

### `push(b, byte) -> Bytes`

Appends one byte to the end of `b`, in place.

- `b` *(Bytes)*: the buffer to grow.
- `byte` *(Int)*: the byte value to append, in `0..=255`.

**Returns:** `b`, the same buffer.
**Raises:** a string error if `byte` is out of range.

```tigr
Bytes := import 'Bytes';

b := Bytes.from_array([1, 2]);
Bytes.push(b, 3);
print(b);               // => Bytes[01 02 03]
```

### `extend(b, other) -> Bytes`

Appends every byte of `other` to `b`, in place. `other` is snapshotted first, so `extend(b, b)` is safe.

- `b` *(Bytes)*: the buffer to grow.
- `other` *(Bytes)*: the buffer whose bytes are appended.

**Returns:** `b`, the same buffer.

```tigr
Bytes := import 'Bytes';

b := Bytes.from_array([1, 2]);
Bytes.extend(b, Bytes.from_array([9, 9]));
print(b);               // => Bytes[01 02 09 09]
```

### `slice(b, start, end) -> Bytes`

Copies `b[start..end]` into a new buffer. A negative index counts from the end, and the bounds are clamped to the buffer. This is the function form of the `b[start:end]` operator.

- `b` *(Bytes)*: the buffer to slice.
- `start` *(Int)*: the first index, inclusive.
- `end` *(Int)*: the index one past the last, exclusive.

**Returns:** a new `Bytes` holding the selected range.

```tigr
Bytes := import 'Bytes';

print(Bytes.slice(Bytes.from_array([10, 20, 30, 40]), 1, 3));   // => Bytes[14 1e]
```

### `concat(a, b) -> Bytes`

Builds a new buffer holding `a` followed by `b`. This is the function form of the `a + b` operator. Neither input is modified.

- `a` *(Bytes)*: the first buffer.
- `b` *(Bytes)*: the second buffer.

**Returns:** a new `Bytes`.

```tigr
Bytes := import 'Bytes';

print(Bytes.concat(Bytes.from_array([1]), Bytes.from_array([2, 3])));   // => Bytes[01 02 03]
```

## Reading and writing integers

For binary protocols, the module has a family of fixed-width integer readers and writers. Each name is built from three parts: the sign (`u` for unsigned, `i` for signed two's-complement), the width in bits (`8`, `16`, `32`, `64`), and, for the multi-byte widths, the byte order (`_be` big-endian, `_le` little-endian). The 8-bit functions have no endianness suffix because a single byte has no byte order.

A reader takes the buffer and a byte offset, and returns the decoded `Int`. A writer takes the buffer, a byte offset, and the value, writes it in place, and returns the buffer. A reader raises a string error if the offset is negative or the field would run off the end of the buffer; `read_u64_*` raises a catchable `overflow` error if the value does not fit a signed 64-bit `Int`. A writer raises a string error if the offset is out of bounds, or if the value does not fit the field (an unsigned writer also rejects a negative value).

The readers are: `read_u8`, `read_i8`, `read_u16_be`, `read_u16_le`, `read_i16_be`, `read_i16_le`, `read_u32_be`, `read_u32_le`, `read_i32_be`, `read_i32_le`, `read_u64_be`, `read_u64_le`, `read_i64_be`, `read_i64_le`. Each has the signature `read_TYPE(b, offset) -> Int`.

The writers are: `write_u8`, `write_i8`, `write_u16_be`, `write_u16_le`, `write_i16_be`, `write_i16_le`, `write_u32_be`, `write_u32_le`, `write_i32_be`, `write_i32_le`, `write_u64_be`, `write_u64_le`, `write_i64_be`, `write_i64_le`. Each has the signature `write_TYPE(b, offset, value) -> Bytes`.

```tigr
Bytes := import 'Bytes';

buf := Bytes.new(4);
Bytes.write_u32_be(buf, 0, 305419896);
print(Bytes.to_hex(buf));                       // => 12345678
print(Bytes.read_u32_be(buf, 0));               // => 305419896
```

Byte order changes the result. The same two bytes `[18, 52]` read as a 16-bit integer give a different value big-endian versus little-endian, and a signed reader sign-extends:

```tigr
Bytes := import 'Bytes';

pair := Bytes.from_array([18, 52]);
print(Bytes.read_u16_be(pair, 0));              // => 4660
print(Bytes.read_u16_le(pair, 0));              // => 13330
print(Bytes.read_i8(Bytes.from_array([255]), 0));   // => -1
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#bytes-v013): the authoritative spec for `Bytes`
- [Net](net.md): socket reads and writes that produce and consume `Bytes`
- [String](string.md): text, and the conversions to and from `Bytes`
