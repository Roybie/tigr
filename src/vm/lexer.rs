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
//! Two string literals: single-quoted `'…'` interpolates `{expr}` and
//! processes backslash escapes (`\n \t \r \\ \' \{`); double-quoted
//! `"…"` is fully raw — no interpolation, no escapes — and always
//! lexes to a plain `Token::Str` (v0.17, see `scan_raw_string`).

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
            '0'..='9' => self.scan_number(start, line),
            'a'..='z' | 'A'..='Z' | '_' => Ok(self.scan_ident(start)),
            '\'' => self.scan_string(start, line),
            '"' => self.scan_raw_string(start, line),

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
                Ok(if self.matches('^') { Token::CaretCaret } else { Token::Caret })
            }
            '~' => {
                self.advance();
                Ok(Token::Tilde)
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
                // Leading-dot float (`.5` ≡ `0.5`). Detect before
                // consuming the dot; if the next char is a digit, hand
                // off to the number scanner so it sees the literal
                // starting at `start`.
                if matches!(self.peek_two(), Some('0'..='9')) {
                    return self.scan_number(start, line);
                }
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
                Ok(if self.matches('=') {
                    Token::EqEq
                } else if self.matches('>') {
                    Token::FatArrow
                } else {
                    Token::Eq
                })
            }
            '!' => {
                self.advance();
                Ok(if self.matches('=') { Token::BangEq } else { Token::Bang })
            }
            '<' => {
                self.advance();
                Ok(if self.matches('=') {
                    Token::LtEq
                } else if self.matches('<') {
                    Token::Shl
                } else {
                    Token::Lt
                })
            }
            '>' => {
                self.advance();
                Ok(if self.matches('=') {
                    Token::GtEq
                } else if self.matches('>') {
                    Token::Shr
                } else {
                    Token::Gt
                })
            }
            '&' => {
                self.advance();
                Ok(if self.matches('&') { Token::AmpAmp } else { Token::Amp })
            }
            '|' => {
                self.advance();
                if self.matches('|') {
                    Ok(Token::PipePipe)
                } else if self.matches('>') {
                    Ok(Token::PipeGt)
                } else {
                    Ok(Token::Pipe)
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

    fn scan_number(&mut self, start: usize, line: u32) -> Result<Token, LexError> {
        let first = self.peek().expect("scan_number called with no input");

        // ---- Leading-dot float (.5 ≡ 0.5) ----
        if first == '.' {
            self.advance(); // consume '.'
            // First char of the digit run must be a digit (the
            // dispatch in `scan_one` already verified peek_two).
            self.advance(); // consume first fractional digit
            self.scan_digit_run(line, 10)?;
            self.maybe_scan_exponent(line)?;
            return self.parse_decimal_token(start, line, true);
        }

        // ---- 0x / 0b / 0o radix int ----
        if first == '0' {
            let radix = match self.peek_two() {
                Some('x') | Some('X') => Some(16u32),
                Some('b') | Some('B') => Some(2),
                Some('o') | Some('O') => Some(8),
                _ => None,
            };
            if let Some(radix) = radix {
                self.advance(); // '0'
                self.advance(); // x/b/o
                // Need at least one valid digit; reject `0x_FF`,
                // `0xZZZ`, bare `0x`.
                match self.peek() {
                    Some(c) if c.is_digit(radix) => {
                        self.advance();
                        self.scan_digit_run(line, radix)?;
                    }
                    _ => {
                        return Err(LexError::new(
                            LexErrorKind::MalformedNumber(
                                self.source[start..self.pos].to_string(),
                            ),
                            Span::new(start, self.pos.max(start + 1), line),
                        ));
                    }
                }
                let lexeme = &self.source[start..self.pos];
                let body = &lexeme[2..]; // strip prefix
                let cleaned: String = body.chars().filter(|c| *c != '_').collect();
                return match i64::from_str_radix(&cleaned, radix) {
                    Ok(n) => Ok(Token::Int(n)),
                    Err(_) => Err(LexError::new(
                        LexErrorKind::NumberOutOfRange(lexeme.to_string()),
                        Span::new(start, self.pos, line),
                    )),
                };
            }
        }

        // ---- Plain decimal int / float ----
        self.advance(); // consume the first digit (already validated)
        self.scan_digit_run(line, 10)?;

        let mut is_float = false;

        // Fractional part: `.` followed by a digit. A bare `5.` (no
        // digit after) leaves the `.` for the next token (`Dot`),
        // matching how `5.method` works.
        if self.peek() == Some('.') && matches!(self.peek_two(), Some('0'..='9')) {
            self.advance(); // '.'
            self.advance(); // first fractional digit
            self.scan_digit_run(line, 10)?;
            is_float = true;
        }

        // Exponent — turns the literal into a Float regardless of
        // whether a fractional part was present.
        if self.maybe_scan_exponent(line)? {
            is_float = true;
        }

        self.parse_decimal_token(start, line, is_float)
    }

    /// Scan zero or more digit-or-underscore characters AFTER the
    /// caller has already consumed a leading digit. Underscores must
    /// be sandwiched between digits — leading, trailing, or doubled
    /// underscores raise a `LexError`.
    fn scan_digit_run(&mut self, line: u32, radix: u32) -> Result<(), LexError> {
        loop {
            match self.peek() {
                Some(c) if c.is_digit(radix) => {
                    self.advance();
                }
                Some('_') => {
                    let pos = self.pos;
                    let next_ok = matches!(self.peek_two(), Some(c) if c.is_digit(radix));
                    if !next_ok {
                        return Err(LexError::new(
                            LexErrorKind::MalformedNumber(
                                "underscore must be between digits".into(),
                            ),
                            Span::new(pos, pos + 1, line),
                        ));
                    }
                    self.advance();
                }
                _ => return Ok(()),
            }
        }
    }

    /// Try to consume an `[eE][+-]?digits` exponent. Returns `Ok(true)`
    /// if one was consumed, `Ok(false)` if the next char isn't `e`/`E`
    /// or the lookahead doesn't form a valid exponent (in which case
    /// the `e` is left for the next token — usually an identifier).
    fn maybe_scan_exponent(&mut self, line: u32) -> Result<bool, LexError> {
        if !matches!(self.peek(), Some('e') | Some('E')) {
            return Ok(false);
        }
        // Look ahead WITHOUT committing.
        let mut iter = self.chars.clone();
        iter.next(); // e/E
        let after = iter.next();
        let valid = match after {
            Some(c) if c.is_ascii_digit() => true,
            Some('+') | Some('-') => matches!(iter.next(), Some(c) if c.is_ascii_digit()),
            _ => false,
        };
        if !valid {
            return Ok(false);
        }
        self.advance(); // e/E
        if matches!(self.peek(), Some('+') | Some('-')) {
            self.advance();
        }
        // We just verified the next char is a digit.
        self.advance();
        self.scan_digit_run(line, 10)?;
        Ok(true)
    }

    fn parse_decimal_token(
        &self,
        start: usize,
        line: u32,
        is_float: bool,
    ) -> Result<Token, LexError> {
        let lexeme = &self.source[start..self.pos];
        let cleaned: String = lexeme.chars().filter(|c| *c != '_').collect();
        if is_float {
            cleaned.parse::<f64>().map(Token::Float).map_err(|_| {
                LexError::new(
                    LexErrorKind::NumberOutOfRange(lexeme.to_string()),
                    Span::new(start, self.pos, line),
                )
            })
        } else {
            cleaned.parse::<i64>().map(Token::Int).map_err(|_| {
                LexError::new(
                    LexErrorKind::NumberOutOfRange(lexeme.to_string()),
                    Span::new(start, self.pos, line),
                )
            })
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
            "continue" => Token::Continue,
            "return" => Token::Return,
            "import" => Token::Import,
            "try" => Token::Try,
            "catch" => Token::Catch,
            "raise" => Token::Raise,
            "match" => Token::Match,
            "spawn" => Token::Spawn,
            "select" => Token::Select,
            "parallel" => Token::Parallel,
            "go" => Token::Go,
            "yield" => Token::Yield,
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

    /// Scan a `"…"` raw string literal (spec §8.2). The opening `"`
    /// has been peeked but not consumed.
    ///
    /// Fully raw: there is **no** `{expr}` interpolation and **no**
    /// backslash escaping — `{`, `}`, and `\` are all ordinary
    /// characters. The only terminator is a closing `"`; a `"` cannot
    /// itself appear inside a `"…"` string (use `'…'` for that).
    /// The result is always a plain `Token::Str`.
    fn scan_raw_string(&mut self, start: usize, line: u32) -> Result<Token, LexError> {
        self.advance(); // opening "
        let mut lit = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(LexError::new(
                        LexErrorKind::UnterminatedString,
                        Span::new(start, self.pos, line),
                    ));
                }
                Some('"') => return Ok(Token::Str(lit)),
                Some(c) => lit.push(c),
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
