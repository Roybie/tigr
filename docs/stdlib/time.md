# `Time`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#time)

`Time` provides wall-clock access, imported with `import 'Time'`. It reads the current time as an offset from the UNIX epoch and pauses the running thread. The two clock readings, `now_ms` and `now_ns`, are most useful for measuring how long a piece of code takes: read the clock before and after, then subtract. For calendar dates instead of raw offsets, see [`DateTime`](datetime.md).

## Functions

| Function | Summary |
|----------|---------|
| [`now_ms() -> Int`](#now_ms---int) | Reads the current wall-clock time as milliseconds since the UNIX epoch (1970-01-01 UTC). |
| [`now_ns() -> Int`](#now_ns---int) | Reads the current wall-clock time as nanoseconds since the UNIX epoch. |
| [`sleep_ms(n) -> null`](#sleep_msn---null) | Blocks the current thread for `n` milliseconds. |


### `now_ms() -> Int`

Reads the current wall-clock time as milliseconds since the UNIX epoch (1970-01-01 UTC).

**Returns:** the elapsed milliseconds as an `Int`.
**Raises:** a string error if the system clock is set before the epoch.

```tigr
Time := import 'Time';

start := Time.now_ms();
Time.sleep_ms(10);
elapsed := Time.now_ms() - start;
print(elapsed >= 10);   // => true
```

### `now_ns() -> Int`

Reads the current wall-clock time as nanoseconds since the UNIX epoch. This is the finer-grained reading, useful for timing short operations.

**Returns:** the elapsed nanoseconds as an `Int`.
**Raises:** a string error if the system clock is set before the epoch.

```tigr
Time := import 'Time';

t := Time.now_ns();
print(type(t));   // => int
```

### `sleep_ms(n) -> null`

Blocks the current thread for `n` milliseconds.

- `n` *(Int)*: the number of milliseconds to sleep. Must not be negative.

**Returns:** `null`.
**Raises:** a string error if `n` is negative or not an `Int`.

```tigr
Time := import 'Time';

print(Time.sleep_ms(0));   // => null
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#time): the authoritative spec for `Time`
- [DateTime](datetime.md): turn epoch milliseconds into calendar fields
- [Os](os.md): process and environment access
