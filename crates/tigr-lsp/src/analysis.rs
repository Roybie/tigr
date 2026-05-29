//! Lexical analysis over the tigr AST for go-to-definition and hover.
//!
//! tigr stores binding *names* as plain strings inside `Pattern` /
//! `Assign` / params — only `Expr::Ident` carries a span, and it only
//! ever appears as a *reference*. So: find the `Ident` under the cursor,
//! rebuild the scope chain active at that point, and resolve the name
//! innermost-first.
//!
//! Definition spans exist only for the common `name := …` form, whose
//! decl span starts exactly at the name. Destructured names, params, and
//! loop variables have no span in the AST, so they are still resolved
//! (for correct shadowing) but report no jump target — better than
//! jumping to the wrong outer binding.

use std::collections::HashMap;

use tigr::vm::ast::{Block, Expr, ObjectMember, Pattern, SpannedExpr, TemplatePart};
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
                    map.insert(alias.clone(), name.clone());
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
fn member_at(program: &Block, offset: usize) -> Option<(String, String)> {
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
        Pattern::Ident(n) => n.clone(),
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
            let mut names = Vec::new();
            pattern_names(pat, &mut names);
            for name in names {
                out.push(SymbolNode {
                    name,
                    detail: None,
                    category: SymbolCategory::Variable,
                    range: decl_span,
                    selection: decl_span,
                    children: Vec::new(),
                });
            }
        }
    }
}

/// A `name := …` declaration. A `fn` initialiser becomes a Function (with
/// its signature and nested locals); an `import` becomes a Module; anything
/// else a Variable.
fn ident_decl_symbol(name: &str, decl_span: Span, init: &SpannedExpr) -> SymbolNode {
    let selection = Span::new(decl_span.start, decl_span.start + name.len(), decl_span.line);
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

/// Every name bound by a (possibly nested) destructuring pattern.
fn pattern_names(pat: &Pattern, out: &mut Vec<String>) {
    match pat {
        Pattern::Wildcard => {}
        Pattern::Ident(name) => out.push(name.clone()),
        Pattern::Array { items, rest } => {
            for it in items {
                pattern_names(it, out);
            }
            if let Some(r) = rest {
                out.push(r.clone());
            }
        }
        Pattern::Object { fields, rest } => {
            for fld in fields {
                pattern_names(&fld.pattern, out);
            }
            if let Some(r) = rest {
                out.push(r.clone());
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
            add_pattern(pat, Some(se.span), BindingKind::Decl, scope);
            // `name := fn(...)` — remember the signature for hover.
            if let (Pattern::Ident(name), Expr::Fn { params, defaults, rest, .. }) =
                (pat, &init.expr)
            {
                if let Some(b) = scope.bindings.get_mut(name) {
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

/// Insert the names a pattern binds. A precise span is recorded only for
/// a bare top-level `Ident` (the decl span starts at the name); every
/// other leaf is in scope but unlocatable.
fn add_pattern(pat: &Pattern, decl_span: Option<Span>, kind: BindingKind, scope: &mut Scope) {
    match pat {
        Pattern::Wildcard => {}
        Pattern::Ident(name) => {
            let def = decl_span
                .map(|s| Span::new(s.start, s.start + name.len(), s.line));
            scope.bindings.insert(name.clone(), Binding::new(def, kind));
        }
        Pattern::Array { items, rest } => {
            for it in items {
                add_pattern(it, None, kind, scope);
            }
            if let Some(r) = rest {
                scope.bindings.insert(r.clone(), Binding::new(None, kind));
            }
        }
        Pattern::Object { fields, rest } => {
            for fld in fields {
                add_pattern(&fld.pattern, None, kind, scope);
            }
            if let Some(r) = rest {
                scope.bindings.insert(r.clone(), Binding::new(None, kind));
            }
        }
    }
}

fn bind_name(scope: &mut Scope, name: &str, kind: BindingKind) {
    scope.bindings.insert(name.to_string(), Binding::new(None, kind));
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
                add_pattern(p, None, BindingKind::Param, &mut s);
            }
            if let Some(r) = rest {
                bind_name(&mut s, r, BindingKind::Param);
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
                    bind_name(&mut s, v, BindingKind::LoopVar);
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
                    bind_name(&mut s, param, BindingKind::CatchParam);
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
        M::Binding(name) => bind_name(scope, name, BindingKind::MatchBinding),
        M::Array { items, rest } => {
            for it in items {
                match_bindings(it, scope);
            }
            if let Some(r) = rest {
                bind_name(scope, r, BindingKind::MatchBinding);
            }
        }
        M::Object { fields, rest } => {
            for fld in fields {
                match &fld.pattern {
                    Some(p) => match_bindings(p, scope),
                    // Shorthand `${name}` binds the key name itself.
                    None => bind_name(scope, &fld.key, BindingKind::MatchBinding),
                }
            }
            if let Some(r) = rest {
                bind_name(scope, r, BindingKind::MatchBinding);
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

    #[test]
    fn document_symbols_expand_destructuring_decls() {
        let src = "[a, b] := [1, 2];";
        let syms = document_symbols(&parse_tree(src));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a") && names.contains(&"b"), "got {names:?}");
    }
}
