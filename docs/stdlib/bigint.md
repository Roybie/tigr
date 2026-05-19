# `BigInt`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#bigint-v013)

A `BigInt` value is an arbitrary-precision integer. It is a value type in its own right, immutable, and not bounded by the 64-bit range of an ordinary `Int`. A `BigInt` is always created explicitly, with `BigInt.new`: an `Int` that overflows does not silently become a `BigInt`, it raises the catchable `overflow` error as before. Once you have one, it works with the ordinary operators `+ - * / % ^^`, unary `-`, and the comparisons. An `Int` operand is promoted to `BigInt`; a `Float` operand promotes the whole expression to `Float`. The `/` operator is exact-or-raise: it yields a `BigInt` only when the division comes out even, and otherwise raises `inexact_division`. For truncating integer division, use `div` and `divmod`. Import the module with `BigInt := import 'BigInt'`.

```tigr
BigInt := import 'BigInt';

n := BigInt.new(2) ^^ 100;
print(n);               // => 1267650600228229401496703205376
```

## Functions

| Function | Summary |
|----------|---------|
| [`new(x) -> BigInt`](#newx---bigint) | Builds a `BigInt` from an `Int`, a decimal `String`, or another `BigInt`. |
| [`to_int(b) -> Int`](#to_intb---int) | Narrows a `BigInt` back to an ordinary `Int`. |
| [`to_float(b) -> Float`](#to_floatb---float) | Converts a `BigInt` to a `Float`. |
| [`to_str_radix(b, radix) -> String`](#to_str_radixb-radix---string) | Renders a `BigInt` as a string in a given base. |
| [`divmod(a, b) -> Array`](#divmoda-b---array) | Divides `a` by `b`, truncating toward zero, and returns the quotient and remainder together. |
| [`div(a, b) -> BigInt`](#diva-b---bigint) | Divides `a` by `b`, truncating toward zero, and returns just the quotient. |
| [`abs(b) -> BigInt`](#absb---bigint) | Returns the absolute value. |
| [`pow(base, exp) -> BigInt`](#powbase-exp---bigint) | Raises `base` to a non-negative integer power, exactly. |
| [`sign(b) -> Int`](#signb---int) | Returns the sign of the value. |
| [`is_negative(b) -> Bool`](#is_negativeb---bool) | Tests whether the value is below zero. |
| [`gcd(a, b) -> BigInt`](#gcda-b---bigint) | Returns the greatest common divisor, always non-negative. |
| [`lcm(a, b) -> BigInt`](#lcma-b---bigint) | Returns the least common multiple. |


### `new(x) -> BigInt`

Builds a `BigInt` from an `Int`, a decimal `String`, or another `BigInt`.

- `x` *(Int, String, or BigInt)*: an `Int` is widened; a `String` is parsed as a decimal integer with an optional leading sign and surrounding whitespace trimmed; a `BigInt` is returned unchanged.

**Returns:** a `BigInt`.
**Raises:** a structured `parse` error if `x` is a malformed string. A string error if `x` is some other type.

```tigr
BigInt := import 'BigInt';

print(BigInt.new('123456789012345678901234567890'));    // => 123456789012345678901234567890
```

### `to_int(b) -> Int`

Narrows a `BigInt` back to an ordinary `Int`.

- `b` *(BigInt or Int)*: the value to narrow.

**Returns:** the value as an `Int`.
**Raises:** the catchable `overflow` error if the value is outside the signed 64-bit range.

```tigr
BigInt := import 'BigInt';

print(BigInt.to_int(BigInt.new(42)));   // => 42
```

### `to_float(b) -> Float`

Converts a `BigInt` to a `Float`. A magnitude beyond the float range saturates to positive or negative infinity. This never raises.

- `b` *(BigInt or Int)*: the value to convert.

**Returns:** the value as a `Float`.

```tigr
BigInt := import 'BigInt';

print(BigInt.to_float(BigInt.new(1000)));   // => 1000.0
```

### `to_str_radix(b, radix) -> String`

Renders a `BigInt` as a string in a given base. This covers radix printing for `BigInt`, which `str()` only does for `Int`.

- `b` *(BigInt or Int)*: the value to render.
- `radix` *(Int)*: the base, from 2 to 36.

**Returns:** the value as a `String` in base `radix`.
**Raises:** a string error if `radix` is outside `2..=36`.

```tigr
BigInt := import 'BigInt';

print(BigInt.to_str_radix(BigInt.new(255), 16));    // => ff
```

### `divmod(a, b) -> Array`

Divides `a` by `b`, truncating toward zero, and returns the quotient and remainder together. The remainder takes the sign of `a`.

- `a` *(BigInt or Int)*: the dividend.
- `b` *(BigInt or Int)*: the divisor.

**Returns:** a two-element `Array`, `[quotient, remainder]`, both `BigInt`.
**Raises:** the catchable `div_by_zero` error if `b` is zero.

```tigr
BigInt := import 'BigInt';

print(BigInt.divmod(BigInt.new(17), 5));    // => [3, 2]
```

### `div(a, b) -> BigInt`

Divides `a` by `b`, truncating toward zero, and returns just the quotient.

- `a` *(BigInt or Int)*: the dividend.
- `b` *(BigInt or Int)*: the divisor.

**Returns:** the quotient as a `BigInt`.
**Raises:** the catchable `div_by_zero` error if `b` is zero.

```tigr
BigInt := import 'BigInt';

print(BigInt.div(BigInt.new(17), 5));   // => 3
```

### `abs(b) -> BigInt`

Returns the absolute value.

- `b` *(BigInt or Int)*: the value.

**Returns:** the absolute value as a `BigInt`.

```tigr
BigInt := import 'BigInt';

print(BigInt.abs(BigInt.new(-9)));      // => 9
```

### `pow(base, exp) -> BigInt`

Raises `base` to a non-negative integer power, exactly.

- `base` *(BigInt or Int)*: the base.
- `exp` *(Int)*: the exponent, which must not be negative (a fractional result is not a `BigInt`).

**Returns:** `base` to the `exp` power, as a `BigInt`.
**Raises:** a string error if `exp` is negative.

```tigr
BigInt := import 'BigInt';

print(BigInt.pow(BigInt.new(2), 64));   // => 18446744073709551616
```

### `sign(b) -> Int`

Returns the sign of the value.

- `b` *(BigInt or Int)*: the value.

**Returns:** `-1` for a negative value, `0` for zero, `1` for a positive value.

```tigr
BigInt := import 'BigInt';

print(BigInt.sign(BigInt.new(-3)));     // => -1
```

### `is_negative(b) -> Bool`

Tests whether the value is below zero.

- `b` *(BigInt or Int)*: the value.

**Returns:** `true` if `b` is negative, otherwise `false`.

```tigr
BigInt := import 'BigInt';

print(BigInt.is_negative(BigInt.new(-3)));      // => true
```

### `gcd(a, b) -> BigInt`

Returns the greatest common divisor, always non-negative.

- `a` *(BigInt or Int)*: the first value.
- `b` *(BigInt or Int)*: the second value.

**Returns:** the greatest common divisor as a `BigInt`.

```tigr
BigInt := import 'BigInt';

print(BigInt.gcd(BigInt.new(48), 18));  // => 6
```

### `lcm(a, b) -> BigInt`

Returns the least common multiple.

- `a` *(BigInt or Int)*: the first value.
- `b` *(BigInt or Int)*: the second value.

**Returns:** the least common multiple as a `BigInt`.

```tigr
BigInt := import 'BigInt';

print(BigInt.lcm(BigInt.new(4), 6));    // => 12
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#bigint-v013): the authoritative spec for `BigInt`
- [Math](math.md): numeric functions for ordinary `Int` and `Float`
- [Errors](../language/errors.md): catching `overflow`, `div_by_zero`, and `parse`
