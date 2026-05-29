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

Known weak spot driving this plan: **hover is thin.** It only describes
local bindings (kind plus definition line). It says nothing for the
things people most want to inspect: builtins, stdlib members like
`Math.sqrt`, and keywords.

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

Recommendation: start with Option B scanning the raw `stdlib/*.tg` text
(cheapest path to value), move to a generated catalog only if startup
cost or sync ever bites. Seed the member-name lists from
`vim-tigr/autoload/tigr.vim` (already curated) to cross-check coverage.

## Phase 3: hover and completion

This is the user-facing priority because of the thin hover.

3a. Hover enrichment.
- Detect `Module.member` under the cursor. In the AST that is
  `Index(Ident("Math"), Str("sqrt"))` (dot-access lowers to `Index` with
  a string key), so look for that shape and query the catalog.
- For a bare builtin or module name, query the catalog directly.
- For a keyword token, show its one-line explanation.
- For a local binding, improve the current output: if the decl is
  `name := fn(...)`, show "function" plus its parameter list (read from
  the `Fn` node); otherwise show the binding kind as today.
- Render as Markdown: a signature line in a tigr code fence, then the
  doc text.

3b. Completion (`textDocument/completion`).
- Member completion after `.`: when the cursor follows `Ident("Mod").`,
  offer that module's members from the catalog.
- Identifier completion elsewhere: in-scope locals (reuse the resolver's
  scope walk), builtins, module names, and keywords.
- Each `CompletionItem` carries `kind`, `detail` (the signature), and
  `documentation` (the catalog docstring).
- This supersedes the static omnifunc in `vim-tigr`.

3c. Signature help (`textDocument/signatureHelp`).
- While typing arguments inside a call, show the callee's parameters and
  highlight the active one. For builtins and stdlib, use the catalog
  arity; for user functions, read params from the `Fn` decl the callee
  resolves to.

## Phase 4: navigation and editing

4a. Document symbols (`textDocument/documentSymbol`). Outline of
top-level declarations and nested functions. Small, high value (powers
breadcrumbs, telescope symbol search, the outline view). Only needs decl
spans, which exist.

4b. References (`textDocument/references`). Find every `Ident` that
resolves to the same binding as the one under the cursor. Reuses the
resolver. Needs the AST span work below to be complete for params and
loop variables.

4c. Rename (`textDocument/rename`). Rename a binding across its scope.
Depends on references plus precise binding spans, so it follows the AST
span work.

## Phase 5: cross-file and workspace

- Import resolution. `import 'Foo'` resolves to `Foo.tg` relative to the
  file (and the stdlib path). Enables go-to-definition into imported
  modules and cross-file references.
- Workspace symbol search (`workspace/symbol`).
- Multi-document lifecycle: resolve and cache trees for files reached
  through imports, not just open buffers.

## Cross-cutting infrastructure

- **AST spans on patterns and params (core change).** Today `Pattern`,
  `Assign` targets, and `Fn`/`For` binders store a bare `String` with no
  span, which is why go-to-definition only works for `name := ...` and
  why params and loop vars cannot be jumped to, found, or renamed. Adding
  a span beside each binding name in the AST (recorded by the parser,
  ignored by the compiler) is the single biggest enabler. Do it before
  references and rename. It ripples through `ast.rs`, `parser.rs`, and
  the exhaustive matches in `compiler.rs`/`fold.rs`.
- **Parse-tree caching.** Hover, definition, and completion each call
  `vm::parse_tree`, so the file is re-parsed per request. Cache one tree
  per document, keyed on the text (or version), and invalidate on change.
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
- **Tests.** `analysis.rs` (the resolver) should get Rust unit tests, and
  a small in-process LSP harness would replace the ad-hoc Python stdio
  scripts used to verify behavior so far.

## Suggested order

1. Symbol catalog, then hover enrichment (3a). Directly fixes the thin
   hover.
2. Completion (3b), then signature help (3c). Same catalog.
3. Document symbols (4a). Cheap and useful.
4. AST spans for patterns/params, then references (4b) and rename (4c).
5. Parse-tree caching once requests multiply.
6. Cross-file/workspace (Phase 5).
7. Distribution and Tier-2/3 recovery as polish.
