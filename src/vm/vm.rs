//! Stack-based bytecode interpreter.
//!
//! Phase 1: a single chunk (the program) with a single value stack.
//! Call frames arrive in Phase 4 once functions land.

use crate::vm::chunk::Chunk;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::opcode::OpCode;
use crate::vm::value::Value;

pub struct Vm {
    stack: Vec<Value>,
}

impl Vm {
    pub fn new() -> Self {
        Vm { stack: Vec::with_capacity(256) }
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<Value, RuntimeError> {
        let mut ip = 0;
        while ip < chunk.code.len() {
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
                    let v = self.stack[slot].clone();
                    self.stack.push(v);
                }
                OpCode::StoreLocal => {
                    let slot = chunk.code[ip] as usize;
                    ip += 1;
                    let top = self.peek(line)?.clone();
                    self.stack[slot] = top;
                }
                OpCode::Pop => {
                    self.pop(line)?;
                }
                OpCode::PushNull => {
                    self.stack.push(Value::Null);
                }
                OpCode::Dup => {
                    let top = self.peek(line)?.clone();
                    self.stack.push(top);
                }
                OpCode::Add => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(arith_add(a, b, line)?);
                }
                OpCode::Sub => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(arith_sub(a, b, line)?);
                }
                OpCode::Mul => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(arith_mul(a, b, line)?);
                }
                OpCode::Div => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(arith_div(a, b, line)?);
                }
                OpCode::Mod => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(arith_mod(a, b, line)?);
                }
                OpCode::Pow => {
                    let b = self.pop(line)?;
                    let a = self.pop(line)?;
                    self.stack.push(arith_pow(a, b, line)?);
                }
                OpCode::Negate => {
                    let v = self.pop(line)?;
                    self.stack.push(arith_neg(v, line)?);
                }
                OpCode::Return => {
                    return self.pop(line);
                }
            }
        }
        // ran off the end without RETURN — shouldn't happen for a
        // well-formed chunk, but return null as a safe default.
        Ok(Value::Null)
    }

    fn pop(&mut self, line: u32) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::StackUnderflow, line)
        })
    }

    fn peek(&self, line: u32) -> Result<&Value, RuntimeError> {
        self.stack.last().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::StackUnderflow, line)
        })
    }
}

// ---- arithmetic helpers (spec §6.2) ----

fn arith_add(a: Value, b: Value, line: u32) -> Result<Value, RuntimeError> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Ok(Int(x + y)),
        (Int(x), Float(y)) => Ok(Float(x as f64 + y)),
        (Float(x), Int(y)) => Ok(Float(x + y as f64)),
        (Float(x), Float(y)) => Ok(Float(x + y)),
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
        // spec §6.2: Int/Int → Int when divides evenly, else Float
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

// spec §6.2: `^` always produces Float
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
