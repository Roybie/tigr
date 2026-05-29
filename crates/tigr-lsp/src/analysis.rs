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

#[derive(Clone, Copy, PartialEq)]
pub enum BindingKind {
    Decl,
    Param,
    LoopVar,
    CatchParam,
    MatchBinding,
}

impl BindingKind {
    fn describe(self) -> &'static str {
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
}

#[derive(Default)]
struct Scope {
    bindings: HashMap<String, Binding>,
}

/// Definition span to jump to for the identifier at `offset`, if any.
pub fn definition(program: &Block, offset: usize) -> Option<Span> {
    resolve(program, offset)?.1?.def
}

/// Markdown hover text for the identifier at `offset`, if it resolves to
/// a known binding.
pub fn hover(program: &Block, offset: usize) -> Option<String> {
    let (name, binding) = resolve(program, offset)?;
    let binding = binding?;
    let mut out = format!("**`{name}`** — {}", binding.kind.describe());
    if let Some(def) = binding.def {
        out.push_str(&format!("\n\n*defined on line {}*", def.line));
    }
    Some(out)
}

/// Find the reference at `offset` and resolve it. Returns the name plus
/// its binding (the inner `Option` is `None` when the name resolves to
/// no local binding — likely a global, builtin, or stdlib module).
fn resolve(program: &Block, offset: usize) -> Option<(String, Option<Binding>)> {
    let (name, _span) = ident_at(program, offset)?;
    let mut scopes = Vec::new();
    let mut root = Scope::default();
    hoist_block(program, &mut root);
    scopes.push(root);
    descend_block(program, offset, &mut scopes);
    let binding = scopes.iter().rev().find_map(|s| s.bindings.get(&name).cloned());
    Some((name, binding))
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
                    .map_or(true, |(_, s)| span_len(se.span) <= span_len(*s));
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
            scope.bindings.insert(name.clone(), Binding { def, kind });
        }
        Pattern::Array { items, rest } => {
            for it in items {
                add_pattern(it, None, kind, scope);
            }
            if let Some(r) = rest {
                scope.bindings.insert(r.clone(), Binding { def: None, kind });
            }
        }
        Pattern::Object { fields, rest } => {
            for fld in fields {
                add_pattern(&fld.pattern, None, kind, scope);
            }
            if let Some(r) = rest {
                scope.bindings.insert(r.clone(), Binding { def: None, kind });
            }
        }
    }
}

fn bind_name(scope: &mut Scope, name: &str, kind: BindingKind) {
    scope.bindings.insert(name.to_string(), Binding { def: None, kind });
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
