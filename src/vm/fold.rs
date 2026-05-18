//! Compile-time constant folding (v0.12).
//!
//! An AST→AST rewrite run between parsing and compilation: a `BinOp`
//! or `UnOp` whose operands are all literals is replaced by the
//! literal it evaluates to, so `2 + 3` becomes `5` before a single
//! opcode is emitted. A fully-parenthesised literal expression
//! (`(3 + 4)`) collapses too, letting the surrounding operator fold
//! in turn.
//!
//! The folder mirrors the VM's arithmetic exactly (the `arith_*` /
//! `bit_*` helpers in `vm.rs`). Crucially it preserves v0.8 overflow
//! semantics: when an operation would *raise* at runtime — integer
//! overflow, divide-by-zero, an out-of-range shift — the folder
//! **declines to fold** and leaves the original expression in place,
//! so the VM still raises the catchable error exactly as before.
//! Folding never changes observable behaviour; it only moves work
//! that cannot fail from run time to compile time.

use crate::vm::ast::{BinOp, Block, Expr, ObjectMember, SpannedExpr, TemplatePart, UnOp};

/// Fold every expression in a parsed program, in place.
pub fn fold_program(block: &mut Block) {
    fold_block(block);
}

fn fold_block(block: &mut Block) {
    for stmt in &mut block.stmts {
        fold_expr(stmt);
    }
    if let Some(tail) = &mut block.tail {
        fold_expr(tail);
    }
}

/// A literal node carries no sub-expressions and is safe to duplicate
/// or to treat as a foldable operand.
fn is_literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null
    )
}

fn fold_expr(e: &mut SpannedExpr) {
    match &mut e.expr {
        // Leaves — nothing to fold.
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Ident(_)
        | Expr::Continue => {}

        Expr::BinOp(op, lhs, rhs) => {
            fold_expr(lhs);
            fold_expr(rhs);
            if let Some(folded) = fold_binop(*op, &lhs.expr, &rhs.expr) {
                e.expr = folded;
            }
        }
        Expr::UnOp(op, operand) => {
            fold_expr(operand);
            if let Some(folded) = fold_unop(*op, &operand.expr) {
                e.expr = folded;
            }
        }

        // A parenthesised expression is an `Expr::Block`. Once its
        // body folds, an empty-statement block whose tail is a literal
        // is equivalent to that literal — collapse it so the enclosing
        // operator can fold against it. (`Scope` is left intact: it
        // opens a lexical scope and is not pure parenthesisation.)
        Expr::Block(b) => {
            fold_block(b);
            if b.stmts.is_empty() {
                if let Some(tail) = &b.tail {
                    if is_literal(&tail.expr) {
                        e.expr = tail.expr.clone();
                    }
                }
            }
        }
        Expr::Scope(b) => fold_block(b),

        Expr::Decl(_, init) => fold_expr(init),
        Expr::Assign(_, _, rhs) => fold_expr(rhs),
        Expr::AssignPattern(_, rhs) => fold_expr(rhs),
        Expr::If(c, t, f) => {
            fold_expr(c);
            fold_expr(t);
            fold_expr(f);
        }
        Expr::While { cond, body, .. } => {
            fold_expr(cond);
            fold_expr(body);
        }
        Expr::For { iter, body, .. } => {
            fold_expr(iter);
            fold_expr(body);
        }
        Expr::Range { from, to, step, .. } => {
            fold_expr(from);
            fold_expr(to);
            if let Some(s) = step {
                fold_expr(s);
            }
        }
        Expr::Break(opt) => {
            if let Some(v) = opt {
                fold_expr(v);
            }
        }
        Expr::Array(items) => {
            for it in items {
                fold_expr(it);
            }
        }
        Expr::Object(members) => {
            for m in members {
                match m {
                    ObjectMember::Pair(_, v) => fold_expr(v),
                    ObjectMember::Spread(v) => fold_expr(v),
                }
            }
        }
        Expr::Spread(inner) => fold_expr(inner),
        Expr::Template(parts) => {
            for p in parts {
                if let TemplatePart::Expr(se) = p {
                    fold_expr(se);
                }
            }
        }
        Expr::Index(base, key) => {
            fold_expr(base);
            fold_expr(key);
        }
        Expr::IndexAssign(base, key, _, val) => {
            fold_expr(base);
            fold_expr(key);
            fold_expr(val);
        }
        Expr::Call(callee, args) => {
            fold_expr(callee);
            for a in args {
                fold_expr(a);
            }
        }
        Expr::Fn { defaults, body, .. } => {
            for d in defaults.iter_mut().flatten() {
                fold_expr(d);
            }
            fold_expr(body);
        }
        Expr::Return(opt) => {
            if let Some(v) = opt {
                fold_expr(v);
            }
        }
        Expr::Import(inner) => fold_expr(inner),
        Expr::Try { body, catch } => {
            fold_expr(body);
            if let Some((_, handler)) = catch {
                fold_expr(handler);
            }
        }
        Expr::Raise(inner) => fold_expr(inner),
        Expr::Spawn(inner) => fold_expr(inner),
        Expr::Go(inner) => fold_expr(inner),
        Expr::Yield(opt) => {
            if let Some(v) = opt {
                fold_expr(v);
            }
        }
        Expr::Match { subject, arms } => {
            fold_expr(subject);
            for arm in arms {
                if let Some(g) = &mut arm.guard {
                    fold_expr(g);
                }
                fold_expr(&mut arm.body);
            }
        }
    }
}

