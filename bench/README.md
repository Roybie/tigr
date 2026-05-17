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
