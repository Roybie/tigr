# `Object`

> Pure-tigr source module, `stdlib/Object.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#object-v06)

An object is the built-in string-keyed record, written with the `${...}` literal. The `Object` module adds utilities for inspecting and transforming objects on top of the field access, indexing, and `for` syntax the language already gives you. Import it as `Object := import 'Object'`.

`map` and `filter` take a callback, a function value you supply that the module calls for you. The callback is invoked as `callback(value, key, whole_object)`, mirroring `Array`'s element/index/array order. Tigr drops extra arguments, so a one-parameter `fn(v)` works just as well as `fn(v, k, obj)`. The transforming functions (`merge`, `map`, `filter`, `from_entries`) return fresh objects and never mutate their input.

```tigr
Object := import 'Object';

print(Object.entries(${a: 1, b: 2}));   // => [[a, 1], [b, 2]]
```

## Functions

### `keys(obj) -> Array`

Collects the keys of `obj` in insertion order.

- `obj` *(Object)*: the object to read.

**Returns:** an `Array<String>` of the keys.

```tigr
Object := import 'Object';

print(Object.keys(${a: 1, b: 2, c: 3}));   // => [a, b, c]
```

### `values(obj) -> Array`

Collects the values of `obj` in insertion order.

- `obj` *(Object)*: the object to read.

**Returns:** an `Array` of the values.

```tigr
Object := import 'Object';

print(Object.values(${a: 1, b: 2, c: 3}));   // => [1, 2, 3]
```

### `entries(obj) -> Array`

Collects the `[key, value]` pairs of `obj` in insertion order.

- `obj` *(Object)*: the object to read.

**Returns:** an `Array` of `[key, value]` pairs.

```tigr
Object := import 'Object';

print(Object.entries(${a: 1, b: 2}));   // => [[a, 1], [b, 2]]
```

### `from_entries(pairs) -> Object`

Builds an object from an array of `[key, value]` pairs. This is the inverse of `entries`. On a duplicate key, the later pair wins.

- `pairs` *(Array)*: an array of `[key, value]` pairs.

**Returns:** a new `Object`.

```tigr
Object := import 'Object';

print(Object.from_entries([['a', 1], ['b', 2]]));   // => ${a: 1, b: 2}
```

### `has(obj, key) -> Bool`

Tests whether `obj` has `key`. Unlike `obj[key]`, this tells a missing key apart from a present `null` value. O(1).

- `obj` *(Object)*: the object to query.
- `key` *(String)*: the key to look for.

**Returns:** `true` if `key` is present, otherwise `false`.

```tigr
Object := import 'Object';

print(Object.has(${a: null}, 'a'));   // => true
print(Object.has(${a: null}, 'b'));   // => false
```

### `merge(a, b) -> Object`

Shallow-merges two objects into a fresh one. On a key collision, `b` wins.

- `a` *(Object)*: the base object.
- `b` *(Object)*: the object whose entries override `a`.

**Returns:** a new `Object`. Neither `a` nor `b` is modified.

```tigr
Object := import 'Object';

print(Object.merge(${a: 1, b: 2}, ${b: 9, c: 3}));   // => ${a: 1, b: 9, c: 3}
```

### `map(obj, func) -> Object`

Transforms every value through `func`, keeping the keys.

- `obj` *(Object)*: the object to read.
- `func` *(Fn)*: called as `func(value, key, whole_object)`, returns the new value.

**Returns:** a new `Object` with the mapped values. The input is not modified.

```tigr
Object := import 'Object';

print(Object.map(${a: 1, b: 2}, fn(v) { v * 10 }));   // => ${a: 10, b: 20}
```

### `filter(obj, pred) -> Object`

Keeps the entries for which `pred` returns a truthy value.

- `obj` *(Object)*: the object to read.
- `pred` *(Fn)*: called as `pred(value, key, whole_object)`.

**Returns:** a new `Object` of the kept entries. The input is not modified.

```tigr
Object := import 'Object';

print(Object.filter(${a: 1, b: 2, c: 3}, fn(v) { v > 1 }));   // => ${b: 2, c: 3}
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#object-v06): the authoritative spec for `Object`
- [Array](array.md): the same callback style for ordered collections
- [Map](map.md): the key/value collection that allows non-string keys
