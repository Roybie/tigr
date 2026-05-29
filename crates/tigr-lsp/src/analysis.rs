//! Lexical analysis over the tigr AST for go-to-definition, hover,
//! references, and rename.
//!
//! Every binding name carries a span: references are `Expr::Ident`, and
//! declarations are `ast::Binder`s (pattern leaves and `...rest`, params,
//! loop variables, the `catch` parameter, `=` assignment targets, and
//! `match`-arm bindings). So: find the name under the cursor, rebuild the
//! scope chain active at that point, and resolve innermost-first. A
//! binding's *identity* is its definition span, which lets references and
//! rename group every occurrence — the declaration plus all uses — that
//! resolves to it.

use std::collections::HashMap;

use tigr::vm::ast::{
    Binder, Block, Expr, MatchPattern, ObjectMember, Pattern, SpannedExpr, TemplatePart,
};
use tigr::vm::token::Span;

use crate::catalog::{Catalog, Member};

#[derive(Clone, Copy, PartialEq)]
pub enum BindingKind {
    Decl,
    Param,
    LoopVar,
    CatchParam,
    MatchBinding,
}

impl BindingKind {
    pub fn describe(self) -> &'static str {
        match self {
            BindingKind::Decl => "variable",
            BindingKind::Param => "parameter",
            BindingKind::LoopVar => "loop variable",
            BindingKind::CatchParam => "caught error",
            BindingKind::MatchBinding => "match binding",
        }
    }
}

#[derive(Clone)]
struct Binding {
    /// Where the name is defined, if the AST preserves it (`name := …`
    /// only). `None` means "in scope but unlocatable".
    def: Option<Span>,
    kind: BindingKind,
    /// For a `name := fn(...)` declaration, the function's signature
    /// (`name(p1, p2, ...rest)`) so hover can show its parameters.
    sig: Option<String>,
}

impl Binding {
    fn new(def: Option<Span>, kind: BindingKind) -> Binding {
        Binding { def, kind, sig: None }
    }
}

#[derive(Default)]
struct Scope {
    bindings: HashMap<String, Binding>,
}

/// Definition span to jump to for the identifier at `offset`, if any.
pub fn definition(program: &Block, offset: usize) -> Option<Span> {
    resolve(program, offset)?.1?.def
}

/// Markdown hover text for the thing at `offset`. Tries, in order: a
/// `Module.member` access, a local binding, a stdlib module name, and a
/// builtin. Keyword hover is handled by the caller (it needs the token
/// stream, not the AST).
pub fn hover(program: &Block, offset: usize, catalog: &Catalog) -> Option<String> {
    // Map each `Alias := import 'Canonical'` so `Alias.member` and a bare
    // `Alias` resolve to the catalog's canonically-named module.
    let imports = import_aliases(program);
    let canonical = |name: &str| imports.get(name).cloned().unwrap_or_else(|| name.to_string());

    // `Module.member` — the member key under the cursor.
    if let Some((alias, member)) = member_at(program, offset) {
        let module = canonical(&alias);
        if let Some(m) = catalog.member(&module, &member) {
            return Some(render_member(&module, m));
        }
    }
    // An identifier reference under the cursor.
    let (name, binding) = resolve(program, offset)?;
    if let Some(binding) = binding {
        // A binding that imports a known module shows the module, not
        // just "variable" — the common `M := import 'M'` case.
        if let Some(m) = catalog.module(&canonical(&name)) {
            return Some(render_module(&canonical(&name), m));
        }
        return Some(render_local(&name, &binding));
    }
    // Not a local — a stdlib module name, or a global builtin.
    if let Some(m) = catalog.module(&name) {
        return Some(render_module(&name, m));
    }
    if let Some(b) = catalog.builtin(&name) {
        return Some(render_member_sig(&b.signature, &b.doc));
    }
    None
}

/// `**Module `Name`**` plus its description.
fn render_module(name: &str, m: &crate::catalog::Module) -> String {
    format!("**Module `{name}`**\n\n{}", m.description)
}

/// Collect every `Alias := import 'Name'` in the program as
/// `alias -> Name`. A flat walk: module imports are effectively
/// program-global, and the last binding of a name wins.
fn import_aliases(program: &Block) -> HashMap<String, String> {
    let mut map = HashMap::new();
    walk_block(program, &mut |se| {
        if let Expr::Decl(Pattern::Ident(alias), init) = &se.expr {
            if let Expr::Import(arg) = &init.expr {
                if let Expr::Str(name) = &arg.expr {
                    map.insert(alias.name.clone(), name.clone());
                }
            }
        }
    });
    map
}

/// Render a stdlib member: its signature qualified with the module name,
/// then its docstring.
fn render_member(module: &str, m: &Member) -> String {
    render_member_sig(&format!("{module}.{}", m.signature), &m.doc)
}

/// A signature in a tigr code fence followed by doc prose.
fn render_member_sig(signature: &str, doc: &str) -> String {
    if doc.is_empty() {
        format!("```tigr\n{signature}\n```")
    } else {
        format!("```tigr\n{signature}\n```\n\n{doc}")
    }
}

/// Render a local binding. A `fn` declaration shows its signature; every
/// other binding shows its kind. Both note the definition line if known.
fn render_local(name: &str, b: &Binding) -> String {
    let mut out = match &b.sig {
        Some(sig) => format!("```tigr\n{sig}\n```\n\n*function*"),
        None => format!("**`{name}`** — {}", b.kind.describe()),
    };
    if let Some(def) = b.def {
        out.push_str(&format!("\n\n*defined on line {}*", def.line));
    }
    out
}

/// The `Module.member` access whose member key contains `offset`, if any.
/// Dot-access lowers to `Index(Ident(module), Str(member))`, and the key
/// `Str` carries the exact span of the member name.
pub fn member_at(program: &Block, offset: usize) -> Option<(String, String)> {
    let mut best: Option<(String, String, Span)> = None;
    walk_block(program, &mut |se| {
        if let Expr::Index(obj, key) = &se.expr {
            if let (Expr::Ident(module), Expr::Str(member)) = (&obj.expr, &key.expr) {
                if contains(key.span, offset) {
                    let better = best
                        .as_ref()
                        .is_none_or(|(_, _, s)| span_len(key.span) <= span_len(*s));
                    if better {
                        best = Some((module.clone(), member.clone(), key.span));
                    }
                }
            }
        }
    });
    best.map(|(m, k, _)| (m, k))
}

