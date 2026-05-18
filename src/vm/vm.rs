//! Stack-based bytecode interpreter.
//!
//! Phase 4 model:
//! - The value stack is shared across all call frames; each frame
//!   indexes into it via `base_slot`.
//! - Built-in functions live in `globals` (separate from the stack)
//!   and are accessed via `LoadGlobal`.
//! - Closures carry `Rc<RefCell<Upvalue>>` cells. While a captured
//!   local is still on the stack, the upvalue is `Open(slot)`. When
//!   that local goes out of scope (`CloseScope` pops it, or `Return`
//!   discards the frame), the upvalue is `Closed(value)` — the value
//!   is lifted off the stack onto the heap.
//! - Multiple closures capturing the same slot share the same
//!   `Rc<RefCell<Upvalue>>`, so mutation through one is visible from
//!   the others (counter pattern).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;
use num_bigint::BigInt as BigIntData;
use num_integer::Integer;
use num_traits::{Pow, Zero};

use crate::vm::error::{RuntimeError, RuntimeErrorKind, TraceFrame};
use crate::vm::gc::{
    self, ArrayKind, ClosureKind, GcRef, Marker, ObjectKind, Trace, UpvalueKind,
};
use crate::vm::opcode::OpCode;
use crate::vm::source_map::SourceMap;
use crate::vm::stdlib;
use crate::vm::value::{
    bigint_to_f64, Closure, Function, IterState, MapKey, RangeData, Upvalue, Value,
};

struct CallFrame {
    closure: GcRef<ClosureKind>,
    ip: usize,
    /// Index in `vm.stack` corresponding to slot 0 of this frame.
    base_slot: usize,
    /// Active `try` frames for this call frame (innermost last).
    /// Empty for almost all frames; cheap to keep around.
    try_frames: Vec<TryFrame>,
    /// What kind of frame this is. `Function` for ordinary calls (and
    /// for the top-level program). `Import(path)` when the frame is
    /// evaluating an imported module; on `Return` the resulting value
    /// is cached against `path`. Distinguishing import frames keeps
    /// the cache-write logic localized to `Return` / `try_catch`.
    kind: FrameKind,
}

enum FrameKind {
    Function,
    Import(PathBuf),
    /// REPL session frame. Persistent — never popped, never closed —
    /// so locals declared by prior lines survive between Halts. The
    /// `try_catch` walker treats this frame as a wall so an uncaught
    /// raise from a single line doesn't tear down the whole session.
    Repl,
}

/// Snapshot of state captured at `PushTry`. On a Raise (or runtime
/// error) the VM walks call frames from innermost outward; the first
/// non-empty `try_frames` stack indicates where to land.
struct TryFrame {
    /// Absolute byte offset in the owning frame's chunk to jump to.
    catch_pc: usize,
    /// Absolute index into `vm.stack`: truncate to this length before
    /// pushing the error value. Snapshotted at `PushTry`.
    stack_len: usize,
}

/// Default ceiling on call-frame depth. Recursion past this raises a
/// catchable `stack_overflow` error rather than crashing the process.
/// Bounds both the heap `frames` Vec and — since `call_value` re-entry
/// also pushes frames — the rare deep re-entrant Rust-stack case.
pub const DEFAULT_MAX_CALL_DEPTH: usize = 10_000;

pub struct Vm {
    frames: Vec<CallFrame>,
    /// Ceiling on `frames.len()`; see [`DEFAULT_MAX_CALL_DEPTH`]. Public
    /// so a driver can tune it.
    pub max_call_depth: usize,
    stack: Vec<Value>,
    globals: Vec<Value>,
    open_upvalues: Vec<GcRef<UpvalueKind>>,
    /// Per-Vm cache of `import 'path'` results, keyed by absolute path.
    /// Spec §12 was no-caching in v0.2; v0.3 adds caching so a module
    /// imported twice within the same run evaluates only once.
    module_cache: HashMap<PathBuf, Value>,
    /// Paths currently being evaluated. A second import of any of
    /// these is a circular-import error (catchable via `try`).
    in_flight: HashSet<PathBuf>,
    /// Registry of source files this Vm has touched. Shared with the
    /// driver (entry function, REPL) so error rendering can resolve
    /// snippets after the run returns.
    pub source_map: Rc<RefCell<SourceMap>>,
}

