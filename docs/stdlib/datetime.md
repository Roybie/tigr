# `DateTime`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#datetime-v06)

`DateTime` converts between epoch milliseconds and calendar dates, available without an `import`. Everything is UTC: there is no timezone support. A *components object* is `${year, month, day, hour, minute, second, ms, weekday, yearday}`, where `month` is 1 to 12, `weekday` is 0 for Sunday through 6 for Saturday, and `yearday` is the 1-based day of the year. To read the actual wall clock as a raw offset, see [`Time`](time.md).

## Functions

| Function | Summary |
|----------|---------|
| [`now() -> Object`](#now---object) | Reads the current UTC time and breaks it into calendar components. |
| [`from_ms(ms) -> Object`](#from_msms---object) | Converts an epoch-milliseconds value into a components object. |
| [`to_ms(obj) -> Int`](#to_msobj---int) | Converts a components object back into epoch milliseconds. |
| [`format(ms, fmt) -> String`](#formatms-fmt---string) | Renders an epoch-milliseconds value as text. |
| [`parse(str) -> Int`](#parsestr---int) | Parses an ISO-8601 datetime string into epoch milliseconds. |


### `now() -> Object`

Reads the current UTC time and breaks it into calendar components.

**Returns:** a components `Object` for the current moment.
**Raises:** a string error if the system clock is set before the epoch.

```tigr
today := DateTime.now();
print(today.year >= 2024);   // => true
```

### `from_ms(ms) -> Object`

Converts an epoch-milliseconds value into a components object.

- `ms` *(Int)*: milliseconds since the UNIX epoch.

**Returns:** a components `Object` for that instant.
**Raises:** a string error if `ms` is not an `Int`.

```tigr
d := DateTime.from_ms(1700000000000);
print(d.year);      // => 2023
print(d.month);     // => 11
print(d.day);       // => 14
print(d.weekday);   // => 2
print(d.yearday);   // => 318
```

### `to_ms(obj) -> Int`

Converts a components object back into epoch milliseconds. Missing fields take defaults: `year` is 1970, `month` and `day` are 1, and the rest are 0.

- `obj` *(Object)*: a components object. Each present field must be an `Int`.

**Returns:** the epoch milliseconds as an `Int`.
**Raises:** a string error if `obj` is not an `Object`, or if a present field is not an `Int`.

```tigr
print(DateTime.to_ms(${year: 2021, month: 1, day: 1}));   // => 1609459200000

d := DateTime.from_ms(1700000000000);
print(DateTime.to_ms(d) == 1700000000000);   // => true
```

### `format(ms, fmt) -> String`

Renders an epoch-milliseconds value as text. Note that `format` takes milliseconds, not a components object: pass a `Time.now_ms()`, `to_ms`, or `parse` result. In `fmt`, a `%` directive is substituted and any other text is copied literally. The directives are `%Y` (4-digit year), `%m` (2-digit month), `%d` (2-digit day), `%H` (2-digit hour), `%M` (2-digit minute), `%S` (2-digit second), `%j` (3-digit day of year), and `%%` for a literal percent sign.

- `ms` *(Int)*: milliseconds since the UNIX epoch.
- `fmt` *(String)*: the format string.

**Returns:** the rendered date as a `String`.
**Raises:** a string error if `ms` is not an `Int`, `fmt` is not a `String`, `fmt` uses an unknown directive, or `fmt` ends with a trailing `%`.

```tigr
print(DateTime.format(1700000000000, '%Y-%m-%d %H:%M:%S'));   // => 2023-11-14 22:13:20
print(DateTime.format(1700000000000, 'day %j of %Y'));        // => day 318 of 2023
```

### `parse(str) -> Int`

Parses an ISO-8601 datetime string into epoch milliseconds. The string is `YYYY-MM-DD`, optionally followed by either a `T` or a space and then `HH:MM:SS`, with an optional `.fff` fractional-second part.

- `str` *(String)*: the ISO-8601 datetime to parse. Surrounding whitespace is trimmed.

**Returns:** the epoch milliseconds as an `Int`.
**Raises:** a string error if `str` is not a `String` or is not a valid ISO-8601 datetime.

```tigr
print(DateTime.parse('2021-06-15'));            // => 1623715200000
print(DateTime.parse('2021-06-15T12:30:00'));   // => 1623760200000

ms := DateTime.parse('2021-06-15T12:30:00');
print(DateTime.format(ms, '%Y/%m/%d'));         // => 2021/06/15
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#datetime-v06): the authoritative spec for `DateTime`
- [Time](time.md): read the wall clock as a raw epoch offset
- [Os](os.md): process and environment access
