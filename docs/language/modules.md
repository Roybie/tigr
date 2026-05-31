# Modules / imports

Spec: [LANGUAGE.md §12](../../LANGUAGE.md#12-modules--imports)

A module is just a tigr file or a built-in module, and `import` brings its value into the current program. There is no `export` keyword: a module hands back whatever its final expression evaluates to, which is usually an object full of functions.

## `import`

`import` takes an expression, evaluates it, and expects the result to be a string path. The whole expression up to the end of the statement is consumed, so string concatenation needs no parentheses.

```tigr
Math := import 'Math';
print(Math.sqrt(144));   // => 12.0
print(Math.max(3, 9, 1)); // => 9
```

```
mod := import './plugins/' + name;   // path built from an expression
```

`import` returns the imported module's final value, so the `:=` on the left binds whatever the module produced.

## Ambient stdlib (no import needed)

Every built-in module is also available without writing `import` at all. A bare `Math`, `String`, `JSON`, and so on just resolves:

```tigr
print(Math.sqrt(144));            // => 12.0
print(String.upper("hi"));        // => "HI"
print(JSON.stringify([1, 2, 3])); // => "[1,2,3]"
```

The rule is simple: a capitalized stdlib module is always in scope; anything you wrote yourself or pulled in as a third-party file still needs an explicit `import` by path. Writing `Math := import 'Math'` keeps working, and a local binding of the same name shadows the ambient module, so you can still name a variable `Map` or hand your own object to `String` if you want to.

Resolution stays lazy. A module is only built the first time you actually reach it, so a program that never mentions `Net` never opens the networking machinery, exactly as if you had never imported it.

## Name resolution

The resolved string has two flavors, and which one applies depends on its shape.

**Bare names** contain no `/`, `\`, or `.`. They resolve against the modules built into tigr, the same set that is [ambient](#ambient-stdlib-no-import-needed): the tigr-written `Array`, `Iter`, `String`, `Math`, `Object`, `Map`, `Set`, `Test`, `Channel`, `LocalChannel`, `Url`, and `Http`, and the native `IO`, `Os`, `Time`, `Path`, `DateTime`, `Random`, `JSON`, `Bytes`, `BigInt`, and `Net`. Writing `import 'Name'` is just the explicit form of reaching one by name. An unknown bare name raises a catchable error. (When tigr is embedded in a host application, the host can register more bare-name modules; see the embedding API.)

**Path-shaped strings** contain a `/`, `\`, or `.`. They resolve relative to the directory of the importing file. The `.tg` extension is appended automatically when absent, so `import './lib/util'` and `import './lib/util.tg'` are the same. A missing file raises a catchable `import_failed` error, and a path that does not evaluate to a string raises a `type_mismatch` error.

A user module is typically a single object literal:

```
// lib/util.tg
${
    map:    fn(arr, f) { for[] (x, arr) { f(x) } },
    filter: fn(arr, f) { for[] (x, arr) { if !f(x) { continue }; x } },
}
```

The importing file then reads functions off the returned object:

```
util := import './lib/util';
util.map([1, 2, 3], fn(x) { x * 10 });   // [10, 20, 30]
```

## Caching

Each path is evaluated at most once per run. The first `import` of a path runs the module file and caches its value, and every later `import` of the same path returns that cached value without re-running anything. Bare-name modules are cached the same way.

A consequence worth knowing: two imports of the same file yield the same underlying object reference, so mutating the module object through one import is visible through the other.

A circular import, where `a.tg` imports `b.tg` which imports `a.tg`, raises a catchable error instead of looping forever.

## See also

- [Errors](errors.md): `import_failed` and the other catchable error kinds
- [stdlib overview](../stdlib/README.md): the bundled modules you can import by bare name
- [LANGUAGE.md §12](../../LANGUAGE.md#12-modules--imports): the authoritative spec
