//! Bytecode container with a constant pool and a per-byte line table
//! for runtime error reporting.

use std::path::PathBuf;
use std::sync::Arc;

use crate::vm::opcode::OpCode;
use crate::vm::source_map::SourceId;
use crate::vm::value::{Function, Value};

/// A compile-time constant in a [`Chunk`]'s pool. Distinct from
/// [`Value`] so a `Chunk` — and therefore an `Arc<Function>` — is
/// `Send + Sync`, letting compiled code be shared across actor threads
/// (v0.14 concurrency). The compiler only ever pools literals, so the
/// five variants here cover the whole pool; a heap value never reaches
/// it.
#[derive(Clone)]
pub enum Const {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// `Arc<str>` (the same backing as `Value::Str`) so a load is a
    /// refcount bump, not a fresh allocation — see `to_value`.
    Str(Arc<str>),
}

impl Const {
    /// Materialize a runtime [`Value`] — called by `OpCode::LoadConst`.
    /// `Str` shares the pooled `Arc<str>`: no allocation per load.
    pub fn to_value(&self) -> Value {
        match self {
            Const::Null => Value::Null,
            Const::Bool(b) => Value::Bool(*b),
            Const::Int(n) => Value::Int(*n),
            Const::Float(x) => Value::Float(*x),
            Const::Str(s) => Value::Str(s.clone()),
        }
    }

    /// Build a `Const` from a primitive `Value`. The compiler pools
    /// only literals, so a non-primitive here is a compiler bug.
    pub fn from_value(v: &Value) -> Const {
        match v {
            Value::Null => Const::Null,
            Value::Bool(b) => Const::Bool(*b),
            Value::Int(n) => Const::Int(*n),
            Value::Float(x) => Const::Float(*x),
            Value::Str(s) => Const::Str(s.clone()),
            other => unreachable!(
                "non-literal in constant pool: {}",
                other.type_name()
            ),
        }
    }
}

impl std::fmt::Debug for Const {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Mirror `Value`'s Debug so disassembly output is unchanged.
        write!(f, "{:?}", self.to_value())
    }
}

