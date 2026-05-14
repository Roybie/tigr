//! Hand-written precedence-climbing parser for Tigr.
//!
//! Phase 1 grammar (subset):
//!
//! ```text
//! Program     ::= Block
//! Block       ::= (Expr ';')* Expr?
//! Expr        ::= Assign
//! Assign      ::= Ident (':=' | '=') Assign
//!               | Additive
//! Additive    ::= Multiplicative (('+' | '-') Multiplicative)*
//! Multiplicative ::= Power (('*' | '/' | '%') Power)*
//! Power       ::= Unary ('^' Power)?              -- right-assoc
//! Unary       ::= '-' Unary | Primary
//! Primary     ::= INT | FLOAT | STR | 'true' | 'false' | 'null' | IDENT
//!               | '(' Block ')'
//! ```

use crate::vm::ast::{BinOp, Block, Expr, SpannedExpr, UnOp};
use crate::vm::error::{ParseError, ParseErrorKind};
use crate::vm::token::{Span, SpannedToken, Token};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Parser { tokens, pos: 0 }
    }

    pub fn parse_program(&mut self) -> Result<Block, ParseError> {
        let block = self.parse_block_until(&[Token::Eof])?;
        if !self.check(&Token::Eof) {
            return Err(self.err(ParseErrorKind::ExpectedEof(self.peek().clone())));
        }
        Ok(block)
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

    fn parse_block_until(&mut self, terminators: &[Token]) -> Result<Block, ParseError> {
        let mut stmts = Vec::new();
        let mut tail: Option<Box<SpannedExpr>> = None;

        if self.check_any(terminators) {
            return Ok(Block { stmts, tail });
        }

        loop {
            let expr = self.parse_expr()?;
            if self.matches(&Token::Semicolon) {
                stmts.push(expr);
                if self.check_any(terminators) {
                    // trailing `;` → tail stays None → block value is null
                    return Ok(Block { stmts, tail });
                }
                continue;
            } else {
                tail = Some(Box::new(expr));
                return Ok(Block { stmts, tail });
            }
        }
    }

    // -- expressions ---------------------------------------------------

    fn parse_expr(&mut self) -> Result<SpannedExpr, ParseError> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<SpannedExpr, ParseError> {
        let left = self.parse_additive()?;

        // Recognise `x := expr` and `x = expr`. Phase 1 only allows a
        // bare identifier on the LHS; pattern destructuring arrives in
        // Phase 7 and indexed assignment in Phase 3.
        match self.peek() {
            Token::ColonEq | Token::Eq => {
                let op = self.peek().clone();
                let op_span = self.peek_span();
                self.advance();
                let name = match &left.expr {
                    Expr::Ident(name) => name.clone(),
                    _ => {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidAssignTarget,
                            left.span,
                        ));
                    }
                };
                let right = self.parse_assign()?; // right-assoc
                let span = left.span.join(op_span).join(right.span);
                let node = match op {
                    Token::ColonEq => Expr::Decl(name, Box::new(right)),
                    Token::Eq => Expr::Assign(name, Box::new(right)),
                    _ => unreachable!(),
                };
                Ok(SpannedExpr::new(node, span))
            }
            _ => Ok(left),
        }
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
        if matches!(self.peek(), Token::Caret) {
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
        match self.peek() {
            Token::Minus => {
                let start = self.peek_span();
                self.advance();
                let inner = self.parse_unary()?;
                let span = start.join(inner.span);
                Ok(SpannedExpr::new(Expr::UnOp(UnOp::Neg, Box::new(inner)), span))
            }
            _ => self.parse_primary(),
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
                let block = self.parse_block_until(&[Token::RParen])?;
                let rparen = self.expect(&Token::RParen)?;
                let span = lparen.join(rparen);
                Ok(SpannedExpr::new(Expr::Block(block), span))
            }
            other => Err(self.err(ParseErrorKind::UnexpectedToken(other))),
        }
    }
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Block, ParseError> {
    Parser::new(tokens).parse_program()
}
