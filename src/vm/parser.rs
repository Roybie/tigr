//! Hand-written precedence-climbing parser for Tigr.
//!
//! Phase 3 grammar:
//!
//! ```text
//! Program     ::= Block
//! Block       ::= (Expr ';')* Expr?
//! Expr        ::= Assign
//! Assign      ::= Postfix (':=' | '=' | '+=' | '-=' | '*=' | '/=' | '%=') Assign
//!               | LogicOr
//! LogicOr     ::= LogicAnd ('||' LogicAnd)*
//! LogicAnd    ::= Equality ('&&' Equality)*
//! Equality    ::= Additive (('==' | '!=' | '<' | '>' | '<=' | '>=') Additive)*
//! Additive    ::= Multiplicative (('+' | '-') Multiplicative)*
//! Multiplicative ::= Power (('*' | '/' | '%') Power)*
//! Power       ::= Unary ('^' Power)?              -- right-assoc
//! Unary       ::= ('-' | '!' | '#') Unary | Postfix
//! Postfix     ::= Primary ( '[' Expr ']' | '.' IDENT | '(' Args? ')' )*
//! Primary     ::= INT | FLOAT | STR | 'true' | 'false' | 'null' | IDENT
//!               | '(' Block ')'
//!               | '{' Block '}'                    -- scope
//!               | '[' (Expr (',' Expr)* ','?)? ']' -- array literal
//!               | '$' '{' (ObjPair (',' ObjPair)* ','?)? '}'  -- object literal
//!               | 'if' Expr Scope ('else' (Scope | If))?
//!               | 'while' Expr Scope
//! ObjPair     ::= (IDENT | STR) ':' Expr
//! Args        ::= Expr (',' Expr)*
//! ```

use crate::vm::ast::{
    expr_to_pattern, BinOp, Block, Expr, ObjectMember, Pattern,
    SpannedExpr, TemplatePart, UnOp,
};
use crate::vm::error::{ParseError, ParseErrorKind};
use crate::vm::lexer::Lexer;
use crate::vm::token::{
    Span, SpannedToken, TemplatePart as LexTemplatePart, Token,
};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

enum AssignKind {
    Decl,
    Assign(Option<BinOp>),
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
                    Expr::Assign(name, op, Box::new(right)),
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
        let mut left = self.parse_pipe()?;
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
            let right = self.parse_pipe()?;
            let span = left.span.join(right.span);
            left = SpannedExpr::new(
                Expr::BinOp(op, Box::new(left), Box::new(right)),
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
        let left = self.parse_additive()?;
        let inclusive = match self.peek() {
            Token::DotDot => false,
            Token::DotDotEq => true,
            _ => return Ok(left),
        };
        self.advance();
        let to = self.parse_additive()?;
        let step = if matches!(self.peek(), Token::Colon) {
            self.advance();
            Some(Box::new(self.parse_additive()?))
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
        let op = match self.peek() {
            Token::Minus => Some(UnOp::Neg),
            Token::Bang => Some(UnOp::Not),
            Token::Hash => Some(UnOp::Len),
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
                let block = self.parse_block_until(&[Token::RParen])?;
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
            Token::Import => self.parse_import(),
            Token::Try => self.parse_try(),
            Token::Raise => self.parse_raise(),
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
            let param = self.parse_ident_token()?;
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

    fn parse_fn(&mut self) -> Result<SpannedExpr, ParseError> {
        let fn_span = self.expect(&Token::Fn)?;
        self.expect(&Token::LParen)?;
        let mut params: Vec<Pattern> = Vec::new();
        let mut rest: Option<String> = None;
        if !self.check(&Token::RParen) {
            loop {
                if matches!(self.peek(), Token::Ellipsis) {
                    let ellipsis_span = self.peek_span();
                    self.advance();
                    let name = self.parse_ident_token()?;
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
                let pat = expr_to_pattern(&expr).map_err(|pe| {
                    ParseError::new(
                        ParseErrorKind::InvalidPattern(pe.message().to_string()),
                        pe.span(),
                    )
                })?;
                params.push(pat);
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
            Expr::Fn { params, rest, body: Box::new(body) },
            span,
        ))
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
        let block = self.parse_block_until(&[Token::RBrace])?;
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

        let first_var = self.parse_ident_token()?;
        self.expect(&Token::Comma)?;

        // Parse next as a generic expression. If a comma follows, it was
        // a second iteration variable (must have been an Ident); the
        // real iterable comes after. If `)` follows, it was the iter.
        let next_expr = self.parse_expr()?;
        let (vars, iter) = if self.matches(&Token::Comma) {
            let second = match next_expr.expr {
                Expr::Ident(n) => n,
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

    /// `import 'path'` — the path must be a single literal string
    /// (no interpolation). Compile-time path resolution happens later
    /// in the compiler.
    fn parse_import(&mut self) -> Result<SpannedExpr, ParseError> {
        let kw_span = self.expect(&Token::Import)?;
        let path_span = self.peek_span();
        let path = match self.peek().clone() {
            Token::Str(s) => {
                self.advance();
                s
            }
            Token::StrTemplate(_) => {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidPattern(
                        "`import` path must be a plain string literal".into(),
                    ),
                    path_span,
                ));
            }
            other => return Err(self.err(ParseErrorKind::UnexpectedToken(other))),
        };
        let span = kw_span.join(path_span);
        Ok(SpannedExpr::new(Expr::Import(path), span))
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
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Block, ParseError> {
    Parser::new(tokens).parse_program()
}
