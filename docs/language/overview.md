# Language overview

Spec: [LANGUAGE.md §1](../../LANGUAGE.md#1-philosophy), [§3](../../LANGUAGE.md#3-types), [§5](../../LANGUAGE.md#5-truthiness)

Tigr is a small, expression-oriented language run by a bytecode VM. This page covers the three ideas you need before anything else makes sense: everything is an expression, the set of value types, and what counts as true.

## Everything is an expression

Every construct in Tigr produces a value. There are no statements. A block evaluates to its last expression (`null` if the expression is terminated with `;`), an `if` evaluates to whichever branch ran, a function evaluates to whatever its body ends with. The common case needs no `return`.

```tigr
a := { 1; 2; 3 }; // => a == 3
b := { 1; 2; 3; }; // => b == null

x := if 5 > 3 { 'big' } else { 'small' };
print(x);   // => big

sum := fn(a, b) { a + b };   // the body's last expression is the result
print(sum(2, 3));            // => 5
```

Loops are expressions too. A plain `for` evaluates to its last iteration's body value; `for[]` collects every body value into an array.

```tigr
total := for (n, 1..=10) { n };
print(total);   // => 10

all := for[] (n, 1..=10) { n };
print(all);     // => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
```

Because every piece composes, you can drop a control-flow construct straight into a larger expression:

```tigr
loud := true;
greeting := 'Hello, ' + (if loud { 'WORLD' } else { 'world' }) + '!';
print(greeting);   // => Hello, WORLD!
```

## Types

Tigr has a fixed set of built-in value types.

| Type       | Examples                                       | Notes                                               |
|------------|------------------------------------------------|-----------------------------------------------------|
| `Int`      | `42`, `0xFF`, `0b1010`, `0o755`, `1_000_000`   | 64-bit signed; hex/bin/oct literals, `_` separators |
| `Float`    | `3.14`, `.5`, `1e6`, `2.5e-3`                  | 64-bit IEEE-754; scientific notation is always `Float` |
| `String`   | `'hello'`, `'name = {n}'`, `"raw"`             | `'…'` interpolates, `"…"` is raw; UTF-8             |
| `Bool`     | `true`, `false`                                |                                                     |
| `Null`     | `null`                                         | The absence-of-value type                           |
| `Array`    | `[1, 'two', true]`                             | Heterogeneous, reference type                       |
| `Object`   | `${name: 'a', age: 1}`                         | String-keyed record, reference type                 |
| `Map`      | `Map.new()`                                    | Arbitrary-keyed dictionary, reference type          |
| `Set`      | `Set.new([1, 2, 3])`                           | Collection of unique values, reference type         |
| `Bytes`    | `Bytes.from_hex('deadbeef')`                   | Mutable byte buffer for binary data, reference type |
| `BigInt`   | `BigInt.new('123…')`                           | Arbitrary-precision integer; immutable value type   |
| `Range`    | `0..10`, `0..=10`, `10..0:-1`                  | First-class lazy iterable                           |
| `Function` | `fn(x) { x * 2 }`                              | Closes over its lexical environment                 |
| `Channel`  | `Channel.new()`                                | Message-passing conduit between actors              |
| `Task`     | `spawn fn() { … }`                             | Handle to a spawned actor's result                  |
| `Socket`   | `Net.connect('host', 80)`                      | TCP / UDP / TLS network socket                      |

`Int` and `Float` together are called Number. Mixed-number arithmetic follows the rules in [Expressions](expressions.md).

In numeric literals, underscores are allowed only between digits: `_5`, `5_`, `5__5`, and `0x_FF` are all rejected. A trailing dot like `5.` lexes as the `Int` `5` followed by a dot, so `5.method` style member access still parses.

`Array`, `Object`, `Map`, `Set`, and `Bytes` are reference types. Passing one to a function or binding it to a new name shares the same underlying value rather than copying it. The other types are value types.

The built-in `type` function reports a value's type as a lowercase string:

```tigr
print(type(42));        // => int
print(type(3.14));      // => float
print(type('hi'));      // => string
print(type([1, 2]));    // => array
print(type(${}));       // => object
print(type(0..5));      // => range
print(type(fn() {}));   // => function
```

## Truthiness

Tigr uses the Lua rule: only `false` and `null` are falsy. Everything else is truthy, including `0`, `0.0`, `''`, `[]`, `${}`, empty ranges, empty maps and sets, and every function. Truthiness tests one thing only: is this value present and not `false`.

To check whether a collection or string is empty, compare its length with `#x == 0`. To check a number for zero, compare it with `n == 0`. Do not lean on truthiness for either.

`!x`, and the boolean contexts of `if`, `while`, `&&`, and `||`, all use this rule.

`&&` and `||` short-circuit and return the operand value that decided the result. They do not coerce to `Bool`:

```tigr
print(0 || 'fallback');      // => 0          (0 is truthy)
print(null || 'fallback');   // => fallback   (null is falsy)
print('a' && 'b');           // => b
print(false && 'b');         // => false
```

This is why `x || default` substitutes a default only when `x` is `null` or `false`, leaving a legitimate `0` or empty value alone.

## See also

- [Bindings](bindings.md): declaring and assigning names, blocks, and scopes
- [Expressions](expressions.md): arithmetic, comparison, bitwise operators, and the pipe
- [Collections](collections.md): arrays, objects, and ranges in depth
- [Strings](strings.md): the two string forms and their operators
- [Control flow](control-flow.md): `if`, `while`, `for`, `match`, and friends as expressions
- [LANGUAGE.md §1](../../LANGUAGE.md#1-philosophy), [§3](../../LANGUAGE.md#3-types), [§5](../../LANGUAGE.md#5-truthiness): the authoritative spec
