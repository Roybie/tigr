//! Bytecode container with a constant pool and a per-byte line table
//! for runtime error reporting.

use std::rc::Rc;

use crate::vm::opcode::OpCode;
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

    /// Pretty-print the bytecode for debugging.
    #[allow(dead_code)] // used during VM development
    pub fn disassemble(&self, name: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("== {name} ==\n"));
        let mut offset = 0;
        while offset < self.code.len() {
            offset = self.disassemble_instruction(offset, &mut out);
        }
        out
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
            other => {
                out.push_str(&format!("{:?}\n", other));
            }
        }
        next
    }
}
