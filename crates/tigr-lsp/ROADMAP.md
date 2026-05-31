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
- Phase 5: cross-file and workspace. 5a go-to-definition, 5b
  multi-document lifecycle (mtime-cached `load_tree` + workspace roots),
  5c `workspace/symbol`, 5d cross-file hover/completion/signature-help
  for user-module members, 5e cross-file references + rename. See the
  "Phase 5x, as built" sections.
- Tier-2 / Tier-3 error recovery: the lexer and compiler now recover too,
  so every stage reports all of its errors at once. See "Tier-2/3
  recovery, as built".

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

### Phase 5a, as built — cross-file go-to-definition

Go-to-definition now follows a path-shaped `import` into the imported
file. `analysis::import_target` decides whether the cursor sits on a
cross-file target:

- the path string of an `import '<path>'` → `ImportTarget::Module`,
- the `member` of `alias.member` where `alias := import '<path>'` is a
  *file* import → `ImportTarget::Member { path, member }`.

Only path-shaped strings count (`is_file_path`: contains `/`, `\`, or
`.`), mirroring the VM — bare names are stdlib/native modules with no
on-disk location, and the catalog already covers them for hover.

`main.rs::import_definition` resolves the target: `resolve_import_url`
mirrors the VM's path rules (relative to the importing file's directory,
`.tg` appended when absent, `.`/`..` collapsed lexically by
`normalize_path` so `Url::from_file_path` accepts it). `load_tree` reads
the target — from the open-document cache if it is open (honouring
unsaved edits), else from the mtime-keyed foreign cache (see Phase 5b).
For a `Member`, `analysis::module_member_def` finds the definition in the
imported tree: it takes the module's final value (its export), and if
that is an object literal with a `member:` field, follows a bare-ident
value (`resolve: _resolve`) through to the top-level declaration it names
— otherwise it jumps to the field value, or falls back to a top-level
declaration named `member`. The whole resolution defaults to the file
head when the member can't be pinpointed, so it always opens the right
file.

Note the **local binding wins**: `goto_definition` tries the in-file
`analysis::definition` first, so jumping on the *receiver* `alias` lands
on its local `alias := import …` line (consistent with any local). The
cross-file path fires only when there is no local binding under the
cursor — the member, and the import path string.

Verified end-to-end over stdio against `examples/dns/` (`main.tg`
imports `./dns`): `DNS.resolve` → the `_resolve` decl in `dns.tg`;
`import './dns'` → `dns.tg` head; the `DNS` receiver → the local import
line in `main.tg`.

### Phase 5b, as built — multi-document lifecycle

`Backend::load_tree(uri)` is the single entry point for any file's text +
recovered AST. It tries, in order: the open-document cache (`tree_and_text`,
so unsaved edits win); a `foreign: HashMap<Url, CachedTree>` keyed by URI
and validated by mtime (a `CachedTree` stores the mtime it was read at, so
a later read re-parses only when the file changed on disk); otherwise it
reads + parses + caches. `initialize` captures the workspace roots
(`workspace_folders`, falling back to the deprecated `root_uri`), and
`workspace_tg_files` walks them for `.tg` files (skipping hidden dirs,
`target`, `node_modules`). This replaces 5a's parse-on-demand loader and
backs every cross-file feature below.

### Phase 5c, as built — workspace symbol search

`workspace/symbol`: walk `workspace_tg_files`, `load_tree` each, run the
existing `analysis::document_symbols`, and flatten the node tree to
`SymbolInformation`s (`collect_workspace_symbols`, parent name →
`container_name`) whose name contains the query (case-insensitive; empty
query → all). `selection`-range locations, so the client jumps to the
name.

### Phase 5d, as built — cross-file member intelligence

Hover, completion, and signature help now understand members of a *user
file* import, not just stdlib. The shared piece is
`analysis::module_exports(tree) -> Vec<ExportedMember>`: it reads the
module's final object literal and, for each `key: value` field, builds a
signature (`key(params)` when the value is — or re-exports — a `fn`,
following a bare-ident re-export to its top-level decl) plus the jump
span. `Backend::foreign_module(importer, program, receiver)` resolves a
receiver alias to a file import (`canonical_module` + `is_file_path`),
loads it, and returns its exports with the module URL and text.

- **Hover** (`foreign_member_hover`): the member's signature qualified
  with the receiver, a doc comment (`doc_comment_above` text-scans the
  contiguous `//` lines above the definition — the lexer drops comments,
  so the AST can't carry them), and the module file name.
- **Completion** after `userMod.`: the module's exported members, with
  signatures and doc comments. (The catalog path is unchanged for stdlib.)
- **Signature help** inside `userMod.member(`: the member's signature,
  via the `build_signature_help` helper factored out of the catalog path.

### Phase 5e, as built — cross-file references and rename

`Backend::member_occurrences(importer, program, offset)` powers both. When
the cursor is on a `userMod.member` access of a file import, it resolves
the module URL, then scans every workspace file (plus the importer and
module themselves, in case they sit outside the roots): for each alias in
that file that imports the same module (`import_alias_pairs` +
`resolve_import_url` comparison), it collects the spans of every
`alias.member` access (`member_access_spans`), and in the module file it
adds the export-object key. The key has no AST span (an
`ObjectMember::Pair` key is a bare `String`), so `export_object_span`
gives the object literal's range and `find_object_key_span` text-scans it
for the key (an identifier followed by `:`, which distinguishes a key from
a same-named value).

- `references` returns every occurrence (honouring `includeDeclaration` by
  filtering the export key); falls through to this only when there is no
  local binding under the cursor (so the receiver alias still gets local
  references).
- `rename` builds a multi-file `WorkspaceEdit`: the export key plus every
  `alias.member` access across importing files, all set to the new name
  (validated by `is_identifier`). The private implementation name (the
  re-export's value, e.g. `_resolve`) is deliberately untouched.
  `prepare_rename` offers the member key as a rename target too.

Verified end-to-end over stdio against `examples/dns/`: `workspace/symbol`
finds `_resolve`; hover/completion/signature-help on `DNS.resolve` show
`DNS.resolve(name, qtype?, server?)` and the other exports with their doc
comments; references return the `main.tg` access plus the `dns.tg` export
key; rename `resolve → lookup` edits both files (export key + access),
leaving `_resolve` alone.

### Phase 5, remaining (deferred polish)

- References/rename **triggered from the export-key site** in the module
  file (today they trigger from an access site; the key isn't an AST node,
  so the cursor there resolves to nothing).
- Go-to-definition / references into **bare stdlib** source modules
  (`Array`, `Math`, …): they're embedded, not on the user's disk, so there
  is no file to open.
- A **file-watcher**-driven index instead of the mtime re-stat on access
  (fine for the small projects tigr targets; revisit if it feels slow).

## Tier-2/3 recovery, as built

Before this, only parse errors came in multiples: the lexer and compiler
were fail-fast, so a single bad character or a single undeclared variable
masked everything after it. Now each stage recovers, so `check_source`
reports *all* of one stage's errors at once. The three kinds are still
never mixed — earliest non-empty stage wins (lex → parse → compile),
because a partial token stream or partial tree would spawn spurious
downstream errors.

- **Tier-2 — lexer recovery.** `Lexer::tokenize_recover() ->
  (Vec<SpannedToken>, Vec<LexError>)` skips the offending input on a bad
  token and keeps scanning. Every scanner already consumes past what it
  rejects before returning the error, so progress is guaranteed; no token
  is emitted for the bad region. `tokenize()` now delegates to it and
  returns the first error (the run path's unchanged contract).
  `vm::parse_tree` lexes with recovery too, so a stray bad character no
  longer wipes out the whole AST the resolver walks.
- **Tier-3 — compiler error accumulation.** The compiler carries a
  `Vec<CompileError>`. The common, recoverable user errors —
  `UndeclaredVariable`, `UndeclaredAssign`, `AssignToBuiltin`,
  `DuplicateDeclaration`, `BreakOutsideLoop`, `ContinueOutsideLoop`,
  `SpreadInInvalidPosition` — are `record`ed and compilation continues
  with a **stack-neutral fallback** (push a `null` where a value is
  expected, `Pop` where one is consumed, declare-anyway for a duplicate).
  Keeping the `stack_height` tracker balanced is what lets compilation
  press on without cascading internal panics. Internal-limit errors
  (`TooManyConstants/Locals/Upvalues`, `JumpTooFar`) and the structural
  `InvalidMatchPattern` stay **fatal** (propagate via `?`): the chunk is
  unusable after them and they aren't multi-error situations anyway.
- **Two entry points off one body.** `compile_main` does the work;
  `compile_with_source` (run path) reduces it to the first error via
  `into_run_result` — and the first *recorded* error is also the first in
  compilation order, since we only ever continue *past* recoverables, so
  the run path's behavior and every existing error test are unchanged.
  `compile_check` (LSP path) returns every error via `into_check_errors`
  (a fatal abort, if any, appended last). The REPL path keeps its
  first-error contract through `into_run_result_tuple`.
- Tests: `src/tests/mod.rs::multi_error` covers multiple lex errors,
  multiple undeclared variables, mixed recoverable compile errors,
  duplicate-decl recovery, and the no-stage-mixing invariant.

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
- **Tier-2 and Tier-3 error recovery. DONE.** See "Tier-2/3 recovery, as
  built" below.
- **Incremental document sync.** Currently full-document sync, which is
  fine for now; revisit only if large files feel slow.
- **Distribution. DONE.** `release.yml` now builds `tigr-lsp` alongside
  `tigr` (a second `cargo build -p tigr-lsp --bin tigr-lsp` step — the
  binary is in a non-default workspace member, so it needs `-p`) and tars
  *both* binaries into the existing `tigr-<target>.tar.gz`. They ship as
  one archive on purpose: `tigr-lsp` embeds the `tigr` frontend via a path
  dep, so bundling guarantees a user's runtime and language server are the
  same build and their diagnostics can't drift from the interpreter. No
  separate asset, no flag — `install.sh` extracts both and places
  `tigr-lsp` on PATH next to `tigr` (guarded by `-f` so an older archive
  without it still installs). The release matrix was already correct: it
  excludes Windows, which matches `tigr-lsp`'s constraints since it
  depends on the unix-only Net/Os `tigr` lib. Downstream step left to the
  user: once a release is installed, simplify the nvim `cmd` from the
  local `target/debug/tigr-lsp` to `{ "tigr-lsp" }`. Left their live dev
  config pointing at the debug build so iteration isn't broken.
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
6. ~~Cross-file/workspace (Phase 5).~~ **Done** — 5a go-to-def, 5b
   multi-document lifecycle, 5c `workspace/symbol`, 5d cross-file
   hover/completion/signature-help, 5e cross-file references + rename.
   See the "Phase 5x, as built" sections. Only deferred polish remains
   (export-site-triggered rename, stdlib-source navigation, a
   file-watcher index).
7. ~~Tier-2/3 recovery.~~ **Done** — see "Tier-2/3 recovery, as built".
8. ~~Distribution (ship the `tigr-lsp` binary via releases).~~ **Done** —
   `release.yml` + `install.sh` ship both binaries in one archive. See
   "Distribution. DONE." under Cross-cutting infrastructure. This was the
   last core roadmap item.

## Host module manifest (embedding), as built

An embedder (purr) registers native/source modules into the VM, and those
are *ambient* at runtime: game code uses them with no `import`. The server
runs the same checker but with no live VM, so it cannot know those names
exist. Without help it would flag a bare `Game.rect(...)` as an undeclared
variable and offer no hover/completion for `Game.*`.

The fix is a small JSON manifest the host ships, read at `initialize` from
two sources: a committable `tigr.modules.json` at a workspace root
(primary), then `initializationOptions` (an override the host can inject at
launch). Both feed two consumers:

1. **Diagnostics** — the module names go into `host_ambient`, passed to
   `check_source_with_ambient` so a bare reference resolves instead of
   flagging. (Stdlib needs nothing here: the compiler already seeds stdlib
   modules as ambient globals, so the server stopped flagging them the day
   ambient stdlib landed.)
2. **Catalog** — `Catalog::with_host_modules` folds the manifest's modules
   in next to the stdlib ones, so hover / completion / signature help cover
   host members. A host module never overrides a stdlib module of the same
   name (core wins, matching the runtime import order).

Schema (every field but the module name is optional):

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

A missing or malformed manifest is ignored, so plain stdlib editing never
depends on it. purr can generate the file from the modules it registers
(it knows the names and member names; signatures and docs come from its own
metadata, since `register_module` carries only names and arities).
