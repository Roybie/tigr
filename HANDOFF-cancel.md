# Handoff: add `cancel(handle)` — cooperative green-thread cancellation

## Context

I'm building **purr**, the game framework embedding tigr (sibling repo `../purr`). Purr animations are sequences inside a `go` coroutine — e.g. `Tween.to(...); wait(1); Tween.to(...)`. We have per-property tween supersede, but it can't cancel a whole *sequence*: a coroutine restarted mid-flight keeps marching through its remaining `wait` + tween statements (the zombie does its later work on its original schedule). Per `docs/language/concurrency.md`, `go` has no cancellation primitive and scheduling is cooperative with no preemption. So the fix has to be a real engine primitive: **`cancel(handle)`**.

The purr-side decision to do this in tigr was made deliberately (the alternatives — a game-level generation guard, or a purr `Routine` with cancel-aware `ctx.wait` — were rejected because only an engine primitive makes a sequence read top-to-bottom and "just work" with **bare** `wait`/`wait_frame`).

## Goal

A built-in `cancel(handle)` (sibling of `join`/`yield`/`spawn`, no import) that marks a `go` coroutine for cancellation. The next time that coroutine would **resume from a park**, instead of returning the park's value, a catchable `cancelled` is raised at the park call site, unwinding the body through the normal error-unwind path (so existing `catch`/cleanup runs).

## Semantics (settled)

- **Checkpoint = any park, not just the `wait` builtin.** This is the load-bearing requirement. Purr's `wait_frame` is a purr-side native (`GameTime.wait_frame`) that parks until `drain_ready`; it is *not* a tigr builtin. Cancellation must fire at the generic park/resume boundary so that `wait`, `wait_frame`, `join`, channel recv, I/O parks — anything that parks — are all cancellation points automatically. Do **not** special-case the `wait` builtin.
- **No preemption preserved.** Cancellation is observed only at park/yield points. A coroutine that never parks again runs to completion. Fine for our use (every sequence parks); document it.
- **`cancel` is non-blocking and idempotent.** Returns immediately; cancelling twice or cancelling a finished coroutine is a harmless no-op.
- **Pending park is abandoned.** For `wait` (a timer) and `wait_frame` this is trivial. Best-effort for I/O parks — out of scope to make pretty, but it must not deadlock the resume path.
- **Catchable.** A body may `try { wait(1) } catch e { /* cleanup */ }`. If it swallows `cancelled` and continues, that's allowed (matches normal error semantics) — note it as a known consequence, not a bug.
- **Must not abort the actor.** An *uncaught* `raise` in a `go` body aborts the whole actor (per the docs). `cancelled` reaching the body root must instead terminate **only that coroutine**, recorded as cancelled — never escalate to actor abort. This is the one place `cancelled` must diverge from a normal uncaught raise.

## Open choices for you + the user (small)

1. **`join` on a cancelled coroutine** — recommend returning a match-friendly `${cancelled: true}` (mirrors `${closed: true}` / `${value}` shapes already in the concurrency surface), rather than re-raising. Pick whichever fits the existing `join` contract best.
2. **`cancel`'s own return value** — recommend a bool: `true` if the target was still live and is now marked, `false` if already finished. Or `null` if you prefer fire-and-forget.
3. **Self-cancel** (`cancel` of the running coroutine) — recommend: marks self, takes effect at its next park/yield. Edge case, just don't let it hang.

## Likely touch points (confirm against current source)

- The scheduler park/resume path and the green-thread handle representation (`src/vm/scheduler.rs`, plus wherever the `go` handle / recorded result lives — `task.rs`/`value.rs`).
- Builtin wiring for `cancel` next to `join`/`spawn`.
- The error/token machinery for injecting the catchable `cancelled` at a resume site (`error.rs`, `token.rs`) — and the body-root handling that records "cancelled" instead of aborting the actor.
- The `wait` builtin's `NativeKind::Park` resume site, only to confirm the generic checkpoint covers it (it should need no special code).

## Conventions (from your memory — please follow)

- Tests go in `tests/` using the Test module + `tigr test`, **not** examples. Cover: cancel mid-`wait`; cancel a finished coroutine (no-op); `join` shape after cancel; cleanup via `catch` around the park; cancel of a coroutine that never parks again (runs to completion); and a `wait_frame`-style park if reproducible at tigr level (otherwise note purr will cover it). Add Rust unit tests for the scheduler logic if it warrants, matching the existing suite.
- Docs: update `docs/language/concurrency.md` (the green-threads `go`/`join` section, and reconcile the standing "no cancellation primitive" wording — note that line currently sits under `parallel[]`, which is a *separate* construct; don't imply `parallel[]` gained cancellation) and the relevant `LANGUAGE.md` appendix. Human prose, no em-dashes.
- Memory upkeep: add a `green-threads-design-decisions` entry for the cancel semantics (esp. the "checkpoint = any park" and "cancelled never aborts the actor" decisions), update `green-threads-progress.md`, record under the current version's progress file, and add the `MEMORY.md` index line.
- No Co-Authored-By trailers. Don't commit or push without the user's go-ahead; if you branch, follow the repo's git-workflow norms.

## Downstream (FYI, not your task)

Once this lands and is published, I'll wire it into purr: a restartable sequence becomes `h := go fn(){ Tween.to(...); wait(1); Tween.to(...) }` with a `cancel(old_h)` on restart, bodies using bare `wait`/`wait_frame`. Designing the cancellation to fire at *any* park (point above) is what makes that work — please don't narrow it to tigr's own blocking builtins.
