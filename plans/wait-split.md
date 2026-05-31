# Plan: split `wait` (language) from `wait_frame` (purr)

## Problem

`wait(secs)` and `wait_frame()` were added to tigr as global builtins for purr.
Both currently raise in any standalone `tigr run` program (the `in_drain` guard in
`coop_wait`, `src/vm/vm.rs:2717`, is only true inside `Vm::drain_ready`, which the
CLI never calls). So they are language builtins that can only ever throw outside an
embedder. Two problems:

1. `wait_frame()` is meaningless outside a frame loop — frames are purr's concept,
   not the language's.
2. `wait(secs)` *is* a coherent language primitive (cooperative sleep that yields to
   sibling green threads) but is only wired to the host frame clock.

Related gap this exposes: there is no cooperative sleep in standalone tigr.
`Time.sleep_ms` is `Pure` → inline `thread::sleep` → **freezes every sibling green
thread** (`src/vm/native_modules/time.rs:58`). `Os.run('sleep', n)` is `Blocking` →
offloads (this is the `tests/io_offload_test.tg:89` "four 0.2s sleeps concurrent"
test) but spawns a whole subprocess. Nothing just parks one coroutine on a clock.

## Decision

- **`wait(secs)` → finish as a real language primitive.** Make it work in any
  `tigr run` with green threads by driving timers from a real clock in the standalone
  run loop. Stays a bare builtin; LSP already accepts it.
- **`wait_frame()` → leave the language; becomes `GameTime.wait_frame()` in purr.**
  Removed from `BUILTIN_NAMES`. Reached as a module member, so it is runtime-resolved
  and **invisible to the static checker / LSP** (same as `Gfx.rect` today — `import`
  compiles to a runtime `OpCode::Import`, members are never statically checked). No
  LSP extensibility work needed.

## Core mechanism: `NativeKind::Park`

Today the VM intercepts `wait`/`wait_frame` by **name** in `wait_target`
(`src/vm/vm.rs:3310`) before normal native dispatch. Replace that name-match with a
native *kind* so any native (builtin or host-registered module member) can request a
cooperative park:

```rust
// src/vm/value.rs, NativeKind enum (~line 408)
enum NativeKind {
    Pure(...),
    Blocking(...),
    Socket(...),
    Park(fn(&[Value]) -> Result<WaitKind, RuntimeError>),  // NEW
}
```

`WaitKind` (already in vm.rs:3294: `Secs(f64) | NextFrame`) becomes `pub(crate)`.
The Park fn validates args and returns the kind of park wanted; it never returns a
normal value (a successful park resumes with `Value::Null`).

Dispatch changes (`src/vm/vm.rs`):
- Call / TailCall sites (the green-thread opcodes, ~1280 and ~1391): delete the
  `wait_target` interception; add a `NativeKind::Park(f)` arm that computes the
  `WaitKind` and calls `coop_wait(kind, line)`.
- Non-green native contexts — host `call_function` path (~2423) and the other
  dispatch at ~1913: `Park` arm raises "wait is only valid inside a green thread".
- Delete `wait_target`.

This generalizes the current two magic names into one capability any frame-driving
embedder can use.

## tigr changes

### `wait` becomes standalone-capable (option 3)
- `src/vm/stdlib.rs`: change the `wait` spec to `NativeKind::Park`, with a fn that
  validates the numeric arg and returns `WaitKind::Secs`. Fold the arg-type check from
  the current `native_wait` fallback into it. Delete `native_wait_frame` and the
  `wait_frame` spec/name.
