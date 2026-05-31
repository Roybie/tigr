//! Tigr language server (Phase 1: diagnostics).
//!
//! Speaks LSP over stdio. On every open/change/save it re-runs the tigr
//! frontend — `lexer → parser → fold → compiler`, via
//! [`tigr::vm::compile_source_with_id`] — without executing, and turns
//! the first lex/parse/compile error into a `Diagnostic`. The frontend
//! is fail-fast (one error at a time); error-recovering multi-error
//! parsing is the documented first step of Phase 2.
//!
//! Run on a current-thread runtime: the compiler allocates into the VM's
//! thread-local GC heap, so each compile and the drop of its result must
//! stay pinned to one thread.

mod analysis;
// The catalog now lives in the `tigr` lib (so the wasm playground can
// reuse it too); re-export it under `crate::catalog` so the rest of this
// crate's `crate::catalog::*` paths are unchanged.
use tigr::catalog;

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use tigr::vm::ast::Block;
use tigr::vm::check_source_with_ambient;
use tigr::vm::error::Error as TigrError;
use tigr::vm::lexer::Lexer;
use tigr::vm::source_map::SourceId;
use tigr::vm::token::Token;

use crate::catalog::Catalog;

/// One open document: its full text (full-sync) and a lazily-parsed,
/// cached recovered AST. The tree is computed on the first request that
/// needs it and reused until the text changes, when `did_change` drops it.
struct Document {
    text: String,
    tree: Option<Arc<Block>>,
}

/// A disk file parsed for a cross-file request, cached alongside the mtime
/// it was read at so a later read re-parses only when the file changed.
struct CachedTree {
    mtime: Option<std::time::SystemTime>,
    text: String,
    tree: Arc<Block>,
}

struct Backend {
    client: Client,
    /// Open documents, keyed by URI.
    docs: Mutex<HashMap<Url, Document>>,
    /// Closed/imported files parsed on demand for cross-file requests,
    /// keyed by URI and validated by mtime. Open documents (in `docs`)
    /// always take precedence so unsaved edits win.
    foreign: Mutex<HashMap<Url, CachedTree>>,
    /// Workspace folder paths captured at `initialize`, searched by
    /// `workspace/symbol` and the cross-file reference/rename scan.
    roots: Mutex<Vec<PathBuf>>,
    /// Position encoding negotiated in `initialize`. tigr spans are byte
    /// offsets; this decides whether a column counts bytes (UTF-8) or
    /// UTF-16 code units when we project an offset onto an LSP position.
    encoding: Mutex<PositionEncodingKind>,
    /// Builtins, stdlib members, and keywords with signatures and docs,
    /// parsed once from the embedded reference docs. Powers hover. Behind
    /// a lock because `initialize` may rebuild it to fold in host modules
    /// from a `tigr.modules.json` manifest; read-only thereafter.
    catalog: RwLock<Catalog>,
    /// Names of host-registered ambient modules (from the manifest),
    /// passed to the checker so a bare reference to one is not flagged as
    /// an undeclared variable. Empty unless an embedder ships a manifest.
    host_ambient: Mutex<Vec<String>>,
}

impl Backend {
    /// A document's text and its recovered AST, parsing and caching the
    /// tree on first use. `None` if the document isn't open. The parse is
    /// done under the `docs` lock; it holds no `.await`, so requests (which
    /// the current-thread runtime serialises) never observe a half-built
    /// cache. Returns an `Arc` clone, so the caller works off a snapshot
    /// even if a later `did_change` invalidates the entry.
    fn tree_and_text(&self, uri: &Url) -> Option<(String, Arc<Block>)> {
        let mut docs = self.docs.lock().unwrap();
        let doc = docs.get_mut(uri)?;
        if doc.tree.is_none() {
            doc.tree = Some(Arc::new(tigr::vm::parse_tree(&doc.text)));
        }
        Some((doc.text.clone(), doc.tree.clone().unwrap()))
    }

    /// Like [`Self::tree_and_text`] but also converts an LSP position into
    /// a byte offset using the negotiated encoding. The common entry point
    /// for position-based requests (hover, definition, references, …).
    fn analyze(&self, uri: &Url, pos: Position) -> Option<(String, usize, Arc<Block>)> {
        let (text, tree) = self.tree_and_text(uri)?;
        let enc = self.encoding.lock().unwrap().clone();
        let offset = position_to_offset(&text, pos, &enc);
        Some((text, offset, tree))
    }