/// Both operands must be `Expr::Int`.
fn int_pair(lhs: &Expr, rhs: &Expr) -> Option<(i64, i64)> {
    match (lhs, rhs) {
        (Expr::Int(x), Expr::Int(y)) => Some((*x, *y)),
        _ => None,
    }
}

/// Evaluate a binary op on two literal operands. Returns `None` to
/// decline folding — for a non-literal operand, a type mismatch, or
/// any operation that would *raise* at runtime (overflow, divide by
/// zero, an out-of-range shift). Declining leaves the original
/// `BinOp` so the VM raises exactly as it would have.
fn fold_binop(op: BinOp, lhs: &Expr, rhs: &Expr) -> Option<Expr> {
    use BinOp::*;
    use Expr::{Float, Int, Str};
    match op {
        Add => match (lhs, rhs) {
            (Int(x), Int(y)) => Some(Int(x.checked_add(*y)?)),
            (Int(x), Float(y)) => Some(Float(*x as f64 + y)),
            (Float(x), Int(y)) => Some(Float(x + *y as f64)),
            (Float(x), Float(y)) => Some(Float(x + y)),
            (Str(x), Str(y)) => Some(Str(format!("{x}{y}"))),
            _ => None,
        },
        Sub => match (lhs, rhs) {
            (Int(x), Int(y)) => Some(Int(x.checked_sub(*y)?)),
            (Int(x), Float(y)) => Some(Float(*x as f64 - y)),
            (Float(x), Int(y)) => Some(Float(x - *y as f64)),
            (Float(x), Float(y)) => Some(Float(x - y)),
            _ => None,
        },
        Mul => match (lhs, rhs) {
            (Int(x), Int(y)) => Some(Int(x.checked_mul(*y)?)),
            (Int(x), Float(y)) => Some(Float(*x as f64 * y)),
            (Float(x), Int(y)) => Some(Float(x * *y as f64)),
            (Float(x), Float(y)) => Some(Float(x * y)),
            _ => None,
        },
        // Mirrors `arith_div`: an exact Int/Int quotient stays Int,
        // otherwise the result is Float. Int division by zero raises,
        // so it is left unfolded.
        Div => match (lhs, rhs) {
            (Int(_), Int(0)) => None,
            (Int(x), Int(y)) => match x.checked_rem(*y) {
                // `i64::MIN / -1` overflows — leave it unfolded for the
                // VM to raise `overflow`.
                None => None,
                Some(0) => Some(Int(x / y)),
                Some(_) => Some(Float(*x as f64 / *y as f64)),
            },
            (Int(x), Float(y)) => Some(Float(*x as f64 / y)),
            (Float(x), Int(y)) => Some(Float(x / *y as f64)),
            (Float(x), Float(y)) => Some(Float(x / y)),
            _ => None,
        },
        Mod => match (lhs, rhs) {
            (Int(_), Int(0)) => None,
            (Int(x), Int(y)) => Some(Int(x.checked_rem(*y).unwrap_or(0))),
            (Int(x), Float(y)) => Some(Float(*x as f64 % y)),
            (Float(x), Int(y)) => Some(Float(x % *y as f64)),
            (Float(x), Float(y)) => Some(Float(x % y)),
            _ => None,
        },
        // `arith_pow` always yields a Float, even for Int operands.
        Pow => {
            let (x, y) = match (lhs, rhs) {
                (Int(x), Int(y)) => (*x as f64, *y as f64),
                (Int(x), Float(y)) => (*x as f64, *y),
                (Float(x), Int(y)) => (*x, *y as f64),
                (Float(x), Float(y)) => (*x, *y),
                _ => return None,
            };
            Some(Float(x.powf(y)))
        }
        BitAnd => int_pair(lhs, rhs).map(|(x, y)| Int(x & y)),
        BitOr => int_pair(lhs, rhs).map(|(x, y)| Int(x | y)),
        BitXor => int_pair(lhs, rhs).map(|(x, y)| Int(x ^ y)),
        // A shift amount outside `0..64` raises, so it is left unfolded.
        Shl => {
            let (x, y) = int_pair(lhs, rhs)?;
            (0..64).contains(&y).then(|| Int(x << (y as u32)))
        }
        Shr => {
            let (x, y) = int_pair(lhs, rhs)?;
            (0..64).contains(&y).then(|| Int(x >> (y as u32)))
        }
        // Comparisons and short-circuiting logical ops are out of
        // scope for v0.12 folding (see ROADMAP v0.12 item 15).
        Eq | Neq | Lt | Le | Gt | Ge | And | Or => None,
    }
}

