# tigr

A small dynamic language where everything is an expression. Every construct in Tigr, from assignments and blocks to conditionals, loops, `break`, `return`, and `raise`, produces a value. There are no statements.

```tigr
double := fn(x) { x * 2 };
squares := for[] (i, 1..=10) { i * i };
print('first square doubled:', double(squares[0]));   // first square doubled: 2
```

This README is the overview: how to run Tigr and a short tour. For the full reference see [`docs/`](docs/README.md), and for the authoritative spec see [`LANGUAGE.md`](LANGUAGE.md).

---

## Running tigr

```bash
cargo build --release
./target/release/tigr path/to/program.tg              # run a script
./target/release/tigr path/to/program.tg arg1 arg2    # script with args (Os.args)
./target/release/tigr                                  # interactive REPL
./target/release/tigr test                             # discover and run tests
./target/release/tigr test path/                       # tests under a path
./target/release/tigr disasm program.tg                # print compiled bytecode
./target/release/tigr disasm program.tg -r             # also recurse into nested functions
./target/release/tigr bench                            # time the bench/ suite
```

When a script finishes, its final value is printed, so a one-line file containing `1 + 1` produces `2`. With no argument, tigr starts an interactive REPL.

`tigr test` discovers test files, meaning any `*_test.tg` file plus every `.tg` file under a `tests/` directory, runs them, and reports pass and fail counts. See the [`Test` module](docs/stdlib/test.md) for writing tests.

`tigr disasm` compiles a program and prints its bytecode without running it, which is useful for seeing what the compiler emitted, constant folding included. Add `-r` to recurse into nested function chunks. `tigr bench` runs the `.tg` files under [`bench/`](bench/) repeatedly and reports min and mean wall time.

Working examples live in [`examples/`](examples/), one `.tg` file per language feature or standard-library module. The original v0.1 tree-walking interpreter keeps its own examples under [`examples/v01/`](examples/v01/); the language has changed since, so those use older syntax.

---

## A short tour

**Everything is an expression.** A block evaluates to its last expression, `if` to its chosen branch, a loop to its last iteration. The `for[]` form collects every iteration into an array instead.

```tigr
x := if 5 > 3 { 'big' } else { 'small' };
print(x);                              // big
print(for (n, 1..=10) { n });          // 10, the last n
print(for[] (n, 1..=5) { n * n });     // [1, 4, 9, 16, 25]
```

**Functions are values and they close over their scope.**

```tigr
adder := fn(n) { fn(v) { v + n } };
add10 := adder(10);
print(add10(5));                       // 15
```

**`match` picks the first arm whose pattern fits.**

```tigr
grade := fn(s) {
    match s {
        90..=100 => 'A',
        80..=89  => 'B',
        _        => 'C',
    }
};
print(grade(85));                      // B
```

**The pipe `|>` threads a value through a chain of calls.** `x |> f(args)` becomes `f(x, args)`. Combined with the lazy [`Iter`](docs/stdlib/iter.md) module, a pipeline never builds the intermediate arrays.

```tigr
Iter := import 'Iter';
result := Iter.from(1..=1000)
    |> Iter.map(fn(n) { n * n })
    |> Iter.filter(fn(n) { n % 2 == 0 })
    |> Iter.take(3)
    |> Iter.collect();
print(result);                         // [4, 16, 36]
```

**Concurrency is OS-thread actors that share no mutable state.** `spawn` starts one, `Task.join` waits for its result.

```tigr
Task := import 'Task';
worker := spawn fn() { 6 * 7 };
print(Task.join(worker));              // 42
```

---

## Documentation

Tigr's documentation comes in three layers.

- **This README** is the overview: what Tigr is, how to install it, and the tour above.
- **[`docs/`](docs/README.md)** is the navigable reference: one page per language topic and one per standard-library module, with signatures, parameters, errors, and runnable examples.
- **[`LANGUAGE.md`](LANGUAGE.md)** is the authoritative spec and compatibility contract. When the reference and the spec disagree, the spec wins.

Start here:

- [Language reference](docs/README.md#language): expressions, control flow, functions, errors, concurrency, and more
- [Standard library](docs/stdlib/README.md): all 22 modules and the global builtins
- [ROADMAP.md](ROADMAP.md): planned work beyond the current release

---

## Status

tigr is feature-complete. 637 Rust tests and 271 tigr tests pass. It runs on a bytecode VM with:

- closures with Lox-style upvalues, first-class lazy ranges, destructuring patterns, pipe `|>`, spread `...`, and string interpolation;
- `try` / `catch` / `raise` with structured errors. `catch` binds the exact raised value, and built-in errors reify to a `${kind, message, line}` object;
- rendered errors with source snippets, plus stack traces on uncaught errors;
- a `match` expression with refutable patterns, bitwise operators, and extended number literals (`0xFF`, `1e6`, `.5`, `_`);
- lazy `Iter` iterators whose pipelines never materialize intermediate arrays, in-place array growth, and `for` and spread consuming iterator objects directly;
- integer-overflow checks, tail-call optimization, and bounded recursion;
- concurrency built on OS-thread actors (`spawn` and `Task.join`), message-passing `Channel`s, a `select` block, and the structured `parallel[]` fan-out. Actors share no mutable state, so the model is race-free by construction;
- a tracing mark-sweep garbage collector. The mutable, potentially-cyclic value types (`Array`, `Object`, `Map`, `Set`, iterators, and closure upvalue cells) are managed by a collector over a per-thread heap, so reference cycles are reclaimed rather than leaked. Collection is automatic, running at VM safepoints once the heap crosses a size threshold, and the `gc()` builtin exposes the collector's counters;
- a standard library of 22 modules spanning `Array`, `Iter`, `String`, `Math`, `Object`, `Map`, `Set`, `Channel`, `Url`, `Http`, and a `Test` framework, all written in tigr itself, plus native `IO`, `Path`, `Os`, `Time`, `DateTime`, `JSON`, `Bytes`, `BigInt`, `Task`, `Net` (TCP/UDP/TLS sockets), and seedable `Random` modules.

See [`LANGUAGE.md`](LANGUAGE.md) for the authoritative spec. The v0.1 tree-walking interpreter source lives under `src/v01/` for reference; it is not currently wired into the build.

Known limitations:

- Runtime error spans are line-only, with no column information. Lex, parse, and compile errors carry full spans.
- No regex. `String.contains`, `split`, `replace`, and `matches_glob` cover the common cases.