    /// The text and recovered AST of `uri`, whether or not it is an open
    /// document. An open buffer comes from the cache (so unsaved edits are
    /// honoured); any other file is read from disk, parsed, and cached
    /// against its mtime — a later read re-parses only if the file changed.
    fn load_tree(&self, uri: &Url) -> Option<(String, Arc<Block>)> {
        if let Some(found) = self.tree_and_text(uri) {
            return Some(found);
        }
        let path = uri.to_file_path().ok()?;
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        // Cache hit only while the file is unchanged since we parsed it.
        if let Some(c) = self.foreign.lock().unwrap().get(uri) {
            if c.mtime == mtime {
                return Some((c.text.clone(), c.tree.clone()));
            }
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let tree = Arc::new(tigr::vm::parse_tree(&text));
        self.foreign.lock().unwrap().insert(
            uri.clone(),
            CachedTree { mtime, text: text.clone(), tree: tree.clone() },
        );
        Some((text, tree))
    }

    /// If `receiver` is a *file*-import alias in `program`, load the
    /// imported module and return its exports with the module's URL and
    /// text. `None` for stdlib/native receivers (the catalog covers those)
    /// and for anything that doesn't resolve to a readable file.
    fn foreign_module(
        &self,
        importer: &Url,
        program: &Block,
        receiver: &str,
    ) -> Option<ForeignModule> {
        let path = analysis::canonical_module(program, receiver);
        if !analysis::is_file_path(&path) {
            return None;
        }
        let url = resolve_import_url(importer, &path)?;
        let (text, tree) = self.load_tree(&url)?;
        Some(ForeignModule { url, text, exports: analysis::module_exports(&tree) })
    }

    /// Hover for `userMod.member` where `userMod` is a file import: the
    /// member's signature, its doc comment, and the module file it lives in.
    fn foreign_member_hover(
        &self,
        importer: &Url,
        program: &Block,
        offset: usize,
    ) -> Option<String> {
        let (receiver, member) = analysis::member_at(program, offset)?;
        let fm = self.foreign_module(importer, program, &receiver)?;
        let m = fm.exports.iter().find(|e| e.name == member)?;
        Some(render_foreign_member(&receiver, m, &fm))
    }

    /// Every occurrence of the exported member under `offset`, across the
    /// workspace: each `alias.member` access in every file that imports the
    /// same module, plus the member's key in the module's export object.
    /// `None` unless the cursor is on a `userMod.member` access of a file
    /// import. The result drives cross-file references and rename.
    fn member_occurrences(
        &self,
        importer: &Url,
        program: &Block,
        offset: usize,
    ) -> Option<MemberRefs> {
        let (receiver, member) = analysis::member_at(program, offset)?;
        let path = analysis::canonical_module(program, &receiver);
        if !analysis::is_file_path(&path) {
            return None;
        }
        let module_url = resolve_import_url(importer, &path)?;
        let enc = self.encoding.lock().unwrap().clone();

        // Scan every workspace `.tg` file, plus the importer and module
        // themselves in case they sit outside the configured roots.
        let mut files = self.workspace_tg_files();
        for u in [importer.clone(), module_url.clone()] {
            if !files.contains(&u) {
                files.push(u);
            }
        }

        let mut occurrences = Vec::new();
        let mut decl = None;
        for uri in files {
            let Some((text, tree)) = self.load_tree(&uri) else {
                continue;
            };
            // Member accesses through any alias that imports this module.
            for (alias, p) in analysis::import_alias_pairs(&tree) {
                if !analysis::is_file_path(&p)
                    || resolve_import_url(&uri, &p).as_ref() != Some(&module_url)
                {
                    continue;
                }
                for span in analysis::member_access_spans(&tree, &alias, &member) {
                    occurrences.push(Location { uri: uri.clone(), range: span_to_range(&text, span, &enc) });
                }
            }
            // The export key in the module's own object literal.
            if uri == module_url {
                if let Some(obj_span) = analysis::export_object_span(&tree) {
                    if let Some(key_span) = find_object_key_span(&text, obj_span, &member) {
                        let loc = Location { uri: uri.clone(), range: span_to_range(&text, key_span, &enc) };
                        occurrences.push(loc.clone());
                        decl = Some(loc);
                    }
                }
            }
        }
        if occurrences.is_empty() {
            return None;
        }
        Some(MemberRefs { occurrences, decl })
    }

    /// Every `.tg` file under the workspace roots, as URLs. Used by
    /// `workspace/symbol` and the cross-file reference/rename scan.
    fn workspace_tg_files(&self) -> Vec<Url> {
        let roots = self.roots.lock().unwrap().clone();
        let mut out = Vec::new();
        for root in roots {
            collect_tg_files(&root, &mut out);
        }
        out
    }

    /// Resolve a cross-file go-to-definition. If `offset` sits on an import
    /// target — the path string of an `import`, or `alias.member` of a file
    /// import — return a `Location` in the imported file: the member's
    /// declaration when known, otherwise the file head.
    fn import_definition(
        &self,
        importer: &Url,
        program: &Block,
        offset: usize,
    ) -> Option<Location> {
        let (path, member) = match analysis::import_target(program, offset)? {
            analysis::ImportTarget::Module { path } => (path, None),
            analysis::ImportTarget::Member { path, member } => (path, Some(member)),
        };
        let target_uri = resolve_import_url(importer, &path)?;
        let (text, tree) = self.load_tree(&target_uri)?;
        // The member's definition span, defaulting to the file head (so a
        // member we can't pinpoint still opens the right file).
        let range = member
            .and_then(|m| analysis::module_member_def(&tree, &m))
            .map(|span| {
                let enc = self.encoding.lock().unwrap().clone();
                span_to_range(&text, span, &enc)
            })
            .unwrap_or_default();
        Some(Location { uri: target_uri, range })
    }

    /// Recompile `text` and publish the resulting diagnostics for `uri`.
    async fn publish(&self, uri: Url, text: &str, version: Option<i32>) {
        let enc = self.encoding.lock().unwrap().clone();
        let host_ambient = self.host_ambient.lock().unwrap().clone();
        let diagnostics = compute_diagnostics(text, &uri, &enc, &host_ambient);
        self.client
            .publish_diagnostics(uri, diagnostics, version)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Honour UTF-8 if the client offers it (Neovim does) so we can use
        // byte columns directly; otherwise fall back to the LSP default of
        // UTF-16 and count code units.
        let encoding = params
            .capabilities
            .general
            .and_then(|g| g.position_encodings)
            .and_then(|encs| {
                encs.into_iter()
                    .find(|e| *e == PositionEncodingKind::UTF8)
            })
            .unwrap_or(PositionEncodingKind::UTF16);
        *self.encoding.lock().unwrap() = encoding.clone();

        // Capture the workspace roots for cross-file search. Prefer the
        // (possibly multi-root) `workspace_folders`; fall back to the
        // deprecated `root_uri` for older clients.
        let mut roots: Vec<PathBuf> = params
            .workspace_folders
            .into_iter()
            .flatten()
            .filter_map(|f| f.uri.to_file_path().ok())
            .collect();
        if roots.is_empty() {
            if let Some(p) = params.root_uri.and_then(|u| u.to_file_path().ok()) {
                roots.push(p);
            }
        }

        // Host module manifest: a committable `tigr.modules.json` in a
        // workspace root (primary), then `initializationOptions` (an
        // override an embedder can inject at launch). Both teach the
        // server about an embedder's ambient modules — so a bare
        // reference doesn't false-flag as undeclared, and hover /
        // completion / signature help cover the host members.
        let mut host_modules: HashMap<String, catalog::Module> = HashMap::new();
        let mut host_names: Vec<String> = Vec::new();
        if let Some(v) = load_manifest_file(&roots) {
            merge_manifest(&v, &mut host_modules, &mut host_names);
        }
        if let Some(v) = &params.initialization_options {
            merge_manifest(v, &mut host_modules, &mut host_names);
        }
        *self.host_ambient.lock().unwrap() = host_names;
        *self.catalog.write().unwrap() =
            catalog::Catalog::with_host_modules(host_modules);

        *self.roots.lock().unwrap() = roots;

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "tigr-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                position_encoding: Some(encoding),
                // Full-document sync keeps Phase 1 simple: every change
                // ships the whole buffer, which we recompile wholesale.
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // `.` re-triggers so member completion fires right
                    // after the dot; identifiers trigger as the client
                    // sees fit (typically on any word character).
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    // `(` opens the arg list; `,` advances to the next
                    // parameter and re-triggers.
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    // We support prepare-rename so the editor can validate
                    // the cursor target (and seed the rename box) first.
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "tigr-lsp ready")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.docs.lock().unwrap().insert(
            doc.uri.clone(),
            Document { text: doc.text.clone(), tree: None },
        );
        self.publish(doc.uri, &doc.text, Some(doc.version)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync → the last change carries the entire new text.
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        // New text → drop the cached tree; it is reparsed on next request.
        self.docs
            .lock()
            .unwrap()
            .insert(uri.clone(), Document { text: change.text.clone(), tree: None });
        self.publish(uri, &change.text, Some(version)).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        // Prefer the text the client sent on save; fall back to our cache.
        let text = params
            .text
            .or_else(|| self.docs.lock().unwrap().get(&uri).map(|d| d.text.clone()));
        if let Some(text) = text {
            self.publish(uri, &text, None).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.docs.lock().unwrap().remove(&uri);
        // Clear diagnostics for a closed file so stale squiggles don't
        // linger in the client.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let p = params.text_document_position_params;
        let uri = p.text_document.uri;
        let Some((text, offset, program)) = self.analyze(&uri, p.position) else {
            return Ok(None);
        };
        // A local binding wins: jump within this file.
        if let Some(span) = analysis::definition(&program, offset) {
            let enc = self.encoding.lock().unwrap().clone();
            let range = span_to_range(&text, span, &enc);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location { uri, range })));
        }
        // Otherwise, the cursor may sit on a cross-file import target.
        if let Some(loc) = self.import_definition(&uri, &program, offset) {
            return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
        }
        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let p = params.text_document_position_params;
        let uri = p.text_document.uri;
        let Some((text, offset, program)) = self.analyze(&uri, p.position) else {
            return Ok(None);
        };
        let cat = self.catalog.read().unwrap();
        let markdown = analysis::hover(&program, offset, &cat)
            .or_else(|| self.foreign_member_hover(&uri, &program, offset))
            .or_else(|| keyword_hover(&text, offset, &cat));
        let Some(markdown) = markdown else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        }))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let p = params.text_document_position;
        let uri = p.text_document.uri;
        let Some((text, offset, program)) = self.analyze(&uri, p.position) else {
            return Ok(None);
        };
        // A `userMod.` receiver that's a file import → offer its exports.
        if let Some(receiver) = member_trigger(&text, offset) {
            if let Some(fm) = self.foreign_module(&uri, &program, &receiver) {
                let items = fm
                    .exports
                    .iter()
                    .map(|m| CompletionItem {
                        label: m.name.clone(),
                        kind: Some(if m.is_function {
                            CompletionItemKind::FUNCTION
                        } else {
                            CompletionItemKind::VARIABLE
                        }),
                        detail: Some(format!("{receiver}.{}", m.signature)),
                        documentation: doc_markup(&doc_comment_above(&fm.text, m.def_span.start)),
                        ..Default::default()
                    })
                    .collect();
                return Ok(Some(CompletionResponse::Array(items)));
            }
        }
        let cat = self.catalog.read().unwrap();
        let items = completion_items(&text, offset, &program, &cat);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let p = params.text_document_position_params;
        let uri = p.text_document.uri;
        let Some((text, offset, program)) = self.analyze(&uri, p.position) else {
            return Ok(None);
        };
        let cat = self.catalog.read().unwrap();
        if let Some(sh) = signature_help(&text, offset, &program, &cat) {
            return Ok(Some(sh));
        }
        // Cross-file: a call on a member of a user file import.
        if let Some((open_paren, active)) = call_context(&text, offset) {
            if let Some(Callee::Member(receiver, member)) = callee_before(&text, open_paren) {
                if let Some(fm) = self.foreign_module(&uri, &program, &receiver) {
                    if let Some(m) = fm.exports.iter().find(|e| e.name == member) {
                        let doc = doc_comment_above(&fm.text, m.def_span.start);
                        return Ok(build_signature_help(
                            &format!("{receiver}.{}", m.signature),
                            active,
                            opt_doc(&doc),
                        ));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some((text, program)) = self.tree_and_text(&uri) else {
            return Ok(None);
        };
        let enc = self.encoding.lock().unwrap().clone();
        let symbols = analysis::document_symbols(&program)
            .into_iter()
            .map(|node| to_document_symbol(node, &text, &enc))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_lowercase();
        let enc = self.encoding.lock().unwrap().clone();
        let mut out = Vec::new();
        for uri in self.workspace_tg_files() {
            let Some((text, tree)) = self.load_tree(&uri) else {
                continue;
            };
            for node in analysis::document_symbols(&tree) {
                collect_workspace_symbols(&node, None, &uri, &text, &enc, &query, &mut out);
            }
        }
        Ok(Some(out))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let p = params.text_document_position;
        let uri = p.text_document.uri;
        let Some((text, offset, program)) = self.analyze(&uri, p.position) else {
            return Ok(None);
        };
        let mut spans = analysis::references(&program, offset);
        if !spans.is_empty() {
            // Local binding. `includeDeclaration: false` drops the
            // declaration occurrence; the resolver makes the declaration the
            // binding's identity, recovered here and filtered out.
            if !params.context.include_declaration {
                if let Some(target) = analysis::rename_spans(&program, offset) {
                    spans.retain(|s| s.start != target.def.start);
                }
            }
            let enc = self.encoding.lock().unwrap().clone();
            let locations = spans
                .into_iter()
                .map(|span| Location {
                    uri: uri.clone(),
                    range: span_to_range(&text, span, &enc),
                })
                .collect();
            return Ok(Some(locations));
        }
        // Cross-file: references to an exported member of a file import.
        if let Some(refs) = self.member_occurrences(&uri, &program, offset) {
            let decl = (!params.context.include_declaration)
                .then_some(refs.decl)
                .flatten();
            let locations = refs
                .occurrences
                .into_iter()
                .filter(|loc| decl.as_ref() != Some(loc))
                .collect();
            return Ok(Some(locations));
        }
        Ok(None)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let Some((text, offset, program)) = self.analyze(&params.text_document.uri, params.position)
        else {
            return Ok(None);
        };
        // A local binding: highlight the occurrence under the cursor.
        if let Some(target) = analysis::rename_spans(&program, offset) {
            let here = target
                .spans
                .iter()
                .copied()
                .find(|s| s.start <= offset && offset <= s.end)
                .unwrap_or(target.def);
            let enc = self.encoding.lock().unwrap().clone();
            return Ok(Some(PrepareRenameResponse::Range(span_to_range(
                &text, here, &enc,
            ))));
        }
        // An exported-member access of a file import is renamable too.
        if let Some((receiver, member)) = analysis::member_at(&program, offset) {
            if analysis::is_file_path(&analysis::canonical_module(&program, &receiver)) {
                if let Some(span) = analysis::member_access_spans(&program, &receiver, &member)
                    .into_iter()
                    .find(|s| s.start <= offset && offset <= s.end)
                {
                    let enc = self.encoding.lock().unwrap().clone();
                    return Ok(Some(PrepareRenameResponse::Range(span_to_range(
                        &text, span, &enc,
                    ))));
                }
            }
        }
        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let p = params.text_document_position;
        let uri = p.text_document.uri;
        let Some((text, offset, program)) = self.analyze(&uri, p.position) else {
            return Ok(None);
        };
        // Reject a new name that isn't a valid tigr identifier, so a typo
        // can't produce broken source.
        if !is_identifier(&params.new_name) {
            return Err(tower_lsp::jsonrpc::Error::invalid_params(
                "new name is not a valid identifier",
            ));
        }
        // Local binding rename — a single-file edit.
        if let Some(target) = analysis::rename_spans(&program, offset) {
            let enc = self.encoding.lock().unwrap().clone();
            let edits: Vec<TextEdit> = target
                .spans
                .into_iter()
                .map(|span| TextEdit {
                    range: span_to_range(&text, span, &enc),
                    new_text: params.new_name.clone(),
                })
                .collect();
            let mut changes = HashMap::new();
            changes.insert(uri, edits);
            return Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }));
        }
        // Cross-file exported-member rename — edits the export key and every
        // `alias.member` access across importing files.
        if let Some(refs) = self.member_occurrences(&uri, &program, offset) {
            let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
            for loc in refs.occurrences {
                changes.entry(loc.uri).or_default().push(TextEdit {
                    range: loc.range,
                    new_text: params.new_name.clone(),
                });
            }
            return Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }));
        }
        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Resolve an import path written in `importer` to the imported file's
