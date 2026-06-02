# Concurrency

Spec: [LANGUAGE.md Appendix L](../../LANGUAGE.md#appendix-l--changes-in-v014)

Tigr runs concurrent work as **actors**. Each `spawn` starts a function on its own OS thread with its own heap. Actors share no mutable state. They communicate only by passing messages through channels, and a message is deep-copied across the heap boundary as it travels. That makes the model race-free by construction, and it fits the per-thread garbage collector with no changes to it (see [Garbage collection](gc.md)).

## `spawn` and `join`

`spawn fn` runs a function as an actor and evaluates immediately to a `Task` handle. It does not block. `join(t)` blocks until the actor finishes and yields its result. Both `spawn` and `join` are built in, so neither needs an import.

```tigr
t := spawn fn() { 21 * 2 };
print(join(t));   // => 42
```

A spawned function is copied across the heap boundary, so it may capture only **sendable** values: primitives, `String`, `Bytes`, `Range`, `BigInt`, the four collections, channels, tasks, and functions whose own captures are themselves sendable. Capturing an iterator, a native function, or a function with a still-open capture raises a catchable `not_sendable`. A cyclic collection raises `cycle`.

Because the function is copied, it cannot see later mutations in the parent. Stdlib modules are ambient in the actor, so the body uses them directly; any local-file `import` it writes runs fresh in the actor. An actor's uncaught error surfaces at `join`, catchable like any other error: a `raise`d value re-raises verbatim, and a built-in error arrives as a `${kind, message, trace, worker}` object.

## Channels

A `Channel` carries messages between actors. It is the one reference type that crosses thread boundaries, and a sent value is deep-copied into the receiving actor's heap. Channels are bidirectional: any holder can both send and receive.

```tigr
ch := Channel.new();
spawn fn() { Channel.send(ch, 'hi') };
print(Channel.recv(ch).value);   // => hi
```

`Channel.new()` is unbounded. `Channel.new(n)` bounds the buffer at `n`, so `send` blocks (backpressure) while the buffer is full. `recv` blocks for the next message and returns `${value: v}`, or `${closed: true}` once the channel is closed and drained. `try_recv` never blocks: it adds a third shape, `${empty: true}`, when nothing is ready. `close` wakes every blocked actor, and a `send` to a closed channel raises the catchable `channel_closed`.

The `${value: v}` and `${closed: true}` shape is designed for `match`, so a receive loop can branch cleanly on what came back.

## `select`

`select` waits on several channels at once and runs the arm of the first one to have a message, binding the named variable to that value. A trailing `else` arm makes `select` non-blocking: it runs when no channel is ready.

```tigr
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

Each body is deep-copied per actor, so the same sendability rule as `spawn` applies. The first body to raise aborts the block, and that error propagates out. Sibling actors already started run to completion, but their results are discarded; `parallel[]` cannot interrupt an actor mid-flight. (The cooperative [`cancel`](#cancelling-a-coroutine-cancel) below is a green-thread primitive, not a `parallel[]` one.)

`parallel[]` is the structured, common-case form for a simple fan-out. Reach for raw `spawn`, `Channel`, and `select` when the work is not a plain fan-out, for example a pipeline or a worker pool.

## Green threads: `go` and `yield`

An actor is heavyweight: one OS thread, one heap, deep-copied messages. For many lightweight tasks that share state inside a single actor, that is the wrong tool. **Green threads** are the lighter axis. `go fn` spawns a function as a coroutine inside the current actor. It shares that actor's heap, so no copying and no channels are needed, and it is scheduled cooperatively onto the same OS thread.

```tigr
log := [];
go fn() { log = log + ['from the coroutine'] };
while (#log == 0) { yield };
print(log);   // => [from the coroutine]
```

Scheduling is cooperative and has no preemption. A coroutine runs until it `yield`s or returns, then the scheduler hands control to the next ready one, round-robin. `yield` with nothing else ready resumes immediately. The actor's main program is itself coroutine zero, so the `while (...) { yield }` idiom above pumps the scheduler until a coroutine has done its work. A coroutine that never yields starves the rest.

A blocking call is handled differently. When other coroutines are live, the call is moved off the actor thread: the calling coroutine cooperatively parks until the result is ready, and its siblings keep running meanwhile, so the blocking call no longer freezes the actor. With nothing else to schedule the call simply runs inline on the actor thread, so a program that uses no `go` is unaffected.

Two backends share the offload work. A *worker pool* handles short blocking work: `Os.run` and `Os.cwd`, the waiting `IO` file and directory calls (`read_file`, `write_file`, `append_file`, the byte variants, `list_dir`, `mkdir`, `remove`, `read_line`), the calls that may need a blocking name lookup (`connect`, `connect_tls`, `send_to`), and the cross-actor waits `Channel.send`, `Channel.recv`, `select`, and `join` on a `Task`. Steady-state socket I/O runs instead on a single *async-I/O reactor* thread built on the operating system's `epoll` or `kqueue`: `accept`, `read`, `write`, `read_exact`, `read_line`, `read_until`, `read_all`, and `recv_from`. The difference shows at scale. A coroutine parked in `read` on the reactor costs one table entry, so one actor can hold tens of thousands of idle connections open at once, where a pool that wanted one OS thread per parked read would run out of threads. A coroutine cannot tell the two backends apart: either way it parks, its siblings run, and the result arrives the same way.

Fast non-waiting calls (`IO.exists`/`is_dir`/`is_file`/`stat`, `Net.listen`/`bind`/`local_addr`/`peer_addr`/`set_timeout`/`close`, `Channel.try_recv`/`close`) stay inline. One consequence of cooperative parking: a green thread may `Channel.recv` from a sibling green thread in the same actor without deadlocking, because the receive parks cooperatively rather than sleeping the shared OS thread.

Two-level mental model: `spawn` is real parallelism across cores, with separate heaps and copied messages; `go` is cheap concurrency on one core, with a shared heap and cooperative hand-off. Pick the axis the work needs.

### Waiting on a coroutine: `join`

`go` evaluates to a **green-thread handle**. The same `join` that waits on a `spawn`ed actor also waits on a `go` coroutine: `join(handle)` cooperatively yields the caller until the coroutine returns, then evaluates to its return value. While the caller is parked the scheduler runs the other coroutines, so a `join` on an unfinished coroutine is a cooperative block, not a busy-wait.

```tigr
h := go fn() {
    total := 0;
    for (i, 1..=100) { total = total + i };
    total
};
print(join(h));   // => 5050
```

A handle may be joined more than once; every `join` returns the recorded result. An uncaught `raise` in a `go` body does not abort the actor: the coroutine ends, and its error is recorded on the handle so a later `join` re-raises it (the raised value reaches `catch` verbatim, a built-in error as the usual `${kind, message, line}` object). A body that wants the joiner to keep going regardless can `catch` internally and return a tagged value instead. `join` from inside a generator body, or a `join` that would block with no other coroutine able to run, raises rather than hanging.

### Cancelling a coroutine: `cancel`

`cancel(handle)` requests cancellation of a `go` coroutine. It does not block: it marks the handle and returns straight away, `true` if the coroutine was still live and is now marked, `false` if it had already finished. Marking it twice is harmless. The cancellation takes effect the next time the coroutine resumes from a park. Any park counts, not only `wait`: a `yield`, a `join`, a channel receive, a blocking IO call, and a host frame wait (`wait_frame` in a purr game) are all cancellation points. On that resume the park's normal value is replaced by a catchable `cancelled` raised at the park's call site, which unwinds the body the same way any other error does, so a `try`/`catch` and its cleanup still run.

```tigr
h := go fn() {
    work_started();
    wait(10);            // parked here
    work_finished();     // never reached once cancelled
};
yield;                   // let the coroutine reach its wait
cancel(h);
print(join(h));          // => ${cancelled: true}
```

If the coroutine was parked when it got cancelled, `join` on it returns `${cancelled: true}` instead of re-raising. That is the same shape `LocalChannel` uses for `${closed: true}` and `${value}`, so it reads well in a `match`. A `cancel` of anything that is not a green-thread handle is a type error.

Because cancellation fires only at a park, two things follow, both on purpose. First, there is no preemption. A coroutine is interrupted only where it parks, so one whose body has no park, or that is cancelled before it starts and then never parks, runs to completion. Cancellation has nowhere to fire and the coroutine is left alone. Second, `cancelled` is an ordinary catchable error, so a `try` around a park can catch it, clean up, and carry on. It fires once per request and is cleared as it is raised, so a cleanup handler may itself `wait` or `yield` without being cancelled again. A body that catches `cancelled` and keeps going is making the same kind of choice it makes when it catches any other error.

A coroutine can also cancel itself by passing its own handle to `cancel`; the mark takes effect at its own next park. Cancelling one that is asleep in `wait(10)` does not sit through the ten seconds. The pending park is dropped and the coroutine resumes right away to see the cancellation.

### Intra-actor channels: `LocalChannel`

`LocalChannel` is a channel *between green threads* of one actor. Because every coroutine shares the actor's heap, a message moves directly, with no deep copy and no transfer-encoding (contrast the cross-actor [`Channel`](../stdlib/channel.md), which copies). `send` is unbounded and never blocks; `recv` on an empty channel `yield`s the coroutine until a value or a close arrives.

```tigr
ch := LocalChannel.new();
go fn() {
    for (i, 1..=3) { LocalChannel.send(ch, i) };
    LocalChannel.close(ch);
};
looping := true;
while (looping) {
    m := LC.recv(ch);
    if (m.closed == true) { looping = false }
    else { print(m.value) };   // => 1, 2, 3
};
```

`recv` and `try_recv` return `${value: v}`, `${closed: true}` once the channel is closed and drained, or (`try_recv` only) `${empty: true}`. `send` on a closed channel raises `channel_closed`.

## Generators: `gen fn`

A `gen fn` is a generator function. Calling it does not run the body. It builds a paused coroutine and returns an iterator object `${next: fn()}`. Each `next()` call runs the body until the next `yield`, which produces a value (`${done: false, value}`); when the body returns, `next()` reports `${done: true}` from then on.

```tigr
ramp := gen fn(n) {
    i := 0;
    while i < n { yield i; i = i + 1; };
};

g := ramp(3);
print(g.next());   // => ${done: false, value: 0}
print([...ramp(3)]);   // => [0, 1, 2]
for (x, ramp(3)) { print(x); };   // => 0, 1, 2
```

Because a generator speaks the ordinary iterator protocol, a `for` loop, the spread forms `[...g]` and `f(...g)`, and the whole [`Iter`](../stdlib/iter.md) module drive it directly. Generators are the natural way to write infinite or streaming sequences: a `gen fn` with `while true` only computes the next value when it is pulled. They compose, too, a generator can `for`-loop over another generator and `yield` transformed values. A `raise` that escapes a generator's body surfaces at the `next()` call site, so it can be caught with an ordinary `try` around the pull.

`Iter` itself is built from `gen fn` generators, so a generator you write drops straight into an `Iter` pipeline.

## See also

- [Channel module](../stdlib/channel.md): the full `Channel` API
- [Iter module](../stdlib/iter.md): lazy pipelines, built from generators
- [Garbage collection](gc.md): the per-thread heap each actor runs on
- [Errors](errors.md): `not_sendable`, `channel_closed`, and `cycle`
- [LANGUAGE.md Appendix L](../../LANGUAGE.md#appendix-l--changes-in-v014): the authoritative spec
