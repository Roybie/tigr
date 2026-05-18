# Collections

Spec: [LANGUAGE.md §7](../../LANGUAGE.md#7-collections)

This page covers the three built-in collection literals: arrays, objects, and ranges. Arrays and objects are reference types, so passing one around shares the underlying value. Ranges are lazy values that double as a slicing tool.

## Arrays

An array is an ordered, heterogeneous sequence written with `[]`.

```tigr
arr := [1, 2, 3];
print(arr[0]);     // => 1
print(arr[-1]);    // => 3    negative indices count from the end
print(#arr);       // => 3    # is the length
arr[0] = 99;       // mutates in place
print(arr);        // => [99, 2, 3]
```

An out-of-range index returns `null`.

### Concatenation with `+`

`Array + Array` concatenates. `Array + value` appends that value as one element. `+` always builds a fresh array and never touches its operands.

```tigr
arr := [1, 2, 3];
print(arr + 4);        // => [1, 2, 3, 4]      append one element
print(arr + [4, 5]);   // => [1, 2, 3, 4, 5]   concatenate
print(arr);            // => [1, 2, 3]         arr itself is unchanged
```

`Array + Array` does not nest. To append an array as a single element, write `arr + [[…]]` or use `Array.push`.

### In-place growth with `+=`

`+=` grows an array in place. It uses the same array-vs-value rule as `+` (an array right-hand side extends, anything else appends one element), but it mutates the existing array instead of rebinding the name. Every alias of that array observes the change, matching how `arr[i] = v` already behaves.

```tigr
a := [1, 2, 3];
b := a;          // b and a share the same array
a += 4;
print(b);        // => [1, 2, 3, 4]      b sees it too
a += [5, 6];
print(a);        // => [1, 2, 3, 4, 5, 6]
```

`Array.push` and `Array.extend` also append in place and are O(1) amortized. Building an array with repeated `arr = arr + x` is O(n²), so prefer `+=`, `push`, or a `for[]` loop for accumulation.

### Slicing with a Range

Indexing an array with a `Range` instead of an `Int` returns a fresh sub-array (a copy). Out-of-range endpoints clamp, and the range's step and direction carry through, so a descending range reverses.

```tigr
arr := [10, 20, 30, 40, 50];
print(arr[1..3]);          // => [20, 30]                  exclusive end
print(arr[1..=3]);         // => [20, 30, 40]              inclusive end
print(arr[0..#arr:2]);     // => [10, 30, 50]              step 2
print(arr[#arr-1..=0]);    // => [50, 40, 30, 20, 10]      descending reverses
print(arr[0..1000]);       // => [10, 20, 30, 40, 50]      out-of-range bounds clamp
```

The same `coll[Range]` slice works on `Bytes` and, character-indexed, on `String`.

### Spread

`...` unpacks an iterable into an array literal:

```tigr
print([1, ...[7, 8], 9]);   // => [1, 7, 8, 9]
```

## Objects

An object is a string-keyed record written with `${}`.

```tigr
obj := ${
    name: 'tigr',
    'with space': 1,
    nested: ${ inner: true },
};
print(obj.name);            // => tigr   .key is sugar for ['key']
print(obj['with space']);   // => 1
obj.color = 'red';          // add a new key in place
print(#obj);                // => 4      number of keys
print(obj.missing);         // => null   a missing key returns null
```

Keys are always strings. An identifier key (`name:`) is sugar for the quoted form (`'name':`). Indexed assignment mutates the object in place.

Object spread merges, with later keys winning:

```tigr
defaults := ${ size: 1, color: 'blue' };
print(${...defaults, color: 'red'});   // => ${size: 1, color: red}
```

Shorthand `${name}` is equivalent to `${name: name}`, taking the value from a binding of that name.

## Ranges

A range is a first-class lazy value, not a loop. It describes a `from`, a `to`, and a `step`.

```
0..10      // [0, 10)  exclusive end
0..=10     // [0, 10]  inclusive end
0..10:2    // step 2:  0, 2, 4, 6, 8
10..0:-1   // descending: 10, 9, …, 1
10..0      // descending; step auto-flips to -1
```

A range whose `step` does not move `from` toward `to` is empty.

Ranges are lazy: they do not materialize their elements unless you spread or index them. They support length, indexing, iteration, spread, and use as a slice key:

```tigr
r := 0..10;
print(#r);                // => 10                     length
print(r[2]);              // => 2                      element at an index
print([...0..5]);         // => [0, 1, 2, 3, 4]        spread to materialize
print(#(10..0:-1));       // => 10                     descending range length
print((0..10:2)[1]);      // => 2                      indexing with a step
```

A range is also the slice key in `arr[1..3]`, covered above.

## See also

- [Expressions](expressions.md): indexing, member access, and spread in general
- [Control flow](control-flow.md): iterating collections with `for` and `for[]`
- [Destructuring](destructuring.md): array and object patterns on the left of `:=`
- [Overview](overview.md): reference vs value types
- [LANGUAGE.md §7](../../LANGUAGE.md#7-collections): the authoritative spec
