# `Iter`

> Pure-tigr source module, `stdlib/Iter.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#iter-v07)

The `Iter` module provides lazy, pull-based iterators. Where `Array.map` followed by `Array.filter` builds a complete intermediate array at every step, an `Iter` pipeline carries one element through the whole chain at a time and never materializes the in-between arrays. That is also what makes infinite sequences and short-circuiting possible. Import it as `Iter := import 'Iter'`.

An iterator is just an object `${next: fn()}`. Each `next()` call returns `${done: true}` or `${done: false, value: v}`. The functions fall into three groups: adapters create an iterator from a source, combinators wrap one iterator in another and run no work until pulled, and consumers drive the pulling and force evaluation. Callbacks are invoked as `callback(value)`.

The adapters and combinators are `gen fn` generators (see [concurrency](../language/concurrency.md)): each is a coroutine that `yield`s its elements one at a time, and calling it hands back the `${next: fn()}` iterator object. A generator you write yourself with `gen fn` is an iterator the whole module composes with — `Iter.map`, `for`, spread and the rest accept it directly.

A `for` loop and the spread forms `[...it]` and `f(...it)` consume an iterator object directly, so `collect` is only needed when you specifically want an `Array` value. `count` and `repeat` are infinite, so only pair them with a bounding combinator like `take` or a short-circuiting consumer like `find` or `nth`.

```tigr
Iter := import 'Iter';

print([1, 2, 3, 4, 5]
  |> Iter.from()
  |> Iter.map(fn(n) { n * n })
  |> Iter.filter(fn(n) { n > 4 })
  |> Iter.collect());               // => [9, 16, 25]
```

## Functions

### `from(iterable) -> Iterator`

Wraps any iterable — Array, Range, String, Object, Map, Set, or another iterator object — as an iterator, lazily: elements are pulled from the source one at a time as `next()` is called, never materialized up front.

- `iterable`: the source to wrap.

**Returns:** an `Iterator` over the source.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.from([1, 2, 3])));   // => [1, 2, 3]
print(Iter.collect(Iter.from(0..4)));        // => [0, 1, 2, 3]
```

### `count(start) -> Iterator`

Creates an infinite iterator yielding `start`, `start + 1`, `start + 2`, and so on. Always bound it with `take` or a short-circuiting consumer.

- `start` *(Int)*: the first value.

**Returns:** an infinite `Iterator`.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.take(Iter.count(10), 4)));   // => [10, 11, 12, 13]
```

### `repeat(value) -> Iterator`

Creates an infinite iterator yielding `value` forever. Always bound it with `take` or a short-circuiting consumer.

- `value` *(value)*: the value to repeat.

**Returns:** an infinite `Iterator`.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.take(Iter.repeat('x'), 3)));   // => [x, x, x]
```

### `map(it, func) -> Iterator`

Wraps `it` so each element is passed through `func` as it is pulled. Lazy: no work runs until the result is consumed.

- `it` *(Iterator)*: the source iterator.
- `func` *(Fn)*: called as `func(value)`, returns the new value.

**Returns:** an `Iterator` of the mapped values.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.map(Iter.from([1, 2, 3]), fn(n) { n + 100 })));   // => [101, 102, 103]
```

### `filter(it, pred) -> Iterator`

Wraps `it` to keep only the elements for which `pred` is truthy. Pulling the result pulls from `it` until a passing element appears or `it` is exhausted.

- `it` *(Iterator)*: the source iterator.
- `pred` *(Fn)*: called as `pred(value)`.

**Returns:** an `Iterator` of the kept elements.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.filter(Iter.from([1, 2, 3, 4, 5, 6]), fn(n) { n % 2 == 0 })));   // => [2, 4, 6]
```

### `take(it, n) -> Iterator`

Wraps `it` to yield at most its first `n` elements, then report done without pulling `it` again. This is what makes an infinite source safe to consume.

- `it` *(Iterator)*: the source iterator.
- `n` *(Int)*: the maximum number of elements to yield.

**Returns:** an `Iterator` of at most `n` elements.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.take(Iter.from([1, 2, 3, 4, 5]), 3)));   // => [1, 2, 3]
```

### `drop(it, n) -> Iterator`

Wraps `it` to skip its first `n` elements and yield the rest.

- `it` *(Iterator)*: the source iterator.
- `n` *(Int)*: how many elements to skip.

**Returns:** an `Iterator` of the elements after the first `n`.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.drop(Iter.from([1, 2, 3, 4, 5]), 2)));   // => [3, 4, 5]
```

