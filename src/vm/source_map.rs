//! Source-text registry used for rendering errors with a snippet,
//! caret, and filename.
//!
//! Every file (or REPL line, or embedded stdlib source) the runtime
//! touches is registered exactly once and assigned a [`SourceId`].
//! The id then flows through compiled chunks and runtime errors so
//! that at print time we can fetch back the text and the original
//! filename.
//!
//! `SourceId(0)` is reserved as "synthetic / unknown" — used as the
//! default for errors built from contexts that don't have a source
//! (e.g. internal compiler errors built by tests directly).

use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(pub u16);

impl SourceId {
    pub const UNKNOWN: SourceId = SourceId(0);

    pub fn is_unknown(self) -> bool {
        self.0 == 0
    }
}

impl Default for SourceId {
    fn default() -> Self {
        SourceId::UNKNOWN
    }
}

/// One registered source. `name` is the user-facing filename (or
/// `<repl>`, `<stdlib:Array>`, etc.); `text` owns the source string
/// for the lifetime of the [`SourceMap`].
#[derive(Clone)]
pub struct SourceFile {
    pub name: String,
    pub text: Rc<str>,
}

#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        SourceMap { files: Vec::new() }
    }

    /// Register a new source. Returns its `SourceId`. Sources are
    /// never deduplicated: two registrations of the same path get
    /// distinct ids (matters for the REPL, where each line is its
    /// own buffer).
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<Rc<str>>) -> SourceId {
        let id = self.files.len() + 1; // +1 reserves 0 for UNKNOWN
        self.files.push(SourceFile {
            name: name.into(),
            text: text.into(),
        });
        SourceId(id as u16)
    }

    /// Convenience for files: registers under the path's display form.
    pub fn add_path(&mut self, path: &PathBuf, text: impl Into<Rc<str>>) -> SourceId {
        self.add(path.display().to_string(), text)
    }

    pub fn get(&self, id: SourceId) -> Option<&SourceFile> {
        if id.is_unknown() {
            return None;
        }
        self.files.get((id.0 - 1) as usize)
    }
}