impl Vm {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_source_map(Rc::new(RefCell::new(SourceMap::new())))
    }

    pub fn with_source_map(source_map: Rc<RefCell<SourceMap>>) -> Self {
        Vm {
            frames: Vec::with_capacity(64),
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
            stack: Vec::with_capacity(256),
            globals: stdlib::builtins(),
            open_upvalues: Vec::new(),
            module_cache: HashMap::new(),
            in_flight: HashSet::new(),
            source_map,
        }
    }

    /// Run a compiled top-level program. Returns its final value.
    pub fn run(&mut self, main: Function) -> Result<Value, RuntimeError> {
        self.stack.clear();
        self.frames.clear();
        self.open_upvalues.clear();

        let main_closure = gc::alloc_closure(Closure {
            function: Arc::new(main),
            upvalues: Vec::new(),
        });
        // slot 0 of main frame = the main closure itself
        self.stack.push(Value::Function(main_closure));
        self.frames.push(CallFrame {
            closure: main_closure,
            ip: 0,
            base_slot: 0,
            try_frames: Vec::new(),
            kind: FrameKind::Function,
        });
        // Surface the result of `exec`, with a loop wrapper so a
        // caught raise can re-enter exec at the new ip.
        loop {
            match self.exec() {
                Ok(v) => return Ok(v),
                Err(mut err) => {
                    self.stamp_error_source(&mut err);
                    if !self.try_catch(0, &mut err) {
                        return Err(err);
                    }
                    // Caught — frame state is now pointing at catch_pc
                    // with the error value on the stack. Loop back into
                    // exec to continue from there.
                }
            }
        }
    }

    /// Run an already-built closure as a fresh top-level program,
    /// invoked with no arguments. Used by spawned actors: a worker
    /// thread builds a `Vm`, decodes the closure into its own heap,
    /// and runs it here.
    pub fn run_closure(
        &mut self,
        closure: GcRef<ClosureKind>,
    ) -> Result<Value, RuntimeError> {
        self.stack.clear();
        self.frames.clear();
        self.open_upvalues.clear();
        self.call_value(Value::Function(closure), Vec::new(), 0)
    }

    /// Start `callee` as an actor: deep-copy it across the heap
    /// boundary, run it on a new OS thread, and return a `Task` handle
    /// for its eventual result. Raises `not_callable` if `callee` is
    /// not a function, or `not_sendable`/`cycle` if it (or a captured
    /// value) cannot cross the boundary.
    fn spawn_actor(
        &mut self,
        callee: Value,
        line: u32,
    ) -> Result<crate::vm::task::TaskHandle, RuntimeError> {
        let closure = match callee {
            Value::Function(c) => c,
            other => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::NotCallable(other.type_name().into()),
                    line,
                ));
            }
        };
        // The spawned closure may hold `Open` upvalues pointing into
        // this live frame's stack. Build a detached copy whose every
        // upvalue is `Closed` so the transfer encoder can encode it.
        let (function, cells) = {
            let cl = closure.borrow();
            (cl.function.clone(), cl.upvalues.clone())
        };
        let mut closed = Vec::with_capacity(cells.len());
        for cell in &cells {
            let captured = match &*cell.borrow() {
                Upvalue::Closed(v) => v.clone(),
                Upvalue::Open(slot) => self.stack[*slot].clone(),
            };
            closed.push(gc::alloc_upvalue(Upvalue::Closed(captured)));
        }
        let detached =
            gc::alloc_closure(Closure { function, upvalues: closed });
        let transfer = crate::vm::transfer::encode(&Value::Function(detached))
            .map_err(|mut e| {
                if e.line == 0 {
                    e.line = line;
                }
                e
            })?;

        let task = crate::vm::task::TaskInner::new();
        let task_worker = task.clone();
        std::thread::spawn(move || {
            let outcome = run_actor(transfer);
            task_worker.complete(outcome);
        });
        Ok(task)
    }

    /// Fill in `err.source` from the chunk on top of the call stack
    /// when it isn't already set. Called at the `exec` boundary —
    /// before `try_catch` may unwind frames.
    fn stamp_error_source(&self, err: &mut RuntimeError) {
        if !err.source.is_unknown() {
            return;
        }
        if let Some(top) = self.frames.last() {
            err.source = top.closure.borrow().function.chunk.source;
        }
    }

    /// Walk frames from innermost outward looking for an active
    /// try-frame. If found: pop intermediate frames, close their
    /// upvalues, truncate stack to the recorded length, push the
    /// caught value (a `raise`d value verbatim, or a built-in error
    /// reified as a `${kind, message, line}` object), set the
    /// surviving frame's ip to the catch PC, and return `true`. If no
    /// try-frame anywhere, leave state untouched and return `false`.
    ///
    /// `floor` bounds the search: frames at index `< floor` are never
    /// inspected or popped. The top-level driver passes `0`; a
    /// re-entrant [`call_value`] passes the frame depth it started at,
    /// so a raise the callee does not catch internally unwinds only the
    /// callee's own frames and then propagates to the caller.
    fn try_catch(&mut self, floor: usize, err: &mut RuntimeError) -> bool {
        while self.frames.len() > floor {
            let frame = self.frames.last_mut().unwrap();
            if let Some(tf) = frame.try_frames.pop() {
                let catch_pc = tf.catch_pc;
                let stack_len = tf.stack_len;
                self.close_upvalues(stack_len);
                self.stack.truncate(stack_len);
                // A `raise`d value reaches the handler verbatim; a
                // built-in error is reified into a structured object
                // `${kind, message, line}` so it can be `match`ed.
                let caught = match &err.kind {
                    RuntimeErrorKind::Raised(v) => v.clone(),
                    kind => {
                        let mut m: IndexMap<Rc<str>, Value> =
                            IndexMap::with_capacity(3);
                        m.insert(Rc::from("kind"),
                            Value::Str(kind.kind_tag().into()));
                        m.insert(Rc::from("message"),
                            Value::Str(format!("{err}").into()));
                        m.insert(Rc::from("line"),
                            Value::Int(err.line as i64));
                        Value::Object(gc::alloc_object(m))
                    }
                };
                self.stack.push(caught);
                self.frames.last_mut().unwrap().ip = catch_pc;
                return true;
            }
            // REPL frame is a wall — never popped on uncaught raise.
            // `run_repl_line` truncates the stack to the pre-line
            // snapshot and surfaces the error to the driver.
            if matches!(frame.kind, FrameKind::Repl) {
                return false;
            }
            // No try-frame in this frame — pop it and close upvalues
            // at its base before continuing the search outward.
            let popped = self.frames.pop().unwrap();
            // Record this frame in the (innermost-first) stack trace.
            // The first frame recorded is the faulting one — use the
            // error's precise line; for callers use the call-site line
            // (`ip` sits just past the `Call` operand). The trace rides
            // on `err`; if a handler is found later it is discarded.
            let popped_closure = popped.closure.borrow();
            let func = &popped_closure.function;
            let line = if err.trace.is_empty() {
                err.line
            } else {
                func.chunk
                    .lines
                    .get(popped.ip.saturating_sub(1))
                    .copied()
                    .unwrap_or(0)
            };
            err.trace.push(TraceFrame {
                name: func.name.clone(),
                source: func.chunk.source,
                line,
            });
            self.close_upvalues(popped.base_slot);
            self.stack.truncate(popped.base_slot);
            // If we just abandoned an in-flight import, drop the
            // in-flight marker so subsequent imports of that path can
            // try again (otherwise the cycle-detection set would leak).
            if let FrameKind::Import(path) = popped.kind {
                self.in_flight.remove(&path);
            }
        }
        false
    }

    /// Set up a long-lived REPL frame with a dummy closure. Call once
    /// per session before any [`run_repl_line`].
    pub fn start_repl(&mut self) {
        self.stack.clear();
        self.frames.clear();
        self.open_upvalues.clear();
        let dummy = gc::alloc_closure(Closure {
            function: Arc::new(crate::vm::value::Function {
                arity: 0,
                has_rest: false,
                chunk: crate::vm::chunk::Chunk::new(),
                upvalues: Vec::new(),
                name: Some("<repl>".to_string()),
            }),
            upvalues: Vec::new(),
        });
        // Slot 0 of the REPL frame holds the *currently active*
        // line's closure. `run_repl_line` replaces this each line.
        self.stack.push(Value::Function(dummy));
        self.frames.push(CallFrame {
            closure: dummy,
            ip: 0,
            base_slot: 0,
            try_frames: Vec::new(),
            kind: FrameKind::Repl,
        });
    }

    /// Run one REPL line. The closure's chunk must end in `Halt`.
    /// `snapshot_len` is the stack length the REPL expects after a
    /// successful run (closure slot + existing user locals). On an
    /// uncaught raise the stack is truncated back to this snapshot.
    pub fn run_repl_line(
        &mut self,
        closure: GcRef<ClosureKind>,
        snapshot_len: usize,
    ) -> Result<Value, RuntimeError> {
        debug_assert!(matches!(self.frames[0].kind, FrameKind::Repl));
        // Install the new line's closure at slot 0 and reset ip.
        self.stack[0] = Value::Function(closure);
        self.frames[0].closure = closure;
        self.frames[0].ip = 0;
        self.frames[0].try_frames.clear();
        loop {
            match self.exec() {
                Ok(v) => return Ok(v), // Halt exit
                Err(mut err) => {
                    self.stamp_error_source(&mut err);
                    if !self.try_catch(0, &mut err) {
                        // Wall hit — restore stack to pre-line state.
                        self.close_upvalues(snapshot_len);
                        self.stack.truncate(snapshot_len);
                        self.frames[0].try_frames.clear();
                        self.frames[0].ip = 0;
                        return Err(err);
                    }
                }
            }
        }
    }

    fn exec(&mut self) -> Result<Value, RuntimeError> {
        self.run_until(0)
    }

    /// The bytecode dispatch loop. Runs until the frame stack drops to
    /// `floor` frames (a `Return` from the frame at index `floor`) or a
    /// `Halt`, returning the produced value; an uncaught error returns
    /// `Err` with the frames left in place for the caller to unwind.
    ///
    /// `floor == 0` is the whole-program run. A re-entrant
    /// [`call_value`] passes the depth it started at so the nested run
    /// returns once its callee frame has returned.
    fn run_until(&mut self, floor: usize) -> Result<Value, RuntimeError> {
        loop {
            // GC safepoint: collect here, before any opcode work, while
            // no borrow guard is live and every root is on a Vm field.
            self.maybe_collect();

            // Snapshot the current frame's chunk for this iteration.
            // The closure handle is `Copy`; cloning the `Rc<Function>`
            // out lets us read the chunk while mutating self.stack /
            // self.frames.
            let closure = self.frames.last().expect("at least one frame").closure;
            let function_rc = closure.borrow().function.clone();
            let chunk = &function_rc.chunk;
            let base_slot = self.frames.last().unwrap().base_slot;
            let mut ip = self.frames.last().unwrap().ip;

            if ip >= chunk.code.len() {
                // ran off the end without RETURN — defensive
                return Ok(Value::Null);
            }

            let line = chunk.lines[ip];
            let byte = chunk.code[ip];
            let op = OpCode::from_u8(byte)
                .unwrap_or_else(|| panic!("invalid opcode {byte} at offset {ip}"));
            ip += 1;

            match op {
                OpCode::LoadConst => {
                    let idx = chunk.code[ip] as usize;
                    ip += 1;
                    self.stack.push(chunk.constants[idx].to_value());
                }
                OpCode::LoadLocal => {
                    let slot = chunk.code[ip] as usize;
                    ip += 1;
                    let v = self.stack[base_slot + slot].clone();
                    self.stack.push(v);
                }
                OpCode::StoreLocal => {
                    let slot = chunk.code[ip] as usize;
                    ip += 1;
                    let top = self.stack.last().expect("stack underflow").clone();
                    self.stack[base_slot + slot] = top;
                }
                OpCode::Pop => {
                    self.stack.pop().ok_or_else(|| underflow(line))?;
                }
                OpCode::PushNull => self.stack.push(Value::Null),
                OpCode::Dup => {
                    let top = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    self.stack.push(top);
                }

                OpCode::Add => self.binop_arith(line, arith_add)?,
                OpCode::AddAssign => {
                    // In-place `+=`: an Array target is mutated, not
                    // rebound; scalars fall back to ordinary `+`.
                    let rhs = self.pop(line)?;
                    let target = self.pop(line)?;
                    match target {
                        Value::Array(a) => {
                            match rhs {
                                // Snapshot `rhs` first so `a += a`
                                // doesn't double-borrow the cell.
                                Value::Array(b) => {
                                    let items: Vec<Value> =
                                        b.borrow().clone();
                                    a.borrow_mut().extend(items);
                                }
                                other => a.borrow_mut().push(other),
                            }
                            self.stack.push(Value::Array(a));
                        }
                        Value::Bytes(a) => {
                            match rhs {
                                // Snapshot first so `b += b` doesn't
                                // double-borrow the cell.
                                Value::Bytes(b) => {
                                    let items: Vec<u8> = b.borrow().clone();
                                    a.borrow_mut().extend(items);
                                }
                                other => return Err(RuntimeError::new(
                                    RuntimeErrorKind::TypeMismatch(format!(
                                        "cannot append {} to bytes (expected bytes)",
                                        other.type_name()
                                    )),
                                    line,
                                )),
                            }
                            self.stack.push(Value::Bytes(a));
                        }
                        other => {
                            let sum = arith_add(other, rhs, line)?;
                            self.stack.push(sum);
                        }
                    }
                }
                OpCode::Sub => self.binop_arith(line, arith_sub)?,
                OpCode::Mul => self.binop_arith(line, arith_mul)?,
                OpCode::Div => self.binop_arith(line, arith_div)?,
                OpCode::Mod => self.binop_arith(line, arith_mod)?,
                OpCode::Pow => self.binop_arith(line, arith_pow)?,
                OpCode::Negate => {
                    let v = self.stack.pop().ok_or_else(|| underflow(line))?;
                    self.stack.push(arith_neg(v, line)?);
                }

                OpCode::BitAnd => self.binop_arith(line, bit_and)?,
                OpCode::BitOr => self.binop_arith(line, bit_or)?,
                OpCode::BitXor => self.binop_arith(line, bit_xor)?,
                OpCode::Shl => self.binop_arith(line, shl)?,
                OpCode::Shr => self.binop_arith(line, shr)?,
                OpCode::BitNot => {
                    let v = self.stack.pop().ok_or_else(|| underflow(line))?;
                    self.stack.push(bit_not(v, line)?);
                }
                OpCode::TypeTest => {
                    let tag = chunk.code[ip];
                    ip += 1;
                    let v = self.stack.last().ok_or_else(|| underflow(line))?;
                    let matched = match (tag, v) {
                        (0, Value::Int(_)) => true,
                        (1, Value::Float(_)) => true,
                        (2, Value::Bool(_)) => true,
                        (3, Value::Str(_)) => true,
                        (4, Value::Array(_)) => true,
                        (5, Value::Object(_)) => true,
                        (6, Value::Range(_)) => true,
                        (7, Value::Null) => true,
                        (8, Value::Int(_) | Value::Float(_)) => true,
                        (9, Value::Function(_) | Value::NativeFn(_)) => true,
                        (10, Value::Map(_)) => true,
                        (11, Value::Set(_)) => true,
                        (12, Value::Bytes(_)) => true,
                        _ => false,
                    };
                    self.stack.push(Value::Bool(matched));
                }

                OpCode::Return => {
                    let result = self.stack.pop().ok_or_else(|| underflow(line))?;
                    let frame = self.frames.pop().unwrap();
                    self.close_upvalues(frame.base_slot);
                    self.stack.truncate(frame.base_slot);
                    // If this frame was evaluating an import, record
                    // the result in the cache (spec §12 — v0.3 adds
                    // caching) and clear the in-flight marker so a
                    // sibling import of the same path is allowed.
                    if let FrameKind::Import(path) = frame.kind {
                        self.module_cache.insert(path.clone(), result.clone());
                        self.in_flight.remove(&path);
                    }
                    if self.frames.len() == floor {
                        return Ok(result);
                    }
                    self.stack.push(result);
                    // current frame's ip is already where it should be;
                    // skip the writeback at the bottom of the loop.
                    continue;
                }

                // -- Phase 2 --
                OpCode::Eq => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(Value::Bool(a == b));
                }
                OpCode::Neq => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(Value::Bool(a != b));
                }
                OpCode::Lt => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(cmp(&a, &b, "<", line, |o| o.is_lt())?);
                }
                OpCode::Le => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(cmp(&a, &b, "<=", line, |o| o.is_le())?);
                }
                OpCode::Gt => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(cmp(&a, &b, ">", line, |o| o.is_gt())?);
                }
                OpCode::Ge => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(cmp(&a, &b, ">=", line, |o| o.is_ge())?);
                }
                OpCode::Not => {
                    let v = self.pop(line)?;
                    self.stack.push(Value::Bool(!v.is_truthy()));
                }
                OpCode::Jump => {
                    let dist = chunk.read_u16(ip);
                    ip += 2 + dist as usize;
                }
                OpCode::Loop => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    ip -= dist as usize;
                }
                OpCode::JumpIfFalse => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    if !self.stack.last().ok_or_else(|| underflow(line))?.is_truthy() {
                        ip += dist as usize;
                    }
                }
                OpCode::JumpIfTrue => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    if self.stack.last().ok_or_else(|| underflow(line))?.is_truthy() {
                        ip += dist as usize;
                    }
                }
                OpCode::JumpIfNotNull => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    let top = self.stack.last().ok_or_else(|| underflow(line))?;
                    if !matches!(top, Value::Null) {
                        ip += dist as usize;
                    }
                }
                OpCode::CloseScope => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    let top = self.pop(line)?;
                    let new_len = self.stack.len() - n;
                    self.close_upvalues(new_len);
                    self.stack.truncate(new_len);
                    self.stack.push(top);
                }

                // -- Phase 3 --
                OpCode::MakeArray => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    let start = self.stack.len() - n;
                    let items: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::Array(gc::alloc_array(items)));
                }
                OpCode::MakeObject => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    let start = self.stack.len() - n * 2;
                    let drained: Vec<Value> = self.stack.drain(start..).collect();
                    let mut obj: IndexMap<Rc<str>, Value> = IndexMap::with_capacity(n);
                    let mut iter = drained.into_iter();
                    while let (Some(k), Some(v)) = (iter.next(), iter.next()) {
                        let key = match k {
                            Value::Str(s) => s,
                            other => return Err(RuntimeError::new(
                                RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                                line,
                            )),
                        };
                        obj.insert(key, v);
                    }
                    self.stack.push(Value::Object(gc::alloc_object(obj)));
                }
                OpCode::IndexGet => {
                    let key = self.pop(line)?;
                    let coll = self.pop(line)?;
                    self.stack.push(index_get(&coll, &key, line)?);
                }
                OpCode::IndexSet => {
                    let value = self.pop(line)?;
                    let key = self.pop(line)?;
                    let coll = self.pop(line)?;
                    index_set(&coll, &key, value.clone(), line)?;
                    self.stack.push(value);
                }
                OpCode::Len => {
                    let v = self.pop(line)?;
                    let n = match &v {
                        Value::Array(a) => a.borrow().len() as i64,
                        Value::Object(o) => o.borrow().len() as i64,
                        Value::Map(m) => m.borrow().len() as i64,
                        Value::Set(s) => s.borrow().len() as i64,
                        Value::Bytes(b) => b.borrow().len() as i64,
                        Value::Str(s) => s.chars().count() as i64,
                        Value::Range(r) => r.length(),
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "cannot apply `#` to {}", other.type_name()
                            )),
                            line,
                        )),
                    };
                    self.stack.push(Value::Int(n));
                }
                OpCode::Call => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    // commit the post-Call ip on the *current* frame
                    // before potentially pushing a new one
                    self.frames.last_mut().unwrap().ip = ip;

                    let args_start = self.stack.len() - n;
                    let callee = self.stack[args_start - 1].clone();
                    match callee {
                        Value::Function(c) => {
                            if self.frames.len() >= self.max_call_depth {
                                return Err(stack_overflow_err(line));
                            }
                            let (arity, has_rest) = {
                                let cf = c.borrow();
                                (cf.function.arity, cf.function.has_rest)
                            };
                            if has_rest {
                                self.pack_rest(args_start, n, arity);
                            } else if n < arity {
                                for _ in n..arity {
                                    self.stack.push(Value::Null);
                                }
                            } else if n > arity {
                                let drop_n = n - arity;
                                self.stack.truncate(self.stack.len() - drop_n);
                            }
                            self.frames.push(CallFrame {
                                closure: c,
                                ip: 0,
                                base_slot: args_start - 1,
                                try_frames: Vec::new(),
                                kind: FrameKind::Function,
                            });
                            continue;
                        }
                        Value::NativeFn(nf) => {
                            let args: Vec<Value> = self.stack.drain(args_start..).collect();
                            self.stack.pop(); // remove callee
                            if !nf.arity.check(args.len()) {
                                return Err(RuntimeError::new(
                                    RuntimeErrorKind::ArityMismatch {
                                        name: nf.name.into(),
                                        expected: nf.arity.describe(),
                                        got: args.len(),
                                    },
                                    line,
                                ));
                            }
                            let result = (nf.func)(&args).map_err(|mut e| {
                                // Backfill the call-site line so an
                                // uncaught error from a builtin reports
                                // where it was *called*, not the
                                // useless line 0 the builtin defaulted
                                // to.
                                if e.line == 0 { e.line = line; }
                                e
                            })?;
                            self.stack.push(result);
                            continue;
                        }
                        other => {
                            return Err(RuntimeError::new(
                                RuntimeErrorKind::NotCallable(other.type_name().into()),
                                line,
                            ));
                        }
                    }
                }
                OpCode::TailCall => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    // commit ip — the native-fn arm below falls through
                    // to the `Return` that the compiler emits after a
                    // tail call, so the current frame's ip must be live.
                    self.frames.last_mut().unwrap().ip = ip;

                    let args_start = self.stack.len() - n;
                    let callee = self.stack[args_start - 1].clone();
                    match callee {
                        Value::Function(c) => {
                            let (arity, has_rest) = {
                                let cf = c.borrow();
                                (cf.function.arity, cf.function.has_rest)
                            };
                            if has_rest {
                                self.pack_rest(args_start, n, arity);
                            } else if n < arity {
                                for _ in n..arity {
                                    self.stack.push(Value::Null);
                                }
                            } else if n > arity {
                                let drop_n = n - arity;
                                self.stack.truncate(self.stack.len() - drop_n);
                            }
                            // Reuse the current frame: lift its captured
                            // locals to the heap, then discard them so
                            // the callee + arity-adjusted args slide down
                            // onto its base slot. No frame is pushed, so
                            // recursion stays O(1) in `frames`.
                            let base = self.frames.last().unwrap().base_slot;
                            self.close_upvalues(base);
                            self.stack.drain(base..args_start - 1);
                            let frame = self.frames.last_mut().unwrap();
                            frame.closure = c;
                            frame.ip = 0;
                            // base_slot unchanged; try_frames is empty —
                            // the compiler never emits TailCall inside a
                            // `try`.
                            continue;
                        }
                        Value::NativeFn(nf) => {
                            let args: Vec<Value> = self.stack.drain(args_start..).collect();
                            self.stack.pop(); // remove callee
                            if !nf.arity.check(args.len()) {
                                return Err(RuntimeError::new(
                                    RuntimeErrorKind::ArityMismatch {
                                        name: nf.name.into(),
                                        expected: nf.arity.describe(),
                                        got: args.len(),
                                    },
                                    line,
                                ));
                            }
                            let result = (nf.func)(&args).map_err(|mut e| {
                                if e.line == 0 { e.line = line; }
                                e
                            })?;
                            self.stack.push(result);
                            continue;
                        }
                        other => {
                            return Err(RuntimeError::new(
                                RuntimeErrorKind::NotCallable(other.type_name().into()),
                                line,
                            ));
                        }
                    }
                }
                OpCode::Dup2 => {
                    let len = self.stack.len();
                    let a = self.stack[len - 2].clone();
                    let b = self.stack[len - 1].clone();
                    self.stack.push(a);
                    self.stack.push(b);
                }

                // -- Phase 4 --
                OpCode::Closure => {
                    let func_idx = chunk.code[ip] as usize;
                    ip += 1;
                    let function = chunk.functions[func_idx].clone();
                    let mut upvalues = Vec::with_capacity(function.upvalues.len());
                    for _ in 0..function.upvalues.len() {
                        let is_local = chunk.code[ip] != 0;
                        ip += 1;
                        let index = chunk.code[ip] as usize;
                        ip += 1;
                        let upvalue = if is_local {
                            let stack_slot = base_slot + index;
                            self.capture_upvalue(stack_slot)
                        } else {
                            // Reuse upvalue from current frame's closure.
                            closure.borrow().upvalues[index]
                        };
                        upvalues.push(upvalue);
                    }
                    let new_closure = gc::alloc_closure(Closure { function, upvalues });
                    self.stack.push(Value::Function(new_closure));
                }
                OpCode::GetUpvalue => {
                    let idx = chunk.code[ip] as usize;
                    ip += 1;
                    let upv = closure.borrow().upvalues[idx];
                    let v = match &*upv.borrow() {
                        Upvalue::Open(slot) => self.stack[*slot].clone(),
                        Upvalue::Closed(v) => v.clone(),
                    };
                    self.stack.push(v);
                }
                OpCode::SetUpvalue => {
                    let idx = chunk.code[ip] as usize;
                    ip += 1;
                    let new_val = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    let upv = closure.borrow().upvalues[idx];
                    let mut up = upv.borrow_mut();
                    match &mut *up {
                        Upvalue::Open(slot) => self.stack[*slot] = new_val,
                        Upvalue::Closed(v) => *v = new_val,
                    }
                }
                OpCode::LoadGlobal => {
                    let idx = chunk.code[ip] as usize;
                    ip += 1;
                    self.stack.push(self.globals[idx].clone());
                }

                // -- Phase 5 --
                OpCode::MakeRange => {
                    let flags = chunk.code[ip];
                    ip += 1;
                    let inclusive = (flags & 1) != 0;
                    let has_step = (flags & 2) != 0;
                    let step = if has_step {
                        match self.pop(line)? {
                            Value::Int(n) => n,
                            other => return Err(RuntimeError::new(
                                RuntimeErrorKind::TypeMismatch(format!(
                                    "range step must be int, got {}", other.type_name()
                                )),
                                line,
                            )),
                        }
                    } else { 0 };
                    let to = match self.pop(line)? {
                        Value::Int(n) => n,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "range bound must be int, got {}", other.type_name()
                            )),
                            line,
                        )),
                    };
                    let from = match self.pop(line)? {
                        Value::Int(n) => n,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "range bound must be int, got {}", other.type_name()
                            )),
                            line,
                        )),
                    };
                    let step = if has_step {
                        step
                    } else if from <= to { 1 } else { -1 };
                    self.stack.push(Value::Range(Rc::new(RangeData {
                        from, to, step, inclusive,
                    })));
                }
                OpCode::MakeIter => {
                    let iter = make_iter(self.pop(line)?, line)?;
                    self.stack.push(Value::Iter(gc::alloc_iter(iter)));
                }
                OpCode::IterNext => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    let iter_val = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    let Value::Iter(it) = &iter_val else {
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(
                                "internal: IterNext on non-iter".into()
                            ),
                            line,
                        ));
                    };
                    // Classify without holding the RefCell borrow across
                    // the (possibly re-entrant) `next()` call below.
                    // `None` = built-in iterator; `Some(None)` = an
                    // iterator object already exhausted; `Some(Some(o))`
                    // = an iterator object to pull from.
                    let pull = match &*it.borrow() {
                        IterState::IterObject { object, done, .. } => {
                            Some(if *done { None } else { Some(object.clone()) })
                        }
                        _ => None,
                    };
                    match pull {
                        None => match it.borrow_mut().next() {
                            Some((_counter, value)) => self.stack.push(value),
                            None => ip += dist as usize,
                        },
                        Some(None) => ip += dist as usize,
                        Some(Some(obj)) => {
                            self.frames.last_mut().unwrap().ip = ip;
                            match self.iter_object_pull(obj, line)? {
                                Some(value) => {
                                    if let IterState::IterObject { index, .. } =
                                        &mut *it.borrow_mut()
                                    {
                                        *index += 1;
                                    }
                                    self.stack.push(value);
                                }
                                None => {
                                    if let IterState::IterObject { done, .. } =
                                        &mut *it.borrow_mut()
                                    {
                                        *done = true;
                                    }
                                    ip += dist as usize;
                                }
                            }
                            self.frames.last_mut().unwrap().ip = ip;
                            continue;
                        }
                    }
                }
                OpCode::IterNext2 => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    let iter_val = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    let Value::Iter(it) = &iter_val else {
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(
                                "internal: IterNext2 on non-iter".into()
                            ),
                            line,
                        ));
                    };
                    let pull = match &*it.borrow() {
                        IterState::IterObject { object, done, .. } => {
                            Some(if *done { None } else { Some(object.clone()) })
                        }
                        _ => None,
                    };
                    match pull {
                        None => match it.borrow_mut().next() {
                            Some((counter, value)) => {
                                self.stack.push(counter);
                                self.stack.push(value);
                            }
                            None => ip += dist as usize,
                        },
                        Some(None) => ip += dist as usize,
                        Some(Some(obj)) => {
                            self.frames.last_mut().unwrap().ip = ip;
                            match self.iter_object_pull(obj, line)? {
                                Some(value) => {
                                    // Synthetic counter (0, 1, 2, ...).
                                    let counter = {
                                        let mut st = it.borrow_mut();
                                        if let IterState::IterObject { index, .. } =
                                            &mut *st
                                        {
                                            let c = *index;
                                            *index += 1;
                                            c
                                        } else {
                                            0
                                        }
                                    };
                                    self.stack.push(Value::Int(counter));
                                    self.stack.push(value);
                                }
                                None => {
                                    if let IterState::IterObject { done, .. } =
                                        &mut *it.borrow_mut()
                                    {
                                        *done = true;
                                    }
                                    ip += dist as usize;
                                }
                            }
                            self.frames.last_mut().unwrap().ip = ip;
                            continue;
                        }
                    }
                }
                OpCode::IterAppend => {
                    let slot = chunk.code[ip] as usize;
                    ip += 1;
                    // Every body value is collected verbatim, including
                    // `null` — `continue` is the only way to skip an item.
                    let v = self.pop(line)?;
                    let target = self.stack[base_slot + slot].clone();
                    match target {
                        Value::Array(a) => a.borrow_mut().push(v),
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: IterAppend target is {}", other.type_name()
                            )),
                            line,
                        )),
                    }
                }
                OpCode::Unwind => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    let target = base_slot + n;
                    self.close_upvalues(target);
                    self.stack.truncate(target);
                }

                // -- Phase 6 --
                OpCode::ArrayPush => {
                    let v = self.pop(line)?;
                    let arr = self.stack.last().ok_or_else(|| underflow(line))?;
                    match arr {
                        Value::Array(a) => a.borrow_mut().push(v),
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: ArrayPush target is {}", other.type_name()
                            )),
                            line,
                        )),
                    }
                }
                OpCode::ArrayExtend => {
                    let src = self.pop(line)?;
                    let target = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    let target_arr = match target {
                        Value::Array(a) => a,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: ArrayExtend target is {}", other.type_name()
                            )),
                            line,
                        )),
                    };
                    // Spread of an iterator object — drive its `next()`
                    // protocol. Covers `[...it]` and `f(...it)` (call
                    // spread builds its arg array with `ArrayExtend`).
                    if let Value::Object(o) = &src {
                        let is_iter = matches!(
                            o.borrow().get("next"),
                            Some(Value::Function(_)) | Some(Value::NativeFn(_))
                        );
                        if is_iter {
                            let o = *o;
                            self.frames.last_mut().unwrap().ip = ip;
                            // `iter_object_pull` re-enters the VM, which
                            // may collect. `src` is already off the
                            // stack — push it back as a temporary root
                            // so the iterator object survives the loop.
                            let root = self.stack.len();
                            self.stack.push(src);
                            loop {
                                match self.iter_object_pull(o, line) {
                                    Ok(Some(v)) => target_arr.borrow_mut().push(v),
                                    Ok(None) => break,
                                    Err(e) => {
                                        self.stack.truncate(root);
                                        return Err(e);
                                    }
                                }
                            }
                            self.stack.truncate(root);
                            continue;
                        }
                    }
                    extend_array(target_arr, src, line)?;
                }
                OpCode::ObjectMerge => {
                    let src = self.pop(line)?;
                    let target = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    let (target_obj, src_obj) = match (target, src) {
                        (Value::Object(t), Value::Object(s)) => (t, s),
                        (_, other) => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "cannot spread {} into object", other.type_name()
                            )),
                            line,
                        )),
                    };
                    // IndexMap.insert keeps existing position when key
                    // exists — preserves source order while letting
                    // later spreads/keys overwrite values.
                    let entries: Vec<_> = src_obj.borrow().iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    let mut t = target_obj.borrow_mut();
                    for (k, v) in entries {
                        t.insert(k, v);
                    }
                }
                OpCode::CallSpread => {
                    let args_val = self.pop(line)?;
                    let args: Vec<Value> = match args_val {
                        Value::Array(a) => a.borrow().clone(),
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: CallSpread args not array: {}",
                                other.type_name()
                            )),
                            line,
                        )),
                    };
                    let n = args.len();
                    // Push args onto the stack as if they were
                    // compiled inline, then dispatch like a normal
                    // Call. Reuse the same logic flow.
                    for a in args { self.stack.push(a); }
                    // commit ip first since the path may push a frame
                    self.frames.last_mut().unwrap().ip = ip;

                    let args_start = self.stack.len() - n;
                    let callee = self.stack[args_start - 1].clone();
                    match callee {
                        Value::Function(c) => {
                            let (arity, has_rest) = {
                                let cf = c.borrow();
                                (cf.function.arity, cf.function.has_rest)
                            };
                            if has_rest {
                                self.pack_rest(args_start, n, arity);
                            } else if n < arity {
                                for _ in n..arity { self.stack.push(Value::Null); }
                            } else if n > arity {
                                let drop_n = n - arity;
                                self.stack.truncate(self.stack.len() - drop_n);
                            }
                            self.frames.push(CallFrame {
                                closure: c,
                                ip: 0,
                                base_slot: args_start - 1,
                                try_frames: Vec::new(),
                                kind: FrameKind::Function,
                            });
                            continue;
                        }
                        Value::NativeFn(nf) => {
                            let call_args: Vec<Value> = self.stack
                                .drain(args_start..).collect();
                            self.stack.pop();
                            if !nf.arity.check(call_args.len()) {
                                return Err(RuntimeError::new(
                                    RuntimeErrorKind::ArityMismatch {
                                        name: nf.name.into(),
                                        expected: nf.arity.describe(),
                                        got: call_args.len(),
                                    },
                                    line,
                                ));
                            }
                            let result = (nf.func)(&call_args).map_err(|mut e| {
                                if e.line == 0 { e.line = line; }
                                e
                            })?;
                            self.stack.push(result);
                            continue;
                        }
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::NotCallable(other.type_name().into()),
                            line,
                        )),
                    }
                }
                OpCode::ConcatN => {
                    let n = chunk.code[ip] as usize;
                    ip += 1;
                    let start = self.stack.len() - n;
                    let parts: Vec<Value> = self.stack.drain(start..).collect();
                    let mut out = String::new();
                    for p in parts {
                        match p {
                            Value::Str(s) => out.push_str(&s),
                            other => out.push_str(&format!("{other}")),
                        }
                    }
                    self.stack.push(Value::Str(out.into()));
                }

                // -- Phase 7 --
                OpCode::SliceFrom => {
                    let start_val = self.pop(line)?;
                    let arr_val = self.pop(line)?;
                    let start = match start_val {
                        Value::Int(n) => n,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                            line,
                        )),
                    };
                    let result = match arr_val {
                        Value::Array(a) => {
                            let src = a.borrow();
                            let len = src.len() as i64;
                            let real = if start < 0 { (start + len).max(0) } else { start.min(len) };
                            let real = real.max(0) as usize;
                            Value::Array(gc::alloc_array(src[real..].to_vec()))
                        }
                        Value::Bytes(b) => {
                            let src = b.borrow();
                            let len = src.len() as i64;
                            let real = if start < 0 { (start + len).max(0) } else { start.min(len) };
                            let real = real.max(0) as usize;
                            Value::Bytes(gc::alloc_bytes(src[real..].to_vec()))
                        }
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "cannot slice {} (only Array and Bytes supported)",
                                other.type_name()
                            )),
                            line,
                        )),
                    };
                    self.stack.push(result);
                }
                OpCode::ObjRest => {
                    let keys_val = self.pop(line)?;
                    let src_val = self.pop(line)?;
                    let exclude: Vec<Rc<str>> = match keys_val {
                        Value::Array(a) => a.borrow().iter()
                            .filter_map(|v| match v {
                                Value::Str(s) => Some(s.clone()),
                                _ => None,
                            }).collect(),
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: ObjRest keys not array: {}",
                                other.type_name()
                            )),
                            line,
                        )),
                    };
                    let src_obj = match src_val {
                        Value::Object(o) => o,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "cannot apply `...rest` pattern to {}",
                                other.type_name()
                            )),
                            line,
                        )),
                    };
                    let mut out: IndexMap<Rc<str>, Value> = IndexMap::new();
                    for (k, v) in src_obj.borrow().iter() {
                        if !exclude.iter().any(|x| x == k) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    self.stack.push(Value::Object(gc::alloc_object(out)));
                }
                OpCode::Import => {
                    let path_val = self.pop(line)?;
                    let path_str = match path_val {
                        Value::Str(s) => s,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "import path must be a string, got {}",
                                other.type_name()
                            )),
                            line,
                        )),
                    };

                    // Bare names (no path separators or extension)
                    // resolve in three steps:
                    //   1. Cache hit under `<bare:Name>` key.
                    //   2. Source stdlib (Array / Math / String).
                    //      Compile the embedded `.tg` and push it as
                    //      an Import frame — its Return caches.
                    //   3. Native modules (IO / Os / Time / _Native*).
                    //      Cache the resulting Object directly.
                    let is_bare = !path_str.contains('/')
                        && !path_str.contains('\\')
                        && !path_str.contains('.');
                    if is_bare {
                        let key = PathBuf::from(format!("<bare:{}>", path_str));
                        if let Some(cached) = self.module_cache.get(&key) {
                            self.stack.push(cached.clone());
                            self.frames.last_mut().unwrap().ip = ip;
                            continue;
                        }
                        if let Some(source) = crate::vm::source_stdlib::source(&path_str) {
                            let sid = self.source_map.borrow_mut().add(
                                format!("<stdlib:{}>", path_str),
                                source,
                            );
                            let main = match crate::vm::compile_source_with_id(
                                source, None, sid,
                            ) {
                                Ok(m) => m,
                                Err(e) => {
                                    return Err(RuntimeError::new(
                                        RuntimeErrorKind::ImportFailed(
                                            path_str.to_string(),
                                            format!("{e}"),
                                        ),
                                        line,
                                    ));
                                }
                            };
                            self.in_flight.insert(key.clone());
                            let mc = gc::alloc_closure(Closure {
                                function: Arc::new(main),
                                upvalues: Vec::new(),
                            });
                            self.frames.last_mut().unwrap().ip = ip;
                            let base = self.stack.len();
                            self.stack.push(Value::Function(mc));
                            self.frames.push(CallFrame {
                                closure: mc,
                                ip: 0,
                                base_slot: base,
                                try_frames: Vec::new(),
                                kind: FrameKind::Import(key),
                            });
                            continue;
                        }
                        if let Some(module) = crate::vm::native_modules::resolve(&path_str) {
                            self.module_cache.insert(key, module.clone());
                            self.stack.push(module);
                            self.frames.last_mut().unwrap().ip = ip;
                            continue;
                        }
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::ImportFailed(
                                path_str.to_string(),
                                "no module of that name".into(),
                            ),
                            line,
                        ));
                    }

                    // File path: resolve → cache → in-flight check →
                    // compile and push as a new frame on this same Vm.
                    // The frame is tagged `Import(path)` so the Return
                    // opcode can write the cache entry. Relative paths
                    // resolve against the importing chunk's base dir
                    // (absent for string-compiled source — then they
                    // resolve against the process cwd). `.tg` is
                    // appended when the path carries no extension.
                    let mut path = if std::path::Path::new(&*path_str).is_absolute() {
                        PathBuf::from(&*path_str)
                    } else {
                        match &chunk.base_dir {
                            Some(d) => d.join(&*path_str),
                            None => PathBuf::from(&*path_str),
                        }
                    };
                    if path.extension().is_none() {
                        path.set_extension("tg");
                    }
                    if let Some(cached) = self.module_cache.get(&path) {
                        self.stack.push(cached.clone());
                        self.frames.last_mut().unwrap().ip = ip;
                        continue;
                    }
                    if self.in_flight.contains(&path) {
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::ImportFailed(
                                path_str.to_string(),
                                "circular import".into(),
                            ),
                            line,
                        ));
                    }
                    let main = match crate::vm::compile_file_into(
                        &path,
                        &mut self.source_map.borrow_mut(),
                    ) {
                        Ok(m) => m,
                        Err(e) => {
                            return Err(RuntimeError::new(
                                RuntimeErrorKind::ImportFailed(
                                    path_str.to_string(),
                                    format!("{e}"),
                                ),
                                line,
                            ));
                        }
                    };
                    self.in_flight.insert(path.clone());
                    let main_closure = gc::alloc_closure(Closure {
                        function: Arc::new(main),
                        upvalues: Vec::new(),
                    });
                    // Commit ip for the importing frame BEFORE pushing
                    // the import frame so resume after Return lands at
                    // the instruction following Import.
                    self.frames.last_mut().unwrap().ip = ip;
                    let base = self.stack.len();
                    self.stack.push(Value::Function(main_closure.clone()));
                    self.frames.push(CallFrame {
                        closure: main_closure,
                        ip: 0,
                        base_slot: base,
                        try_frames: Vec::new(),
                        kind: FrameKind::Import(path),
                    });
                    continue;
                }

                // -- v0.3 try/catch/raise --
                OpCode::PushTry => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    let catch_pc = ip + dist as usize;
                    let stack_len = self.stack.len();
                    self.frames.last_mut().unwrap().try_frames.push(TryFrame {
                        catch_pc,
                        stack_len,
                    });
                }
                OpCode::PopTry => {
                    self.frames
                        .last_mut()
                        .unwrap()
                        .try_frames
                        .pop()
                        .expect("PopTry with no active try-frame");
                }
                OpCode::Raise => {
                    // The raised value is stored verbatim — `catch`
                    // binds exactly this, no string coercion.
                    let v = self.pop(line)?;
                    // Commit ip onto the frame so try_catch can rely on
                    // it (though try_catch overwrites with catch_pc).
                    self.frames.last_mut().unwrap().ip = ip;
                    return Err(RuntimeError::new(
                        RuntimeErrorKind::Raised(v),
                        line,
                    ));
                }
                OpCode::Halt => {
                    // REPL line end: surface the value but keep the
                    // frame so the next line resumes with locals
                    // intact. `run_repl_line` resets ip before reuse.
                    let value = self.stack.pop().ok_or_else(|| underflow(line))?;
                    self.frames.last_mut().unwrap().ip = ip;
                    return Ok(value);
                }
                OpCode::NoMatchError => {
                    // A `match` fell through every arm. Raise a catchable
                    // built-in error rather than yielding `null`.
                    self.frames.last_mut().unwrap().ip = ip;
                    return Err(RuntimeError::new(
                        RuntimeErrorKind::NoMatch,
                        line,
                    ));
                }
                OpCode::Spawn => {
                    // Pop the function and start it as an actor on its
                    // own OS thread + heap; push a `Task` handle.
                    let callee = self.pop(line)?;
                    let task = self.spawn_actor(callee, line)?;
                    self.stack.push(Value::Task(task));
                }
            }

            // commit ip back to current frame
            self.frames.last_mut().unwrap().ip = ip;
        }
    }

    // -- helpers ------------------------------------------------------

    fn pop(&mut self, line: u32) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(|| underflow(line))
    }

    fn binop_arith<F>(&mut self, line: u32, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(Value, Value, u32) -> Result<Value, RuntimeError>,
    {
        let b = self.pop(line)?;
        let a = self.pop(line)?;
        self.stack.push(f(a, b, line)?);
        Ok(())
    }

    /// Pack the args at `[args_start..]` into the rest-array layout
    /// expected by a `has_rest` function. After this:
    ///   - slots `args_start..args_start+arity` hold the fixed args
    ///     (padded with `null` if `n < arity`);
    ///   - slot `args_start+arity` holds an Array of extras
    ///     (possibly empty).
    fn pack_rest(&mut self, args_start: usize, n: usize, arity: usize) {
        if n < arity {
            for _ in n..arity { self.stack.push(Value::Null); }
            self.stack.push(Value::Array(gc::alloc_array(Vec::new())));
        } else {
            let rest_start = args_start + arity;
            let extras: Vec<Value> = self.stack.drain(rest_start..).collect();
            self.stack.push(Value::Array(gc::alloc_array(extras)));
        }
    }

    /// Invoke `callee` with `args` re-entrantly — from inside opcode
    /// execution — and return its result. A `NativeFn` runs directly; a
    /// tigr closure gets a fresh frame and a nested [`run_until`] down
    /// to the current frame depth. A raise the callee catches with its
    /// own `try` is handled here and the call resumes; one it does not
    /// catch unwinds the callee's frames and propagates as `Err`.
    fn call_value(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        line: u32,
    ) -> Result<Value, RuntimeError> {
        match callee {
            Value::NativeFn(nf) => {
                if !nf.arity.check(args.len()) {
                    return Err(RuntimeError::new(
                        RuntimeErrorKind::ArityMismatch {
                            name: nf.name.into(),
                            expected: nf.arity.describe(),
                            got: args.len(),
                        },
                        line,
                    ));
                }
                (nf.func)(&args).map_err(|mut e| {
                    if e.line == 0 { e.line = line; }
                    e
                })
            }
            Value::Function(c) => {
                let floor = self.frames.len();
                if floor >= self.max_call_depth {
                    return Err(stack_overflow_err(line));
                }
                let (arity, has_rest) = {
                    let cf = c.borrow();
                    (cf.function.arity, cf.function.has_rest)
                };
                let n = args.len();
                // Mirror the `Call` opcode's stack layout: callee slot
                // followed by the arity-adjusted args.
                let base_slot = self.stack.len();
                self.stack.push(Value::Function(c.clone()));
                let args_start = self.stack.len();
                for a in args {
                    self.stack.push(a);
                }
                if has_rest {
                    self.pack_rest(args_start, n, arity);
                } else if n < arity {
                    for _ in n..arity {
                        self.stack.push(Value::Null);
                    }
                } else if n > arity {
                    self.stack.truncate(self.stack.len() - (n - arity));
                }
                self.frames.push(CallFrame {
                    closure: c,
                    ip: 0,
                    base_slot,
                    try_frames: Vec::new(),
                    kind: FrameKind::Function,
                });
                loop {
                    match self.run_until(floor) {
                        Ok(v) => return Ok(v),
                        Err(mut err) => {
                            self.stamp_error_source(&mut err);
                            if self.try_catch(floor, &mut err) {
                                continue;
                            }
                            return Err(err);
                        }
                    }
                }
            }
            other => Err(RuntimeError::new(
                RuntimeErrorKind::NotCallable(other.type_name().into()),
                line,
            )),
        }
    }

    /// Pull one element from an iterator object (`${ next: fn() }`).
    /// `Ok(None)` means the iterator reported `done: true`;
    /// `Ok(Some(v))` is the next `value`. Used by `for` loops and
    /// spread over iterator objects.
    fn iter_object_pull(
        &mut self,
        obj: GcRef<ObjectKind>,
        line: u32,
    ) -> Result<Option<Value>, RuntimeError> {
        let next_fn = obj.borrow().get("next").cloned();
        let next_fn = match next_fn {
            Some(v @ (Value::Function(_) | Value::NativeFn(_))) => v,
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::TypeMismatch(
                        "iterator object's `next` field is not callable".into(),
                    ),
                    line,
                ))
            }
        };
        let result = self.call_value(next_fn, Vec::new(), line)?;
        let result_obj = match result {
            Value::Object(o) => o,
            other => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::TypeMismatch(format!(
                        "iterator next() must return an object, got {}",
                        other.type_name()
                    )),
                    line,
                ))
            }
        };
        let ro = result_obj.borrow();
        let done = match ro.get("done") {
            Some(d) => d.is_truthy(),
            None => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::TypeMismatch(
                        "iterator next() result is missing a `done` field".into(),
                    ),
                    line,
                ))
            }
        };
        if done {
            Ok(None)
        } else {
            Ok(Some(ro.get("value").cloned().unwrap_or(Value::Null)))
        }
    }

    fn capture_upvalue(&mut self, stack_slot: usize) -> GcRef<UpvalueKind> {
        for up in &self.open_upvalues {
            if let Upvalue::Open(slot) = *up.borrow() {
                if slot == stack_slot {
                    return *up;
                }
            }
        }
        let new_up = gc::alloc_upvalue(Upvalue::Open(stack_slot));
        self.open_upvalues.push(new_up);
        new_up
    }

    /// Close (lift to heap) every open upvalue whose stack slot is at
    /// or above `target_slot`.
    fn close_upvalues(&mut self, target_slot: usize) {
        let mut still_open = Vec::with_capacity(self.open_upvalues.len());
        for up in self.open_upvalues.drain(..) {
            let slot_opt = match *up.borrow() {
                Upvalue::Open(slot) if slot >= target_slot => Some(slot),
                _ => None,
            };
            match slot_opt {
                Some(slot) => {
                    let value = self.stack[slot].clone();
                    *up.borrow_mut() = Upvalue::Closed(value);
                    // dropped: not added to still_open
                }
                None => still_open.push(up),
            }
        }
        self.open_upvalues = still_open;
    }

    /// Mark every GC root this Vm holds. The root set is exactly these
    /// five fields — nothing else retains a `Value` (see `gc.rs`).
    fn trace_roots(&self, m: &mut Marker) {
        for v in &self.stack {
            v.trace(m);
        }
        for v in &self.globals {
            v.trace(m);
        }
        for up in &self.open_upvalues {
            m.mark_upvalue(*up);
        }
        for frame in &self.frames {
            m.mark_closure(frame.closure);
        }
        for v in self.module_cache.values() {
            v.trace(m);
        }
    }

    /// Run one mark-sweep collection over the managed heap.
    fn collect(&mut self) {
        gc::collect(|m| self.trace_roots(m));
    }

    /// Collect if the heap trigger fires. Called only at the dispatch-
    /// loop safepoint: no borrow guard is live there and the whole root
    /// set is reachable from the Vm's five fields, so a sweep is safe.
    #[inline]
    fn maybe_collect(&mut self) {
        if gc::should_collect() {
            self.collect();
        }
    }
}