### `enumerate(it) -> Iterator`

Wraps `it` to yield `[index, value]` pairs, with the index starting at `0`.

- `it` *(Iterator)*: the source iterator.

**Returns:** an `Iterator` of `[index, value]` pairs.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.enumerate(Iter.from(['a', 'b', 'c']))));   // => [[0, a], [1, b], [2, c]]
```

### `zip(a, b) -> Iterator`

Wraps two iterators to yield `[a_elem, b_elem]` pairs. It reports done as soon as either side is done.

- `a` *(Iterator)*: the first iterator.
- `b` *(Iterator)*: the second iterator.

**Returns:** an `Iterator` of pairs, as long as the shorter side.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.zip(Iter.from([1, 2, 3]), Iter.from(['a', 'b']))));   // => [[1, a], [2, b]]
```

### `chain(a, b) -> Iterator`

Wraps two iterators to yield every element of `a`, then every element of `b`.

- `a` *(Iterator)*: the first iterator.
- `b` *(Iterator)*: the second iterator.

**Returns:** an `Iterator` over `a` followed by `b`.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.chain(Iter.from([1, 2]), Iter.from([3, 4]))));   // => [1, 2, 3, 4]
```

### `collect(it) -> Array`

Drains the iterator into a fresh array. This forces evaluation of the whole pipeline. Diverges on an infinite iterator, so bound it with `take` first.

- `it` *(Iterator)*: the iterator to drain.

**Returns:** an `Array` of every yielded element.

```tigr
Iter := import 'Iter';

print(Iter.collect(Iter.from([1, 2, 3])));   // => [1, 2, 3]
```

### `reduce(it, func, seed) -> value`

Folds the iterator left to right into a single value.

- `it` *(Iterator)*: the iterator to drain.
- `func` *(Fn)*: called as `func(acc, value)`, returns the new accumulator.
- `seed` *(value)*: the initial accumulator.

**Returns:** the final accumulator value.

```tigr
Iter := import 'Iter';

print(Iter.reduce(Iter.from([1, 2, 3, 4]), fn(acc, x) { acc + x }, 0));   // => 10
```

### `for_each(it, func) -> Null`

Calls `func` on every element for its side effects.

- `it` *(Iterator)*: the iterator to drain.
- `func` *(Fn)*: called as `func(value)`.

**Returns:** `null`.

```tigr
Iter := import 'Iter';

Iter.for_each(Iter.from([1, 2, 3]), fn(x) { print(x) });
// => 1
// => 2
// => 3
```

### `count_of(it) -> Int`

Counts how many elements the iterator yields. Diverges on an infinite iterator, so bound it with `take` first.

- `it` *(Iterator)*: the iterator to drain.

**Returns:** the element count as an `Int`.

```tigr
Iter := import 'Iter';

print(Iter.count_of(Iter.filter(Iter.from(0..10), fn(n) { n % 3 == 0 })));   // => 4
```

### `find(it, pred) -> value`

Finds the first element for which `pred` is truthy. It short-circuits, so it is safe on an infinite iterator that contains a match.

- `it` *(Iterator)*: the iterator to search.
- `pred` *(Fn)*: called as `pred(value)`.

**Returns:** the first matching element, or `null` if none match.

```tigr
Iter := import 'Iter';

print(Iter.find(Iter.count(0), fn(n) { n > 100 }));   // => 101
```

### `nth(it, n) -> value`

Gets the element at 0-based index `n`. It short-circuits once index `n` is reached.

- `it` *(Iterator)*: the iterator to read.
- `n` *(Int)*: the index to fetch.

**Returns:** the element at index `n`, or `null` if the iterator is shorter.

```tigr
Iter := import 'Iter';

print(Iter.nth(Iter.from(['a', 'b', 'c', 'd']), 2));   // => c
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#iter-v07): the authoritative spec for `Iter`
- [Array](array.md): the eager equivalents that build arrays at each step
- [Control flow](../language/control-flow.md): how `for` consumes an iterator object directly
