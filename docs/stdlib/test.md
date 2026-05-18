# `Test`

> Pure-tigr source module, `stdlib/Test.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#test-v09)

`Test` is a small test framework written in tigr itself. Tests are plain data: `case(name, func)` packages an unrun test, and `suite(name, cases)` runs a list of them, prints a PASS/FAIL line per case and a tally, then returns a result object. Import it as `Test := import 'Test'`.

The assertions (`assert`, `assert_eq`, `assert_ne`, `assert_raises`, `fail`) `raise` on failure, so they work standalone anywhere, not only inside a suite. A `suite` simply catches the raise and records it. The result object is `${name, passed, failed, total, failures}`, where `failures` is an array of `${name, error}`.

The `tigr test` CLI subcommand discovers `*_test.tg` files (and every `.tg` file under any `tests/` directory), runs each, sums the `suite` result objects a file's final expression yields, and exits non-zero if any test failed. A test file's final expression should be a `suite(...)` result, or an array of them for several suites.

```tigr
Test := import 'Test';

Test.suite('arithmetic', [
    Test.case('adds', fn() { Test.assert_eq(1 + 1, 2) }),
    Test.case('div zero raises', fn() {
        Test.assert_raises(fn() { 1 / 0 }, 'div_by_zero')
    }),
])
// => suite arithmetic
// =>   PASS  adds
// =>   PASS  div zero raises
// =>   2 passed, 0 failed
```

## Functions

### `assert(cond, msg?) -> Bool`

Raises `msg` unless `cond` is truthy.

- `cond` *(value)*: the condition to check.
- `msg` *(String, optional)*: the message to raise on failure. Defaults to `'assertion failed'`.

**Returns:** `true` when `cond` holds.
**Raises:** `msg` when `cond` is falsy.

```tigr
Test := import 'Test';

check := fn() {
    Test.assert(2 > 1);
    Test.assert(3 > 1, 'three beats one')
};
print(check());         // => true
```

### `assert_eq(actual, expected, msg?) -> Bool`

Raises unless `actual == expected`. The comparison is structural for arrays and objects.

- `actual` *(value)*: the value produced by the code under test.
- `expected` *(value)*: the value it should equal.
- `msg` *(String, optional)*: a prefix for the failure message. When omitted, only the expected/actual detail is shown.

**Returns:** `true` when the values are equal.
**Raises:** a message showing both values when they differ.

```tigr
Test := import 'Test';

print(try { Test.assert_eq([1, 2], [1, 3]) } catch (e) { e });
// => expected [1, 3], got [1, 2]
```

### `assert_ne(a, b, msg?) -> Bool`

Raises unless `a != b`.

- `a` *(value)*: the first value.
- `b` *(value)*: the second value.
- `msg` *(String, optional)*: a prefix for the failure message.

**Returns:** `true` when the values differ.
**Raises:** a message naming the shared value when they are equal.

```tigr
Test := import 'Test';

print(Test.assert_ne('a', 'b'));                    // => true
print(try { Test.assert_ne(5, 5) } catch (e) { e });
// => expected values to differ, both were 5
```

### `assert_raises(thunk, kind?) -> value`

Runs `thunk` and raises unless `thunk` itself raised. When `kind` is given, the raised value must match it: for a reified built-in error its `kind` field, otherwise the raised value itself.

- `thunk` *(Function)*: a zero-argument function expected to raise.
- `kind` *(value, optional)*: the error kind or raised value the failure must match. When omitted, any raise passes.

**Returns:** the caught error, so the caller can assert further on it.
**Raises:** a message when `thunk` did not raise, or when it raised something other than `kind`.

```tigr
Test := import 'Test';

err := Test.assert_raises(fn() { raise ${kind: 'bad_input', message: 'no'} }, 'bad_input');
print(err.message);     // => no
```

### `fail(msg?) -> value`

Raises unconditionally. Use it to mark an unreachable branch or an unfinished test.

- `msg` *(String, optional)*: the message to raise. Defaults to `'explicit failure'`.

**Returns:** never returns normally.
**Raises:** `msg` every time.

```tigr
Test := import 'Test';

print(try { Test.fail('not ready yet') } catch (e) { e });
// => not ready yet
```

### `case(name, func) -> Object`

Packages a named, unrun test. `func` does nothing until a suite drives it; it should call the assertions and raise on failure.

- `name` *(String)*: the test's name, shown in the PASS/FAIL line.
- `func` *(Function)*: a zero-argument function holding the test body.

**Returns:** the descriptor `${name, func}`.

```tigr
Test := import 'Test';

c := Test.case('pythagoras', fn() { Test.assert_eq(3*3 + 4*4, 5*5) });
print(c.name);          // => pythagoras
print(c.func());        // => true
```

### `suite(name, cases) -> Object`

Runs an array of `case` descriptors. Prints a header, a `PASS` or `FAIL` line per case, and a tally.

- `name` *(String)*: the suite's name.
- `cases` *(Array)*: an array of descriptors built with `case`.

**Returns:** the result object `${name, passed, failed, total, failures}`, where `failures` is an array of `${name, error}` for the cases that failed.

```tigr
Test := import 'Test';

result := Test.suite('mixed', [
    Test.case('ok', fn() { Test.assert(true) }),
    Test.case('bad', fn() { Test.assert_eq(1, 2) }),
]);
${failed: result.failed, who: result.failures[0].name}
// => suite mixed
// =>   PASS  ok
// =>   FAIL  bad  —  expected 2, got 1
// =>   1 passed, 1 failed
// => then the final expression is ${failed: 1, who: bad}
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#test-v09): the authoritative spec for `Test`
- [Errors](../language/errors.md): `try`, `catch`, and `raise`, which the assertions build on
- [Control flow](../language/control-flow.md): `match` and the other expression forms