#[derive(Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<Const>,
    /// Function templates referenced by `OpCode::Closure` instructions
    /// in this chunk. Indexed by the operand following the opcode.
    /// `Arc` (not `Rc`) so compiled code can cross actor threads.
    pub functions: Vec<Arc<Function>>,
    /// One entry per byte of `code`. Source line for that byte. Used
    /// when reporting runtime errors.
    pub lines: Vec<u32>,
    /// Which registered source this chunk was compiled from. Stamped
    /// post-compile by the entry function (and recursively for every
    /// nested function chunk). `SourceId::UNKNOWN` until stamped.
    pub source: SourceId,
    /// Directory of the source file this chunk was compiled from, if
    /// known. The VM resolves runtime `import` paths relative to it.
    /// `None` for source compiled from a string (no file context).
    pub base_dir: Option<PathBuf>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk::default()
    }

    pub fn write_op(&mut self, op: OpCode, line: u32) {
        self.code.push(op as u8);
        self.lines.push(line);
    }

    pub fn write_byte(&mut self, byte: u8, line: u32) {
        self.code.push(byte);
        self.lines.push(line);
    }

    /// Write a 16-bit big-endian operand.
    pub fn write_u16(&mut self, value: u16, line: u32) {
        self.code.push((value >> 8) as u8);
        self.code.push((value & 0xff) as u8);
        self.lines.push(line);
        self.lines.push(line);
    }

    /// Patch a 16-bit big-endian value at `offset` (and `offset+1`).
    /// Used for forward-jump back-patching.
    pub fn patch_u16(&mut self, offset: usize, value: u16) {
        self.code[offset] = (value >> 8) as u8;
        self.code[offset + 1] = (value & 0xff) as u8;
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        ((self.code[offset] as u16) << 8) | (self.code[offset + 1] as u16)
    }

    /// Write a 32-bit big-endian operand. Pushes four `lines` entries so
    /// the per-byte line table stays the same length as `code`.
    pub fn write_u32(&mut self, value: u32, line: u32) {
        self.code.push((value >> 24) as u8);
        self.code.push((value >> 16) as u8);
        self.code.push((value >> 8) as u8);
        self.code.push((value & 0xff) as u8);
        self.lines.push(line);
        self.lines.push(line);
        self.lines.push(line);
        self.lines.push(line);
    }

    /// Patch a 32-bit big-endian value at `offset..offset+4`. Used for
    /// forward-jump back-patching.
    pub fn patch_u32(&mut self, offset: usize, value: u32) {
        self.code[offset] = (value >> 24) as u8;
        self.code[offset + 1] = (value >> 16) as u8;
        self.code[offset + 2] = (value >> 8) as u8;
        self.code[offset + 3] = (value & 0xff) as u8;
    }

    pub fn read_u32(&self, offset: usize) -> u32 {
        ((self.code[offset] as u32) << 24)
            | ((self.code[offset + 1] as u32) << 16)
            | ((self.code[offset + 2] as u32) << 8)
            | (self.code[offset + 3] as u32)
    }

    /// Add a constant and return its `u16` pool index (or error if the
    /// pool is already full). `LoadConst` carries a 2-byte index.
    pub fn add_constant(&mut self, value: Const) -> Result<u16, ()> {
        if self.constants.len() > u16::MAX as usize {
            return Err(());
        }
        let idx = self.constants.len() as u16;
        self.constants.push(value);
        Ok(idx)
    }

    /// Add a function template and return its `u16` index in this
    /// chunk's function table. `Closure` carries a 2-byte fn-index.
    pub fn add_function(&mut self, function: Arc<Function>) -> Result<u16, ()> {
        if self.functions.len() > u16::MAX as usize {
            return Err(());
        }
        let idx = self.functions.len() as u16;
        self.functions.push(function);
        Ok(idx)
    }

    /// Pretty-print this chunk's bytecode.
    pub fn disassemble(&self, name: &str) -> String {
        let mut out = String::new();
        self.disassemble_into(name, &mut out);
        out
    }

    /// Pretty-print this chunk and, recursively, every nested function
    /// chunk it references via `OpCode::Closure`.
    pub fn disassemble_recursive(&self, name: &str) -> String {
        let mut out = String::new();
        self.disassemble_rec_into(name, &mut out);
        out
    }

    fn disassemble_into(&self, name: &str, out: &mut String) {
        out.push_str(&format!("== {name} ==\n"));
        let mut offset = 0;
        while offset < self.code.len() {
            offset = self.disassemble_instruction(offset, out);
        }
    }

    fn disassemble_rec_into(&self, name: &str, out: &mut String) {
        self.disassemble_into(name, out);
        for (i, func) in self.functions.iter().enumerate() {
            let fname = func.name.clone().unwrap_or_else(|| format!("fn#{i}"));
            out.push('\n');
            func.chunk.disassemble_rec_into(&format!("{name} > {fname}"), out);
        }
    }

    fn disassemble_instruction(&self, offset: usize, out: &mut String) -> usize {
        out.push_str(&format!("{offset:04} "));
        let line = self.lines[offset];
        if offset > 0 && self.lines[offset - 1] == line {
            out.push_str("   | ");
        } else {
            out.push_str(&format!("{line:4} "));
        }
        let byte = self.code[offset];
        let Some(op) = OpCode::from_u8(byte) else {
            out.push_str(&format!("UNKNOWN_OP {byte}\n"));
            return offset + 1;
        };
        let operands = op.operand_bytes();
        let next = offset + 1 + operands;
        match op {
            OpCode::LoadConst => {
                let idx = self.read_u16(offset + 1);
                let val = &self.constants[idx as usize];
                out.push_str(&format!("LOAD_CONST  {idx:3} ; {val:?}\n"));
            }
            OpCode::LoadLocal => {
                let slot = self.code[offset + 1];
                out.push_str(&format!("LOAD_LOCAL  {slot:3}\n"));
            }
            OpCode::StoreLocal => {
                let slot = self.code[offset + 1];
                out.push_str(&format!("STORE_LOCAL {slot:3}\n"));
            }
            // `Closure` has variable-length operands: a u16 fn-index
            // followed by 2 bytes per captured upvalue. Compute the
            // real instruction width from the referenced template.
            OpCode::Closure => {
                let idx = self.read_u16(offset + 1);
                let func = self.functions.get(idx as usize);
                let nup = func.map(|f| f.upvalues.len()).unwrap_or(0);
                let fname = func
                    .and_then(|f| f.name.as_deref())
                    .unwrap_or("<anonymous>");
                out.push_str(&format!(
                    "CLOSURE     {idx:3} ; {fname} ({nup} upvalue(s))\n"
                ));
                return offset + 3 + nup * 2;
            }
            // Forward jumps: annotate the absolute target offset.
            OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::JumpIfNotNull
            | OpCode::PushTry
            | OpCode::IterNext
            | OpCode::IterNext2 => {
                let arg = self.read_u32(offset + 1);
                out.push_str(&format!("{op:?} -> {:04}\n", next + arg as usize));
            }
            OpCode::Loop => {
                let arg = self.read_u32(offset + 1);
                out.push_str(&format!("Loop -> {:04}\n", next - arg as usize));
            }
            other => {
                if operands == 1 {
                    out.push_str(&format!("{other:?} {}\n", self.code[offset + 1]));
                } else {
                    out.push_str(&format!("{other:?}\n"));
                }
            }
        }
        next
    }

    /// Byte length of the instruction at `offset`, accounting for
    /// `Closure`'s variable-length upvalue operands (2-byte fn-index
    /// plus 2 bytes per captured upvalue).
    fn instr_len(&self, offset: usize) -> usize {
        match OpCode::from_u8(self.code[offset]) {
            Some(OpCode::Closure) => {
                let idx = self.read_u16(offset + 1) as usize;
                let nup = self.functions.get(idx).map(|f| f.upvalues.len()).unwrap_or(0);
                3 + nup * 2
            }
            Some(op) => 1 + op.operand_bytes(),
            None => 1,
        }
    }

    /// Peephole pass — jump threading (v0.12). A forward jump whose
    /// target is an unconditional `Jump` is retargeted past it, to the
    /// chain's final destination. Only operand bytes are rewritten;
    /// `code` and `lines` do not move, so no offset relocation is
    /// needed. Each hop strictly increases the target offset, so the
    /// chain always terminates. Idempotent.
    pub fn thread_jumps(&mut self) {
        use OpCode::*;
        let mut offset = 0;
        while offset < self.code.len() {
            let Some(op) = OpCode::from_u8(self.code[offset]) else {
                offset += 1;
                continue;
            };
            if matches!(op, Jump | JumpIfFalse | JumpIfTrue | JumpIfNotNull) {
                // The operand is a forward distance from `next`, the
                // byte after this 5-byte jump instruction (1 opcode +
                // 4-byte operand).
                let next = offset + 5;
                let mut target = next + self.read_u32(offset + 1) as usize;
                let mut hops = 0;
                while target + 5 <= self.code.len()
                    && self.code[target] == Jump as u8
                    && hops < self.code.len()
                {
                    target = target + 5 + self.read_u32(target + 1) as usize;
                    hops += 1;
                }
                if let Ok(dist) = u32::try_from(target.saturating_sub(next)) {
                    self.patch_u32(offset + 1, dist);
                }
            }
            offset += self.instr_len(offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::opcode::OpCode;

    #[test]
    fn thread_jumps_collapses_a_jump_chain() {
        // Built via the chunk API so it survives operand-width changes.
        // A jump is 5 bytes (1 opcode + 4-byte operand).
        //  0: Jump A  -> 5  (the second jump)
        //  5: Jump B  -> 13 (Return)
        // 10: PushNull / PushNull / PushNull
        // 13: Return
        let mut chunk = Chunk::new();

        chunk.write_op(OpCode::Jump, 1);
        let a = chunk.code.len(); // operand offset of jump A
        chunk.write_u32(0xffff_ffff, 1);

        let b_start = chunk.code.len(); // = 5
        chunk.write_op(OpCode::Jump, 1);
        let b = chunk.code.len(); // operand offset of jump B
        chunk.write_u32(0xffff_ffff, 1);

        chunk.write_op(OpCode::PushNull, 1);
        chunk.write_op(OpCode::PushNull, 1);
        chunk.write_op(OpCode::PushNull, 1);
        let ret = chunk.code.len(); // = 13
        chunk.write_op(OpCode::Return, 1);

        // A targets jump B; B targets Return. Distance is measured from
        // the byte after the 5-byte jump instruction (operand + 4).
        chunk.patch_u32(a, (b_start - (a + 4)) as u32);
        chunk.patch_u32(b, (ret - (b + 4)) as u32);

        chunk.thread_jumps();

        // A is retargeted straight at Return: 13 - (a + 4) = 8.
        assert_eq!(chunk.read_u32(a), (ret - (a + 4)) as u32);
        // B is unchanged: 13 - (b + 4) = 3.
        assert_eq!(chunk.read_u32(b), (ret - (b + 4)) as u32);
    }
}
