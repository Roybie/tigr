# `Random`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#random-v09)

The `Random` module draws pseudo-random numbers. It exposes no value type of its own. Every function draws from one per-thread PRNG stream, the same stream the bare `rand()` builtin uses, so `Random.seed(n)` makes both this module and `rand()` reproducible. Import it with `Random := import 'Random'`. Seeding before a draw is the way to get deterministic output in a test.

```tigr
Random := import 'Random';

Random.seed(1);
print(Random.int(1, 6));        // => 3
```

## Functions

### `seed(n) -> null`

Pins the PRNG stream to `n`. Any `Int` works, `seed(0)` included. After seeding, the sequence of draws is fully determined.

- `n` *(Int)*: the seed value.

**Returns:** `null`.
**Raises:** a string error if `n` is not an `Int`.

```tigr
Random := import 'Random';

Random.seed(42);
a := Random.int(0, 100);
Random.seed(42);
b := Random.int(0, 100);
print(a == b);                  // => true
```

### `float() -> Float`

Draws a uniform `Float` in the half-open range `[0, 1)`.

**Returns:** a `Float` that is at least `0` and below `1`.

```tigr
Random := import 'Random';

Random.seed(7);
print(Random.float());          // => 0.8597941211851238
```

### `int(lo, hi) -> Int`

Draws a uniform `Int` from the inclusive range `[lo, hi]`. Both endpoints can be drawn.

- `lo` *(Int)*: the lowest value that can be drawn.
- `hi` *(Int)*: the highest value that can be drawn.

**Returns:** an `Int` between `lo` and `hi`, both inclusive.
**Raises:** a string error if `lo` exceeds `hi`, or if either argument is not an `Int`.

```tigr
Random := import 'Random';

Random.seed(3);
print(Random.int(1, 6));        // => 1
```

### `bool() -> Bool`

Draws `true` or `false`, each with probability one half.

**Returns:** a `Bool`.

```tigr
Random := import 'Random';

Random.seed(9);
print(Random.bool());           // => false
```

### `choice(arr) -> value`

Picks one element of `arr` uniformly at random.

- `arr` *(Array)*: a non-empty array to draw from.

**Returns:** one element of `arr`.
**Raises:** a string error if `arr` is empty, or if it is not an `Array`.

```tigr
Random := import 'Random';

Random.seed(2);
print(Random.choice(['rock', 'paper', 'scissors']));    // => paper
```

### `range(r) -> Int`

Picks one value of a Range uniformly at random. The range's step is respected, so `range(0..=8:2)` draws one of `0, 2, 4, 6, 8`.

- `r` *(Range)*: a non-empty range to draw from.

**Returns:** an `Int` that the range would yield.
**Raises:** a string error if `r` is empty, or if it is not a `Range`.

```tigr
Random := import 'Random';

Random.seed(5);
print(Random.range(0..=8:2));   // => 6
```

### `shuffle(arr) -> Array`

Builds a new array holding `arr`'s elements in a random order, using a Fisher-Yates shuffle. The input array is left untouched.

- `arr` *(Array)*: the array to shuffle.

**Returns:** a fresh `Array` with the same elements in a new order.
**Raises:** a string error if `arr` is not an `Array`.

```tigr
Random := import 'Random';

Random.seed(4);
print(Random.shuffle([1, 2, 3, 4, 5]));     // => [3, 4, 2, 1, 5]
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#random-v09): the authoritative spec for `Random`
- [Math](math.md): deterministic numeric functions
- [Array](array.md): the collection `choice` and `shuffle` work on