/// Render a function signature `name(p1, p2, ...rest)`. A parameter with
/// a default is suffixed `?`; destructuring params show their shape.
fn fn_signature(
    name: &str,
    params: &[Pattern],
    defaults: &[Option<Box<SpannedExpr>>],
    rest: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, p) in params.iter().enumerate() {
        let mut s = pattern_str(p);
        if defaults.get(i).is_some_and(|d| d.is_some()) {
            s.push('?');
        }
        parts.push(s);
    }
    if let Some(r) = rest {
        parts.push(format!("...{r}"));
    }
    format!("{name}({})", parts.join(", "))
}

/// A parameter pattern as it reads in a signature.
fn pattern_str(p: &Pattern) -> String {
    match p {
        Pattern::Ident(n) => n.name.clone(),
        Pattern::Wildcard => "_".to_string(),
        Pattern::Array { .. } => "[...]".to_string(),
        Pattern::Object { .. } => "${...}".to_string(),
    }
}

/// Find the reference at `offset` and resolve it. Returns the name plus
/// its binding (the inner `Option` is `None` when the name resolves to
/// no local binding — likely a global, builtin, or stdlib module).
fn resolve(program: &Block, offset: usize) -> Option<(String, Option<Binding>)> {
    let (name, _span) = ident_at(program, offset)?;
    let scopes = scopes_at(program, offset);
    let binding = scopes.iter().rev().find_map(|s| s.bindings.get(&name).cloned());
    Some((name, binding))
}

/// The scope chain active at `offset`, outermost first: the hoisted
/// top-level scope, then one scope per scope-opening construct on the
/// path to the cursor.
fn scopes_at(program: &Block, offset: usize) -> Vec<Scope> {
    let mut scopes = Vec::new();
    let mut root = Scope::default();
    hoist_block(program, &mut root);
    scopes.push(root);
    descend_block(program, offset, &mut scopes);
    scopes
}

/// One in-scope binding, for completion. A `fn` decl carries its
/// signature so completion can show parameters in the detail.
pub struct Local {
    pub name: String,
    pub kind: BindingKind,
    pub sig: Option<String>,
}

/// Every binding visible at `offset`, innermost shadowing outermost.
pub fn locals_in_scope(program: &Block, offset: usize) -> Vec<Local> {
    let mut map: HashMap<String, Local> = HashMap::new();
    // `scopes_at` is outermost-first, so a later (inner) entry overwrites
    // an outer one of the same name — the shadowing the resolver applies.
    for scope in scopes_at(program, offset) {
        for (name, b) in scope.bindings {
            map.insert(
                name.clone(),
                Local { name, kind: b.kind, sig: b.sig },
            );
        }
    }
    map.into_values().collect()
}

/// Resolve an identifier to the module it imports, if it is an
/// `Alias := import 'Name'` binding; otherwise return it unchanged.
pub fn canonical_module(program: &Block, name: &str) -> String {
    import_aliases(program)
        .remove(name)
        .unwrap_or_else(|| name.to_string())
}

// ---------------- cross-file imports (Phase 5a) ----------------

/// Something under the cursor that points into another file. The `path` is
/// the raw import-path string as written (e.g. `./dns`); the caller
/// resolves it to a file relative to the importing document.
pub enum ImportTarget {
    /// The whole module: the cursor is on an `import '<path>'` path string.
    Module { path: String },
    /// A specific member: the cursor is on the `member` of `Alias.member`,
    /// where `Alias` is a `:= import '<path>'` file import.
    Member { path: String, member: String },
}

/// Whether an import path resolves to a file (rather than a built-in
/// module). Mirrors the VM: a path is file-shaped iff it contains a path
/// separator or a `.`; bare names resolve against the stdlib/native
/// modules, which have no on-disk location to jump to.
pub fn is_file_path(path: &str) -> bool {
    path.contains('/') || path.contains('\\') || path.contains('.')
}

/// If `offset` sits on something that refers into an imported file, return
/// what to resolve. Two shapes: the path string of an `import` expression,
/// or the `member` of a `file_alias.member` access. Bare (stdlib) imports
/// are not file targets and return `None` here (hover/the catalog cover
/// them).
pub fn import_target(program: &Block, offset: usize) -> Option<ImportTarget> {
    // `Alias.member` — resolve the receiver alias to its import path.
    if let Some((alias, member)) = member_at(program, offset) {
        if let Some(path) = import_aliases(program).remove(&alias) {
            if is_file_path(&path) {
                return Some(ImportTarget::Member { path, member });
            }
        }
    }
    // The path string of an `import '<path>'` under the cursor.
    let mut found: Option<String> = None;
    walk_block(program, &mut |se| {
        if let Expr::Import(arg) = &se.expr {
            if let Expr::Str(path) = &arg.expr {
                if contains(arg.span, offset) && is_file_path(path) {
                    found = Some(path.clone());
                }
            }
        }
    });
    found.map(|path| ImportTarget::Module { path })
}

/// The definition span of `member` within an imported module's tree.
///
/// A tigr module hands back the value of its final expression, which is
/// conventionally an object literal of `name: value` exports. So: take the
/// module's final value; if it is an object literal with a `member:` field,
/// jump to the field value — following a bare-identifier value (the common
/// `resolve: _resolve` re-export) through to the top-level declaration it
/// names. If the export object yields nothing, fall back to a top-level
/// declaration named `member` directly.
pub fn module_member_def(foreign: &Block, member: &str) -> Option<Span> {
    if let Some(value) = export_object_field(foreign, member) {
        // `member: some_ident` re-exports a top-level binding — prefer its
        // declaration over the reference in the export object.
        if let Expr::Ident(name) = &value.expr {
            if let Some(span) = top_level_decl_span(foreign, name) {
                return Some(span);
            }
        }
        return Some(value.span);
    }
    top_level_decl_span(foreign, member)
}