fn underflow(line: u32) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::StackUnderflow, line)
}

/// Worker-thread entry point for `spawn`. Builds a fresh `Vm` (its own
/// thread-local heap), decodes the spawned closure into that heap,
/// runs it, and encodes the outcome back into `Send`-able form. An
/// uncaught actor error is rendered against the worker's own
/// `SourceMap` (the parent's is not `Send`).
fn run_actor(transfer: crate::vm::transfer::Transfer) -> crate::vm::task::ActorOutcome {
    use crate::vm::transfer::{decode, encode, TransferError};

    let mut vm = Vm::new();
    let closure = match decode(transfer) {
        Value::Function(c) => c,
        _ => unreachable!("spawn always encodes a closure"),
    };
    match vm.run_closure(closure) {
        Ok(v) => encode(&v).map_err(|e| TransferError {
            kind_tag: e.kind.kind_tag().to_string(),
            message: format!("actor return value could not be sent: {e}"),
            rendered_trace: String::new(),
            raised: None,
        }),
        Err(e) => {
            let kind_tag = e.kind.kind_tag().to_string();
            let message = format!("{e}");
            // If the actor did `raise <value>`, carry that value so the
            // parent's `catch` binds exactly it. A non-sendable raised
            // value falls back to `None` — its `str()` form is already
            // in `message`.
            let raised = match &e.kind {
                RuntimeErrorKind::Raised(v) => encode(v).ok(),
                _ => None,
            };
            let rendered_trace = crate::vm::error::Error::Runtime(e)
                .render(&vm.source_map.borrow());
            Err(TransferError { kind_tag, message, rendered_trace, raised })
        }
    }
}

