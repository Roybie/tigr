# Tigr Roadmap

Planned work beyond v0.7b. The `v0.N` line continues — v0.8–v0.11 are
shipped, v0.12–v0.15 are planned below, and 1.0 is a stabilization
release after them. Items 1–14 keep the reference number from the
original design discussion; items 15+ were added in a later roadmap
extension.

Conventions for every release below:

- Update `LANGUAGE.md` (new appendix) and `README.md`.
- Add an `examples/vNN/` directory with runnable sample programs.
- Land Rust tests in step with the feature (`cargo test` stays green).
- Add tigr tests for new features in the repo-root `tests/` directory
  (`tests/<feature>_test.tg`, using the `Test` module — `tigr test`
  stays green). `examples/` is for demos only, not tests.
- New opcodes are appended at the **end** of the `OpCode` enum (see
  `memory/v06-design-decisions.md` — mid-enum insertion desyncs
  `from_u8`).

---

## v0.8 — Core semantics & diagnostics

VM- and language-core work. One breaking change, consolidated here.
Items 4 and 11 share the call-frame and error machinery, so they ship
together. This is the heaviest release; do it first while the core is
still small.

### 1. `for` and spread iterate `Iter` objects  ✅ done  *(additive)*

`for` and `[...x]` currently can't consume a v0.7 lazy iterator —
you must `Iter.collect()` first, materializing the array the iterator
existed to avoid. Close the seam:

- `for (v, it)` / `for (i, v, it)` pulls `it.next()` until
  `${done: true}`; spread `[...it]` does the same.
- **Detection rule (design detail to nail down):** an object whose
  `next` field is callable is treated as an iterator by `for` and
  spread. Document that a plain object you want iterated as key/value
  pairs must not have a callable `next`. Duck-typed, matches the rest
  of the language.
- No new opcode strictly required — can lower to the existing
  object-call path — but a dedicated iterator-loop setup is cleaner.

### 2. Integer overflow raises a catchable error  ✅ done  *(BREAKING)*

`arith_add/sub/mul/neg/pow` in `vm.rs` use plain `i64` operators —
undefined behavior (debug panic / release wraparound).

- Switch to `checked_*`; on overflow raise a runtime error with a new
  `RuntimeErrorKind` whose `kind_tag()` is `overflow`.
- Caught, it reifies to `${kind: 'overflow', message: 'integer
  overflow', line}` like the other built-in errors (v0.7b).
- Spec §6.2 gains an explicit overflow paragraph.
- Breaking only for code that relied on silent wraparound — expected
  to be effectively no existing Tigr programs.

### 3. Named function expressions  — dropped, see Deferred.

### 4. Tail calls + bounded recursion  ✅ done  *(additive)*

The spec teaches recursion (`sum([head, ...tl])`), but a deep input
overflows the host stack and crashes.

- New `TailCall` opcode: a call in tail position reuses the current
  frame instead of pushing. Implemented for general tail-position
  calls (not just self-recursive) — `if`/`match`/block tails included.
- Independently: a configurable max call depth that raises a
  catchable `stack_overflow` error instead of crashing the process.

### 11. Stack traces on uncaught errors  ✅ done  *(additive)*

- Capture the call-frame stack when an error is raised; each frame
  records the function name and the call-site line.
- Function names come from inference on the binding (`f := fn(){}` →
  `"f"`), with an `<anonymous>` fallback for unbound functions. This
  item adds the small `name` field to the closure value itself.
- Render the trace beneath the existing source-snippet error report.

### 13. `JSON.stringify` cycle detection  ✅ done  *(additive)*

`JSON.stringify` of a cyclic array/object recurses until the host
stack overflows (`LANGUAGE.md` §15.1). This is a stringify-recursion
bug, *separate* from the GC (item 14) — a collector would not fix it.

- Track visited array/object references during `stringify`; on a
  repeat, raise a catchable error (`kind: 'cycle'` or reuse an
  existing kind).
- Fits this release's correctness/diagnostics theme; small.

---

