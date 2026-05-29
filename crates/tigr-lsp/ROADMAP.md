# tigr-lsp roadmap

Plan for the remaining language-server features. Lives with the code on
the `lsp` branch. See the commit history for what shipped; this file is
forward-looking.

## Done so far

- Phase 1: diagnostics over stdio, reusing the frontend via
  `vm::check_source`. UTF-8/UTF-16 position-encoding negotiation.
- Phase 2 foundation: error-recovering parser (`parser::parse_recover`),
  so multiple parse errors surface at once.
- Phase 2: go-to-definition and hover, via a lexical resolver over the
  recovered AST (`analysis.rs`, `vm::parse_tree`).
- Phase 3a: the symbol catalog (`catalog.rs`) and hover enrichment.
  Hover now covers `Module.member` access (including aliased imports),
  bare module names, builtins, keywords, and local functions (with their
  parameter list). See "Phase 3a, as built" below.
- Phase 3b: completion (member-after-dot + identifier completion).
- Phase 3c: signature help (active-parameter popup inside a call).
- Phase 4a: document symbols (outline of top-level declarations, with a
  function's nested declarations beneath it). See "Phase 4a, as built".
- AST binder spans: `ast::Binder { name, span }` now carries the span of
  every binding-name occurrence (pattern leaves, `...rest`, params, loop
  vars, the `catch` param, `=` targets). The prerequisite for 4b/4c.
- Phase 4b: references (`textDocument/references`).
- Phase 4c: rename (`textDocument/rename` + prepare-rename).
  See "Phase 4b/4c, as built".
- `match`-arm binding spans: `MatchPattern::Binding`, the `...rest` of
  array/object patterns, and the `${name}` object shorthand now carry
  spans (`Binder`, plus a `MatchField::key_span`). References and rename
  now work on match bindings too — the last binder-kind gap is closed.
- Parse-tree caching: the recovered AST is parsed once per document and
  reused across requests, invalidated on `did_change`. See
  "Parse-tree caching, as built".

## Guiding principles (unchanged)

- Reuse the language frontend; never fork the parser.
- Keep heavy LSP deps in `crates/tigr-lsp`, out of the `tigr` binary and
  the wasm build.
- Current-thread tokio runtime (the compiler uses the VM thread-local GC
  heap).

## The linchpin: a symbol catalog

Richer hover, completion, and signature help all need the same thing: a
catalog of the language's named entities with a signature and a
docstring. Build this once and three features fall out of it.

Entries to cover:

1. Global builtins. Names and arities are in `src/vm/stdlib.rs` (the
   `Spec { name, arity, .. }` table: `print`, `str`, `num`, `int`,
   `float`, `bool`, `floor`, `ceil`, `rand`, `type`, `gc`, `join`).
2. Stdlib module members. Names and arities are in
   `src/vm/native_modules/*.rs` (e.g. `("sqrt", native("sqrt",
   Arity::Exact(1), ...))`).
3. Docstrings. The best source is `stdlib/*.tg`: every public member has
   a `//` comment directly above it (e.g. Array.tg documents `push`,
   `pop`, etc.). `docs/stdlib/*.md` has longer prose per module.
4. Keywords. A fixed list with one-line explanations (`fn`, `if`, `for`,
   `while`, `match`, `try`/`catch`, `import`, `spawn`, `go`, `yield`,
   `return`, `break`, `continue`, `raise`).

How to build it (decision needed at implementation time):

- Option A, generate at build time. A `build.rs` (or a small committed
  generator) reads `stdlib/*.tg`, pairs each `member:` with its leading
  `//` block, joins arities from the native tables, and emits a static
  `catalog.rs`. Pro: zero startup cost, no runtime file access (works if
  the binary is moved). Con: a build step to maintain.
- Option B, load at startup. The server already links `tigr`; it can read
  the stdlib sources (they ship in `source_stdlib`) and parse the doc
  comments once on `initialize`. Pro: always in sync, simplest. Con:
  needs the comments preserved through the lexer, or a light re-scan of
  the raw text.

Recommendation considered: Option B scanning the raw `stdlib/*.tg` text.

**What shipped (Option C — docs-driven).** Neither A nor B: the catalog
is parsed from the committed reference Markdown under `docs/stdlib/`,
embedded with `include_str!`. Every module page and `builtins.md` share
one shape — a `` # `Name` `` header, an intro paragraph, then one
`` ### `signature` `` detail section per member (functions *and*
constants like `Math.PI`). Each section's heading gives the signature;
the prose up to its first code fence is the docstring. This covers *all*
modules uniformly — source (Array, Math, …) and native (JSON, IO, Path,
Time, DateTime, Random, Bytes, BigInt, Os, Net) — with no runtime/heap
coupling, no AST parsing of `.tg`, and no native-table scan. It can't
drift from a moved binary and stays in sync with the docs the same edit
already touches. Cost: a member missing a `### ` section (or a new
module not added to `MODULE_DOCS`) won't appear; the
`every_module_doc_parses_to_some_members` test guards the gross failure
mode. Keywords aren't in the docs pages, so they're a hand-written table
in `catalog.rs` mirroring the lexer's keyword tokens.

