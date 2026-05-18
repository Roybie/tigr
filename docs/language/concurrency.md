# Concurrency

Spec: [LANGUAGE.md Appendix L](../../LANGUAGE.md#appendix-l--changes-in-v014)

Tigr runs concurrent work as **actors**. Each `spawn` starts a function on its own OS thread with its own heap. Actors share no mutable state. They communicate only by passing messages through channels, and a message is deep-copied across the heap boundary as it travels. That makes the model race-free by construction, and it fits the per-thread garbage collector with no changes to it (see [Garbage collection](gc.md)).

## `spawn` and `Task.join`

`spawn fn` runs a function as an actor and evaluates immediately to a `Task` handle. It does not block. `Task.join(t)` blocks until the actor finishes and yields its result.

```tigr
Task := import 'Task';
t := spawn fn() { 21 * 2 };
print(Task.join(t));   // => 42
```

A spawned function is copied across the heap boundary, so it may capture only **sendable** values: primitives, `String`, `Bytes`, `Range`, `BigInt`, the four collections, channels, tasks, and functions whose own captures are themselves sendable. Capturing an iterator, a native function, or a function with a still-open capture raises a catchable `not_sendable`. A cyclic collection raises `cycle`.

Because the function is copied, it cannot see later mutations in the parent, and it runs its own `import`s. An actor's uncaught error surfaces at `join`, catchable like any other error: a `raise`d value re-raises verbatim, and a built-in error arrives as a `${kind, message, trace, worker}` object.

## Channels

A `Channel` carries messages between actors. It is the one reference type that crosses thread boundaries, and a sent value is deep-copied into the receiving actor's heap. Channels are bidirectional: any holder can both send and receive.

```tigr
Channel := import 'Channel';
ch := Channel.new();
spawn fn() { C := import 'Channel'; C.send(ch, 'hi') };
print(Channel.recv(ch).value);   // => hi
```

`Channel.new()` is unbounded. `Channel.new(n)` bounds the buffer at `n`, so `send` blocks (backpressure) while the buffer is full. `recv` blocks for the next message and returns `${value: v}`, or `${closed: true}` once the channel is closed and drained. `try_recv` never blocks: it adds a third shape, `${empty: true}`, when nothing is ready. `close` wakes every blocked actor, and a `send` to a closed channel raises the catchable `channel_closed`.

The `${value: v}` and `${closed: true}` shape is designed for `match`, so a receive loop can branch cleanly on what came back.

## `select`

`select` waits on several channels at once and runs the arm of the first one to have a message, binding the named variable to that value. A trailing `else` arm makes `select` non-blocking: it runs when no channel is ready.

```tigr
Channel := import 'Channel';
jobs := Channel.new();
Channel.send(jobs, 'task-1');

result := select {
    job := jobs => 'got ' + job,
    else        => 'idle'
};
print(result);   // => got task-1

empty := Channel.new();
idle := select {
    x := empty => 'got ' + x,
    else       => 'nothing ready'
};
print(idle);     // => nothing ready
```

A closed channel is skipped. If every channel in the `select` is closed and there is no `else`, the `select` raises `channel_closed`. `select` is not a new core construct: it desugars to a `match`.

## `parallel[]`

`parallel[]` mirrors `for[]` but runs each iteration's body as its own actor, all concurrently, then collects the results into an array in input order.

```tigr
squares := parallel[] (n, 1..=8) { n * n };
print(squares);   // => [1, 4, 9, 16, 25, 36, 49, 64]
```

Each body is deep-copied per actor, so the same sendability rule as `spawn` applies. The first body to raise aborts the block, and that error propagates out. Sibling actors already started run to completion, but their results are discarded. There is no cancellation primitive.

`parallel[]` is the structured, common-case form for a simple fan-out. Reach for raw `spawn`, `Channel`, and `select` when the work is not a plain fan-out, for example a pipeline or a worker pool.

## See also

- [Channel module](../stdlib/channel.md): the full `Channel` API
- [Task module](../stdlib/task.md): the `Task` handle and `join`
- [Garbage collection](gc.md): the per-thread heap each actor runs on
- [Errors](errors.md): `not_sendable`, `channel_closed`, and `cycle`
- [LANGUAGE.md Appendix L](../../LANGUAGE.md#appendix-l--changes-in-v014): the authoritative spec
