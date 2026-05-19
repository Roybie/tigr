# Playground UI — design contract

The tigr playground frontend is split in two:

- **Runtime glue** (`worker.js`, `app.js`) — owns all behavior: loading
  the WebAssembly VM, running code, rendering results, tab switching.
- **Visual design** — owns all layout and styling, authored separately
  in Claude design.

For the design to drop into the glue without rework, its exported
markup must carry the hook points below. The glue binds to these by
`id`; it never depends on visual styling, and it adds no styling of its
own beyond reusing the listed classes on elements it inserts.

`app.js` calls `bindUI()` on load and will log a console warning naming
any missing hook. Missing hooks degrade gracefully (that feature goes
dead) but should be treated as contract violations.

## Required hook points

### Tabs

| `id` | Element | Purpose |
|------|---------|---------|
| `tab-repl` | clickable | Activates the REPL console panel |
| `tab-editor` | clickable | Activates the editor playground panel |
| `panel-repl` | container | The REPL console panel |
| `panel-editor` | container | The editor playground panel |

The glue toggles a `is-active` class on the tab and panel of the
selected tab, and removes it from the other. The design styles
`.is-active` (e.g. selected tab, `display` of panels).

### REPL console panel (`#panel-repl`)

| `id` | Element | Purpose |
|------|---------|---------|
| `repl-scrollback` | scrollable container | Past entries are appended here |
| `repl-prompt` | text element | Shows `tigr>`, or `..>` mid multi-line |
| `repl-input` | `<textarea>` or `<input>` | Current line; Enter submits |
| `repl-reset` | clickable | Clears the session and scrollback |

Each submission appends one `.entry` element to `#repl-scrollback`,
built by the glue from these classes:

- `.entry-input` — the echoed source (prefixed with the prompt)
- `.entry-output` — captured `print` / `eprint` text (omitted if empty)
- `.entry-value` — the result value (omitted on error)
- `.entry-error` — the rendered error (only on error)

`.entry-output` and `.entry-error` hold pre-formatted monospace text
(the error carries caret-underlined source); the design must render
them with `white-space: pre` / `pre-wrap` in a monospace font.

### Editor playground panel (`#panel-editor`)

| `id` | Element | Purpose |
|------|---------|---------|
| `editor` | `<textarea>` (or mount) | The program source |
| `editor-run` | clickable | Runs the whole program fresh |
| `editor-stop` | clickable | Kills a runaway run |
| `editor-output` | container | Holds captured output + result/error |
| `editor-examples` | container of `.example-item`s | Picks a bundled example into `#editor` |

`#editor-examples` holds a list of `.example-item` buttons, each with a
`data-value` naming a key in `examples.js`. The glue delegates clicks on
the container, loads `EXAMPLES[data-value]` into `#editor`, and mirrors
selection by toggling `.is-active` on the chosen item. (The original
contract specified a `<select>`; the as-built design uses a `<nav>`
sidebar list, and the glue follows the `data-value` model above.)

`#editor` may be a plain `<textarea>` or a mount node for a richer
editor (CodeMirror) — the glue handles either. `#editor-output` is
populated with the same `.entry-output` / `.entry-value` /
`.entry-error` classes as the console.

### Status

| `id` | Element | Purpose |
|------|---------|---------|
| `status` | text element | Shows `running…`, `ready`, `stopped — session reset` |

## Notes for the design

- Style only; do not add scripts or rename the `id`s above.
- The glue sets `textContent` on the elements it fills — no HTML is
  injected, so styling is fully the design's domain.
- A plain monospace `<textarea>` is an acceptable `#editor`; richer
  editing is a later enhancement and not required by this contract.
