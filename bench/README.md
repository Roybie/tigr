# tigr benchmark suite

Run with `tigr bench` (defaults to this directory) or `tigr bench <path>`.

Each file is a self-contained tigr program exercising one subsystem.
`tigr bench` runs every file repeatedly and reports **min** and **mean**
wall time. Timing is measured inside the already-running process, so it
covers lex+parse+compile+run but *not* process startup — it tracks
exactly what the v0.12 optimizer (constant folding + peephole) affects.
Use **min** for before/after comparison; it is the least noisy.

| File | Exercises |
|---|---|
| `recursion_bench.tg` | recursive calls — frame setup/teardown |
| `loops_bench.tg` | tight arithmetic loop (literal-heavy, folds well) |
| `arith_unfolded_bench.tg` | tight arithmetic loop, variable-only operands (nothing folds) — raw dispatch |
| `calls_bench.tg` | non-recursive call/return churn at flat depth |
| `nested_loops_bench.tg` | nested loops — jump- and dispatch-bound, no alloc/calls |
| `locals_bench.tg` | dense LoadLocal/StoreLocal churn |
| `string_bench.tg` | string allocation + interpolation |
| `gc_bench.tg` | short-lived object/array allocation — GC pressure |

## v0.12 — constant folding + peephole

Measured on the development machine; absolute numbers vary by hardware,
the deltas are what matter. "Before" is v0.12 with item 15 not yet
landed; "after" is with constant folding + jump-threading in place.

| File | before | after | change |
|---|---|---|---|
| `recursion_bench.tg` | 139.5 ms | 139.9 ms | — (no literal arithmetic to fold) |
| `loops_bench.tg` | 106.3 ms | 68.2 ms | **−36%** (literal sub-expressions folded) |
| `gc_bench.tg` | 66.0 ms | 65.5 ms | — |
| `string_bench.tg` | 15.0 ms | 14.9 ms | — |

Constant folding pays off exactly where a hot path contains literal
sub-expressions; it does nothing for code whose operands are all
variables. Jump threading is a small, branch-shape-dependent win not
visible at this suite's resolution. Both are kept for the regression
guard they provide going forward.

## Dispatch loop frame cache

The dispatch loop used to recompute the current frame's closure,
function, and chunk on *every* bytecode instruction: an arena borrow of
the closure (a thread-local heap access + `RefCell` borrow + generation
check) plus an `Rc<Function>` clone, all dropped at the end of the
iteration. That bookkeeping had nothing to do with the instruction's
actual work, and it ran tens of millions of times on a hot loop.

The frame state now persists across loop iterations and is refreshed
only when the current frame's closure handle actually changes — a call,
return, tail-call, or coroutine switch. The per-instruction check is a
single integer-identity comparison; the expensive borrow + clone happen
once per frame transition instead of once per instruction. The change
is observationally identical (a closure's `function` is immutable, so a
matching handle guarantees the cached chunk is the right one), so the
whole test suite — including `TIGR_GC_TORTURE=1` — stays green;
`tests/dispatch_frame_cache_test.tg` adds focused coverage of the frame
transitions the cache governs.

"Before" is the parent commit; "after" is the frame cache. Best of three
full-suite runs (min varied < 1% across runs); same machine.

| File | before | after | change |
|---|---|---|---|
| `arith_unfolded_bench.tg` | 272.8 ms | 152.2 ms | **−44%** (1.79×) |
| `nested_loops_bench.tg` | 348.1 ms | 193.5 ms | **−44%** (1.80×) |
| `loops_bench.tg` | 64.3 ms | 36.0 ms | **−44%** (1.79×) |
| `calls_bench.tg` | 216.4 ms | 126.8 ms | **−41%** (1.71×) |
| `locals_bench.tg` | 285.5 ms | 170.0 ms | **−41%** (1.68×) |
| `recursion_bench.tg` | 128.1 ms | 85.7 ms | **−33%** (1.49×) |
| `gc_bench.tg` | 67.3 ms | 52.9 ms | **−21%** (1.27×) |
| `string_bench.tg` | 11.1 ms | 10.7 ms | — (allocation-bound) |

The win scales with how dispatch-bound the code is: a tight loop of
cheap opcodes (arithmetic, loads, in-frame jumps) sees ~1.8×, while
allocation-bound code (`string_bench`) is unaffected because its
bottleneck was never the dispatch prologue. `recursion_bench` lands
lower than the pure-loop benches because half its instructions are the
calls/returns that still pay the refresh.
