# `Net`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#net-v015)

The `Net` module does TCP, UDP, and TLS networking. A socket is a value type in its own right: `type(s)` is `'socket'`, and a socket is sendable across actor boundaries the same way a channel is, so an accepted connection can be passed into a `spawn`ed per-connection handler. Import the module with `Net := import 'Net'`. Reads come in two layers. The low-level `read(sock, n)` returns up to `n` bytes, with an empty `Bytes` meaning end-of-stream. On top of it sit the framed helpers `read_exact`, `read_line`, `read_until`, and `read_all`; the socket carries an internal buffer, so a helper that reads past a frame boundary keeps the surplus for the next call. A failure raises a structured `${kind, message}` error, where `kind` is one of `timeout`, `closed`, `eof`, `refused`, `dns`, `tls`, `addr_in_use`, `decode`, or `io`.

The waiting calls are offloaded when they run inside a green thread, so a coroutine waiting on the network does not stall the actor's siblings (see [concurrency](../language/concurrency.md)). Steady-state socket I/O (`accept`, `read`, `write`, `read_exact`, `read_line`, `read_until`, `read_all`, and `recv_from`) is driven on a single async-I/O reactor thread, so one actor can keep tens of thousands of connections open at once. `connect`, `connect_tls`, and `send_to` go to a worker pool instead, since each may need a blocking DNS lookup. The non-waiting calls (`listen`, `listen_tls`, `bind`, `local_addr`, `peer_addr`, `set_timeout`, `close`) run inline.

```tigr
Net := import 'Net';

listener := Net.listen('127.0.0.1', 0);
print(Net.local_addr(listener).host);       // => 127.0.0.1
```

## Functions

### `listen(host, port) -> Socket`

Creates a TCP listener bound to `host:port`. Pass port `0` to let the OS pick a free port, then read it back with `local_addr`.

- `host` *(String)*: the local address to bind, such as `'127.0.0.1'` or `'0.0.0.0'`.
- `port` *(Int)*: the port, from 0 to 65535.

**Returns:** a listener `Socket`.
**Raises:** a structured error, for example `addr_in_use` if the port is taken.

```tigr
Net := import 'Net';

listener := Net.listen('127.0.0.1', 0);
print(Net.local_addr(listener).host);       // => 127.0.0.1
```

### `accept(listener) -> Socket`

Blocks until the next inbound connection arrives, then returns it.

- `listener` *(Socket)*: a listener from `listen`.

**Returns:** a connected stream `Socket`.
**Raises:** `closed` if the listener is closed, including by another actor while this `accept` is waiting.

```tigr
Net  := import 'Net';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net := import 'Net';
    conn := Net.accept(listener);
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
print(Net.peer_addr(client).port == port);      // => true
Net.close(client);
join(server);
```

### `connect(host, port) -> Socket`

Opens a TCP stream to `host:port`.

- `host` *(String)*: the remote host name or address.
- `port` *(Int)*: the remote port, from 0 to 65535.

**Returns:** a connected stream `Socket`.
**Raises:** a structured error, for example `refused` if nothing is listening, or `dns` if the host name does not resolve.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net   := import 'Net';
    Bytes := import 'Bytes';
    conn := Net.accept(listener);
    line := Net.read_line(conn);
    Net.write(conn, Bytes.from_string('hello, ' + line));
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
Net.write(client, Bytes.from_string('world\n'));
reply := Net.read_all(client);
print(Bytes.to_string(reply));                  // => hello, world
Net.close(client);
join(server);
```

### `connect_tls(host, port, [ca_pem]) -> Socket`

Opens a TLS-encrypted stream. The `host` is also the name checked against the server's certificate.

- `host` *(String)*: the remote host name, verified against the server certificate.
- `port` *(Int)*: the remote port, from 0 to 65535.
- `ca_pem` *(String, optional)*: extra trusted root certificates, as PEM content. They are trusted in addition to the operating system trust store — pass this to reach a private-CA or self-signed service (for example a tigr `listen_tls` server).

**Returns:** a connected, encrypted stream `Socket`.
**Raises:** a structured error, for example `tls` if the certificate fails verification, or `dns` if the host name does not resolve.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

conn := Net.connect_tls('example.com', 443);
Net.write(conn, Bytes.from_string('GET / HTTP/1.0\r\nHost: example.com\r\n\r\n'));
status := Net.read_line(conn);
print(status);                          // => HTTP/1.1 200 OK
Net.close(conn);
```

### `listen_tls(host, port, cert_pem, key_pem) -> Socket`

