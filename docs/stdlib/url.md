# `Url`

> Pure-tigr source module, `stdlib/Url.tg`
> Spec: [LANGUAGE.md §13.3](../../LANGUAGE.md#url-v015)

`Url` provides parsing, building, and percent-coding helpers for URLs and query strings, layered on the native `String` and `Bytes` modules. It exposes no runtime type of its own; it works with plain strings and objects. It is ambient, so a bare module name works without an `import`.

`parse(url)` splits an absolute URL into `${scheme, host, port, path, query, fragment}`, and `build(parts)` is its inverse, so `build(parse(u))` round-trips. `encode` and `decode` are RFC-3986 percent-coding done byte-wise over the UTF-8 encoding, so non-ASCII text survives a round trip. `encode_query` and `parse_query` convert between an `Object` and an `a=1&b=x%20y` query string.

## Functions

| Function | Summary |
|----------|---------|
| [`parse(url) -> Object`](#parseurl---object) | Splits an absolute URL into its parts. |
| [`build(parts) -> String`](#buildparts---string) | Reassembles a parts object into a URL string. |
| [`encode(s) -> String`](#encodes---string) | Percent-encodes `s`. |
| [`decode(s) -> String`](#decodes---string) | Percent-decodes `s`. |
| [`parse_query(s) -> Object`](#parse_querys---object) | Parses an `a=1&b=x%20y` query string into an object. |
| [`encode_query(obj) -> String`](#encode_queryobj---string) | Encodes an object into an `a=1&b=x%20y` query string. |


### `parse(url) -> Object`

Splits an absolute URL into its parts. `port` is an `Int` or `null`, `path` defaults to `'/'` when the URL has none, and `query` and `fragment` are the raw (still-encoded) substrings or `null`.

- `url` *(String)*: an absolute URL, with a scheme.

**Returns:** `${scheme, host, port, path, query, fragment}`.
**Raises:** an error when the URL has no `://` scheme separator.

```tigr
p := Url.parse('https://example.com:8443/docs/intro?q=tigr#top');
print(p.scheme);        // => https
print(p.host);          // => example.com
print(p.port);          // => 8443
print(p.path);          // => /docs/intro
print(p.query);         // => q=tigr
print(p.fragment);      // => top
```

### `build(parts) -> String`

Reassembles a parts object into a URL string. It is the inverse of `parse`.

- `parts` *(Object)*: an object shaped like a `parse` result. `port`, `query`, and `fragment` may be `null` to omit them.

**Returns:** the assembled URL as a `String`.

```tigr
print(Url.build(${scheme: 'http', host: 'localhost', port: 3000,
            path: '/api', query: 'x=1', fragment: null}));
// => http://localhost:3000/api?x=1
```

### `encode(s) -> String`

Percent-encodes `s`. Bytes in the RFC-3986 unreserved set `A-Za-z0-9-._~` pass through; every other byte becomes `%XX` with uppercase hex. The encoding works over the UTF-8 bytes of `s`, so non-ASCII characters become multi-byte escapes.

- `s` *(String)*: the text to encode.

**Returns:** the percent-encoded `String`.

```tigr
print(Url.encode('a b/c'));     // => a%20b%2Fc
print(Url.encode('café'));      // => caf%C3%A9
```

### `decode(s) -> String`

Percent-decodes `s`. A `+` is left literal, since `+`-means-space is form semantics handled only by `parse_query`.

- `s` *(String)*: the percent-encoded text to decode.

**Returns:** the decoded `String`.
**Raises:** a structured `decode` error on a malformed or truncated `%`-escape.

```tigr
print(Url.decode('a%20b%2Fc'));                             // => a b/c
print(try { Url.decode('bad%2') } catch (e) { e.kind });    // => decode
```

### `parse_query(s) -> Object`

Parses an `a=1&b=x%20y` query string into an object. Both the key and the value of each pair are form-decoded, so a `+` becomes a space before percent-decoding. On a duplicate key the last value wins.

- `s` *(String)*: the raw query string, without a leading `?`.

**Returns:** an `Object` of decoded key/value pairs.

```tigr
q := Url.parse_query('name=Ada+Lovelace&lang=tigr&lang=rust');
print(q.name);          // => Ada Lovelace
print(q.lang);          // => rust
```

### `encode_query(obj) -> String`

Encodes an object into an `a=1&b=x%20y` query string. Values are stringified with `str` before encoding.

- `obj` *(Object)*: the key/value pairs to encode.

**Returns:** the encoded query string as a `String`.

```tigr
print(Url.encode_query(${q: 'a b', n: 5}));     // => q=a%20b&n=5
```

## See also

- [LANGUAGE.md §13.3](../../LANGUAGE.md#url-v015): the authoritative spec for `Url`
- [Http](http.md): the HTTP client and server, which uses `Url` to parse request targets
- [LANGUAGE.md Appendix N](../../LANGUAGE.md#appendix-n--changes-in-v015-http--url): the v0.15 notes covering `Url` and `Http`