## v0.9 — Standard library expansion

All additive. Lead with the test framework so every new module below
ships with `.tg` tests written *in Tigr* — dogfooding the language.

### 10. Test framework  ✅ done  *(tooling + source-stdlib)*

- A `Test` source-stdlib module: `suite`, `assert`, `assert_eq`,
  `assert_raises`, etc.
- A `tigr test` CLI subcommand that discovers and runs `*_test.tg`
  (or a `tests/` directory) and reports pass/fail counts.

### 6. `Set` and `Map`  ✅ done  *(native value types)*

Object keys are String-only — no way to key by Int, no dedup
primitive. Added `Set` (membership, union/intersection/difference) and
`Map` (arbitrary-typed keys).

- Shipped as **native value types** (`Value::Map` / `Value::Set` over
  `IndexMap`/`IndexSet`), *not* the stringified-key library the
  original sketch suggested: stringifying allocates on every op and
  collides `1` with `'1'`. Distinct native types give true O(1) ops
  and correct key identity.
- Keys restricted to hashable primitives (null/bool/int/string); a
  `Float` or collection key raises the new `invalid_key_type` error.
- `m[k]`/`s[x]` indexing, `#` length, `for` iteration; not
  JSON-serializable.
- Also fixed `Object`: `has` is now O(1) (native `_NativeObject`),
  `keys`/`values`/`entries` are O(n) (were O(n²)).

### 7. `Random` module  ✅ done  *(library)*

`rand()` is unseedable, so nothing random is reproducible (bad for
tests — which now exist, item 10). Add `Random`: `seed`, `int(lo,
hi)`, `float`, `choice`, `shuffle`.

Shipped as a native module sharing one per-thread PRNG stream with the
`rand()` builtin — `Random.seed` makes `rand()` reproducible too.
`int(lo, hi)` is inclusive of both ends; `shuffle` is non-destructive
(returns a fresh array). Added `bool()` and `range(r)` (a random
element of a `Range`, honouring its step) beyond the original sketch.

### 8. More `Array` combinators  ✅ done  *(library)*

Add `group_by`, `chunk`, `windows`, `partition`, `flat_map`, and a
predicate `count_of`. Pure source-stdlib; in-place append where it
helps (v0.7 `_NativeArray`).

Shipped as pure source-stdlib. `group_by` returns a `Map`, not an
Object, so non-string group keys work. Two fixes landed alongside:

- **In-place removal.** `_NativeArray` could only grow an array
  (`push`/`extend`); pure tigr has no way to shrink one. Added native
  `pop`, `shift`, `unshift`, `insert`, `remove` (single element, or a
  `start`/`count` range), and `clear` — so a deck can actually be dealt
  from, not just copied.
- **Negative-aware `head`/`tail`.** `head` ignored a negative `n` and
  fell into a descending range (garbage output); it and `tail` are now
  Python-slice style — `head(arr, -1)` is all but the last element —
  and so genuinely distinct from the negative-clamping `take`/`drop`.

### 9. String formatting  ✅ done  *(library)*

Interpolation only does `str(expr)` — no width, precision, or
alignment. Add a `String.format` (or printf-style) helper covering
width / precision / alignment / fill.

Shipped as two `String` functions sharing one spec mini-language —
`[[fill]align][sign][#][width][,][.precision][type]`. `String.format(
value, spec)` formats a single value (drops into interpolation);
`String.printf(template, args)` substitutes `%(SPEC)` placeholders
(`%%` is a literal percent). Type codes cover `s d f e E x X b o`,
plus sign, alternate-form base prefixes, and thousands grouping.

---

## v0.10 — Memory model: tracing GC

### 14. Tracing garbage collector  ✅ done  *(core)*

Collections were `Rc<RefCell<...>>` — reference cycles leaked and were
never reclaimed. v0.10 replaces the representation with a hand-written
tracing collector.

