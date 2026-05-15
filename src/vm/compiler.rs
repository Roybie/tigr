//! Compiles the AST to bytecode.
//!
//! The compiler manages a *stack* of [`FuncCompiler`]s — the top is
//! whatever function (or the top-level "main") we're currently writing
//! bytecode for. Encountering a `fn (…) { … }` literal pushes a new
//! `FuncCompiler`, compiles its body into its own [`Chunk`], pops, and
//! emits a `Closure` opcode in the now-current chunk that wraps the
//! template in a runtime [`Closure`].
//!
//! ## Slot layout
//!
//! - Slot 0 of every function frame is reserved for the closure value
//!   itself (occupies one local slot in the compiler so subsequent
//!   names don't collide with it). Phase 4 doesn't use it directly,
//!   but it gives us a uniform Lox-style frame.
//! - Slots 1..=arity hold parameters.
//! - Body locals start above that.
//!
//! ## Identifier resolution (the heart of upvalue capture)
//!
//! [`Compiler::resolve`] walks newest-first through:
//!   1. Locals of the current function.
//!   2. Locals/upvalues of enclosing functions, recursively. A hit at
//!      any enclosing level becomes an upvalue in every intermediate
//!      function (with `is_local = true` only at the level where the
//!      original local lives).
//!   3. Globals (the built-in stdlib).
//!
//! ## Recursion
//!
//! `f := fn() { f() }` works because `f` is *declared* (slot reserved,
//! marked undefined) before its initializer is compiled, and the
//! upvalue-capture path doesn't enforce the "defined" check — by the
//! time the closure is *called*, the outer init has finished.

use std::collections::HashMap;
use std::rc::Rc;

use crate::vm::ast::{
    BinOp, Block, Expr, ObjectMember, Pattern, SpannedExpr, TemplatePart, UnOp,
};
use std::path::PathBuf;
use crate::vm::chunk::Chunk;
use crate::vm::error::{CompileError, CompileErrorKind};
use crate::vm::opcode::OpCode;
use crate::vm::stdlib;
use crate::vm::token::Span;
use crate::vm::value::{Function, UpvalueInfo, Value};

struct Local {
    name: String,
    depth: u32,
    /// Absolute stack slot from the frame base. With expression-position
    /// declarations, this is NOT necessarily equal to the local's index
    /// in `locals[]` — transients above earlier locals (e.g., the LHS
    /// of a binop whose RHS is `while …`) push the actual slot upward.
    slot: u8,
    is_captured: bool,
}

/// Per-loop bookkeeping. Pushed onto [`FuncCompiler::loop_stack`] when
/// compiling a `for` / `while`, popped when the loop completes. Carries:
///
/// - `result_slot` — local slot holding the loop's accumulating value
///   (null for `for`/`while`; empty array for the array-collecting
///   forms). `break v` writes into this slot.
/// - `base_stack_height` — stack height at loop entry (after the
///   result/iter locals were declared). `break` emits
///   `Unwind base_stack_height` to truncate the stack back to this
///   point regardless of how deeply nested the break is.
/// - `is_array_form` — true for `for[]` / `while[]`; switches break
///   between "store into slot" and "append to array".
/// - `exit_jumps` — forward jumps emitted by `break` that need
///   back-patching to the loop's exit point.
/// - `skip` — temporarily set to `true` while compiling the *value*
///   expression of a `break` targeting this loop, so that a nested
///   `break` inside the value redirects to the next enclosing loop
///   (spec §9.4 chained-break semantics).
struct LoopCtx {
    result_slot: u8,
    base_stack_height: u32,
    is_array_form: bool,
    exit_jumps: Vec<usize>,
    skip: bool,
}

struct FuncCompiler {
    chunk: Chunk,
    locals: Vec<Local>,
    upvalues: Vec<UpvalueInfo>,
    scope_depth: u32,
    arity: usize,
    name: Option<String>,
    loop_stack: Vec<LoopCtx>,
    /// Tracks the actual VM stack height (from frame base), counting
    /// both declared locals AND transient mid-expression values. The
    /// slot for a `declare_local` is computed from this, NOT from
    /// `locals.len()`, so that declarations in expression position
    /// (e.g. `while … { x := y }` used as the RHS of `==`) land on the
    /// real stack slot rather than colliding with a transient.
    stack_height: u32,
    /// Stack of per-scope maps tracking names that were *hoisted* — i.e.,
    /// pre-declared at scope entry because a nested `:=` for them
    /// appears inside one of the scope's expressions. A nested
    /// `(x := 5)` would otherwise land at a stack slot above existing
    /// transients, which any surrounding op (binop, call, …) would
    /// pop. Hoisting puts `x`'s slot in the contiguous-locals region
    /// at scope entry; the nested `:=` becomes a `StoreLocal` to that
    /// stable slot.
    hoisted_scopes: Vec<HashMap<String, u8>>,
}

impl FuncCompiler {
    fn new(arity: usize, name: Option<String>) -> Self {
        FuncCompiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            arity,
            name,
            loop_stack: Vec::new(),
            stack_height: 0,
            hoisted_scopes: vec![HashMap::new()],
        }
    }
}

#[derive(Clone, Copy)]
enum Resolved {
    Local(u8),
    Upvalue(u8),
    Global(u8),
}

pub struct Compiler {
    funcs: Vec<FuncCompiler>,
    globals: Vec<&'static str>,
    /// Directory of the source file being compiled, if known. Used to
    /// resolve `import 'path'` paths at compile time (spec §12).
    /// `None` for source compiled from a string (no path context).
    base_dir: Option<PathBuf>,
}

impl Compiler {
    /// Compile a top-level program into the "main" function. The
    /// source is treated as having no file context (relative imports
    /// will error). Use [`compile_with_dir`] when the source comes
    /// from a known file.
    #[allow(dead_code)]
    pub fn compile(program: &Block) -> Result<Function, CompileError> {
        Self::compile_with_dir(program, None)
    }

    /// Compile with a base directory for relative-path import
    /// resolution.
    pub fn compile_with_dir(
        program: &Block,
        base_dir: Option<PathBuf>,
    ) -> Result<Function, CompileError> {
        let mut c = Compiler {
            funcs: Vec::new(),
            globals: stdlib::names().to_vec(),
            base_dir,
        };
        c.push_function(0, Some("<main>".to_string()));
        // slot 0 = main closure placeholder. The frame is set up so
        // that the VM has placed *something* at base_slot before we
        // start running — bump the tracker by 1 to match.
        c.current_mut().stack_height = 1;
        c.declare_local("", Span::new(0, 0, 1))?;

        // Hoist nested `:=` declarations at the top-level scope (the
        // implicit `<main>` body). Top-level Decls keep their existing
        // declare-after-init semantics; only nested ones get a stable
        // slot here.
        let mut hoisted = Vec::new();
        c.visit_block_for_hoist(program, &mut hoisted);
        c.emit_hoist_prologue(hoisted, Span::new(0, 0, 1))?;

        c.compile_block_value(program)?;
        let last_line = c.current_chunk().lines.last().copied().unwrap_or(1);
        c.current_chunk_mut().write_op(OpCode::Return, last_line);

        let fc = c.funcs.pop().expect("main function compiler popped");
        Ok(Function {
            arity: 0,
            has_rest: false,
            chunk: fc.chunk,
            upvalues: fc.upvalues,
            name: fc.name,
        })
    }

