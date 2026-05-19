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
use std::sync::Arc;

use crate::vm::ast::{
    BinOp, Block, Expr, LiteralPat, MatchArm, MatchPattern,
    ObjectMember, Pattern, SpannedExpr, TemplatePart, UnOp,
};
use std::path::PathBuf;
use crate::vm::chunk::{Chunk, Const};
use crate::vm::error::{CompileError, CompileErrorKind};
use crate::vm::opcode::OpCode;
use crate::vm::source_map::SourceId;
use crate::vm::stdlib;
use crate::vm::token::Span;
use crate::vm::value::{Function, UpvalueInfo, Value};

/// Hashable key for constant-pool deduplication. Mirrors [`Const`] but
/// stores `Float` as raw bits, so it can derive `Hash + Eq` (`f64` is
/// neither). Bit-pattern keying is the correct pooling semantics: it
/// keeps `0.0` and `-0.0` distinct (a program that wrote one
/// specifically should get that one — `1.0 / -0.0 ≠ 1.0 / 0.0`) and
/// treats bit-identical `NaN`s as equal.
#[derive(Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(Arc<str>),
}

impl ConstKey {
    fn of(c: &Const) -> ConstKey {
        match c {
            Const::Null => ConstKey::Null,
            Const::Bool(b) => ConstKey::Bool(*b),
            Const::Int(n) => ConstKey::Int(*n),
            Const::Float(x) => ConstKey::Float(x.to_bits()),
            Const::Str(s) => ConstKey::Str(s.clone()),
        }
    }
}

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
/// - `loop_start` — bytecode offset of the loop's head (the `IterNext`
///   for `for`, the condition for `while`). `continue` emits a backward
///   `Loop` to this offset.
struct LoopCtx {
    result_slot: u8,
    base_stack_height: u32,
    is_array_form: bool,
    exit_jumps: Vec<usize>,
    skip: bool,
    loop_start: usize,
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
    /// Deduplication index for this function's constant pool. Maps a
    /// constant value to the pool slot it already occupies, so a
    /// repeated literal reuses one slot instead of spending a fresh
    /// entry (the pool is per-chunk and capped — see `add_constant`).
    const_dedup: HashMap<ConstKey, u16>,
}

