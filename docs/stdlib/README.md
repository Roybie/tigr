# Standard library

Tigr's standard library is 23 modules plus a handful of global builtin functions. Every module is **ambient**: you reach it by name, with no `import`.

```tigr
print(Math.sqrt(144));      // => 12.0
print(String.upper('hi'));  // => HI
```

You can still write `M := import 'Math'` to alias a module or to be explicit, and a local binding of the same name shadows the ambient module. Only local files and third-party code need an `import` (by path). The rule is simple: a capitalized stdlib name is always in scope; anything you wrote yourself you import.

Modules come in two kinds. **Source modules** are written in Tigr itself and live in `stdlib/*.tg`; **native modules** are implemented in Rust. The kind rarely matters when you use a module, but each page notes it.

## Builtins

- [Builtins](builtins.md): `print`, `str`, `num`, `int`, `float`, `bool`, `type`, `gc`, `rand`, `floor`, `ceil`, `join`, `wait`. Always in scope, no import needed.

## Collections and data

- [Array](array.md): array utilities, both pure helpers and in-place mutation
- [Iter](iter.md): lazy iterators and the combinators that drive them
- [Object](object.md): keys, values, entries, and merging for objects
- [Map](map.md): an insertion-ordered key/value collection
- [Set](set.md): an insertion-ordered collection of unique values
- [String](string.md): text search, splitting, casing, formatting
- [Bytes](bytes.md): a mutable byte buffer with integer pack and unpack
- [BigInt](bigint.md): arbitrary-precision integers
- [JSON](json.md): parse and stringify JSON

## Numbers and time

- [Math](math.md): constants and numeric functions
- [Random](random.md): a seedable pseudo-random number generator
- [Time](time.md): a monotonic clock
- [DateTime](datetime.md): UTC calendar dates and formatting

## System

- [IO](io.md): file and directory operations
- [Os](os.md): process arguments, environment, and subprocesses
- [Path](path.md): path string manipulation

## Concurrency

- [Channel](channel.md): typed message channels between actors
- [LocalChannel](localchannel.md): no-copy message channels between green threads of one actor
- [Deferred](deferred.md): a write-once result a coroutine waits on and anything can complete

## Networking

- [Net](net.md): TCP, UDP, and TLS sockets
- [Url](url.md): URL parsing, building, and percent-coding
- [Http](http.md): an HTTP/1.1 client and server helpers
- [WS](ws.md): a WebSocket client, the one transport shared by native and web

## Testing

- [Test](test.md): the test framework used by `tigr test`

---

Each page is the navigable reference. LANGUAGE.md §13 remains the authoritative spec for the same modules.
