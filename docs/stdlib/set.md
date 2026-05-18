# `Set`

> Pure-tigr source module, `stdlib/Set.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#set-v09)

A `Set` is an insertion-ordered collection of unique values. Elements may be any `null`, `Bool`, `Int`, or `String` value, the same restriction that applies to `Map` keys; a `Float` or a collection element raises `invalid_key_type`.

Three pieces of syntax work directly on a set. `s[x]` tests membership and returns `true` or `false`. `#s` is the element count. `for (x, s) { ... }` iterates the elements in insertion order. Writing `s[x] = ...` is an error; change a set with `add` and `delete`.

Every operation is O(1) amortized, except the set-algebra functions, which are O(n).

```tigr
Set := import 'Set';

s := Set.new([1, 2, 2, 3]);
print(s[2]);   // => true
print(#s);     // => 3
```

## Functions

### `new(array?) -> Set`

Creates a set. With no argument it is empty; with an array, the set is built from the array's elements and duplicates collapse.

- `array` *(Array, optional)*: initial elements.

**Returns:** a new `Set`.
**Raises:** `invalid_key_type` if an element is a `Float` or a collection.

```tigr
Set := import 'Set';

print(Set.items(Set.new()));            // => []
print(Set.items(Set.new([3, 1, 1])));   // => [3, 1]
```

### `add(s, x) -> Set`

Inserts `x` into `s` in place. Adding a value already present is a no-op.

- `s` *(Set)*: the set to mutate.
- `x` *(value)*: the element to insert.

**Returns:** `s`, the same set, so calls can be chained.
**Raises:** `invalid_key_type` if `x` is a `Float` or a collection.

```tigr
Set := import 'Set';

s := Set.new();
Set.add(s, 'a');
Set.add(s, 'a');
print(#s);   // => 1
```

### `has(s, x) -> Bool`

Tests whether `x` is a member of `s`. This is the same test as `s[x]`.

- `s` *(Set)*: the set to query.
- `x` *(value)*: the element to look for.

**Returns:** `true` if `x` is a member, otherwise `false`.

```tigr
Set := import 'Set';

s := Set.new([10, 20]);
print(Set.has(s, 10));   // => true
print(Set.has(s, 99));   // => false
```

### `delete(s, x) -> Bool`

Removes `x` from `s` in place.

- `s` *(Set)*: the set to mutate.
- `x` *(value)*: the element to remove.

**Returns:** `true` if `x` was present and removed, `false` if it was not there.

```tigr
Set := import 'Set';

s := Set.new([1, 2]);
print(Set.delete(s, 2));   // => true
print(Set.delete(s, 2));   // => false
```

### `items(s) -> Array`

Collects the set's elements into an array.

- `s` *(Set)*: the set to read.

**Returns:** an `Array` of the elements in insertion order.

```tigr
Set := import 'Set';

print(Set.items(Set.new([3, 1, 2])));   // => [3, 1, 2]
```

### `size(s) -> Int`

Counts the elements. This is the same value as `#s`.

- `s` *(Set)*: the set to read.

**Returns:** the element count as an `Int`.

```tigr
Set := import 'Set';

print(Set.size(Set.new([1, 2, 3])));   // => 3
```

### `clear(s) -> Set`

Removes every element from `s` in place.

- `s` *(Set)*: the set to empty.

**Returns:** `s`, now empty.

```tigr
Set := import 'Set';

s := Set.new([1, 2, 3]);
Set.clear(s);
print(#s);   // => 0
```

### `union(a, b) -> Set`

Builds the set of every element in either `a` or `b`.

- `a` *(Set)*: the first set.
- `b` *(Set)*: the second set.

**Returns:** a fresh `Set`. `a` and `b` are not modified.

```tigr
Set := import 'Set';

a := Set.new([1, 2]);
b := Set.new([2, 3]);
print(Set.items(Set.union(a, b)));   // => [1, 2, 3]
```

### `intersection(a, b) -> Set`

Builds the set of elements found in both `a` and `b`.

- `a` *(Set)*: the first set.
- `b` *(Set)*: the second set.

**Returns:** a fresh `Set`. `a` and `b` are not modified.

```tigr
Set := import 'Set';

a := Set.new([1, 2, 3]);
b := Set.new([2, 3, 4]);
print(Set.items(Set.intersection(a, b)));   // => [2, 3]
```

### `difference(a, b) -> Set`

Builds the set of `a`'s elements that are not in `b`.

- `a` *(Set)*: the set to take elements from.
- `b` *(Set)*: the set of elements to exclude.

**Returns:** a fresh `Set`. `a` and `b` are not modified.

```tigr
Set := import 'Set';

a := Set.new([1, 2, 3]);
b := Set.new([2, 3, 4]);
print(Set.items(Set.difference(a, b)));   // => [1]
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#set-v09): the authoritative spec for `Set`
- [Map](map.md): the key/value collection with the same key rules
- [Array](array.md): for ordered collections that allow duplicates