    // -- function compiler stack helpers ------------------------------

    fn current(&self) -> &FuncCompiler {
        self.funcs.last().expect("at least one func compiler")
    }
    fn current_mut(&mut self) -> &mut FuncCompiler {
        self.funcs.last_mut().expect("at least one func compiler")
    }
    fn current_chunk(&self) -> &Chunk {
        &self.current().chunk
    }
    fn current_chunk_mut(&mut self) -> &mut Chunk {
        &mut self.current_mut().chunk
    }

    fn push_function(&mut self, arity: usize, name: Option<String>) {
        self.funcs.push(FuncCompiler::new(arity, name));
    }

    // -- block / scope -----------------------------------------------

    fn compile_block_value(&mut self, block: &Block) -> Result<(), CompileError> {
        for stmt in &block.stmts {
            self.compile_expr(stmt)?;
            self.emit_op(OpCode::Pop, stmt.span.line);
        }
        if let Some(tail) = &block.tail {
            self.compile_expr(tail)?;
        } else {
            let line = block.stmts.last().map(|s| s.span.line).unwrap_or(1);
            self.emit_op(OpCode::PushNull, line);
        }
        Ok(())
    }

    fn begin_scope(&mut self) {
        self.current_mut().scope_depth += 1;
        self.current_mut().hoisted_scopes.push(HashMap::new());
    }

    fn end_scope(&mut self, line: u32) -> Result<(), CompileError> {
        let mut count: u8 = 0;
        loop {
            let fc = self.current();
            let Some(local) = fc.locals.last() else { break };
            if local.depth < fc.scope_depth {
                break;
            }
            self.current_mut().locals.pop();
            count = count.checked_add(1).ok_or_else(|| {
                CompileError::new(CompileErrorKind::TooManyLocals, Span::new(0, 0, line))
            })?;
        }
        if count > 0 {
            self.emit_op(OpCode::CloseScope, line);
            self.emit_byte(count, line);
            // CloseScope n: drops `n` slots from below the top.
            self.adjust_stack(-(count as i32));
        }
        self.current_mut().scope_depth -= 1;
        self.current_mut().hoisted_scopes.pop();
        Ok(())
    }

    // -- declaration / resolution ------------------------------------

    /// Declare a new local in the current function's current scope.
    /// `name == ""` denotes an anonymous slot (used for closure
    /// placeholder at slot 0, loop-internals like `$for_iter`, and
    /// pattern-source holders); these are NEVER duplicate-checked
    /// because they can be declared repeatedly within the same scope.
    ///
    /// The slot is taken from `stack_height - 1` — i.e., the value that
    /// must already have been pushed onto the stack becomes the local.
    /// The caller is responsible for pushing the value first.
    fn declare_local(&mut self, name: &str, span: Span) -> Result<(), CompileError> {
        let height = self.current().stack_height;
        if height == 0 {
            return Err(CompileError::new(CompileErrorKind::TooManyLocals, span));
        }
        self.declare_local_at(name, (height - 1) as u8, span)
    }

    /// Like [`declare_local`] but with an explicit slot. Used when a
    /// single opcode pushes multiple values that each become a local
    /// (e.g., `IterNext2` pushes counter+value for 2-var `for`).
    fn declare_local_at(
        &mut self,
        name: &str,
        slot: u8,
        span: Span,
    ) -> Result<(), CompileError> {
        let depth = self.current().scope_depth;
        if !name.is_empty() && !name.starts_with('$')
            && self.current().locals.iter().any(|l| l.name == name && l.depth == depth)
        {
            return Err(CompileError::new(
                CompileErrorKind::DuplicateDeclaration(name.to_string()),
                span,
            ));
        }
        self.current_mut().locals.push(Local {
            name: name.to_string(),
            depth,
            slot,
            is_captured: false,
        });
        Ok(())
    }

    fn resolve(&mut self, name: &str, span: Span) -> Result<Option<Resolved>, CompileError> {
        let last = self.funcs.len() - 1;
        self.resolve_in(last, name, span)
    }

    fn resolve_in(
        &mut self,
        func_idx: usize,
        name: &str,
        span: Span,
    ) -> Result<Option<Resolved>, CompileError> {
        for local in self.funcs[func_idx].locals.iter().rev() {
            if local.name == name {
                return Ok(Some(Resolved::Local(local.slot)));
            }
        }
        if func_idx > 0 {
            if let Some(r) = self.resolve_in(func_idx - 1, name, span)? {
                match r {
                    Resolved::Local(slot) => {
                        // Mark the parent's local by slot (locals[] index
                        // may differ from slot when there were transients
                        // present at declaration time).
                        for l in self.funcs[func_idx - 1].locals.iter_mut() {
                            if l.slot == slot {
                                l.is_captured = true;
                                break;
                            }
                        }
                        let upv = self.add_upvalue(func_idx, slot, true, span)?;
                        return Ok(Some(Resolved::Upvalue(upv)));
                    }
                    Resolved::Upvalue(idx) => {
                        let upv = self.add_upvalue(func_idx, idx, false, span)?;
                        return Ok(Some(Resolved::Upvalue(upv)));
                    }
                    Resolved::Global(idx) => return Ok(Some(Resolved::Global(idx))),
                }
            }
        }
        Ok(self
            .globals
            .iter()
            .position(|n| *n == name)
            .map(|i| Resolved::Global(i as u8)))
    }

    fn add_upvalue(
        &mut self,
        func_idx: usize,
        index: u8,
        is_local: bool,
        span: Span,
    ) -> Result<u8, CompileError> {
        // de-dup
        let existing = self.funcs[func_idx]
            .upvalues
            .iter()
            .position(|u| u.is_local == is_local && u.index == index);
        if let Some(i) = existing {
            return Ok(i as u8);
        }
        if self.funcs[func_idx].upvalues.len() >= 256 {
            return Err(CompileError::new(CompileErrorKind::TooManyUpvalues, span));
        }
        self.funcs[func_idx]
            .upvalues
            .push(UpvalueInfo { is_local, index });
        Ok((self.funcs[func_idx].upvalues.len() - 1) as u8)
    }

    // -- emit helpers -------------------------------------------------

