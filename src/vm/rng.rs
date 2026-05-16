//! Shared thread-local pseudo-random number generator.
//!
//! A small xorshift64 PRNG — no `rand` crate; a hobby generator is
//! plenty for a hobby language. Both the `rand()` builtin (`stdlib.rs`)
//! and the `Random` native module draw from this single per-thread
//! stream, so `Random.seed(n)` makes `rand()` reproducible too.
//!
//! State `0` means "not yet seeded"; the first draw lazily seeds from
//! the wall clock. [`seed`] mixes its argument so any value — `0`
//! included — yields a usable non-zero state.

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

thread_local! {
    static STATE: Cell<u64> = Cell::new(0);
}

/// Mix a raw seed into a non-zero xorshift state. The constant is the
/// golden-ratio odd integer; it diverges two near-identical seeds.
fn mix(raw: u64) -> u64 {
    let x = raw ^ 0x9E3779B97F4A7C15;
    if x == 0 { 0xdeadbeef } else { x }
}

/// Explicitly seed the stream. Any `u64` is accepted; the result is
/// always a usable non-zero state, so `seed(0)` is fine.
pub fn seed(raw: u64) {
    STATE.with(|s| s.set(mix(raw)));
}

/// Advance the generator and return the next 64-bit word. Lazily
/// seeds from the wall clock if the stream has never been used.
pub fn next_u64() -> u64 {
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdeadbeef);
            x = mix(nanos);
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

/// Uniform `f64` in `[0, 1)` — the top 53 bits divided by `2^53`.
pub fn next_f64() -> f64 {
    let bits = next_u64() >> 11;
    (bits as f64) / ((1u64 << 53) as f64)
}

/// Uniform `u64` in `[0, n)`, unbiased via rejection sampling.
/// `n` must be non-zero.
pub fn next_below(n: u64) -> u64 {
    debug_assert!(n != 0, "next_below: n must be non-zero");
    // `n.wrapping_neg() % n` == `2^64 mod n`: the size of the leading
    // partial block. Rejecting `[0, thresh)` leaves a count divisible
    // by `n`, so every residue is equally likely.
    let thresh = n.wrapping_neg() % n;
    loop {
        let r = next_u64();
        if r >= thresh {
            return r % n;
        }
    }
}
