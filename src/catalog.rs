//! A catalog of the language's named entities — builtins, stdlib module
//! members, and keywords — each with a signature and a docstring. One
//! data source powers richer hover (and, later, completion and signature
//! help).
//!
//! The source is the committed Markdown reference under `docs/stdlib/`.
//! Every module page and the builtins page share one shape: a `# `Name``
//! header, an intro paragraph, then one `### `signature`` detail section
//! per member (functions *and* constants). Each section's heading gives
//! the signature; the prose up to its first code fence is the docstring.
//! Those pages are embedded at compile time, so the catalog needs no file
//! access at runtime and can never drift from a moved binary — and it
//! stays in sync with the docs the same edit already has to touch.
//!
//! Keywords aren't in those pages, so they're a small hand-written table.

use std::collections::HashMap;

/// One named entity: a builtin, a module member, or a module constant.
#[derive(Clone, Debug)]
pub struct Member {
    /// The signature as written in the docs, e.g. `sqrt(x) -> Float` or,
    /// for a constant, just its name (`PI`).
    pub signature: String,
    /// Markdown docstring — the section prose, parameter bullets, and
    /// `**Returns:**` line, but not the runnable example that follows.
    pub doc: String,
}

/// One stdlib module: its one-line-ish description plus its members,
/// keyed by member name (the part of the signature before `(`).
#[derive(Clone, Debug, Default)]
pub struct Module {
    pub description: String,
    pub members: HashMap<String, Member>,
}

/// The whole catalog: modules, top-level builtins, and keywords.
pub struct Catalog {
    modules: HashMap<String, Module>,
    builtins: HashMap<String, Member>,
    keywords: HashMap<&'static str, &'static str>,
}

/// `(import name, embedded docs/stdlib/<file>.md)` for every stdlib
/// module. The import name is what users write in `import '...'` and
/// before the `.` in member access, so it is the catalog key.
const MODULE_DOCS: &[(&str, &str)] = &[
    // Pure-tigr source modules.
    ("Array", include_str!("../docs/stdlib/array.md")),
    ("Math", include_str!("../docs/stdlib/math.md")),
    ("String", include_str!("../docs/stdlib/string.md")),
    ("Map", include_str!("../docs/stdlib/map.md")),
    ("Set", include_str!("../docs/stdlib/set.md")),
    ("Object", include_str!("../docs/stdlib/object.md")),
    ("Iter", include_str!("../docs/stdlib/iter.md")),
    ("Http", include_str!("../docs/stdlib/http.md")),
    ("Url", include_str!("../docs/stdlib/url.md")),
    ("Channel", include_str!("../docs/stdlib/channel.md")),
    ("LocalChannel", include_str!("../docs/stdlib/localchannel.md")),
    ("Test", include_str!("../docs/stdlib/test.md")),
    // Native (Rust) modules.
    ("JSON", include_str!("../docs/stdlib/json.md")),
    ("IO", include_str!("../docs/stdlib/io.md")),
    ("Path", include_str!("../docs/stdlib/path.md")),
    ("Time", include_str!("../docs/stdlib/time.md")),
    ("DateTime", include_str!("../docs/stdlib/datetime.md")),
    ("Random", include_str!("../docs/stdlib/random.md")),
    ("Bytes", include_str!("../docs/stdlib/bytes.md")),
    ("BigInt", include_str!("../docs/stdlib/bigint.md")),
    ("Os", include_str!("../docs/stdlib/os.md")),
    ("Net", include_str!("../docs/stdlib/net.md")),
];

/// The builtins page has the same `### `sig`` shape but no module name.
const BUILTINS_DOC: &str = include_str!("../docs/stdlib/builtins.md");

/// Keywords with a one-line explanation. Not in the docs pages, so
/// hand-maintained; the list mirrors the lexer's keyword tokens.
const KEYWORDS: &[(&str, &str)] = &[
    ("fn", "Defines a function value: `fn(params) { body }`."),
    ("if", "Conditional expression: `if cond { ... } else { ... }`. Yields the taken branch's value."),
    ("else", "The alternative branch of an `if`."),
    ("for", "Iterates over a range, array, or iterator: `for x in xs { ... }`. Use `for[]` to collect."),
    ("while", "Loops while a condition is truthy: `while cond { ... }`."),
    ("match", "Pattern-matches a value against arms: `match x { pat => expr, ... }`."),
    ("try", "Evaluates an expression, catching any raised error: `try expr catch (e) { ... }`."),
    ("catch", "Handles an error raised inside the preceding `try`."),
    ("import", "Loads a module or file, returning its value: `M := import 'Name'`."),
    ("raise", "Raises an error, unwinding to the nearest `try`/`catch`."),
    ("return", "Returns a value from the enclosing function."),
    ("break", "Exits the enclosing loop, optionally with a value."),
    ("continue", "Skips to the next iteration of the enclosing loop."),
    ("spawn", "Starts an OS-thread actor running the given expression."),
    ("go", "Starts a green thread (cooperative coroutine) running the expression."),
    ("yield", "Suspends a generator, producing a value to its consumer."),
    ("gen", "Marks a function as a generator: `gen fn() { ... yield x ... }`."),
    ("null", "The absence of a value."),
    ("true", "The boolean true literal."),
    ("false", "The boolean false literal."),
];

impl Catalog {
    /// Build the catalog by parsing the embedded docs once. Cheap (a few
    /// string scans); call it once and cache the result.
    pub fn load() -> Catalog {
        let modules = MODULE_DOCS
            .iter()
            .map(|(name, text)| (name.to_string(), parse_doc(text)))
            .collect();
        let builtins = parse_doc(BUILTINS_DOC).members;
        let keywords = KEYWORDS.iter().copied().collect();
        Catalog { modules, builtins, keywords }
    }

