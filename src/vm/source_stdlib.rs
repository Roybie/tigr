//! Embedded tigr-source stdlib modules (`Array`, `Math`, `String`).
//!
//! These are `.tg` files at the repo's `stdlib/` directory, embedded
//! at compile time via `include_str!`. Bare-name imports check this
//! registry first; misses fall through to `native_modules::resolve`.
//!
//! The source modules themselves may import the underlying native
//! primitives by the `_Native*` names (e.g. `import '_NativeMath'`),
//! which resolve via `native_modules::resolve` exclusively — there's
//! no source/native cycle.

/// Embedded source for a bare-name module, or `None` if not part of
/// the source stdlib.
pub fn source(name: &str) -> Option<&'static str> {
    match name {
        "Array"  => Some(include_str!("../../stdlib/Array.tg")),
        "Math"   => Some(include_str!("../../stdlib/Math.tg")),
        "String" => Some(include_str!("../../stdlib/String.tg")),
        _ => None,
    }
}
