# `Http`

> Pure-tigr source module, `stdlib/Http.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#http-v015)

`Http` is an HTTP/1.1 client and server helper, layered on the native `Net`, `String`, `Bytes`, and `JSON` modules. It exposes no runtime type of its own; requests and responses are plain objects. It is ambient, so a bare module name works without an `import`.

The client side is `request(opts)` plus the `get`, `post`, `put`, `delete`, `head`, and `patch` wrappers. A request returns `${status, status_text, headers, body}`, where `headers` keys are lowercased (a duplicate header collapses, last value wins) and `body` is always `Bytes`. Decode the body with the `text(resp)` and `json(resp)` helpers. 3xx redirects are followed automatically, capped at 10.

The server side is the low-level pair `read_request(sock)` and `write_response(sock, resp)`, plus `serve(listener, handler)`, an accept loop that `spawn`s one actor per connection. Because a spawned closure is deep-copied across the actor boundary, the `handler` passed to `serve` must be sendable. Stdlib modules are ambient inside the spawned actor, so the body reaches for `JSON`, `String`, and the rest directly with no `import`; what it cannot do is capture a non-sendable value (a module object or a native function) from the enclosing scope. v1 has no keep-alive, so every request sends `Connection: close`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);

server := spawn fn() {
    Http.serve(listener, fn(req) { 'hello ' + req.path })
};

resp := Http.get(base + '/world');
print(resp.status);         // => 200
print(Http.text(resp));     // => hello /world
Net.close(listener);
join(server);
```

## Functions

| Function | Summary |
|----------|---------|
| [`request(opts) -> Object`](#requestopts---object) | Performs one HTTP request, following 3xx redirects automatically. |
| [`get(url, opts?) -> Object`](#geturl-opts---object) | Performs a GET request. |
| [`post(url, body?, opts?) -> Object`](#posturl-body-opts---object) | Performs a POST request with an optional body. |
| [`put(url, body?, opts?) -> Object`](#puturl-body-opts---object) | Performs a PUT request with an optional body. |
| [`delete(url, opts?) -> Object`](#deleteurl-opts---object) | Performs a DELETE request. |
| [`head(url, opts?) -> Object`](#headurl-opts---object) | Performs a HEAD request. |
| [`patch(url, body?, opts?) -> Object`](#patchurl-body-opts---object) | Performs a PATCH request with an optional body. |
| [`text(resp) -> String`](#textresp---string) | Decodes a response or request body as UTF-8 text. |
| [`json(resp) -> value`](#jsonresp---value) | Parses a response or request body as JSON. |
| [`read_request(sock) -> Object`](#read_requestsock---object) | Reads one HTTP request from an accepted connection. |
| [`write_response(sock, resp) -> Int`](#write_responsesock-resp---int) | Writes an HTTP response to a connection. |
| [`serve(listener, handler) -> Null`](#servelistener-handler---null) | Runs an accept loop on `listener`, handing each connection to its own `spawn`ed actor. |


### `request(opts) -> Object`

Performs one HTTP request, following 3xx redirects automatically.

- `opts` *(Object)*: the request options `${url, method, headers, body, max_redirects, follow_redirects, timeout}`. Only `url` is required. `method` defaults to `'GET'`; `body` may be a `String` or `Bytes`; `max_redirects` defaults to 10; `follow_redirects` defaults to `true`; `timeout` is in milliseconds and bounds each socket read or write.

**Returns:** `${status, status_text, headers, body}`, with `headers` lowercased and `body` as `Bytes`.
**Raises:** a structured error on a connection or protocol failure, `too_many_redirects` past the cap, or `unsupported_scheme` for a non-http/https URL.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) { 'agent=' + str(req.headers['user-agent']) })
};

resp := Http.request(${
    url: base + '/',
    method: 'GET',
    headers: ${'user-agent': 'tigr-doc'},
});
print(Http.text(resp));     // => agent=tigr-doc
Net.close(listener);
join(server);
```

### `get(url, opts?) -> Object`

Performs a GET request. A convenience wrapper over `request`.

- `url` *(String)*: the URL to fetch.
- `opts` *(Object, optional)*: extra request options, merged with `url` and the method.

**Returns:** the response object, the same shape `request` returns.
**Raises:** the same errors as `request`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) { req.method + ' ' + req.path })
};

print(Http.text(Http.get(base + '/page')));     // => GET /page
Net.close(listener);
join(server);
```

### `post(url, body?, opts?) -> Object`

Performs a POST request with an optional body.

- `url` *(String)*: the URL to post to.
- `body` *(String or Bytes, optional)*: the request body.
- `opts` *(Object, optional)*: extra request options.

**Returns:** the response object.
**Raises:** the same errors as `request`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) {
        ${status: 201, headers: ${}, body: JSON.stringify(${echo: JSON.parse(Http.text(req)).name})}
    })
};

resp := Http.post(base + '/users', JSON.stringify(${name: 'ada'}));
print(resp.status);             // => 201
print(Http.json(resp).echo);    // => ada
Net.close(listener);
join(server);
```

### `put(url, body?, opts?) -> Object`

Performs a PUT request with an optional body.

- `url` *(String)*: the target URL.
- `body` *(String or Bytes, optional)*: the request body.
- `opts` *(Object, optional)*: extra request options.

**Returns:** the response object.
**Raises:** the same errors as `request`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) {
        'got: ' + Http.text(req)
    })
};

print(Http.text(Http.put(base + '/item/1', 'updated')));   // => got: updated
Net.close(listener);
join(server);
```

### `delete(url, opts?) -> Object`

Performs a DELETE request.

- `url` *(String)*: the target URL.
- `opts` *(Object, optional)*: extra request options.

**Returns:** the response object.
**Raises:** the same errors as `request`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) { req.method + ' ' + req.path })
};

print(Http.text(Http.delete(base + '/item/1')));    // => DELETE /item/1
Net.close(listener);
join(server);
```

### `head(url, opts?) -> Object`

Performs a HEAD request. The server sends no body for a HEAD, so the response `body` is always empty.

- `url` *(String)*: the target URL.
- `opts` *(Object, optional)*: extra request options.

**Returns:** the response object, with an empty `body`.
**Raises:** the same errors as `request`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) { 'body text' })
};

resp := Http.head(base + '/');
print(resp.status);     // => 200
print(#resp.body);      // => 0
Net.close(listener);
join(server);
```

### `patch(url, body?, opts?) -> Object`

Performs a PATCH request with an optional body.

- `url` *(String)*: the target URL.
- `body` *(String or Bytes, optional)*: the request body.
- `opts` *(Object, optional)*: extra request options.

**Returns:** the response object.
**Raises:** the same errors as `request`.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) {
        'patched: ' + Http.text(req)
    })
};

