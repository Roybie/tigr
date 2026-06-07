# `WS`

> Source module `stdlib/WS.tg` on native targets; a browser-`WebSocket` backend (`src/vm/native_modules/ws_web.rs`) on web

`WS` is a WebSocket (RFC 6455) client. On native targets it is pure tigr, layered on `Net`, `Bytes`, `Url`, `Random`, and `String`; in a browser the same API is backed by the host's native `WebSocket`. It is ambient, so a bare module name works without an `import`. WebSockets are the one transport that spans every target a tigr program can run on, native and web, so a networked game writes its messaging against `WS` once.

The API is poll-based, so it drops straight into a frame loop with no callbacks and no extra threads. `connect(url)` opens a connection and returns an opaque handle; `send(ws, data)` queues a frame; `poll(ws)` and `drain(ws)` return inbound messages without ever blocking; `state(ws)` reports liveness; `close(ws)` shuts the connection down. An inbound text message arrives as a `String` and a binary message as `Bytes`, so `type(msg)` tells them apart.

```tigr
ws := WS.connect('wss://echo.example.com');
WS.send(ws, 'hello');

// ... each frame of a game loop:
for (msg, WS.drain(ws)) {
    if type(msg) == 'string' { print('text:', msg) }
    else { print('binary:', #msg, 'bytes') }
}

if WS.state(ws) == 'closed' { print('peer hung up') }
WS.close(ws);
```

The client masks every frame it sends, as RFC 6455 requires, reassembles fragmented messages, and answers pings with pongs on its own. A `wss://` url connects over `Net.connect_tls`, inheriting the OS trust store. There is no permessage-deflate, and the server's `Sec-WebSocket-Accept` is not verified (TLS provides the real security).

## Platform notes

On native targets `connect` performs the TCP and WebSocket handshakes before it returns, so the handle is already `open`. In a browser the underlying `WebSocket` connects asynchronously, so `state(ws)` may briefly report `'connecting'` before `'open'`. Code that works on every target should treat a connection as ready only once `state(ws) == 'open'`, which is always already true on native.

The browser backend talks to the host through five `env` imports the host supplies (purr's miniquad loader does this with a small JS plugin; `web/tigr_ws.js` is the reference implementation). The ABI, for a host author wiring it up:

```
tigr_ws_connect(url_ptr, url_len) -> i32   // handle id >= 0, or < 0 on a sync reject
tigr_ws_send(id, ptr, len, is_binary)      // is_binary: 1 binary, 0 text
tigr_ws_poll(id, out_ptr, out_cap) -> i32  // framed length, 0 = none, < 0 = closed-and-empty
tigr_ws_state(id) -> i32                    // 0 connecting, 1 open, 2 closed
tigr_ws_close(id)
```

A polled message is written as one kind-tag byte (`0` text, `1` binary) followed by the payload; a return greater than `out_cap` means the buffer was too small and nothing was consumed, so the caller retries with a buffer of that size.

## Functions

| Function | Summary |
|----------|---------|
| [`connect(url) -> handle`](#connecturl---handle) | Opens a WebSocket connection and runs the handshake. |
| [`send(ws, data) -> Null`](#sendws-data---null) | Sends one message: a `String` as a text frame, `Bytes` as binary. |
| [`poll(ws) -> value`](#pollws---value) | Returns the next inbound message, or `null` if none is ready. |
| [`drain(ws) -> Array`](#drainws---array) | Returns every message buffered this tick, as an array. |
| [`state(ws) -> String`](#statews---string) | Reports the connection state. |
| [`close(ws) -> Null`](#closews---null) | Closes the connection. |

### `connect(url) -> handle`

Opens a WebSocket connection to `url` and performs the RFC 6455 handshake.

- `url` *(String)*: a `ws://` or `wss://` URL. `wss://` connects over TLS. The port defaults to 80 for `ws://` and 443 for `wss://`.

**Returns:** an opaque connection handle to pass to the other `WS` functions. On native targets the connection is already `open`; in a browser it may still be `connecting`.

**Raises:** `unsupported_scheme` for a non-`ws`/`wss` URL, `handshake` if the server does not return `101 Switching Protocols`, or `closed` if the connection drops during the handshake.

```tigr
ws := WS.connect('wss://echo.example.com/socket');
```

### `send(ws, data) -> Null`

Sends one message frame. A `String` is sent as a text frame, `Bytes` as a binary frame. The frame is masked, as the protocol requires of a client.

- `ws` *(handle)*: a connection from `connect`.
- `data` *(String | Bytes)*: the message payload.

**Returns:** `null`.

**Raises:** `closed` if the connection is not open.

```tigr
WS.send(ws, 'a text message');
WS.send(ws, Bytes.from_string('binary'));
```

### `poll(ws) -> value`

Returns the next inbound message, or `null` if none has arrived. Never blocks: it reads whatever bytes are available, parses any complete frames, answers pings, and hands back the next message. A text message is a `String`, a binary message is `Bytes`.

- `ws` *(handle)*: a connection from `connect`.

**Returns:** the next message (`String` or `Bytes`), or `null` if none is ready this tick.

```tigr
msg := WS.poll(ws);
if msg != null { print('got', msg) }
```

### `drain(ws) -> Array`

Returns every message buffered this tick, in arrival order, as an array (empty if none). Like `poll` it never blocks; it is the natural fit for a frame loop that wants to process all pending messages at once.

- `ws` *(handle)*: a connection from `connect`.

**Returns:** an `Array` of messages, each a `String` or `Bytes`.

```tigr
for (msg, WS.drain(ws)) { handle(msg) }
```

### `state(ws) -> String`

Reports the connection state.

- `ws` *(handle)*: a connection from `connect`.

**Returns:** `'connecting'`, `'open'`, or `'closed'`. On native targets a fresh handle is already `'open'`; `'connecting'` only occurs in a browser.

```tigr
if WS.state(ws) == 'open' { WS.send(ws, 'ready') }
```

### `close(ws) -> Null`

Closes the connection, sending a close frame if it is still open. Idempotent.

- `ws` *(handle)*: a connection from `connect`.

**Returns:** `null`.

```tigr
WS.close(ws);
```