    /// Emit an opcode and apply its fixed stack effect to the
    /// compiler's `stack_height` tracker. Opcodes whose effect depends
    /// on an operand (e.g. `MakeArray n`, `Call n`, `CloseScope n`)
    /// return 0 here — the call site must apply the variable effect
    /// via [`adjust_stack`] after emitting the operand.
    fn emit_op(&mut self, op: OpCode, line: u32) {
        self.current_chunk_mut().write_op(op, line);
        let delta: i32 = match op {
            // +1: push something
            OpCode::LoadConst | OpCode::LoadLocal | OpCode::LoadGlobal
            | OpCode::GetUpvalue | OpCode::PushNull | OpCode::Dup
            | OpCode::Closure => 1,
            // +2
            OpCode::Dup2 => 2,
            // -1: pop one without pushing
            OpCode::Pop => -1,
            // 0: peek / unary in-place / jump / etc.
            OpCode::StoreLocal | OpCode::SetUpvalue
            | OpCode::JumpIfFalse | OpCode::JumpIfTrue
            | OpCode::Jump | OpCode::Loop
            | OpCode::Negate | OpCode::Not | OpCode::Len
            | OpCode::Import | OpCode::MakeIter => 0,
            // -1: pop two, push one (typical binop / index / extend)
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div
            | OpCode::Mod | OpCode::Pow | OpCode::Eq | OpCode::Neq
            | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge
            | OpCode::IndexGet | OpCode::ArrayPush | OpCode::ArrayExtend
            | OpCode::ObjectMerge | OpCode::SliceFrom | OpCode::ObjRest
            | OpCode::IterAppend | OpCode::CallSpread => -1,
            // -2: IndexSet pops collection/key/value, pushes value back
            OpCode::IndexSet => -2,
            // Operand-dependent: caller adjusts.
            OpCode::MakeArray | OpCode::MakeObject | OpCode::Call
            | OpCode::ConcatN | OpCode::MakeRange
            | OpCode::IterNext | OpCode::IterNext2
            | OpCode::CloseScope | OpCode::Unwind
            // Return is a control-transfer; its stack effect on this
            // chunk is "ends the frame". We don't decrement here so
            // that branch-convergence after a returning then-branch
            // stays consistent with the non-returning else-branch.
            | OpCode::Return => 0,
        };
        self.adjust_stack(delta);
    }
    fn emit_byte(&mut self, b: u8, line: u32) {
        self.current_chunk_mut().write_byte(b, line);
    }

    /// Apply a manual stack-height delta. Use after an operand-dependent
    /// opcode (e.g. after `emit_byte(n, ...)` for `MakeArray n`).
    fn adjust_stack(&mut self, delta: i32) {
        let h = self.current().stack_height as i64 + delta as i64;
        debug_assert!(h >= 0, "compiler stack_height went negative");
        self.current_mut().stack_height = h as u32;
    }

    /// Reset the compiler's stack tracker to a known absolute height.
    /// Used at branch-convergence points (after `patch_jump`) and after
    /// `Unwind` opcodes whose runtime effect is "truncate to height N".
    fn set_stack_height(&mut self, h: u32) {
        self.current_mut().stack_height = h;
    }

    fn emit_constant(
        &mut self,
        value: Value,
        line: u32,
        span: Span,
    ) -> Result<(), CompileError> {
        let idx = self
            .current_chunk_mut()
            .add_constant(value)
            .map_err(|_| CompileError::new(CompileErrorKind::TooManyConstants, span))?;
        self.emit_op(OpCode::LoadConst, line);
        self.emit_byte(idx, line);
        Ok(())
    }

    // -- hoist pre-walk ----------------------------------------------

