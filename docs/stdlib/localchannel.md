# `LocalChannel`

> Pure-tigr source module, `stdlib/LocalChannel.tg`

A `LocalChannel` carries messages between **green threads** of one actor, the coroutines spawned with `go`. Unlike a [`Channel`](channel.md), which crosses actor (OS-thread) boundaries and deep-copies every message, a `LocalChannel` never leaves the actor's heap. Every coroutine that touches it shares that heap, so a message moves *directly*: no copy, no transfer-encoding. `type(ch)` is `'local_channel'`, and a `LocalChannel` is not JSON-serializable and cannot be sent across actors. Import it as `LC := import 'LocalChannel'`.

`LocalChannel.new()` takes no capacity: the channel is unbounded, so `send` never blocks. `recv` on an empty channel `yield`s the coroutine (a *cooperative* block) and retries when the scheduler comes back to it, so a sender coroutine gets a turn in between. `recv` and `try_recv` return an object to inspect or `match`: `${value: v}` for a message, `${closed: true}` once the channel is closed and drained, and `${empty: true}` from `try_recv` when nothing is ready.

```tigr
LC := import 'LocalChannel';

ch := LC.new();
go fn() {
    for (i, 1..=3) { LC.send(ch, i) };
    LC.close(ch)
};
sum := 0;
draining := true;
while draining {
    msg := LC.recv(ch);
    if msg.closed == true {
        draining = false
    } else {
        sum = sum + msg.value
    }
};
print(sum);                     // => 6
```

A `recv` with no coroutine that could ever send spins forever, exactly as a cross-actor `recv` on a never-fed `Channel` blocks forever. Hand work to a `go` coroutine before you `recv`.

## Functions

### `new() -> LocalChannel`

Creates an empty, unbounded intra-actor channel.

**Returns:** a new `LocalChannel`.

```tigr
LC := import 'LocalChannel';

ch := LC.new();
print(type(ch));                // => local_channel
```

### `send(ch, msg) -> Null`

Enqueues `msg` by value, with no copy, since coroutines share the heap. Never blocks.

- `ch` *(LocalChannel)*: the channel to send on.
- `msg` *(value)*: the message to enqueue.

**Returns:** `null`.
**Raises:** `channel_closed` if the channel is already closed.

```tigr
LC := import 'LocalChannel';

ch := LC.new();
LC.send(ch, 7);
print(LC.try_recv(ch).value);   // => 7
```

### `recv(ch) -> Object`

Returns the next message, cooperatively waiting for one. While the channel is empty and open, `recv` `yield`s the coroutine so other green threads (a sender) can run.

- `ch` *(LocalChannel)*: the channel to receive from.

**Returns:** `${value: v}` for the next message, or `${closed: true}` once the channel is closed and every buffered message has been drained.

```tigr
LC := import 'LocalChannel';

ch := LC.new();
go fn() { LC.send(ch, 'hello') };
print(LC.recv(ch).value);       // => hello
```

### `try_recv(ch) -> Object`

Checks for a message without ever blocking or yielding.

- `ch` *(LocalChannel)*: the channel to poll.

**Returns:** `${value: v}` for a ready message, `${closed: true}` once the channel is closed and drained, or `${empty: true}` when nothing is ready right now.

```tigr
LC := import 'LocalChannel';

ch := LC.new();
print(LC.try_recv(ch).empty);   // => true
LC.send(ch, 42);
print(LC.try_recv(ch).value);   // => 42
```

### `close(ch) -> Null`

Closes the channel. A `recv` or `try_recv` after the buffer drains then returns `${closed: true}`, and a `send` on a closed channel raises `channel_closed`.

- `ch` *(LocalChannel)*: the channel to close.

**Returns:** `null`.

```tigr
LC := import 'LocalChannel';

ch := LC.new();
LC.close(ch);
print(try { LC.send(ch, 1) } catch (e) { e.kind });   // => channel_closed
print(LC.recv(ch).closed);                            // => true
```

## See also

- [Concurrency](../language/concurrency.md): `go`, `yield`, `join`, and green threads
- [Channel](channel.md): the cross-actor counterpart, which deep-copies messages
- [Control flow](../language/control-flow.md): `match`, useful for inspecting `recv` results
