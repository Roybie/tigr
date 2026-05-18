# Garbage collection

Spec: [LANGUAGE.md §15.1](../../LANGUAGE.md#151-value-representation), [Appendix J](../../LANGUAGE.md#appendix-j--changes-in-v010)

Tigr manages memory for you. The mutable, potentially-cyclic value types live on a heap reclaimed by a tracing garbage collector, so you never free anything by hand and reference cycles do not leak.

## The collector model

The managed types are `Array`, `Object`, `Map`, `Set`, iterators, and the cells closures capture for their upvalues. They live on a per-thread arena heap, and a `Value` carries a small generation-tagged handle into that heap rather than the data itself.

The collector is **mark-sweep**. It walks every live object from the roots, then reclaims whatever it did not reach. A plain reference count cannot do this: a structure that points back at itself keeps its own count above zero and leaks forever. A tracing collector reclaims such a cycle like any other garbage.

```tigr
node := ${value: 1, next: null};
node.next = node;   // a cycle: node points at itself
print(node.value);  // => 1
```

When `node` goes out of scope, the collector reclaims it on a later pass even though the cycle keeps its reference count alive.

Collection is automatic. It runs at safe points between bytecode instructions, once the live-object count crosses a threshold that grows as the program does. There is no way to force a collection from tigr code, and collection has no effect observable from your program beyond reclaiming memory.

`Str`, `Range`, and the immutable function template are not managed by the collector. They are acyclic, so a reference count reclaims them correctly and the collector does not need to trace them.

Each actor spawned with `spawn` runs on its own heap with its own collector, which is why the model needs no coordination between threads (see [Concurrency](concurrency.md)).

## The `gc()` builtin

`gc()` is a zero-argument builtin that returns a read-only snapshot of the collector's counters. It is meant for tests and for observing memory behavior. It does not trigger a collection.

The returned object has four fields:

- `live`: the current count of managed objects on the heap.
- `collections`: how many collections have run so far.
- `allocated`: the lifetime total of managed objects allocated.
- `freed`: the lifetime total of managed objects reclaimed.

```tigr
stats := gc();
print(stats.collections);   // => 0

for (i, 0..50000) { tmp := [i, i, i] };
after := gc();
print(after.collections > 0);   // => true
```

The first snapshot shows a fresh program with no collections run yet. After allocating tens of thousands of short-lived arrays, the counter confirms the collector has run at least once on its own. The `live`, `allocated`, and `freed` fields report object counts the same way; their exact values depend on what the program has done so far.

## See also

- [Concurrency](concurrency.md): each actor runs on its own heap and collector
- [LANGUAGE.md §15.1](../../LANGUAGE.md#151-value-representation): the value representation behind the heap
- [LANGUAGE.md Appendix J](../../LANGUAGE.md#appendix-j--changes-in-v010): the collector and the `gc()` builtin