/// URL, mirroring the VM: relative to the importing file's directory, with
/// `.tg` appended when the path carries no extension. `None` if `importer`
/// is not a `file:` URL (so it has no directory to resolve against).
fn resolve_import_url(importer: &Url, path: &str) -> Option<Url> {
    let importer_path = importer.to_file_path().ok()?;
    let base = importer_path.parent()?;
    let mut resolved = normalize_path(&base.join(path));
    if resolved.extension().is_none() {
        resolved.set_extension("tg");
    }
    Url::from_file_path(resolved).ok()
}

/// The cross-file occurrences of an exported member: every access site and
/// the export-object key. `decl` is the export key (a subset of
/// `occurrences`), so `references` can honour `includeDeclaration`.
struct MemberRefs {
    occurrences: Vec<Location>,
    decl: Option<Location>,
}

/// The span of the key `member` inside an object literal occupying
/// `obj_span` of `text`. A key is an identifier (or quoted string) followed
/// by `:`; matching the `:` distinguishes a key from a same-named value
/// (`${ resolve: resolve }` finds the first `resolve`). The object key has
/// no AST span, so this text scan stands in. `None` if not found.
fn find_object_key_span(
    text: &str,
    obj_span: tigr::vm::token::Span,
    member: &str,
) -> Option<tigr::vm::token::Span> {
    let end = obj_span.end.min(text.len());
    let region = text.get(obj_span.start..end)?;
    let bytes = region.as_bytes();
    let mlen = member.len();
    let mut i = 0;
    while i + mlen <= bytes.len() {
        // A word-boundary match of `member`.
        let matches = &region[i..i + mlen] == member
            && (i == 0 || !is_ident_byte(bytes[i - 1]))
            && (i + mlen == bytes.len() || !is_ident_byte(bytes[i + mlen]));
        if matches {
            // The next non-whitespace byte must be `:` (a key, not a value).
            let mut j = i + mlen;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b':' {
                let start = obj_span.start + i;
                return Some(tigr::vm::token::Span::new(start, start + mlen, obj_span.line));
            }
        }
        i += 1;
    }
    None
}

