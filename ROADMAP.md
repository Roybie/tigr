# Tigr Roadmap

Planned work beyond v0.7b. Four releases, all in the `v0.N` line — no
jump to 1.0. Each numbered item keeps the reference number from the
design discussion that produced this roadmap (no item 5 — `match`
exhaustiveness was considered and deferred).

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

### 14. Tracing garbage collector  *(core)*

Collections are `Rc<RefCell<...>>` (`LANGUAGE.md` §15.1) — reference
cycles leak and are never reclaimed. Replace the representation with a
tracing collector.

- Mark-sweep collector over the heap-allocated value types
  (`Array`, `Object`, `Function`/closure cells, upvalue cells).
- Touches the `Value` enum and every native module that holds values
  — the largest single change on this roadmap; its own release.
- **Staged option** if a full collector proves too large at once:
  ship interim cycle *detection* (raise/refuse on cycle creation
  where it bites) first, then the collector. Decide when scoping the
  release.
- Independent of the v0.8 `JSON.stringify` fix (item 13), which
  handles stringify recursion regardless of how memory is managed.

---

## v0.11 — Editor tooling

Language core stable (v0.8), stdlib filled out (v0.9), memory model
sound (v0.10) — v0.11 is the editor-support milestone.

### 12. Vim support  *(tooling)*

- **Syntax highlighting** — `editors/vim/syntax/tigr.vim` +
  `ftdetect/tigr.vim`. Fully achievable; no language server needed.
- **Autocomplete, tier 1** — an `omnifunc` / `completefunc` over a
  static set: keywords, built-ins, and stdlib module symbols. Ships
  in v0.11.
- **Autocomplete, tier 2 (scope-aware locals, `obj.` members)** —
  needs a language server; see Deferred below.

---

## Deferred

- **`match` exhaustiveness** (item 5) — falling through to `null` is
  a deliberate design choice; revisit only if it proves a real
  footgun.
- **Named function expressions** (item 3) — self-recursion of a bound
  function already works (`:=` declares a `fn` initialiser before
  compiling its body, so `f := fn(){ f() }` resolves `f`). The form
  would only add inline-lambda recursion and decoupling of the
  recursive call from the binding name — largely redundant with a
  future block-level hoisting of `fn` bindings, which would also give
  ergonomic *mutual* recursion. Revisit hoisting if it proves needed.
- **Language server (LSP)** — unlocks semantic autocomplete, go-to-
  definition, and editor support beyond Vim.
- **Formatter** (`tigr fmt`).
- **`Regex` module.**
