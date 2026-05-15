//! Single-pass byte-indexed lexer.
//!
//! Phase 1 subset: identifiers, keywords (null/true/false plus the rest
//! reserved), int/float literals, simple single-quoted strings (no
//! interpolation yet — `{` inside a string is currently a literal char),
//! arithmetic operators, comparison & logical operators, assignment,
//! parens/braces/brackets, ; , : . # $.
//!
//! Multi-char operators (`..` / `..=` / `...`, `==`, `!=`, `<=`, `>=`,
//! `&&`, `||`, `:=`, `+=` etc., `|>`) all dispatch by peeking the next
//! char.
//!
//! String interpolation arrives in Phase 6; for now `'foo {x} bar'` is
//! lexed as the literal string `foo {x} bar`. Backslash escapes
//! supported: `\n \t \r \\ \' \{`.

use std::str::Chars;

use crate::vm::error::{LexError, LexErrorKind};
use crate::vm::token::{Span, SpannedToken, TemplatePart, Token};

pub struct Lexer<'src> {
    source: &'src str,
    chars: Chars<'src>,
    /// byte position of the *next* char to be consumed
    pos: usize,
    line: u32,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Lexer {
            source,
            chars: source.chars(),
            pos: 0,
            line: 1,
        }
    }

    /// Tokenize the entire source. Returns a `Vec<SpannedToken>` ending
    /// in `Token::Eof`. For Phase 1 we tokenize eagerly; if this becomes
    /// a memory issue later we can switch to a streaming `Iterator`.
    pub fn tokenize(mut self) -> Result<Vec<SpannedToken>, LexError> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            let start = self.pos;
            let line = self.line;
            let Some(c) = self.peek() else {
                out.push(SpannedToken::new(Token::Eof, Span::new(start, start, line)));
                return Ok(out);
            };
            let token = self.scan_one(c)?;
            let end = self.pos;
            out.push(SpannedToken::new(token, Span::new(start, end, line)));
        }
    }

    // -- single-char inspection -----------------------------------------

    fn peek(&self) -> Option<char> {
        self.chars.clone().next()
    }

    fn peek_two(&self) -> Option<char> {
        let mut iter = self.chars.clone();
        iter.next();
        iter.next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn matches(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    // -- skipping -------------------------------------------------------

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('/') if self.peek_two() == Some('/') => {
                    self.advance();
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('/') if self.peek_two() == Some('*') => {
                    self.advance();
                    self.advance();
                    loop {
                        match self.advance() {
                            None => return, // unterminated comment — let parser flag EOF
                            Some('*') if self.peek() == Some('/') => {
                                self.advance();
                                break;
                            }
                            Some(_) => {}
                        }
                    }
                }
                _ => return,
            }
        }
    }

    // -- main dispatch --------------------------------------------------

    fn scan_one(&mut self, c: char) -> Result<Token, LexError> {
        let start = self.pos;
        let line = self.line;
        match c {
            '0'..='9' => Ok(self.scan_number(start)),
            'a'..='z' | 'A'..='Z' | '_' => Ok(self.scan_ident(start)),
            '\'' => self.scan_string(start, line),

            '+' => {
                self.advance();
                Ok(if self.matches('=') { Token::PlusEq } else { Token::Plus })
            }
            '-' => {
                self.advance();
                Ok(if self.matches('=') { Token::MinusEq } else { Token::Minus })
            }
            '*' => {
                self.advance();
                Ok(if self.matches('=') { Token::StarEq } else { Token::Star })
            }
            '/' => {
                // comments are handled in skip_*; a bare `/` is division
                self.advance();
                Ok(if self.matches('=') { Token::SlashEq } else { Token::Slash })
            }
            '%' => {
                self.advance();
                Ok(if self.matches('=') { Token::PercentEq } else { Token::Percent })
            }
            '^' => {
                self.advance();
                Ok(Token::Caret)
            }
            '#' => {
                self.advance();
                Ok(Token::Hash)
            }
            '$' => {
                self.advance();
                Ok(Token::Dollar)
            }
            ',' => {
                self.advance();
                Ok(Token::Comma)
            }
            ';' => {
                self.advance();
                Ok(Token::Semicolon)
            }
            '(' => {
                self.advance();
                Ok(Token::LParen)
            }
            ')' => {
                self.advance();
                Ok(Token::RParen)
            }
            '{' => {
                self.advance();
                Ok(Token::LBrace)
            }
            '}' => {
                self.advance();
                Ok(Token::RBrace)
            }
            '[' => {
                self.advance();
                Ok(Token::LBrack)
            }
            ']' => {
                self.advance();
                Ok(Token::RBrack)
            }

            '.' => {
                self.advance();
                if self.matches('.') {
                    if self.matches('.') {
                        Ok(Token::Ellipsis)
                    } else if self.matches('=') {
                        Ok(Token::DotDotEq)
                    } else {
                        Ok(Token::DotDot)
                    }
                } else {
                    Ok(Token::Dot)
                }
            }
            ':' => {
                self.advance();
                Ok(if self.matches('=') { Token::ColonEq } else { Token::Colon })
            }
            '=' => {
                self.advance();
                Ok(if self.matches('=') { Token::EqEq } else { Token::Eq })
            }
            '!' => {
                self.advance();
                Ok(if self.matches('=') { Token::BangEq } else { Token::Bang })
            }
            '<' => {
                self.advance();
                Ok(if self.matches('=') { Token::LtEq } else { Token::Lt })
            }
            '>' => {
                self.advance();
                Ok(if self.matches('=') { Token::GtEq } else { Token::Gt })
            }
            '&' => {
                self.advance();
                if self.matches('&') {
                    Ok(Token::AmpAmp)
                } else {
                    Err(LexError::new(
                        LexErrorKind::InvalidChar('&'),
                        Span::new(start, self.pos, line),
                    ))
                }
            }
            '|' => {
                self.advance();
                if self.matches('|') {
                    Ok(Token::PipePipe)
                } else if self.matches('>') {
                    Ok(Token::PipeGt)
                } else {
                    Err(LexError::new(
                        LexErrorKind::InvalidChar('|'),
                        Span::new(start, self.pos, line),
                    ))
                }
            }
            other => {
                self.advance();
                Err(LexError::new(
                    LexErrorKind::InvalidChar(other),
                    Span::new(start, self.pos, line),
                ))
            }
        }
    }

    // -- specific scanners ---------------------------------------------

    fn scan_number(&mut self, start: usize) -> Token {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        // Float? Need a '.' followed by a digit (so we don't swallow
        // the `..` range operator, or a trailing `.method`).
        if self.peek() == Some('.') && matches!(self.peek_two(), Some('0'..='9')) {
            self.advance(); // consume '.'
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            let lexeme = &self.source[start..self.pos];
            Token::Float(lexeme.parse().unwrap())
        } else {
            let lexeme = &self.source[start..self.pos];
            Token::Int(lexeme.parse().unwrap())
        }
    }

    fn scan_ident(&mut self, start: usize) -> Token {
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        let lexeme = &self.source[start..self.pos];
        match lexeme {
            "null" => Token::Null,
            "true" => Token::True,
            "false" => Token::False,
            "fn" => Token::Fn,
            "if" => Token::If,
            "else" => Token::Else,
            "for" => Token::For,
            "while" => Token::While,
            "break" => Token::Break,
            "return" => Token::Return,
            "import" => Token::Import,
            _ => Token::Ident(lexeme.to_string()),
        }
    }

    /// Scan a `'…'` string literal, capturing `{expr}` interpolation
    /// segments along the way (spec §8.2). The opening `'` has been
    /// peeked but not consumed.
    ///
    /// - `\{` escapes a literal `{`; all other backslash escapes work
    ///   as before.
    /// - When at least one `{…}` appears, the result is a
    ///   `Token::StrTemplate` with alternating `Lit`/`Expr` parts.
    ///   Otherwise the result is the existing `Token::Str`, so plain
    ///   strings cost nothing extra.
    /// - Brace-counting inside `{…}` is local to this scanner and
    ///   ignores braces that occur inside nested string literals — so
    ///   `'{ if x { 'yes' } else { 'no' } }'` parses correctly.
    fn scan_string(&mut self, start: usize, line: u32) -> Result<Token, LexError> {
        self.advance(); // opening '
        let mut current_lit = String::new();
        let mut parts: Vec<TemplatePart> = Vec::new();
        loop {
            match self.advance() {
                None => {
                    return Err(LexError::new(
                        LexErrorKind::UnterminatedString,
                        Span::new(start, self.pos, line),
                    ));
                }
                Some('\'') => {
                    if parts.is_empty() {
                        return Ok(Token::Str(current_lit));
                    }
                    if !current_lit.is_empty() {
                        parts.push(TemplatePart::Lit(current_lit));
                    }
                    return Ok(Token::StrTemplate(parts));
                }
                Some('{') => {
                    if !current_lit.is_empty() {
                        parts.push(TemplatePart::Lit(std::mem::take(&mut current_lit)));
                    }
                    let expr_src = self.scan_interp_expr(start, line)?;
                    parts.push(TemplatePart::Expr(expr_src));
                }
                Some('\\') => match self.advance() {
                    Some('n') => current_lit.push('\n'),
                    Some('t') => current_lit.push('\t'),
                    Some('r') => current_lit.push('\r'),
                    Some('\\') => current_lit.push('\\'),
                    Some('\'') => current_lit.push('\''),
                    Some('{') => current_lit.push('{'),
                    Some(other) => current_lit.push(other),
                    None => {
                        return Err(LexError::new(
                            LexErrorKind::UnterminatedString,
                            Span::new(start, self.pos, line),
                        ));
                    }
                },
                Some(c) => current_lit.push(c),
            }
        }
    }

    /// Scan from just after a `{` to its matching `}`. Returns the
    /// source slice between them. Tracks brace depth, skipping braces
    /// inside nested string literals so the depth counter doesn't
    /// confuse a nested `'a {x}'` with a real interpolation level.
    fn scan_interp_expr(
        &mut self,
        str_start: usize,
        line: u32,
    ) -> Result<String, LexError> {
        let expr_start = self.pos;
        let mut depth: i32 = 1;
        let mut in_str = false;
        let mut escape = false;
        loop {
            let c = match self.advance() {
                Some(c) => c,
                None => {
                    return Err(LexError::new(
                        LexErrorKind::UnterminatedString,
                        Span::new(str_start, self.pos, line),
                    ));
                }
            };
            if in_str {
                if escape {
                    escape = false;
                    continue;
                }
                match c {
                    '\\' => escape = true,
                    '\'' => in_str = false,
                    _ => {}
                }
            } else {
                match c {
                    '\'' => in_str = true,
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            // `self.pos` now points just past the `}`.
                            let expr_end = self.pos - 1;
                            return Ok(self.source[expr_start..expr_end].to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
