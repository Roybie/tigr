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

## Name resolution

The resolved string has two flavors, and which one applies depends on its shape.

**Bare names** contain no `/`, `\`, or `.`. They resolve against the modules built into tigr. Some of these are written in tigr itself: `Array`, `Iter`, `String`, `Math`, `Object`, `Map`, `Set`, `Test`, `Channel`, `Url`, and `Http`. Others are native modules implemented in the host: `IO`, `Os`, `Time`, and `Net`. An unknown bare name raises a catchable error.

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
