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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use tigr::vm::compile_source_with_id;
use tigr::vm::error::Error as TigrError;
use tigr::vm::source_map::SourceId;

struct Backend {
    client: Client,
    /// Open documents, keyed by URI. Full-sync, so each entry is the
    /// complete current text.
    docs: Mutex<HashMap<Url, String>>,
    /// Position encoding negotiated in `initialize`. tigr spans are byte
    /// offsets; this decides whether a column counts bytes (UTF-8) or
    /// UTF-16 code units when we project an offset onto an LSP position.
    encoding: Mutex<PositionEncodingKind>,
}

impl Backend {
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

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
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
    match compile_source_with_id(text, base_dir, SourceId::UNKNOWN) {
        Ok(_) => Vec::new(),
        Err(err) => match error_span(&err) {
            Some((start, end, message)) => {
                let range = Range {
                    start: offset_to_position(text, start, enc),
                    end: offset_to_position(text, end, enc),
                };
                vec![Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("tigr".into()),
                    message,
                    ..Default::default()
                }]
            }
            // A runtime error can't surface from compilation alone, so this
            // arm is unreachable in practice; drop it rather than guess a
            // span.
            None => Vec::new(),
        },
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

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: Mutex::new(HashMap::new()),
        encoding: Mutex::new(PositionEncodingKind::UTF16),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