/// The value expression of the `member:` field in the module's exported
/// object literal, if the module's final value is one.
fn export_object_field<'a>(foreign: &'a Block, member: &str) -> Option<&'a SpannedExpr> {
    let value = module_value(foreign)?;
    if let Expr::Object(members) = &value.expr {
        for m in members {
            if let ObjectMember::Pair(key, v) = m {
                if key == member {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The module's value: its final expression. Unwraps a trailing
/// block/scope so `( ...; ${...} )` or `{ ...; ${...} }` still reach the
/// object inside.
fn module_value(block: &Block) -> Option<&SpannedExpr> {
    let tail = block.tail.as_deref()?;
    match &tail.expr {
        Expr::Block(inner) | Expr::Scope(inner) => module_value(inner),
        _ => Some(tail),
    }
}

/// The binder span of a top-level `name := …` declaration, if present.
fn top_level_decl_span(block: &Block, name: &str) -> Option<Span> {
    top_level_decl(block, name).map(|(span, _)| span)
}

/// The binder span and initialiser of a top-level `name := init`
/// declaration, if present.
fn top_level_decl<'a>(block: &'a Block, name: &str) -> Option<(Span, &'a SpannedExpr)> {
    for se in block.stmts.iter().chain(block.tail.as_deref()) {
        if let Expr::Decl(Pattern::Ident(binder), init) = &se.expr {
            if binder.name == name {
                return Some((binder.span, init));
            }
        }
    }
    None
}

/// Every `alias := import 'path'` in `program`, as `(alias, path)` pairs in
/// source order (duplicates kept — a file may bind the same module twice).
/// Unlike `import_aliases`, this is a list, so all aliases of one module are
/// available to the cross-file reference scan.
pub fn import_alias_pairs(program: &Block) -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk_block(program, &mut |se| {
        if let Expr::Decl(Pattern::Ident(alias), init) = &se.expr {
            if let Expr::Import(arg) = &init.expr {
                if let Expr::Str(path) = &arg.expr {
                    out.push((alias.name.clone(), path.clone()));
                }
            }
        }
    });
    out
}

/// The key spans of every `receiver.member` access in `program` (the
/// member-name `Str` of an `Index(Ident(receiver), Str(member))`). Used to
/// find and rename cross-file uses of an exported member.
pub fn member_access_spans(program: &Block, receiver: &str, member: &str) -> Vec<Span> {
    let mut out = Vec::new();
    walk_block(program, &mut |se| {
        if let Expr::Index(obj, key) = &se.expr {
            if let (Expr::Ident(r), Expr::Str(m)) = (&obj.expr, &key.expr) {
                if r == receiver && m == member {
                    out.push(key.span);
                }
            }
        }
    });
    out
}

/// The span of a module's exported object literal (`module_value`), when its
/// final value is one. The caller text-scans within it to locate an export
/// key, which the AST doesn't span (an `ObjectMember::Pair` key is a bare
/// `String`).
pub fn export_object_span(foreign: &Block) -> Option<Span> {
    let value = module_value(foreign)?;
    matches!(value.expr, Expr::Object(_)).then_some(value.span)
}

/// One member of a user module's exported object literal.
pub struct ExportedMember {
    /// The export key (the name read off the imported object).
    pub name: String,
    /// A function shows its parameter list (`resolve(name, qtype?)`);
    /// any other value is just the bare name.
    pub signature: String,
    pub is_function: bool,
    /// Where the member is defined in the module file — the re-exported
    /// declaration, or the inline value expression.
    pub def_span: Span,
}

/// The members a user module exports: the fields of its final object
/// literal (`module_value`). A `key: ident` re-export takes its signature
/// and jump target from the top-level declaration `ident` names; an inline
/// value uses the value itself. A module whose value is not an object
/// literal exports nothing here.
pub fn module_exports(foreign: &Block) -> Vec<ExportedMember> {
    let Some(value) = module_value(foreign) else {
        return Vec::new();
    };
    let Expr::Object(members) = &value.expr else {
        return Vec::new();
    };
    members
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Pair(key, v) => Some(export_member(foreign, key, v)),
            ObjectMember::Spread(_) => None,
        })
        .collect()
}

fn export_member(foreign: &Block, key: &str, value: &SpannedExpr) -> ExportedMember {
    // `key: some_ident` re-exports a top-level binding: describe and locate
    // it from its declaration rather than from the export field.
    if let Expr::Ident(name) = &value.expr {
        if let Some((span, init)) = top_level_decl(foreign, name) {
            let (signature, is_function) = fn_sig_or_name(key, init);
            return ExportedMember { name: key.to_string(), signature, is_function, def_span: span };
        }
    }
    let (signature, is_function) = fn_sig_or_name(key, value);
    ExportedMember { name: key.to_string(), signature, is_function, def_span: value.span }
}

/// `(signature, is_function)` for an export value: a `fn` literal shows its
/// parameter list under `key`; anything else is just `key`.
fn fn_sig_or_name(key: &str, e: &SpannedExpr) -> (String, bool) {
    if let Expr::Fn { params, defaults, rest, .. } = &e.expr {
        (fn_signature(key, params, defaults, rest.as_deref()), true)
    } else {
        (key.to_string(), false)
    }
}

/// Look up `name` in the scope chain active at `offset`, innermost first.
fn binding_of(program: &Block, name: &str, offset: usize) -> Option<Binding> {
    scopes_at(program, offset)
        .iter()
        .rev()
        .find_map(|s| s.bindings.get(name).cloned())
}

// ---------------- references and rename ----------------

/// Whether a name occurrence introduces a binding or refers to one.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    /// A declaration site: a decl/param/loop/catch binder. Its identity
    /// is its own span.
    Def,
    /// A use of an existing binding: an `Expr::Ident`, an `=` assignment
    /// target, or a destructuring-assignment leaf. Its identity is the
    /// def span of whatever binding it resolves to.
    Ref,
}

/// One occurrence of a binding name in the source.
struct Occurrence {
    name: String,
    span: Span,
    role: Role,
}

/// The binding identity an occurrence belongs to: a `Def` is its own
/// definition span; a `Ref` resolves to the binding visible at its
/// position. `None` means it ties to no locatable binding — a builtin or
/// a stdlib module — so it is never grouped or renamed. Compiler-internal
/// `$`-names are excluded too.
fn identity(occ: &Occurrence, program: &Block) -> Option<Span> {
    if occ.name.starts_with('$') {
        return None;
    }
    match occ.role {
        Role::Def => Some(occ.span),
        Role::Ref => binding_of(program, &occ.name, occ.span.start).and_then(|b| b.def),
    }
}