- `BUILTIN_NAMES` (`src/vm/stdlib.rs:91`): drop `"wait_frame"`, size 15 → 14.
- `coop_wait` (`src/vm/vm.rs:2700`): refine the guard.
  - `WaitKind::Secs`: allowed under a host drive **or** the standalone driver (any
    context with a clock). Generator guard stays (synchronous → still raises).
  - `WaitKind::NextFrame`: still requires a host frame drive; raise otherwise. (purr
    always provides one, so its `GameTime.wait_frame` is fine. Standalone code can't
    reach NextFrame anyway — it isn't a builtin.)
- **Standalone timer driving** — the new piece. In the actor run loop
  (`drive` / `run_until`, around the `pick_next() == None` / `HostYield` path,
  vm.rs:2741 and the `drive` catch at ~399/622): when the ready queue is empty and the
  only remaining coroutines are timer-parked, compute the earliest `wake_time`,
  `thread::sleep` the actor thread until then, call `scheduler.wake_timers(now)`, and
  continue. (Under a host drive this path still unwinds via `HostYield` — the host
  owns the clock. The difference is purely "who advances time".)
  - Decision to confirm: behavior of `wait` on the bare **main** coroutine with no
    siblings. Simplest: allow it; with nothing else to run it degenerates to a sleep.
    Alternative: keep the "must be inside a `go`" restriction. Leaning: allow it (one
    fewer special case), document that with no other coroutine it just sleeps.

### embed API helper (for purr)
- `src/embed.rs` (or `src/vm/native_modules/mod.rs` alongside `native`/`native_blocking`):
  export `native_frame_wait(name, arity) -> Value` that builds a `NativeFn` with
  `NativeKind::Park` returning `WaitKind::NextFrame`. Keeps `WaitKind` internal to tigr;
  purr just calls the helper.

### docs / LSP / playground (reverse the earlier `wait_frame` additions; rewrite `wait`)
- `docs/stdlib/builtins.md`: remove the `wait_frame` section + table row. Rewrite the
  `wait` section: it is a cooperative sleep usable in any program — parks the running
  green thread for `secs`, lets siblings run, resumes off the clock; the standalone
  driver advances time. Update Raises (no longer "host-driven only"; still raises in a
  generator). Note the contrast with `Time.sleep_ms` (blocks the actor) and
  `Os.run('sleep')` (offloads a subprocess).
- `LANGUAGE.md` §13.1 table: drop the `wait_frame` row; reword the `wait` row.
- `docs/stdlib/README.md:13`: drop `wait_frame` from the builtin list.
- `web/editor.js` `FALLBACK_BUILTINS`: drop `'wait_frame'` (keep `'wait'`).
- The LSP catalog auto-derives from `builtins.md`, so removing the section drops it
  from hover/completion automatically. Rebuild the debug binary nvim uses:
  `cargo build -p tigr-lsp`, then `:LspRestart`.

### tests
- Add a tigr test proving standalone `wait` is cooperative: two `go` coroutines, one
  `wait`s while the other advances a counter; assert the sibling ran during the wait
  and total wall-clock ≈ the wait, not the sum. (Mirror the shape of
  `tests/io_offload_test.tg`.) Per the test convention, put it in `tests/`.
- Keep/repoint the existing `tests/embed_wait_test.tg` Rust-side wait test.

## purr changes (`/Users/roybie/Projects/misc/purr`)

- `GameTime` module (purr/src/gametime.rs): add
  `("wait_frame", tigr::embed::native_frame_wait("wait_frame", Arity::Exact(0)))`.
- purr docs: document `GameTime.wait_frame()` as the per-frame yield.
- No game-script changes required: `comet.tg:181` uses `wait(0.18)`, which is
  unaffected. Optionally add a `GameTime.wait_frame()` demo.

## What this buys

- `wait` is a genuine language feature (cooperative sleep), works in any `tigr run`.
- `wait_frame` leaves the language; lives where it makes sense (purr `GameTime`).
- LSP never flags `GameTime.wait_frame()` in purr — module members aren't statically
  checked — with **zero** LSP extensibility work.
- The VM loses two hardcoded magic names in favour of one reusable `Park` capability.

## Open questions to confirm before coding

1. `wait` on the bare main coroutine: allow (degenerates to sleep) or restrict to `go`?
   (Leaning: allow.)
2. Units: `wait(secs)` (float seconds) vs `Time.sleep_ms` (int ms) — keep the
   mismatch (seconds reads better for a float wait) or reconcile? (Leaning: keep.)
3. Naming: two sleeps now exist — `wait` (cooperative) and `Time.sleep_ms` (blocking).
   Document the distinction clearly; no rename. (Leaning: keep, just document.)
```
