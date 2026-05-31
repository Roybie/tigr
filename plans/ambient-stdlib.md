# Plan: ambient stdlib + host-extensible LSP

## Problem

Every stdlib use today needs an explicit binding first: `String := import 'String'`
before `String.format(...)`. For a scripting/game language this is friction on every
file. We want stdlib modules (and host-registered modules from embedders like purr)
usable *without* import, while keeping local files and third-party code explicit.

The blocker is that tigr resolves names **at compile time**, and resolution picks the
opcode. `resolve_in` (`src/vm/compiler.rs:656`) walks local -> upvalue -> global; a
miss is a compile error (`CompileErrorKind::UndeclaredVariable`, `compiler.rs:1036`),
not a runtime lookup. Globals are a fixed `Vec` seeded from `stdlib::names()` (the 14
builtins only); there is no `SetGlobal`. So "ambient" cannot be a runtime fallback —
the compiler must know the ambient module names while compiling.

That is fine, because the name sets are static: `source_stdlib::source`
(`src/vm/source_stdlib.rs:16`) and `native_modules::resolve`
(`src/vm/native_modules/mod.rs:49`) are exhaustive matches.

## Decisions (settled)

1. **All public stdlib is ambient.** One rule: capitalized stdlib module = always
   there; anything you wrote or installed = `import` it. The `_Native*` backend
   modules stay internal (NOT ambient). Explicit `import String` stays legal as a
   shadowing no-op so existing code/examples keep compiling.
2. **Host modules are ambient too at runtime** (purr writes `Game.rect(...)` with no
   import). For the LSP, the host describes its modules in a committable
   `tigr.modules.json` sidecar (primary), with `initializationOptions` as an override.
   One manifest feeds both the ambient-name set (no false diagnostics) and the catalog
   (hover/completion/signature).

## Core mechanism: lazy-global

Ambient module names are appended to the global namespace, seeded with lazy
placeholders that resolve-and-memoize on first use. After first touch, every reference
is a plain `LoadGlobal idx` (array index, same speed class as a local) with no
per-access HashMap or allocation — matching explicit-import steady-state performance.

1. **Compiler**: append the ambient module names (stdlib + host) to the globals name
   list after the 14 builtins (`compiler.rs:262`). No new `Resolved` variant and no
   change to `resolve_in` — its existing global-position check (`compiler.rs:691`) now
   finds ambient names as `Resolved::Global(idx)`. The `_Native*` backends are NOT in
   this list (stay import-only).
2. **VM**: seed the globals vec (`vm.rs:270`, via `stdlib::builtins()`) with a
   `Value::LazyModule(Arc<str>)` placeholder at each ambient index, in the exact same
   order the compiler used.
3. **VM `LoadGlobal`** (`vm.rs:1578`): if the loaded value is `LazyModule(name)`,
   resolve the module, write it back into `globals[idx]` (memoize), then push. Once
   memoized the slot holds the real module Value, so every later load is a pure index.

Resolution reuses the existing Import logic — factor the `OpCode::Import` body
(`vm.rs:2118`) into a shared path:
- **Native modules** resolve synchronously (`native_modules::resolve` returns a Value):
  write `globals[idx]`, push, done.
- **Source modules** (Array, String, Math, ...) must compile+run their `.tg` to build
  the export object, which is frame-based — so `LoadGlobal` pushes an Import frame the
  same way `OpCode::Import` already does (save ip, push frame, continue). The frame
  carries the target global index: `FrameKind::Import` gains an `Option<u8>` writeback,
  and its `Return` writes the result into `globals[idx]` (in addition to
  `module_cache`). This is the one genuinely new piece of machinery.

Properties:
- **Shadowing is free.** Resolution is still local -> upvalue -> global, so a local
  `String := ...` or an explicit `import` (which binds a local) shadows the ambient
  global. Existing `import String` keeps compiling.
- **True laziness preserved.** A `LazyModule` resolves only when its `LoadGlobal`
  actually executes, so an ambient `Net` behind an untaken branch never builds the
  reactor. (This is why lazy-global beats a prelude that pre-imports every *mentioned*
  module: dead-path `Net` would spin the reactor at startup.)
- **No steady-state cost.** Post-memoization, hot-loop `Math.x` is `LoadGlobal idx`
  plus the same member lookup explicit imports already do. No per-access HashMap/alloc.

Costs/edges:
- `LoadGlobal` gains one discriminant check ("is this a LazyModule?") on every global
  load, including builtins. Once memoized it fails fast; marginal.
- Re-entrancy: `module_cache` already guards repeat resolution. A source module that
  references itself mid-load is a pre-existing Import concern, not new here; a
  "resolving" sentinel in the slot can detect cycles if needed.

## Why the LSP needs almost nothing for stdlib

Diagnostics delegate to the compiler: `compute_diagnostics`
(`crates/tigr-lsp/src/main.rs:1291`) calls `tigr::vm::check_source`. Once the compiler
treats stdlib names as ambient, the LSP stops flagging `String.format` automatically.
No diagnostic change.

