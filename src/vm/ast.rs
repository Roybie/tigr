//! Tigr v0.2 AST.
//!
//! Phase 1 covers literals, identifiers, binary/unary arithmetic,
//! blocks, declarations (`x := expr`) and assignments (`x = expr`).
//! Later phases extend `Expr` with additional variants; this enum will
//! grow but existing variants stay stable.

use crate::vm::token::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct SpannedExpr {
    pub expr: Expr,
    pub span: Span,
}

impl SpannedExpr {
    pub fn new(expr: Expr, span: Span) -> Self {
        SpannedExpr { expr, span }
    }
}

/// Convert an expression to a binding pattern, if possible.
///
/// The spec's "literal-vs-pattern" rule for `${...}` works by parsing
/// the LHS of `:=` as an ordinary expression first, then converting on
/// demand. This means a malformed pattern (e.g. `[1, 2] := …`)
/// produces a clear error rather than a parse-time mystery.
///
/// Mapping rules (spec §11):
/// - `Ident("_")` → `Wildcard`
/// - `Ident(name)` → `Ident(name)`
/// - `Array([items])` → `Array` pattern; a trailing `Spread(Ident(r))`
///   becomes the rest.
/// - `Object([members])` → `Object` pattern. `Pair(k, v)` becomes a
///   field with `v` recursively converted. A trailing
///   `Spread(Ident(r))` becomes the rest.
pub fn expr_to_pattern(e: &SpannedExpr) -> Result<Pattern, PatternError> {
    match &e.expr {
        Expr::Ident(name) if name == "_" => Ok(Pattern::Wildcard),
        Expr::Ident(name) => Ok(Pattern::Ident(name.clone())),
        Expr::Array(items) => {
            let mut out_items = Vec::with_capacity(items.len());
            let mut rest = None;
            for (i, item) in items.iter().enumerate() {
                if let Expr::Spread(inner) = &item.expr {
                    if i != items.len() - 1 {
                        return Err(PatternError::RestNotLast(item.span));
                    }
                    match &inner.expr {
                        Expr::Ident(n) => rest = Some(n.clone()),
                        _ => return Err(PatternError::RestNotIdent(inner.span)),
                    }
                } else {
                    out_items.push(expr_to_pattern(item)?);
                }
            }
            Ok(Pattern::Array { items: out_items, rest })
        }
        Expr::Object(members) => {
            let mut fields = Vec::with_capacity(members.len());
            let mut rest = None;
            for (i, m) in members.iter().enumerate() {
                match m {
                    ObjectMember::Pair(key, value) => {
                        fields.push(ObjectField {
                            key: key.clone(),
                            pattern: expr_to_pattern(value)?,
                        });
                    }
                    ObjectMember::Spread(inner) => {
                        if i != members.len() - 1 {
                            return Err(PatternError::RestNotLast(inner.span));
                        }
                        match &inner.expr {
                            Expr::Ident(n) => rest = Some(n.clone()),
                            _ => return Err(PatternError::RestNotIdent(inner.span)),
                        }
                    }
                }
            }
            Ok(Pattern::Object { fields, rest })
        }
        _ => Err(PatternError::NotPatternable(e.span)),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PatternError {
    NotPatternable(Span),
    RestNotLast(Span),
    RestNotIdent(Span),
}

impl PatternError {
    pub fn span(&self) -> Span {
        match *self {
            PatternError::NotPatternable(s)
            | PatternError::RestNotLast(s)
            | PatternError::RestNotIdent(s) => s,
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            PatternError::NotPatternable(_) => "expression cannot be used as a binding pattern",
            PatternError::RestNotLast(_) => "rest element `...` must be last",
            PatternError::RestNotIdent(_) => "rest element must name a binding (`...name`)",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<SpannedExpr>,
    /// `None` if the block ends with `;` (or is empty); the block's
    /// value is `null` in that case.
    pub tail: Option<Box<SpannedExpr>>,
}

/// Parsed form of one piece of an interpolated string. Mirrors the
/// lexer's `TemplatePart` but with a fully-parsed expression instead
/// of raw source.
#[derive(Clone, Debug, PartialEq)]
pub enum TemplatePart {
    Lit(String),
    Expr(SpannedExpr),
}

/// Binding pattern (spec §11). Used on LHS of `:=` / `=` and at
/// function parameter positions.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    /// `_` — discard the value at this slot.
    Wildcard,
    /// A bare name. Binds the value to `name`.
    Ident(String),
    /// `[p1, p2, ..., ...rest?]` — array destructuring. Missing
    /// elements bind to `null`.
    Array { items: Vec<Pattern>, rest: Option<String> },
    /// `${k1, k2: alias, ..., ...rest?}` — object destructuring.
    /// Missing keys bind to `null`. `rest` collects all unconsumed
    /// keys into a new object (insertion order preserved).
    Object { fields: Vec<ObjectField>, rest: Option<String> },
}

impl Pattern {
    /// Collect every binding name introduced by this pattern, in the
    /// source-textual order. Used by the compiler's hoisting pre-walk
    /// to pre-allocate slots for mid-expression `:=` decls.
    pub fn leaf_names(&self, out: &mut Vec<String>) {
        match self {
            Pattern::Wildcard => {}
            Pattern::Ident(name) => out.push(name.clone()),
            Pattern::Array { items, rest } => {
                for it in items { it.leaf_names(out); }
                if let Some(r) = rest { out.push(r.clone()); }
            }
            Pattern::Object { fields, rest } => {
                for f in fields { f.pattern.leaf_names(out); }
                if let Some(r) = rest { out.push(r.clone()); }
            }
        }
    }
}

/// One field of an object pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectField {
    /// Key in the source object to extract.
    pub key: String,
    /// Sub-pattern to bind the value to. For shorthand `${name}` this
    /// is `Pattern::Ident("name")`. For rename `${name: alias}` this
    /// is `Pattern::Ident("alias")`. For nested `${k: [a, b]}` it's
    /// the inner array pattern.
    pub pattern: Pattern,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectMember {
    /// `key: value` in an object literal. Keys are statically known
    /// strings (identifier or quoted-string syntax handled by parser).
    Pair(String, SpannedExpr),
    /// `...expr` — must evaluate to an Object at runtime; its entries
    /// are merged into the literal in insertion order (later keys
    /// win, per spec §6.6).
    Spread(SpannedExpr),
}

// ---------------- match patterns (v0.5) ----------------

/// A literal value usable in a `match` pattern. Kept separate from the
/// full `Expr` so pattern-land never drags in arbitrary expressions.
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralPat {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
}

/// A refutable pattern in a `match` arm (v0.5, spec §match). Distinct
/// from `Pattern` (which is irrefutable: it always binds, with missing
/// values → `null`). A `MatchPattern` can *fail* to match, in which
/// case the next arm is tried.
#[derive(Clone, Debug, PartialEq)]
pub enum MatchPattern {
    /// Matches if the subject `==` this literal.
    Literal(LiteralPat),
    /// Bare name — matches anything, binds the subject to `name`.
    Binding(String),
    /// `_` — matches anything, binds nothing.
    Wildcard,
    /// `[p1, p2, ...rest?]` — subject must be an Array of the right
    /// length (exact, or `>= items.len()` when `rest` is present).
    Array { items: Vec<MatchPattern>, rest: Option<String> },
    /// `${k: subpat, shorthand, ...rest?}` — subject must be an Object.
    Object { fields: Vec<MatchField>, rest: Option<String> },
    /// `from..to` / `from..=to` — subject must be a number in range.
    Range { from: LiteralPat, to: LiteralPat, inclusive: bool },
    /// `p1 | p2 | ...` — matches if any alternative matches. By v0.5
    /// rule, alternatives may not bind variables.
    Or(Vec<MatchPattern>),
}

/// One field of a `match` object pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchField {
    pub key: String,
    /// `None` for shorthand `${name}` (binds the value to `name`);
    /// `Some(p)` for `${key: p}`.
    pub pattern: Option<MatchPattern>,
}

/// One arm of a `match` expression.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub guard: Option<SpannedExpr>,
    pub body: SpannedExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,

    // Variable reference
    Ident(String),

    // Operators
    BinOp(BinOp, Box<SpannedExpr>, Box<SpannedExpr>),
    UnOp(UnOp, Box<SpannedExpr>),

    // Block of `;`-separated expressions. Used for the top-level
    // program and for parenthesised blocks `(a; b; c)`.
    Block(Block),

    // `pattern := expr` — declares binding(s) in the current scope.
    // For the common bare-identifier case the pattern is just
    // `Pattern::Ident(name)`; structural patterns destructure the
    // initialiser per spec §11.
    Decl(Pattern, Box<SpannedExpr>),

    // `x = expr` (op = None) or `x op= expr` (op = Some(BinOp)).
    // Assigns to an existing binding (error if absent).
    Assign(String, Option<BinOp>, Box<SpannedExpr>),

    // `[a, b] = rhs` / `${k1, k2: alias} = rhs` — destructure rhs into
    // EXISTING bindings. Spec §11 says patterns work on both `:=` and
    // `=`; this variant covers the `=` form. Compound ops like `+=`
    // are NOT permitted with patterns (per spec §11.4) so there's no
    // op slot here. Each leaf must already be declared, otherwise it
    // raises `UndeclaredAssign` at compile time.
    AssignPattern(Pattern, Box<SpannedExpr>),

    // `{ a; b; c }` — opens a new lexical scope.
    Scope(Block),

    // `if cond then else` — `else` defaults to a `Null` literal when
    // the source has no `else` branch.
    If(Box<SpannedExpr>, Box<SpannedExpr>, Box<SpannedExpr>),

    // `while cond body` (is_array = false) or `while[] cond body`
    // (is_array = true). The array form collects each iteration's
    // body value (nulls filtered) per spec §9.2.
    While {
        is_array: bool,
        cond: Box<SpannedExpr>,
        body: Box<SpannedExpr>,
    },

    // `for (var, iter) body` / `for (var1, var2, iter) body`, plus the
    // `for[]` array-collecting form per spec §7.4 / §9.3.
    For {
        is_array: bool,
        vars: Vec<String>,
        iter: Box<SpannedExpr>,
        body: Box<SpannedExpr>,
    },

    // `from..to` / `from..=to` / `from..to:step`. Per spec §7.3 — first
    // class. `step` defaults to ±1 at runtime depending on from/to.
    Range {
        from: Box<SpannedExpr>,
        to: Box<SpannedExpr>,
        step: Option<Box<SpannedExpr>>,
        inclusive: bool,
    },

    // `break` (None) or `break value` / `break (expr)` (Some). Per
    // spec §9.4, break is an expression whose "value" can be chained
    // into another break.
    Break(Option<Box<SpannedExpr>>),

    // `continue` — skip the rest of the current loop iteration. The
    // iteration contributes `null` (nothing appended in `for[]`/
    // `while[]`). Carries no value.
    Continue,

    // `[a, b, c]` — array literal. Items may be `Expr::Spread(...)`.
    Array(Vec<SpannedExpr>),

    // `${ k: v, k2: v2, ...other }` — object literal. Members are
    // pairs or spreads (later keys win, spec §6.6).
    Object(Vec<ObjectMember>),

    // `...inner` — only legal as a direct child of array literals,
    // call arguments, or as an object member. Parser rejects it
    // elsewhere; compiler only handles it in those contexts.
    Spread(Box<SpannedExpr>),

    // Interpolated string literal (spec §8.2). Compiles to a sequence
    // of `LoadConst`s and expr emissions joined by `ConcatN`. Each
    // expression part is `str`-coerced before joining.
    Template(Vec<TemplatePart>),

    // `obj[key]` — also produced by `obj.key` (with key = Str literal).
    Index(Box<SpannedExpr>, Box<SpannedExpr>),

    // `obj[key] = value` (op = None) or `obj[key] op= value`.
    IndexAssign(
        Box<SpannedExpr>,
        Box<SpannedExpr>,
        Option<BinOp>,
        Box<SpannedExpr>,
    ),

    // `callee(arg, arg, ...)`.
    Call(Box<SpannedExpr>, Vec<SpannedExpr>),

    // `fn(p1, p2, ...rest) { body }`. Each param is a `Pattern` so
    // call sites can pass `fn([a, b], ${name}) { ... }`. `rest` is
    // the optional final `...name` parameter (spec §10.3).
    //
    // `defaults` runs parallel to `params`: `defaults[i]` is the
    // `Some(expr)` default for `params[i]`, used when that argument
    // slot is `null`. Defaults are only permitted on `Pattern::Ident`
    // parameters (the parser enforces this).
    Fn {
        params: Vec<Pattern>,
        defaults: Vec<Option<Box<SpannedExpr>>>,
        rest: Option<String>,
        body: Box<SpannedExpr>,
    },

    // `return` (None) or `return value` / `return (expr)` (Some).
    Return(Option<Box<SpannedExpr>>),

    // `import 'path'` (spec §12). Path is a string literal; compile
    // resolves it relative to the importing file. Disabled inside
    // top-level expressions other than the immediate decl initialiser,
    // though we don't strictly enforce that.
    Import(String),

    // `try expr` (catch = None) or `try expr catch (param) { handler }`
    // (catch = Some). The try expression evaluates to `body`'s value on
    // success or — on a raised/runtime error caught here — the
    // handler's value (or `null` if catch is omitted). The handler is
    // always a `Scope` per grammar.
    Try {
        body: Box<SpannedExpr>,
        catch: Option<(String, Box<SpannedExpr>)>,
    },

    // `raise expr` — raises a string error (the value is coerced via
    // `str`). Like `break`/`return`, the runtime never reaches consumers
    // of this expression.
    Raise(Box<SpannedExpr>),

    // `match subject { pat => body, pat if guard => body, ... }`
    // (v0.5). Arms are tried top-to-bottom; the value is the body of
    // the first matching arm, or `null` if no arm matches.
    Match {
        subject: Box<SpannedExpr>,
        arms: Vec<MatchArm>,
    },
}

#[allow(dead_code)] // Eq..Or land in Phase 2
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    // Comparison (Phase 2+)
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical (Phase 2+)
    And,
    Or,
    // Bitwise (v0.5) — Int-only
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[allow(dead_code)] // Len lands in Phase 3
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    Len,
    BitNot,
}
