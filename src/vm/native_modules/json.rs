//! `import 'JSON'` — `parse` and `stringify` for JSON.
//!
//! Numbers are always parsed as `Float` (per JSON's "all numbers are
//! IEEE 754 doubles" convention; Python's stdlib and JS both work this
//! way). On the way out, `Int(n)` stringifies to `"n"` with no
//! decimal; `Float(n)` keeps a `.0` suffix when integer-valued so the
//! reader can distinguish.
//!
//! Circular references in arrays/objects are detected during
//! `stringify` (v0.8): `write_value` carries an ancestor-path set of
//! GC handles and raises a catchable `cycle` error on a repeat. A
//! non-cyclic shared subtree (DAG) still serializes fine.

use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc::{self, ArrayKind, GcRef, ObjectKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("parse",     native("parse",     Arity::Exact(1),    parse)),
        ("stringify", native("stringify", Arity::Range(1, 2), stringify)),
    ])
}

fn raise(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

// ---------------- parse ----------------

fn parse(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(raise(format!(
                "JSON.parse: expected String, got {}",
                other.type_name()
            )));
        }
    };
    let mut p = Parser::new(&s);
    p.skip_ws();
    let value = p.parse_value()?;
    p.skip_ws();
    if p.pos < p.bytes.len() {
        return Err(p.err("trailing content after JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { bytes: s.as_bytes(), pos: 0, line: 1, col: 1 }
    }

    fn err(&self, msg: &str) -> RuntimeError {
        raise(format!(
            "JSON.parse: {} at line {}, column {}",
            msg, self.line, self.col
        ))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, want: u8, label: &str) -> Result<(), RuntimeError> {
        match self.peek() {
            Some(b) if b == want => { self.advance(); Ok(()) }
            Some(b) => Err(self.err(&format!(
                "expected {label}, got '{}'", b as char
            ))),
            None => Err(self.err(&format!("expected {label}, got end of input"))),
        }
    }

    fn parse_value(&mut self) -> Result<Value, RuntimeError> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => self.parse_keyword("null", Value::Null),
            Some(b't') => self.parse_keyword("true", Value::Bool(true)),
            Some(b'f') => self.parse_keyword("false", Value::Bool(false)),
            Some(b'"') => self.parse_string().map(|s| Value::Str(s.into())),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b) if b == b'-' || b.is_ascii_digit() => self.parse_number(),
            Some(b) => Err(self.err(&format!("unexpected character '{}'", b as char))),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn parse_keyword(&mut self, kw: &str, value: Value) -> Result<Value, RuntimeError> {
        for c in kw.bytes() {
            if self.peek() != Some(c) {
                return Err(self.err(&format!("expected `{kw}`")));
            }
            self.advance();
        }
        Ok(value)
    }

    fn parse_string(&mut self) -> Result<String, RuntimeError> {
        self.expect(b'"', "`\"`")?;
        let mut out = String::new();
        loop {
            match self.advance() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => return Ok(out),
                Some(b'\\') => {
                    let esc = self.advance().ok_or_else(|| {
                        self.err("unterminated escape sequence")
                    })?;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            let cp = self.parse_hex4()?;
                            // Handle surrogate pair if high surrogate.
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // Need \uXXXX low surrogate.
                                if self.advance() != Some(b'\\')
                                    || self.advance() != Some(b'u')
                                {
                                    return Err(self.err(
                                        "expected low surrogate after high surrogate",
                                    ));
                                }
                                let lo = self.parse_hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&lo) {
                                    return Err(self.err(
                                        "invalid low surrogate value",
                                    ));
                                }
                                let combined =
                                    0x10000 + (((cp - 0xD800) << 10) | (lo - 0xDC00));
                                match char::from_u32(combined) {
                                    Some(c) => out.push(c),
                                    None => return Err(self.err(
                                        "invalid surrogate pair",
                                    )),
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                return Err(self.err(
                                    "unexpected low surrogate without high surrogate",
                                ));
                            } else {
                                match char::from_u32(cp) {
                                    Some(c) => out.push(c),
                                    None => return Err(self.err(
                                        "invalid unicode escape",
                                    )),
                                }
                            }
                        }
                        _ => return Err(self.err(&format!(
                            "invalid escape sequence \\{}", esc as char
                        ))),
                    }
                }
                Some(b) if b < 0x20 => {
                    return Err(self.err(
                        "control character in string must be escaped",
                    ));
                }
                Some(_) => {
                    // Re-include the byte in the output. Since `out`
                    // is a String, we need to push the original UTF-8
                    // byte sequence — for an ASCII byte that's just
                    // the byte; for multi-byte chars we need to
                    // backtrack and copy the whole UTF-8 sequence.
                    let byte_pos = self.pos - 1;
                    if self.bytes[byte_pos] < 0x80 {
                        out.push(self.bytes[byte_pos] as char);
                    } else {
                        // Multi-byte UTF-8: read leading byte and
                        // figure out the length, then copy bytes.
                        let lead = self.bytes[byte_pos];
                        let len = if lead & 0xE0 == 0xC0 { 2 }
                                  else if lead & 0xF0 == 0xE0 { 3 }
                                  else if lead & 0xF8 == 0xF0 { 4 }
                                  else {
                                      return Err(self.err(
                                          "invalid UTF-8 in string",
                                      ));
                                  };
                        for _ in 1..len {
                            self.advance().ok_or_else(|| {
                                self.err("truncated UTF-8 sequence")
                            })?;
                        }
                        let end = byte_pos + len;
                        match std::str::from_utf8(&self.bytes[byte_pos..end]) {
                            Ok(s) => out.push_str(s),
                            Err(_) => return Err(self.err("invalid UTF-8 in string")),
                        }
                    }
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, RuntimeError> {
        let mut acc = 0u32;
        for _ in 0..4 {
            let b = self.advance().ok_or_else(|| {
                self.err("expected 4 hex digits after \\u")
            })?;
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => return Err(self.err("expected hex digit in \\uXXXX escape")),
            };
            acc = (acc << 4) | d;
        }
        Ok(acc)
    }

    fn parse_number(&mut self) -> Result<Value, RuntimeError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance();
        }
        // Integer part: 0 alone, or 1-9 then more digits.
        match self.peek() {
            Some(b'0') => { self.advance(); }
            Some(b'1'..=b'9') => {
                self.advance();
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.advance();
                }
            }
            _ => return Err(self.err("invalid number")),
        }
        // Fractional?
        if self.peek() == Some(b'.') {
            self.advance();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected digit after `.`"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }
        // Exponent?
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.advance();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected digit in exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }
        let lex = std::str::from_utf8(&self.bytes[start..self.pos])
            .expect("ASCII number lex");
        lex.parse::<f64>()
            .map(Value::Float)
            .map_err(|_| self.err(&format!("invalid number {lex:?}")))
    }

    fn parse_array(&mut self) -> Result<Value, RuntimeError> {
        self.expect(b'[', "`[`")?;
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.advance();
            return Ok(Value::Array(gc::alloc_array(items)));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.advance(); self.skip_ws(); }
                Some(b']') => { self.advance(); break; }
                Some(b) => return Err(self.err(&format!(
                    "expected `,` or `]`, got '{}'", b as char
                ))),
                None => return Err(self.err("unterminated array")),
            }
        }
        Ok(Value::Array(gc::alloc_array(items)))
    }

    fn parse_object(&mut self) -> Result<Value, RuntimeError> {
        self.expect(b'{', "`{`")?;
        self.skip_ws();
        let mut map: IndexMap<Rc<str>, Value> = IndexMap::new();
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok(Value::Object(gc::alloc_object(map)));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':', "`:`")?;
            let value = self.parse_value()?;
            map.insert(Rc::from(key.as_str()), value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.advance(); }
                Some(b'}') => { self.advance(); break; }
                Some(b) => return Err(self.err(&format!(
                    "expected `,` or `}}`, got '{}'", b as char
                ))),
                None => return Err(self.err("unterminated object")),
            }
        }
        Ok(Value::Object(gc::alloc_object(map)))
    }
}

