//! Hand-written precedence-climbing parser for Tigr.
//!
//! Grammar (v0.5):
//!
//! ```text
//! Program     ::= Block
//! Block       ::= (Expr ';')* Expr?
//! Expr        ::= Assign
//! Assign      ::= Postfix (':=' | '=' | '+=' | '-=' | '*=' | '/=' | '%=') Assign
//!               | LogicOr
//! LogicOr     ::= LogicAnd ('||' LogicAnd)*
//! LogicAnd    ::= Equality ('&&' Equality)*
//! Equality    ::= BitOr (('==' | '!=' | '<' | '>' | '<=' | '>=') BitOr)*
//! BitOr       ::= BitXor ('|' BitXor)*
//! BitXor      ::= BitAnd ('^' BitAnd)*
//! BitAnd      ::= Pipe ('&' Pipe)*
//! Pipe        ::= Range ('|>' Range)*
//! Range       ::= Shift (('..' | '..=') Shift (':' Shift)?)?
//! Shift       ::= Additive (('<<' | '>>') Additive)*
//! Additive    ::= Multiplicative (('+' | '-') Multiplicative)*
//! Multiplicative ::= Power (('*' | '/' | '%') Power)*
//! Power       ::= Unary ('^^' Power)?             -- right-assoc
//! Unary       ::= ('-' | '!' | '#' | '~') Unary | Postfix
//! Postfix     ::= Primary ( '[' Expr ']' | '.' IDENT | '(' Args? ')' )*
//! Primary     ::= INT | FLOAT | STR | 'true' | 'false' | 'null' | IDENT
//!               | '(' Block ')'
//!               | '{' Block '}'                    -- scope
//!               | '[' (Expr (',' Expr)* ','?)? ']' -- array literal
//!               | '$' '{' (ObjPair (',' ObjPair)* ','?)? '}'  -- object literal
//!               | 'if' Expr Scope ('else' (Scope | If))?
//!               | 'while' Expr Scope
//!               | 'match' Expr '{' (MatchArm (',' MatchArm)* ','?)? '}'
//! MatchArm    ::= MatchPattern ('if' Expr)? '=>' Expr
//! ObjPair     ::= (IDENT | STR) ':' Expr
//! Args        ::= Expr (',' Expr)*
//! ```

use crate::vm::ast::{
    expr_to_pattern, BinOp, Binder, Block, Expr, LiteralPat, MatchArm, MatchField,
    MatchPattern, ObjectMember, Pattern, SpannedExpr, TemplatePart, UnOp,
};
use crate::vm::error::{ParseError, ParseErrorKind};
use crate::vm::lexer::Lexer;
use crate::vm::token::{
    Span, SpannedToken, TemplatePart as LexTemplatePart, Token,
};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    /// Parse errors collected during recovery. The block loop catches a
    /// failed statement, records it here, resynchronises to the next `;`
    /// or block terminator, and keeps going — so one syntax error no
    /// longer hides every later one. The run path still reports only the
    /// first (see the free `parse` function); the LSP consumes them all.
    errors: Vec<ParseError>,
}