Creates a TLS server listener bound to `host:port`. `accept` on the returned listener performs the TCP accept *and* the TLS handshake, so it yields an already-encrypted server `Socket` — the same kind `read` / `write` / `close` already handle. Because `accept` is transparent, `Http.serve(Net.listen_tls(...), handler)` is an HTTPS server with no other change.

- `host` *(String)*: the local address to bind. Pass port `0` to let the OS pick a free port, then read it back with `local_addr`.
- `port` *(Int)*: the local port, from 0 to 65535.
- `cert_pem` *(String)*: the server certificate chain, as PEM content (not a file path).
- `key_pem` *(String)*: the matching private key, as PEM content (not a file path).

**Returns:** a TLS listener `Socket`.
**Raises:** `tls` if the certificate or key PEM is malformed, the chain is empty, or the certificate and key do not match; `addr_in_use` if the port is taken.

```tigr
Net := import 'Net';

// `cert` and `key` are PEM strings — read them from a file or embed them.
listener := Net.listen_tls('127.0.0.1', 8443, cert, key);
conn := Net.accept(listener);           // a TLS server socket
request := Net.read_line(conn);
Net.close(conn);
Net.close(listener);
```

### `bind(host, port) -> Socket`

Creates a UDP datagram socket bound to `host:port`.

- `host` *(String)*: the local address to bind.
- `port` *(Int)*: the local port, from 0 to 65535. Pass `0` to let the OS pick one.

**Returns:** a UDP `Socket`.
**Raises:** a structured error, for example `addr_in_use` if the port is taken.

```tigr
Net := import 'Net';

sock := Net.bind('127.0.0.1', 0);
print(Net.local_addr(sock).host);               // => 127.0.0.1
```

### `send_to(sock, bytes, host, port) -> Int`

Sends one UDP datagram to `host:port`.

- `sock` *(Socket)*: a UDP socket from `bind`.
- `bytes` *(Bytes)*: the datagram payload.
- `host` *(String)*: the destination host.
- `port` *(Int)*: the destination port.

**Returns:** the number of bytes sent, as an `Int`.
**Raises:** a structured error if the address cannot be resolved or the send fails.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

a := Net.bind('127.0.0.1', 0);
b := Net.bind('127.0.0.1', 0);
addr := Net.local_addr(b);
print(Net.send_to(a, Bytes.from_string('ping'), addr.host, addr.port));     // => 4
```

### `recv_from(sock, n) -> Object`

Receives one UDP datagram, up to `n` bytes.

- `sock` *(Socket)*: a UDP socket from `bind`.
- `n` *(Int)*: the most bytes to accept.

**Returns:** an object `${data, host, port}`, where `data` is a `Bytes` and `host`/`port` identify the sender.
**Raises:** a structured error if the receive fails.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

a := Net.bind('127.0.0.1', 0);
b := Net.bind('127.0.0.1', 0);
addr := Net.local_addr(b);
Net.send_to(a, Bytes.from_string('ping'), addr.host, addr.port);
msg := Net.recv_from(b, 64);
print(Bytes.to_string(msg.data));               // => ping
```

### `read(sock, n) -> Bytes`

Reads up to `n` bytes from a stream. This is the low-level read; it returns as soon as any data is available, which may be fewer than `n` bytes.

- `sock` *(Socket)*: a connected stream socket.
- `n` *(Int)*: the most bytes to read.

**Returns:** a `Bytes` of up to `n` bytes. An empty `Bytes` means the stream has ended.
**Raises:** a structured error such as `closed` or `timeout`.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net   := import 'Net';
    Bytes := import 'Bytes';
    conn := Net.accept(listener);
    Net.write(conn, Bytes.from_string('abc'));
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
chunk := Net.read(client, 64);
print(Bytes.to_string(chunk));                  // => abc
Net.close(client);
join(server);
```

### `write(sock, bytes) -> Int`

Writes every byte of `bytes` to a stream.

- `sock` *(Socket)*: a connected stream socket.
- `bytes` *(Bytes)*: the data to write.

**Returns:** the number of bytes written, as an `Int`.
**Raises:** a structured error such as `closed` or `timeout`.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net := import 'Net';
    conn := Net.accept(listener);
    Net.read_all(conn);
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
print(Net.write(client, Bytes.from_string('hello')));   // => 5
Net.close(client);
join(server);
```

### `read_exact(sock, n) -> Bytes`

Reads exactly `n` bytes, blocking until all of them have arrived.

- `sock` *(Socket)*: a connected stream socket.
- `n` *(Int)*: the exact number of bytes to read.