/// Every binding-name occurrence in `program`, declarations and uses.
fn collect_occurrences(program: &Block) -> Vec<Occurrence> {
    let mut out = Vec::new();
    walk_block(program, &mut |se| match &se.expr {
        Expr::Ident(name) => out.push(Occurrence {
            name: name.clone(),
            span: se.span,
            role: Role::Ref,
        }),
        // `x = …` / `x += …` — the target refers to an existing binding.
        Expr::Assign(target, _, _) => out.push(Occurrence {
            name: target.name.clone(),
            span: target.span,
            role: Role::Ref,
        }),
        // `pat := …` declares; `pat = …` writes existing bindings.
        Expr::Decl(pat, _) => collect_pattern_occurrences(pat, Role::Def, &mut out),
        Expr::AssignPattern(pat, _) => collect_pattern_occurrences(pat, Role::Ref, &mut out),
        Expr::Fn { params, rest, .. } => {
            for p in params {
                collect_pattern_occurrences(p, Role::Def, &mut out);
            }
            if let Some(r) = rest {
                out.push(binder_occ(r, Role::Def));
            }
        }
        Expr::For { vars, .. } => {
            for v in vars {
                out.push(binder_occ(v, Role::Def));
            }
        }
        Expr::Try { catch: Some((param, _)), .. } => out.push(binder_occ(param, Role::Def)),
        // A `match` arm's pattern introduces bindings, each at its own
        // span (refs to them inside the body/guard are walked as `Ident`s).
        Expr::Match { arms, .. } => {
            for arm in arms {
                collect_match_pattern_occurrences(&arm.pattern, &mut out);
            }
        }
        _ => {}
    });
    out
}

/// Push a `Def` occurrence for every binding in a refutable `match`
/// pattern, each at its own span.
fn collect_match_pattern_occurrences(pat: &MatchPattern, out: &mut Vec<Occurrence>) {
    match pat {
        MatchPattern::Binding(b) => out.push(binder_occ(b, Role::Def)),
        MatchPattern::Array { items, rest } => {
            for it in items {
                collect_match_pattern_occurrences(it, out);
            }
            if let Some(r) = rest {
                out.push(binder_occ(r, Role::Def));
            }
        }
        MatchPattern::Object { fields, rest } => {
            for fld in fields {
                match &fld.pattern {
                    Some(p) => collect_match_pattern_occurrences(p, out),
                    // Shorthand `${name}` binds the key, at its span.
                    None => out.push(binder_occ(
                        &Binder::new(fld.key.clone(), fld.key_span),
                        Role::Def,
                    )),
                }
            }
            if let Some(r) = rest {
                out.push(binder_occ(r, Role::Def));
            }
        }
        MatchPattern::Literal(_)
        | MatchPattern::Wildcard
        | MatchPattern::Range { .. }
        | MatchPattern::Or(_) => {}
    }
}

fn binder_occ(b: &Binder, role: Role) -> Occurrence {
    Occurrence { name: b.name.clone(), span: b.span, role }
}

/// Push an occurrence for every binder in a pattern, all with `role`.
fn collect_pattern_occurrences(pat: &Pattern, role: Role, out: &mut Vec<Occurrence>) {
    let mut binders = Vec::new();
    pattern_binders(pat, &mut binders);
    for b in binders {
        out.push(binder_occ(b, role));
    }
}

/// All occurrence spans of the binding at `offset` — its declaration plus
/// every use — sorted by position. `None` when the cursor is not on a
/// locatable binding (a builtin, stdlib module, or nothing). Powers both
/// references and rename.
fn binding_occurrences(program: &Block, offset: usize) -> Option<Vec<Span>> {
    let occs = collect_occurrences(program);
    // The occurrence under the cursor: the tightest span containing it.
    let target = occs
        .iter()
        .filter(|o| contains(o.span, offset))
        .min_by_key(|o| span_len(o.span))?;
    let target_id = identity(target, program)?;

    let mut spans: Vec<Span> = occs
        .iter()
        .filter(|o| identity(o, program) == Some(target_id))
        .map(|o| o.span)
        .collect();
    spans.sort_by_key(|s| s.start);
    spans.dedup_by_key(|s| s.start);
    Some(spans)
}

/// Spans of every reference to the binding at `offset` (declaration
/// included). Empty if the cursor is not on a locatable binding.
pub fn references(program: &Block, offset: usize) -> Vec<Span> {
    binding_occurrences(program, offset).unwrap_or_default()
}

/// The set of spans a rename should rewrite, and the definition span among
/// them. `None` when the target cannot be renamed (a builtin, stdlib
/// module, or nothing under the cursor).
pub fn rename_spans(program: &Block, offset: usize) -> Option<RenameTarget> {
    let occs = collect_occurrences(program);
    let target = occs
        .iter()
        .filter(|o| contains(o.span, offset))
        .min_by_key(|o| span_len(o.span))?;
    let def = identity(target, program)?;
    let mut spans: Vec<Span> = occs
        .iter()
        .filter(|o| identity(o, program) == Some(def))
        .map(|o| o.span)
        .collect();
    spans.sort_by_key(|s| s.start);
    spans.dedup_by_key(|s| s.start);
    Some(RenameTarget { def, spans })
}

/// The result of resolving a rename request.
pub struct RenameTarget {
    /// The declaration span (one of `spans`); useful for prepare-rename.
    pub def: Span,
    /// Every span to rewrite, sorted by position.
    pub spans: Vec<Span>,
}

// ---------------- document symbols (outline) ----------------

/// What kind of entity a [`SymbolNode`] is, kept independent of the LSP
/// `SymbolKind` so this module stays free of `tower_lsp`. The caller maps
/// it onto the protocol type.
#[derive(Clone, Copy, PartialEq)]
pub enum SymbolCategory {
    Function,
    Variable,
    Module,
}

/// One node in the document outline. Spans are byte offsets into the
/// source; the caller projects them onto LSP ranges. `range` covers the
/// whole declaration, `selection` just the name.
pub struct SymbolNode {
    pub name: String,
    /// A function's signature, or an import's `'name'`; `None` otherwise.
    pub detail: Option<String>,
    pub category: SymbolCategory,
    pub range: Span,
    pub selection: Span,
    /// Nested declarations inside a function body.
    pub children: Vec<SymbolNode>,
}

/// The outline of `program`: top-level declarations, with the locals of a
/// `name := fn(...)` nested beneath it. Only `:=` declarations surface;
/// every other statement is skipped.
pub fn document_symbols(program: &Block) -> Vec<SymbolNode> {
    let mut out = Vec::new();
    symbols_in_block(program, &mut out);
    out
}

fn symbols_in_block(block: &Block, out: &mut Vec<SymbolNode>) {
    for se in block.stmts.iter().chain(block.tail.as_deref()) {
        if let Expr::Decl(pat, init) = &se.expr {
            decl_symbols(pat, se.span, init, out);
        }
    }
}