enum AssignKind {
    Decl,
    Assign(Option<BinOp>),
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Parser { tokens, pos: 0, errors: Vec::new() }
    }

    /// Parse the whole token stream, recovering past errors. Always
    /// returns a (possibly partial) program plus every error collected.
    pub fn parse_program_recover(mut self) -> (Block, Vec<ParseError>) {
        let block = self.parse_block_until(&[Token::Eof]);
        (block, self.errors)
    }

    // -- token-stream helpers ------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn advance(&mut self) -> &SpannedToken {
        let t = &self.tokens[self.pos];
        if !matches!(t.token, Token::Eof) {
            self.pos += 1;
        }
        &self.tokens[self.pos.saturating_sub(1)]
    }

    fn check(&self, tok: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(tok)
    }

    fn check_any(&self, toks: &[Token]) -> bool {
        toks.iter().any(|t| self.check(t))
    }

    fn matches(&mut self, tok: &Token) -> bool {
        if self.check(tok) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Token) -> Result<Span, ParseError> {
        if self.check(tok) {
            let span = self.peek_span();
            self.advance();
            Ok(span)
        } else {
            Err(self.err(ParseErrorKind::Expected {
                expected: tok.clone(),
                found: self.peek().clone(),
            }))
        }
    }

    fn err(&self, kind: ParseErrorKind) -> ParseError {
        ParseError::new(kind, self.peek_span())
    }

    // -- block ---------------------------------------------------------

    /// Parse a `;`-separated block up to one of `terminators` (or EOF).
    /// Infallible: a statement that fails to parse is recorded in
    /// `self.errors`, after which we resynchronise to the next statement
    /// boundary and continue, so later statements still parse.
    fn parse_block_until(&mut self, terminators: &[Token]) -> Block {
        let mut stmts = Vec::new();
        let mut tail: Option<Box<SpannedExpr>> = None;

        loop {
            if self.check_any(terminators) || self.check(&Token::Eof) {
                break;
            }
            match self.parse_expr() {
                Ok(expr) => {
                    if self.matches(&Token::Semicolon) {
                        stmts.push(expr);
                        continue;
                    } else if self.check_any(terminators) || self.check(&Token::Eof) {
                        tail = Some(Box::new(expr));
                        break;
                    } else {
                        // Parsed fine, but a second expression starts with
                        // no `;` between. Record it as a forgotten `;` at
                        // the end of this expression, then treat this as a
                        // statement and keep going — don't skip the next
                        // expression, which a generic resync would.
                        let prev = self.tokens[self.pos.saturating_sub(1)].span;
                        self.errors.push(ParseError::new(
                            ParseErrorKind::MissingSemicolon {
                                found: self.peek().clone(),
                            },
                            Span::new(prev.end, prev.end + 1, prev.line),
                        ));
                        stmts.push(expr);
                        continue;
                    }
                }
                Err(e) => {
                    self.errors.push(e);
                    // Resync to the next statement boundary. If that runs
                    // out the input, stop — the block ends here.
                    if !self.synchronize(terminators) {
                        break;
                    }
                }
            }
        }

        Block { stmts, tail }
    }

    /// Skip tokens after a parse error until we reach a plausible place to
    /// resume: a `;` (consumed) or one of `terminators`/EOF (left in
    /// place for the caller). Bracket depth is tracked so a `;` *inside* a
    /// nested `()`/`[]`/`{}` doesn't end the skip prematurely, and a
    /// terminator only stops the skip at depth zero. Always advances at
    /// least one token so the caller's loop can't spin. Returns `false`
    /// when it reaches EOF (nothing left to parse).
    fn synchronize(&mut self, terminators: &[Token]) -> bool {
        let mut depth = 0i32;
        loop {
            if self.check(&Token::Eof) {
                return false;
            }
            // A terminator at depth zero ends the enclosing block; leave
            // it in place for the caller to handle.
            if depth == 0 && self.check_any(terminators) {
                return true;
            }
            match self.peek() {
                Token::LParen | Token::LBrack | Token::LBrace => depth += 1,
                Token::RParen | Token::RBrack | Token::RBrace => {
                    // Only descend for matched closers; a stray depth-zero
                    // closer is junk that we consume below to make progress.
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                // A `;` at depth zero is the next statement boundary:
                // consume it and resume parsing after it.
                Token::Semicolon if depth == 0 => {
                    self.advance();
                    return true;
                }
                _ => {}
            }
            // Always advance — guarantees the caller's loop makes progress.
            self.advance();
        }
    }

    // -- expressions ---------------------------------------------------

    fn parse_expr(&mut self) -> Result<SpannedExpr, ParseError> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<SpannedExpr, ParseError> {
        let left = self.parse_logic_or()?;

        // Map any assign-class token to (is_decl, optional compound op).
        let assign = match self.peek() {
            Token::ColonEq => Some(AssignKind::Decl),
            Token::Eq => Some(AssignKind::Assign(None)),
            Token::PlusEq => Some(AssignKind::Assign(Some(BinOp::Add))),
            Token::MinusEq => Some(AssignKind::Assign(Some(BinOp::Sub))),
            Token::StarEq => Some(AssignKind::Assign(Some(BinOp::Mul))),
            Token::SlashEq => Some(AssignKind::Assign(Some(BinOp::Div))),
            Token::PercentEq => Some(AssignKind::Assign(Some(BinOp::Mod))),
            _ => None,
        };
        let Some(kind) = assign else {
            return Ok(left);
        };
        let op_span = self.peek_span();
        self.advance();
        let right = self.parse_assign()?; // right-assoc
        let span = left.span.join(op_span).join(right.span);

        match kind {
            AssignKind::Decl => {
                let pat = expr_to_pattern(&left).map_err(|pe| {
                    ParseError::new(
                        ParseErrorKind::InvalidPattern(pe.message().to_string()),
                        pe.span(),
                    )
                })?;
                Ok(SpannedExpr::new(Expr::Decl(pat, Box::new(right)), span))
            }
            AssignKind::Assign(op) => match left.expr {
                Expr::Ident(name) => Ok(SpannedExpr::new(
                    Expr::Assign(Binder::new(name, left.span), op, Box::new(right)),
                    span,
                )),
                Expr::Index(obj, key) => Ok(SpannedExpr::new(
                    Expr::IndexAssign(obj, key, op, Box::new(right)),
                    span,
                )),
                // Pattern-shaped LHS — only valid for plain `=`.
                // `[a, b] += ...` falls through to the catchall.
                Expr::Array(_) | Expr::Object(_) if op.is_none() => {
                    let pat = expr_to_pattern(&left).map_err(|pe| {
                        ParseError::new(
                            ParseErrorKind::InvalidPattern(pe.message().to_string()),
                            pe.span(),
                        )
                    })?;
                    Ok(SpannedExpr::new(
                        Expr::AssignPattern(pat, Box::new(right)),
                        span,
                    ))
                }
                _ => Err(ParseError::new(
                    ParseErrorKind::InvalidAssignTarget,
                    left.span,
                )),
            },
        }
    }

    fn parse_logic_or(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_logic_and()?;
        while matches!(self.peek(), Token::PipePipe) {
            self.advance();
            let right = self.parse_logic_and()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(BinOp::Or, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn parse_logic_and(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), Token::AmpAmp) {
            self.advance();
            let right = self.parse_equality()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(BinOp::And, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_bit_or()?;
        loop {
            let op = match self.peek() {
                Token::EqEq => BinOp::Eq,
                Token::BangEq => BinOp::Neq,
                Token::Lt => BinOp::Lt,
                Token::LtEq => BinOp::Le,
                Token::Gt => BinOp::Gt,
                Token::GtEq => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_bit_or()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(op, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    /// Bitwise `|`. Below comparison, above `^` — Rust-style precedence
    /// (`a & b == c` parses as `(a & b) == c`). Int-only at runtime.
    fn parse_bit_or(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_bit_xor()?;
        while matches!(self.peek(), Token::Pipe) {
            self.advance();
            let right = self.parse_bit_xor()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(BinOp::BitOr, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    /// Bitwise XOR `^`. (Exponentiation moved to `^^`.)
    fn parse_bit_xor(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_bit_and()?;
        while matches!(self.peek(), Token::Caret) {
            self.advance();
            let right = self.parse_bit_and()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(BinOp::BitXor, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    /// Bitwise `&`.
    fn parse_bit_and(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_pipe()?;
        while matches!(self.peek(), Token::Amp) {
            self.advance();
            let right = self.parse_pipe()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(BinOp::BitAnd, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    /// `x |> rhs`. Pure parser-level desugar (spec §15.5):
    /// - `x |> f(a, b)` → `f(x, a, b)`
    /// - `x |> f`       → `f(x)`
    /// Left-associative; RHS parsed at range-precedence so chains like
    /// `x |> f |> g` are walked left-to-right.
    fn parse_pipe(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_range()?;
        while matches!(self.peek(), Token::PipeGt) {
            self.advance();
            let right = self.parse_range()?;
            let span = left.span.join(right.span);
            let desugared = match right.expr {
                Expr::Call(callee, mut args) => {
                    args.insert(0, left);
                    Expr::Call(callee, args)
                }
                _ => Expr::Call(Box::new(right), vec![left]),
            };
            left = SpannedExpr::new(desugared, span);
        }
        Ok(left)
    }

    /// Range parses as `Additive ('..' | '..=') Additive (':' Additive)?`.
    /// Non-associative; chaining `a..b..c` is a parse error.
    fn parse_range(&mut self) -> Result<SpannedExpr, ParseError> {
        let left = self.parse_shift()?;
        let inclusive = match self.peek() {
            Token::DotDot => false,
            Token::DotDotEq => true,
            _ => return Ok(left),
        };
        self.advance();
        let to = self.parse_shift()?;
        let step = if matches!(self.peek(), Token::Colon) {
            self.advance();
            Some(Box::new(self.parse_shift()?))
        } else {
            None
        };
        let end_span = step.as_ref().map(|s| s.span).unwrap_or(to.span);
        let span = left.span.join(end_span);
        Ok(SpannedExpr::new(
            Expr::Range {
                from: Box::new(left),
                to: Box::new(to),
                step,
                inclusive,
            },
            span,
        ))
    }

    /// Bit shifts `<<` `>>`. Looser than `+ -`, tighter than `&` —
    /// Rust-style (`a + b << c` parses as `(a + b) << c`). Int-only.
    /// `>>` is an arithmetic (sign-preserving) shift.
    fn parse_shift(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_additive()?;
        while matches!(self.peek(), Token::Shl | Token::Shr) {
            let op = if matches!(self.peek(), Token::Shl) { BinOp::Shl } else { BinOp::Shr };
            self.advance();
            let right = self.parse_additive()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(op, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        while matches!(self.peek(), Token::Plus | Token::Minus) {
            let op = if matches!(self.peek(), Token::Plus) { BinOp::Add } else { BinOp::Sub };
            self.advance();
            let right = self.parse_multiplicative()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(op, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut left = self.parse_power()?;
        while matches!(self.peek(), Token::Star | Token::Slash | Token::Percent) {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => unreachable!(),
            };
            self.advance();
            let right = self.parse_power()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(op, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<SpannedExpr, ParseError> {
        let left = self.parse_unary()?;
        if matches!(self.peek(), Token::CaretCaret) {
            self.advance();
            // right-assoc: recurse into parse_power, not parse_unary
            let right = self.parse_power()?;
            let span = left.span.join(right.span);
            Ok(SpannedExpr::new(
                Expr::BinOp(BinOp::Pow, Box::new(left), Box::new(right)),
                span,
            ))
        } else {
            Ok(left)
        }
    }

    fn parse_unary(&mut self) -> Result<SpannedExpr, ParseError> {
        let op = match self.peek() {
            Token::Minus => Some(UnOp::Neg),
            Token::Bang => Some(UnOp::Not),
            Token::Hash => Some(UnOp::Len),
            Token::Tilde => Some(UnOp::BitNot),
            _ => None,
        };
        if let Some(op) = op {
            let start = self.peek_span();
            self.advance();
            let inner = self.parse_unary()?;
            let span = start.join(inner.span);
            Ok(SpannedExpr::new(Expr::UnOp(op, Box::new(inner)), span))
        } else {
            self.parse_postfix()
        }
    }

    /// Parse a primary, then apply zero-or-more postfix operators:
    /// `expr[i]`, `expr.k`, `expr(args)`. Left-associative; `f(x)[0].k`
    /// chains the way you'd expect.
    fn parse_postfix(&mut self) -> Result<SpannedExpr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::LBrack => {
                    self.advance();
                    let key = self.parse_expr()?;
                    let close = self.expect(&Token::RBrack)?;
                    let span = expr.span.join(close);
                    expr = SpannedExpr::new(
                        Expr::Index(Box::new(expr), Box::new(key)),
                        span,
                    );
                }
                Token::Dot => {
                    self.advance();
                    let (name, name_span) = match self.peek().clone() {
                        Token::Ident(n) => {
                            let span = self.peek_span();
                            self.advance();
                            (n, span)
                        }
                        other => {
                            return Err(self.err(ParseErrorKind::UnexpectedToken(other)));
                        }
                    };
                    let span = expr.span.join(name_span);
                    let key = SpannedExpr::new(Expr::Str(name), name_span);
                    expr = SpannedExpr::new(
                        Expr::Index(Box::new(expr), Box::new(key)),
                        span,
                    );
                }
                Token::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            args.push(self.parse_spreadable_expr()?);
                            if !self.matches(&Token::Comma) {
                                break;
                            }
                            // allow trailing comma
                            if self.check(&Token::RParen) {
                                break;
                            }
                        }
                    }
                    let close = self.expect(&Token::RParen)?;
                    let span = expr.span.join(close);
                    expr = SpannedExpr::new(Expr::Call(Box::new(expr), args), span);
                }
                _ => return Ok(expr),
            }
        }
    }

    fn parse_primary(&mut self) -> Result<SpannedExpr, ParseError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Int(n), span))
            }
            Token::Float(x) => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Float(x), span))
            }
            Token::Str(s) => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Str(s), span))
            }
            Token::StrTemplate(lex_parts) => {
                self.advance();
                let parts = self.compile_template_parts(lex_parts, span)?;
                Ok(SpannedExpr::new(Expr::Template(parts), span))
            }
            Token::True => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Bool(true), span))
            }
            Token::False => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Bool(false), span))
            }
            Token::Null => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Null, span))
            }
            Token::Ident(name) => {
                self.advance();
                Ok(SpannedExpr::new(Expr::Ident(name), span))
            }
            Token::LParen => {
                let lparen = self.peek_span();
                self.advance();
                let block = self.parse_block_until(&[Token::RParen]);
                let rparen = self.expect(&Token::RParen)?;
                let span = lparen.join(rparen);
                Ok(SpannedExpr::new(Expr::Block(block), span))
            }
            Token::LBrace => self.parse_scope(),
            Token::LBrack => self.parse_array(),
            Token::Dollar => self.parse_object(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Fn => self.parse_fn(),
            Token::Return => self.parse_return(),
            Token::Break => self.parse_break(),
            Token::Continue => self.parse_continue(),
            Token::Import => self.parse_import(),
            Token::Try => self.parse_try(),
            Token::Raise => self.parse_raise(),
            Token::Spawn => self.parse_spawn(),
            Token::Select => self.parse_select(),
            Token::Parallel => self.parse_parallel(),
            Token::Go => self.parse_go(),
            Token::Yield => self.parse_yield(),
            Token::Gen => self.parse_gen(),
            Token::Match => self.parse_match(),
            other => Err(self.err(ParseErrorKind::UnexpectedToken(other))),
        }
    }

    /// `try expr` or `try expr catch (param) { handler }` — both
    /// produce values per spec §9.6. The handler is required to be a
    /// scope `{ ... }`.
    ///
    /// The body is parsed at `&&` precedence (one level tighter than
    /// `||`) so that `try f(x) || 'default'` parses as
    /// `(try f(x)) || 'default'` — the natural fallback idiom. Users
    /// who want `try` to span an `||`-expression must parenthesize.
    fn parse_try(&mut self) -> Result<SpannedExpr, ParseError> {
        let try_span = self.expect(&Token::Try)?;
        let body = self.parse_logic_and()?;
        let (catch, end_span) = if self.matches(&Token::Catch) {
            self.expect(&Token::LParen)?;
            let param = self.parse_ident_binder()?;
            self.expect(&Token::RParen)?;
            let handler = self.parse_scope()?;
            let span = handler.span;
            (Some((param, Box::new(handler))), span)
        } else {
            (None, body.span)
        };
        let span = try_span.join(end_span);
        Ok(SpannedExpr::new(
            Expr::Try { body: Box::new(body), catch },
            span,
        ))
    }

    /// `raise expr` — always carries a value; the value is the error
    /// message (coerced to string at runtime). Unlike `break`/`return`,
    /// there's no zero-arg form because an empty raise has no meaning.
    fn parse_raise(&mut self) -> Result<SpannedExpr, ParseError> {
        let raise_span = self.expect(&Token::Raise)?;
        let value = self.parse_expr()?;
        let span = raise_span.join(value.span);
        Ok(SpannedExpr::new(Expr::Raise(Box::new(value)), span))
    }

    /// `spawn expr` (v0.14) — `expr` is the function to run as an actor.
    fn parse_spawn(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Spawn)?;
        let callee = self.parse_expr()?;
        let span = kw_span.join(callee.span);
        Ok(SpannedExpr::new(Expr::Spawn(Box::new(callee)), span))
    }

    /// `go expr` — `expr` is the function to run as a green thread
    /// (coroutine) inside the current actor.
    fn parse_go(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Go)?;
        let callee = self.parse_expr()?;
        let span = kw_span.join(callee.span);
        Ok(SpannedExpr::new(Expr::Go(Box::new(callee)), span))
    }

    /// `yield expr` or bare `yield` — suspends the running green
    /// thread. Like `break`/`return`, the value may be omitted when
    /// the next token cannot start an expression.
    fn parse_yield(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Yield)?;
        let omit = matches!(
            self.peek(),
            Token::Semicolon | Token::RParen | Token::RBrace | Token::RBrack
            | Token::Comma | Token::Eof
        );
        if omit {
            Ok(SpannedExpr::new(Expr::Yield(None), kw_span))
        } else {
            let value = self.parse_expr()?;
            let span = kw_span.join(value.span);
            Ok(SpannedExpr::new(Expr::Yield(Some(Box::new(value))), span))
        }
    }

    /// `select { name := chan => body, ..., else => body }` (v0.14).
    ///
    /// Desugars — there is no dedicated AST node — to a `match` over
    /// the result of the internal `__select` builtin:
    ///
    /// ```text
    /// match __select([chan0, chan1], <has_else>) {
    ///     ${index: 0, value: name0} => body0,
    ///     ${index: 1, value: name1} => body1,
    ///     _ => else_body,            // only when an `else` arm exists
    /// }
    /// ```
    ///
    /// Each arm binds a plain identifier (or `_`) to the received
    /// message value.
    fn parse_select(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Select)?;
        self.expect(&Token::LBrace)?;

        let mut channels: Vec<SpannedExpr> = Vec::new();
        let mut arms: Vec<MatchArm> = Vec::new();
        let mut else_body: Option<SpannedExpr> = None;

        while !self.check(&Token::RBrace) {
            if self.matches(&Token::Else) {
                self.expect(&Token::FatArrow)?;
                else_body = Some(self.parse_expr()?);
                if !self.matches(&Token::Comma) {
                    break;
                }
                continue;
            }
            // `name := channel => body`
            let bind = self.parse_select_binding()?;
            self.expect(&Token::ColonEq)?;
            let channel = self.parse_expr()?;
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            let index = channels.len() as i64;
            channels.push(channel);
            arms.push(MatchArm {
                pattern: MatchPattern::Object {
                    fields: vec![
                        MatchField {
                            key: "index".to_string(),
                            pattern: Some(MatchPattern::Literal(
                                LiteralPat::Int(index),
                            )),
                        },
                        MatchField {
                            key: "value".to_string(),
                            pattern: Some(bind),
                        },
                    ],
                    rest: None,
                },
                guard: None,
                body,
            });
            if !self.matches(&Token::Comma) {
                break;
            }
        }
        let rbrace = self.expect(&Token::RBrace)?;
        let span = kw_span.join(rbrace);

        let has_else = else_body.is_some();
        if let Some(eb) = else_body {
            arms.push(MatchArm {
                pattern: MatchPattern::Wildcard,
                guard: None,
                body: eb,
            });
        }

        // subject: __select([channels...], has_else)
        let arr = SpannedExpr::new(Expr::Array(channels), span);
        let callee =
            SpannedExpr::new(Expr::Ident("__select".to_string()), span);
        let flag = SpannedExpr::new(Expr::Bool(has_else), span);
        let subject = SpannedExpr::new(
            Expr::Call(Box::new(callee), vec![arr, flag]),
            span,
        );

        Ok(SpannedExpr::new(
            Expr::Match { subject: Box::new(subject), arms },
            span,
        ))
    }

    /// Parse a `select` arm's binding — a plain identifier, or `_`.
    fn parse_select_binding(&mut self) -> Result<MatchPattern, ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                if name == "_" {
                    Ok(MatchPattern::Wildcard)
                } else {
                    Ok(MatchPattern::Binding(name))
                }
            }
            other => Err(self.err(ParseErrorKind::UnexpectedToken(other))),
        }
    }

    /// `parallel[] (var, iter) { body }` (v0.14) — runs each iteration
    /// of the body as its own actor, in parallel, and collects the
    /// results into an array in input order.
    ///
    /// Desugars — no dedicated AST node — to spawn-all then join-all:
    ///
    /// ```text
    /// {
    ///   $parallel_tasks := for[] (var, iter) { spawn fn() { body } };
    ///   for[] ($parallel_t, $parallel_tasks) { join($parallel_t) }
    /// }
    /// ```
    ///
    /// The first `join` to see an actor error re-raises it out of the
    /// block; siblings already spawned run to completion but their
    /// results are discarded.
    fn parse_parallel(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Parallel)?;
        // `parallel` is collect-only; an explicit `[]` is canonical
        // but optional (there is no non-collecting form to confuse it
        // with).
        let _ = self.parse_array_form_marker()?;
        self.expect(&Token::LParen)?;

        let first_var = self.parse_ident_binder()?;
        self.expect(&Token::Comma)?;
        let next_expr = self.parse_expr()?;
        let (vars, iter) = if self.matches(&Token::Comma) {
            let second = match next_expr.expr {
                Expr::Ident(n) => Binder::new(n, next_expr.span),
                _ => {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidAssignTarget,
                        next_expr.span,
                    ));
                }
            };
            (vec![first_var, second], self.parse_expr()?)
        } else {
            (vec![first_var], next_expr)
        };
        let rparen = self.expect(&Token::RParen)?;
        let body = self.parse_scope()?;
        let span = kw_span.join(rparen).join(body.span);

        let s = |e| SpannedExpr::new(e, span);

        // spawn fn() { body }
        let actor = s(Expr::Fn {
            params: Vec::new(),
            defaults: Vec::new(),
            rest: None,
            body: Box::new(body),
            is_generator: false,
        });
        let spawn_actor = s(Expr::Spawn(Box::new(actor)));

        // $parallel_tasks := for[] (vars, iter) { spawn fn() { body } }
        let collect = s(Expr::For {
            is_array: true,
            vars,
            iter: Box::new(iter),
            body: Box::new(spawn_actor),
        });
        let decl = s(Expr::Decl(
            Pattern::Ident(Binder::new("$parallel_tasks", span)),
            Box::new(collect),
        ));

        // for[] ($parallel_t, $parallel_tasks) { join($parallel_t) }
        let join_call = s(Expr::Call(
            Box::new(s(Expr::Ident("join".to_string()))),
            vec![s(Expr::Ident("$parallel_t".to_string()))],
        ));
        let join_all = s(Expr::For {
            is_array: true,
            vars: vec![Binder::new("$parallel_t", span)],
            iter: Box::new(s(Expr::Ident("$parallel_tasks".to_string()))),
            body: Box::new(join_call),
        });

        Ok(s(Expr::Block(Block {
            stmts: vec![decl],
            tail: Some(Box::new(join_all)),
        })))
    }

    /// `match subject { pat => body, pat if guard => body, ... }`
    /// (v0.5). Comma-separated arms, optional trailing comma. The
    /// subject is a full expression; it stops before the `{` exactly
    /// as an `if` condition does.
    fn parse_match(&mut self) -> Result<SpannedExpr, ParseError> {
        let match_span = self.expect(&Token::Match)?;
        let subject = self.parse_expr()?;
        self.expect(&Token::LBrace)?;
        let mut arms: Vec<MatchArm> = Vec::new();
        while !self.check(&Token::RBrace) {
            let pattern = self.parse_match_pattern()?;
            let guard = if self.matches(&Token::If) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            arms.push(MatchArm { pattern, guard, body });
            if !self.matches(&Token::Comma) {
                break;
            }
        }
        let rbrace = self.expect(&Token::RBrace)?;
        let span = match_span.join(rbrace);
        Ok(SpannedExpr::new(
            Expr::Match { subject: Box::new(subject), arms },
            span,
        ))
    }

    /// A `match` pattern, possibly an or-pattern `p1 | p2 | ...`. The
    /// `|` here is unambiguously an or-separator — pattern grammar
    /// never descends into expression-level bitwise-or.
    fn parse_match_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        let first = self.parse_match_pattern_primary()?;
        if !matches!(self.peek(), Token::Pipe) {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.matches(&Token::Pipe) {
            alts.push(self.parse_match_pattern_primary()?);
        }
        Ok(MatchPattern::Or(alts))
    }

    fn parse_match_pattern_primary(&mut self) -> Result<MatchPattern, ParseError> {
        match self.peek().clone() {
            Token::Int(_) | Token::Float(_) | Token::Minus => {
                let from = self.parse_numeric_literal_pat()?;
                let inclusive = match self.peek() {
                    Token::DotDot => false,
                    Token::DotDotEq => true,
                    _ => return Ok(MatchPattern::Literal(from)),
                };
                self.advance();
                let to = self.parse_numeric_literal_pat()?;
                Ok(MatchPattern::Range { from, to, inclusive })
            }
            Token::Str(s) => {
                self.advance();
                Ok(MatchPattern::Literal(LiteralPat::Str(s)))
            }
            Token::True => {
                self.advance();
                Ok(MatchPattern::Literal(LiteralPat::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(MatchPattern::Literal(LiteralPat::Bool(false)))
            }
            Token::Null => {
                self.advance();
                Ok(MatchPattern::Literal(LiteralPat::Null))
            }
            Token::Ident(name) => {
                self.advance();
                if name == "_" {
                    Ok(MatchPattern::Wildcard)
                } else {
                    Ok(MatchPattern::Binding(name))
                }
            }
            Token::LBrack => self.parse_match_array_pattern(),
            Token::Dollar => self.parse_match_object_pattern(),
            other => Err(self.err(ParseErrorKind::UnexpectedToken(other))),
        }
    }

    /// A numeric literal (with optional leading `-`) used in a literal
    /// or range pattern.
    fn parse_numeric_literal_pat(&mut self) -> Result<LiteralPat, ParseError> {
        let neg = self.matches(&Token::Minus);
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(LiteralPat::Int(if neg { -n } else { n }))
            }
            Token::Float(x) => {
                self.advance();
                Ok(LiteralPat::Float(if neg { -x } else { x }))
            }
            other => Err(ParseError::new(
                ParseErrorKind::UnexpectedToken(other),
                span,
            )),
        }
    }

    fn parse_match_array_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        self.expect(&Token::LBrack)?;
        let mut items: Vec<MatchPattern> = Vec::new();
        let mut rest: Option<String> = None;
        while !self.check(&Token::RBrack) {
            if matches!(self.peek(), Token::Ellipsis) {
                let ellipsis_span = self.peek_span();
                self.advance();
                rest = Some(self.parse_ident_token()?);
                if !self.check(&Token::RBrack) {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidPattern(
                            "`...rest` must be the last array element".into(),
                        ),
                        ellipsis_span,
                    ));
                }
                break;
            }
            items.push(self.parse_match_pattern()?);
            if !self.matches(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrack)?;
        Ok(MatchPattern::Array { items, rest })
    }

    fn parse_match_object_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        self.expect(&Token::Dollar)?;
        self.expect(&Token::LBrace)?;
        let mut fields: Vec<MatchField> = Vec::new();
        let mut rest: Option<String> = None;
        while !self.check(&Token::RBrace) {
            if matches!(self.peek(), Token::Ellipsis) {
                let ellipsis_span = self.peek_span();
                self.advance();
                rest = Some(self.parse_ident_token()?);
                if !self.check(&Token::RBrace) {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidPattern(
                            "`...rest` must be the last object field".into(),
                        ),
                        ellipsis_span,
                    ));
                }
                break;
            }
            let key = self.parse_ident_token()?;
            let pattern = if self.matches(&Token::Colon) {
                Some(self.parse_match_pattern()?)
            } else {
                None
            };
            fields.push(MatchField { key, pattern });
            if !self.matches(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(MatchPattern::Object { fields, rest })
    }

    fn parse_fn(&mut self) -> Result<SpannedExpr, ParseError> {
        let fn_span = self.expect(&Token::Fn)?;
        self.expect(&Token::LParen)?;
        let mut params: Vec<Pattern> = Vec::new();
        let mut defaults: Vec<Option<Box<SpannedExpr>>> = Vec::new();
        let mut rest: Option<Binder> = None;
        if !self.check(&Token::RParen) {
            loop {
                if matches!(self.peek(), Token::Ellipsis) {
                    let ellipsis_span = self.peek_span();
                    self.advance();
                    let name = self.parse_ident_binder()?;
                    rest = Some(name);
                    // rest must be the LAST param (spec §10.3)
                    if !self.check(&Token::RParen) {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidPattern(
                                "`...rest` must be the last parameter".into(),
                            ),
                            ellipsis_span,
                        ));
                    }
                    break;
                }
                let expr = self.parse_expr()?;
                // `name = default` parses as an `Assign` — treat it as a
                // defaulted parameter. Defaults are identifier-only.
                match &expr.expr {
                    Expr::Assign(name, None, rhs) => {
                        params.push(Pattern::Ident(name.clone()));
                        defaults.push(Some(rhs.clone()));
                    }
                    Expr::Assign(_, Some(_), _) => {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidPattern(
                                "a default parameter value must use `=`, not a \
                                 compound assignment"
                                    .into(),
                            ),
                            expr.span,
                        ));
                    }
                    Expr::AssignPattern(_, _) => {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidPattern(
                                "default values are only allowed on simple \
                                 (identifier) parameters"
                                    .into(),
                            ),
                            expr.span,
                        ));
                    }
                    _ => {
                        let pat = expr_to_pattern(&expr).map_err(|pe| {
                            ParseError::new(
                                ParseErrorKind::InvalidPattern(pe.message().to_string()),
                                pe.span(),
                            )
                        })?;
                        params.push(pat);
                        defaults.push(None);
                    }
                }
                if !self.matches(&Token::Comma) {
                    break;
                }
                if self.check(&Token::RParen) {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        let body = self.parse_scope()?;
        let span = fn_span.join(body.span);
        Ok(SpannedExpr::new(
            Expr::Fn {
                params,
                defaults,
                rest,
                body: Box::new(body),
                is_generator: false,
            },
            span,
        ))
    }

    /// `gen fn (params) { body }` — a generator function. Parses the
    /// `fn` literal that must follow and flips its `is_generator` flag.
    fn parse_gen(&mut self) -> Result<SpannedExpr, ParseError> {
        let gen_span = self.expect(&Token::Gen)?;
        let mut fn_expr = self.parse_fn()?;
        match &mut fn_expr.expr {
            Expr::Fn { is_generator, .. } => *is_generator = true,
            _ => unreachable!("parse_fn always yields Expr::Fn"),
        }
        fn_expr.span = gen_span.join(fn_expr.span);
        Ok(fn_expr)
    }

    fn parse_return(&mut self) -> Result<SpannedExpr, ParseError> {
        let ret_span = self.expect(&Token::Return)?;
        // `return` may stand alone or take a single expression. The
        // value is omitted if the next token cannot start an expression
        // (statement separators / closers / EOF).
        let omit = matches!(
            self.peek(),
            Token::Semicolon | Token::RParen | Token::RBrace | Token::RBrack
            | Token::Comma | Token::Eof
        );
        if omit {
            Ok(SpannedExpr::new(Expr::Return(None), ret_span))
        } else {
            let value = self.parse_expr()?;
            let span = ret_span.join(value.span);
            Ok(SpannedExpr::new(Expr::Return(Some(Box::new(value))), span))
        }
    }

    fn parse_array(&mut self) -> Result<SpannedExpr, ParseError> {
        let lbrack = self.expect(&Token::LBrack)?;
        let mut items = Vec::new();
        if !self.check(&Token::RBrack) {
            loop {
                items.push(self.parse_spreadable_expr()?);
                if !self.matches(&Token::Comma) {
                    break;
                }
                if self.check(&Token::RBrack) {
                    break;
                }
            }
        }
        let rbrack = self.expect(&Token::RBrack)?;
        let span = lbrack.join(rbrack);
        Ok(SpannedExpr::new(Expr::Array(items), span))
    }

    fn parse_object(&mut self) -> Result<SpannedExpr, ParseError> {
        let dollar = self.expect(&Token::Dollar)?;
        self.expect(&Token::LBrace)?;
        let mut members = Vec::new();
        if !self.check(&Token::RBrace) {
            loop {
                if matches!(self.peek(), Token::Ellipsis) {
                    self.advance();
                    let inner = self.parse_expr()?;
                    members.push(ObjectMember::Spread(inner));
                } else {
                    let key_span = self.peek_span();
                    let key = match self.peek().clone() {
                        Token::Ident(n) => {
                            self.advance();
                            n
                        }
                        Token::Str(s) => {
                            self.advance();
                            s
                        }
                        other => {
                            return Err(self.err(ParseErrorKind::UnexpectedToken(other)));
                        }
                    };
                    let value = if self.matches(&Token::Colon) {
                        self.parse_expr()?
                    } else {
                        // Shorthand `${name}` (and `${name, ...}`). The
                        // value is an Ident with the same name —
                        // a no-op in an object literal context, but
                        // critical for the `${name} := obj` pattern
                        // form (spec §11.2).
                        SpannedExpr::new(Expr::Ident(key.clone()), key_span)
                    };
                    members.push(ObjectMember::Pair(key, value));
                }
                if !self.matches(&Token::Comma) {
                    break;
                }
                if self.check(&Token::RBrace) {
                    break;
                }
            }
        }
        let rbrace = self.expect(&Token::RBrace)?;
        let span = dollar.join(rbrace);
        Ok(SpannedExpr::new(Expr::Object(members), span))
    }

    /// In contexts where `...expr` is allowed (array items, call
    /// arguments), parse an expression — but if it starts with `...`,
    /// wrap it as `Expr::Spread`.
    fn parse_spreadable_expr(&mut self) -> Result<SpannedExpr, ParseError> {
        if matches!(self.peek(), Token::Ellipsis) {
            let start = self.peek_span();
            self.advance();
            let inner = self.parse_expr()?;
            let span = start.join(inner.span);
            Ok(SpannedExpr::new(Expr::Spread(Box::new(inner)), span))
        } else {
            self.parse_expr()
        }
    }

    fn parse_scope(&mut self) -> Result<SpannedExpr, ParseError> {
        let lbrace = self.expect(&Token::LBrace)?;
        let block = self.parse_block_until(&[Token::RBrace]);
        let rbrace = self.expect(&Token::RBrace)?;
        let span = lbrace.join(rbrace);
        Ok(SpannedExpr::new(Expr::Scope(block), span))
    }

    fn parse_if(&mut self) -> Result<SpannedExpr, ParseError> {
        let if_span = self.expect(&Token::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_scope()?;
        let else_branch = if self.matches(&Token::Else) {
            // `else if ...` chains, otherwise must be a `{ ... }` scope
            if matches!(self.peek(), Token::If) {
                self.parse_if()?
            } else {
                self.parse_scope()?
            }
        } else {
            // synthesise a Null literal so the AST always has three children
            let span = then_branch.span;
            SpannedExpr::new(Expr::Null, span)
        };
        let span = if_span.join(else_branch.span);
        Ok(SpannedExpr::new(
            Expr::If(Box::new(cond), Box::new(then_branch), Box::new(else_branch)),
            span,
        ))
    }

    fn parse_while(&mut self) -> Result<SpannedExpr, ParseError> {
        let while_span = self.expect(&Token::While)?;
        let is_array = self.parse_array_form_marker()?;
        let cond = self.parse_expr()?;
        let body = self.parse_scope()?;
        let span = while_span.join(body.span);
        Ok(SpannedExpr::new(
            Expr::While {
                is_array,
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span,
        ))
    }

    fn parse_for(&mut self) -> Result<SpannedExpr, ParseError> {
        let for_span = self.expect(&Token::For)?;
        let is_array = self.parse_array_form_marker()?;
        self.expect(&Token::LParen)?;

        let first_var = self.parse_ident_binder()?;
        self.expect(&Token::Comma)?;

        // Parse next as a generic expression. If a comma follows, it was
        // a second iteration variable (must have been an Ident); the
        // real iterable comes after. If `)` follows, it was the iter.
        let next_expr = self.parse_expr()?;
        let (vars, iter) = if self.matches(&Token::Comma) {
            let second = match next_expr.expr {
                Expr::Ident(n) => Binder::new(n, next_expr.span),
                _ => {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidAssignTarget,
                        next_expr.span,
                    ));
                }
            };
            let iter = self.parse_expr()?;
            (vec![first_var, second], iter)
        } else {
            (vec![first_var], next_expr)
        };

        let rparen = self.expect(&Token::RParen)?;
        let body = self.parse_scope()?;
        let span = for_span.join(rparen).join(body.span);
        Ok(SpannedExpr::new(
            Expr::For {
                is_array,
                vars,
                iter: Box::new(iter),
                body: Box::new(body),
            },
            span,
        ))
    }

    /// `import expr` — the path is an arbitrary expression evaluated at
    /// runtime to a string. Path resolution happens in the VM.
    fn parse_import(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Import)?;
        let path_expr = self.parse_expr()?;
        let span = kw_span.join(path_expr.span);
        Ok(SpannedExpr::new(Expr::Import(Box::new(path_expr)), span))
    }

    fn parse_break(&mut self) -> Result<SpannedExpr, ParseError> {
        let break_span = self.expect(&Token::Break)?;
        let omit = matches!(
            self.peek(),
            Token::Semicolon | Token::RParen | Token::RBrace | Token::RBrack
            | Token::Comma | Token::Eof
        );
        if omit {
            Ok(SpannedExpr::new(Expr::Break(None), break_span))
        } else {
            let value = self.parse_expr()?;
            let span = break_span.join(value.span);
            Ok(SpannedExpr::new(Expr::Break(Some(Box::new(value))), span))
        }
    }

    fn parse_continue(&mut self) -> Result<SpannedExpr, ParseError> {
        let span = self.expect(&Token::Continue)?;
        Ok(SpannedExpr::new(Expr::Continue, span))
    }

    /// `[]` immediately after `for` / `while` marks the array-collecting
    /// form. Returns `true` if consumed.
    fn parse_array_form_marker(&mut self) -> Result<bool, ParseError> {
        if matches!(self.peek(), Token::LBrack) {
            self.advance();
            self.expect(&Token::RBrack)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Convert lexer-level template parts (literal text + raw source
    /// for each `{…}`) into AST-level parts by sub-lexing+sub-parsing
    /// each `Expr` slice as a standalone single-expression program.
    fn compile_template_parts(
        &self,
        lex_parts: Vec<LexTemplatePart>,
        outer_span: Span,
    ) -> Result<Vec<TemplatePart>, ParseError> {
        let mut out = Vec::with_capacity(lex_parts.len());
        for p in lex_parts {
            match p {
                LexTemplatePart::Lit(s) => out.push(TemplatePart::Lit(s)),
                LexTemplatePart::Expr(src) => {
                    let tokens = Lexer::new(&src).tokenize().map_err(|le| {
                        ParseError::new(
                            ParseErrorKind::InterpolationError(format!("{le}")),
                            outer_span,
                        )
                    })?;
                    let mut sub = Parser::new(tokens);
                    let expr = sub.parse_expr()?;
                    if !sub.check(&Token::Eof) {
                        return Err(ParseError::new(
                            ParseErrorKind::InterpolationError(
                                "interpolation must be a single expression".into(),
                            ),
                            outer_span,
                        ));
                    }
                    out.push(TemplatePart::Expr(expr));
                }
            }
        }
        Ok(out)
    }

    fn parse_ident_token(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(n) => {
                self.advance();
                Ok(n)
            }
            other => Err(self.err(ParseErrorKind::UnexpectedToken(other))),
        }
    }

    /// Like [`parse_ident_token`] but records the name's span as a
    /// [`Binder`], for binding positions (params, `...rest`, loop vars,
    /// the `catch` parameter) the tooling needs to locate.
    fn parse_ident_binder(&mut self) -> Result<Binder, ParseError> {
        let span = self.peek_span();
        let name = self.parse_ident_token()?;
        Ok(Binder::new(name, span))
    }
}

/// Parse, recovering past errors. Returns a (possibly partial) program
/// and every parse error found. Used by tooling (the LSP) that wants all
/// diagnostics at once.
pub fn parse_recover(tokens: Vec<SpannedToken>) -> (Block, Vec<ParseError>) {
    Parser::new(tokens).parse_program_recover()
}

/// Parse, reporting only the first error. This is the run path's
/// contract: a program with any syntax error can't execute, so the first
/// error is all the caller needs.
pub fn parse(tokens: Vec<SpannedToken>) -> Result<Block, ParseError> {
    let (block, mut errors) = parse_recover(tokens);
    if errors.is_empty() {
        Ok(block)
    } else {
        Err(errors.remove(0))
    }
}