## Phase 3: hover and completion

### Phase 3a, as built

Hover (`analysis::hover` + `keyword_hover` in `main.rs`) tries, in order:
- `Module.member` under the cursor — `Index(Ident(mod), Str(member))`,
  with the member `Str` carrying the exact name span. Import aliases are
  resolved first (`import_aliases`), so `M := import 'Math'; M.sqrt`
  canonicalizes `M` → `Math` before the catalog lookup.
- A local binding: a `name := fn(...)` decl shows its signature and
  "function"; any binding that is itself an import of a known module
  shows the module doc; otherwise the binding kind plus definition line.
- A bare module name or builtin: queried directly from the catalog.
- A keyword token: its one-line explanation (token-stream scan, since
  keywords aren't AST identifiers).

Rendered as Markdown — a signature in a `tigr` code fence, then the doc.

### Phase 3b, as built

Completion (`textDocument/completion`, `completion_items` in `main.rs`),
with `.` registered as a trigger character:
- **Member completion after `.`.** A backward text scan
  (`member_trigger`) over `<receiver> . <partial>` finds the receiver
  identifier even when the buffer is mid-edit and won't parse into a
  clean `Index` (the common case right after typing the dot). The
  receiver is canonicalized through `analysis::canonical_module`, then
  the catalog's members are offered. Constants get
  `CompletionItemKind::CONSTANT`, others `FUNCTION`. Nothing else is
  offered in member position (no builtin/keyword leakage).
- **Identifier completion elsewhere.** In-scope locals
  (`analysis::locals_in_scope`, reusing the resolver's scope walk, with a
  `fn` decl's signature in the detail), builtins, module names, and
  keywords.
- Each item carries `kind`, `detail` (the signature), and `documentation`
  (the catalog docstring as Markdown). Prefix filtering is left to the
  client.
- Supersedes the static omnifunc in `vim-tigr`.

Not yet done in 3b: completing a partial *member* of a user object/local
(only catalog modules are offered after `.`); ranking/sorting hints
(`sortText`); snippet insertion of call parens.

### Phase 3c, as built

Signature help (`textDocument/signatureHelp`, `signature_help` in
`main.rs`), with `(` and `,` as trigger/retrigger characters:
- **Call detection is a backward scan** (`call_context`), mirroring 3b:
  from the cursor, skip balanced brackets to the enclosing unclosed `(`,
  counting top-level commas as the active-parameter index. A `[`/`{` at
  depth 0 means the cursor is in an array/object literal, not a call, so
  it bails. Robust mid-edit (the call usually won't parse yet).
- **Callee** (`callee_before`): the identifier — or `receiver.member` —
  immediately before that `(`. A paren with no leading identifier is a
  grouping paren, not a call.
- **Resolution** (`resolve_callee`): a member access goes through the
  catalog (alias-canonicalized); a bare name is a local `fn` first (its
  recorded decl signature), then a builtin. The signature string is
  reused as the popup label; `parse_params` splits its parameter list on
  top-level commas to build the highlightable parameters. The active
  index is clamped onto the last parameter (so an extra arg to a variadic
  still shows the popup rather than dropping it).

### Remaining (signature help polish)

- Multiple signatures / overloads: tigr has none, so one signature only.
- Per-parameter docs (`ParameterInformation.documentation`): not split
  out of the member docstring yet.

## Phase 4: navigation and editing

### Phase 4a, as built

Document symbols (`textDocument/documentSymbol`,
`analysis::document_symbols` + `to_document_symbol` in `main.rs`). Walks
the recovered tree for top-level `:=` declarations and emits a nested
`DocumentSymbol` tree:
- A `name := fn(...)` is a `FUNCTION` whose `detail` is the signature
  (reusing `fn_signature`); its body block is walked recursively so
  nested declarations appear as children.
- A `name := import 'Mod'` is a `MODULE` with `detail` `import 'Mod'`.
- Any other `name := …` is a `VARIABLE`.
- Destructuring decls (`[a, b] := …`, `${x} := …`) expand to one
  `VARIABLE` per bound name; since the AST keeps no per-name span, they
  share the declaration's range (the same span limitation as go-to-def).
The `range` covers the whole declaration; the `selection_range` covers
just the name (`decl_span.start .. start + name.len()`, the same
name-at-start fact go-to-def relies on). Only declarations surface —
other statements are skipped — and only function bodies are descended
into, so the outline stays a clean decl tree rather than every block.
`analysis` stays free of `tower_lsp`: it returns a `SymbolNode` tree with
a local `SymbolCategory`, which `main.rs` maps to the LSP types.

Original plan: Outline of top-level declarations and nested functions.
Small, high value (powers breadcrumbs, telescope symbol search, the
outline view). Only needs decl spans, which exist.

### Phase 4b/4c, as built

The AST-span prerequisite (see "Cross-cutting") landed first:
`ast::Binder { name, span }` replaces the bare `String` at every binding
site. It `Deref`s to `str`, so the compiler/fold keep reading just the
name; the parser fills in each binder's span. This made go-to-definition
work for params, loop vars, destructured names, and the `catch` param
too (previously `def: None`).

References and rename share one engine in `analysis.rs`:
`collect_occurrences` walks the tree once and records every name
occurrence as `(name, span, role)`. A `Role::Def` (a decl/param/loop/
catch binder) has identity = its own span; a `Role::Ref` (an
`Expr::Ident`, an `=` target, or a destructuring-assignment leaf) has
identity = the def span of the binding it resolves to (via `binding_of`,
which reuses the scope walk). Two occurrences belong together iff their
identities match — def-span identity distinguishes shadowed bindings of
the same name. Compiler-internal `$`-names and unspanned `match` bindings
(identity `None`) are never grouped or renamed.

- `references` (`main.rs::references`) returns every occurrence's
  location; honours `includeDeclaration`.
- `rename_spans` returns the def span plus all occurrence spans;
  `main.rs::rename` turns them into a single-file `WorkspaceEdit` after
  validating the new name with `is_identifier`. `prepare_rename`
  validates the cursor target up front (returns the range under the
  cursor, or nothing when the target can't be renamed).

All binder kinds now carry spans (see "Match-arm binding spans" below),
so references and rename work for every local binding including match
arms. Cross-file rename waits on Phase 5.

### Match-arm binding spans, as built

The last binder kind that stored a bare `String` in the core AST. Now:
- `MatchPattern::Binding(Binder)` (was `Binding(String)`),
- `MatchPattern::Array { rest: Option<Binder> }` and
  `Object { rest: Option<Binder> }` (were `Option<String>`),
- `MatchField` gained a `key_span: Span` so the `${name}` *shorthand*
  binding (where the key *is* the bound name) is locatable; the field
  `key` itself stays a `String` (the compiler reads it as the object
  key). The synthetic `select`-desugar fields carry the channel span.

The parser records each span via `peek_span`/`parse_ident_binder`. The
compiler was untouched beyond the field-type change — it reads names
through `Binder`'s `Deref<str>` exactly as before. In `analysis.rs`,
`match_bindings` now binds via `bind_binder` (spanned) instead of the
removed unspanned `bind_name`, and `collect_occurrences` gained an
`Expr::Match` arm (`collect_match_pattern_occurrences`) that emits a
`Def` occurrence per pattern binding. So a match binding is now a
first-class definition for go-to-def, references, and rename.

### Parse-tree caching, as built

Hover, definition, completion, signature help, references, and rename
all need the recovered AST; previously each re-ran `vm::parse_tree` per
request. Now a `Document { text, tree: Option<Arc<Block>> }` (replacing
the bare `String` in `Backend::docs`) caches the parse. `tree_and_text`
parses lazily on first use and returns an `Arc` clone; `analyze` layers
the position→offset conversion on top (the common entry point for
position-based requests). `did_change` replaces the `Document` with a
fresh one (`tree: None`), invalidating the cache; `did_open` starts with
`tree: None`. The parse runs under the `docs` mutex, which is safe
because no `.await` is held across it and the current-thread runtime
serialises requests. Keyed on document identity + version-by-replacement
rather than a text hash, since full-sync already hands us the whole text.

## Phase 5: cross-file and workspace

- Import resolution. `import 'Foo'` resolves to `Foo.tg` relative to the
  file (and the stdlib path). Enables go-to-definition into imported
  modules and cross-file references.
- Workspace symbol search (`workspace/symbol`).
- Multi-document lifecycle: resolve and cache trees for files reached
  through imports, not just open buffers.

## Cross-cutting infrastructure

- **AST spans on patterns and params (core change). DONE.** `Pattern`,
  `Assign` targets, and `Fn`/`For`/`Try` binders now hold an
  `ast::Binder { name, span }` instead of a bare `String`. `Binder`
  `Deref`s to `str`, so `compiler.rs`/`fold.rs` were almost untouched
  (only sites that needed a `String` — `fn_name_hint`, a couple of error
  variants — changed to `.name.clone()`; `compile_for`/`compile_try`
  signatures took `&[Binder]`/`&(Binder, _)`). The parser records each
  span via `parse_ident_binder`. This was the prerequisite for 4b/4c and
  also made params/loop-vars/destructured-names jumpable. The one
  remaining gap was `MatchPattern` bindings — **now closed** (see
  "Match-arm binding spans, as built"). Every binder kind carries a span.
- **Parse-tree caching. DONE.** Hover, definition, completion, signature
  help, references, and rename reuse one cached recovered AST per
  document, invalidated on `did_change`. See "Parse-tree caching, as
  built".
- **Tier-2 and Tier-3 error recovery.** The lexer and compiler are still
  fail-fast, so only parse errors come in multiples. Lexer recovery
  (emit an error token, continue) and compiler error accumulation would
  surface multiple lex and semantic errors.
- **Incremental document sync.** Currently full-document sync, which is
  fine for now; revisit only if large files feel slow.
- **Distribution.** The nvim config points at a local debug build. To
  ship it, add `--bin tigr-lsp` plus a tarball to `release.yml`, and let
  `install.sh` (or a flag) place it on PATH; then simplify the nvim `cmd`
  to `{ "tigr-lsp" }`.
- **Tests.** `catalog.rs` and `analysis::hover` now have Rust unit tests
  (catalog parsing per module, plus the hover paths). Still wanted: tests
  for the resolver's go-to-definition/scoping, and a small in-process LSP
  harness to replace the ad-hoc Python stdio scripts (the keyword-hover
  path in `main.rs` is still only covered that way).

## Suggested order

1. ~~Symbol catalog, then hover enrichment (3a).~~ **Done.** The catalog
   (`catalog.rs`) and enriched hover have shipped.
2. ~~Completion (3b), then signature help (3c).~~ **Done.** Both reuse
   `catalog.rs`; 3b/3c also reuse `analysis::locals_in_scope` and the
   `name := fn(...)` signatures the resolver records.
3. ~~Document symbols (4a).~~ **Done.** A decl-tree outline reusing the
   recovered AST and `fn_signature`; functions nest their inner decls.
4. ~~AST spans for patterns/params, then references (4b) and rename
   (4c).~~ **Done**, including `match`-binding spans. See "Phase 4b/4c"
   and "Match-arm binding spans, as built".
5. ~~Parse-tree caching once requests multiply.~~ **Done.** See
   "Parse-tree caching, as built".
6. Cross-file/workspace (Phase 5).
7. Distribution and Tier-2/3 recovery as polish.
