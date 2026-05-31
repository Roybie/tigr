# `Array`

> Pure-tigr source module, `stdlib/Array.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#array)

An array is the built-in ordered collection. The `Array` module adds utilities on top of the array literal, indexing, and `for` syntax that the language already gives you. It is ambient, so a bare module name works without an `import`. The in-place mutators (`push`, `extend`, `pop`, `shift`, `unshift`, `insert`, `remove`, `clear`) are backed by native code; everything else is pure tigr.

Several functions take a callback, a function value you supply that the module calls for you. Unless a function's description says otherwise, the callback is invoked as `callback(element, index, whole_array)`. Tigr drops extra arguments, so a one-parameter `fn(x)` works just as well as `fn(x, i, arr)`.

```tigr
print(Array.sum(Array.filter([1, 2, 3, 4, 5], fn(x) { x % 2 == 0 })));   // => 6
```

## Functions

| Function | Summary |
|----------|---------|
| [`push(arr, value) -> Array`](#pusharr-value---array) | Appends `value` to `arr` in place. |
| [`extend(arr, other) -> Array`](#extendarr-other---array) | Appends every element of `other` to `arr` in place. |
| [`pop(arr) -> value`](#poparr---value) | Removes and returns the last element of `arr` in place. |
| [`shift(arr) -> value`](#shiftarr---value) | Removes and returns the first element of `arr` in place. |
| [`unshift(arr, value) -> Array`](#unshiftarr-value---array) | Prepends `value` to the front of `arr` in place. |
| [`insert(arr, index, value) -> Array`](#insertarr-index-value---array) | Inserts `value` at `index` in place. |
| [`remove(arr, index, count?) -> value`](#removearr-index-count---value) | Removes elements from `arr` in place. |
| [`clear(arr) -> Array`](#cleararr---array) | Removes every element from `arr` in place. |
| [`create(len, func) -> Array`](#createlen-func---array) | Builds an array of length `len` where element `i` is `func(i)`. |
| [`concat(a, b) -> Array`](#concata-b---array) | Joins two arrays into one. |
| [`map(arr, func) -> Array`](#maparr-func---array) | Applies `func` to every element and collects the results. |
| [`filter(arr, pred) -> Array`](#filterarr-pred---array) | Keeps the elements for which `pred` returns a truthy value. |
| [`reduce(arr, func, seed) -> value`](#reducearr-func-seed---value) | Folds the array left to right into a single value. |
| [`flatten(arr) -> Array`](#flattenarr---array) | Concatenates one level of nested arrays. |
| [`reverse(arr) -> Array`](#reversearr---array) | Reverses the order of the elements. |
| [`index(arr, elem) -> Int`](#indexarr-elem---int) | Finds the first index whose element is `==` to `elem`. |
| [`find(arr, pred) -> value`](#findarr-pred---value) | Finds the first element for which `pred` is truthy. |
| [`find_index(arr, pred) -> Int`](#find_indexarr-pred---int) | Finds the index of the first element for which `pred` is truthy. |
| [`any(arr, pred) -> Bool`](#anyarr-pred---bool) | Tests whether `pred` holds for at least one element. |
| [`all(arr, pred) -> Bool`](#allarr-pred---bool) | Tests whether `pred` holds for every element. |
| [`head(arr, n) -> Array`](#headarr-n---array) | Takes the first `n` elements. |
| [`tail(arr, n) -> Array`](#tailarr-n---array) | Takes the last `n` elements. |
| [`take(arr, n) -> Array`](#takearr-n---array) | Takes the first `n` elements, clamping `n` to `[0, #arr]`. |
| [`drop(arr, n) -> Array`](#droparr-n---array) | Drops the first `n` elements and keeps the rest, clamping `n` to `[0, #arr]`. |
| [`slice(arr, start, end) -> Array`](#slicearr-start-end---array) | Takes the elements in the range `[start, end)`. |
| [`sum(arr) -> Number`](#sumarr---number) | Adds up the elements. |
| [`max_of(arr) -> value`](#max_ofarr---value) | Finds the largest element by `>` comparison. |
| [`min_of(arr) -> value`](#min_ofarr---value) | Finds the smallest element by `<` comparison. |
| [`uniq(arr) -> Array`](#uniqarr---array) | Keeps the first occurrence of each distinct element, preserving order. |
| [`zip(a, b) -> Array`](#zipa-b---array) | Pairs elements of `a` and `b` by position. |
| [`join(arr, sep) -> String`](#joinarr-sep---string) | Joins the elements into a string, calling `str` on each one. |
| [`group_by(arr, key) -> Map`](#group_byarr-key---map) | Groups elements into a `Map` keyed by `key(element)`. |
| [`chunk(arr, size) -> Array`](#chunkarr-size---array) | Splits `arr` into consecutive sub-arrays of length `size`. |
| [`windows(arr, size) -> Array`](#windowsarr-size---array) | Builds every contiguous sub-array of length `size` (a sliding window). |
| [`partition(arr, pred) -> Array`](#partitionarr-pred---array) | Splits `arr` into the elements that match `pred` and those that do not. |
| [`flat_map(arr, func) -> Array`](#flat_maparr-func---array) | Maps each element through `func`, then flattens the result one level. |
| [`count_of(arr, pred) -> Int`](#count_ofarr-pred---int) | Counts the elements for which `pred` is truthy. |
| [`sort(arr) -> Array`](#sortarr---array) | Sorts the elements in ascending order, comparing them directly. |
| [`sort_by(arr, key) -> Array`](#sort_byarr-key---array) | Sorts the elements in ascending order, comparing `key(element)` rather than the elements themselves. |


### `push(arr, value) -> Array`

Appends `value` to `arr` in place. O(1) amortized, which makes building an array element by element O(n) overall, unlike repeated `arr += [x]`.

- `arr` *(Array)*: the array to mutate.
- `value` *(value)*: the element to append.

**Returns:** `arr`, the same array, so calls can be chained.

```tigr
a := [1, 2];
Array.push(a, 3);
print(a);   // => [1, 2, 3]
```

### `extend(arr, other) -> Array`

Appends every element of `other` to `arr` in place.

- `arr` *(Array)*: the array to mutate.
- `other` *(Array)*: the elements to append.

**Returns:** `arr`, the same array.

```tigr
a := [1, 2];
Array.extend(a, [3, 4]);
print(a);   // => [1, 2, 3, 4]
```

### `pop(arr) -> value`

Removes and returns the last element of `arr` in place.

- `arr` *(Array)*: the array to mutate.

**Returns:** the removed element, or `null` if `arr` was empty.

```tigr
a := [1, 2, 3];
print(Array.pop(a));   // => 3
print(a);              // => [1, 2]
```

### `shift(arr) -> value`

Removes and returns the first element of `arr` in place.

- `arr` *(Array)*: the array to mutate.

**Returns:** the removed element, or `null` if `arr` was empty.

```tigr
a := [1, 2, 3];
print(Array.shift(a));   // => 1
print(a);                // => [2, 3]
```

### `unshift(arr, value) -> Array`

Prepends `value` to the front of `arr` in place.

- `arr` *(Array)*: the array to mutate.
- `value` *(value)*: the element to prepend.

**Returns:** `arr`, the same array.

```tigr
a := [2, 3];
Array.unshift(a, 1);
print(a);   // => [1, 2, 3]
```

### `insert(arr, index, value) -> Array`

Inserts `value` at `index` in place. A negative `index` counts from the end, and the index is clamped to `[0, #arr]`.

- `arr` *(Array)*: the array to mutate.
- `index` *(Int)*: where to insert.
- `value` *(value)*: the element to insert.

**Returns:** `arr`, the same array.

```tigr
a := [1, 2, 4];
Array.insert(a, 2, 3);
print(a);   // => [1, 2, 3, 4]
```

### `remove(arr, index, count?) -> value`

Removes elements from `arr` in place. With two arguments it removes the single element at `index`. With three arguments it removes `count` elements starting at `index`. Negative indices count from the end.

- `arr` *(Array)*: the array to mutate.
- `index` *(Int)*: where to start removing.
- `count` *(Int, optional)*: how many elements to remove.

**Returns:** the single removed element with two arguments (`null` if out of range), or a new array of the removed elements with three.

```tigr
a := [1, 2, 3, 4];
print(Array.remove(a, 1));      // => 2
print(Array.remove(a, 0, 2));   // => [1, 3]
print(a);                       // => [4]
```

### `clear(arr) -> Array`

Removes every element from `arr` in place.

- `arr` *(Array)*: the array to empty.

**Returns:** `arr`, now empty.

```tigr
a := [1, 2, 3];
Array.clear(a);
print(a);   // => []
```

### `create(len, func) -> Array`

Builds an array of length `len` where element `i` is `func(i)`.

- `len` *(Int)*: the length of the array to build.
- `func` *(Fn)*: called with each index `i`, returns that element.

**Returns:** a new `Array` of length `len`.

```tigr
print(Array.create(5, fn(i) { i * i }));   // => [0, 1, 4, 9, 16]
```

### `concat(a, b) -> Array`

Joins two arrays into one.

- `a` *(Array)*: the first array.
- `b` *(Array)*: the second array.

**Returns:** a fresh `Array`. Neither `a` nor `b` is modified.

```tigr
print(Array.concat([1, 2], [3, 4]));   // => [1, 2, 3, 4]
```

### `map(arr, func) -> Array`

Applies `func` to every element and collects the results.

- `arr` *(Array)*: the array to read.
- `func` *(Fn)*: called as `func(element, index, whole_array)`, returns the new element.

**Returns:** a new `Array` of the mapped values.

```tigr
print(Array.map([1, 2, 3], fn(x) { x * 10 }));   // => [10, 20, 30]
```

### `filter(arr, pred) -> Array`

Keeps the elements for which `pred` returns a truthy value.

- `arr` *(Array)*: the array to read.
- `pred` *(Fn)*: called as `pred(element, index, whole_array)`.

**Returns:** a new `Array` of the kept elements.

```tigr
print(Array.filter([1, 2, 3, 4], fn(x) { x % 2 == 0 }));   // => [2, 4]
```

### `reduce(arr, func, seed) -> value`

Folds the array left to right into a single value.

- `arr` *(Array)*: the array to read.
- `func` *(Fn)*: called as `func(acc, element, index, whole_array)`, returns the new accumulator.
- `seed` *(value)*: the initial accumulator.

**Returns:** the final accumulator value.

```tigr
print(Array.reduce([1, 2, 3, 4], fn(acc, x) { acc + x }, 0));   // => 10
```

### `flatten(arr) -> Array`

Concatenates one level of nested arrays. A non-array element is kept as is.

- `arr` *(Array)*: the array to flatten.

**Returns:** a new `Array` flattened by one level.

```tigr
print(Array.flatten([[1, 2], [3], [4, 5]]));   // => [1, 2, 3, 4, 5]
```

### `reverse(arr) -> Array`

Reverses the order of the elements.

- `arr` *(Array)*: the array to read.

**Returns:** a new `Array` with the elements in reverse order.

```tigr
print(Array.reverse([1, 2, 3]));   // => [3, 2, 1]
```

### `index(arr, elem) -> Int`

Finds the first index whose element is `==` to `elem`.

- `arr` *(Array)*: the array to search.
- `elem` *(value)*: the value to look for.

**Returns:** the first matching index, or `null` if `elem` is not present.

```tigr
print(Array.index(['a', 'b', 'c'], 'b'));   // => 1
print(Array.index(['a', 'b', 'c'], 'z'));   // => null
```

### `find(arr, pred) -> value`

Finds the first element for which `pred` is truthy.

- `arr` *(Array)*: the array to search.
- `pred` *(Fn)*: called with each element.

**Returns:** the first matching element, or `null` if none match.

```tigr
print(Array.find([1, 3, 4, 7], fn(x) { x % 2 == 0 }));   // => 4
```

### `find_index(arr, pred) -> Int`

Finds the index of the first element for which `pred` is truthy.

- `arr` *(Array)*: the array to search.
- `pred` *(Fn)*: called with each element.

**Returns:** the first matching index, or `-1` if none match.

```tigr
print(Array.find_index([1, 3, 4, 7], fn(x) { x % 2 == 0 }));   // => 2
```

### `any(arr, pred) -> Bool`

Tests whether `pred` holds for at least one element.

- `arr` *(Array)*: the array to test.
- `pred` *(Fn)*: called with each element.

**Returns:** `true` if any element matches, otherwise `false`.

```tigr
print(Array.any([1, 2, 3], fn(x) { x > 2 }));   // => true
```

### `all(arr, pred) -> Bool`

Tests whether `pred` holds for every element.

- `arr` *(Array)*: the array to test.
- `pred` *(Fn)*: called with each element.

**Returns:** `true` if every element matches, otherwise `false`. An empty array gives `true`.

```tigr
print(Array.all([2, 4, 6], fn(x) { x % 2 == 0 }));   // => true
```

### `head(arr, n) -> Array`

Takes the first `n` elements. A negative `n` counts from the end, so `head(arr, -1)` is everything but the last element.

- `arr` *(Array)*: the array to read.
- `n` *(Int)*: how many elements to take.

**Returns:** a new `Array` of the first `n` elements.

```tigr
print(Array.head([1, 2, 3, 4], 2));    // => [1, 2]
print(Array.head([1, 2, 3, 4], -1));   // => [1, 2, 3]
```

### `tail(arr, n) -> Array`

Takes the last `n` elements. A negative `n` counts from the start, so `tail(arr, -1)` is everything but the first element.

- `arr` *(Array)*: the array to read.
- `n` *(Int)*: how many elements to take.

**Returns:** a new `Array` of the last `n` elements.

```tigr
print(Array.tail([1, 2, 3, 4], 2));    // => [3, 4]
print(Array.tail([1, 2, 3, 4], -1));   // => [2, 3, 4]
```

### `take(arr, n) -> Array`

Takes the first `n` elements, clamping `n` to `[0, #arr]`. Unlike `head`, a negative `n` becomes `0`.

- `arr` *(Array)*: the array to read.
- `n` *(Int)*: how many elements to take.

**Returns:** a new `Array` of the first `n` elements.

```tigr
print(Array.take([1, 2, 3, 4], 2));   // => [1, 2]
print(Array.take([1, 2, 3, 4], 9));   // => [1, 2, 3, 4]
```

### `drop(arr, n) -> Array`

Drops the first `n` elements and keeps the rest, clamping `n` to `[0, #arr]`.

- `arr` *(Array)*: the array to read.
- `n` *(Int)*: how many elements to drop.

**Returns:** a new `Array` of the remaining elements.

```tigr
print(Array.drop([1, 2, 3, 4], 2));   // => [3, 4]
```

### `slice(arr, start, end) -> Array`

Takes the elements in the range `[start, end)`. Out-of-range bounds are clamped.

- `arr` *(Array)*: the array to read.
- `start` *(Int)*: the inclusive start index.
- `end` *(Int)*: the exclusive end index.

**Returns:** a new `Array` of the selected elements, empty if `start >= end`.

```tigr
print(Array.slice([1, 2, 3, 4, 5], 1, 4));   // => [2, 3, 4]
```

### `sum(arr) -> Number`

Adds up the elements.

- `arr` *(Array)*: the array of numbers.

**Returns:** the sum, or `0` for an empty array.
**Raises:** the usual arithmetic error if an element is not a number.

```tigr
print(Array.sum([1, 2, 3, 4]));   // => 10
```

### `max_of(arr) -> value`

Finds the largest element by `>` comparison.

- `arr` *(Array)*: the array to scan.

**Returns:** the largest element, or `null` if `arr` is empty.

```tigr
print(Array.max_of([3, 1, 4, 1, 5]));   // => 5
```

### `min_of(arr) -> value`

Finds the smallest element by `<` comparison.

- `arr` *(Array)*: the array to scan.

**Returns:** the smallest element, or `null` if `arr` is empty.

```tigr
print(Array.min_of([3, 1, 4, 1, 5]));   // => 1
```

### `uniq(arr) -> Array`

Keeps the first occurrence of each distinct element, preserving order. This is O(n^2), fine for small data.

- `arr` *(Array)*: the array to read.

**Returns:** a new `Array` of the first-seen unique elements.

```tigr
print(Array.uniq([1, 2, 2, 3, 1, 3]));   // => [1, 2, 3]
```

### `zip(a, b) -> Array`

Pairs elements of `a` and `b` by position.

- `a` *(Array)*: the first array.
- `b` *(Array)*: the second array.

**Returns:** a new `Array` of `[a[i], b[i]]` pairs. Its length is `min(#a, #b)`.

```tigr
print(Array.zip([1, 2, 3], ['a', 'b', 'c']));   // => [[1, a], [2, b], [3, c]]
```

### `join(arr, sep) -> String`

Joins the elements into a string, calling `str` on each one.

- `arr` *(Array)*: the array to join.
- `sep` *(String)*: the separator placed between elements.

**Returns:** the joined `String`, or `''` for an empty array.

```tigr
print(Array.join([1, 2, 3], '-'));   // => 1-2-3
```

### `group_by(arr, key) -> Map`

Groups elements into a `Map` keyed by `key(element)`. Each value is the array of elements that produced that key, in first-seen order. The result is a `Map` (not an Object) so non-string keys work.

- `arr` *(Array)*: the array to read.
- `key` *(Fn)*: called as `key(element, index, whole_array)`, returns the grouping key.

**Returns:** a `Map` from key to array of elements.

```tigr
g := Array.group_by([1, 2, 3, 4, 5], fn(x) { x % 2 });
print(Map.get(g, 0));   // => [2, 4]
print(Map.get(g, 1));   // => [1, 3, 5]
```

### `chunk(arr, size) -> Array`

Splits `arr` into consecutive sub-arrays of length `size`. The last chunk is shorter if `#arr` is not a multiple of `size`.

- `arr` *(Array)*: the array to split.
- `size` *(Int)*: the chunk length.

**Returns:** a new `Array` of sub-arrays, or `[]` if `size < 1`.

```tigr
print(Array.chunk([1, 2, 3, 4, 5], 2));   // => [[1, 2], [3, 4], [5]]
```

### `windows(arr, size) -> Array`

Builds every contiguous sub-array of length `size` (a sliding window).

- `arr` *(Array)*: the array to read.
- `size` *(Int)*: the window length.

**Returns:** a new `Array` of windows, or `[]` if `size < 1` or `size > #arr`.

```tigr
print(Array.windows([1, 2, 3, 4], 2));   // => [[1, 2], [2, 3], [3, 4]]
```

### `partition(arr, pred) -> Array`

Splits `arr` into the elements that match `pred` and those that do not.

- `arr` *(Array)*: the array to split.
- `pred` *(Fn)*: called as `pred(element, index, whole_array)`.

**Returns:** a two-element `Array`, `[matching, non_matching]`.

```tigr
print(Array.partition([1, 2, 3, 4], fn(x) { x % 2 == 0 }));   // => [[2, 4], [1, 3]]
```

### `flat_map(arr, func) -> Array`

Maps each element through `func`, then flattens the result one level. This is `map` followed by `flatten` in a single pass.

- `arr` *(Array)*: the array to read.
- `func` *(Fn)*: called as `func(element, index, whole_array)`, returns an array or value.

**Returns:** a new `Array`, flattened by one level.

```tigr
print(Array.flat_map([1, 2, 3], fn(x) { [x, x * 10] }));   // => [1, 10, 2, 20, 3, 30]
```

### `count_of(arr, pred) -> Int`

Counts the elements for which `pred` is truthy.

- `arr` *(Array)*: the array to read.
- `pred` *(Fn)*: called as `pred(element, index, whole_array)`.

**Returns:** the count as an `Int`.

```tigr
print(Array.count_of([1, 2, 3, 4, 5], fn(x) { x > 2 }));   // => 3
```

### `sort(arr) -> Array`

Sorts the elements in ascending order, comparing them directly. This is an insertion sort, O(n^2).

- `arr` *(Array)*: the array to sort.

**Returns:** a new sorted `Array`. The input is not modified.

```tigr
print(Array.sort([3, 1, 4, 1, 5, 9, 2]));   // => [1, 1, 2, 3, 4, 5, 9]
```

### `sort_by(arr, key) -> Array`

Sorts the elements in ascending order, comparing `key(element)` rather than the elements themselves. Use it to sort by a field or a computed property.

- `arr` *(Array)*: the array to sort.
- `key` *(Fn)*: called with each element, returns the value to sort on.

**Returns:** a new sorted `Array`. The input is not modified.

```tigr
print(Array.sort_by(['ccc', 'a', 'bb'], fn(w) { #w }));   // => [a, bb, ccc]
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#array): the authoritative spec for `Array`
- [Iter](iter.md): lazy pipelines that avoid building intermediate arrays
- [Object](object.md): the same callback style for key/value collections
- [Control flow](../language/control-flow.md): the `for` and `for[]` loops these functions are built on