Symbol features already assume nothing about imports: the catalog (`src/catalog.rs`) is
docs-driven from `docs/stdlib/*.md` and holds every module unconditionally; completion
iterates `catalog.modules()` regardless of imports; hover on a bare module name hits
`catalog.module()`. So ambient stdlib is a compiler-only change and the LSP follows.

## Host modules: the part the LSP genuinely can't infer

At runtime, host modules already live in `Vm::host_modules` / `host_source_modules`
(private, `vm.rs:148/158`) and Import resolves them. To make them ambient, `Session::load`
(`src/embed.rs`) adds the registered host names to the compiler `ambient` set before
compiling. Session wraps `register_module` already, so it tracks names itself (or add
`Vm::host_module_names()`).

But the **LSP runs the compiler standalone** — no Session, no purr. So without help it
false-flags `Game.rect` as undeclared and has no hover/completion for `Game.*`. The host
must tell it. Manifest shape mirrors the catalog `Module` so it drops straight in:

```json
{
  "modules": {
    "Game": {
      "description": "purr engine surface",
      "members": {
        "rect": { "signature": "rect(x, y, w, h) -> Null", "doc": "Draw a filled rectangle." }
      }
    }
  }
}
```

Two consumers inside the LSP:
1. **Ambient-name set** -> threaded into `check_source`. Requires a signature change:
   `check_source` (`src/vm/mod.rs`) + the compiler accept an extra "ambient module
   names" argument so they don't false-flag host names. Same hook the runtime uses,
   sourced from the manifest instead of a live Session.
2. **Catalog** -> `Catalog` (`src/catalog.rs`) gains `host_modules: HashMap<String, Module>`
   from the manifest; hover/completion/signature-help check it alongside stdlib (they
   already route through `catalog.member()` / `catalog.module()`).

Transport: read `tigr.modules.json` from workspace roots on `initialize`
(`main.rs:270`, currently ignores `initializationOptions`), re-stat like other workspace
files; `initializationOptions.modules` overrides/merges. purr can generate the manifest
(it knows registered names + `NativeFn` member names; signatures/docs come from purr's
own metadata, since `register_module` carries only names + arities).

## Change list

Core (ambient stdlib + host-ambient at runtime):
- `src/vm/source_stdlib.rs`: add `names() -> &[&str]` (the 12 source modules).
- `src/vm/native_modules/mod.rs`: add `names() -> &[&str]` (public modules only,
  exclude `_Native*`; respect the `cfg` gates on Os/Net).
- `src/vm/value.rs`: add `Value::LazyModule(Arc<str>)` (GC-leaf, like `NativeFn`).
- `src/vm/stdlib.rs`: `names()`/`builtins()` append the ambient module names + matching
  `LazyModule` placeholders after the 14 builtins, in one shared canonical order so the
  compiler name-list and VM globals vec agree by index.
- `src/vm/compiler.rs`: globals name-list includes ambient names (constructor);
  `resolve_in` unchanged (finds them as `Resolved::Global`).
- `src/vm/vm.rs`: factor the Import body into a shared `resolve_module` path;
  `LoadGlobal` resolves+memoizes a `LazyModule` (native synchronous; source via an
  Import frame tagged with the writeback global idx); `FrameKind::Import` gains an
  `Option<u8>` writeback target whose `Return` writes `globals[idx]`.
- `src/embed.rs` / `src/vm/vm.rs`: Session appends registered host names to the ambient
  set (compiler name-list + VM globals placeholders), **append-only** across loads so
  indices stay stable in the persistent REPL frame; a module registered between loads
  gets a fresh trailing index. Optional `Vm::host_module_names()`.
- Keep explicit `import` legal (no removal).
- Tests: ambient stdlib use with no import; shadowing by local + by explicit import;
  effectful module (`Net`) stays unbuilt when referenced only on a dead branch;
  memoization (source module's `.tg` runs exactly once across many references);
  host-ambient via Session; global-index stability across multiple Session loads,
  including a `register_module` between two loads.

LSP (host modules only; stdlib needs nothing):
- `src/vm/mod.rs`: `check_source` gains an extra-ambient-names parameter (default empty
  preserves current callers; thread through to the compiler).
- `crates/tigr-lsp/src/main.rs`: read `tigr.modules.json` + `initializationOptions` in
  `initialize`; pass ambient names into `compute_diagnostics` -> `check_source`.
- `src/catalog.rs`: `Catalog` holds `host_modules`; `module()/member()` consult it;
  `load_with_host(manifest)` or a merge method.
- Docs: document the sidecar format under `docs/`.

## Open / deferred

- Go-to-definition into ambient stdlib source modules stays the existing deferred LSP
  limitation (embedded sources, not on disk); unrelated to this work.
- Manifest hot-reload (file-watcher) can reuse the LSP's existing mtime re-stat; not
  required for v1.
