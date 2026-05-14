//! Bytecode opcodes.
//!
//! Phase 1 only emits the arithmetic / local / return ops. Later phases
//! add control flow, calls, collections, etc. The discriminant order is
//! stable for clarity in disassembly.
//!
//! Opcodes that need a single-byte operand are listed alongside their
//! consumer; the chunk encoding uses `u8` for the opcode followed by
//! its inline operand bytes.

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    /// Push constant from the constant pool.
    /// Operand: u8 const-index.
    LoadConst,

    /// Push the value at the given stack slot.
    /// Operand: u8 slot.
    LoadLocal,

    /// Pop the top of stack and write it to the given slot. Leaves the
    /// value also on the stack so an assignment expression evaluates to
    /// the assigned value.
    /// Operand: u8 slot.
    StoreLocal,

    /// Pop the top of stack.
    Pop,

    /// Push `null`.
    PushNull,

    /// Push a copy of the top of stack. Used so that `x := expr`
    /// produces a value (the duplicate) while leaving the local in
    /// place below it.
    Dup,

    // Arithmetic — pop b, pop a, push (a OP b).
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,

    // Unary — pop a, push (OP a).
    Negate,

    /// End the chunk; pop the top of stack and return it as the chunk's
    /// value.
    Return,
}

impl OpCode {
    pub fn from_u8(b: u8) -> Option<Self> {
        use OpCode::*;
        Some(match b {
            0 => LoadConst,
            1 => LoadLocal,
            2 => StoreLocal,
            3 => Pop,
            4 => PushNull,
            5 => Dup,
            6 => Add,
            7 => Sub,
            8 => Mul,
            9 => Div,
            10 => Mod,
            11 => Pow,
            12 => Negate,
            13 => Return,
            _ => return None,
        })
    }

    /// Number of inline operand bytes following this opcode.
    #[allow(dead_code)] // used by disassembler
    pub fn operand_bytes(self) -> usize {
        use OpCode::*;
        match self {
            LoadConst | LoadLocal | StoreLocal => 1,
            _ => 0,
        }
    }
}