// -- arithmetic helpers (spec §6.2 + §7.1) --

/// Wrap a `num_bigint::BigInt` back into a `Value`.
fn big(n: BigIntData) -> Value {
    Value::BigInt(Rc::new(n))
}

/// A non-exact `BigInt /` raises this catchable structured error —
/// `${kind: 'inexact_division', message}` — rather than silently
/// dropping precision into a `Float`. `BigInt.divmod` / `BigInt.div`
/// give integer division.
fn inexact_div_err(line: u32) -> RuntimeError {
    let obj = crate::vm::native_modules::object(&[
        ("kind", Value::Str("inexact_division".into())),
        (
            "message",
            Value::Str(
                "BigInt division is not exact; use BigInt.divmod or \
                 BigInt.div for integer division"
                    .into(),
            ),
        ),
    ]);
    RuntimeError::new(RuntimeErrorKind::Raised(obj), line)
}

/// `BigInt / BigInt`: exact → `BigInt`, otherwise raise (see
/// `inexact_div_err`). Divide-by-zero raises `DivisionByZero`.
fn bigint_div(x: &BigIntData, y: &BigIntData, line: u32) -> Result<Value, RuntimeError> {
    if y.is_zero() {
        return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, line));
    }
    let (q, r) = x.div_rem(y);
    if r.is_zero() {
        Ok(big(q))
    } else {
        Err(inexact_div_err(line))
    }
}