// ---------------- stringify ----------------

fn stringify(args: &[Value]) -> Result<Value, RuntimeError> {
    let indent = if args.len() == 2 {
        match &args[1] {
            Value::Int(n) if *n >= 0 => Some(" ".repeat(*n as usize)),
            Value::Str(s) => Some(s.to_string()),
            Value::Null => None,
            other => {
                return Err(raise(format!(
                    "JSON.stringify: indent must be Int or String, got {}",
                    other.type_name()
                )));
            }
        }
    } else {
        None
    };
    let mut out = String::new();
    // Ancestor-path sets, one per managed kind a cycle can route
    // through. Array and Object live in separate arenas, so a node is
    // identified by its handle within its own kind's set.
    let mut seen_a: Vec<GcRef<ArrayKind>> = Vec::new();
    let mut seen_o: Vec<GcRef<ObjectKind>> = Vec::new();
    write_value(&mut out, &args[0], indent.as_deref(), 0, &mut seen_a, &mut seen_o)?;
    Ok(Value::Str(out.into()))
}

/// Raise the catchable `cycle` error. Line `0` — the VM stamps the
/// `JSON.stringify` call-site line for native errors.
fn cycle_err() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Cycle, 0)
}

fn write_value(
    out: &mut String,
    v: &Value,
    indent: Option<&str>,
    depth: usize,
    seen_a: &mut Vec<GcRef<ArrayKind>>,
    seen_o: &mut Vec<GcRef<ObjectKind>>,
) -> Result<(), RuntimeError> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Float(x) => write_float(out, *x)?,
        Value::Str(s) => write_string(out, s),
        Value::Array(a) => {
            let arr = a.borrow();
            if arr.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            // Cycle guard: a node already on the ancestor path → cycle.
            if seen_a.contains(a) {
                return Err(cycle_err());
            }
            seen_a.push(*a);
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_indent(out, indent, depth + 1);
                write_value(out, item, indent, depth + 1, seen_a, seen_o)?;
            }
            write_indent(out, indent, depth);
            out.push(']');
            seen_a.pop();
        }
        Value::Object(o) => {
            let obj = o.borrow();
            if obj.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            if seen_o.contains(o) {
                return Err(cycle_err());
            }
            seen_o.push(*o);
            out.push('{');
            for (i, (k, val)) in obj.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_indent(out, indent, depth + 1);
                write_string(out, k);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, val, indent, depth + 1, seen_a, seen_o)?;
            }
            write_indent(out, indent, depth);
            out.push('}');
            seen_o.pop();
        }
        // Non-serializable value types — raise so the caller can `try`.
        Value::Function(_)
        | Value::NativeFn(_)
        | Value::Range(_)
        | Value::Iter(_)
        | Value::Map(_)
        | Value::Set(_) => {
            return Err(raise(format!(
                "JSON.stringify: cannot serialize {}",
                v.type_name()
            )));
        }
    }
    Ok(())
}

fn write_indent(out: &mut String, indent: Option<&str>, depth: usize) {
    if let Some(unit) = indent {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(unit);
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_float(out: &mut String, x: f64) -> Result<(), RuntimeError> {
    if !x.is_finite() {
        return Err(raise(format!(
            "JSON.stringify: cannot serialize {} (JSON has no Infinity or NaN)",
            x
        )));
    }
    let s = x.to_string();
    // Ensure integer-valued floats keep a `.0` suffix so the reader
    // can tell them apart from JSON ints (which we always Float-parse,
    // but a downstream consumer may care).
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        out.push_str(&s);
        out.push_str(".0");
    } else {
        out.push_str(&s);
    }
    Ok(())
}