**Returns:** a `Bytes` of exactly `n` bytes.
**Raises:** `eof` if the stream ends before `n` bytes arrive, or another structured error.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net   := import 'Net';
    Bytes := import 'Bytes';
    conn := Net.accept(listener);
    Net.write(conn, Bytes.from_string('abcdef'));
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
head := Net.read_exact(client, 3);
print(Bytes.to_string(head));                   // => abc
Net.close(client);
join(server);
```

### `read_line(sock) -> String`

Reads one line, terminated by `\n`. A trailing `\r\n` or `\n` is stripped from the returned string.

- `sock` *(Socket)*: a connected stream socket.

**Returns:** the line as a `String`, or `null` at end-of-stream.
**Raises:** `decode` if the line is not valid UTF-8, or another structured error.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net   := import 'Net';
    Bytes := import 'Bytes';
    conn := Net.accept(listener);
    Net.write(conn, Bytes.from_string('one\ntwo\n'));
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
first := Net.read_line(client);
print(first);                           // => one
Net.close(client);
join(server);
```

### `read_until(sock, byte) -> Bytes`

Reads up to and including the next occurrence of `byte`.

- `sock` *(Socket)*: a connected stream socket.
- `byte` *(Int)*: the delimiter byte, in `0..=255`.

**Returns:** a `Bytes` ending with `byte` (the delimiter is included), or `null` at end-of-stream. Trailing data with no delimiter comes back as a final chunk.
**Raises:** a structured error such as `closed` or `timeout`.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net   := import 'Net';
    Bytes := import 'Bytes';
    conn := Net.accept(listener);
    Net.write(conn, Bytes.from_string('field;rest'));
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
field := Net.read_until(client, 59);    // 59 is ';'
print(Bytes.to_string(field));                  // => field;
Net.close(client);
join(server);
```

### `read_all(sock) -> Bytes`

Reads every remaining byte until end-of-stream.

- `sock` *(Socket)*: a connected stream socket.

**Returns:** a `Bytes` holding all remaining data.
**Raises:** a structured error such as `closed` or `timeout`.

```tigr
Net   := import 'Net';
Bytes := import 'Bytes';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net   := import 'Net';
    Bytes := import 'Bytes';
    conn := Net.accept(listener);
    Net.write(conn, Bytes.from_string('payload'));
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
all := Net.read_all(client);
print(Bytes.to_string(all));                    // => payload
Net.close(client);
join(server);
```

### `local_addr(sock) -> Object`

Returns the socket's own bound address.

- `sock` *(Socket)*: any socket.

**Returns:** an object `${host, port}`.

```tigr
Net := import 'Net';

listener := Net.listen('127.0.0.1', 0);
print(Net.local_addr(listener).host);           // => 127.0.0.1
```

### `peer_addr(sock) -> Object`

Returns the address of the connected peer.

- `sock` *(Socket)*: a connected stream socket.

**Returns:** an object `${host, port}`.
**Raises:** a structured error if the socket is not connected.

```tigr
Net  := import 'Net';

listener := Net.listen('127.0.0.1', 0);
port := Net.local_addr(listener).port;
server := spawn fn() {
    Net := import 'Net';
    conn := Net.accept(listener);
    Net.close(conn);
    Net.close(listener);
    null
};
client := Net.connect('127.0.0.1', port);
print(Net.peer_addr(client).host);              // => 127.0.0.1
Net.close(client);
join(server);
```

### `set_timeout(sock, ms) -> null`

Bounds subsequent reads and writes on `sock` to `ms` milliseconds. A read or write that runs over raises `timeout`.

- `sock` *(Socket)*: any socket.
- `ms` *(Int)*: the timeout in milliseconds. A value of `0` or below clears the timeout, so operations block indefinitely.

**Returns:** `null`.

```tigr
Net := import 'Net';

sock := Net.bind('127.0.0.1', 0);
Net.set_timeout(sock, 500);
caught := try { Net.recv_from(sock, 16) } catch (e) { e.kind };
print(caught);                                  // => timeout
```

### `close(sock) -> null`

Closes the socket. The call is idempotent. Closing a socket unblocks a reader stuck mid-`read`, and unblocks an actor stuck in `accept` on a listener, which then raises `closed`.

- `sock` *(Socket)*: the socket to close.

**Returns:** `null`.

```tigr
Net := import 'Net';

sock := Net.listen('127.0.0.1', 0);
Net.close(sock);
Net.close(sock);                        // idempotent, no error
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#net-v015): the authoritative spec for `Net`
- [Bytes](bytes.md): the buffer type that socket reads and writes use
- [Errors](../language/errors.md): catching the structured `${kind, message}` errors
