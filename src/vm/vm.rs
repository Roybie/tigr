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

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::opcode::OpCode;
use crate::vm::source_map::SourceMap;
use crate::vm::stdlib;
use crate::vm::value::{Closure, Function, IterState, RangeData, Upvalue, Value};

struct CallFrame {
    closure: Rc<Closure>,
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

pub struct Vm {
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    globals: Vec<Value>,
    open_upvalues: Vec<Rc<RefCell<Upvalue>>>,
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

        let main_closure = Rc::new(Closure {
            function: Rc::new(main),
            upvalues: Vec::new(),
        });
        // slot 0 of main frame = the main closure itself
        self.stack.push(Value::Function(main_closure.clone()));
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
                    if !self.try_catch(0, &err) {
                        return Err(err);
                    }
                    // Caught — frame state is now pointing at catch_pc
                    // with the error value on the stack. Loop back into
                    // exec to continue from there.
                }
            }
        }
    }

    /// Fill in `err.source` from the chunk on top of the call stack
    /// when it isn't already set. Called at the `exec` boundary —
    /// before `try_catch` may unwind frames.
    fn stamp_error_source(&self, err: &mut RuntimeError) {
        if !err.source.is_unknown() {
            return;
        }
        if let Some(top) = self.frames.last() {
            err.source = top.closure.function.chunk.source;
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
    fn try_catch(&mut self, floor: usize, err: &RuntimeError) -> bool {
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
                        Value::Object(Rc::new(RefCell::new(m)))
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
        let dummy = Rc::new(Closure {
            function: Rc::new(crate::vm::value::Function {
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
        self.stack.push(Value::Function(dummy.clone()));
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
        closure: Rc<Closure>,
        snapshot_len: usize,
    ) -> Result<Value, RuntimeError> {
        debug_assert!(matches!(self.frames[0].kind, FrameKind::Repl));
        // Install the new line's closure at slot 0 and reset ip.
        self.stack[0] = Value::Function(closure.clone());
        self.frames[0].closure = closure;
        self.frames[0].ip = 0;
        self.frames[0].try_frames.clear();
        loop {
            match self.exec() {
                Ok(v) => return Ok(v), // Halt exit
                Err(mut err) => {
                    self.stamp_error_source(&mut err);
                    if !self.try_catch(0, &err) {
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
            // Snapshot the current frame's chunk for this iteration.
            // Cloning the Rc<Closure> is cheap and lets us read from
            // the chunk while mutating self.stack / self.frames.
            let closure = Rc::clone(&self.frames.last().expect("at least one frame").closure);
            let function_rc = Rc::clone(&closure.function);
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
                    self.stack.push(chunk.constants[idx].clone());
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
                    self.stack.push(Value::Array(Rc::new(RefCell::new(items))));
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
                    self.stack.push(Value::Object(Rc::new(RefCell::new(obj))));
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
                            let arity = c.function.arity;
                            let has_rest = c.function.has_rest;
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
                            closure.upvalues[index].clone()
                        };
                        upvalues.push(upvalue);
                    }
                    let new_closure = Rc::new(Closure { function, upvalues });
                    self.stack.push(Value::Function(new_closure));
                }
                OpCode::GetUpvalue => {
                    let idx = chunk.code[ip] as usize;
                    ip += 1;
                    let upv = closure.upvalues[idx].clone();
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
                    let upv = closure.upvalues[idx].clone();
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
                    self.stack.push(Value::Iter(Rc::new(RefCell::new(iter))));
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
                            match self.iter_object_pull(&obj, line)? {
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
                            match self.iter_object_pull(&obj, line)? {
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
                    let v = self.pop(line)?;
                    if !matches!(v, Value::Null) {
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
                            let o = o.clone();
                            self.frames.last_mut().unwrap().ip = ip;
                            while let Some(v) = self.iter_object_pull(&o, line)? {
                                target_arr.borrow_mut().push(v);
                            }
                            continue;
                        }
                    }
                    extend_array(&target_arr, src, line)?;
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
                            let arity = c.function.arity;
                            let has_rest = c.function.has_rest;
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
                    let out = match arr_val {
                        Value::Array(a) => {
                            let src = a.borrow();
                            let len = src.len() as i64;
                            let real = if start < 0 { (start + len).max(0) } else { start.min(len) };
                            let real = real.max(0) as usize;
                            src[real..].to_vec()
                        }
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "cannot slice {} (only Array supported)",
                                other.type_name()
                            )),
                            line,
                        )),
                    };
                    self.stack.push(Value::Array(Rc::new(RefCell::new(out))));
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
                    self.stack.push(Value::Object(Rc::new(RefCell::new(out))));
                }
                OpCode::Import => {
                    let path_val = self.pop(line)?;
                    let path_str = match path_val {
                        Value::Str(s) => s,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: Import path not string: {}",
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
                            let mc = Rc::new(Closure {
                                function: Rc::new(main),
                                upvalues: Vec::new(),
                            });
                            self.frames.last_mut().unwrap().ip = ip;
                            let base = self.stack.len();
                            self.stack.push(Value::Function(mc.clone()));
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

                    // File path: cache → in-flight check → compile and
                    // push as a new frame on this same Vm. The frame
                    // is tagged `Import(path)` so the Return opcode
                    // can write the cache entry.
                    let path = PathBuf::from(&*path_str);
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
                    let main_closure = Rc::new(Closure {
                        function: Rc::new(main),
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
            self.stack.push(Value::Array(Rc::new(RefCell::new(Vec::new()))));
        } else {
            let rest_start = args_start + arity;
            let extras: Vec<Value> = self.stack.drain(rest_start..).collect();
            self.stack.push(Value::Array(Rc::new(RefCell::new(extras))));
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
                let arity = c.function.arity;
                let has_rest = c.function.has_rest;
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
                            if self.try_catch(floor, &err) {
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
        obj: &Rc<RefCell<IndexMap<Rc<str>, Value>>>,
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

    fn capture_upvalue(&mut self, stack_slot: usize) -> Rc<RefCell<Upvalue>> {
        for up in &self.open_upvalues {
            if let Upvalue::Open(slot) = *up.borrow() {
                if slot == stack_slot {
                    return up.clone();
                }
            }
        }
        let new_up = Rc::new(RefCell::new(Upvalue::Open(stack_slot)));
        self.open_upvalues.push(new_up.clone());
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
}

fn underflow(line: u32) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::StackUnderflow, line)
}

// -- arithmetic helpers (spec §6.2 + §7.1) --

fn arith_add(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Ok(Int(x.checked_add(y).ok_or_else(|| overflow_err(line))?)),
        (Int(x), Float(y)) => Ok(Float(x as f64 + y)),
        (Float(x), Int(y)) => Ok(Float(x + y as f64)),
        (Float(x), Float(y)) => Ok(Float(x + y)),
        (Str(x), Str(y)) => {
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(&x);
            s.push_str(&y);
            Ok(Str(s.into()))
        }
        (Array(x), Array(y)) => {
            let mut v: Vec<Value> = x.borrow().clone();
            v.extend(y.borrow().iter().cloned());
            Ok(Array(Rc::new(RefCell::new(v))))
        }
        (Array(x), other) => {
            let mut v: Vec<Value> = x.borrow().clone();
            v.push(other);
            Ok(Array(Rc::new(RefCell::new(v))))
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
        (a, b) => Err(type_err("%", &a, &b, line)),
    }
}

fn arith_pow(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    let (x, y) = match (a, b) {
        (Int(x), Int(y)) => (x as f64, y as f64),
        (Int(x), Float(y)) => (x as f64, y),
        (Float(x), Int(y)) => (x, y as f64),
        (Float(x), Float(y)) => (x, y),
        (a, b) => return Err(type_err("^^", &a, &b, line)),
    };
    Ok(Float(x.powf(y)))
}

fn arith_neg(a: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match a {
        Int(x) => Ok(Int(x.checked_neg().ok_or_else(|| overflow_err(line))?)),
        Float(x) => Ok(Float(-x)),
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
    target: &Rc<RefCell<Vec<Value>>>,
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
            let idx = match key {
                Value::Int(n) => normalize_index(*n, arr.len()),
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            Ok(idx.and_then(|i| arr.get(i).cloned()).unwrap_or(Value::Null))
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
        Value::Str(s) => {
            let idx = match key {
                Value::Int(n) => normalize_index(*n, s.chars().count()),
                other => return Err(RuntimeError::new(
                    RuntimeErrorKind::InvalidIndexType(other.type_name().into()),
                    line,
                )),
            };
            Ok(idx
                .and_then(|i| s.chars().nth(i))
                .map(|c| Value::Str(c.to_string().into()))
                .unwrap_or(Value::Null))
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