    /// Visit one expression looking for `Pattern::Ident` decls that
    /// need hoisting. A decl is collected when it appears *inside* a
    /// larger expression — its slot would otherwise be clobbered by
    /// the surrounding op. Stops at `Scope`, `Fn`, `While`, `For`
    /// because those introduce their own scopes and do their own
    /// hoisting at entry.
    fn visit_for_hoist(&self, e: &SpannedExpr, out: &mut Vec<String>) {
        match &e.expr {
            Expr::Decl(Pattern::Ident(name), init) => {
                out.push(name.clone());
                self.visit_for_hoist(init, out);
            }
            // Non-Ident patterns are NOT hoisted (a separate, more
            // intricate fix would be needed for nested array/object
            // destructures in expression position). Their init is
            // still visited for any nested Ident decls.
            Expr::Decl(_, init) => self.visit_for_hoist(init, out),

            // Parenthesised blocks `(a; b; c)` don't introduce a scope —
            // they're transparent. Every Decl inside (including a tail
            // Decl) is nested w.r.t. the enclosing scope, so we don't
            // apply the "skip top-level Decl" rule that
            // `visit_block_for_hoist` uses for the outermost block of
            // a scope.
            Expr::Block(b) => {
                for stmt in &b.stmts {
                    self.visit_for_hoist(stmt, out);
                }
                if let Some(t) = &b.tail {
                    self.visit_for_hoist(t, out);
                }
            }

            // Stop boundaries — handle their own hoisting.
            Expr::Scope(_) | Expr::Fn { .. }
            | Expr::While { .. } | Expr::For { .. } => {}

            Expr::BinOp(_, l, r) => {
                self.visit_for_hoist(l, out);
                self.visit_for_hoist(r, out);
            }
            Expr::UnOp(_, inner) => self.visit_for_hoist(inner, out),
            Expr::If(c, t, el) => {
                self.visit_for_hoist(c, out);
                self.visit_for_hoist(t, out);
                self.visit_for_hoist(el, out);
            }
            Expr::Range { from, to, step, .. } => {
                self.visit_for_hoist(from, out);
                self.visit_for_hoist(to, out);
                if let Some(s) = step { self.visit_for_hoist(s, out); }
            }
            Expr::Break(v) | Expr::Return(v) => {
                if let Some(v) = v { self.visit_for_hoist(v, out); }
            }
            Expr::Assign(_, _, v) => self.visit_for_hoist(v, out),
            Expr::Array(items) => for i in items { self.visit_for_hoist(i, out); },
            Expr::Object(members) => {
                for m in members {
                    match m {
                        ObjectMember::Pair(_, v) => self.visit_for_hoist(v, out),
                        ObjectMember::Spread(inner) => self.visit_for_hoist(inner, out),
                    }
                }
            }
            Expr::Spread(inner) => self.visit_for_hoist(inner, out),
            Expr::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Expr(inner) = p {
                        self.visit_for_hoist(inner, out);
                    }
                }
            }
            Expr::Index(o, k) => {
                self.visit_for_hoist(o, out);
                self.visit_for_hoist(k, out);
            }
            Expr::IndexAssign(o, k, _, v) => {
                self.visit_for_hoist(o, out);
                self.visit_for_hoist(k, out);
                self.visit_for_hoist(v, out);
            }
            Expr::Call(callee, args) => {
                self.visit_for_hoist(callee, out);
                for a in args { self.visit_for_hoist(a, out); }
            }
            // Leaves
            Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_)
            | Expr::Null | Expr::Ident(_) | Expr::Import(_) => {}
        }
    }

    /// Walk a block's stmts/tail. The block's *top-level* `Decl`
    /// (a stmt or tail that is itself a Decl(Ident, ...)) is NOT
    /// hoisted — it keeps the declare-after-init semantics so the
    /// variable's scope still starts at its source location. Only
    /// inits and other inner expressions are scanned for nested
    /// hoist candidates.
    fn visit_block_for_hoist(&self, block: &Block, out: &mut Vec<String>) {
        for stmt in &block.stmts {
            match &stmt.expr {
                Expr::Decl(Pattern::Ident(_), init) => self.visit_for_hoist(init, out),
                _ => self.visit_for_hoist(stmt, out),
            }
        }
        if let Some(tail) = &block.tail {
            match &tail.expr {
                Expr::Decl(Pattern::Ident(_), init) => self.visit_for_hoist(init, out),
                _ => self.visit_for_hoist(tail, out),
            }
        }
    }

    /// Emit the prologue for hoisted names: one `PushNull` + declare
    /// per name. Records each name→slot in the current
    /// `hoisted_scopes` entry so subsequent `:=` Decls compile to
    /// `StoreLocal` against this stable slot.
    fn emit_hoist_prologue(
        &mut self,
        names: Vec<String>,
        span: Span,
    ) -> Result<(), CompileError> {
        for name in names {
            // Skip duplicates within the same scope-prologue (the same
            // name can syntactically appear in multiple nested decls;
            // they all share one slot).
            if self
                .current()
                .hoisted_scopes
                .last()
                .map_or(false, |m| m.contains_key(&name))
            {
                continue;
            }
            self.emit_op(OpCode::PushNull, span.line);
            self.declare_local(&name, span)?;
            let slot = self.current().locals.last().unwrap().slot;
            self.current_mut()
                .hoisted_scopes
                .last_mut()
                .unwrap()
                .insert(name, slot);
        }
        Ok(())
    }

    /// Look up the slot a name was hoisted to in the *current* scope.
    /// Returns `None` for un-hoisted names (top-level decls and outer
    /// scopes' locals).
    fn lookup_hoisted(&self, name: &str) -> Option<u8> {
        self.current().hoisted_scopes.last()?.get(name).copied()
    }

    // -- main expression dispatch ------------------------------------

    fn compile_expr(&mut self, e: &SpannedExpr) -> Result<(), CompileError> {
        let line = e.span.line;
        match &e.expr {
            Expr::Int(n) => self.emit_constant(Value::Int(*n), line, e.span)?,
            Expr::Float(x) => self.emit_constant(Value::Float(*x), line, e.span)?,
            Expr::Str(s) => self.emit_constant(Value::Str(s.as_str().into()), line, e.span)?,
            Expr::Bool(b) => self.emit_constant(Value::Bool(*b), line, e.span)?,
            Expr::Null => self.emit_op(OpCode::PushNull, line),

            Expr::Ident(name) => {
                let r = self.resolve(name, e.span)?.ok_or_else(|| {
                    CompileError::new(
                        CompileErrorKind::UndeclaredVariable(name.clone()),
                        e.span,
                    )
                })?;
                self.emit_load(r, line);
            }

            Expr::BinOp(op, lhs, rhs) => self.compile_binop(*op, lhs, rhs, line)?,

            Expr::UnOp(op, inner) => {
                self.compile_expr(inner)?;
                let opcode = match op {
                    UnOp::Neg => OpCode::Negate,
                    UnOp::Not => OpCode::Not,
                    UnOp::Len => OpCode::Len,
                };
                self.emit_op(opcode, line);
            }

            Expr::Block(b) => self.compile_block_value(b)?,

            Expr::Scope(b) => {
                self.begin_scope();
                let mut hoisted = Vec::new();
                self.visit_block_for_hoist(b, &mut hoisted);
                self.emit_hoist_prologue(hoisted, e.span)?;
                self.compile_block_value(b)?;
                self.end_scope(line)?;
            }

            Expr::If(cond, then_branch, else_branch) => {
                self.compile_if(cond, then_branch, else_branch, line)?;
            }

            Expr::While { is_array, cond, body } => {
                self.compile_while(*is_array, cond, body, line)?;
            }

            Expr::For { is_array, vars, iter, body } => {
                self.compile_for(*is_array, vars, iter, body, line, e.span)?;
            }

            Expr::Range { from, to, step, inclusive } => {
                self.compile_range(from, to, step.as_deref(), *inclusive, line)?;
            }

            Expr::Break(value) => {
                self.compile_break(value.as_deref(), line, e.span)?;
            }

            Expr::Decl(pat, init) => {
                // Simple `name := value` keeps the Phase-4
                // declare-before-init shape for `Fn` initialisers,
                // which lets the body refer to its own name
                // (recursion). Other patterns can't recurse — there's
                // no single name to declare ahead of time — so they
                // unconditionally declare-after.
                match pat {
                    Pattern::Ident(name) => {
                        if let Some(slot) = self.lookup_hoisted(name) {
                            // Hoisted: the local was pre-declared at
                            // scope entry. Just compile the init and
                            // `StoreLocal` to its stable slot. The
                            // peek leaves the value on top as the
                            // Decl-expression's result.
                            self.compile_expr(init)?;
                            self.emit_op(OpCode::StoreLocal, line);
                            self.emit_byte(slot, line);
                        } else if matches!(init.expr, Expr::Fn { .. }) {
                            // Push a Null placeholder, declare the
                            // local at that slot, compile the Fn (it
                            // captures the name as an upvalue), then
                            // StoreLocal copies the closure into the
                            // reserved slot. The Decl's value is the
                            // closure (left on top via the peek).
                            self.emit_op(OpCode::PushNull, line);
                            self.declare_local(name, e.span)?;
                            let slot = self.current().locals.last().unwrap().slot;
                            self.compile_expr(init)?;
                            self.emit_op(OpCode::StoreLocal, line);
                            self.emit_byte(slot, line);
                        } else {
                            self.compile_expr(init)?;
                            self.declare_local(name, e.span)?;
                            self.emit_op(OpCode::Dup, line);
                        }
                    }
                    Pattern::Wildcard => {
                        // `_ := expr` — evaluate side-effect, leave
                        // a `null` as the decl's value.
                        self.compile_expr(init)?;
                        // Decl expressions evaluate to the bound
                        // value. For wildcard there's no name, but
                        // the value remains useful for chaining.
                        // No declare_local; just leave init value
                        // on the stack as the expression's result.
                    }
                    Pattern::Array { .. } | Pattern::Object { .. } => {
                        // Compile init: pushes the source value.
                        self.compile_expr(init)?;
                        // Destructure consumes the source from the
                        // top of stack and declares names. We push a
                        // Null afterwards because `pattern := expr`
                        // as an expression should still produce a
                        // value — we lose the structural source
                        // (per spec it's "the names" anyway).
                        self.compile_pattern(pat, e.span)?;
                        self.emit_op(OpCode::PushNull, line);
                    }
                }
            }

            Expr::Assign(name, op, value) => {
                let r = self.resolve(name, e.span)?.ok_or_else(|| {
                    CompileError::new(
                        CompileErrorKind::UndeclaredAssign(name.clone()),
                        e.span,
                    )
                })?;
                if let Resolved::Global(_) = r {
                    return Err(CompileError::new(
                        CompileErrorKind::AssignToBuiltin(name.clone()),
                        e.span,
                    ));
                }
                if let Some(op) = op {
                    self.emit_load(r, line);
                    self.compile_expr(value)?;
                    self.emit_op(binop_to_opcode(*op), line);
                } else {
                    self.compile_expr(value)?;
                }
                self.emit_store(r, line);
            }

            Expr::Array(items) => {
                let has_spread = items.iter()
                    .any(|i| matches!(i.expr, Expr::Spread(_)));
                if !has_spread {
                    // Fast path: contiguous element pushes + MakeArray.
                    if items.len() > 255 {
                        return Err(CompileError::new(
                            CompileErrorKind::TooManyConstants,
                            e.span,
                        ));
                    }
                    for item in items {
                        self.compile_expr(item)?;
                    }
                    self.emit_op(OpCode::MakeArray, line);
                    self.emit_byte(items.len() as u8, line);
                    // MakeArray n: pops n, pushes 1.
                    self.adjust_stack(-(items.len() as i32) + 1);
                } else {
                    // Build incrementally so the spread elements can
                    // be runtime-counted.
                    self.emit_op(OpCode::MakeArray, line);
                    self.emit_byte(0, line);
                    self.adjust_stack(1); // MakeArray 0 pushes empty array
                    for item in items {
                        match &item.expr {
                            Expr::Spread(inner) => {
                                self.compile_expr(inner)?;
                                self.emit_op(OpCode::ArrayExtend, line);
                            }
                            _ => {
                                self.compile_expr(item)?;
                                self.emit_op(OpCode::ArrayPush, line);
                            }
                        }
                    }
                }
            }

            Expr::Object(members) => {
                let has_spread = members.iter()
                    .any(|m| matches!(m, ObjectMember::Spread(_)));
                if !has_spread {
                    if members.len() > 255 {
                        return Err(CompileError::new(
                            CompileErrorKind::TooManyConstants,
                            e.span,
                        ));
                    }
                    for m in members {
                        let ObjectMember::Pair(key, value) = m else { unreachable!() };
                        self.emit_constant(
                            Value::Str(key.as_str().into()), line, e.span,
                        )?;
                        self.compile_expr(value)?;
                    }
                    self.emit_op(OpCode::MakeObject, line);
                    self.emit_byte(members.len() as u8, line);
                    // MakeObject n: pops 2n (k,v pairs), pushes 1.
                    self.adjust_stack(-(members.len() as i32 * 2) + 1);
                } else {
                    // Incremental build via Dup + IndexSet/Pop for
                    // pairs and ObjectMerge for spreads. Later keys
                    // win because IndexSet on an existing key replaces.
                    self.emit_op(OpCode::MakeObject, line);
                    self.emit_byte(0, line);
                    self.adjust_stack(1); // MakeObject 0 pushes empty obj
                    for m in members {
                        match m {
                            ObjectMember::Pair(key, value) => {
                                self.emit_op(OpCode::Dup, line);
                                self.emit_constant(
                                    Value::Str(key.as_str().into()),
                                    line,
                                    e.span,
                                )?;
                                self.compile_expr(value)?;
                                self.emit_op(OpCode::IndexSet, line);
                                self.emit_op(OpCode::Pop, line);
                            }
                            ObjectMember::Spread(inner) => {
                                self.compile_expr(inner)?;
                                self.emit_op(OpCode::ObjectMerge, line);
                            }
                        }
                    }
                }
            }

            Expr::Spread(_) => {
                // Spread only valid as a direct child of Array / Call
                // / ObjectMember. Reaching here means it appeared in a
                // free expression position.
                return Err(CompileError::new(
                    CompileErrorKind::SpreadInInvalidPosition,
                    e.span,
                ));
            }

            Expr::Template(parts) => {
                if parts.len() > 255 {
                    return Err(CompileError::new(
                        CompileErrorKind::TooManyConstants,
                        e.span,
                    ));
                }
                // Single-literal templates would have been emitted as
                // `Token::Str` by the lexer, so we don't have to
                // optimise that case. Emit each part and then ConcatN.
                for part in parts {
                    match part {
                        TemplatePart::Lit(s) => {
                            self.emit_constant(
                                Value::Str(s.as_str().into()),
                                line,
                                e.span,
                            )?;
                        }
                        TemplatePart::Expr(inner) => {
                            self.compile_expr(inner)?;
                        }
                    }
                }
                self.emit_op(OpCode::ConcatN, line);
                self.emit_byte(parts.len() as u8, line);
                self.adjust_stack(-(parts.len() as i32) + 1);
            }

            Expr::Index(obj, key) => {
                self.compile_expr(obj)?;
                self.compile_expr(key)?;
                self.emit_op(OpCode::IndexGet, line);
            }

            Expr::IndexAssign(obj, key, op, value) => {
                self.compile_expr(obj)?;
                self.compile_expr(key)?;
                if let Some(op) = op {
                    self.emit_op(OpCode::Dup2, line);
                    self.emit_op(OpCode::IndexGet, line);
                    self.compile_expr(value)?;
                    self.emit_op(binop_to_opcode(*op), line);
                } else {
                    self.compile_expr(value)?;
                }
                self.emit_op(OpCode::IndexSet, line);
            }

            Expr::Call(callee, args) => {
                let has_spread = args.iter()
                    .any(|a| matches!(a.expr, Expr::Spread(_)));
                if !has_spread {
                    if args.len() > 255 {
                        return Err(CompileError::new(
                            CompileErrorKind::TooManyConstants,
                            e.span,
                        ));
                    }
                    self.compile_expr(callee)?;
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_op(OpCode::Call, line);
                    self.emit_byte(args.len() as u8, line);
                    // Call n: pops callee + n args, pushes 1 result.
                    self.adjust_stack(-(args.len() as i32 + 1) + 1);
                } else {
                    // Build args-array first, then CallSpread expands
                    // it at runtime. The runtime arity matches the
                    // array length.
                    self.compile_expr(callee)?;
                    self.emit_op(OpCode::MakeArray, line);
                    self.emit_byte(0, line);
                    self.adjust_stack(1); // empty array pushed
                    for arg in args {
                        match &arg.expr {
                            Expr::Spread(inner) => {
                                self.compile_expr(inner)?;
                                self.emit_op(OpCode::ArrayExtend, line);
                            }
                            _ => {
                                self.compile_expr(arg)?;
                                self.emit_op(OpCode::ArrayPush, line);
                            }
                        }
                    }
                    // CallSpread: pops callee + args-array, pushes
                    // result. emit_op already applies -1 (fixed effect
                    // tabled above).
                    self.emit_op(OpCode::CallSpread, line);
                }
            }

            Expr::Fn { params, rest, body } => {
                self.compile_fn(params, rest.as_deref(), body, e.span)?
            }

            Expr::Import(path) => self.compile_import(path, line, e.span)?,

            Expr::Return(value) => {
                if let Some(v) = value {
                    self.compile_expr(v)?;
                } else {
                    self.emit_op(OpCode::PushNull, line);
                }
                self.emit_op(OpCode::Return, line);
            }
        }
        Ok(())
    }

    fn emit_load(&mut self, r: Resolved, line: u32) {
        match r {
            Resolved::Local(slot) => {
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(slot, line);
            }
            Resolved::Upvalue(idx) => {
                self.emit_op(OpCode::GetUpvalue, line);
                self.emit_byte(idx, line);
            }
            Resolved::Global(idx) => {
                self.emit_op(OpCode::LoadGlobal, line);
                self.emit_byte(idx, line);
            }
        }
    }

    fn emit_store(&mut self, r: Resolved, line: u32) {
        match r {
            Resolved::Local(slot) => {
                self.emit_op(OpCode::StoreLocal, line);
                self.emit_byte(slot, line);
            }
            Resolved::Upvalue(idx) => {
                self.emit_op(OpCode::SetUpvalue, line);
                self.emit_byte(idx, line);
            }
            Resolved::Global(_) => unreachable!("AssignToBuiltin caught earlier"),
        }
    }

    // -- binary ops --------------------------------------------------

    fn compile_binop(
        &mut self,
        op: BinOp,
        lhs: &SpannedExpr,
        rhs: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        match op {
            BinOp::And => return self.compile_and(lhs, rhs, line),
            BinOp::Or => return self.compile_or(lhs, rhs, line),
            _ => {}
        }
        self.compile_expr(lhs)?;
        self.compile_expr(rhs)?;
        self.emit_op(binop_to_opcode(op), line);
        Ok(())
    }

    fn compile_and(
        &mut self,
        lhs: &SpannedExpr,
        rhs: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        self.compile_expr(lhs)?;
        let end_jump = self.emit_jump(OpCode::JumpIfFalse, line);
        self.emit_op(OpCode::Pop, line);
        self.compile_expr(rhs)?;
        self.patch_jump(end_jump)?;
        Ok(())
    }

    fn compile_or(
        &mut self,
        lhs: &SpannedExpr,
        rhs: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        self.compile_expr(lhs)?;
        let end_jump = self.emit_jump(OpCode::JumpIfTrue, line);
        self.emit_op(OpCode::Pop, line);
        self.compile_expr(rhs)?;
        self.patch_jump(end_jump)?;
        Ok(())
    }

    // -- if / while ---------------------------------------------------

    fn compile_if(
        &mut self,
        cond: &SpannedExpr,
        then_branch: &SpannedExpr,
        else_branch: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        self.compile_expr(cond)?;
        // Height with cond on top — this is also the height at the
        // `to_else` jump's landing point (JumpIfFalse peeks).
        let cond_height = self.current().stack_height;
        let to_else = self.emit_jump(OpCode::JumpIfFalse, line);
        self.emit_op(OpCode::Pop, line);
        self.compile_expr(then_branch)?;
        let to_end = self.emit_jump(OpCode::Jump, line);
        // Reset tracker for the else-path entry.
        self.set_stack_height(cond_height);
        self.patch_jump(to_else)?;
        self.emit_op(OpCode::Pop, line);
        self.compile_expr(else_branch)?;
        self.patch_jump(to_end)?;
        Ok(())
    }

    /// Lowering:
    /// ```text
    ///   begin_scope
    ///     <push initial result>           ; null or empty array
    ///     decl_local("$while_result")     ; slot S
    /// loop_start:
    ///     <cond>
    ///     jump_if_false exit              ; peeks, jumps on falsy
    ///     pop                              ; pop the truthy cond
    ///     <body>                           ; pushes body value
    ///     (store_local S; pop)             OR (iter_append S)
    ///     loop loop_start
    /// exit:
    ///     pop                              ; pop the falsy cond
    ///     load_local S                     ; copy result to top
    ///   end_scope                          ; close_scope drops $while_result
    /// ```
    /// Break jumps to `exit` directly via the loop_stack mechanism.
    fn compile_while(
        &mut self,
        is_array: bool,
        cond: &SpannedExpr,
        body: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        self.begin_scope();

        if is_array {
            self.emit_op(OpCode::MakeArray, line);
            self.emit_byte(0, line);
            self.adjust_stack(1); // empty array pushed
        } else {
            self.emit_op(OpCode::PushNull, line);
        }
        self.declare_local("$while_result", Span::new(0, 0, line))?;
        let result_slot = self.current().locals.last().unwrap().slot;

        // Hoist nested `:=` declarations from the cond into this
        // while-scope. Body is a Scope (per parser) and handles its
        // own hoisting at its begin_scope.
        let mut hoisted = Vec::new();
        self.visit_for_hoist(cond, &mut hoisted);
        self.emit_hoist_prologue(hoisted, Span::new(0, 0, line))?;

        let base_stack_height = self.current().stack_height;

        self.current_mut().loop_stack.push(LoopCtx {
            result_slot,
            base_stack_height,
            is_array_form: is_array,
            exit_jumps: Vec::new(),
            skip: false,
        });

        let loop_start = self.current_chunk().code.len();
        self.compile_expr(cond)?;
        // Snapshot the height with `cond` on the stack; the exit jump
        // target lands here (JumpIfFalse peeks, doesn't pop).
        let cond_height = self.current().stack_height;
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse, line);
        self.emit_op(OpCode::Pop, line); // truthy cond
        self.compile_expr(body)?;
        if is_array {
            self.emit_op(OpCode::IterAppend, line);
            self.emit_byte(result_slot, line);
        } else {
            self.emit_op(OpCode::StoreLocal, line);
            self.emit_byte(result_slot, line);
            self.emit_op(OpCode::Pop, line);
        }
        self.emit_loop(loop_start, line)?;
        // Reset to the state at the exit-jump target (cond on top).
        self.set_stack_height(cond_height);
        self.patch_jump(exit_jump)?;
        // exit: stack tip is the falsy cond
        self.emit_op(OpCode::Pop, line);

        // Break-jumps land here with the stack already truncated to
        // base_stack_height by Unwind — same height as after the
        // falsy-cond Pop.
        let ctx = self.current_mut().loop_stack.pop().unwrap();
        for jmp in ctx.exit_jumps {
            self.patch_jump(jmp)?;
        }

        self.emit_op(OpCode::LoadLocal, line);
        self.emit_byte(result_slot, line);
        self.end_scope(line)?;
        Ok(())
    }

    /// Range expression lowering:
    /// ```text
    ///   <from>
    ///   <to>
    ///   <step>      ; only if explicit
    ///   make_range flags
    /// ```
    /// `flags` packs `inclusive` (bit 0) and `has_step` (bit 1). When
    /// step is absent, the VM auto-picks ±1 from from/to (spec §7.3).
    fn compile_range(
        &mut self,
        from: &SpannedExpr,
        to: &SpannedExpr,
        step: Option<&SpannedExpr>,
        inclusive: bool,
        line: u32,
    ) -> Result<(), CompileError> {
        self.compile_expr(from)?;
        self.compile_expr(to)?;
        let mut flags: u8 = 0;
        if inclusive { flags |= 1; }
        let popped = if let Some(s) = step {
            flags |= 2;
            self.compile_expr(s)?;
            3
        } else {
            2
        };
        self.emit_op(OpCode::MakeRange, line);
        self.emit_byte(flags, line);
        // MakeRange: pops 2 or 3, pushes 1.
        self.adjust_stack(-popped + 1);
        Ok(())
    }

    /// Lowering of `for (vars, iter) body`:
    /// ```text
    ///   begin_scope (outer)
    ///     <init result>                     ; null or empty []
    ///     decl_local("$for_result")         ; slot R
    ///     <iter>
    ///     make_iter
    ///     decl_local("$for_iter")           ; slot R+1
    /// loop_start:
    ///     iter_next  exit                   ; (or iter_next2) — pushes
    ///                                       ; var(s); jumps on done
    ///     begin_scope (per-iteration)
    ///       decl_local(var1) [, var2]
    ///       <body>                          ; pushes body value
    ///       store_local R; pop              ; OR iter_append R
    ///       unwind base_locals_count + 0    ; drops the iter var(s)
    ///                                       ; AND closes their upvalues
    ///                                       ; (the §10.4 fresh-slot bit)
    ///     end_scope-bookkeeping (no opcode)
    ///     loop loop_start
    /// exit:
    ///     load_local R
    ///   end_scope (outer)                    ; closes $for_iter, $for_result
    /// ```
    fn compile_for(
        &mut self,
        is_array: bool,
        vars: &[String],
        iter: &SpannedExpr,
        body: &SpannedExpr,
        line: u32,
        span: Span,
    ) -> Result<(), CompileError> {
        if vars.is_empty() || vars.len() > 2 {
            return Err(CompileError::new(
                CompileErrorKind::TooManyLocals, // misuse — should be caught by parser
                span,
            ));
        }
        self.begin_scope();

        if is_array {
            self.emit_op(OpCode::MakeArray, line);
            self.emit_byte(0, line);
            self.adjust_stack(1); // empty array pushed
        } else {
            self.emit_op(OpCode::PushNull, line);
        }
        self.declare_local("$for_result", span)?;
        let result_slot = self.current().locals.last().unwrap().slot;

        // Hoist nested `:=` declarations from the iter expression
        // into the for-scope. Body is a Scope (per parser) and will
        // hoist its own at its own begin_scope.
        let mut hoisted_iter = Vec::new();
        self.visit_for_hoist(iter, &mut hoisted_iter);
        self.emit_hoist_prologue(hoisted_iter, span)?;

        self.compile_expr(iter)?;
        self.emit_op(OpCode::MakeIter, line);
        self.declare_local("$for_iter", span)?;

        // Anything declared at or above this height (the iteration
        // variables) gets unwound by break / end-of-iter.
        let base_stack_height = self.current().stack_height;

        self.current_mut().loop_stack.push(LoopCtx {
            result_slot,
            base_stack_height,
            is_array_form: is_array,
            exit_jumps: Vec::new(),
            skip: false,
        });

        let loop_start = self.current_chunk().code.len();
        let iter_op = if vars.len() == 1 { OpCode::IterNext } else { OpCode::IterNext2 };
        let exit_jump = self.emit_jump(iter_op, line);
        // On the success path, IterNext pushes 1 var (IterNext2 pushes
        // 2: counter then value). The two values live at consecutive
        // slots; we declare each at the right slot so a name→slot
        // lookup returns the matching value, not whichever is on top.
        self.adjust_stack(if vars.len() == 1 { 1 } else { 2 });

        // Per-iteration scope so each iter's vars get fresh slots (and
        // closing upvalues at end of iter heap-promotes captured vars —
        // distinct cells per iter, per spec §10.4).
        self.begin_scope();
        let top_slot = (self.current().stack_height - 1) as u8;
        for (i, v) in vars.iter().enumerate() {
            // vars[0] = counter (pushed first → lower slot)
            // vars[1] = value   (pushed second → higher slot, = top_slot)
            let slot = top_slot - (vars.len() - 1 - i) as u8;
            self.declare_local_at(v, slot, span)?;
        }

        self.compile_expr(body)?;
        if is_array {
            self.emit_op(OpCode::IterAppend, line);
            self.emit_byte(result_slot, line);
        } else {
            self.emit_op(OpCode::StoreLocal, line);
            self.emit_byte(result_slot, line);
            self.emit_op(OpCode::Pop, line);
        }
        // Drop the iter var(s) for the next iter; also close their
        // upvalues so each iter's closure captures its own cell.
        self.emit_op(OpCode::Unwind, line);
        self.emit_byte(base_stack_height as u8, line);
        self.set_stack_height(base_stack_height);
        // Update compiler-side locals tracking to mirror the runtime
        // pop (we cannot call end_scope because it preserves the top
        // value, which we don't have here).
        let depth_to_drop = self.current().scope_depth;
        while let Some(l) = self.current().locals.last() {
            if l.depth < depth_to_drop { break; }
            self.current_mut().locals.pop();
        }
        self.current_mut().scope_depth -= 1;
        // Pop the per-iter hoisted-scope entry pushed by begin_scope.
        // (end_scope handles this normally, but here we're doing the
        // teardown manually because the top-of-stack value would
        // otherwise be preserved by CloseScope.)
        self.current_mut().hoisted_scopes.pop();

        self.emit_loop(loop_start, line)?;
        // On the IterNext "done" path, no var was pushed — height is
        // still base_stack_height.
        self.set_stack_height(base_stack_height);
        self.patch_jump(exit_jump)?;

        let ctx = self.current_mut().loop_stack.pop().unwrap();
        for jmp in ctx.exit_jumps {
            self.patch_jump(jmp)?;
        }

        // exit: stack tip is just the IterState ($for_iter local).
        self.emit_op(OpCode::LoadLocal, line);
        self.emit_byte(result_slot, line);
        self.end_scope(line)?;
        Ok(())
    }

    /// Break compilation (spec §15.3 strategy B). Stores the value into
    /// the target loop's result slot, unwinds any nested locals, then
    /// jumps to the loop exit. Chained `break (break v)` works because
    /// the outer break marks its target as `skip` before evaluating its
    /// value; the inner break re-runs the search and finds the next
    /// loop out.
    fn compile_break(
        &mut self,
        value: Option<&SpannedExpr>,
        line: u32,
        span: Span,
    ) -> Result<(), CompileError> {
        let entry_height = self.current().stack_height;
        let target = self
            .current()
            .loop_stack
            .iter()
            .rposition(|lc| !lc.skip)
            .ok_or_else(|| {
                CompileError::new(CompileErrorKind::BreakOutsideLoop, span)
            })?;

        // While we compile the value, mark target as skipped so any
        // nested break inside the value redirects past this loop.
        self.current_mut().loop_stack[target].skip = true;
        if let Some(v) = value {
            self.compile_expr(v)?;
        } else {
            self.emit_op(OpCode::PushNull, line);
        }
        self.current_mut().loop_stack[target].skip = false;

        let ctx = &self.current().loop_stack[target];
        let result_slot = ctx.result_slot;
        let base_stack_height = ctx.base_stack_height;
        let is_array_form = ctx.is_array_form;

        if is_array_form {
            self.emit_op(OpCode::IterAppend, line);
            self.emit_byte(result_slot, line);
        } else {
            self.emit_op(OpCode::StoreLocal, line);
            self.emit_byte(result_slot, line);
            self.emit_op(OpCode::Pop, line);
        }
        self.emit_op(OpCode::Unwind, line);
        self.emit_byte(base_stack_height as u8, line);

        let jmp = self.emit_jump(OpCode::Jump, line);
        self.current_mut().loop_stack[target].exit_jumps.push(jmp);
        // Break is `compile_expr`'s convention "leaves +1" — but the
        // runtime never actually reaches code after the Jump. We
        // restore `entry_height + 1` so subsequent fall-through
        // emissions stay consistent with the surrounding expression's
        // tracker. (Any branch convergence point will reset properly.)
        self.set_stack_height(entry_height + 1);
        Ok(())
    }

    // -- function literal --------------------------------------------

    fn compile_fn(
        &mut self,
        params: &[Pattern],
        rest: Option<&str>,
        body: &SpannedExpr,
        span: Span,
    ) -> Result<(), CompileError> {
        let line = span.line;
        self.push_function(params.len(), None);

        // slot 0 = closure placeholder; slots 1..=arity = fixed
        // params; slot arity+1 = rest array (if any). The runtime
        // call-protocol puts the args in those exact slots, so seed
        // the height tracker to match.
        self.current_mut().stack_height = 1;
        self.declare_local("", span)?;
        for pat in params {
            self.current_mut().stack_height += 1;
            // For simple-Ident patterns, use the name directly so
            // recursion and upvalue capture see the right local. For
            // structural patterns leave the slot anonymous and
            // destructure below.
            match pat {
                Pattern::Ident(n) => self.declare_local(n, span)?,
                _ => self.declare_local("", span)?,
            }
        }
        if let Some(name) = rest {
            self.current_mut().stack_height += 1;
            self.declare_local(name, span)?;
        }

        // Now destructure any structural patterns. The argument's
        // value is already at slot (i+1).
        for (i, pat) in params.iter().enumerate() {
            match pat {
                Pattern::Ident(_) | Pattern::Wildcard => {} // direct or discarded
                _ => {
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte((i + 1) as u8, line);
                    self.compile_pattern(pat, span)?;
                }
            }
        }

        // body is always a Scope (per parser)
        self.compile_expr(body)?;

        // implicit Return at the end
        self.emit_op(OpCode::Return, line);

        let fc = self.funcs.pop().unwrap();
        let upvalues_info = fc.upvalues.clone();
        let function = Function {
            arity: fc.arity,
            has_rest: rest.is_some(),
            chunk: fc.chunk,
            upvalues: fc.upvalues,
            name: fc.name,
        };

        let idx = self
            .current_chunk_mut()
            .add_function(Rc::new(function))
            .map_err(|_| CompileError::new(CompileErrorKind::TooManyConstants, span))?;
        self.emit_op(OpCode::Closure, line);
        self.emit_byte(idx, line);
        for up in &upvalues_info {
            self.emit_byte(if up.is_local { 1 } else { 0 }, line);
            self.emit_byte(up.index, line);
        }
        Ok(())
    }

    /// Destructure the value currently on top of the stack according
    /// to `pat`. Consumes the value; declares names per the pattern.
    ///
    /// Implementation strategy: for structural patterns we declare an
    /// anonymous local at the source-value slot, then load+index for
    /// each element. Sub-patterns recurse. The anonymous slot
    /// "leaks" until the enclosing scope ends — same as if the user
    /// had written `tmp := rhs; a := tmp[0]; ...`.
    fn compile_pattern(
        &mut self,
        pat: &Pattern,
        span: Span,
    ) -> Result<(), CompileError> {
        let line = span.line;
        match pat {
            Pattern::Wildcard => {
                self.emit_op(OpCode::Pop, line);
            }
            Pattern::Ident(name) => {
                self.declare_local(name, span)?;
            }
            Pattern::Array { items, rest } => {
                // Make the source an anonymous local at its current
                // stack slot. From here on, locals/stack grows above.
                self.declare_local("", span)?;
                let src_slot = self.current().locals.last().unwrap().slot;
                for (i, item) in items.iter().enumerate() {
                    if matches!(item, Pattern::Wildcard) {
                        continue; // skip — no need to extract
                    }
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Int(i as i64), line, span)?;
                    self.emit_op(OpCode::IndexGet, line);
                    self.compile_pattern(item, span)?;
                }
                if let Some(rest_name) = rest {
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Int(items.len() as i64), line, span)?;
                    self.emit_op(OpCode::SliceFrom, line);
                    self.declare_local(rest_name, span)?;
                }
            }
            Pattern::Object { fields, rest } => {
                self.declare_local("", span)?;
                let src_slot = self.current().locals.last().unwrap().slot;
                for f in fields {
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(
                        Value::Str(f.key.as_str().into()),
                        line,
                        span,
                    )?;
                    self.emit_op(OpCode::IndexGet, line);
                    self.compile_pattern(&f.pattern, span)?;
                }
                if let Some(rest_name) = rest {
                    // Push source, then a literal array of consumed
                    // keys; ObjRest builds the remainder.
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    for f in fields {
                        self.emit_constant(
                            Value::Str(f.key.as_str().into()),
                            line,
                            span,
                        )?;
                    }
                    self.emit_op(OpCode::MakeArray, line);
                    self.emit_byte(fields.len() as u8, line);
                    self.adjust_stack(-(fields.len() as i32) + 1);
                    self.emit_op(OpCode::ObjRest, line);
                    self.declare_local(rest_name, span)?;
                }
            }
        }
        Ok(())
    }

    /// Resolve an `import 'path'` to an absolute path, store it in
    /// the constant pool, and emit `Import idx`. Path resolution
    /// mirrors spec §12: relative to the importing file's directory,
    /// `.tg` appended if absent.
    fn compile_import(
        &mut self,
        path: &str,
        line: u32,
        span: Span,
    ) -> Result<(), CompileError> {
        let mut resolved = if std::path::Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            match &self.base_dir {
                Some(d) => d.join(path),
                None => PathBuf::from(path),
            }
        };
        if resolved.extension().is_none() {
            resolved.set_extension("tg");
        }
        let path_str = resolved.to_string_lossy().to_string();
        self.emit_constant(Value::Str(path_str.into()), line, span)?;
        self.emit_op(OpCode::Import, line);
        // The Import opcode operand is the const index of the path
        // that we just pushed — but we already used LoadConst to
        // push it onto the stack, so Import just consumes it. No
        // additional operand needed.
        Ok(())
    }

    // -- jump emission / patching ------------------------------------

    fn emit_jump(&mut self, op: OpCode, line: u32) -> usize {
        self.emit_op(op, line);
        let offset = self.current_chunk().code.len();
        self.current_chunk_mut().write_u16(0xffff, line);
        offset
    }

    fn patch_jump(&mut self, offset: usize) -> Result<(), CompileError> {
        let here = self.current_chunk().code.len();
        let dist = here.checked_sub(offset + 2).ok_or_else(|| {
            CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, 0))
        })?;
        if dist > u16::MAX as usize {
            return Err(CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, 0)));
        }
        self.current_chunk_mut().patch_u16(offset, dist as u16);
        Ok(())
    }

    fn emit_loop(&mut self, target: usize, line: u32) -> Result<(), CompileError> {
        self.emit_op(OpCode::Loop, line);
        let here = self.current_chunk().code.len();
        let dist = (here + 2).checked_sub(target).ok_or_else(|| {
            CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, line))
        })?;
        if dist > u16::MAX as usize {
            return Err(CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, line)));
        }
        self.current_chunk_mut().write_u16(dist as u16, line);
        Ok(())
    }
}

fn binop_to_opcode(op: BinOp) -> OpCode {
    match op {
        BinOp::Add => OpCode::Add,
        BinOp::Sub => OpCode::Sub,
        BinOp::Mul => OpCode::Mul,
        BinOp::Div => OpCode::Div,
        BinOp::Mod => OpCode::Mod,
        BinOp::Pow => OpCode::Pow,
        BinOp::Eq => OpCode::Eq,
        BinOp::Neq => OpCode::Neq,
        BinOp::Lt => OpCode::Lt,
        BinOp::Le => OpCode::Le,
        BinOp::Gt => OpCode::Gt,
        BinOp::Ge => OpCode::Ge,
        BinOp::And | BinOp::Or => unreachable!("handled by short-circuit lowering"),
    }
}
