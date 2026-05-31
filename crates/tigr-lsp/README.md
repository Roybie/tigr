# tigr-lsp

The language server for tigr. It speaks LSP over stdio and reuses the
compiler frontend, so its diagnostics are exactly what `tigr run` would
report. Features: diagnostics, go-to-definition, hover, completion,
signature help, document and workspace symbols, references, and rename,
across files.

## Running it

Build the binary and point your editor at it:

```
cargo build -p tigr-lsp --release
# binary at target/release/tigr-lsp
```

Most editors launch the server themselves once told the command. The
server negotiates UTF-8 position encoding when the client offers it
(Neovim does) and otherwise uses UTF-16.

## Stdlib needs no setup

Every stdlib module is ambient: a bare `Math.sqrt(x)` or `JSON.parse(s)`
resolves with no `import`. The server knows this, so it never flags a
stdlib reference as undeclared, and hover/completion cover the modules out
of the box.

## Host module manifest

When tigr is embedded in a host application, the host can register its own
native or source modules with the VM (see `embed::Session::register_module`
/ `register_source_module`). Those modules are ambient too: game or plugin
code calls them with no `import`. The game framework **purr** registers a
`Game` surface this way, for example.

The server runs the checker with no live VM, so on its own it cannot know
those host modules exist. Without help it would flag a bare `Game.rect(...)`
as an undeclared variable and offer no hover or completion for `Game.*`. A
small JSON manifest fixes that.

### Where it is read from

At `initialize`, the server reads host modules from two sources, in order:

1. **`tigr.modules.json`** at a workspace root. This is the primary
   source: commit it next to the project so every contributor's editor
   picks it up with no per-machine setup. The first such file found across
   the workspace roots is used.
2. **`initializationOptions`** sent by the client. These merge on top of
   the file, so a host that launches the server itself can inject or
   override modules at startup.

A missing or malformed manifest is ignored, so plain stdlib editing never
depends on one.

### Format

```json
{
  "modules": {
    "Game": {
      "description": "purr game engine surface",
      "members": {
        "rect":   { "signature": "rect(x, y, w, h) -> Null", "doc": "Draw a filled rectangle." },
        "sprite": { "signature": "sprite(id, x, y) -> Null", "doc": "Blit a sprite by id." }
      }
    },
    "Audio": {
      "description": "sound playback",
      "members": {
        "play": { "signature": "play(id) -> Null" }
      }
    }
  }
}
```

Every field but the module name is optional:

- `description` is shown when you hover the bare module name.
- `members` maps a member name to its `signature` and `doc`. `signature`
  defaults to the bare member name; `doc` defaults to empty. Both feed
  hover, completion, and signature help.

The same shape is accepted under `initializationOptions` (the top-level
`modules` key).

### What it changes

- **Diagnostics.** The module names are treated as resolvable, so a bare
  reference to a host module is no longer flagged as an undeclared
  variable. The names only need to be present for this; members are not
  checked (member access is dynamic in tigr).
- **Hover, completion, signature help.** Host modules and their members
  are folded in next to the stdlib ones. A host module never overrides a
  stdlib module of the same name: core always wins, matching the runtime
  import order.

### Generating it

A host knows the modules and member names it registers, so it can emit the
manifest as part of its build or project scaffolding. Signatures and
docstrings come from the host's own metadata, since `register_module`
carries only names and arities. Checking `tigr.modules.json` into the
project repository is the intended workflow.