/// `BigInt % BigInt`: always a `BigInt` (Rust truncated remainder,
/// sign of the dividend — matches `Int % Int`).
fn bigint_rem(x: &BigIntData, y: &BigIntData, line: u32) -> Result<Value, RuntimeError> {
    if y.is_zero() {
        return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, line));
    }
    Ok(big(x % y))
}

fn arith_add(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Ok(Int(x.checked_add(y).ok_or_else(|| overflow_err(line))?)),
        (Int(x), Float(y)) => Ok(Float(x as f64 + y)),
        (Float(x), Int(y)) => Ok(Float(x + y as f64)),
        (Float(x), Float(y)) => Ok(Float(x + y)),
        (BigInt(x), BigInt(y)) => Ok(big(&*x + &*y)),
        (BigInt(x), Int(y)) => Ok(big(&*x + &BigIntData::from(y))),
        (Int(x), BigInt(y)) => Ok(big(&BigIntData::from(x) + &*y)),
        (BigInt(x), Float(y)) => Ok(Float(bigint_to_f64(&x) + y)),
        (Float(x), BigInt(y)) => Ok(Float(x + bigint_to_f64(&y))),
        (Str(x), Str(y)) => {
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(&x);
            s.push_str(&y);
            Ok(Str(s.into()))
        }
        (Array(x), Array(y)) => {
            let mut v: Vec<Value> = x.borrow().clone();
            v.extend(y.borrow().iter().cloned());
            Ok(Array(gc::alloc_array(v)))
        }
        (Array(x), other) => {
            let mut v: Vec<Value> = x.borrow().clone();
            v.push(other);
            Ok(Array(gc::alloc_array(v)))
        }
        (Bytes(x), Bytes(y)) => {
            let mut v: Vec<u8> = x.borrow().clone();
            v.extend(y.borrow().iter().copied());
            Ok(Bytes(gc::alloc_bytes(v)))
        }
        (a, b) => Err(type_err("+", &a, &b, line)),
    }
}