/// Emit the symbol(s) for one declaration. A bare `Ident` becomes a single
/// precisely-located node; a destructuring pattern becomes one node per
/// bound name, all sharing the declaration's span (the AST keeps no span
/// for individual destructured names).
fn decl_symbols(pat: &Pattern, decl_span: Span, init: &SpannedExpr, out: &mut Vec<SymbolNode>) {
    match pat {
        Pattern::Ident(name) => out.push(ident_decl_symbol(name, decl_span, init)),
        Pattern::Wildcard => {}
        Pattern::Array { .. } | Pattern::Object { .. } => {
            let mut binders = Vec::new();
            pattern_binders(pat, &mut binders);
            for b in binders {
                out.push(SymbolNode {
                    name: b.name.clone(),
                    detail: None,
                    category: SymbolCategory::Variable,
                    range: decl_span,
                    selection: b.span,
                    children: Vec::new(),
                });
            }
        }
    }
}

/// A `name := …` declaration. A `fn` initialiser becomes a Function (with
/// its signature and nested locals); an `import` becomes a Module; anything
/// else a Variable.
fn ident_decl_symbol(binder: &Binder, decl_span: Span, init: &SpannedExpr) -> SymbolNode {
    let name = &binder.name;
    let selection = binder.span;
    match &init.expr {
        Expr::Fn { params, defaults, rest, body, .. } => {
            let mut children = Vec::new();
            if let Expr::Scope(b) | Expr::Block(b) = &body.expr {
                symbols_in_block(b, &mut children);
            }
            SymbolNode {
                name: name.to_string(),
                detail: Some(fn_signature(name, params, defaults, rest.as_deref())),
                category: SymbolCategory::Function,
                range: decl_span,
                selection,
                children,
            }
        }
        Expr::Import(arg) => {
            let detail = match &arg.expr {
                Expr::Str(module) => Some(format!("import '{module}'")),
                _ => None,
            };
            SymbolNode {
                name: name.to_string(),
                detail,
                category: SymbolCategory::Module,
                range: decl_span,
                selection,
                children: Vec::new(),
            }
        }
        _ => SymbolNode {
            name: name.to_string(),
            detail: None,
            category: SymbolCategory::Variable,
            range: decl_span,
            selection,
            children: Vec::new(),
        },
    }
}

/// Every binder of a (possibly nested) destructuring pattern, in source
/// order, each with its own span.
fn pattern_binders<'a>(pat: &'a Pattern, out: &mut Vec<&'a Binder>) {
    match pat {
        Pattern::Wildcard => {}
        Pattern::Ident(b) => out.push(b),
        Pattern::Array { items, rest } => {
            for it in items {
                pattern_binders(it, out);
            }
            if let Some(r) = rest {
                out.push(r);
            }
        }
        Pattern::Object { fields, rest } => {
            for fld in fields {
                pattern_binders(&fld.pattern, out);
            }
            if let Some(r) = rest {
                out.push(r);
            }
        }
    }
}