/// A user file import resolved to its module: where it lives, its source
/// text (for doc comments and ranges), and its exported members.
struct ForeignModule {
    url: Url,
    text: String,
    exports: Vec<analysis::ExportedMember>,
}

/// Markdown hover for an exported member: its signature qualified with the
/// receiver, the member's doc comment if any, and the module file's name.
fn render_foreign_member(
    receiver: &str,
    m: &analysis::ExportedMember,
    fm: &ForeignModule,
) -> String {
    let mut out = format!("```tigr\n{receiver}.{}\n```", m.signature);
    let doc = doc_comment_above(&fm.text, m.def_span.start);
    if !doc.is_empty() {
        out.push_str(&format!("\n\n{doc}"));
    }
    if let Some(file) = fm.url.path_segments().and_then(|mut s| s.next_back()) {
        out.push_str(&format!("\n\n*exported by `{file}`*"));
    }
    out
}

/// The contiguous block of `//` line comments immediately above the line
/// containing byte `offset`, joined with newlines and stripped of the
/// leading `//`. Empty if there is no such comment. A lightweight stand-in
/// for doc comments, which the lexer discards before the AST.
fn doc_comment_above(text: &str, offset: usize) -> String {
    let offset = offset.min(text.len());
    // Walk back to the start of the line `offset` sits on.
    let line_start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let mut lines: Vec<&str> = Vec::new();
    let mut cursor = line_start;
    // Collect preceding `//` lines, nearest last, then reverse.
    while cursor > 0 {
        let prev_end = cursor - 1; // the '\n' before this line
        let prev_start = text[..prev_end].rfind('\n').map_or(0, |n| n + 1);
        let line = text[prev_start..prev_end].trim();
        if let Some(comment) = line.strip_prefix("//") {
            lines.push(comment.trim());
            cursor = prev_start;
        } else {
            break;
        }
    }
    lines.reverse();
    lines.join("\n")
}

/// Recursively collect `.tg` files under `dir` as URLs. Skips hidden
/// directories and the usual build/dependency dumps so a large `target/`
/// doesn't dominate the workspace scan.
fn collect_tg_files(dir: &Path, out: &mut Vec<Url>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_tg_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "tg") {
            if let Ok(url) = Url::from_file_path(&path) {
                out.push(url);
            }
        }
    }
}

/// Collapse `.` and `..` components of a path lexically, without touching
/// the filesystem (the target may not exist yet, and we only need a stable
/// URL). Mirrors how `path.join("./dns")` would otherwise leave a `.`
/// component that breaks `Url::from_file_path`.
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A tigr span projected onto an LSP [`Range`] in the negotiated encoding.
fn span_to_range(text: &str, span: tigr::vm::token::Span, enc: &PositionEncodingKind) -> Range {
    Range {
        start: offset_to_position(text, span.start, enc),
        end: offset_to_position(text, span.end, enc),
    }
}

/// Whether `s` is a valid tigr identifier: a non-digit leader then
/// alphanumerics/underscores. Guards rename against invalid new names.
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Project an [`analysis::SymbolNode`] onto the LSP [`DocumentSymbol`],
/// converting its byte-offset spans to ranges in the negotiated encoding.
#[allow(deprecated)] // `DocumentSymbol::deprecated` is a required struct field
fn to_document_symbol(
    node: analysis::SymbolNode,
    text: &str,
    enc: &PositionEncodingKind,
) -> DocumentSymbol {
    let range = |span: tigr::vm::token::Span| Range {
        start: offset_to_position(text, span.start, enc),
        end: offset_to_position(text, span.end, enc),
    };
    let kind = match node.category {
        analysis::SymbolCategory::Function => SymbolKind::FUNCTION,
        analysis::SymbolCategory::Variable => SymbolKind::VARIABLE,
        analysis::SymbolCategory::Module => SymbolKind::MODULE,
    };
    let children: Vec<DocumentSymbol> = node
        .children
        .into_iter()
        .map(|child| to_document_symbol(child, text, enc))
        .collect();
    DocumentSymbol {
        name: node.name,
        detail: node.detail,
        kind,
        tags: None,
        deprecated: None,
        range: range(node.range),
        selection_range: range(node.selection),
        children: (!children.is_empty()).then_some(children),
    }
}

