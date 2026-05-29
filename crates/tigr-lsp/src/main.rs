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

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
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
