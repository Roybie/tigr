# DNS resolver

A small DNS resolver written in tigr. Builds raw query packets, sends
them over UDP, and parses the response — including name compression,
A / AAAA / CNAME record types, and `rcode` error handling.

## Files

- `dns.tg` — the resolver module. Exports `resolve`, `build_query`,
  `parse_response`, `read_name`, and `type_to_string`.
- `main.tg` — CLI that wraps `DNS.resolve` and prints the answers.
- `dns_test.tg` — unit tests for `build_query`, `read_name`, and
  `parse_response` against hand-rolled packet fixtures.

## Usage

From this directory:

```
tigr main.tg <name> [--type A|AAAA|CNAME] [--server <ip>] [--show-ttl]
```

Defaults: type `A`, server `1.1.1.1`, TTL hidden.

```
$ tigr main.tg example.com
example.com   A     93.184.216.34

$ tigr main.tg example.com --type AAAA --show-ttl
example.com   3600   AAAA  2606:2800:220:1:248:1893:25c8:1946

$ tigr main.tg www.github.com --type CNAME --server 8.8.8.8
www.github.com   CNAME  github.com
```

## Running the tests

```
tigr test dns_test.tg
```

The tests do not touch the network — they parse hand-built byte
arrays — so they're safe to run anywhere.

## Using the module from your own code

```
DNS := import './dns';

response := DNS.resolve('example.com', 1);   // 1 = A
for (a, response.answer) {
    print(a.name, DNS.type_to_string(a.type), a.rdata)
}
```

`DNS.resolve` raises a structured error (`${kind: 'rcode', value, message}`
or `${kind: 'mismatched_id', message}`) on a non-zero RCODE or a
response whose ID does not match the query; wrap the call in `try`
to handle it.