print(Http.text(Http.patch(base + '/item/1', 'x')));    // => patched: x
Net.close(listener);
join(server);
```

### `text(resp) -> String`

Decodes a response or request body as UTF-8 text.

- `resp` *(Object)*: a response from a client call, or a request from `read_request`. Any object with a `Bytes` `body` field works.

**Returns:** the body decoded as a `String`.

```tigr
print(Http.text(${body: Bytes.from_string('hello')}));   // => hello
```

### `json(resp) -> value`

Parses a response or request body as JSON.

- `resp` *(Object)*: a response or request whose `body` is JSON text in `Bytes`.

**Returns:** the parsed JSON value.
**Raises:** a JSON parse error when the body is not valid JSON.

```tigr
src := JSON.stringify(${n: 5});
print(Http.json(${body: Bytes.from_string(src)}).n);    // => 5.0
```

### `read_request(sock) -> Object`

Reads one HTTP request from an accepted connection. The body is read only when a `Content-Length` or `Transfer-Encoding` header is present, since otherwise the read would block waiting for end-of-stream.

- `sock` *(socket)*: a connection from `Net.accept`.

**Returns:** `${method, path, query, headers, body}`, where `query` is an `Object` of parsed query parameters and `body` is `Bytes`.
**Raises:** an `eof` error when the connection closes before a request line arrives.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    conn := Net.accept(listener);
    req := Http.read_request(conn);
    Http.write_response(conn, ${status: 200, headers: ${}, body: 'method was ' + req.method});
    Net.close(conn);
    Net.close(listener)
};

print(Http.text(Http.get(base + '/')));     // => method was GET
join(server);
```

### `write_response(sock, resp) -> Int`

Writes an HTTP response to a connection. `Content-Length` and `Connection: close` are always set.

- `sock` *(socket)*: the connection to write to.
- `resp` *(Object)*: `${status, headers, body}`. `status` defaults to 200; `body` may be a `String` or `Bytes`.

**Returns:** the number of bytes written.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    conn := Net.accept(listener);
    Http.read_request(conn);
    Http.write_response(conn, ${status: 201, headers: ${}, body: 'created'});
    Net.close(conn);
    Net.close(listener)
};

resp := Http.get(base + '/');
print(resp.status);         // => 201
print(Http.text(resp));     // => created
join(server);
```

### `serve(listener, handler) -> Null`

Runs an accept loop on `listener`, handing each connection to its own `spawn`ed actor. A `handler` returning a `String` becomes a `200 text/plain` response; an `Object` is sent as the response as-is. A handler that raises yields a best-effort `500`, so one bad request never stops the loop. `serve` runs until its `listener` is closed: `close(listener)` from any actor makes the next `accept` raise `closed`, which `serve` catches and returns from cleanly.

- `listener` *(socket)*: a listening socket from `Net.listen`, or from `Net.listen_tls`, which makes `serve` an HTTPS server. A TLS listener's `accept` yields encrypted sockets transparently, so neither `serve` nor the `handler` changes.
- `handler` *(Function)*: a sendable function taking a request and returning a `String` or a response `Object`. It runs in its own spawned actor, where stdlib modules are ambient, so it uses them directly; it must not capture non-sendable values from the enclosing scope.

**Returns:** `null`, once the listener is closed.

```tigr
listener := Net.listen('127.0.0.1', 0);
base := 'http://127.0.0.1:' + str(Net.local_addr(listener).port);
server := spawn fn() {
    Http.serve(listener, fn(req) {
        ${status: 200, headers: ${'content-type': 'text/plain'}, body: 'echo ' + req.path}
    })
};

print(Http.text(Http.get(base + '/greet')));    // => echo /greet
Net.close(listener);
join(server);
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#http-v015): the authoritative spec for `Http`
- [Url](url.md): URL and query-string parsing, used by `Http` internally
- [Channel](channel.md): the message-passing primitive behind the `spawn`ed per-connection actors
- [LANGUAGE.md Appendix N](../../LANGUAGE.md#appendix-n--changes-in-v015-http--url): the v0.15 notes covering `Http` and `Url`
