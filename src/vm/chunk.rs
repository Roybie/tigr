//! Bytecode container with a constant pool and a per-byte line table
//! for runtime error reporting.

use std::path::PathBuf;
use std::rc::Rc;

use crate::vm::opcode::OpCode;
use crate::vm::source_map::SourceId;
use crate::vm::value::{Function, Value};

#[derive(Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<Value>,
    /// Function templates referenced by `OpCode::Closure` instructions
    /// in this chunk. Indexed by the operand following the opcode.
    pub functions: Vec<Rc<Function>>,
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

    /// Add a constant and return its index (or error if the pool is
    /// already full).
    pub fn add_constant(&mut self, value: Value) -> Result<u8, ()> {
        if self.constants.len() >= 256 {
            return Err(());
        }
        let idx = self.constants.len() as u8;
        self.constants.push(value);
        Ok(idx)
    }

    /// Add a function template and return its index in this chunk's
    /// function table.
    pub fn add_function(&mut self, function: Rc<Function>) -> Result<u8, ()> {
        if self.functions.len() >= 256 {
            return Err(());
        }
        let idx = self.functions.len() as u8;
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
                let idx = self.code[offset + 1];
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
            // `Closure` has variable-length operands: a u8 fn-index
            // followed by 2 bytes per captured upvalue. Compute the
            // real instruction width from the referenced template.
            OpCode::Closure => {
                let idx = self.code[offset + 1];
                let func = self.functions.get(idx as usize);
                let nup = func.map(|f| f.upvalues.len()).unwrap_or(0);
                let fname = func
                    .and_then(|f| f.name.as_deref())
                    .unwrap_or("<anonymous>");
                out.push_str(&format!(
                    "CLOSURE     {idx:3} ; {fname} ({nup} upvalue(s))\n"
                ));
                return offset + 2 + nup * 2;
            }
            // Forward jumps: annotate the absolute target offset.
            OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::JumpIfNotNull
            | OpCode::PushTry
            | OpCode::IterNext
            | OpCode::IterNext2 => {
                let arg = self.read_u16(offset + 1);
                out.push_str(&format!("{op:?} -> {:04}\n", next + arg as usize));
            }
            OpCode::Loop => {
                let arg = self.read_u16(offset + 1);
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
    /// `Closure`'s variable-length upvalue operands (1 byte fn-index
    /// plus 2 bytes per captured upvalue).
    fn instr_len(&self, offset: usize) -> usize {
        match OpCode::from_u8(self.code[offset]) {
            Some(OpCode::Closure) => {
                let idx = self.code[offset + 1] as usize;
                let nup = self.functions.get(idx).map(|f| f.upvalues.len()).unwrap_or(0);
                2 + nup * 2
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
                // byte after this 3-byte jump instruction.
                let next = offset + 3;
                let mut target = next + self.read_u16(offset + 1) as usize;
                let mut hops = 0;
                while target + 3 <= self.code.len()
                    && self.code[target] == Jump as u8
                    && hops < self.code.len()
                {
                    target = target + 3 + self.read_u16(target + 1) as usize;
                    hops += 1;
                }
                if let Ok(dist) = u16::try_from(target.saturating_sub(next)) {
                    self.patch_u16(offset + 1, dist);
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
        // 0: Jump  -> 3   (operand 0)
        // 3: Jump  -> 9   (operand 3)
        // 6: PushNull / PushNull / PushNull
        // 9: Return
        let mut chunk = Chunk::new();
        let j = OpCode::Jump as u8;
        chunk.code = vec![
            j, 0, 0, // 0: Jump -> 3
            j, 0, 3, // 3: Jump -> 9
            OpCode::PushNull as u8,
            OpCode::PushNull as u8,
            OpCode::PushNull as u8,
            OpCode::Return as u8, // 9
        ];
        chunk.lines = vec![1; chunk.code.len()];

        chunk.thread_jumps();

        // The jump at 0 should now point straight at 9: 9 - (0 + 3) = 6.
        assert_eq!(chunk.read_u16(1), 6);
        // The second jump is unchanged: 9 - (3 + 3) = 3.
        assert_eq!(chunk.read_u16(4), 3);
    }
}