/// Flatten a [`analysis::SymbolNode`] tree into `SymbolInformation`s whose
/// name matches `query` (case-insensitive substring; an empty query matches
/// all). Each child carries its parent's name as `container_name`.
#[allow(deprecated)] // `SymbolInformation::deprecated` is a required field
fn collect_workspace_symbols(
    node: &analysis::SymbolNode,
    container: Option<&str>,
    uri: &Url,
    text: &str,
    enc: &PositionEncodingKind,
    query: &str,
    out: &mut Vec<SymbolInformation>,
) {
    if query.is_empty() || node.name.to_lowercase().contains(query) {
        let kind = match node.category {
            analysis::SymbolCategory::Function => SymbolKind::FUNCTION,
            analysis::SymbolCategory::Variable => SymbolKind::VARIABLE,
            analysis::SymbolCategory::Module => SymbolKind::MODULE,
        };
        out.push(SymbolInformation {
            name: node.name.clone(),
            kind,
            tags: None,
            deprecated: None,
            location: Location {
                uri: uri.clone(),
                range: span_to_range(text, node.selection, enc),
            },
            container_name: container.map(str::to_string),
        });
    }
    for child in &node.children {
        collect_workspace_symbols(child, Some(&node.name), uri, text, enc, query, out);
    }
}

/// A call's callee as recovered from the text before its `(`.
enum Callee {
    /// A bare name: a builtin or a local function.
    Bare(String),
    /// `receiver.member` — a stdlib member access.
    Member(String, String),
}

/// Signature help for the call enclosing `offset`, if the cursor sits in
/// a call's argument list and the callee resolves to a known signature.
fn signature_help(
    text: &str,
    offset: usize,
    program: &tigr::vm::ast::Block,
    catalog: &Catalog,
) -> Option<SignatureHelp> {
    let (open_paren, active) = call_context(text, offset)?;
    let callee = callee_before(text, open_paren)?;
    let (signature, doc) = resolve_callee(program, catalog, &callee, offset)?;
    build_signature_help(&signature, active, doc)
}

/// Build the `SignatureHelp` popup for `signature` with `active` as the
/// highlighted parameter (clamped onto the last parameter so an extra arg
/// to a variadic still shows). `None` when the signature has no parameters
/// to highlight (a constant, or `f()`). Shared by the catalog/local path
/// and the cross-file member path.
fn build_signature_help(
    signature: &str,
    active: usize,
    doc: Option<String>,
) -> Option<SignatureHelp> {
    let params = parse_params(signature);
    if params.is_empty() {
        return None;
    }
    let active = (active.min(params.len() - 1)) as u32;
    let parameters = params
        .into_iter()
        .map(|label| ParameterInformation {
            label: ParameterLabel::Simple(label),
            documentation: None,
        })
        .collect();
    let documentation = doc.map(|d| {
        Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: d,
        })
    });
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: signature.to_string(),
            documentation,
            parameters: Some(parameters),
            active_parameter: Some(active),
        }],
        active_signature: Some(0),
        active_parameter: Some(active),
    })
}

/// Scan backward from `offset` for the `(` of the call the cursor is
/// inside, counting top-level commas to get the active parameter index.
/// Returns `(open_paren_byte_index, active_param)`. `None` if the cursor
/// is not directly inside parentheses (e.g. inside an array/object
/// literal, or at the top level).
fn call_context(text: &str, offset: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let mut i = offset.min(b.len());
    let mut depth: i32 = 0; // brackets closed (and not yet reopened) while scanning back
    let mut commas = 0usize;
    while i > 0 {
        i -= 1;
        match b[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' => {
                if depth == 0 {
                    return Some((i, commas));
                }
                depth -= 1;
            }
            b'[' | b'{' => {
                if depth == 0 {
                    return None; // inside an array/object literal, not a call
                }
                depth -= 1;
            }
            b',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    None
}

/// The callee immediately before the `(` at `open_paren`: a bare
/// identifier or a `receiver.member` access. `None` if no identifier
/// precedes the paren (so it's a grouping paren, not a call).
fn callee_before(text: &str, open_paren: usize) -> Option<Callee> {
    let b = text.as_bytes();
    let mut i = open_paren;
    while i > 0 && b[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    let end = i;
    while i > 0 && is_ident_byte(b[i - 1]) {
        i -= 1;
    }
    if i == end {
        return None; // grouping `(...)`, not a call
    }
    let name = text[i..end].to_string();

    // A leading `receiver.` makes it a member call.
    let mut j = i;
    while j > 0 && b[j - 1].is_ascii_whitespace() {
        j -= 1;
    }
    if j > 0 && b[j - 1] == b'.' {
        j -= 1;
        while j > 0 && b[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        let recv_end = j;
        while j > 0 && is_ident_byte(b[j - 1]) {
            j -= 1;
        }
        if j < recv_end {
            return Some(Callee::Member(text[j..recv_end].to_string(), name));
        }
    }
    Some(Callee::Bare(name))
}

/// Resolve a callee to `(signature, doc)`. Member access goes through the
/// catalog (alias-canonicalized); a bare name is a local function first
/// (its decl signature, no doc), then a builtin.
fn resolve_callee(
    program: &tigr::vm::ast::Block,
    catalog: &Catalog,
    callee: &Callee,
    offset: usize,
) -> Option<(String, Option<String>)> {
    match callee {
        Callee::Member(recv, member) => {
            let module = analysis::canonical_module(program, recv);
            let m = catalog.member(&module, member)?;
            Some((format!("{module}.{}", m.signature), opt_doc(&m.doc)))
        }
        Callee::Bare(name) => {
            // A local `name := fn(...)` wins over a builtin of the same name.
            let local_sig = analysis::locals_in_scope(program, offset)
                .into_iter()
                .find(|l| &l.name == name)
                .and_then(|l| l.sig);
            if let Some(sig) = local_sig {
                return Some((sig, None));
            }
            let b = catalog.builtin(name)?;
            Some((b.signature.clone(), opt_doc(&b.doc)))
        }
    }
}

fn opt_doc(doc: &str) -> Option<String> {
    (!doc.is_empty()).then(|| doc.to_string())
}

/// The parameter labels in a signature: the text between its first `(`
/// and the matching `)`, split on top-level commas. Empty parameters
/// (e.g. from `f()`) are dropped.
fn parse_params(signature: &str) -> Vec<String> {
    let Some(open) = signature.find('(') else {
        return Vec::new();
    };
    let mut params = Vec::new();
    let mut depth: i32 = 0;
    let mut start = open + 1;
    let bytes = signature.as_bytes();
    let mut idx = open + 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    push_param(&mut params, &signature[start..idx]);
                    break;
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                push_param(&mut params, &signature[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
        idx += 1;
    }
    params
}

fn push_param(params: &mut Vec<String>, raw: &str) {
    let p = raw.trim();
    if !p.is_empty() {
        params.push(p.to_string());
    }
}

/// Build the completion list for `offset`. After `module.` it offers that
/// module's members; otherwise it offers in-scope locals, builtins,
/// module names, and keywords. Prefix filtering is left to the client.
fn completion_items(
    text: &str,
    offset: usize,
    program: &tigr::vm::ast::Block,
    catalog: &Catalog,
) -> Vec<CompletionItem> {
    // `module.` member access — offer only that module's members.
    if let Some(module) = member_trigger(text, offset) {
        let canonical = analysis::canonical_module(program, &module);
        let Some(m) = catalog.module(&canonical) else {
            return Vec::new(); // unknown receiver: nothing to suggest
        };
        return m
            .members
            .iter()
            .map(|(name, member)| CompletionItem {
                label: name.clone(),
                kind: Some(if member.is_constant() {
                    CompletionItemKind::CONSTANT
                } else {
                    CompletionItemKind::FUNCTION
                }),
                detail: Some(format!("{canonical}.{}", member.signature)),
                documentation: doc_markup(&member.doc),
                ..Default::default()
            })
            .collect();
    }

    // Otherwise: locals, builtins, modules, keywords.
    let mut items = Vec::new();

    for local in analysis::locals_in_scope(program, offset) {
        let detail = local
            .sig
            .clone()
            .unwrap_or_else(|| local.kind.describe().to_string());
        items.push(CompletionItem {
            label: local.name,
            kind: Some(if local.sig.is_some() {
                CompletionItemKind::FUNCTION
            } else {
                CompletionItemKind::VARIABLE
            }),
            detail: Some(detail),
            ..Default::default()
        });
    }

    for (name, member) in catalog.builtins() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(member.signature.clone()),
            documentation: doc_markup(&member.doc),
            ..Default::default()
        });
    }

    for (name, module) in catalog.modules() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("module".to_string()),
            documentation: doc_markup(&module.description),
            ..Default::default()
        });
    }

    for (name, explanation) in catalog.keywords() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("keyword".to_string()),
            documentation: doc_markup(explanation),
            ..Default::default()
        });
    }

    items
}