- A mark-sweep collector over a per-thread arena heap. The mutable,
  potentially-cyclic value types — `Array`, `Object`, `Map`, `Set`,
  iterators, and closure upvalue cells — are GC-managed; a `Value`
  carries a small generation-tagged handle into the heap. `Str`,
  `Range`, and `Function` templates stay `Rc` (immutable, acyclic).
- Collection is automatic, running at VM dispatch-loop safepoints once
  the live-object count crosses a growing threshold. The `gc()`
  built-in exposes the collector's counters.
- A `gc-torture` build (and the `TIGR_GC_TORTURE` env var) collects on
  every dispatch step — used to prove the root set is exhaustive.
- Shipped as the full collector, not the staged cycle-detection
  fallback the original sketch allowed for.

---

## v0.11 — null-conflation cleanup & editor tooling

Language core stable (v0.8), stdlib filled out (v0.9), memory model
sound (v0.10) — v0.11 bundles the editor-support milestone with a set
of breaking semantic fixes that stop overloading `null`.

### 5. null-conflation cleanup  ✅ done  *(BREAKING)*

`null` was overloaded as a value, "missing", "skip", and "no result".
Three fixes make it an ordinary value again:

- **Collecting loops** — `for[]` / `while[]` collect every body value
  verbatim, including `null`. `continue` is the only way to omit an
  item. `break <value>` appends its value (even `null`); a bare
  `break` appends nothing.
- **`match`** — a non-exhaustive `match` that matches no arm raises a
  catchable `no_match` error instead of yielding `null`. An unguarded
  wildcard / binding last arm is provably exhaustive and never raises.
- **Truthiness** — Lua-style: only `false` and `null` are falsy.
  `0`, `0.0`, `''`, `[]`, `${}`, and empty ranges/maps/sets are
  truthy. This also fixes the `x || default` idiom for legitimate
  zero/empty values.

### 12. Vim support  ✅ done  *(tooling)*

