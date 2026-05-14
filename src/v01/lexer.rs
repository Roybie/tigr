use syntax::{Token};

pub type Spanned<Tok, Loc, Error> = Result<(Loc, Tok, Loc), Error>;

pub struct Lexer <'a> {
    i : usize,
    j : usize,
    line: usize,
    token : Option<Token<'a>>,
    source : &'a str,

    size_left : usize, // in bytes
    size_right : usize, // in bytes
}

#[derive(Debug)]
pub enum LexicalError<'a> {
    InvalidToken(usize, Token<'a>, usize),
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Lexer {
            i:0,
            j:0,
            line:1,
            token : None,
            source : source,

            size_left : 0,
            size_right : 0,
        }
    }

    fn scan_spaces(&mut self){
        let mut x = self.i;
        let mut new_right = self.size_left;
        loop {
            match self.source.char_indices().nth(x) {
                Some((i,' ')) | Some((i,'\t')) => {
                    x += 1;
                    new_right = i + ' '.len_utf8();
                },
                Some((i,'\n')) => {
                    self.line += 1;
                    x += 1;
                    new_right = i + '\n'.len_utf8();
                },
                _ => break,
            }
        }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            self.token = Some(Token::IgnoreWhitespace);
        }
    }

    fn scan_comment_monoline(&mut self){
        let mut x = self.i;
        let mut new_right = self.size_left;
        if self.source.chars().nth(x) == Some('/') &&
            self.source.chars().nth(x+1) == Some('/') {
                x += 2;
                new_right +=  '/'.len_utf8()*2;
                loop {
                    match self.source.char_indices().nth(x) {
                        None => break,
                        Some((_,'\n')) => break,
                        Some((i,c)) => {
                            x += 1;
                            new_right = i + c.len_utf8();
                        },
                    }
                }
            }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            self.token = Some(Token::IgnoreComment);
        }
    }

    fn scan_comment_multiline(&mut self){
        let mut x = self.i;
        let mut new_right = self.size_left;
        let mut iter = self.source.char_indices();
        if iter.nth(x) == Some((new_right,'/')) &&
            iter.next() == Some((new_right+'/'.len_utf8(),'*')) {
                x += 2;
                new_right +=  '/'.len_utf8() + '*'.len_utf8();
                'outer: loop {
                    match iter.next() {
                        Some((i,'*')) => {
                            x += 1;
                            new_right = i + '*'.len_utf8();
                            'inner: loop {
                                match iter.next() {
                                    Some((i,'/')) => {
                                        x += 1;
                                        new_right = i + '/'.len_utf8();
                                        break 'outer;
                                    },
                                    Some((i,'*')) => {
                                        x += 1;
                                        new_right = i + '*'.len_utf8();
                                        continue 'inner;
                                    },
                                    Some((i,'\n')) => {
                                        self.line += 1;
                                        x += 1;
                                        new_right = i + '\n'.len_utf8();
                                        continue 'outer;
                                    },
                                    Some((i,c)) => {
                                        x += 1;
                                        new_right = i + c.len_utf8();
                                        continue 'outer;
                                    },
                                    None => {
                                        break 'outer;
                                    },
                                }
                            }
                        },
                        Some((i,'\n')) => {
                            self.line += 1;
                            x += 1;
                            new_right = i + '\n'.len_utf8();
                            continue 'outer;
                        },
                        Some((i,c)) => {
                            x += 1;
                            new_right = i + c.len_utf8();
                            continue 'outer;
                        },
                        None => {
                            break 'outer;
                        },
                    }
                }
            }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            self.token = Some(Token::IgnoreComment);
        }
    }

    fn scan_number(&mut self){
        let mut x = self.i;
        let mut new_right = self.size_left;
        let mut float = false;
        let mut iter = self.source.char_indices().skip(x);
        loop {
            match iter.next() {
                Some((i,'0' ... '9')) => {
                    x += 1;
                    new_right = i + '0'.len_utf8();
                },
                Some((i,'.')) => {
                    let mut iter_tmp = iter.clone();
                    // Looking for a '..' operator which can confuse with a float like "1."
                    match iter_tmp.next() {
                        Some((_, '.')) => break,
                        Some((_, '0' ... '9')) => {},
                        _ => break,
                    }
                    if float == true { break; }
                    float = true;
                    x += 1;
                    new_right = i + '.'.len_utf8();
                }
                _ => break,
            }
        }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            let content = &self.source[self.size_left..self.size_right];
            self.token = if float {
                Some(Token::Float(content))
            } else {
                Some(Token::Integer(content))
            };
        }
    }

    fn scan_identifier(&mut self){
        let mut x = self.i;
        let mut new_right = self.size_left;
        let mut iter = self.source.char_indices().skip(self.i);
        'outer: loop {
            match iter.next() {
                Some((i,c)) if c.is_alphabetic() || c == '_' => {
                    x += 1;
                    new_right = i + c.len_utf8();
                    'inner: loop {
                        match iter.next() {
                            Some((i,c)) if c.is_alphabetic() ||
                                c.is_numeric() ||
                                c == '_' => {
                                    x += 1;
                                    new_right = i + c.len_utf8();
                                    continue 'inner;
                                },
                                _ => break 'outer,
                        }
                    }
                },
                _ => {
                    break 'outer
                },
            }
        }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            let content = &self.source[self.size_left..self.size_right];
            self.token = Some(Token::Id(content));
        }
    }

    fn scan_string_type(&mut self) {
        let mut x = self.i;
        let mut new_right = self.size_left;
        let mut new_left = self.size_left;
        let mut iter = self.source.char_indices().skip(self.i);
        'outer: loop {
            match iter.next() {
                Some((i,c)) if c == '\'' => {
                    x += 1;
                    new_left = i + c.len_utf8();
                    'inner: loop {
                        match iter.next() {
                            Some((_,'\\')) => {
                                x += 2;
                                iter.next();
                                continue 'inner;
                            },
                            Some((i,'\'')) => {
                                x += 1;
                                new_right = i + '\''.len_utf8();
                                break 'outer;
                            },
                            _ => {
                                x += 1;
                                continue 'inner;
                            },
                        }
                    }
                },
                _ => break 'outer,
            }
        }

        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            let content = &self.source[new_left..(self.size_right-'\''.len_utf8())];
            self.token = Some(Token::String(content));
        }
    }

    fn scan_string(&mut self, keyword : &str, tok : Token<'a>){
        let mut x = self.i;
        let mut new_right = self.size_left;
        let iter = self.source.char_indices().skip(self.i);
        let ik = keyword.chars();
        for ((i,a),b) in iter.zip(ik) {
            if a == b {
                x += 1;
                new_right = i + a.len_utf8();
            } else {
                return;
            }
        }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            self.token = Some(tok);
        }
    }

    fn scan_bool(&mut self, bl : &'a str) {
        let mut x = self.i;
        let mut new_right = self.size_left;
        let iter = self.source.char_indices().skip(self.i);
        let ik = bl.chars();
        for ((i,a),b) in iter.zip(ik) {
            if a == b {
                x += 1;
                new_right = i + a.len_utf8();
            } else {
                break;
            }
        }
        if self.j < x {
            self.j = x;
            self.size_right = new_right;
            self.token = Some(Token::Bool(bl));
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Spanned<Token<'a>, usize, LexicalError<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.token = None;
            self.scan_spaces();
            self.scan_comment_monoline();
            self.scan_comment_multiline();
            self.scan_number();
            self.scan_string_type();

            for &string in &["true", "false"] {
                self.scan_bool(string);
            }

            for &(string,tok) in &::syntax::KEYWORDS {
                self.scan_string(string,tok);
            }

            self.scan_identifier();

            for &(string,tok) in &::syntax::OPERATORS {
                self.scan_string(string,tok);
            }

            for &(string,tok) in &::syntax::TOKENS {
                self.scan_string(string,tok);
            }

            if self.size_right >= self.source.len() {
                return None;
            }
            let i = self.i;
            self.i = self.j;
            self.size_left = self.size_right;
            match self.token {
                Some(Token::IgnoreComment) | Some(Token::IgnoreWhitespace) => continue,
                Some(tok) => return Some(Ok((self.line, tok, i))),
                None => return Some(Err(LexicalError::InvalidToken(self.line, Token::OpUnexpected(self.source.chars().nth(i).unwrap_or(' ')), i))),
            }
        }
    }
}