fn arith_sub(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Ok(Int(x.checked_sub(y).ok_or_else(|| overflow_err(line))?)),
        (Int(x), Float(y)) => Ok(Float(x as f64 - y)),
        (Float(x), Int(y)) => Ok(Float(x - y as f64)),
        (Float(x), Float(y)) => Ok(Float(x - y)),
        (BigInt(x), BigInt(y)) => Ok(big(&*x - &*y)),
        (BigInt(x), Int(y)) => Ok(big(&*x - &BigIntData::from(y))),
        (Int(x), BigInt(y)) => Ok(big(&BigIntData::from(x) - &*y)),
        (BigInt(x), Float(y)) => Ok(Float(bigint_to_f64(&x) - y)),
        (Float(x), BigInt(y)) => Ok(Float(x - bigint_to_f64(&y))),
        (a, b) => Err(type_err("-", &a, &b, line)),
    }
}

fn arith_mul(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Ok(Int(x.checked_mul(y).ok_or_else(|| overflow_err(line))?)),
        (Int(x), Float(y)) => Ok(Float(x as f64 * y)),
        (Float(x), Int(y)) => Ok(Float(x * y as f64)),
        (Float(x), Float(y)) => Ok(Float(x * y)),
        (BigInt(x), BigInt(y)) => Ok(big(&*x * &*y)),
        (BigInt(x), Int(y)) => Ok(big(&*x * &BigIntData::from(y))),
        (Int(x), BigInt(y)) => Ok(big(&BigIntData::from(x) * &*y)),
        (BigInt(x), Float(y)) => Ok(Float(bigint_to_f64(&x) * y)),
        (Float(x), BigInt(y)) => Ok(Float(x * bigint_to_f64(&y))),
        (a, b) => Err(type_err("*", &a, &b, line)),
    }
}