    pub fn module(&self, name: &str) -> Option<&Module> {
        self.modules.get(name)
    }

    pub fn member(&self, module: &str, member: &str) -> Option<&Member> {
        self.modules.get(module)?.members.get(member)
    }

    pub fn builtin(&self, name: &str) -> Option<&Member> {
        self.builtins.get(name)
    }

    pub fn keyword(&self, name: &str) -> Option<&str> {
        self.keywords.get(name).copied()
    }

    /// Every module, as `(name, module)`. Order is unspecified.
    pub fn modules(&self) -> impl Iterator<Item = (&str, &Module)> {
        self.modules.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Every builtin, as `(name, member)`. Order is unspecified.
    pub fn builtins(&self) -> impl Iterator<Item = (&str, &Member)> {
        self.builtins.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Every keyword, as `(name, explanation)`. Order is unspecified.
    pub fn keywords(&self) -> impl Iterator<Item = (&str, &str)> {
        self.keywords.iter().map(|(k, v)| (*k, *v))
    }
}

impl Member {
    /// Whether this entry is a constant (e.g. `Math.PI`) rather than a
    /// callable. Constants have a bare-name signature with no `(`.
    pub fn is_constant(&self) -> bool {
        !self.signature.contains('(')
    }
}

/// Parse one docs page into a [`Module`]. The page's intro paragraph
/// becomes the description; each `### `sig`` section becomes a member.
fn parse_doc(text: &str) -> Module {
    let lines: Vec<&str> = text.lines().collect();
    Module {
        description: parse_description(&lines),
        members: parse_members(&lines),
    }
}

/// The first prose paragraph after the `# ` title, skipping the `>`
/// metadata blockquote. Stops at the next blank line, heading, or fence.
fn parse_description(lines: &[&str]) -> String {
    let mut started = false;
    let mut out: Vec<&str> = Vec::new();
    for line in lines {
        let t = line.trim();
        if !started {
            // Wait until past the title and its blockquote.
            if t.starts_with("# ") || t.starts_with('>') || t.is_empty() {
                continue;
            }
            started = true;
        }
        if t.is_empty() || t.starts_with('#') || t.starts_with("```") {
            break;
        }
        out.push(t);
    }
    out.join(" ")
}

/// Every `### `signature`` section, keyed by the name in the signature
/// (the text before `(`, or the whole signature for a constant). The
/// docstring is the section body up to its first code fence or the next
/// heading.
fn parse_members(lines: &[&str]) -> HashMap<String, Member> {
    let mut members = HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if let Some(sig) = section_signature(line) {
            let name = member_name(&sig);
            let mut body: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() {
                let b = lines[i].trim();
                if b.starts_with("```") || b.starts_with('#') {
                    break;
                }
                body.push(lines[i]);
                i += 1;
            }
            let doc = body.join("\n").trim().to_string();
            members.insert(name, Member { signature: sig, doc });
            continue; // `i` already points at the terminator.
        }
        i += 1;
    }
    members
}

/// If `line` is a `### `…`` member heading, return the backticked
/// signature. Anything else (prose, tables, deeper or shallower
/// headings) yields `None`.
fn section_signature(line: &str) -> Option<String> {
    let rest = line.strip_prefix("### ")?;
    backtick_content(rest)
}

/// The text inside the first pair of backticks in `s`.
fn backtick_content(s: &str) -> Option<String> {
    let start = s.find('`')? + 1;
    let end = s[start..].find('`')? + start;
    Some(s[start..end].to_string())
}

/// The member's lookup key: the signature up to its first `(`, trimmed.
/// A constant signature has no `(`, so the whole thing is the name.
fn member_name(sig: &str) -> String {
    sig.split('(').next().unwrap_or(sig).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_native_module() {
        let cat = Catalog::load();
        let json = cat.module("JSON").expect("JSON module");
        assert!(json.description.contains("JSON"));
        let parse = json.members.get("parse").expect("JSON.parse");
        assert_eq!(parse.signature, "parse(text) -> value");
        assert!(parse.doc.contains("Parses one JSON value"));
    }

    #[test]
    fn loads_a_source_module_and_its_constant() {
        let cat = Catalog::load();
        let math = cat.module("Math").expect("Math module");
        assert_eq!(math.members.get("sqrt").unwrap().signature, "sqrt(x) -> Float");
        // Constants live in `### `PI`` sections, not a table.
        let pi = math.members.get("PI").expect("Math.PI constant");
        assert_eq!(pi.signature, "PI");
        assert!(pi.doc.contains("circle"));
    }

    #[test]
    fn member_accessor_matches_module_lookup() {
        let cat = Catalog::load();
        assert!(cat.member("Array", "map").is_some());
        assert!(cat.member("Array", "nonesuch").is_none());
        assert!(cat.member("Nonesuch", "map").is_none());
    }

    #[test]
    fn loads_builtins() {
        let cat = Catalog::load();
        let print = cat.builtin("print").expect("print builtin");
        assert!(print.signature.starts_with("print("));
        assert!(cat.builtin("type").is_some());
        assert!(cat.builtin("nonesuch").is_none());
    }

    #[test]
    fn knows_keywords() {
        let cat = Catalog::load();
        assert!(cat.keyword("match").unwrap().contains("attern"));
        assert!(cat.keyword("notakeyword").is_none());
    }

    #[test]
    fn every_module_doc_parses_to_some_members() {
        let cat = Catalog::load();
        for (name, _) in MODULE_DOCS {
            let m = cat.module(name).unwrap();
            assert!(
                !m.members.is_empty(),
                "module {name} parsed to zero members"
            );
            assert!(!m.description.is_empty(), "module {name} has no description");
        }
    }
}
