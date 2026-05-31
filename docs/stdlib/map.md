# `Map`

> Pure-tigr source module, `stdlib/Map.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#map-v09)

A `Map` is an arbitrary-keyed, insertion-ordered dictionary. Unlike `Object`, whose keys are strings only, a `Map` key may be any `null`, `Bool`, `Int`, or `String` value; a `Float` or a collection key raises `invalid_key_type`. `type(m)` is `'map'`, and a `Map` is not JSON-serializable. It is ambient, so a bare module name works without an `import`.

Four pieces of syntax work directly on a map. `m[k]` reads an entry and returns `null` when the key is absent. `m[k] = v` inserts or overwrites an entry. `#m` is the entry count. `for (k, v, m) { ... }` iterates entries in insertion order. The `get` and `set` functions below do the same work as the index syntax, so they are rarely needed directly.

Every operation is O(1) amortized, except `keys`, `values`, and `entries`, which build their result in O(n). The functions are thin re-exports of the native `_NativeMap` backend.

```tigr
m := Map.new();
m[1] = 'one';       // Int key
m['1'] = 'string';  // distinct String key
print(m[1]);        // => one
print(#m);          // => 2
```

## Functions

| Function | Summary |
|----------|---------|
| [`new(source?) -> Map`](#newsource---map) | Creates a map. |
| [`get(m, key) -> value`](#getm-key---value) | Reads the value stored under `key`. |
| [`set(m, key, value) -> Map`](#setm-key-value---map) | Inserts or overwrites the entry for `key` in place. |
| [`has(m, key) -> Bool`](#hasm-key---bool) | Tests whether `key` is present. |
| [`delete(m, key) -> Bool`](#deletem-key---bool) | Removes the entry for `key` in place. |
| [`keys(m) -> Array`](#keysm---array) | Collects the map's keys. |
| [`values(m) -> Array`](#valuesm---array) | Collects the map's values. |
| [`entries(m) -> Array`](#entriesm---array) | Collects the map's entries as `[key, value]` pairs. |
| [`size(m) -> Int`](#sizem---int) | Counts the entries. |
| [`clear(m) -> Map`](#clearm---map) | Removes every entry from `m` in place. |


### `new(source?) -> Map`

Creates a map. With no argument it is empty. With an `Object`, the new map copies that object's entries. With an array of `[key, value]` pairs, it builds an entry per pair.

- `source` *(Object or Array, optional)*: initial entries, either an object to copy or an array of pairs.

**Returns:** a new `Map`.
**Raises:** `invalid_key_type` if a key is a `Float` or a collection.

```tigr
print(Map.keys(Map.new(${a: 1, b: 2})));            // => [a, b]
print(Map.entries(Map.new([[1, 'x'], [2, 'y']])));  // => [[1, x], [2, y]]
```

### `get(m, key) -> value`

Reads the value stored under `key`. This is the same lookup as `m[key]`.

- `m` *(Map)*: the map to query.
- `key` *(value)*: the key to look up.

**Returns:** the entry's value, or `null` if the key is absent.

```tigr
m := Map.new();
Map.set(m, 'k', 10);
print(Map.get(m, 'k'));         // => 10
print(Map.get(m, 'missing'));   // => null
```

### `set(m, key, value) -> Map`

Inserts or overwrites the entry for `key` in place. This is the same write as `m[key] = value`.

- `m` *(Map)*: the map to mutate.
- `key` *(value)*: the key to write.
- `value` *(value)*: the value to store.

**Returns:** `m`, the same map, so calls can be chained.
**Raises:** `invalid_key_type` if `key` is a `Float` or a collection.

```tigr
m := Map.new();
Map.set(m, 'a', 1);
Map.set(m, 'a', 2);
print(m['a']);          // => 2
```

### `has(m, key) -> Bool`

Tests whether `key` is present. Unlike `m[key]`, this tells a missing key apart from a key whose stored value is `null`.

- `m` *(Map)*: the map to query.
- `key` *(value)*: the key to look for.

**Returns:** `true` if the key is present, otherwise `false`.

```tigr
m := Map.new(${name: 'ada'});
m['x'] = null;
print(Map.has(m, 'name'));      // => true
print(Map.has(m, 'x'));         // => true
print(Map.has(m, 'y'));         // => false
```

### `delete(m, key) -> Bool`

Removes the entry for `key` in place.

- `m` *(Map)*: the map to mutate.
- `key` *(value)*: the key to remove.

**Returns:** `true` if the key was present and removed, `false` if it was not there.

```tigr
m := Map.new([[1, 'a'], [2, 'b']]);
print(Map.delete(m, 1));        // => true
print(Map.delete(m, 1));        // => false
```

### `keys(m) -> Array`

Collects the map's keys.

- `m` *(Map)*: the map to read.

**Returns:** an `Array` of the keys in insertion order.

```tigr
m := Map.new([[3, 'c'], [1, 'a']]);
print(Map.keys(m));             // => [3, 1]
```

### `values(m) -> Array`

Collects the map's values.

- `m` *(Map)*: the map to read.

**Returns:** an `Array` of the values in insertion order.

```tigr
m := Map.new([[3, 'c'], [1, 'a']]);
print(Map.values(m));           // => [c, a]
```

### `entries(m) -> Array`

Collects the map's entries as `[key, value]` pairs.

- `m` *(Map)*: the map to read.

**Returns:** an `Array` of `[key, value]` pairs in insertion order.

```tigr
m := Map.new([[1, 'a'], [2, 'b']]);
print(Map.entries(m));          // => [[1, a], [2, b]]
```

### `size(m) -> Int`

Counts the entries. This is the same value as `#m`.

- `m` *(Map)*: the map to read.

**Returns:** the entry count as an `Int`.

```tigr
print(Map.size(Map.new([[1, 'a'], [2, 'b'], [3, 'c']])));   // => 3
```

### `clear(m) -> Map`

Removes every entry from `m` in place.

- `m` *(Map)*: the map to empty.

**Returns:** `m`, now empty.

```tigr
m := Map.new([[1, 'a'], [2, 'b']]);
Map.clear(m);
print(#m);              // => 0
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#map-v09): the authoritative spec for `Map`
- [Set](set.md): the collection of unique values with the same key rules
- [Control flow](../language/control-flow.md): the `for (k, v, m)` iteration form