fn arith_div(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(_), Int(0)) => Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, line)),
        (Int(x), Int(y)) => {
            if x % y == 0 { Ok(Int(x / y)) } else { Ok(Float(x as f64 / y as f64)) }
        }
        (Int(x), Float(y)) => Ok(Float(x as f64 / y)),
        (Float(x), Int(y)) => Ok(Float(x / y as f64)),
        (Float(x), Float(y)) => Ok(Float(x / y)),
        (BigInt(x), BigInt(y)) => bigint_div(&x, &y, line),
        (BigInt(x), Int(y)) => bigint_div(&x, &BigIntData::from(y), line),
        (Int(x), BigInt(y)) => bigint_div(&BigIntData::from(x), &y, line),
        (BigInt(x), Float(y)) => Ok(Float(bigint_to_f64(&x) / y)),
        (Float(x), BigInt(y)) => Ok(Float(x / bigint_to_f64(&y))),
        (a, b) => Err(type_err("/", &a, &b, line)),
    }
}

fn arith_mod(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(_), Int(0)) => Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, line)),
        (Int(x), Int(y)) => Ok(Int(x % y)),
        (Int(x), Float(y)) => Ok(Float(x as f64 % y)),
        (Float(x), Int(y)) => Ok(Float(x % y as f64)),
        (Float(x), Float(y)) => Ok(Float(x % y)),
        (BigInt(x), BigInt(y)) => bigint_rem(&x, &y, line),
        (BigInt(x), Int(y)) => bigint_rem(&x, &BigIntData::from(y), line),
        (Int(x), BigInt(y)) => bigint_rem(&BigIntData::from(x), &y, line),
        (BigInt(x), Float(y)) => Ok(Float(bigint_to_f64(&x) % y)),
        (Float(x), BigInt(y)) => Ok(Float(x % bigint_to_f64(&y))),
        (a, b) => Err(type_err("%", &a, &b, line)),
    }
}

fn arith_pow(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use num_traits::ToPrimitive;
    use Value::*;
    // A `BigInt` base raised to a non-negative integer exponent stays
    // exact — `^^` then yields a `BigInt`, not a lossy `Float`.
    match (&a, &b) {
        (BigInt(x), Int(y)) if *y >= 0 => return Ok(big(Pow::pow(&**x, *y as u64))),
        (BigInt(x), BigInt(y)) => {
            if let Some(e) = y.to_u64() {
                return Ok(big(Pow::pow(&**x, e)));
            }
        }
        _ => {}
    }
    // Otherwise fall back to `f64` — a negative, fractional, or
    // astronomically large exponent has no exact `BigInt` result.
    let (x, y) = match (a, b) {
        (Int(x), Int(y)) => (x as f64, y as f64),
        (Int(x), Float(y)) => (x as f64, y),
        (Float(x), Int(y)) => (x, y as f64),
        (Float(x), Float(y)) => (x, y),
        (BigInt(x), Int(y)) => (bigint_to_f64(&x), y as f64),
        (BigInt(x), Float(y)) => (bigint_to_f64(&x), y),
        (BigInt(x), BigInt(y)) => (bigint_to_f64(&x), bigint_to_f64(&y)),
        (Int(x), BigInt(y)) => (x as f64, bigint_to_f64(&y)),
        (Float(x), BigInt(y)) => (x, bigint_to_f64(&y)),
        (a, b) => return Err(type_err("^^", &a, &b, line)),
    };
    Ok(Float(x.powf(y)))
}

fn arith_neg(a: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match a {
        Int(x) => Ok(Int(x.checked_neg().ok_or_else(|| overflow_err(line))?)),
        Float(x) => Ok(Float(-x)),
        BigInt(x) => Ok(big(-&*x)),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!("cannot negate {}", other.type_name())),
            line,
        )),
    }
}

// -- bitwise helpers (v0.5, spec §6.x) — Int-only --

fn bit_and(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x & y)),
        (a, b) => Err(type_err("&", &a, &b, line)),
    }
}

fn bit_or(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x | y)),
        (a, b) => Err(type_err("|", &a, &b, line)),
    }
}

fn bit_xor(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x ^ y)),
        (a, b) => Err(type_err("^", &a, &b, line)),
    }
}

/// A shift amount must be a non-negative Int below 64; anything else
/// raises rather than panicking (Rust's `<<`/`>>` panic in debug and
/// are UB-shaped past the bit width).
fn shift_amount(y: i64, op: &str, line: u32) -> Result<u32, RuntimeError> {
    if (0..64).contains(&y) {
        Ok(y as u32)
    } else {
        Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "`{op}` shift amount {y} is out of range (0..64)"
            )),
            line,
        ))
    }
}

fn shl(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x << shift_amount(y, "<<", line)?)),
        (a, b) => Err(type_err("<<", &a, &b, line)),
    }
}

fn shr(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    match (a, b) {
        // `>>` on a signed i64 is an arithmetic (sign-preserving) shift.
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x >> shift_amount(y, ">>", line)?)),
        (a, b) => Err(type_err(">>", &a, &b, line)),
    }
}

fn bit_not(a: Value, line: u32) -> Result<Value, RuntimeError> {
    match a {
        Value::Int(x) => Ok(Value::Int(!x)),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "operator `~` does not apply to {}",
                other.type_name()
            )),
            line,
        )),
    }
}