/// Evaluate a unary op on a literal operand. `Neg` on `i64::MIN`
/// overflows and is left unfolded; logical `!` and length `#` are out
/// of scope for v0.12.
fn fold_unop(op: UnOp, operand: &Expr) -> Option<Expr> {
    match (op, operand) {
        (UnOp::Neg, Expr::Int(x)) => Some(Expr::Int(x.checked_neg()?)),
        (UnOp::Neg, Expr::Float(x)) => Some(Expr::Float(-x)),
        (UnOp::BitNot, Expr::Int(x)) => Some(Expr::Int(!x)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::lexer::Lexer;
    use crate::vm::parser;

    /// Lex, parse, and fold a snippet; return the program's tail expr.
    fn fold_tail(src: &str) -> Expr {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let mut program = parser::parse(tokens).unwrap();
        fold_program(&mut program);
        program.tail.expect("snippet has a tail expression").expr
    }

    #[test]
    fn folds_nested_int_arithmetic() {
        assert_eq!(fold_tail("2 + 3 * 4"), Expr::Int(14));
    }

    #[test]
    fn collapses_parens_so_outer_op_folds() {
        assert_eq!(fold_tail("(3 + 4) * 2"), Expr::Int(14));
        assert_eq!(fold_tail("64 / 8"), Expr::Int(8));
    }

    #[test]
    fn folds_string_concat() {
        assert_eq!(fold_tail("'a' + 'b'"), Expr::Str("ab".to_string()));
    }

    #[test]
    fn folds_unary_neg_and_bitwise() {
        assert_eq!(fold_tail("-5"), Expr::Int(-5));
        assert_eq!(fold_tail("0xF0 & 0x0F"), Expr::Int(0));
        assert_eq!(fold_tail("1 << 10"), Expr::Int(1024));
    }

    #[test]
    fn declines_on_overflow() {
        // Left as a BinOp so the VM still raises a catchable `overflow`.
        assert!(matches!(
            fold_tail("9223372036854775807 + 1"),
            Expr::BinOp(..)
        ));
    }

    #[test]
    fn declines_on_division_by_zero() {
        assert!(matches!(fold_tail("1 / 0"), Expr::BinOp(..)));
        assert!(matches!(fold_tail("5 % 0"), Expr::BinOp(..)));
    }

    #[test]
    fn declines_on_out_of_range_shift() {
        assert!(matches!(fold_tail("1 << 64"), Expr::BinOp(..)));
    }

    #[test]
    fn leaves_non_literal_operands_alone() {
        assert!(matches!(fold_tail("x + 1"), Expr::BinOp(..)));
    }
}
