# `Channel`

> Pure-tigr source module, `stdlib/Channel.tg`
> Spec: [LANGUAGE.md Appendix L](../../LANGUAGE.md#appendix-l--changes-in-v014)

A `Channel` carries messages between actors (the v0.14 concurrency model). It is the one reference type that crosses actor threads: a value sent through a channel is deep-copied into the receiving actor's heap. Channels are bidirectional, so any holder may both send and receive. `type(ch)` is `'channel'`, and a `Channel` is not JSON-serializable. Import it as `Channel := import 'Channel'`.

`Channel.new()` is unbounded. `Channel.new(n)` bounds the buffer at `n` messages, so `send` blocks (backpressure) while the buffer is full. `recv` and `try_recv` return an object to inspect or `match`: `${value: v}` for a message, `${closed: true}` once the channel is closed and drained, and `${empty: true}` from `try_recv` when nothing is ready. The functions are thin re-exports of the native `_NativeChannel` backend, except `new`, which defaults the capacity.

```tigr
Channel := import 'Channel';
Task    := import 'Task';

ch := Channel.new();
producer := spawn fn() {
    C := import 'Channel';
    C.send(ch, 'ping');
    C.send(ch, 'pong');
    C.close(ch)
};
print(Channel.recv(ch).value);          // => ping
print(Channel.recv(ch).value);          // => pong
print(Channel.recv(ch).closed);         // => true
Task.join(producer);
```

## Functions

### `new(capacity?) -> Channel`

Creates a channel. With no argument the channel is unbounded. With a positive `Int` capacity, the buffer is bounded at that many messages, and `send` blocks once it is full.

- `capacity` *(Int, optional)*: the buffer bound. When omitted, the channel is unbounded.

**Returns:** a new `Channel`.

```tigr
Channel := import 'Channel';

ch := Channel.new(2);
print(type(ch));                // => channel
```

### `send(ch, msg) -> Null`

Enqueues `msg`, deep-copying it into the channel. On a full bounded channel `send` blocks until space frees up.

- `ch` *(Channel)*: the channel to send on.
- `msg` *(value)*: the message to enqueue.

**Returns:** `null`.
**Raises:** `channel_closed` if the channel is already closed, or `not_sendable` / `cycle` if `msg` cannot cross the heap boundary.

```tigr
Channel := import 'Channel';

ch := Channel.new();
Channel.send(ch, 7);
print(Channel.try_recv(ch).value);      // => 7
```

### `recv(ch) -> Object`

Blocks for the next message.

- `ch` *(Channel)*: the channel to receive from.

**Returns:** `${value: v}` for the next message, or `${closed: true}` once the channel is closed and every buffered message has been drained.

```tigr
Channel := import 'Channel';
Task    := import 'Task';

ch := Channel.new(4);
t := spawn fn() {
    C := import 'Channel';
    for (i, 1..=3) { C.send(ch, i * 10) };
    C.close(ch)
};
sum := 0;
drained := false;
while !drained {
    msg := Channel.recv(ch);
    if msg.closed == true {
        drained = true
    } else {
        sum = sum + msg.value
    }
};
print(sum);                     // => 60
Task.join(t);
```

### `try_recv(ch) -> Object`

Checks for a message without ever blocking.

- `ch` *(Channel)*: the channel to poll.

**Returns:** `${value: v}` for a ready message, `${closed: true}` once the channel is closed and drained, or `${empty: true}` when nothing is ready right now.

```tigr
Channel := import 'Channel';

ch := Channel.new();
print(Channel.try_recv(ch).empty);      // => true
Channel.send(ch, 42);
print(Channel.try_recv(ch).value);      // => 42
```

### `close(ch) -> Null`

Closes the channel, waking every blocked sender and receiver. A `recv` after the buffer drains then returns `${closed: true}`, and a `send` on a closed channel raises `channel_closed`.

- `ch` *(Channel)*: the channel to close.

**Returns:** `null`.

```tigr
Channel := import 'Channel';

ch := Channel.new();
Channel.close(ch);
print(try { Channel.send(ch, 1) } catch (e) { e.kind });   // => channel_closed
print(Channel.recv(ch).closed);                            // => true
```

## See also

- [LANGUAGE.md Appendix L](../../LANGUAGE.md#appendix-l--changes-in-v014): the v0.14 concurrency spec, including `Channel`
- [Control flow](../language/control-flow.md): `match`, useful for inspecting `recv` results
- [Http](http.md): a module whose server uses `spawn` per connection
