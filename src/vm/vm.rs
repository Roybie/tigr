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
use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::opcode::OpCode;
use crate::vm::stdlib;
use crate::vm::value::{Closure, Function, IterState, RangeData, Upvalue, Value};

struct CallFrame {
    closure: Rc<Closure>,
    ip: usize,
    /// Index in `vm.stack` corresponding to slot 0 of this frame.
    base_slot: usize,
}

pub struct Vm {
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    globals: Vec<Value>,
    open_upvalues: Vec<Rc<RefCell<Upvalue>>>,
}

impl Vm {
    pub fn new() -> Self {
        Vm {
            frames: Vec::with_capacity(64),
            stack: Vec::with_capacity(256),
            globals: stdlib::builtins(),
            open_upvalues: Vec::new(),
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
        });
        self.exec()
    }

    fn exec(&mut self) -> Result<Value, RuntimeError> {
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
                OpCode::Sub => self.binop_arith(line, arith_sub)?,
                OpCode::Mul => self.binop_arith(line, arith_mul)?,
                OpCode::Div => self.binop_arith(line, arith_div)?,
                OpCode::Mod => self.binop_arith(line, arith_mod)?,
                OpCode::Pow => self.binop_arith(line, arith_pow)?,
                OpCode::Negate => {
                    let v = self.stack.pop().ok_or_else(|| underflow(line))?;
                    self.stack.push(arith_neg(v, line)?);
                }

                OpCode::Return => {
                    let result = self.stack.pop().ok_or_else(|| underflow(line))?;
                    let frame = self.frames.pop().unwrap();
                    self.close_upvalues(frame.base_slot);
                    self.stack.truncate(frame.base_slot);
                    if self.frames.is_empty() {
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
                            let result = (nf.func)(&args)?;
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
                    let next = match &iter_val {
                        Value::Iter(it) => it.borrow_mut().next(),
                        _ => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(
                                "internal: IterNext on non-iter".into()
                            ),
                            line,
                        )),
                    };
                    match next {
                        Some((_counter, value)) => self.stack.push(value),
                        None => ip += dist as usize,
                    }
                }
                OpCode::IterNext2 => {
                    let dist = chunk.read_u16(ip);
                    ip += 2;
                    let iter_val = self.stack.last().ok_or_else(|| underflow(line))?.clone();
                    let next = match &iter_val {
                        Value::Iter(it) => it.borrow_mut().next(),
                        _ => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(
                                "internal: IterNext2 on non-iter".into()
                            ),
                            line,
                        )),
                    };
                    match next {
                        Some((counter, value)) => {
                            self.stack.push(counter);
                            self.stack.push(value);
                        }
                        None => ip += dist as usize,
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
                            let result = (nf.func)(&call_args)?;
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
                    let path = match path_val {
                        Value::Str(s) => s,
                        other => return Err(RuntimeError::new(
                            RuntimeErrorKind::TypeMismatch(format!(
                                "internal: Import path not string: {}",
                                other.type_name()
                            )),
                            line,
                        )),
                    };
                    let value = crate::vm::run_file(std::path::Path::new(&*path))
                        .map_err(|e| RuntimeError::new(
                            RuntimeErrorKind::ImportFailed(
                                path.to_string(), format!("{e}"),
                            ),
                            line,
                        ))?;
                    self.stack.push(value);
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
        (Int(x), Int(y)) => Ok(Int(x + y)),
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
        (Int(x), Int(y)) => Ok(Int(x - y)),
        (Int(x), Float(y)) => Ok(Float(x as f64 - y)),
        (Float(x), Int(y)) => Ok(Float(x - y as f64)),
        (Float(x), Float(y)) => Ok(Float(x - y)),
        (a, b) => Err(type_err("-", &a, &b, line)),
    }
}

fn arith_mul(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Ok(Int(x * y)),
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
        (a, b) => return Err(type_err("^", &a, &b, line)),
    };
    Ok(Float(x.powf(y)))
}

fn arith_neg(a: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match a {
        Int(x) => Ok(Int(-x)),
        Float(x) => Ok(Float(-x)),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!("cannot negate {}", other.type_name())),
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
        Value::Object(o) => Ok(IterState::Object { object: o, index: 0 }),
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
