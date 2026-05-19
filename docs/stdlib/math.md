# `Math`

> Pure-tigr source module, `stdlib/Math.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#math)

The `Math` module provides numeric functions and two constants. Trig, logarithm, and exponential functions are backed by native code; the small helpers (`abs`, `sign`, `min`, `max`, `clamp`) are pure tigr. Import it as `Math := import 'Math'`.

The numeric functions raise on a non-`Number` argument. Angles are in radians.

```tigr
Math := import 'Math';

print(Math.sqrt(144));   // => 12.0
```

## Constants

### `PI` *(Float)*

The ratio of a circle's circumference to its diameter, `3.141592653589793`.

```tigr
Math := import 'Math';

print(Math.PI);   // => 3.141592653589793
```

### `E` *(Float)*

Euler's number, the base of the natural logarithm, `2.718281828459045`.

```tigr
Math := import 'Math';

print(Math.E);   // => 2.718281828459045
```

## Functions

| Function | Summary |
|----------|---------|
| [`sqrt(x) -> Float`](#sqrtx---float) | Computes the square root of `x`. |
| [`log(x) -> Float`](#logx---float) | Computes the natural logarithm (base `E`) of `x`. |
| [`log2(x) -> Float`](#log2x---float) | Computes the base-2 logarithm of `x`. |
| [`log10(x) -> Float`](#log10x---float) | Computes the base-10 logarithm of `x`. |
| [`exp(x) -> Float`](#expx---float) | Computes `E` raised to the power `x`. |
| [`sin(x) -> Float`](#sinx---float) | Computes the sine of `x`, where `x` is in radians. |
| [`cos(x) -> Float`](#cosx---float) | Computes the cosine of `x`, where `x` is in radians. |
| [`tan(x) -> Float`](#tanx---float) | Computes the tangent of `x`, where `x` is in radians. |
| [`pow(x, y) -> Float`](#powx-y---float) | Raises `x` to the power `y`. |
| [`abs(x) -> Number`](#absx---number) | Computes the absolute value of `x`. |
| [`sign(x) -> Int`](#signx---int) | Reports the sign of `x`. |
| [`min(a, b) -> value`](#mina-b---value) | Returns the smaller of `a` and `b`, using `<` to compare. |
| [`max(a, b) -> value`](#maxa-b---value) | Returns the larger of `a` and `b`, using `>` to compare. |
| [`clamp(x, lo, hi) -> value`](#clampx-lo-hi---value) | Constrains `x` to the range `[lo, hi]`. |


### `sqrt(x) -> Float`

Computes the square root of `x`.

- `x` *(Number)*: the value to take the root of.

**Returns:** the square root as a `Float`.

```tigr
Math := import 'Math';

print(Math.sqrt(16));   // => 4.0
```

### `log(x) -> Float`

Computes the natural logarithm (base `E`) of `x`.

- `x` *(Number)*: the value to take the logarithm of.

**Returns:** the natural logarithm as a `Float`.

```tigr
Math := import 'Math';

print(Math.log(Math.E));   // => 1.0
```

### `log2(x) -> Float`

Computes the base-2 logarithm of `x`.

- `x` *(Number)*: the value to take the logarithm of.

**Returns:** the base-2 logarithm as a `Float`.

```tigr
Math := import 'Math';

print(Math.log2(8));   // => 3.0
```

### `log10(x) -> Float`

Computes the base-10 logarithm of `x`.

- `x` *(Number)*: the value to take the logarithm of.

**Returns:** the base-10 logarithm as a `Float`.

```tigr
Math := import 'Math';

print(Math.log10(1000));   // => 3.0
```

### `exp(x) -> Float`

Computes `E` raised to the power `x`.

- `x` *(Number)*: the exponent.

**Returns:** `E ^^ x` as a `Float`.

```tigr
Math := import 'Math';

print(Math.exp(0));   // => 1.0
```

### `sin(x) -> Float`

Computes the sine of `x`, where `x` is in radians.

- `x` *(Number)*: the angle in radians.

**Returns:** the sine as a `Float`.

```tigr
Math := import 'Math';

print(Math.sin(0));   // => 0.0
```

### `cos(x) -> Float`

Computes the cosine of `x`, where `x` is in radians.

- `x` *(Number)*: the angle in radians.

**Returns:** the cosine as a `Float`.

```tigr
Math := import 'Math';

print(Math.cos(0));   // => 1.0
```

### `tan(x) -> Float`

Computes the tangent of `x`, where `x` is in radians.

- `x` *(Number)*: the angle in radians.

**Returns:** the tangent as a `Float`.

```tigr
Math := import 'Math';

print(Math.tan(0));   // => 0.0
```

### `pow(x, y) -> Float`

Raises `x` to the power `y`. The result is always a `Float`. The `^^` operator does the same thing and works for both integer and float results.

- `x` *(Number)*: the base.
- `y` *(Number)*: the exponent.

**Returns:** `x` raised to `y` as a `Float`.

```tigr
Math := import 'Math';

print(Math.pow(2, 10));   // => 1024.0
```

### `abs(x) -> Number`

Computes the absolute value of `x`.

- `x` *(Number)*: the value.

**Returns:** the absolute value, with the same numeric type as `x`.

```tigr
Math := import 'Math';

print(Math.abs(-7));     // => 7
print(Math.abs(-2.5));   // => 2.5
```

### `sign(x) -> Int`

Reports the sign of `x`.

- `x` *(Number)*: the value.

**Returns:** `-1` if `x` is negative, `1` if positive, `0` if zero.

```tigr
Math := import 'Math';

print(Math.sign(-9));   // => -1
print(Math.sign(0));    // => 0
print(Math.sign(4));    // => 1
```

### `min(a, b) -> value`

Returns the smaller of `a` and `b`, using `<` to compare.

- `a` *(value)*: the first value.
- `b` *(value)*: the second value.

**Returns:** `a` if `a < b`, otherwise `b`.

```tigr
Math := import 'Math';

print(Math.min(3, 8));   // => 3
```

### `max(a, b) -> value`

Returns the larger of `a` and `b`, using `>` to compare.

- `a` *(value)*: the first value.
- `b` *(value)*: the second value.

**Returns:** `a` if `a > b`, otherwise `b`.

```tigr
Math := import 'Math';

print(Math.max(3, 8));   // => 8
```

### `clamp(x, lo, hi) -> value`

Constrains `x` to the range `[lo, hi]`.

- `x` *(value)*: the value to constrain.
- `lo` *(value)*: the lower bound.
- `hi` *(value)*: the upper bound.

**Returns:** `lo` if `x < lo`, `hi` if `x > hi`, otherwise `x`.

```tigr
Math := import 'Math';

print(Math.clamp(15, 0, 10));   // => 10
print(Math.clamp(-3, 0, 10));   // => 0
print(Math.clamp(7, 0, 10));    // => 7
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#math): the authoritative spec for `Math`
- [Array](array.md): `sum`, `min_of`, and `max_of` aggregate over a whole array