impl FuncCompiler {
    fn new(
        arity: usize,
        name: Option<String>,
        source: SourceId,
        base_dir: Option<PathBuf>,
    ) -> Self {
        let mut chunk = Chunk::new();
        chunk.source = source;
        chunk.base_dir = base_dir;
        FuncCompiler {
            chunk,
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            arity,
            name,
            loop_stack: Vec::new(),
            stack_height: 0,
            hoisted_scopes: vec![HashMap::new()],
            const_dedup: HashMap::new(),
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
    /// Stamped on every chunk produced by this compiler so runtime and
    /// compile-time errors can be rendered against the right source.
    source: SourceId,
    /// Name to attach to the next `fn` literal compiled, inferred from
    /// the binding it initialises (`f := fn(){}` → `"f"`). Set by the
    /// `Decl` handler; consumed (taken) by `compile_fn` so only the
    /// outermost `fn` of the initialiser is named.
    fn_name_hint: Option<String>,
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
    /// resolution. No source attribution — errors render bare.
    pub fn compile_with_dir(
        program: &Block,
        base_dir: Option<PathBuf>,
    ) -> Result<Function, CompileError> {
        Self::compile_with_source(program, base_dir, SourceId::UNKNOWN)
    }

    /// Compile with a base directory AND a [`SourceId`]. Every chunk
    /// produced (the top-level main + every nested function) is
    /// stamped with `source` so runtime errors render against it.
    /// The returned `CompileError`'s `source` is also stamped.
    pub fn compile_with_source(
        program: &Block,
        base_dir: Option<PathBuf>,
        source: SourceId,
    ) -> Result<Function, CompileError> {
        (|| {
            let mut c = Compiler {
                funcs: Vec::new(),
                globals: stdlib::names().to_vec(),
                base_dir,
                source,
                fn_name_hint: None,
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

            c.compile_block_value(program, false)?;
            let last_line = c.current_chunk().lines.last().copied().unwrap_or(1);
            c.current_chunk_mut().write_op(OpCode::Return, last_line);

            let mut fc = c.funcs.pop().expect("main function compiler popped");
            fc.chunk.thread_jumps();
            Ok(Function {
                arity: 0,
                has_rest: false,
                chunk: fc.chunk,
                upvalues: fc.upvalues,
                name: fc.name,
                is_generator: false,
            })
        })()
        .map_err(|mut e: CompileError| {
            if e.source.is_unknown() { e.source = source; }
            e
        })
    }

    /// Compile one REPL line.
    ///
    /// `existing_locals` is the list of (name, slot) pairs the REPL
    /// has accumulated from previous lines. They're pre-declared at
    /// their slots so name resolution emits the right `LoadLocal`. The
    /// chunk ends in `Halt` (not `Return`) so the REPL frame survives
    /// for the next line.
    ///
    /// Returns the compiled function and the list of NEW top-level
    /// locals the line declared. The driver appends those to its
    /// state for use on the next line.
    #[allow(dead_code)]
    pub fn compile_repl(
        program: &Block,
        existing_locals: &[(String, u8)],
    ) -> Result<(Function, Vec<(String, u8)>), CompileError> {
        Self::compile_repl_with_source(program, existing_locals, SourceId::UNKNOWN)
    }

    /// REPL compile with a [`SourceId`] for the line's buffer so
    /// errors render against it.
    pub fn compile_repl_with_source(
        program: &Block,
        existing_locals: &[(String, u8)],
        source: SourceId,
    ) -> Result<(Function, Vec<(String, u8)>), CompileError> {
        (|| {
        let mut c = Compiler {
            funcs: Vec::new(),
            globals: stdlib::names().to_vec(),
            base_dir: None,
            source,
            fn_name_hint: None,
        };
        c.push_function(0, Some("<repl>".to_string()));
        // Slot 0 = closure placeholder (the REPL frame holds the
        // line's closure here).
        c.current_mut().stack_height = 1;
        c.declare_local("", Span::new(0, 0, 1))?;

        // Pre-declare the REPL's accumulated locals at their slots.
        // The runtime stack at slots 1..=M holds the actual values
        // from prior lines — the compiler just needs name → slot to
        // resolve identifiers correctly.
        for (name, slot) in existing_locals {
            c.current_mut().stack_height = (*slot as u32) + 1;
            c.declare_local_at(name, *slot, Span::new(0, 0, 1))?;
        }
        c.current_mut().stack_height =
            1 + existing_locals.len() as u32;

        // The number of locals before compiling this line — anything
        // pushed past this is "new" and will be reported back.
        let pre_locals = c.current().locals.len();

        // Same hoisting story as the standard entry.
        let mut hoisted = Vec::new();
        c.visit_block_for_hoist(program, &mut hoisted);
        c.emit_hoist_prologue(hoisted, Span::new(0, 0, 1))?;

        c.compile_block_value(program, false)?;
        let last_line = c.current_chunk().lines.last().copied().unwrap_or(1);
        c.current_chunk_mut().write_op(OpCode::Halt, last_line);

        let mut fc = c.funcs.pop().expect("repl function compiler popped");
        fc.chunk.thread_jumps();

        // Collect the new top-level locals (skip closure placeholder
        // and existing). Every local the line declares in the REPL's
        // top-level scope persists on the stack between lines — the
        // scope never closes — so all of them must be reported, not
        // just the user-nameable ones. In particular a destructuring
        // decl (`${x} := ...`) leaves an anonymous slot holding the
        // source value below the bound names; omitting it desyncs the
        // REPL's `snapshot_len`, which then truncates real bindings on
        // the next uncaught error. (Sub-scope temporaries — `$for_*`
        // and friends — are already gone from `locals` via end_scope.)
        let new_locals: Vec<(String, u8)> = fc
            .locals
            .iter()
            .skip(pre_locals)
            .map(|l| (l.name.clone(), l.slot))
            .collect();

        Ok((
            Function {
                arity: 0,
                has_rest: false,
                chunk: fc.chunk,
                upvalues: fc.upvalues,
                name: fc.name,
                is_generator: false,
            },
            new_locals,
        ))
        })()
        .map_err(|mut e: CompileError| {
            if e.source.is_unknown() { e.source = source; }
            e
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
        self.funcs.push(FuncCompiler::new(
            arity,
            name,
            self.source,
            self.base_dir.clone(),
        ));
    }

    // -- block / scope -----------------------------------------------

    fn compile_block_value(
        &mut self,
        block: &Block,
        tail: bool,
    ) -> Result<(), CompileError> {
        for stmt in &block.stmts {
            self.compile_expr(stmt)?;
            self.emit_op(OpCode::Pop, stmt.span.line);
        }
        if let Some(t) = &block.tail {
            self.compile_maybe_tail(t, tail)?;
        } else {
            let line = block.stmts.last().map(|s| s.span.line).unwrap_or(1);
            self.emit_op(OpCode::PushNull, line);
        }
        Ok(())
    }

    /// Compile `e` in tail position. A non-spread `Call` here is
    /// emitted as `TailCall` — the VM reuses the current frame instead
    /// of pushing one, so tail recursion runs in O(1) frames. Tail-ness
    /// propagates through `if`/`else`, `match` arms and block tail
    /// expressions; any other form — and a spread call — is compiled
    /// normally (an ordinary `Call`).
    fn compile_tail(&mut self, e: &SpannedExpr) -> Result<(), CompileError> {
        let line = e.span.line;
        match &e.expr {
            Expr::Call(callee, args)
                if !args.iter().any(|a| matches!(a.expr, Expr::Spread(_))) =>
            {
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
                self.emit_op(OpCode::TailCall, line);
                self.emit_byte(args.len() as u8, line);
                // Same net stack effect as `Call`: pops callee + n args,
                // leaves one result.
                self.adjust_stack(-(args.len() as i32 + 1) + 1);
            }
            Expr::If(cond, then_branch, else_branch) => {
                self.compile_if(cond, then_branch, else_branch, line, true)?;
            }
            Expr::Match { subject, arms } => {
                self.compile_match(subject, arms, e.span, true)?;
            }
            Expr::Block(b) => self.compile_block_value(b, true)?,
            Expr::Scope(b) => {
                self.begin_scope();
                let mut hoisted = Vec::new();
                self.visit_block_for_hoist(b, &mut hoisted);
                self.emit_hoist_prologue(hoisted, e.span)?;
                self.compile_block_value(b, true)?;
                self.end_scope(line)?;
            }
            _ => self.compile_expr(e)?,
        }
        Ok(())
    }

    fn compile_maybe_tail(
        &mut self,
        e: &SpannedExpr,
        tail: bool,
    ) -> Result<(), CompileError> {
        if tail {
            self.compile_tail(e)
        } else {
            self.compile_expr(e)
        }
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
            | OpCode::Closure | OpCode::TypeTest => 1,
            // +2
            OpCode::Dup2 => 2,
            // -1: pop one without pushing
            OpCode::Pop => -1,
            // 0: peek / unary in-place / jump / etc.
            OpCode::StoreLocal | OpCode::SetUpvalue
            | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::JumpIfNotNull
            | OpCode::Jump | OpCode::Loop
            | OpCode::Negate | OpCode::Not | OpCode::Len | OpCode::BitNot
            | OpCode::Import | OpCode::MakeIter
            | OpCode::PushTry | OpCode::PopTry
            // Spawn pops the function and pushes a Task — net 0.
            | OpCode::Spawn
            // Go pops the function and pushes `null` — net 0.
            // Yield pops the yielded value and pushes the resume
            // value — net 0. Resume pops a generator handle and pushes
            // the `${ done, value }` result — net 0; the compiler
            // never emits it directly (it lives in a synthetic chunk),
            // but the match must stay exhaustive.
            | OpCode::Go | OpCode::Yield | OpCode::Resume
            // NoMatchError pops nothing and always raises; the runtime
            // never reaches code after it. Tracked as 0.
            | OpCode::NoMatchError => 0,
            // Raise pops its message. The runtime never reaches code
            // after Raise; the compiler still tracks -1 here for
            // consistency, and the surrounding `raise` compilation
            // calls `set_stack_height` to override afterwards.
            OpCode::Raise => -1,
            // Halt pops the line value before exiting. Tracked as -1
            // here for the rare case downstream emission needs the
            // post-Halt height (none currently — Halt is end-of-chunk
            // for REPL lines).
            OpCode::Halt => -1,
            // -1: pop two, push one (typical binop / index / extend)
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div
            | OpCode::Mod | OpCode::Pow | OpCode::Eq | OpCode::Neq
            | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge
            | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor
            | OpCode::Shl | OpCode::Shr
            | OpCode::IndexGet | OpCode::ArrayPush | OpCode::ArrayExtend
            | OpCode::ObjectMerge | OpCode::SliceFrom | OpCode::ObjRest
            | OpCode::IterAppend | OpCode::CallSpread
            | OpCode::AddAssign => -1,
            // -2: IndexSet pops collection/key/value, pushes value back
            OpCode::IndexSet => -2,
            // Operand-dependent: caller adjusts.
            OpCode::MakeArray | OpCode::MakeObject | OpCode::Call
            | OpCode::TailCall
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
        let konst = Const::from_value(&value);
        let key = ConstKey::of(&konst);
        let idx: u16 = match self.current().const_dedup.get(&key) {
            Some(&i) => i,
            None => {
                let i = self
                    .current_chunk_mut()
                    .add_constant(konst)
                    .map_err(|_| {
                        CompileError::new(CompileErrorKind::TooManyConstants, span)
                    })?;
                self.current_mut().const_dedup.insert(key, i);
                i
            }
        };
        self.emit_op(OpCode::LoadConst, line);
        self.current_chunk_mut().write_u16(idx, line);
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
            Expr::Decl(pat, init) => {
                // Hoist EVERY leaf name introduced by this Decl. For
                // a bare `name :=`, that's just `name`. For an
                // array/object pattern `[a, b] :=` or `${x, y} :=`
                // we collect each leaf so the mid-expression slot
                // accounting works out (v0.4 phase 3b — closes the
                // limitation noted in v02-stack-tracking).
                pat.leaf_names(out);
                self.visit_for_hoist(init, out);
            }

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
            | Expr::While { .. } | Expr::For { .. }
            | Expr::Match { .. } => {}

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
            Expr::Raise(v) => self.visit_for_hoist(v, out),
            Expr::Spawn(v) => self.visit_for_hoist(v, out),
            Expr::Go(v) => self.visit_for_hoist(v, out),
            Expr::Yield(v) => {
                if let Some(v) = v { self.visit_for_hoist(v, out); }
            }
            // `try ... catch (e) { handler }` — the handler is a
            // `Scope` per grammar (handles its own hoisting). We only
            // scan the protected body, treating it like any other
            // inline expression. A `:=` directly inside `try expr`
            // (no scope) gets hoisted to the enclosing scope so its
            // slot survives the surrounding op.
            Expr::Try { body, catch: _ } => {
                self.visit_for_hoist(body, out);
            }
            Expr::Assign(_, _, v) => self.visit_for_hoist(v, out),
            Expr::AssignPattern(_, v) => self.visit_for_hoist(v, out),
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
            Expr::Import(inner) => self.visit_for_hoist(inner, out),
            // Leaves
            Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_)
            | Expr::Null | Expr::Ident(_) | Expr::Continue => {}
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
                // Top-level Decls of any pattern shape keep their
                // declare-after-init semantics so the variable's scope
                // starts at its source location. Only their inits are
                // scanned for nested hoist candidates.
                Expr::Decl(_, init) => self.visit_for_hoist(init, out),
                _ => self.visit_for_hoist(stmt, out),
            }
        }
        if let Some(tail) = &block.tail {
            match &tail.expr {
                Expr::Decl(_, init) => self.visit_for_hoist(init, out),
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
                    UnOp::BitNot => OpCode::BitNot,
                };
                self.emit_op(opcode, line);
            }

            Expr::Block(b) => self.compile_block_value(b, false)?,

            Expr::Scope(b) => {
                self.begin_scope();
                let mut hoisted = Vec::new();
                self.visit_block_for_hoist(b, &mut hoisted);
                self.emit_hoist_prologue(hoisted, e.span)?;
                self.compile_block_value(b, false)?;
                self.end_scope(line)?;
            }

            Expr::If(cond, then_branch, else_branch) => {
                self.compile_if(cond, then_branch, else_branch, line, false)?;
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

            Expr::Continue => {
                self.compile_continue(line, e.span)?;
            }

            Expr::Try { body, catch } => {
                self.compile_try(body, catch.as_ref(), line, e.span)?;
            }

            Expr::Raise(value) => {
                self.compile_raise(value, line)?;
            }

            Expr::Spawn(callee) => {
                self.compile_spawn(callee, line)?;
            }

            Expr::Go(callee) => {
                self.compile_go(callee, line)?;
            }

            Expr::Yield(value) => {
                self.compile_yield(value.as_deref(), line)?;
            }

            Expr::Match { subject, arms } => {
                self.compile_match(subject, arms, e.span, false)?;
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
                        // Name the function after the binding it
                        // initialises, for stack traces. `compile_fn`
                        // takes the hint; harmless if `init` is not a
                        // bare `fn` literal.
                        if matches!(init.expr, Expr::Fn { .. }) {
                            self.fn_name_hint = Some(name.clone());
                        }
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
                        // Mid-expression patterns get their leaves
                        // pre-allocated by the scope's hoist prologue.
                        // Detect that and route through
                        // `compile_assign_pattern` so we Store into
                        // the reserved slots instead of declaring
                        // fresh ones (which would clobber the
                        // hoisted slots). Top-level pattern Decls
                        // aren't hoisted (see `visit_block_for_hoist`)
                        // and use the original declare-after-init
                        // shape.
                        let mut leaves = Vec::new();
                        pat.leaf_names(&mut leaves);
                        let hoisted = leaves
                            .first()
                            .map(|n| self.lookup_hoisted(n).is_some())
                            .unwrap_or(false);
                        if hoisted {
                            self.compile_expr(init)?;
                            // One copy survives as the Decl-expr's
                            // value; the other gets destructured.
                            self.emit_op(OpCode::Dup, line);
                            self.compile_assign_pattern(pat, e.span)?;
                        } else {
                            self.compile_expr(init)?;
                            self.compile_pattern(pat, e.span)?;
                            self.emit_op(OpCode::PushNull, line);
                        }
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
                    self.emit_op(compound_to_opcode(*op), line);
                } else {
                    self.compile_expr(value)?;
                }
                self.emit_store(r, line);
            }

            Expr::AssignPattern(pat, value) => {
                // Compile rhs → source on top. Dup so one copy stays
                // as the assign-expression's value while the other
                // gets consumed by the destructure.
                self.compile_expr(value)?;
                self.emit_op(OpCode::Dup, line);
                self.compile_assign_pattern(pat, e.span)?;
                // After compile_assign_pattern, the duplicate has
                // been consumed; the original rhs is on top.
            }

            Expr::Array(items) => {
                let has_spread = items.iter()
                    .any(|i| matches!(i.expr, Expr::Spread(_)));
                if !has_spread && items.len() <= 255 {
                    // Fast path: contiguous element pushes + MakeArray.
                    for item in items {
                        self.compile_expr(item)?;
                    }
                    self.emit_op(OpCode::MakeArray, line);
                    self.emit_byte(items.len() as u8, line);
                    // MakeArray n: pops n, pushes 1.
                    self.adjust_stack(-(items.len() as i32) + 1);
                } else {
                    // Build incrementally: spread elements are
                    // runtime-counted, and a literal with more than 255
                    // elements exceeds MakeArray's u8 count operand.
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
                if !has_spread && members.len() <= 255 {
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
                    // Also taken when a literal has more than 255 pairs
                    // (exceeding MakeObject's u8 count operand).
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
                    self.emit_op(compound_to_opcode(*op), line);
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

            Expr::Fn { params, defaults, rest, body, is_generator } => {
                self.compile_fn(
                    params,
                    defaults,
                    rest.as_deref(),
                    body,
                    *is_generator,
                    e.span,
                )?
            }

            Expr::Import(path) => self.compile_import(path, line)?,

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
        tail: bool,
    ) -> Result<(), CompileError> {
        self.compile_expr(cond)?;
        // Height with cond on top — this is also the height at the
        // `to_else` jump's landing point (JumpIfFalse peeks).
        let cond_height = self.current().stack_height;
        let to_else = self.emit_jump(OpCode::JumpIfFalse, line);
        self.emit_op(OpCode::Pop, line);
        // When the `if` is itself in tail position, each branch is too.
        self.compile_maybe_tail(then_branch, tail)?;
        let to_end = self.emit_jump(OpCode::Jump, line);
        // Reset tracker for the else-path entry.
        self.set_stack_height(cond_height);
        self.patch_jump(to_else)?;
        self.emit_op(OpCode::Pop, line);
        self.compile_maybe_tail(else_branch, tail)?;
        self.patch_jump(to_end)?;
        Ok(())
    }

    /// `match` expression (v0.5). Lowering — the subject lives in an
    /// anonymous slot `S`, the running result in `$match_result` (`R`,
    /// default `null`). Each arm runs its own scope; a refutable test
    /// `JumpIfFalse`s to the arm's fail label on mismatch, where
    /// `Unwind base_height` clears any partial pattern slots before the
    /// next arm. A matched arm stores its body value into `R` and
    /// jumps to the end. If no arm matches, `R` is still `null`.
    /// A `match` is provably exhaustive iff its last arm is an
    /// unguarded irrefutable pattern — a bare `Wildcard` or `Binding`.
    /// A guard can fail, so a guarded arm never makes the match
    /// exhaustive. Conservative: an `Or` of wildcards is not treated
    /// as exhaustive here (it would only suppress a dead opcode).
    fn match_is_exhaustive(arms: &[MatchArm]) -> bool {
        match arms.last() {
            Some(arm) if arm.guard.is_none() => matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            ),
            _ => false,
        }
    }

    fn compile_match(
        &mut self,
        subject: &SpannedExpr,
        arms: &[MatchArm],
        span: Span,
        tail: bool,
    ) -> Result<(), CompileError> {
        let line = span.line;
        self.begin_scope();

        // Subject: hoist any mid-expression `:=` it contains, then
        // compile it into the anonymous subject slot S.
        let mut subj_hoist = Vec::new();
        self.visit_for_hoist(subject, &mut subj_hoist);
        self.emit_hoist_prologue(subj_hoist, span)?;
        self.compile_expr(subject)?;
        self.declare_local("", span)?;
        let subject_slot = self.current().locals.last().unwrap().slot;

        // Result slot R — the value of a match with no matching arm.
        self.emit_op(OpCode::PushNull, line);
        self.declare_local("$match_result", span)?;
        let result_slot = self.current().locals.last().unwrap().slot;

        let base_height = self.current().stack_height;
        let mut end_jumps: Vec<usize> = Vec::new();

        for arm in arms {
            self.begin_scope();
            let arm_depth = self.current().scope_depth;

            // Hoist mid-expression `:=` decls in the guard and body.
            let mut arm_hoist = Vec::new();
            if let Some(g) = &arm.guard {
                self.visit_for_hoist(g, &mut arm_hoist);
            }
            self.visit_for_hoist(&arm.body, &mut arm_hoist);
            self.emit_hoist_prologue(arm_hoist, span)?;

            // Refutable test — failures jump to this arm's fail label.
            let mut fail_jumps: Vec<usize> = Vec::new();
            self.compile_match_test(&arm.pattern, subject_slot, &mut fail_jumps, span)?;

            // Optional guard.
            if let Some(g) = &arm.guard {
                self.compile_expr(g)?;
                fail_jumps.push(self.emit_jump(OpCode::JumpIfFalse, line));
                self.emit_op(OpCode::Pop, line);
            }

            // Body → result slot. When the `match` is in tail position
            // each arm body is too; a `TailCall` there reuses the frame
            // and never reaches the store/unwind below (dead but
            // harmless bytecode).
            self.compile_maybe_tail(&arm.body, tail)?;
            self.emit_op(OpCode::StoreLocal, line);
            self.emit_byte(result_slot, line);
            self.emit_op(OpCode::Pop, line);

            // Success teardown: drop the arm's runtime locals.
            self.emit_op(OpCode::Unwind, line);
            self.emit_byte(base_height as u8, line);
            self.set_stack_height(base_height);
            end_jumps.push(self.emit_jump(OpCode::Jump, line));

            // Compiler-side teardown of the arm scope. Done once — the
            // fail path below is bytecode only (CloseScope would keep a
            // top value, which we don't have here, so unwind manually —
            // same shape as `compile_for`'s per-iteration teardown).
            while let Some(l) = self.current().locals.last() {
                if l.depth < arm_depth { break; }
                self.current_mut().locals.pop();
            }
            self.current_mut().scope_depth -= 1;
            self.current_mut().hoisted_scopes.pop();

            // Fail label: `Unwind` truncates the runtime stack (clearing
            // partial pattern slots and the peeked test bool) so the
            // next arm starts clean.
            for j in fail_jumps {
                self.patch_jump(j)?;
            }
            self.emit_op(OpCode::Unwind, line);
            self.emit_byte(base_height as u8, line);
            self.set_stack_height(base_height);
        }

        // Fall-through: no arm matched. Unless the match is provably
        // exhaustive (an unguarded wildcard / binding last arm), raise
        // a catchable `no_match` error rather than yielding `null`.
        // A successful arm jumps past this point straight to match_end.
        if !Self::match_is_exhaustive(arms) {
            self.emit_op(OpCode::NoMatchError, line);
        }

        // match_end: every successful arm lands here.
        for j in end_jumps {
            self.patch_jump(j)?;
        }
        self.emit_op(OpCode::LoadLocal, line);
        self.emit_byte(result_slot, line);
        self.end_scope(line)?;
        Ok(())
    }

    /// Emit the refutable test for one match pattern. The value under
    /// test lives at `src_slot`. Each sub-check appends a `JumpIfFalse`
    /// patch site to `fail_jumps`; on the fall-through (match) path the
    /// stack is left as it started plus one local per bound name.
    fn compile_match_test(
        &mut self,
        pat: &MatchPattern,
        src_slot: u8,
        fail_jumps: &mut Vec<usize>,
        span: Span,
    ) -> Result<(), CompileError> {
        let line = span.line;
        match pat {
            MatchPattern::Wildcard => {}
            MatchPattern::Binding(name) => {
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(src_slot, line);
                self.declare_local(name, span)?;
            }
            MatchPattern::Literal(lit) => {
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(src_slot, line);
                self.emit_constant(literal_pat_value(lit), line, span)?;
                self.emit_op(OpCode::Eq, line);
                fail_jumps.push(self.emit_jump(OpCode::JumpIfFalse, line));
                self.emit_op(OpCode::Pop, line);
            }
            MatchPattern::Range { from, to, inclusive } => {
                // Subject must be a number (tag 8 = Int|Float).
                self.emit_type_test(src_slot, 8, fail_jumps, line);
                // from <= subject
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(src_slot, line);
                self.emit_constant(literal_pat_value(from), line, span)?;
                self.emit_op(OpCode::Ge, line);
                fail_jumps.push(self.emit_jump(OpCode::JumpIfFalse, line));
                self.emit_op(OpCode::Pop, line);
                // subject < to  (or <= to when inclusive)
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(src_slot, line);
                self.emit_constant(literal_pat_value(to), line, span)?;
                self.emit_op(if *inclusive { OpCode::Le } else { OpCode::Lt }, line);
                fail_jumps.push(self.emit_jump(OpCode::JumpIfFalse, line));
                self.emit_op(OpCode::Pop, line);
            }
            MatchPattern::Array { items, rest } => {
                self.emit_type_test(src_slot, 4, fail_jumps, line); // 4 = Array
                // Length: exact when no rest, `>=` when there is one.
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(src_slot, line);
                self.emit_op(OpCode::Len, line);
                self.emit_constant(Value::Int(items.len() as i64), line, span)?;
                self.emit_op(
                    if rest.is_some() { OpCode::Ge } else { OpCode::Eq },
                    line,
                );
                fail_jumps.push(self.emit_jump(OpCode::JumpIfFalse, line));
                self.emit_op(OpCode::Pop, line);
                for (i, item) in items.iter().enumerate() {
                    if matches!(item, MatchPattern::Wildcard) {
                        continue;
                    }
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Int(i as i64), line, span)?;
                    self.emit_op(OpCode::IndexGet, line);
                    self.declare_local("", span)?;
                    let elem_slot = self.current().locals.last().unwrap().slot;
                    self.compile_match_test(item, elem_slot, fail_jumps, span)?;
                }
                if let Some(rest_name) = rest {
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Int(items.len() as i64), line, span)?;
                    self.emit_op(OpCode::SliceFrom, line);
                    self.declare_local(rest_name, span)?;
                }
            }
            MatchPattern::Object { fields, rest } => {
                self.emit_type_test(src_slot, 5, fail_jumps, line); // 5 = Object
                for f in fields {
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Str(f.key.as_str().into()), line, span)?;
                    self.emit_op(OpCode::IndexGet, line);
                    match &f.pattern {
                        // Shorthand `${name}` — bind the value. A
                        // missing key reads `null` (does not fail).
                        None => self.declare_local(&f.key, span)?,
                        Some(sub) => {
                            self.declare_local("", span)?;
                            let fslot = self.current().locals.last().unwrap().slot;
                            self.compile_match_test(sub, fslot, fail_jumps, span)?;
                        }
                    }
                }
                if let Some(rest_name) = rest {
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
            MatchPattern::Or(alts) => {
                // v0.5: or-pattern alternatives must be non-binding and
                // stack-neutral (literal / range / `_`). This sidesteps
                // slot reconciliation across the converging branches.
                for alt in alts {
                    if !matches!(
                        alt,
                        MatchPattern::Literal(_)
                            | MatchPattern::Range { .. }
                            | MatchPattern::Wildcard
                    ) {
                        return Err(CompileError::new(
                            CompileErrorKind::InvalidMatchPattern(
                                "or-pattern alternatives must be literals, ranges, \
                                 or `_` — no bindings or structural patterns"
                                    .into(),
                            ),
                            span,
                        ));
                    }
                }
                let last = alts.len() - 1;
                let mut matched_jumps: Vec<usize> = Vec::new();
                for (i, alt) in alts.iter().enumerate() {
                    if i < last {
                        let mut local_fail: Vec<usize> = Vec::new();
                        self.compile_match_test(alt, src_slot, &mut local_fail, span)?;
                        matched_jumps.push(self.emit_jump(OpCode::Jump, line));
                        for j in local_fail {
                            self.patch_jump(j)?;
                        }
                    } else {
                        self.compile_match_test(alt, src_slot, fail_jumps, span)?;
                    }
                }
                for j in matched_jumps {
                    self.patch_jump(j)?;
                }
            }
        }
        Ok(())
    }

    /// Emit a non-raising runtime type check on the value at `src_slot`.
    /// Pushes nothing net; on a type mismatch jumps via `fail_jumps`.
    fn emit_type_test(
        &mut self,
        src_slot: u8,
        tag: u8,
        fail_jumps: &mut Vec<usize>,
        line: u32,
    ) {
        self.emit_op(OpCode::LoadLocal, line);
        self.emit_byte(src_slot, line);
        self.emit_op(OpCode::TypeTest, line);
        self.emit_byte(tag, line);
        fail_jumps.push(self.emit_jump(OpCode::JumpIfFalse, line));
        self.emit_op(OpCode::Pop, line); // the test bool
        self.emit_op(OpCode::Pop, line); // the loaded src copy
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

        let loop_start = self.current_chunk().code.len();
        self.current_mut().loop_stack.push(LoopCtx {
            result_slot,
            base_stack_height,
            is_array_form: is_array,
            exit_jumps: Vec::new(),
            skip: false,
            loop_start,
        });

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

        let loop_start = self.current_chunk().code.len();
        self.current_mut().loop_stack.push(LoopCtx {
            result_slot,
            base_stack_height,
            is_array_form: is_array,
            exit_jumps: Vec::new(),
            skip: false,
            loop_start,
        });

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

        // Read `is_array_form` up front: a bare `break` in an array
        // loop must emit nothing at all (it appends no item), so we
        // need this before deciding whether to push a value.
        let is_array_form = self.current().loop_stack[target].is_array_form;
        let has_value = value.is_some();

        // While we compile the value, mark target as skipped so any
        // nested break inside the value redirects past this loop.
        self.current_mut().loop_stack[target].skip = true;
        if let Some(v) = value {
            self.compile_expr(v)?;
        } else if !is_array_form {
            // Plain-form bare `break`: the loop's value is `null`.
            self.emit_op(OpCode::PushNull, line);
        }
        // Array-form bare `break`: nothing pushed, nothing appended.
        self.current_mut().loop_stack[target].skip = false;

        let ctx = &self.current().loop_stack[target];
        let result_slot = ctx.result_slot;
        let base_stack_height = ctx.base_stack_height;

        if is_array_form {
            // `break <value>` appends the value verbatim (incl. `null`);
            // bare `break` appends nothing.
            if has_value {
                self.emit_op(OpCode::IterAppend, line);
                self.emit_byte(result_slot, line);
            }
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

    /// `continue` lowering. In an array-collecting loop the iteration
    /// contributes nothing to the result; in a plain loop its value
    /// becomes `null`. Either way nested locals are unwound, then
    /// control jumps *backward* to the loop head rather than forward
    /// to the exit. For
    /// `for` the head is `IterNext` (which re-reads the still-live
    /// `$for_iter` IterState below `base_stack_height`); for `while` it
    /// is the condition re-evaluation.
    fn compile_continue(&mut self, line: u32, span: Span) -> Result<(), CompileError> {
        let entry_height = self.current().stack_height;
        let target = self
            .current()
            .loop_stack
            .iter()
            .rposition(|lc| !lc.skip)
            .ok_or_else(|| {
                CompileError::new(CompileErrorKind::ContinueOutsideLoop, span)
            })?;

        let ctx = &self.current().loop_stack[target];
        let result_slot = ctx.result_slot;
        let base_stack_height = ctx.base_stack_height;
        let is_array_form = ctx.is_array_form;
        let loop_start = ctx.loop_start;

        if is_array_form {
            // Array form: `continue` contributes nothing to the result
            // array — it is the only way to omit an item from a
            // `for[]` / `while[]`.
        } else {
            // Plain form: this iteration's value becomes `null`.
            self.emit_op(OpCode::PushNull, line);
            self.emit_op(OpCode::StoreLocal, line);
            self.emit_byte(result_slot, line);
            self.emit_op(OpCode::Pop, line);
        }
        self.emit_op(OpCode::Unwind, line);
        self.emit_byte(base_stack_height as u8, line);
        self.emit_loop(loop_start, line)?;
        // Like `break`: code after the backward jump is unreachable;
        // restore the tracker to the surrounding expression's +1.
        self.set_stack_height(entry_height + 1);
        Ok(())
    }

    /// `try` / `catch` lowering (spec §9.6).
    ///
    /// Success path:
    /// ```text
    ///   push_try catch_pc        ; snapshot stack length
    ///   <body>                   ; pushes 1
    ///   pop_try                  ; success — discard the try-frame
    ///   jump end
    /// catch_pc:                  ; reached only via Raise — at this
    ///                            ; point the VM has truncated stack
    ///                            ; to the snapshot and pushed the
    ///                            ; error string
    ///   (no-catch)  pop          ; drop error
    ///               push_null    ; produce null
    ///   (catch)     <handler>    ; runs in a scope with `param` bound
    ///                            ; to the error
    /// end:                       ; either path leaves +1 on the stack
    /// ```
    fn compile_try(
        &mut self,
        body: &SpannedExpr,
        catch: Option<&(String, Box<SpannedExpr>)>,
        line: u32,
        span: Span,
    ) -> Result<(), CompileError> {
        // Snapshot AT the PushTry — this is the stack height the VM
        // restores to before pushing the error.
        let push_try_at = self.emit_jump(OpCode::PushTry, line);

        self.compile_expr(body)?;
        let success_height = self.current().stack_height;

        self.emit_op(OpCode::PopTry, line);
        let end_jump = self.emit_jump(OpCode::Jump, line);

        // Catch path. PushTry's target lands here; VM has already
        // pushed the error value, so reset the tracker accordingly.
        self.patch_jump(push_try_at)?;
        // success_height = snapshot + 1 (from <body>); catch path also
        // leaves snapshot + 1 (the error value the VM pushed). Use
        // success_height directly.
        self.set_stack_height(success_height);

        if let Some((param, handler)) = catch {
            // Declare `param` so the handler scope sees the error
            // value as `param`. Use a real scope so the binding goes
            // out of scope after the handler, and CloseScope drops
            // it while preserving the handler's value.
            self.begin_scope();
            self.declare_local(param, span)?;
            self.compile_expr(handler)?;
            self.end_scope(line)?;
        } else {
            // No catch: discard the error, produce null.
            self.emit_op(OpCode::Pop, line);
            self.emit_op(OpCode::PushNull, line);
        }

        self.patch_jump(end_jump)?;
        // Both arms converge at the success height.
        self.set_stack_height(success_height);
        Ok(())
    }

    /// `raise` lowering (spec §9.6). Compiles the value, emits Raise.
    /// The runtime never reaches code after Raise; we reset the
    /// compiler's stack tracker to "+1 over entry" so downstream
    /// emission stays consistent (mirrors how `break` is handled).
    fn compile_raise(
        &mut self,
        value: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        let entry_height = self.current().stack_height;
        self.compile_expr(value)?;
        self.emit_op(OpCode::Raise, line);
        self.set_stack_height(entry_height + 1);
        Ok(())
    }

    /// `spawn callee` (v0.14) — compile the callee expression, then
    /// `Spawn` pops that function and pushes a `Task`. Net stack
    /// effect is zero, so the result height is `entry + 1`.
    fn compile_spawn(
        &mut self,
        callee: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        let entry_height = self.current().stack_height;
        self.compile_expr(callee)?;
        self.emit_op(OpCode::Spawn, line);
        self.set_stack_height(entry_height + 1);
        Ok(())
    }

    /// `go callee` — compile the callee expression, then `Go` pops
    /// that function and pushes `null`. Net stack effect is zero.
    fn compile_go(
        &mut self,
        callee: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        let entry_height = self.current().stack_height;
        self.compile_expr(callee)?;
        self.emit_op(OpCode::Go, line);
        self.set_stack_height(entry_height + 1);
        Ok(())
    }

    /// `yield expr` / `yield` — push the yielded value (`null` when
    /// omitted), then `Yield` pops it and later pushes the resume
    /// value. Net stack effect is zero.
    fn compile_yield(
        &mut self,
        value: Option<&SpannedExpr>,
        line: u32,
    ) -> Result<(), CompileError> {
        let entry_height = self.current().stack_height;
        match value {
            Some(v) => self.compile_expr(v)?,
            None => self.emit_op(OpCode::PushNull, line),
        }
        self.emit_op(OpCode::Yield, line);
        self.set_stack_height(entry_height + 1);
        Ok(())
    }

    // -- function literal --------------------------------------------

    fn compile_fn(
        &mut self,
        params: &[Pattern],
        defaults: &[Option<Box<SpannedExpr>>],
        rest: Option<&str>,
        body: &SpannedExpr,
        is_generator: bool,
        span: Span,
    ) -> Result<(), CompileError> {
        let line = span.line;
        // Take the binding-name hint set by the `Decl` handler, if any;
        // an unbound `fn` (or a nested one) gets `None` → `<anonymous>`.
        let name = self.fn_name_hint.take();
        self.push_function(params.len(), name);

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

        // Default parameter values: where an argument slot holds
        // `null` (missing or explicitly passed `null`), evaluate the
        // default and store it. Each block is runtime stack-neutral —
        // both the not-null and null paths converge at `skip` with the
        // loaded value on top, which the trailing `Pop` discards.
        for (i, default) in defaults.iter().enumerate() {
            if let Some(default) = default {
                let slot = (i + 1) as u8;
                self.emit_op(OpCode::LoadLocal, line);
                self.emit_byte(slot, line);
                let skip = self.emit_jump(OpCode::JumpIfNotNull, line);
                self.compile_expr(default)?;
                self.emit_op(OpCode::StoreLocal, line);
                self.emit_byte(slot, line);
                self.emit_op(OpCode::Pop, line);
                self.patch_jump(skip)?;
                self.emit_op(OpCode::Pop, line);
            }
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

        // body is always a Scope (per parser). Compile it in tail
        // position so a call as the function's result becomes a
        // frame-reusing `TailCall`.
        self.compile_tail(body)?;

        // implicit Return at the end (dead code when the body ends in a
        // TailCall, but harmless)
        self.emit_op(OpCode::Return, line);

        let mut fc = self.funcs.pop().unwrap();
        fc.chunk.thread_jumps();
        let upvalues_info = fc.upvalues.clone();
        let function = Function {
            arity: fc.arity,
            has_rest: rest.is_some(),
            chunk: fc.chunk,
            upvalues: fc.upvalues,
            name: fc.name,
            is_generator,
        };

        let idx = self
            .current_chunk_mut()
            .add_function(Arc::new(function))
            .map_err(|_| CompileError::new(CompileErrorKind::TooManyConstants, span))?;
        self.emit_op(OpCode::Closure, line);
        self.current_chunk_mut().write_u16(idx, line);
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
    /// Destructure the value on top of stack into EXISTING bindings
    /// (mirror of [`compile_pattern`] which DECLARES them). Each leaf
    /// `Ident` resolves to its slot and is `StoreLocal`'d; rest binds
    /// the same way. Net stack effect: -1 (consumes the source).
    fn compile_assign_pattern(
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
                let r = self.resolve(name, span)?.ok_or_else(|| {
                    CompileError::new(
                        CompileErrorKind::UndeclaredAssign(name.clone()),
                        span,
                    )
                })?;
                if let Resolved::Global(_) = r {
                    return Err(CompileError::new(
                        CompileErrorKind::AssignToBuiltin(name.clone()),
                        span,
                    ));
                }
                self.emit_store(r, line);
                self.emit_op(OpCode::Pop, line);
            }
            Pattern::Array { items, rest } => {
                // Stash source as anonymous local so we can index into
                // it repeatedly. Reclaimed at the bottom of this arm.
                self.declare_local("", span)?;
                let src_slot = self.current().locals.last().unwrap().slot;
                for (i, item) in items.iter().enumerate() {
                    if matches!(item, Pattern::Wildcard) {
                        continue;
                    }
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Int(i as i64), line, span)?;
                    self.emit_op(OpCode::IndexGet, line);
                    self.compile_assign_pattern(item, span)?;
                }
                if let Some(rest_name) = rest {
                    self.emit_op(OpCode::LoadLocal, line);
                    self.emit_byte(src_slot, line);
                    self.emit_constant(Value::Int(items.len() as i64), line, span)?;
                    self.emit_op(OpCode::SliceFrom, line);
                    self.assign_leaf_name(rest_name, line, span)?;
                }
                // Drop the anonymous source we stashed: pop the runtime
                // value and un-declare the slot so the per-iteration
                // accounting in any outer Array/Object loop stays
                // balanced (caller expects net -1 from this call).
                self.emit_op(OpCode::Pop, line);
                let popped = self.current_mut().locals.pop()
                    .expect("anonymous source slot");
                debug_assert!(popped.name.is_empty());
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
                    self.compile_assign_pattern(&f.pattern, span)?;
                }
                if let Some(rest_name) = rest {
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
                    self.assign_leaf_name(rest_name, line, span)?;
                }
                self.emit_op(OpCode::Pop, line);
                let popped = self.current_mut().locals.pop()
                    .expect("anonymous source slot");
                debug_assert!(popped.name.is_empty());
            }
        }
        Ok(())
    }

    /// Resolve a leaf identifier to a non-Global slot and emit the
    /// store + pop that consumes the value on top. Used for both
    /// regular leaves (in Pattern::Ident) and rest bindings.
    fn assign_leaf_name(
        &mut self,
        name: &str,
        line: u32,
        span: Span,
    ) -> Result<(), CompileError> {
        let r = self.resolve(name, span)?.ok_or_else(|| {
            CompileError::new(
                CompileErrorKind::UndeclaredAssign(name.to_string()),
                span,
            )
        })?;
        if let Resolved::Global(_) = r {
            return Err(CompileError::new(
                CompileErrorKind::AssignToBuiltin(name.to_string()),
                span,
            ));
        }
        self.emit_store(r, line);
        self.emit_op(OpCode::Pop, line);
        Ok(())
    }

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

    /// Compile an `import expr`: evaluate the operand, then emit
    /// `Import`. The operand must produce a string at runtime; the VM
    /// resolves it (bare module name vs. file path, relative to the
    /// importing chunk's `base_dir`) and reports a catchable error if
    /// it is not a string or the file is missing.
    fn compile_import(
        &mut self,
        path_expr: &SpannedExpr,
        line: u32,
    ) -> Result<(), CompileError> {
        self.compile_expr(path_expr)?;
        self.emit_op(OpCode::Import, line);
        Ok(())
    }

    // -- jump emission / patching ------------------------------------

    fn emit_jump(&mut self, op: OpCode, line: u32) -> usize {
        self.emit_op(op, line);
        let offset = self.current_chunk().code.len();
        self.current_chunk_mut().write_u32(0xffff_ffff, line);
        offset
    }

    fn patch_jump(&mut self, offset: usize) -> Result<(), CompileError> {
        let here = self.current_chunk().code.len();
        let dist = here.checked_sub(offset + 4).ok_or_else(|| {
            CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, 0))
        })?;
        if dist > u32::MAX as usize {
            return Err(CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, 0)));
        }
        self.current_chunk_mut().patch_u32(offset, dist as u32);
        Ok(())
    }

    fn emit_loop(&mut self, target: usize, line: u32) -> Result<(), CompileError> {
        self.emit_op(OpCode::Loop, line);
        let here = self.current_chunk().code.len();
        let dist = (here + 4).checked_sub(target).ok_or_else(|| {
            CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, line))
        })?;
        if dist > u32::MAX as usize {
            return Err(CompileError::new(CompileErrorKind::JumpTooFar, Span::new(0, 0, line)));
        }
        self.current_chunk_mut().write_u32(dist as u32, line);
        Ok(())
    }
}

/// The runtime `Value` a match literal pattern compares against.
fn literal_pat_value(lit: &LiteralPat) -> Value {
    match lit {
        LiteralPat::Int(n) => Value::Int(*n),
        LiteralPat::Float(x) => Value::Float(*x),
        LiteralPat::Str(s) => Value::Str(s.as_str().into()),
        LiteralPat::Bool(b) => Value::Bool(*b),
        LiteralPat::Null => Value::Null,
    }
}

/// Opcode for a compound-assignment operator (`+=`, `-=`, ...). `+=`
/// gets the in-place `AddAssign` so an Array target is mutated rather
/// than rebound; every other operator reuses its plain binary opcode.
fn compound_to_opcode(op: BinOp) -> OpCode {
    match op {
        BinOp::Add => OpCode::AddAssign,
        other => binop_to_opcode(other),
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
        BinOp::BitAnd => OpCode::BitAnd,
        BinOp::BitOr => OpCode::BitOr,
        BinOp::BitXor => OpCode::BitXor,
        BinOp::Shl => OpCode::Shl,
        BinOp::Shr => OpCode::Shr,
        BinOp::And | BinOp::Or => unreachable!("handled by short-circuit lowering"),
    }
}
