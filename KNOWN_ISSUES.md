# Known issues

Open issues found during the codebase review of 2026-05-19. Each entry
gives a location, the impact, and a suggested fix. Severity is the
reviewer's estimate; none of these are crashes or data-corruption bugs
(those found in the same review were fixed — see the note at the end).

When you fix one, delete its entry.

## Confirmed

### 1. GC slot generation counter wraps silently in release builds

- **Where:** `src/vm/gc.rs`, ~line 241 — `slot.generation.wrapping_add(1)`.
- **Impact:** After 2^32 free/reuse cycles of a single arena slot the
  generation wraps to 0, so a very stale handle issued at generation 0
  resolves to a fresh, unrelated object — a use-after-free. The
  `debug_assert!` on the next line catches it only in debug builds.
- **Reachability:** Astronomically unlikely (2^32 reuses of *one* slot),
  but a genuine soundness gap.
- **Fix:** Use `assert!` instead of `debug_assert!`, or saturate the
  counter and treat `u32::MAX` as a permanently dead slot.

### 2. `String.format` renders NaN / Infinity as text instead of raising

- **Where:** `src/vm/native_modules/string.rs`, `render_float` /
  `render_exp` (~lines 630-661); `as_number` (~line 574).
- **Impact:** `String.format(nan, 'f')` yields the string `"NaN"` and
  `String.format(inf, '+f')` silently drops the sign. `JSON.stringify`
  raises on non-finite floats, so the two are inconsistent.
- **Fix:** `as_number` should reject non-finite floats and raise, the
  way `as_integer` already rejects out-of-range values.

### 3. `String.repeat` has no upper bound

- **Where:** `src/vm/native_modules/string.rs`, `s_repeat` (~lines 138-155).
- **Impact:** A huge count is passed straight to `str::repeat`, which
  drives an allocator abort (uncatchable, kills the process) rather
  than a catchable tigr error.
- **Fix:** Cap the output size (e.g. some MB) and raise a catchable
  error past the cap.

### 4. `Bytes.write_u64` cannot represent values >= 2^63

- **Where:** `src/vm/native_modules/bytes.rs`, `write_u64_be` /
  `write_u64_le` (~lines 365-380).
- **Impact:** tigr has no unsigned integer type, so `write_u64` takes an
  `i64` and cannot write the range 2^63 .. 2^64-1. `read_u64` *can*
  produce those values, so the write/read pair does not round-trip and
  the write side reports no error.
- **Fix:** Document the limitation, or detect and raise on the
  unrepresentable range.

### 5. `Http` client redirect / status edge cases

- **Where:** `stdlib/Http.tg`, `_request`.
- **Impact:**
  - 1xx informational responses are treated as the final response (the
    body-skip condition leaves 1xx to break the request loop). A server
    answering `Expect: 100-continue` on a POST confuses the client.
  - A 301/302/303 redirect forwards caller-supplied `Authorization`
    headers to the redirect target, including cross-origin — not
    RFC-correct (cross-origin redirects should drop `Authorization`).
  - Relative redirects without a leading `/` are not `..`-normalized.
- **Fix:** Skip and re-read past 1xx; strip `Authorization` on a
  cross-origin redirect; normalize `..` in `_resolve`.

## Lower confidence / latent

### 6. Reactor token counter can wrap (32-bit targets only)

- **Where:** `src/vm/reactor.rs`, ~line 505 — `next_token` is a `usize`.
- **Impact:** On a 32-bit target the counter wraps after ~4e9 ops and
  can reuse a token still held by an in-flight op, evicting it from the
  op table (its fd is never deregistered; results misroute). On 64-bit
  this is unreachable.
- **Fix:** After incrementing, skip any token already in the op map.

### 7. `nonblocking`-mode flag TOCTOU

- **Where:** `src/vm/socket.rs`, `set_nonblocking_mode` (~line 687).
- **Impact:** The load-check / store of the `nonblocking` atomic is not
  atomic as a unit. Safe today only because the `is_idle()` dispatch
  invariant keeps the inline executor and the reactor from touching one
  socket at the same time — a latent hazard if dispatch logic changes.
- **Fix:** Couple the check and store under a lock, or make the flag
  authoritative only on one owner.

### 8. JSON cycle guard sits after the empty-collection early return

- **Where:** `src/vm/native_modules/json.rs`, ~lines 408-436.
- **Impact:** The cycle check runs only for non-empty containers. Not
  exploitable today (an empty container cannot be made cyclic), but
  structurally fragile.
- **Fix:** Move the cycle check before the `is_empty` early return — no
  cost, removes the ambiguity.

### 9. `IO.remove` TOCTOU

- **Where:** `src/vm/native_modules/io.rs`, ~line 202.
- **Impact:** A race between `is_dir()` and `remove_dir_all` /
  `remove_file`. Benign in almost all real scenarios.

### 10. `Path.join` silently drops the base on an absolute component

- **Where:** `src/vm/native_modules/path.rs`, ~line 40.
- **Impact:** `Path.join('/base', '/abs')` returns `/abs`, not
  `/base/abs` (the std `PathBuf::push` semantics). No current caller
  triggers it, but it is a latent footgun with no doc warning.

---

*Already fixed in the same review (for the record, not open):* the
`i64::MIN / -1` process crash, the interpolation scanner miscounting
braces in raw strings, a cyclic-collection `==` stack overflow, a
duplicate `Content-Length` in `Http.serve`, and `Array.sort_by`'s
O(n^2) key evaluation.