fn cmp(
    a: &Value,
    b: &Value,
    op: &str,
    line: u32,
    pred: impl FnOnce(std::cmp::Ordering) -> bool,
) -> Result<Value, RuntimeError> {
    match a.partial_cmp(b) {
        Some(o) => Ok(Value::Bool(pred(o))),
        None => Err(type_err(op, a, b, line)),
    }
}

/// Extend the array `target` in place with the elements of `src`.
/// Backs `ArrayExtend` (array spread) and indirectly `CallSpread`.
fn extend_array(
    target: GcRef<ArrayKind>,
    src: Value,
    line: u32,
) -> Result<(), RuntimeError> {
    match src {
        Value::Array(a) => {
            // Borrow source through a clone of the Vec to avoid a
            // double-borrow when target IS source (e.g. `[...a, ...a]`).
            let items: Vec<Value> = a.borrow().clone();
            target.borrow_mut().extend(items);
        }
        Value::Range(r) => {
            let len = r.length();
            let mut out = target.borrow_mut();
            for i in 0..len {
                out.push(Value::Int(r.nth(i)));
            }
        }
        Value::Str(s) => {
            let mut out = target.borrow_mut();
            for c in s.chars() {
                out.push(Value::Str(c.to_string().into()));
            }
        }
        Value::Bytes(b) => {
            let src = b.borrow();
            let mut out = target.borrow_mut();
            for &byte in src.iter() {
                out.push(Value::Int(byte as i64));
            }
        }
        other => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::TypeMismatch(format!(
                    "cannot spread {} into array/call", other.type_name()
                )),
                line,
            ));
        }
    }
    Ok(())
}

fn make_iter(v: Value, line: u32) -> Result<IterState, RuntimeError> {
    match v {
        Value::Range(r) => Ok(IterState::Range {
            current: r.from,
            to: r.to,
            step: r.step,
            inclusive: r.inclusive,
            index: 0,
        }),
        Value::Array(a) => Ok(IterState::Array { array: a, index: 0 }),
        Value::Object(o) => {
            // An object whose `next` field is callable is an iterator
            // object (the `Iter` protocol); otherwise iterate entries.
            let is_iter = matches!(
                o.borrow().get("next"),
                Some(Value::Function(_)) | Some(Value::NativeFn(_))
            );
            if is_iter {
                Ok(IterState::IterObject { object: o, index: 0, done: false })
            } else {
                Ok(IterState::Object { object: o, index: 0 })
            }
        }
        Value::Map(m) => Ok(IterState::Map { map: m, index: 0 }),
        Value::Set(s) => Ok(IterState::Set { set: s, index: 0 }),
        Value::Bytes(b) => Ok(IterState::Bytes { bytes: b, index: 0 }),
        Value::Str(s) => Ok(IterState::String { string: s, char_index: 0, byte_index: 0 }),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "cannot iterate over {}", other.type_name()
            )),
            line,
        )),
    }
}

fn index_get(coll: &Value, key: &Value, line: u32) -> Result<Value, RuntimeError> {
    match coll {
        Value::Array(a) => {
            let arr = a.borrow();
            match key {
                Value::Int(n) => {
                    let idx = normalize_index(*n, arr.len());
                    Ok(idx.and_then(|i| arr.get(i).cloned()).unwrap_or(Value::Null))
                }
                Value::Range(r) => {
                    let items: Vec<Value> = range_indices(r, arr.len())
                        .into_iter()
                        .map(|i| arr[i].clone())
                        .collect();
                    Ok(Value::Array(gc::alloc_array(items)))
                }
                other => Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            }
        }
        Value::Range(r) => {
            let len = r.length();
            let idx = match key {
                Value::Int(n) => normalize_index(*n, len.max(0) as usize),
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            Ok(idx.map(|i| Value::Int(r.nth(i as i64))).unwrap_or(Value::Null))
        }
        Value::Object(o) => {
            let key = match key {
                Value::Str(s) => s.clone(),
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            Ok(o.borrow().get(&key).cloned().unwrap_or(Value::Null))
        }
        Value::Map(m) => {
            let key = MapKey::from_value(key, line)?;
            Ok(m.borrow().get(&key).cloned().unwrap_or(Value::Null))
        }
        Value::Set(s) => {
            let key = MapKey::from_value(key, line)?;
            Ok(Value::Bool(s.borrow().contains(&key)))
        }
        Value::Bytes(b) => {
            let bytes = b.borrow();
            match key {
                Value::Int(n) => {
                    let idx = normalize_index(*n, bytes.len());
                    Ok(idx.map(|i| Value::Int(bytes[i] as i64)).unwrap_or(Value::Null))
                }
                Value::Range(r) => {
                    let out: Vec<u8> = range_indices(r, bytes.len())
                        .into_iter()
                        .map(|i| bytes[i])
                        .collect();
                    Ok(Value::Bytes(gc::alloc_bytes(out)))
                }
                other => Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            }
        }
        Value::Str(s) => {
            match key {
                Value::Int(n) => {
                    let idx = normalize_index(*n, s.chars().count());
                    Ok(idx
                        .and_then(|i| s.chars().nth(i))
                        .map(|c| Value::Str(c.to_string().into()))
                        .unwrap_or(Value::Null))
                }
                Value::Range(r) => {
                    let chars: Vec<char> = s.chars().collect();
                    let out: String = range_indices(r, chars.len())
                        .into_iter()
                        .map(|i| chars[i])
                        .collect();
                    Ok(Value::Str(out.into()))
                }
                other => Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            }
        }
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!("cannot index {}", other.type_name())),
            line,
        )),
    }
}

fn index_set(coll: &Value, key: &Value, value: Value, line: u32) -> Result<(), RuntimeError> {
    match coll {
        Value::Array(a) => {
            let mut arr = a.borrow_mut();
            let idx = match key {
                Value::Int(n) => *n,
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            let len = arr.len() as i64;
            let real = if idx < 0 { idx + len } else { idx };
            if real < 0 || real >= len {
                return Err(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds(idx), line));
            }
            arr[real as usize] = value;
            Ok(())
        }
        Value::Object(o) => {
            let key: Rc<str> = match key {
                Value::Str(s) => s.clone(),
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            o.borrow_mut().insert(key, value);
            Ok(())
        }
        Value::Map(m) => {
            let key = MapKey::from_value(key, line)?;
            m.borrow_mut().insert(key, value);
            Ok(())
        }
        Value::Set(_) => Err(RuntimeError::new(
            RuntimeErrorKind::ImmutableTarget("set (use Set.add)".into()),
            line,
        )),
        Value::Bytes(b) => {
            let idx = match key {
                Value::Int(n) => *n,
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            let byte = match &value {
                Value::Int(n) if (0..=255).contains(n) => *n as u8,
                Value::Int(n) => return Err(RuntimeError::new(
                    RuntimeErrorKind::Raised(Value::Str(format!(
                        "bytes index assignment: byte value {n} out of range 0..=255"
                    ).into())),
                    line,
                )),
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::TypeMismatch(format!(
                        "bytes index assignment: expected Int 0..=255, got {}",
                        other.type_name()
                    )),
                    line,
                )),
            };
            let mut buf = b.borrow_mut();
            let len = buf.len() as i64;
            let real = if idx < 0 { idx + len } else { idx };
            if real < 0 || real >= len {
                return Err(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds(idx), line));
            }
            buf[real as usize] = byte;
            Ok(())
        }
        Value::Str(_) => Err(RuntimeError::new(
            RuntimeErrorKind::ImmutableTarget("string".into()),
            line,
        )),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!("cannot index {}", other.type_name())),
            line,
        )),
    }
}

fn normalize_index(idx: i64, len: usize) -> Option<usize> {
    let len_i = len as i64;
    let real = if idx < 0 { idx + len_i } else { idx };
    if real < 0 || real >= len_i { None } else { Some(real as usize) }
}

/// Resolve a `Range` index key into the element positions it selects.
/// Negative endpoints count from the end; positions outside `[0, len)`
/// are dropped — which clamps an over-long slice. Step and inclusivity
/// are honoured, so a descending range yields a reversed slice.
fn range_indices(r: &RangeData, len: usize) -> Vec<usize> {
    let len_i = len as i64;
    let resolve = |v: i64| if v < 0 { v.saturating_add(len_i) } else { v };
    let from = resolve(r.from);
    let to = resolve(r.to);
    let step = r.step;
    let mut out = Vec::new();
    if step == 0 {
        return out;
    }

    // Fast-forward past a long run of leading out-of-bounds positions so
    // `arr[-1_000_000_000..5]` does not spin a billion iterations.
    let mut v = from;
    if step > 0 && v < 0 {
        let i = (-v + step - 1) / step; // ceil((0 - v) / step)
        v = v.saturating_add(i.saturating_mul(step));
    } else if step < 0 && v > len_i - 1 {
        let i = (v - (len_i - 1) + (-step) - 1) / (-step);
        v = v.saturating_add(i.saturating_mul(step));
    }

    loop {
        let done = if step > 0 {
            if r.inclusive { v > to } else { v >= to }
        } else if r.inclusive {
            v < to
        } else {
            v <= to
        };
        if done {
            break;
        }
        if v >= 0 && v < len_i {
            out.push(v as usize);
        } else {
            break; // past the far end — every later position is OOB too
        }
        v = v.saturating_add(step);
    }
    out
}

fn type_err(op: &str, a: &Value, b: &Value, line: u32) -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::TypeMismatch(format!(
            "operator `{op}` does not apply to {} and {}",
            a.type_name(),
            b.type_name()
        )),
        line,
    )
}

fn overflow_err(line: u32) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Overflow, line)
}

fn stack_overflow_err(line: u32) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::StackOverflow, line)
}
