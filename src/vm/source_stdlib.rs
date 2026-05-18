//! Embedded tigr-source stdlib modules (`Array`, `Channel`, `Http`,
//! `Iter`, `LocalChannel`, `Map`, `Math`, `Object`, `Set`, `String`,
//! `Test`, `Url`).
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
        "Channel" => Some(include_str!("../../stdlib/Channel.tg")),
        "Http"   => Some(include_str!("../../stdlib/Http.tg")),
        "Iter"   => Some(include_str!("../../stdlib/Iter.tg")),
        "LocalChannel" => Some(include_str!("../../stdlib/LocalChannel.tg")),
        "Map"    => Some(include_str!("../../stdlib/Map.tg")),
        "Math"   => Some(include_str!("../../stdlib/Math.tg")),
        "Object" => Some(include_str!("../../stdlib/Object.tg")),
        "Set"    => Some(include_str!("../../stdlib/Set.tg")),
        "String" => Some(include_str!("../../stdlib/String.tg")),
        "Test"   => Some(include_str!("../../stdlib/Test.tg")),
        "Url"    => Some(include_str!("../../stdlib/Url.tg")),
        _ => None,
    }
}
