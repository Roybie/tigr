//! Compiles the AST to a `Chunk` of bytecode.
//!
//! Phase 1 model:
//! - The runtime stack hosts locals at the bottom and temporaries on top.
//! - A local's slot is its index in `self.locals` at the moment of `:=`.
//!   The init expression's result is the local's storage — no separate
//!   `DECL_LOCAL` opcode.
//! - `:=` evaluates to the assigned value, so we emit a `DUP` after
//!   declaring; in stmt position the surrounding `Block` compilation
//!   pops the duplicate and the local persists below.

use crate::vm::ast::{BinOp, Block, Expr, SpannedExpr, UnOp};
use crate::vm::chunk::Chunk;
use crate::vm::error::{CompileError, CompileErrorKind};
use crate::vm::opcode::OpCode;
use crate::vm::value::Value;

struct Local {
    name: String,
    depth: u32,
}

pub struct Compiler {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: u32,
}

impl Compiler {
    pub fn compile(program: &Block) -> Result<Chunk, CompileError> {
        let mut c = Compiler { chunk: Chunk::new(), locals: Vec::new(), scope_depth: 0 };
        c.compile_block_value(program)?;
        let last_line = c.chunk.lines.last().copied().unwrap_or(1);
        c.chunk.write_op(OpCode::Return, last_line);
        Ok(c.chunk)
    }

    /// Compile a block so that its value is left on top of the stack.
    fn compile_block_value(&mut self, block: &Block) -> Result<(), CompileError> {
        for stmt in &block.stmts {
            self.compile_expr(stmt)?;
            self.chunk.write_op(OpCode::Pop, stmt.span.line);
        }
        if let Some(tail) = &block.tail {
            self.compile_expr(tail)?;
        } else {
            let line = block.stmts.last().map(|s| s.span.line).unwrap_or(1);
            self.chunk.write_op(OpCode::PushNull, line);
        }
        Ok(())
    }

    fn compile_expr(&mut self, e: &SpannedExpr) -> Result<(), CompileError> {
        let line = e.span.line;
        match &e.expr {
            Expr::Int(n) => self.emit_constant(Value::Int(*n), line, e.span)?,
            Expr::Float(x) => self.emit_constant(Value::Float(*x), line, e.span)?,
            Expr::Str(s) => self.emit_constant(Value::Str(s.as_str().into()), line, e.span)?,
            Expr::Bool(b) => self.emit_constant(Value::Bool(*b), line, e.span)?,
            Expr::Null => self.chunk.write_op(OpCode::PushNull, line),

            Expr::Ident(name) => {
                let slot = self.resolve_local(name).ok_or_else(|| {
                    CompileError::new(
                        CompileErrorKind::UndeclaredVariable(name.clone()),
                        e.span,
                    )
                })?;
                self.chunk.write_op(OpCode::LoadLocal, line);
                self.chunk.write_byte(slot, line);
            }

            Expr::BinOp(op, lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                let op_code = match op {
                    BinOp::Add => OpCode::Add,
                    BinOp::Sub => OpCode::Sub,
                    BinOp::Mul => OpCode::Mul,
                    BinOp::Div => OpCode::Div,
                    BinOp::Mod => OpCode::Mod,
                    BinOp::Pow => OpCode::Pow,
                    // Phase 2+ operators reach here only if the parser
                    // emitted them; for Phase 1 we treat as not-yet-impl.
                    _ => unreachable!("op {op:?} reached compiler in Phase 1"),
                };
                self.chunk.write_op(op_code, line);
            }

            Expr::UnOp(op, inner) => {
                self.compile_expr(inner)?;
                match op {
                    UnOp::Neg => self.chunk.write_op(OpCode::Negate, line),
                    _ => unreachable!("op {op:?} reached compiler in Phase 1"),
                }
            }

            Expr::Block(b) => {
                // Phase 1: blocks (parens) do not open a new lexical scope.
                self.compile_block_value(b)?;
            }

            Expr::Decl(name, init) => {
                if self.locals.iter().any(|l| l.name == *name && l.depth == self.scope_depth) {
                    return Err(CompileError::new(
                        CompileErrorKind::DuplicateDeclaration(name.clone()),
                        e.span,
                    ));
                }
                self.compile_expr(init)?;
                if self.locals.len() >= 256 {
                    return Err(CompileError::new(CompileErrorKind::TooManyLocals, e.span));
                }
                self.locals.push(Local { name: name.clone(), depth: self.scope_depth });
                self.chunk.write_op(OpCode::Dup, line);
            }

            Expr::Assign(name, value) => {
                let slot = self.resolve_local(name).ok_or_else(|| {
                    CompileError::new(
                        CompileErrorKind::UndeclaredAssign(name.clone()),
                        e.span,
                    )
                })?;
                self.compile_expr(value)?;
                self.chunk.write_op(OpCode::StoreLocal, line);
                self.chunk.write_byte(slot, line);
            }
        }
        Ok(())
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u8);
            }
        }
        None
    }

    fn emit_constant(
        &mut self,
        value: Value,
        line: u32,
        span: crate::vm::token::Span,
    ) -> Result<(), CompileError> {
        let idx = self
            .chunk
            .add_constant(value)
            .map_err(|_| CompileError::new(CompileErrorKind::TooManyConstants, span))?;
        self.chunk.write_op(OpCode::LoadConst, line);
        self.chunk.write_byte(idx, line);
        Ok(())
    }
}
