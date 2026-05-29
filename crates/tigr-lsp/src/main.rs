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
mod catalog;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use tigr::vm::check_source;
use tigr::vm::error::Error as TigrError;
use tigr::vm::lexer::Lexer;
use tigr::vm::source_map::SourceId;
use tigr::vm::token::Token;

use crate::catalog::Catalog;

struct Backend {
    client: Client,
    /// Open documents, keyed by URI. Full-sync, so each entry is the
    /// complete current text.
    docs: Mutex<HashMap<Url, String>>,
    /// Position encoding negotiated in `initialize`. tigr spans are byte
    /// offsets; this decides whether a column counts bytes (UTF-8) or
    /// UTF-16 code units when we project an offset onto an LSP position.
    encoding: Mutex<PositionEncodingKind>,
    /// Builtins, stdlib members, and keywords with signatures and docs,
    /// parsed once from the embedded reference docs. Powers hover.
    catalog: Catalog,
}

impl Backend {
    /// Fetch a document's text and convert an LSP position into a byte
    /// offset into it, using the negotiated encoding. `None` if the
    /// document isn't open.
    fn locate(&self, uri: &Url, pos: Position) -> Option<(String, usize)> {
        let text = self.docs.lock().unwrap().get(uri).cloned()?;
        let enc = self.encoding.lock().unwrap().clone();
        let offset = position_to_offset(&text, pos, &enc);
        Some((text, offset))
    }

    /// Recompile `text` and publish the resulting diagnostics for `uri`.
    async fn publish(&self, uri: Url, text: &str, version: Option<i32>) {
        let enc = self.encoding.lock().unwrap().clone();
        let diagnostics = compute_diagnostics(text, &uri, &enc);
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
        self.docs
            .lock()
            .unwrap()
            .insert(doc.uri.clone(), doc.text.clone());
        self.publish(doc.uri, &doc.text, Some(doc.version)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync → the last change carries the entire new text.
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        self.docs
            .lock()
            .unwrap()
            .insert(uri.clone(), change.text.clone());
        self.publish(uri, &change.text, Some(version)).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        // Prefer the text the client sent on save; fall back to our cache.
        let text = params
            .text
            .or_else(|| self.docs.lock().unwrap().get(&uri).cloned());
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
        let Some((text, offset)) = self.locate(&uri, p.position) else {
            return Ok(None);
        };
        let program = tigr::vm::parse_tree(&text);
        let Some(span) = analysis::definition(&program, offset) else {
            return Ok(None);
        };
        let enc = self.encoding.lock().unwrap().clone();
        let range = Range {
            start: offset_to_position(&text, span.start, &enc),
            end: offset_to_position(&text, span.end, &enc),
        };
        Ok(Some(GotoDefinitionResponse::Scalar(Location { uri, range })))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let p = params.text_document_position_params;
        let Some((text, offset)) = self.locate(&p.text_document.uri, p.position) else {
            return Ok(None);
        };
        let program = tigr::vm::parse_tree(&text);
        let markdown = analysis::hover(&program, offset, &self.catalog)
            .or_else(|| keyword_hover(&text, offset, &self.catalog));
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
        let Some((text, offset)) = self.locate(&p.text_document.uri, p.position) else {
            return Ok(None);
        };
        let program = tigr::vm::parse_tree(&text);
        let items = completion_items(&text, offset, &program, &self.catalog);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let p = params.text_document_position_params;
        let Some((text, offset)) = self.locate(&p.text_document.uri, p.position) else {
            return Ok(None);
        };
        let program = tigr::vm::parse_tree(&text);
        Ok(signature_help(&text, offset, &program, &self.catalog))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
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

    let params = parse_params(&signature);
    if params.is_empty() {
        return None; // nothing to highlight (constant, or `f()`)
    }
    // Clamp past-the-end (e.g. extra args to a variadic) onto the last
    // parameter rather than dropping the popup.
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
            label: signature,
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
fn compute_diagnostics(
    text: &str,
    uri: &Url,
    enc: &PositionEncodingKind,
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
    check_source(text, base_dir, SourceId::UNKNOWN)
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
    fn no_signature_help_outside_a_call() {
        let cat = Catalog::load();
        let (text, off) = cursor("x := 1|;");
        let prog = tigr::vm::parse_tree(&text);
        assert!(signature_help(&text, off, &prog, &cat).is_none());
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: Mutex::new(HashMap::new()),
        encoding: Mutex::new(PositionEncodingKind::UTF16),
        catalog: Catalog::load(),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
