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

/// The bare names of every source-stdlib module, in a stable order.
/// Used to seed the ambient global namespace (these modules are usable
/// without an explicit `import`); must stay in sync with [`source`].
pub fn names() -> &'static [&'static str] {
    &[
        "Array", "Channel", "Http", "Iter", "LocalChannel", "Map",
        "Math", "Object", "Set", "String", "Test", "Url", "WS",
    ]
}

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
        // `WS.tg` is the pure-tigr WebSocket client (over `Net`). On
        // `wasm32` there is no `Net`, so it is not offered here — the
        // import falls through to `native_modules::resolve`, which
        // returns the browser-`WebSocket` backend (`ws_web`).
        #[cfg(not(target_arch = "wasm32"))]
        "WS"     => Some(include_str!("../../stdlib/WS.tg")),
        _ => None,
    }
}
