# `Deferred`

> Native (Rust) module
> Spec: [LANGUAGE.md Appendix P](../../LANGUAGE.md#appendix-p--green-threads-generators-and-io-offload)

A `Deferred` is a first-class, write-once result a coroutine can wait on and anything can complete. `Deferred.new()` mints one, the ordinary [`join`](builtins.md#joinhandle---value) waits on it, and `Deferred.resolve` / `Deferred.reject` settle it. `type(d)` is `'deferred'`; a `Deferred` is not JSON-serializable and cannot be sent across actors. It is ambient, so the bare module name works without an `import`.

It generalises `join`. Where `join` waits on a coroutine's return, a deferred waits on a value *anyone* can supply, so a barrier, fan-in, a one-shot signal, or first-to-complete is writable in pure tigr without a host.

```tigr
d := Deferred.new();
go fn() { Deferred.resolve(d, 42) };
print(join(d));   // => 42
```

A deferred is a **latch**: the result is recorded once, so a `join` after the settle returns (or re-raises) immediately, and a value resolved before anyone waits is still delivered. `resolve` and `reject` **broadcast** — every coroutine parked in `join(d)` wakes with the same value. `reject` re-raises its value at each awaiter's `join`, the same way an uncaught error in a `go` body reaches its joiner, so it drops into an ordinary `try`/`catch`.

Settling is **once**: `resolve` and `reject` return `true` if they settled the deferred and `false` if it was already settled (mirroring [`go_cancel`](builtins.md#go_cancelhandle---bool)). A race is therefore safe to write directly: several coroutines can try to resolve, the first wins, the rest are harmless no-ops.

A `join(d)` is a cancellation point like any other park, so [`go_cancel`](builtins.md#go_cancelhandle---bool) unblocks a coroutine waiting on a deferred. A deferred that nothing ever resolves leaves its awaiters parked, the same as a `recv` with no sender; a standalone program that would block on a deferred with no way to resolve it raises a catchable deadlock rather than hanging.

## Functions

| Function | Summary |
|----------|---------|
| [`new() -> Deferred`](#new---deferred) | Mints an unsettled deferred. |
| [`resolve(d, value) -> Bool`](#resolved-value---bool) | Settles `d` with a value, waking every awaiter. |
| [`reject(d, error) -> Bool`](#rejectd-error---bool) | Settles `d` with an error that re-raises at each awaiter's `join`. |

To wait on a deferred, use the built-in [`join(d)`](builtins.md#joinhandle---value); there is no separate `Deferred` wait function.

### `new() -> Deferred`

Mints an unsettled deferred.

**Returns:** a new `Deferred`.

```tigr
d := Deferred.new();
print(type(d));   // => deferred
```

### `resolve(d, value) -> Bool`

Settles `d` with `value` and wakes every coroutine parked in `join(d)`, each receiving `value`. Non-blocking: the awaiters run when the scheduler next reaches them.

- `d` *(Deferred)*: the deferred to settle.
- `value` *(value)*: the result to deliver.

**Returns:** `true` if this call settled the deferred, `false` if it was already settled (resolved or rejected), in which case nothing changes.

```tigr
d := Deferred.new();
print(Deferred.resolve(d, 1));   // => true
print(Deferred.resolve(d, 2));   // => false (already settled)
print(join(d));                  // => 1
```

### `reject(d, error) -> Bool`

Settles `d` so that each awaiter's `join(d)` re-raises `error` verbatim, composing with `try`/`catch` exactly as a `raise` does.

- `d` *(Deferred)*: the deferred to settle.
- `error` *(value)*: the value to raise at each awaiter's `join`.

**Returns:** `true` if this call settled the deferred, `false` if it was already settled.

```tigr
d := Deferred.new();
go fn() { Deferred.reject(d, 'boom') };
print(try { join(d) } catch (e) { 'caught: ' + e });   // => caught: boom
```

## Host completion

An embedding host can complete a `Deferred` from outside the worker pool: a native hands a `Deferred` back, and the host resolves it later from its own loop (a GPU readback, an OS event, a file dialog) with `Session::resolve` / `Session::reject`. The parked coroutine then resumes on the next `drain_ready`, so the waiting tigr code reads top to bottom: `img := join(screenshot())`. This is the async-completion seam the worker-pool offload (which resumes from a *blocking* call) does not cover.

## See also

- [Concurrency](../language/concurrency.md#deferred-values-deferred): deferreds among `go`, `join`, and green threads
- [Built-in functions](builtins.md#joinhandle---value): `join`, which waits on a deferred
- [Control flow](../language/control-flow.md): `try`/`catch`, which catches a rejected deferred