Shipped as a standalone plugin repo —
[`Roybie/vim-tigr`](https://github.com/Roybie/vim-tigr).

- **Syntax highlighting** — `syntax/tigr.vim` + `ftdetect/tigr.vim`;
  no language server needed. `ftplugin/tigr.vim` also sets
  `commentstring` to `// %s`.
- **Autocomplete, tier 1** — done — an `omnifunc` (`autoload/tigr.vim`)
  over a static set: keywords, built-ins, and stdlib module symbols
  (including `Module.` members).
- **Autocomplete, tier 2 (scope-aware locals, `obj.` members)** —
  needs a language server; see Deferred below.

---

## v0.12 — Performance & developer tooling

The first release to treat speed as the deliverable. The core is
stable (v0.8) and the GC is sound (v0.10), so the VM can finally be
optimized without chasing a moving target. Nothing here changes
language semantics — all additive.

Scope was set by measurement. A 300-run timing of the release binary
showed process startup (~1.6 ms) dominates short runs, and the entire
front end (lex+parse+compile, imports included) is only ~0.1–0.45 ms —
so bytecode caching would save sub-millisecond. The originally-planned
`tigr build` / `.tgc` item was therefore **cut** (moved to Deferred);
the real win is faster *VM execution* of compute-heavy programs, which
is what constant folding and the peephole pass deliver.

### 15. Constant folding + peephole optimization  ✅ done  *(additive)*

The compiler emits naive bytecode: `2 + 3` compiles to two `Const`
pushes and an `Add`, literal arithmetic is never folded, and obvious
sequences (a `Pop` after a dead `Const`, jump-to-jump chains) survive
into the final chunk.

- A constant-folding pass — over the AST, or post-compile over the
  chunk — that evaluates literal arithmetic, bitwise ops, and string
  concatenation at compile time.
- A peephole pass over emitted bytecode: collapse jump-to-jump, drop
  `Const`/`Pop` pairs, fuse common opcode runs.
- `Chunk::disassemble()` makes every pass verifiable — add Rust tests
  that assert the optimized opcode sequence, not just the result.
- **Design detail:** folding must preserve v0.8 overflow semantics —
  a literal `9223372036854775807 + 1` still raises `overflow` (or
  becomes a compile error), never silently wraps.

Shipped as two passes. **Constant folding** (`src/vm/fold.rs`) is an
AST→AST rewrite between parse and compile: it folds arithmetic,
bitwise, unary, and string-concat on literal operands, and collapses
fully-parenthesised literals so the enclosing operator folds in turn.
It mirrors the VM arithmetic exactly and **declines to fold** any
operation that would raise — overflow, divide-by-zero, out-of-range
shift — so the catchable error and its source line survive unchanged
(no compile-error path was needed). **Peephole** shipped as jump
threading only (`Chunk::thread_jumps`): a forward jump onto an
unconditional `Jump` is retargeted past it (operands only, no code
relocation). The code-shrinking dead-code pass was deferred (see
Deferred). Measured: `loops_bench` −36%; benches without literal hot-
path arithmetic unchanged — folding helps exactly where literal
sub-expressions sit in a hot path.

### 16. Bytecode serialization — `tigr build`  — cut, see Deferred.

### 17. Disassembler CLI — `tigr disasm`  ✅ done  *(tooling)*

`Chunk::disassemble()` already exists but nothing exposes it.

- `tigr disasm <file.tg>` prints the human-readable bytecode listing
  for a program — and, with a flag, for each nested function chunk.
- Useful on its own; also the inspection tool for verifying item 15.

Shipped as the `tigr disasm` subcommand (`src/disasm_runner.rs`),
reusing the existing compile-without-run path; `-r`/`--nested`
recurses into nested function chunks. The disassembler itself was
fixed along the way — it had mishandled `Closure`'s variable-length
operands (desyncing on any chunk with closures) — and now annotates
jump targets with absolute offsets.

### 18. Benchmark suite  ✅ done  *(tooling)*

There is no committed performance baseline — the old
`examples/v03/bench.tg` was removed.

- A `bench/` directory of representative programs: recursion,
  array/loop churn, string building, GC pressure.
- A `tigr bench` runner (or a documented harness) reporting timings,
  so item 15 gains are measurable and regressions are caught.
- **Sequencing:** land this *before* item 15 and commit the
  unoptimized numbers as the baseline — optimization gains can only be
  shown as a delta against a recorded "before".

Shipped as a `bench/` directory of four programs plus the `tigr bench`
subcommand (`src/bench_runner.rs`), which times each file over an
adaptive iteration count and reports min/mean. The pre-item-15
baseline is recorded in `bench/README.md`. Timing is measured inside
the process (front end + run, not startup), so it tracks exactly what
the optimizer affects.

Stretch / design work, not taken in v0.12: inline caching for member
access, and NaN-boxing the `Value` representation — both larger VM
reworks, now listed under Deferred.

---

## v0.13 — Standard library II

The modules a "small but real" language eventually needs. All
additive.

### 19. `String` II — targeted text helpers  ✅ done  *(library)*

`String` only offers literal search/replace, so any real text work —
tokenizing, line handling, locating, pattern checks — is awkward.

- Twelve additive `String` functions: `words`, `lines`, `split_any`,
  `find_all`, `count`, `replace_first`, `reverse`, `strip_prefix`,
  `strip_suffix`, `capitalize`, `is_blank`, and `matches_glob` (a
  whole-string shell-style `*`/`?`/`[...]` matcher).
- **Design detail — why not a `Regex` module.** Item 19 was originally
  a full `Regex` module, with an open build-vs-buy question. Measuring
  the `regex` crate showed a modest build cost (+1.35 s) but a near
  doubling of binary size (+1.58 MiB, +4 crates). More to the point,
  most "regex" needs — whitespace splitting, counting, line handling,
  glob-style checks — are met by a handful of focused helpers without
  an engine to maintain ("now you have two problems"). The one thing
  they cannot do is **pattern-as-data** — matching a rule unknown until
  runtime (a grep-like tool, user-authored config rules) — and no
  concrete need for that exists in tigr today. So v0.13 ships these
  helpers and `Regex` is deferred (see below), to be revisited when a
  real pattern-as-data use case appears.

Shipped as twelve `String` functions — eleven native in
`src/vm/native_modules/string.rs`, plus `is_blank` in pure tigr.
`matches_glob` is a linear two-pointer scan (no catastrophic
backtracking); a malformed glob raises a catchable error.

### 20. `Bytes` type + binary IO  ✅ done  *(value type + library)*

`String` is UTF-8 only and `IO` is text only — there is no way to
read a non-UTF-8 file, handle binary data, or (later) touch a socket.

- A `Bytes` value type: a mutable, GC-managed byte buffer — indexable
  (bytes as `Int` 0–255), `#`-length, `for`-iterable, spreadable,
  concatenable with `+`/`+=`, content-compared with `==`. Slicing is
  `Bytes.slice` plus array-destructuring `...rest` (tigr has no
  user-facing `[a:]` slice operator).
- Binary `IO`: `read_bytes` / `write_bytes` / `append_bytes`.
- Conversions: `Bytes` ⇄ `String` (UTF-8; the decode direction raises
  a catchable `decode` error), `Bytes` ⇄ `[Int]`, hex, and base64.
- A named fixed-width integer family (`read_u32_be`, `write_i16_le`, …)
  for binary-protocol work — self-documenting call sites over a
  magic-argument `(width, endian)` pair.
- **Design detail — streaming deferred.** Whole-buffer only:
  `read_bytes`/`write_bytes` mirror `read_file`/`write_file`. Streaming
  IO (stateful file/socket handles, `read(n)`, `seek`) is a separate
  axis that arrives with networking — the mutable `Bytes` buffer is its
  enabler. The prerequisite for any future networking or non-text-file
  work.

### 26. Index a collection with a `Range`  ✅ done  *(additive)*

Slicing today means the `Array.slice` / `Bytes.slice` functions — there
is no slice *syntax*. Rather than add a dedicated `[a:b]` operator
(cross-cutting across lexer/parser/opcodes, and redundant with those
functions), let a `Range` be an index key:

- `b[2..5]`, `arr[0..=3]`, `b[0..10:2]` — `..` / `..=` and step already
  exist on the `Range` type, so this needs **no new syntax**:
  `coll[range]` already parses and compiles to `IndexGet`; today it just
  raises `cannot index with range` at runtime.
- The whole change is a `Range`-key arm in `index_get` (`src/vm/vm.rs`)
  for `Array`, `Bytes`, and `String`, returning a new collection of the
  same type — copy semantics, like the `slice` functions. Negative
  bounds resolve from the end; out-of-range bounds clamp.
- Consistent with element indexing: `coll[Int]` → one element,
  `coll[Range]` → a sub-collection. Same bracket; the key type picks the
  behaviour.
- `Array.slice` / `Bytes.slice` stay — they remain the form for an
  open-ended slice (`b[2..#b]` works but reads worse) and the v0.13
  `Bytes` work already shipped them.
- **Design detail:** `String` indexes by character (`#s` and `s[i]` are
  char-based), so a string slice is char-indexed and O(n) — consistent,
  but worth noting. Range-keyed *assignment* (splice) is out of scope;
  slicing is read-only.

Sequenced before item 21 — a small ergonomics win to land before the
`BigInt` stretch.

Shipped exactly as scoped: a `Range`-key arm in `index_get`
(`src/vm/vm.rs`) for `Array`, `Bytes`, and `String`, behind one
`range_indices` helper. The helper resolves negative endpoints from the
end and **filters** out-of-bounds positions — for a monotonic range that
*is* clamping, and it sidesteps the inclusive/step corner cases (a
clamped `arr[0..=100]` must not yield index `len`) that endpoint-clamping
would need to special-case. The range's step and direction carry
through, so a descending range yields a reversed slice. One quirk worth
noting: a range literal fixes its direction from the written endpoints,
so `arr[1..-1]` is a *descending* range (start `1` > end `-1`) — the
non-flipping end-relative idiom is `arr[1..#arr-1]`.

### 21. `BigInt`  ✅ done  *(value type — stretch)*

A natural complement to v0.8's "overflow raises" decision: an
arbitrary-precision integer for code that genuinely needs it —
Project Euler problems already brush the `i64` ceiling.

- A `BigInt` value type with the full arithmetic surface.
- **Design detail:** explicit (`BigInt.new(n)`) vs. automatic
  promotion on overflow. Automatic promotion is friendlier but
  silently changes a value's type mid-computation and conflicts with
  v0.8's catchable `overflow`. Recommend explicit. Stretch item —
  drop to a later release if v0.13 runs long.

Shipped as an immutable `BigInt` value type (`Value::BigInt`,
`Rc`-managed like `Str`/`Range` — not GC-managed, since it carries no
handles), backed by the `num-bigint` crate. A worktree measurement
settled the build-vs-buy question the other way from item 19's `regex`
call: `num-bigint` adds only ~75 KiB / 3 small crates, ~21× cheaper
than `regex`. Construction is **explicit** (`BigInt.new`) as
recommended — no auto-promotion, so v0.8's `overflow` is unchanged. The
ordinary operators (`+ - * / % ^^`, unary `-`, comparisons) work, with
an `Int` operand promoted and a `Float` operand promoting the result;
`==`/ordering compare a `BigInt` against an `Int` by value. Division
`/` is **exact-or-raise** — it yields a `BigInt` only when the result
is exact, otherwise raises a catchable `inexact_division`, so a
`BigInt` operator never silently decays to a lossy `Float`;
`BigInt.divmod` / `BigInt.div` give integer division. The `BigInt`
module adds conversion (`to_int` raising `overflow`, `to_float`,
`to_str_radix`), `pow`, `abs`, `sign`, `is_negative`, `gcd`, `lcm`.
Bitwise operators stay `Int`-only; a `BigInt` is not a `Map`/`Set` key
and is not JSON-serializable.

---

## v0.14 — Concurrency model

The one design-led release, and possibly breaking. Everything below
v0.14 is single-threaded; this release decides whether — and how —
Tigr runs concurrent work.

### 22. Concurrency model  *(design-led, possibly BREAKING)*

The v0.10 GC is deliberately **per-thread** (`memory/
v10-design-decisions.md`): a thread-local arena heap with `GcRef`
handles. That decision forecloses shared-memory threading — a `Value`
cannot safely cross threads — and pushes toward a message-passing
model where each worker owns its own heap.

Likely shape, to be confirmed before implementation:

- **Message-passing actors / structured async**, not shared-memory
  threads. Each unit of concurrency gets its own heap; communication
  is by sending values down channels, which deep-copy across the heap
  boundary.
- New surface: spawning a task, a channel type, and a `select`/await
  form — kept expression-oriented like the rest of the language.
- **Open questions to settle first:** cooperative vs. OS threads;
  whether channels copy or move; how an uncaught error in a worker
  surfaces (extend the existing stack traces across the send
  boundary); GC implications of one heap per worker.

This item should get a dedicated design pass — quite possibly an
explicit decision check with the project owner — before any code
lands. It is the heaviest item on the extended roadmap.

---

## v0.15 — Editor & developer tooling  *(optional)*

Make Tigr pleasant to *work in*. v0.11 shipped static Vim completion;
this release adds the tools that need real program analysis. Pulls two
items off the Deferred list.

This release is **optional**: the language is feature-complete without
it (see *Toward 1.0*), so it can land before or after 1.0 as
developer-experience polish rather than gating the stable release.

### 23. Formatter — `tigr fmt`  *(tooling)*

A canonical-form pretty-printer over the AST.

- `tigr fmt <file.tg>` rewrites in place; `tigr fmt --check` exits
  non-zero on unformatted input (CI-friendly).
- Reuses the existing parser; the only new code is AST → source.
- **Design detail:** decide comment handling first. The parser must
  retain comments (or attach them to AST nodes) or `fmt` drops them —
  this may need a small lexer/parser change.

### 24. Language server (LSP)  *(tooling — formerly Deferred)*

The big editor-tooling unlock. A standalone LSP binary (`tigr lsp`, or
a sibling crate) speaking the Language Server Protocol.

- Tier 1: diagnostics — lex/parse/compile errors as you type,
  reusing the existing error machinery.
- Tier 2: scope-aware completion and go-to-definition — the
  vim-tigr "tier 2" that v0.11 explicitly blocked on this.
- Hover showing inferred binding kind / stdlib signatures.
- **Design detail:** the compiler runs front-to-back and bails on the
  first error. An LSP wants error-recovery parsing and a reusable
  symbol table — scope this honestly; it may be the largest single
  item on the extended roadmap.

### 25. Test coverage reporting  *(tooling)*

`tigr test` reports pass/fail counts but not what the suite exercised.

- Instrument the VM (line hits via the source map); have
  `tigr test --coverage` report per-file line coverage.
- Optional: a coverage-threshold flag for CI.

---

## Toward 1.0

After v0.14 the language is feature-complete in every dimension this
roadmap treats as required: core semantics, diagnostics, stdlib,
memory model, and concurrency. (The v0.15 editor-tooling release is
optional — it may land before or after 1.0.) 1.0 is then a
*stabilization* release, not a feature release:

- **Spec freeze.** `LANGUAGE.md` becomes a compatibility contract;
  further breaking changes require a 2.0.
- **Compatibility statement.** Document what is and isn't guaranteed
  stable — surface syntax, stdlib signatures, the `.tgc` format, the
  embedding API if any.
- **Legacy cleanup.** Either finish wiring `src/v01/` + the
  `--legacy` flag back in, or remove both — a permanently dead flag
  shouldn't ship in 1.0.
- **Documentation pass.** README, LANGUAGE.md, and examples reviewed
  end to end for the post-v0.14 surface.

No new numbered items — 1.0 is the line under the existing ones.

---

## Deferred

- **Named function expressions** (item 3) — self-recursion of a bound
  function already works (`:=` declares a `fn` initialiser before
  compiling its body, so `f := fn(){ f() }` resolves `f`). The form
  would only add inline-lambda recursion and decoupling of the
  recursive call from the binding name — largely redundant with a
  future block-level hoisting of `fn` bindings, which would also give
  ergonomic *mutual* recursion. Revisit hoisting if it proves needed.
- **Inline caching / NaN-boxing the `Value`** — larger VM reworks
  raised under v0.12 item 18; revisit if profiling shows the
  complexity is worth it.
- **Peephole — code-shrinking dead-code elimination** (v0.12 item 15
  "Pass 2") — dropping instructions unreachable after an unconditional
  `Return`/`Raise`/`Jump`. Deferred because it resizes `code` and so
  needs jump-offset and line-table relocation; the no-resize jump-
  threading pass shipped instead. Revisit if the disassembler shows
  enough dead bytecode to be worth the relocation machinery.
- **Bytecode serialization — `tigr build` / `.tgc`** (item 16) — cut
  from v0.12 once measurement showed the startup payoff is
  sub-millisecond (the front end is already ~0.1–0.45 ms; process
  startup dominates). Not a performance feature. Its real value is
  *distribution* — shipping runnable bytecode without source — so
  revisit if and when that becomes a need, scoped as tooling rather
  than optimization.
- **`Regex` module** — scheduled as v0.13 item 19, then deferred.
  Measuring the `regex` crate showed it nearly doubles binary size
  (+1.58 MiB, +4 crates), and most everyday text work is served by the
  targeted `String` helpers that shipped in its place (item 19). The
  genuine gap a regex engine fills is **pattern-as-data** — matching a
  rule supplied at runtime — for which no concrete tigr use case exists
  yet. Revisit when one does; the build-vs-buy question (the `regex`
  crate vs. a hand-written engine) reopens at that point.

The formerly-deferred **Language server** and **Formatter** are now
scheduled (v0.15, items 24 and 23).