fn contains(span: Span, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

fn span_len(span: Span) -> usize {
    span.end.saturating_sub(span.start)
}

// ---------------- find the reference under the cursor ----------------

/// The innermost `Expr::Ident` whose span contains `offset`.
fn ident_at(program: &Block, offset: usize) -> Option<(String, Span)> {
    let mut best: Option<(String, Span)> = None;
    walk_block(program, &mut |se| {
        if let Expr::Ident(name) = &se.expr {
            if contains(se.span, offset) {
                let better = best
                    .as_ref()
                    .is_none_or(|(_, s)| span_len(se.span) <= span_len(*s));
                if better {
                    best = Some((name.clone(), se.span));
                }
            }
        }
    });
    best
}

fn walk_block(block: &Block, f: &mut impl FnMut(&SpannedExpr)) {
    for se in block.stmts.iter().chain(block.tail.as_deref()) {
        walk_expr(se, f);
    }
}

fn walk_expr(se: &SpannedExpr, f: &mut impl FnMut(&SpannedExpr)) {
    f(se);
    for child in child_exprs(se) {
        walk_expr(child, f);
    }
}

// ---------------- scope construction ----------------

/// Collect the `:=` declarations belonging to this scope: descend through
/// everything except the constructs that open their own scope, so a
/// nested function's locals don't leak outward.
fn hoist_block(block: &Block, scope: &mut Scope) {
    for se in block.stmts.iter().chain(block.tail.as_deref()) {
        hoist_expr(se, scope);
    }
}

fn hoist_expr(se: &SpannedExpr, scope: &mut Scope) {
    match &se.expr {
        Expr::Decl(pat, init) => {
            add_pattern(pat, BindingKind::Decl, scope);
            // `name := fn(...)` — remember the signature for hover.
            if let (Pattern::Ident(name), Expr::Fn { params, defaults, rest, .. }) =
                (pat, &init.expr)
            {
                if let Some(b) = scope.bindings.get_mut(name.name.as_str()) {
                    b.sig = Some(fn_signature(name, params, defaults, rest.as_deref()));
                }
            }
            // A decl inside the initialiser (e.g. `(y := 1; y)`) shares
            // this scope unless it's behind a scope boundary, which the
            // recursion below stops at.
            hoist_expr(init, scope);
        }
        // Scope boundaries own their declarations.
        Expr::Scope(_)
        | Expr::Fn { .. }
        | Expr::For { .. }
        | Expr::While { .. }
        | Expr::Match { .. } => {}
        _ => {
            for child in child_exprs(se) {
                hoist_expr(child, scope);
            }
        }
    }
}

/// Insert the names a pattern binds, each located at its own `Binder`
/// span (so destructured leaves and `...rest` are jumpable too).
fn add_pattern(pat: &Pattern, kind: BindingKind, scope: &mut Scope) {
    match pat {
        Pattern::Wildcard => {}
        Pattern::Ident(b) => bind_binder(scope, b, kind),
        Pattern::Array { items, rest } => {
            for it in items {
                add_pattern(it, kind, scope);
            }
            if let Some(r) = rest {
                bind_binder(scope, r, kind);
            }
        }
        Pattern::Object { fields, rest } => {
            for fld in fields {
                add_pattern(&fld.pattern, kind, scope);
            }
            if let Some(r) = rest {
                bind_binder(scope, r, kind);
            }
        }
    }
}

/// Bind a spanned name: its definition span is the binder's own span.
fn bind_binder(scope: &mut Scope, b: &Binder, kind: BindingKind) {
    scope.bindings.insert(b.name.clone(), Binding::new(Some(b.span), kind));
}

/// Walk from `block` toward `offset`, pushing a new `Scope` each time the
/// path crosses a scope-opening construct.
fn descend_block(block: &Block, offset: usize, scopes: &mut Vec<Scope>) {
    for se in block.stmts.iter().chain(block.tail.as_deref()) {
        if contains(se.span, offset) {
            descend_expr(se, offset, scopes);
            return;
        }
    }
}

fn descend_expr(se: &SpannedExpr, offset: usize, scopes: &mut Vec<Scope>) {
    match &se.expr {
        // `(a; b; c)` and bare blocks share the enclosing scope.
        Expr::Block(b) => descend_block(b, offset, scopes),

        Expr::Scope(b) => {
            let mut s = Scope::default();
            hoist_block(b, &mut s);
            scopes.push(s);
            descend_block(b, offset, scopes);
        }

        Expr::Fn { params, defaults, rest, body, .. } => {
            let mut s = Scope::default();
            for p in params {
                add_pattern(p, BindingKind::Param, &mut s);
            }
            if let Some(r) = rest {
                bind_binder(&mut s, r, BindingKind::Param);
            }
            scopes.push(s);
            for d in defaults.iter().flatten() {
                if contains(d.span, offset) {
                    descend_expr(d, offset, scopes);
                    return;
                }
            }
            if contains(body.span, offset) {
                descend_expr(body, offset, scopes);
            }
        }

        Expr::For { vars, iter, body, .. } => {
            if contains(iter.span, offset) {
                descend_expr(iter, offset, scopes);
                return;
            }
            if contains(body.span, offset) {
                let mut s = Scope::default();
                for v in vars {
                    bind_binder(&mut s, v, BindingKind::LoopVar);
                }
                scopes.push(s);
                descend_expr(body, offset, scopes);
            }
        }

        Expr::Match { subject, arms } => {
            if contains(subject.span, offset) {
                descend_expr(subject, offset, scopes);
                return;
            }
            for arm in arms {
                let in_guard =
                    arm.guard.as_ref().is_some_and(|g| contains(g.span, offset));
                if in_guard || contains(arm.body.span, offset) {
                    let mut s = Scope::default();
                    match_bindings(&arm.pattern, &mut s);
                    scopes.push(s);
                    if in_guard {
                        descend_expr(arm.guard.as_ref().unwrap(), offset, scopes);
                    } else {
                        descend_expr(&arm.body, offset, scopes);
                    }
                    return;
                }
            }
        }

        Expr::Try { body, catch } => {
            if contains(body.span, offset) {
                descend_expr(body, offset, scopes);
                return;
            }
            if let Some((param, handler)) = catch {
                if contains(handler.span, offset) {
                    let mut s = Scope::default();
                    bind_binder(&mut s, param, BindingKind::CatchParam);
                    scopes.push(s);
                    descend_expr(handler, offset, scopes);
                }
            }
        }

        // No new scope: descend into whichever child holds the offset.
        _ => {
            for child in child_exprs(se) {
                if contains(child.span, offset) {
                    descend_expr(child, offset, scopes);
                    return;
                }
            }
        }
    }
}

/// Names bound by a refutable `match` pattern.
fn match_bindings(pat: &tigr::vm::ast::MatchPattern, scope: &mut Scope) {
    use tigr::vm::ast::MatchPattern as M;
    match pat {
        M::Binding(b) => bind_binder(scope, b, BindingKind::MatchBinding),
        M::Array { items, rest } => {
            for it in items {
                match_bindings(it, scope);
            }
            if let Some(r) = rest {
                bind_binder(scope, r, BindingKind::MatchBinding);
            }
        }
        M::Object { fields, rest } => {
            for fld in fields {
                match &fld.pattern {
                    Some(p) => match_bindings(p, scope),
                    // Shorthand `${name}` binds the key name itself, at
                    // the key's span.
                    None => bind_binder(
                        scope,
                        &Binder::new(fld.key.clone(), fld.key_span),
                        BindingKind::MatchBinding,
                    ),
                }
            }
            if let Some(r) = rest {
                bind_binder(scope, r, BindingKind::MatchBinding);
            }
        }
        // Literals, wildcards, ranges, and or-alternatives bind nothing
        // (or-patterns may not bind, per spec §match).
        M::Literal(_) | M::Wildcard | M::Range { .. } | M::Or(_) => {}
    }
}

// ---------------- direct child expressions ----------------

/// Every direct child `SpannedExpr` of `se`, in source order. Used by
/// both the reference search and the generic (non-scope) descent.
fn child_exprs(se: &SpannedExpr) -> Vec<&SpannedExpr> {
    let mut out: Vec<&SpannedExpr> = Vec::new();
    match &se.expr {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Ident(_)
        | Expr::Continue => {}

        Expr::BinOp(_, l, r) => {
            out.push(l);
            out.push(r);
        }
        Expr::UnOp(_, e) => out.push(e),

        Expr::Block(b) | Expr::Scope(b) => {
            out.extend(b.stmts.iter());
            if let Some(t) = &b.tail {
                out.push(t);
            }
        }

        Expr::Decl(_, init) => out.push(init),
        Expr::Assign(_, _, rhs) => out.push(rhs),
        Expr::AssignPattern(_, rhs) => out.push(rhs),

        Expr::If(c, t, e) => {
            out.push(c);
            out.push(t);
            out.push(e);
        }
        Expr::While { cond, body, .. } => {
            out.push(cond);
            out.push(body);
        }
        Expr::For { iter, body, .. } => {
            out.push(iter);
            out.push(body);
        }
        Expr::Range { from, to, step, .. } => {
            out.push(from);
            out.push(to);
            if let Some(s) = step {
                out.push(s);
            }
        }
        Expr::Break(v) | Expr::Return(v) | Expr::Yield(v) => {
            if let Some(v) = v {
                out.push(v);
            }
        }
        Expr::Array(items) => out.extend(items.iter()),
        Expr::Object(members) => {
            for m in members {
                match m {
                    ObjectMember::Pair(_, e) | ObjectMember::Spread(e) => out.push(e),
                }
            }
        }
        Expr::Spread(e) => out.push(e),
        Expr::Template(parts) => {
            for p in parts {
                if let TemplatePart::Expr(e) = p {
                    out.push(e);
                }
            }
        }
        Expr::Index(o, k) => {
            out.push(o);
            out.push(k);
        }
        Expr::IndexAssign(o, k, _, v) => {
            out.push(o);
            out.push(k);
            out.push(v);
        }
        Expr::Call(callee, args) => {
            out.push(callee);
            out.extend(args.iter());
        }
        Expr::Fn { defaults, body, .. } => {
            for d in defaults.iter().flatten() {
                out.push(d);
            }
            out.push(body);
        }
        Expr::Import(e) | Expr::Raise(e) | Expr::Spawn(e) | Expr::Go(e) => out.push(e),
        Expr::Try { body, catch } => {
            out.push(body);
            if let Some((_, handler)) = catch {
                out.push(handler);
            }
        }
        Expr::Match { subject, arms } => {
            out.push(subject);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    out.push(g);
                }
                out.push(&arm.body);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tigr::vm::parse_tree;

    /// Byte offset just inside the first occurrence of `needle` (one past
    /// its start, so the cursor lands on the token, not its boundary).
    fn at(src: &str, needle: &str) -> usize {
        src.find(needle).expect("needle present") + 1
    }

    fn hover_at(src: &str, needle: &str) -> Option<String> {
        let cat = Catalog::load();
        hover(&parse_tree(src), at(src, needle), &cat)
    }

    #[test]
    fn hovers_a_module_member() {
        let src = "Math := import 'Math';\nMath.sqrt(2.0);";
        let h = hover_at(src, "sqrt").expect("hover on sqrt");
        assert!(h.contains("Math.sqrt(x) -> Float"), "got: {h}");
        assert!(h.contains("square root"), "got: {h}");
    }

    #[test]
    fn resolves_an_aliased_import_for_member_access() {
        // `M` aliases `Math`; member lookup must canonicalize.
        let src = "M := import 'Math';\nM.sqrt(2.0);";
        let h = hover_at(src, "sqrt").expect("hover on aliased sqrt");
        assert!(h.contains("Math.sqrt(x) -> Float"), "got: {h}");
    }

    #[test]
    fn hovers_an_import_alias_as_its_module() {
        let src = "Math := import 'Math';\nMath.sqrt(2.0);";
        // The bare `Math` reference on line 2.
        let off = src.rfind("Math").unwrap() + 1;
        let h = hover(&parse_tree(src), off, &Catalog::load()).expect("hover on Math");
        assert!(h.contains("Module `Math`"), "got: {h}");
    }

    #[test]
    fn hovers_a_builtin() {
        let src = "print(42);";
        let h = hover_at(src, "print").expect("hover on print");
        assert!(h.contains("print("), "got: {h}");
    }

    #[test]
    fn hovers_a_local_function_with_its_signature() {
        let src = "add := fn(a, b) { a + b };\nadd(1, 2);";
        // The call site reference, not the declaration.
        let off = src.rfind("add").unwrap() + 1;
        let h = hover(&parse_tree(src), off, &Catalog::load()).expect("hover on add");
        assert!(h.contains("add(a, b)"), "got: {h}");
        assert!(h.contains("function"), "got: {h}");
    }

    #[test]
    fn plain_local_shows_its_kind() {
        let src = "x := 41;\nx + 1;";
        let off = src.rfind('x').unwrap();
        let h = hover(&parse_tree(src), off, &Catalog::load()).expect("hover on x");
        assert!(h.contains("variable"), "got: {h}");
    }

    #[test]
    fn no_hover_for_an_unknown_bare_identifier() {
        let src = "foo + 1;";
        assert!(hover_at(src, "foo").is_none());
    }

    #[test]
    fn locals_in_scope_sees_outer_decls_and_inner_params() {
        let src = "a := 1;\nf := fn(p) { b := 2; p };\nc := 3;";
        // Offset inside the function body, on `b`.
        let off = src.find("b := 2").unwrap();
        let names: std::collections::HashSet<String> =
            locals_in_scope(&parse_tree(src), off)
                .into_iter()
                .map(|l| l.name)
                .collect();
        for want in ["a", "f", "c", "p", "b"] {
            assert!(names.contains(want), "missing {want} in {names:?}");
        }
    }

    #[test]
    fn local_function_carries_its_signature() {
        let src = "f := fn(p) { b := 2; p };";
        let off = src.find("b := 2").unwrap();
        let f = locals_in_scope(&parse_tree(src), off)
            .into_iter()
            .find(|l| l.name == "f")
            .unwrap();
        assert_eq!(f.sig.as_deref(), Some("f(p)"));
    }

    #[test]
    fn canonical_module_resolves_aliases() {
        let prog = parse_tree("M := import 'Math';");
        assert_eq!(canonical_module(&prog, "M"), "Math");
        assert_eq!(canonical_module(&prog, "Other"), "Other");
    }

    #[test]
    fn import_target_on_member_of_a_file_import() {
        let src = "DNS := import './dns';\nDNS.resolve('a');";
        let off = at(src, "resolve(");
        match import_target(&parse_tree(src), off) {
            Some(ImportTarget::Member { path, member }) => {
                assert_eq!(path, "./dns");
                assert_eq!(member, "resolve");
            }
            other => panic!("expected Member, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn import_target_on_a_path_string() {
        let src = "DNS := import './dns';";
        let off = at(src, "./dns");
        match import_target(&parse_tree(src), off) {
            Some(ImportTarget::Module { path }) => assert_eq!(path, "./dns"),
            _ => panic!("expected Module"),
        }
    }

    #[test]
    fn import_target_ignores_bare_stdlib_imports() {
        let src = "Math := import 'Math';\nMath.sqrt(4.0);";
        // Neither the path string nor the member of a bare import is a file.
        assert!(import_target(&parse_tree(src), at(src, "'Math'")).is_none());
        assert!(import_target(&parse_tree(src), at(src, "sqrt(")).is_none());
    }

    #[test]
    fn module_member_def_follows_a_reexport_to_its_decl() {
        // The conventional shape: private decls re-exported by an object.
        let module = "_resolve := fn(n) { n };\n${ resolve: _resolve }";
        let tree = parse_tree(module);
        let span = module_member_def(&tree, "resolve").expect("member def");
        // Jumps to the `_resolve` declaration, not the export field.
        assert_eq!(&module[span.start..span.end], "_resolve");
        assert_eq!(span.start, module.find("_resolve :=").unwrap());
    }

    #[test]
    fn module_member_def_handles_inline_and_missing_members() {
        let module = "${ answer: 42, f: fn(x) { x } }";
        let tree = parse_tree(module);
        // An inline (non-ident) value: jump to the value expression.
        let span = module_member_def(&tree, "answer").expect("inline member");
        assert_eq!(&module[span.start..span.end], "42");
        // An unknown member resolves to nothing.
        assert!(module_member_def(&tree, "nope").is_none());
    }

    #[test]
    fn module_member_def_falls_back_to_a_top_level_decl() {
        // No export object — the module's value is the decl itself.
        let module = "greet := fn() { 'hi' }";
        let tree = parse_tree(module);
        let span = module_member_def(&tree, "greet").expect("top-level decl");
        assert_eq!(&module[span.start..span.end], "greet");
    }

    #[test]
    fn module_exports_lists_members_with_signatures() {
        let module = "_resolve := fn(name, qtype = 1) { name };\n\
                      ${ resolve: _resolve, answer: 42 }";
        let tree = parse_tree(module);
        let exports = module_exports(&tree);
        let resolve = exports.iter().find(|e| e.name == "resolve").unwrap();
        assert!(resolve.is_function);
        assert_eq!(resolve.signature, "resolve(name, qtype?)");
        // Re-export jumps to the private decl, not the export field.
        assert_eq!(&module[resolve.def_span.start..resolve.def_span.end], "_resolve");
        let answer = exports.iter().find(|e| e.name == "answer").unwrap();
        assert!(!answer.is_function);
        assert_eq!(answer.signature, "answer");
    }

    #[test]
    fn member_access_spans_find_every_use() {
        let src = "DNS.resolve('a');\nx := DNS.resolve('b');\nDNS.other();";
        let spans = member_access_spans(&parse_tree(src), "DNS", "resolve");
        assert_eq!(spans.len(), 2);
        assert!(spans.iter().all(|s| &src[s.start..s.end] == "resolve"));
    }

    #[test]
    fn import_alias_pairs_lists_all_imports() {
        let src = "A := import './a';\nB := import 'Math';";
        let pairs = import_alias_pairs(&parse_tree(src));
        assert!(pairs.contains(&("A".to_string(), "./a".to_string())));
        assert!(pairs.contains(&("B".to_string(), "Math".to_string())));
    }

    #[test]
    fn document_symbols_categorise_top_level_decls() {
        let src = "Math := import 'Math';\nx := 41;\nadd := fn(a, b) { a + b };";
        let syms = document_symbols(&parse_tree(src));
        let by_name = |n: &str| syms.iter().find(|s| s.name == n).unwrap();

        let m = by_name("Math");
        assert!(m.category == SymbolCategory::Module);
        assert_eq!(m.detail.as_deref(), Some("import 'Math'"));

        let x = by_name("x");
        assert!(x.category == SymbolCategory::Variable);

        let add = by_name("add");
        assert!(add.category == SymbolCategory::Function);
        assert_eq!(add.detail.as_deref(), Some("add(a, b)"));
        // The selection range covers just the name, not the whole decl.
        assert_eq!(span_len(add.selection), "add".len());
    }

    #[test]
    fn document_symbols_nest_locals_under_a_function() {
        let src = "outer := fn(p) { helper := fn(q) { q }; p };";
        let syms = document_symbols(&parse_tree(src));
        assert_eq!(syms.len(), 1);
        let outer = &syms[0];
        let helper = outer.children.iter().find(|s| s.name == "helper").unwrap();
        assert!(helper.category == SymbolCategory::Function);
        assert_eq!(helper.detail.as_deref(), Some("helper(q)"));
    }

    /// The substrings `src[span]` for each reference span at the cursor
    /// just inside the first occurrence of `needle`, plus the count.
    fn refs_at(src: &str, needle: &str) -> Vec<(usize, String)> {
        let off = at(src, needle);
        references(&parse_tree(src), off)
            .into_iter()
            .map(|s| (s.start, src[s.start..s.end].to_string()))
            .collect()
    }

    #[test]
    fn references_find_decl_and_all_uses() {
        let src = "x := 1;\ny := x + x;\nx = 3;";
        let refs = refs_at(src, "x :=");
        // decl + two reads + the `x = 3` write = 4 occurrences.
        assert_eq!(refs.len(), 4, "got {refs:?}");
        assert!(refs.iter().all(|(_, t)| t == "x"));
    }

    #[test]
    fn references_work_from_a_use_site_and_respect_shadowing() {
        // Two distinct `n`: the param and the outer decl. A reference
        // inside the function must group only with the param.
        let src = "n := 99;\nf := fn(n) { n + n };\nn + 1;";
        // Cursor on the first `n` inside the body.
        let body_n = src.find("n + n").unwrap();
        let refs: Vec<Span> = references(&parse_tree(src), body_n);
        // param decl + two body uses = 3; the outer `n` decl/use excluded.
        assert_eq!(refs.len(), 3, "got {refs:?}");
        // None of them is the outer decl at offset 0.
        assert!(refs.iter().all(|s| s.start >= src.find("fn(n)").unwrap()));
    }

    #[test]
    fn references_cover_loop_variables() {
        let src = "for (i, 0..3) { print(i); }";
        let refs = refs_at(src, "i, 0");
        assert_eq!(refs.len(), 2, "loop var decl + use, got {refs:?}");
    }

    #[test]
    fn no_references_for_a_builtin() {
        let src = "print(1);";
        assert!(references(&parse_tree(src), at(src, "print")).is_empty());
    }

    #[test]
    fn rename_collects_every_occurrence_including_the_decl() {
        let src = "count := 0;\ncount = count + 1;";
        let off = at(src, "count :=");
        let target = rename_spans(&parse_tree(src), off).expect("renameable");
        assert_eq!(target.spans.len(), 3, "got {:?}", target.spans);
        // The declaration is the first occurrence.
        assert_eq!(target.def.start, 0);
        assert_eq!(target.spans[0].start, 0);
    }

    #[test]
    fn rename_declines_builtins() {
        // A builtin cannot be renamed.
        let b = "print(1);";
        assert!(rename_spans(&parse_tree(b), at(b, "print")).is_none());
    }

    #[test]
    fn rename_renames_match_bindings() {
        // A match-arm binding now carries a span, so its declaration and
        // every use in the arm body rename together.
        let m = "match x { y => y + 1 };";
        let tree = parse_tree(m);
        // From the body reference...
        let body_y = m.rfind('y').unwrap();
        let from_use = rename_spans(&tree, body_y).expect("renameable from use");
        assert_eq!(from_use.spans.len(), 2, "got {:?}", from_use.spans);
        // ...and from the binding site, the same set.
        let bind_y = m.find('y').unwrap();
        let from_def = rename_spans(&tree, bind_y).expect("renameable from def");
        assert_eq!(from_def.def.start, bind_y);
        assert_eq!(from_def.spans.len(), 2, "got {:?}", from_def.spans);
    }

    #[test]
    fn references_find_object_shorthand_match_binding() {
        // `${name}` shorthand binds `name` at the key's span.
        let m = "match p { ${x} => x + 1 };";
        let tree = parse_tree(m);
        let refs = references(&tree, m.rfind('x').unwrap());
        assert_eq!(refs.len(), 2, "got {refs:?}");
    }

    #[test]
    fn document_symbols_expand_destructuring_decls() {
        let src = "[a, b] := [1, 2];";
        let syms = document_symbols(&parse_tree(src));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a") && names.contains(&"b"), "got {names:?}");
    }
}
