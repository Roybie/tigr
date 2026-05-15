//! Bytecode opcodes.
//!
//! Phase 1: arithmetic / locals / return.
//! Phase 2: control flow, comparison, logical, scope close.
//!
//! Each opcode is a single byte. Inline operand encoding:
//! - 1-byte operand: LoadConst / LoadLocal / StoreLocal / CloseScope
//! - 2-byte operand (big-endian): Jump / Loop / JumpIfFalse / JumpIfTrue
//!
//! 2-byte jumps give a 64KiB max chunk size — fine for a hobby VM. If a
//! chunk grows past that the compiler errors out.

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    // -- Phase 1 --
    /// Push constant from the constant pool. Operand: u8 const-index.
    LoadConst,
    /// Push the value at the given stack slot. Operand: u8 slot.
    LoadLocal,
    /// Pop top, write it to the given slot, push it back. Operand: u8 slot.
    /// (Leaves the value so an assignment expression evaluates to the value.)
    StoreLocal,
    /// Pop the top of stack.
    Pop,
    /// Push `null`.
    PushNull,
    /// Push a copy of the top of stack.
    Dup,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Negate,

    /// End the chunk; pop the top of stack and return it.
    Return,

    // -- Phase 2 --
    /// Pop two, push (a == b). Per spec §6.3.
    Eq,
    /// Pop two, push (a != b).
    Neq,
    /// Pop two, push (a < b). Numbers only for now.
    Lt,
    Le,
    Gt,
    Ge,

    /// Pop one, push `!is_truthy(x)`.
    Not,

    /// Unconditional forward jump. Operand: u16 BE byte offset added to ip.
    Jump,
    /// Unconditional backward jump. Operand: u16 BE byte offset subtracted from ip.
    Loop,
    /// Peek top; if falsy, jump forward by operand. Does NOT pop. Operand: u16 BE.
    JumpIfFalse,
    /// Peek top; if truthy, jump forward by operand. Does NOT pop. Operand: u16 BE.
    JumpIfTrue,

    /// Close a scope: take the top, pop `n` values from below it, then
    /// push the top back. Used at end of `{ ... }` to discard locals
    /// while preserving the scope's value. Operand: u8 count.
    CloseScope,

    // -- Phase 3 --
    /// Pop `n` values, push them as an Array (in source order).
    /// Operand: u8 n.
    MakeArray,
    /// Pop `2n` values (key, value, key, value, ...) and push as Object.
    /// Operand: u8 n.
    MakeObject,
    /// Pop key, pop collection, push collection[key].
    IndexGet,
    /// Pop value, pop key, pop collection. Set collection[key]=value;
    /// push value.
    IndexSet,
    /// Pop one, push its length (#x). Spec §6.6.
    Len,
    /// Call: pop n args, pop callee, push result. Operand: u8 n.
    Call,
    /// Duplicate the top two values: `[..., a, b]` → `[..., a, b, a, b]`.
    /// Used for compound indexed assignment to evaluate `obj[key]`
    /// once.
    Dup2,

    // -- Phase 4 --
    /// Read function template at `chunk.functions[idx]`. For each of
    /// its `upvalues.len()` upvalues, read 2 bytes (`is_local`, then
    /// `index`) to capture or thread through. Push a Closure value.
    /// Operand: u8 fn_idx, then 2 bytes per upvalue.
    Closure,
    /// Push the value of the current closure's upvalue at `idx`.
    /// Operand: u8 upvalue index.
    GetUpvalue,
    /// Write top to the current closure's upvalue at `idx`. Leaves
    /// value on stack. Operand: u8 upvalue index.
    SetUpvalue,
    /// Push the value of the global at `idx`. Globals hold built-ins
    /// (spec §13). Operand: u8 global index.
    LoadGlobal,

    // -- Phase 5 --
    /// Construct a Range. Operand: 1 byte flags — bit 0 = inclusive,
    /// bit 1 = has_step. Pops `to`, `from` (and `step` if has_step).
    /// If step is missing, picks +1 or -1 at runtime depending on
    /// whether `from <= to`.
    MakeRange,
    /// Pop iterable (Range/Array/Object/String) and push a fresh
    /// IterState value that wraps it with an internal position.
    MakeIter,
    /// Peek the IterState at the top of stack. If it has another
    /// element: advance it and push 1 value (one-var loop forms). If
    /// exhausted: jump forward by the u16 operand without pushing.
    IterNext,
    /// Like `IterNext` but pushes 2 values per advance — the counter
    /// for Range/Array/String, the key for Object, plus the element.
    IterNext2,
    /// Pop the top value; if it is not `null`, append it to the Array
    /// stored at local slot `slot`. Used to collect body values inside
    /// `for[]` / `while[]`. Operand: u8 slot.
    IterAppend,
    /// Truncate the stack to `base_slot + n`, closing any open upvalues
    /// at the popped slots. Used by `break` to unwind arbitrary
    /// nesting before jumping to the loop exit. Operand: u8 n.
    Unwind,

    // -- Phase 6 --
    /// Pop the top value and append it to the Array now on top of
    /// stack (mutated in place). Used by array literals with mixed
    /// regular elements and spreads.
    ArrayPush,
    /// Pop the top value (must be iterable: Range/Array/String) and
    /// extend the Array now on top of stack with each element.
    ArrayExtend,
    /// Pop the top value (must be Object) and merge its entries into
    /// the Object now on top of stack — preserving insertion order,
    /// with later keys winning (spec §6.6).
    ObjectMerge,
    /// Spread-aware call. Pops args-array, pops callee, calls callee
    /// with each element of the array as a separate argument. Allows
    /// runtime-determined arity for `f(x, ...args, y)`.
    CallSpread,
    /// Concatenate the top `n` values into a single string by `str`
    /// coercing each then joining. Used by string interpolation.
    /// Operand: u8 n.
    ConcatN,

    // -- Phase 7 --
    /// Pop `start_index` (Int), pop array, push the slice of array
    /// elements from `start_index` onwards as a fresh Array. Out-of-
    /// range start clamps to an empty array. Used by `[a, ...rest]`
    /// patterns.
    SliceFrom,
    /// Pop a `keys` Array and a source Object; push a new Object
    /// containing all source entries whose keys are NOT in `keys`,
    /// preserving insertion order. Used by `${a, ...rest}` patterns.
    ObjRest,
    /// Pop a string from the stack, read+lex+parse+compile+run that
    /// file as a standalone program, then push its final value.
    /// Spec §12: each `import` re-evaluates (no caching). The path
    /// is resolved at compile time relative to the importing file.
    Import,

    // -- v0.3 — try/catch/raise --
    /// Push a try-frame onto the current call frame. Records the
    /// current stack length and the catch PC (= ip + operand). When a
    /// raise / runtime error fires, the VM walks the try-frame stack,
    /// truncates the value stack to the recorded length, pushes the
    /// error value, and jumps to the catch PC. Operand: u16 BE forward
    /// distance (same encoding as `Jump`).
    PushTry,
    /// Pop the top try-frame from the current call frame. Emitted at
    /// the success-path end of a `try` expression so that the catch
    /// path is no longer live.
    PopTry,
    /// Pop the top value as an error message (coerced to string),
    /// then unwind: close upvalues, pop call frames as needed, and
    /// transfer control to the nearest active try-frame's catch PC.
    /// If no try-frame is active in any frame, the program exits with
    /// a runtime error.
    Raise,

    // -- v0.3 Phase 5 — REPL --
    /// Pop the top value and exit `exec` with `Ok(value)` WITHOUT
    /// popping the current call frame or closing its upvalues. The
    /// REPL emits this at the end of each compiled line so the
    /// session state (locals at slots 1..M) survives between lines.
    Halt,

    // -- v0.5 — bitwise (Int-only) --
    /// Pop two, push `a & b`. Both must be Int.
    BitAnd,
    /// Pop two, push `a | b`. Both must be Int.
    BitOr,
    /// Pop two, push `a ^ b`. Both must be Int.
    BitXor,
    /// Pop one, push `!x` (bitwise complement). Must be Int.
    BitNot,
    /// Pop two, push `a << b`. Both Int; raises if `b` is out of
    /// `0..64`.
    Shl,
    /// Pop two, push `a >> b` — arithmetic (sign-preserving) shift.
    /// Both Int; raises if `b` is out of `0..64`.
    Shr,

    // -- v0.5 — match --
    /// Peek the top of stack; push `Bool(true)` if its runtime type
    /// matches the operand tag, else `Bool(false)`. Does NOT pop the
    /// subject. Operand: u8 tag —
    /// 0=Int 1=Float 2=Bool 3=Str 4=Array 5=Object 6=Range 7=Null
    /// 8=Number(Int|Float) 9=callable(Function|NativeFn).
    TypeTest,
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
            14 => Eq,
            15 => Neq,
            16 => Lt,
            17 => Le,
            18 => Gt,
            19 => Ge,
            20 => Not,
            21 => Jump,
            22 => Loop,
            23 => JumpIfFalse,
            24 => JumpIfTrue,
            25 => CloseScope,
            26 => MakeArray,
            27 => MakeObject,
            28 => IndexGet,
            29 => IndexSet,
            30 => Len,
            31 => Call,
            32 => Dup2,
            33 => Closure,
            34 => GetUpvalue,
            35 => SetUpvalue,
            36 => LoadGlobal,
            37 => MakeRange,
            38 => MakeIter,
            39 => IterNext,
            40 => IterNext2,
            41 => IterAppend,
            42 => Unwind,
            43 => ArrayPush,
            44 => ArrayExtend,
            45 => ObjectMerge,
            46 => CallSpread,
            47 => ConcatN,
            48 => SliceFrom,
            49 => ObjRest,
            50 => Import,
            51 => PushTry,
            52 => PopTry,
            53 => Raise,
            54 => Halt,
            55 => BitAnd,
            56 => BitOr,
            57 => BitXor,
            58 => BitNot,
            59 => Shl,
            60 => Shr,
            61 => TypeTest,
            _ => return None,
        })
    }

    /// Number of inline operand bytes following this opcode.
    #[allow(dead_code)] // used by disassembler
    pub fn operand_bytes(self) -> usize {
        use OpCode::*;
        match self {
            LoadConst | LoadLocal | StoreLocal | CloseScope
            | MakeArray | MakeObject | Call
            | GetUpvalue | SetUpvalue | LoadGlobal
            | MakeRange | IterAppend | Unwind | ConcatN | TypeTest => 1,
            // Closure has variable-length operands (1 byte fn_idx
            // followed by 2 bytes per captured upvalue) — the
            // disassembler handles it specially, so we report only the
            // fn_idx here. This method is informational only.
            Closure => 1,
            Jump | Loop | JumpIfFalse | JumpIfTrue
            | IterNext | IterNext2 | PushTry => 2,
            _ => 0,
        }
    }
}
