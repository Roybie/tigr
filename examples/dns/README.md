# DNS resolver

A small DNS resolver written in tigr. Builds raw query packets, sends
them over UDP, and parses the response, including name compression,
A / AAAA / CNAME / TXT record types, EDNS(0), and `rcode` error handling.

## Files

- `dns.tg`: the resolver module. Exports `resolve`, `build_query`,
  `parse_response`, `read_name`, and `type_to_string`.
- `main.tg`: CLI that wraps `DNS.resolve` and prints the answers.
- `dns_test.tg`: unit tests for `build_query`, `read_name`, and
  `parse_response` against hand-rolled packet fixtures.

## Usage

From this directory:

```
tigr main.tg <name> [--type A|AAAA|CNAME|TXT] [--server <ip>] [--show-ttl] [--show-opt] [--no-edns] [--bufsize <n>]
```

Defaults: type `A`, server `1.1.1.1`, TTL hidden, EDNS(0) on with a 1232-byte UDP buffer.

```
$ tigr main.tg example.com
example.com   A     104.20.23.154

$ tigr main.tg example.com --type AAAA --show-ttl
example.com   3600   AAAA  2606:2800:220:1:248:1893:25c8:1946

$ tigr main.tg www.github.com --type CNAME --server 8.8.8.8
www.github.com   CNAME  github.com

$ tigr main.tg google.com --type TXT
google.com   TXT   v=spf1 include:_spf.google.com ~all
google.com   TXT   docusign=05958488-4752-4ef2-95eb-aa7ba8a3bd0e

$ tigr main.tg example.com --show-opt
EDNS: version 0, udp 1232, do
example.com   A     104.20.23.154
```

## Flags

- `--type A|AAAA|CNAME|TXT`: record type to query (default `A`).
- `--server <ip>`: resolver to query (default `1.1.1.1`).
- `--show-ttl`: include each record's TTL in the output.
- `--show-opt`: print the EDNS OPT record from the response (version, advertised UDP buffer size, DO flag, and any options such as COOKIE or NSID).
- `--no-edns`: send a classic query with no OPT record. Responses are then capped at 512 bytes, so large answers come back truncated.
- `--bufsize <n>`: advertised EDNS UDP payload size, 0 to 65535 (default 1232). Ignored when `--no-edns` is set.

## Running the tests

```
tigr test dns_test.tg
```

The tests do not touch the network; they parse hand-built byte
arrays, so they're safe to run anywhere.

## Using the module from your own code

```
DNS := import './dns';

response := DNS.resolve('example.com', 1);   // 1 = A
for (a, response.answer) {
    print(a.name, DNS.type_to_string(a.type), a.rdata)
}
```

The full signature is `resolve(name, qtype = 1, server = '1.1.1.1', bufsize = 1232, no_edns = false)`.
When EDNS is on, the parsed response carries the server's OPT record in
`response.opt` (`udp_size`, `version`, `dnssec_ok`, and an `options` list);
it is `null` for a `no_edns` query.

`DNS.resolve` raises a structured error (`${kind: 'rcode', value, message}`
or `${kind: 'mismatched_id', message}`) on a non-zero RCODE or a
response whose ID does not match the query; wrap the call in `try`
to handle it.
