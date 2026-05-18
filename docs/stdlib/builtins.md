# Built-in functions

> Global builtin functions, always in scope (no import needed)
> Spec: [LANGUAGE.md §13.1](../../LANGUAGE.md#131-required-built-ins-for-v02)

The built-ins are ordinary bindings in the root environment, so they need no `import`. They cover printing, type conversion, type inspection, rounding, and a couple of runtime hooks. Being plain bindings, they can be shadowed, passed to other functions, or stored in a variable like any value.

## Functions

### `print(value1, value2?) -> value`

Writes each argument to stdout in its `str` form, separated by single spaces, followed by a newline. With no arguments it writes just the newline. A `String` is printed without surrounding quotes.

- `value1` *(value)*: the first thing to print. `print` is variadic, so any number of arguments may follow.

**Returns:** the last argument, or `null` if called with none.

```tigr
print('x is', 41 + 1);   // => x is 42
print();                 // =>
last := print(1, 2, 3);  // => 1 2 3
print(last);             // => 3
```

### `str(value, radix?, prefix?) -> String`

Produces the canonical string form of a value. With one argument: `null` becomes `'null'`, numbers become decimal text (an `Int` has no point, a `Float` always does), a `String` is returned unchanged, arrays and objects are bracketed with their elements `str`-ed. With a `radix`, an `Int` is rendered in that base. With `prefix` set to `true`, the literal base marker is prepended.

- `value` *(value)*: the value to render.
- `radix` *(Int, optional)*: a base in `2..=36`, lowercase digits. Only valid when `value` is an `Int`.
- `prefix` *(Bool, optional)*: prepend `0b`, `0o`, or `0x` for radix 2, 8, or 16. A negative number's `-` precedes the marker.

**Returns:** the string form as a `String`.
**Raises:** a non-`Int` value with a `radix`, an out-of-range `radix`, or `prefix == true` for a radix without a literal marker.

```tigr
print(str(42));            // => 42
print(str([1, 2, 3]));     // => [1, 2, 3]
print(str(255, 16));       // => ff
print(str(255, 16, true)); // => 0xff
print(str(-10, 2, true));  // => -0b1010
```

### `num(value) -> Number | null`

Parses a `String` into a number, or passes a number straight through. The string may parse to an `Int` or a `Float` depending on its form.

- `value` *(String or Number)*: the value to convert.

**Returns:** the parsed `Int` or `Float`, or `null` if a `String` does not parse.

```tigr
print(num('42'));    // => 42
print(num('3.5'));   // => 3.5
print(num(7));       // => 7
print(num('hello')); // => null
```

### `int(value) -> Int`

Converts a value to an `Int`, truncating toward zero. A `Float` drops its fractional part, and a numeric `String` is parsed then truncated.

- `value` *(value)*: the value to convert.

**Returns:** the value as an `Int`.

```tigr
print(int(3.9));   // => 3
print(int(-3.9));  // => -3
print(int('17'));  // => 17
```

### `float(value) -> Float`

Converts a value to a `Float`. An `Int` is widened, and a numeric `String` is parsed.

- `value` *(value)*: the value to convert.

**Returns:** the value as a `Float`.

```tigr
print(float(7));     // => 7.0
print(float('2.5')); // => 2.5
```

### `bool(value) -> Bool`

Applies the language's truthiness rule. Only `false` and `null` are falsy; everything else is truthy, including `0`, `0.0`, an empty `String`, and an empty collection. To test whether a collection is empty, compare its length instead (`#c == 0`).

- `value` *(value)*: the value to test.

**Returns:** `true` or `false`.

```tigr
print(bool(0));      // => true
print(bool(''));     // => true
print(bool([1]));    // => true
print(bool('text')); // => true
```

### `type(value) -> String`

Names the value's type. The result is one of `'int'`, `'float'`, `'string'`, `'bool'`, `'null'`, `'array'`, `'object'`, `'range'`, or `'function'`. Both user closures and native built-ins report `'function'`.

- `value` *(value)*: the value to inspect.

**Returns:** the type name as a `String`.

```tigr
print(type(42));         // => int
print(type(3.5));        // => float
print(type('hi'));       // => string
print(type([1, 2]));     // => array
print(type(fn(x) { x })); // => function
```

### `floor(value) -> Int`

Rounds a number down to the nearest integer (toward negative infinity).

- `value` *(Number)*: the number to round.

**Returns:** the rounded value as an `Int`.

```tigr
print(floor(2.7));  // => 2
print(floor(-2.1)); // => -3
```

### `ceil(value) -> Int`

Rounds a number up to the nearest integer (toward positive infinity).

- `value` *(Number)*: the number to round.

**Returns:** the rounded value as an `Int`.

```tigr
print(ceil(2.1));  // => 3
print(ceil(-2.7)); // => -2
```

### `rand() -> Float`

Returns a uniformly distributed random `Float` in the half-open range `[0, 1)`. The stream can be seeded for reproducible runs with `Random.seed` from the [`Random`](random.md) module.

**Returns:** a `Float` in `[0, 1)`.

```tigr
r := rand();
print(r >= 0.0 && r < 1.0);  // => true
```

### `gc() -> Object`

Returns a read-only snapshot of the tracing garbage collector's state. Collection runs automatically; `gc()` only observes it.

**Returns:** an `Object` `${live, collections, allocated, freed}`. `live` is the current managed-object count, `collections` is the number of collections run so far, and `allocated` / `freed` are lifetime totals.

```tigr
snap := gc();
print(snap.live >= 0);         // => true
print(type(snap.collections)); // => int
```

### `join(handle) -> value`

Waits for a concurrent computation to finish and returns its result. `join` accepts either kind of handle:

- A **`Task`** from `spawn` — `join` blocks the OS thread until the actor finishes. The result is deep-copied into the calling actor's heap. If the actor ended in an error, `join` re-raises it so the caller can `try`/`catch` it. Joining the same task twice raises.
- A **green-thread handle** from `go` — `join` *cooperatively* yields the caller until the coroutine returns, letting the scheduler run the other coroutines meanwhile, then evaluates to the coroutine's return value (no copy — coroutines share a heap). A green-thread handle may be joined any number of times. `join` from inside a generator body, or one that would block with no other coroutine able to run, raises rather than hanging.

`spawn`/`go` and `join` are a pair: one starts a computation, `join` waits for it; neither needs an import.

- `handle` *(Task or green thread)*: the handle of the computation to wait for.

**Returns:** the value the actor's or coroutine's function evaluated to.
**Raises:** for a `Task`, whatever the actor raised — a `raise`d value verbatim, or a built-in runtime error as `${kind, message, trace, worker}` with `worker` true; joining the same task twice raises a string error. (An uncaught `raise` in a `go` coroutine aborts the whole actor, so it never surfaces at the coroutine's `join`.)

```tigr
t := spawn fn() { 6 * 7 };
print(join(t));   // => 42

tasks := for[] (i, 1..=4) { spawn fn() { i * i } };
print(for[] (t, tasks) { join(t) });  // => [1, 4, 9, 16]

h := go fn() { 1 + 2 + 3 };
print(join(h));   // => 6
```

## See also

- [LANGUAGE.md §13.1](../../LANGUAGE.md#131-required-built-ins-for-v02): the authoritative spec for the built-ins
- [Concurrency](../language/concurrency.md): `spawn`, `join`, channels, and `select`
- [Math](math.md): rounding, trigonometry, and the rest of the numeric toolkit
- [Random](random.md): a seedable PRNG that backs `rand`