/// Wrap a docstring as Markdown completion documentation, or `None` if
/// empty.
fn doc_markup(doc: &str) -> Option<Documentation> {
    if doc.is_empty() {
        return None;
    }
    Some(Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: doc.to_string(),
    }))
}

/// If the text just before `offset` is `<ident> . <partial>?`, return the
/// receiver identifier — the cursor is completing a member access. Walks
/// backward over an optional partial member name, the dot, and the
/// receiver, all of which are ASCII, so byte indexing is safe.
fn member_trigger(text: &str, offset: usize) -> Option<String> {
    let b = text.as_bytes();
    let mut i = offset.min(b.len());
    // The partial member name being typed (may be empty right after `.`).
    while i > 0 && is_ident_byte(b[i - 1]) {
        i -= 1;
    }
    while i > 0 && b[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || b[i - 1] != b'.' {
        return None;
    }
    i -= 1; // the dot
    while i > 0 && b[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    let end = i;
    while i > 0 && is_ident_byte(b[i - 1]) {
        i -= 1;
    }
    if i == end {
        return None; // no receiver identifier
    }
    Some(text[i..end].to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Hover text for a keyword token under `offset`, if any. Keywords are
/// syntax, not AST identifiers, so this scans the token stream rather
/// than the tree.
fn keyword_hover(text: &str, offset: usize, catalog: &Catalog) -> Option<String> {
    let tokens = Lexer::new(text).tokenize().ok()?;
    let st = tokens
        .iter()
        .find(|st| st.span.start <= offset && offset < st.span.end.max(st.span.start + 1))?;
    let kw = keyword_str(&st.token)?;
    let doc = catalog.keyword(kw)?;
    Some(format!("**keyword `{kw}`**\n\n{doc}"))
}

/// The source spelling of a keyword token, or `None` for any non-keyword
/// token. Kept explicit (rather than `Token::to_string`) so a string or
/// identifier that happens to read like a keyword can't trigger it.
fn keyword_str(t: &Token) -> Option<&'static str> {
    use Token::*;
    Some(match t {
        Fn => "fn",
        If => "if",
        Else => "else",
        For => "for",
        While => "while",
        Break => "break",
        Continue => "continue",
        Return => "return",
        Import => "import",
        Try => "try",
        Catch => "catch",
        Raise => "raise",
        Match => "match",
        Spawn => "spawn",
        Go => "go",
        Yield => "yield",
        Gen => "gen",
        Null => "null",
        True => "true",
        False => "false",
        _ => return None,
    })
}

/// Run the frontend over `text` and return diagnostics. Empty on success.
/// `host_ambient` names (from an embedder's manifest) are treated as
/// resolvable so bare host-module references aren't flagged.
fn compute_diagnostics(
    text: &str,
    uri: &Url,
    enc: &PositionEncodingKind,
    host_ambient: &[String],
) -> Vec<Diagnostic> {
    // base_dir mirrors what the CLI passes so relative-import resolution
    // (a runtime concern) is set up consistently; compilation never reads
    // it, but keeping it correct costs nothing.
    let base_dir = uri
        .to_file_path()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));

    // SourceId only matters for the core's own snippet rendering, which we
    // bypass; UNKNOWN is fine since we render from `text` directly.
    // check_source recovers past syntax errors, so this is every error in
    // the file, not just the first.
    check_source_with_ambient(text, base_dir, SourceId::UNKNOWN, host_ambient)
        .iter()
        .filter_map(|err| {
            let (start, end, message) = error_span(err)?;
            Some(Diagnostic {
                range: Range {
                    start: offset_to_position(text, start, enc),
                    end: offset_to_position(text, end, enc),
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("tigr".into()),
                message,
                ..Default::default()
            })
        })
        .collect()
}

/// Read the first `tigr.modules.json` found in a workspace root. Missing
/// or malformed files are ignored (the manifest is optional), so a bad
/// file never breaks plain stdlib editing.
fn load_manifest_file(roots: &[PathBuf]) -> Option<serde_json::Value> {
    for root in roots {
        let path = root.join("tigr.modules.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                return Some(v);
            }
        }
    }
    None
}

/// Merge a host-module manifest into the running maps. Shape:
/// `{ "modules": { "Name": { "description": str,
///    "members": { "fn": { "signature": str, "doc": str } } } } }`.
/// Every field but the module name is optional. A later call overrides an
/// earlier one for the same module (so `initializationOptions` wins over
/// the file). Unknown / malformed entries are skipped, not fatal.
fn merge_manifest(
    value: &serde_json::Value,
    modules: &mut HashMap<String, catalog::Module>,
    names: &mut Vec<String>,
) {
    let Some(mods) = value.get("modules").and_then(|m| m.as_object()) else {
        return;
    };
    for (name, def) in mods {
        let description = def
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        let mut members = HashMap::new();
        if let Some(ms) = def.get("members").and_then(|m| m.as_object()) {
            for (mname, mdef) in ms {
                let signature = mdef
                    .get("signature")
                    .and_then(|s| s.as_str())
                    .unwrap_or(mname)
                    .to_string();
                let doc = mdef
                    .get("doc")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                members.insert(
                    mname.clone(),
                    catalog::Member { signature, doc },
                );
            }
        }
        modules.insert(name.clone(), catalog::Module { description, members });
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
}

/// Extract `(start_byte, end_byte, message)` from a frontend error.
/// Returns `None` for runtime errors, which carry only a line and never
/// arise from compilation.
fn error_span(err: &TigrError) -> Option<(usize, usize, String)> {
    let (span, message) = match err {
        TigrError::Lex(e) => (e.span, e.to_string()),
        TigrError::Parse(e) => (e.span, e.to_string()),
        TigrError::Compile(e) => (e.span, e.to_string()),
        TigrError::Runtime(_) => return None,
    };
    // Guarantee a non-empty range so the squiggle is visible even for
    // zero-width spans (e.g. an error at EOF).
    Some((span.start, span.end.max(span.start + 1), message))
}

/// Project a byte offset into `text` onto an LSP [`Position`]. The column
/// is counted in the negotiated unit: raw bytes for UTF-8, UTF-16 code
/// units otherwise.
fn offset_to_position(
    text: &str,
    offset: usize,
    enc: &PositionEncodingKind,
) -> Position {
    let offset = offset.min(text.len());
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, b) in text.as_bytes().iter().enumerate() {
        if i >= offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let prefix = &text[line_start..offset];
    let character = if *enc == PositionEncodingKind::UTF8 {
        prefix.len() as u32
    } else {
        prefix.chars().map(|c| c.len_utf16() as u32).sum()
    };
    Position { line, character }
}

/// Inverse of [`offset_to_position`]: an LSP position back to a byte
/// offset, counting `character` in the negotiated unit. Clamps to the
/// line's end so an out-of-range column lands on the newline rather than
/// spilling into the next line.
fn position_to_offset(text: &str, pos: Position, enc: &PositionEncodingKind) -> usize {
    // Byte offset of the start of `pos.line`.
    let mut line_start = 0usize;
    let mut line = 0u32;
    for (i, b) in text.as_bytes().iter().enumerate() {
        if line == pos.line {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    if line < pos.line {
        return text.len(); // line beyond EOF
    }
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |n| line_start + n);
    let line_text = &text[line_start..line_end];
    let want = pos.character as usize;
    if *enc == PositionEncodingKind::UTF8 {
        line_start + want.min(line_text.len())
    } else {
        // Advance UTF-16 code units across the line.
        let mut units = 0usize;
        for (byte_idx, ch) in line_text.char_indices() {
            if units >= want {
                return line_start + byte_idx;
            }
            units += ch.len_utf16();
        }
        line_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte offset of the cursor marked `|` in `src`, with the marker
    /// removed from the returned text.
    fn cursor(src: &str) -> (String, usize) {
        let off = src.find('|').expect("cursor marker");
        (src.replace('|', ""), off)
    }

    #[test]
    fn member_trigger_detects_receiver_after_dot() {
        let (text, off) = cursor("Math.|");
        assert_eq!(member_trigger(&text, off).as_deref(), Some("Math"));
    }

    #[test]
    fn member_trigger_detects_receiver_with_partial() {
        let (text, off) = cursor("Math.sq|");
        assert_eq!(member_trigger(&text, off).as_deref(), Some("Math"));
    }

    #[test]
    fn member_trigger_is_none_for_a_bare_word() {
        let (text, off) = cursor("Mat|");
        assert_eq!(member_trigger(&text, off), None);
    }

    #[test]
    fn member_completion_lists_module_members() {
        let cat = Catalog::load();
        let (text, off) = cursor("M := import 'Math';\nM.|");
        let prog = tigr::vm::parse_tree(&text);
        let items = completion_items(&text, off, &prog, &cat);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"sqrt"), "got {labels:?}");
        // Aliased receiver canonicalizes, and detail is qualified.
        let sqrt = items.iter().find(|i| i.label == "sqrt").unwrap();
        assert_eq!(sqrt.detail.as_deref(), Some("Math.sqrt(x) -> Float"));
        // Member context offers only members, not builtins/keywords.
        assert!(!labels.contains(&"print"), "leaked builtin into member list");
    }

    #[test]
    fn identifier_completion_offers_locals_builtins_modules_keywords() {
        let cat = Catalog::load();
        let (text, off) = cursor("x := 1;\n|");
        let prog = tigr::vm::parse_tree(&text);
        let labels: Vec<String> = completion_items(&text, off, &prog, &cat)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert!(labels.contains(&"x".to_string()), "local missing");
        assert!(labels.contains(&"print".to_string()), "builtin missing");
        assert!(labels.contains(&"Math".to_string()), "module missing");
        assert!(labels.contains(&"match".to_string()), "keyword missing");
    }

    #[test]
    fn call_context_counts_active_parameter() {
        let (t1, o1) = cursor("f(a, b|)");
        assert_eq!(call_context(&t1, o1).map(|(_, a)| a), Some(1));
        let (t0, o0) = cursor("f(a|, b)");
        assert_eq!(call_context(&t0, o0).map(|(_, a)| a), Some(0));
        // Inside a nested call, count resets to the inner arg list.
        let (tn, on) = cursor("f(g(x, y|), z)");
        assert_eq!(call_context(&tn, on).map(|(_, a)| a), Some(1));
    }

    #[test]
    fn call_context_ignores_array_and_object_literals() {
        let (t, o) = cursor("[1, 2|]");
        assert_eq!(call_context(&t, o), None);
    }

    #[test]
    fn parse_params_splits_top_level_commas() {
        assert_eq!(parse_params("clamp(x, lo, hi) -> value"), ["x", "lo", "hi"]);
        assert_eq!(parse_params("rand() -> Float"), Vec::<String>::new());
        // A nested signature comma is not a top-level split.
        assert_eq!(parse_params("f(a, g(b, c)) -> x"), ["a", "g(b, c)"]);
    }

    #[test]
    fn signature_help_for_a_stdlib_member() {
        let cat = Catalog::load();
        let (text, off) = cursor("Math := import 'Math';\nMath.pow(2.0, |)");
        let prog = tigr::vm::parse_tree(&text);
        let sh = signature_help(&text, off, &prog, &cat).expect("signature help");
        let sig = &sh.signatures[0];
        assert_eq!(sig.label, "Math.pow(x, y) -> Float");
        assert_eq!(sh.active_parameter, Some(1));
        assert_eq!(sig.parameters.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn signature_help_for_an_aliased_member() {
        let cat = Catalog::load();
        let (text, off) = cursor("M := import 'Math';\nM.sqrt(|)");
        let prog = tigr::vm::parse_tree(&text);
        let sh = signature_help(&text, off, &prog, &cat).expect("signature help");
        assert_eq!(sh.signatures[0].label, "Math.sqrt(x) -> Float");
        assert_eq!(sh.active_parameter, Some(0));
    }

    #[test]
    fn signature_help_for_a_local_function() {
        let cat = Catalog::load();
        let (text, off) = cursor("add := fn(a, b) { a + b };\nadd(1, |)");
        let prog = tigr::vm::parse_tree(&text);
        let sh = signature_help(&text, off, &prog, &cat).expect("signature help");
        assert_eq!(sh.signatures[0].label, "add(a, b)");
        assert_eq!(sh.active_parameter, Some(1));
    }

    #[test]
    fn signature_help_for_a_builtin() {
        let cat = Catalog::load();
        let (text, off) = cursor("str(42, |)");
        let prog = tigr::vm::parse_tree(&text);
        let sh = signature_help(&text, off, &prog, &cat).expect("signature help");
        assert!(sh.signatures[0].label.starts_with("str("), "got {}", sh.signatures[0].label);
        assert_eq!(sh.active_parameter, Some(1));
    }

    #[test]
    fn find_object_key_span_locates_the_key_not_a_value() {
        // The key `resolve` precedes the colon; the value `_resolve` and the
        // value-position `resolve` (in `x: resolve`) must not match.
        let text = "${ x: resolve, resolve: _resolve }";
        let span = tigr::vm::token::Span::new(0, text.len(), 1);
        let key = find_object_key_span(text, span, "resolve").expect("key span");
        assert_eq!(&text[key.start..key.end], "resolve");
        // It is the key occurrence (followed by `:`), not the value one.
        assert_eq!(text[key.end..].trim_start().as_bytes()[0], b':');
    }

    #[test]
    fn doc_comment_above_collects_contiguous_line_comments() {
        let text = "x := 1;\n// first line\n// second line\nresolve := fn() {};";
        let off = text.find("resolve").unwrap();
        assert_eq!(doc_comment_above(text, off), "first line\nsecond line");
        // No comment above → empty.
        assert_eq!(doc_comment_above(text, text.find("x :=").unwrap()), "");
    }

    #[test]
    fn resolve_import_url_appends_tg_and_normalizes() {
        let importer = Url::from_file_path("/proj/dns/main.tg").unwrap();
        // `./dns` resolves beside the importer, with `.tg` appended and the
        // `.` component collapsed.
        let target = resolve_import_url(&importer, "./dns").unwrap();
        assert_eq!(target.to_file_path().unwrap(), PathBuf::from("/proj/dns/dns.tg"));
        // A `..` climbs out of the importing directory.
        let up = resolve_import_url(&importer, "../lib/util").unwrap();
        assert_eq!(up.to_file_path().unwrap(), PathBuf::from("/proj/lib/util.tg"));
        // An explicit extension is kept.
        let kept = resolve_import_url(&importer, "./dns.tg").unwrap();
        assert_eq!(kept.to_file_path().unwrap(), PathBuf::from("/proj/dns/dns.tg"));
    }

    #[test]
    fn no_signature_help_outside_a_call() {
        let cat = Catalog::load();
        let (text, off) = cursor("x := 1|;");
        let prog = tigr::vm::parse_tree(&text);
        assert!(signature_help(&text, off, &prog, &cat).is_none());
    }

    // -- host module manifest (embedding: purr) ----------------------

    fn uri() -> Url {
        Url::parse("file:///game.tg").unwrap()
    }

    /// Without a manifest, a bare reference to a host module is flagged
    /// as an undeclared variable — the baseline the manifest fixes.
    #[test]
    fn host_module_unknown_without_manifest() {
        let enc = PositionEncodingKind::UTF8;
        let diags = compute_diagnostics("x := Game.rect(1, 2, 3, 4);", &uri(), &enc, &[]);
        assert!(
            diags.iter().any(|d| d.message.contains("Game")),
            "expected an undeclared-variable diagnostic for Game: {diags:?}"
        );
    }

    /// With `Game` in the host-ambient set, the same code is clean — no
    /// false "undeclared variable" diagnostic.
    #[test]
    fn host_ambient_suppresses_undeclared() {
        let enc = PositionEncodingKind::UTF8;
        let host = vec!["Game".to_string()];
        let diags =
            compute_diagnostics("x := Game.rect(1, 2, 3, 4);", &uri(), &enc, &host);
        assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
        // A genuinely undeclared name is still reported.
        let diags2 = compute_diagnostics("y := Nope.x();", &uri(), &enc, &host);
        assert!(diags2.iter().any(|d| d.message.contains("Nope")));
    }

    /// The manifest parses into module names plus a catalog the hover /
    /// completion path can read — member signature and doc included.
    #[test]
    fn manifest_parses_into_catalog_and_names() {
        let json = serde_json::json!({
            "modules": {
                "Game": {
                    "description": "purr engine",
                    "members": {
                        "rect": { "signature": "rect(x, y, w, h) -> Null", "doc": "Draw a rect." }
                    }
                }
            }
        });
        let mut modules = HashMap::new();
        let mut names = Vec::new();
        merge_manifest(&json, &mut modules, &mut names);
        assert_eq!(names, vec!["Game".to_string()]);

        let cat = Catalog::with_host_modules(modules);
        let m = cat.member("Game", "rect").expect("Game.rect in catalog");
        assert_eq!(m.signature, "rect(x, y, w, h) -> Null");
        assert!(m.doc.contains("Draw a rect"));
        // Completion after `Game.` offers the host member.
        let (text, off) = cursor("Game.|");
        let prog = tigr::vm::parse_tree(&text);
        let items = completion_items(&text, off, &prog, &cat);
        assert!(
            items.iter().any(|i| i.label == "rect"),
            "expected rect in completion: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    /// A host module never overrides a stdlib module of the same name —
    /// core wins, matching the runtime import order.
    #[test]
    fn manifest_does_not_override_stdlib() {
        let json = serde_json::json!({
            "modules": { "Math": { "members": { "bogus": { "signature": "bogus()" } } } }
        });
        let mut modules = HashMap::new();
        let mut names = Vec::new();
        merge_manifest(&json, &mut modules, &mut names);
        let cat = Catalog::with_host_modules(modules);
        // The real Math (with sqrt) survived; the bogus member is absent.
        assert!(cat.member("Math", "sqrt").is_some());
        assert!(cat.member("Math", "bogus").is_none());
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: Mutex::new(HashMap::new()),
        foreign: Mutex::new(HashMap::new()),
        roots: Mutex::new(Vec::new()),
        encoding: Mutex::new(PositionEncodingKind::UTF16),
        catalog: RwLock::new(Catalog::load()),
        host_ambient: Mutex::new(Vec::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
