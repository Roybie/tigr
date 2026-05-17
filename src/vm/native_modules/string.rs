//! `import '_NativeString'` — Rust string primitives.
//!
//! Backend for `stdlib/String.tg`. Pure-tigr versions of these would
//! be O(n) per character (every `s[i]` walks UTF-8 from the start),
//! so we expose Rust implementations that are linear over bytes.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc;
use crate::vm::stdlib::int_to_radix;
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        ("split",       native("split",       Arity::Exact(2), s_split)),
        ("replace",     native("replace",     Arity::Exact(3), s_replace)),
        ("contains",    native("contains",    Arity::Exact(2), s_contains)),
        ("index_of",    native("index_of",    Arity::Exact(2), s_index_of)),
        ("lower",       native("lower",       Arity::Exact(1), s_lower)),
        ("upper",       native("upper",       Arity::Exact(1), s_upper)),
        ("starts_with", native("starts_with", Arity::Exact(2), s_starts_with)),
        ("ends_with",   native("ends_with",   Arity::Exact(2), s_ends_with)),
        ("trim",        native("trim",        Arity::Exact(1), s_trim)),
        ("trim_start",  native("trim_start",  Arity::Exact(1), s_trim_start)),
        ("trim_end",    native("trim_end",    Arity::Exact(1), s_trim_end)),
        ("repeat",      native("repeat",      Arity::Exact(2), s_repeat)),
        ("chars",       native("chars",       Arity::Exact(1), s_chars)),
        ("format",      native("format",      Arity::Exact(2), s_format)),

        // v0.13 — "String II" targeted text helpers.
        ("words",         native("words",         Arity::Exact(1), s_words)),
        ("lines",         native("lines",         Arity::Exact(1), s_lines)),
        ("split_any",     native("split_any",     Arity::Exact(2), s_split_any)),
        ("find_all",      native("find_all",      Arity::Exact(2), s_find_all)),
        ("count",         native("count",         Arity::Exact(2), s_count)),
        ("replace_first", native("replace_first", Arity::Exact(3), s_replace_first)),
        ("matches_glob",  native("matches_glob",  Arity::Exact(2), s_matches_glob)),
        ("reverse",       native("reverse",       Arity::Exact(1), s_reverse)),
        ("strip_prefix",  native("strip_prefix",  Arity::Exact(2), s_strip_prefix)),
        ("strip_suffix",  native("strip_suffix",  Arity::Exact(2), s_strip_suffix)),
        ("capitalize",    native("capitalize",    Arity::Exact(1), s_capitalize)),
    ])
}

fn as_str<'a>(v: &'a Value, label: &str) -> Result<&'a str, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(format!(
                "String.{label}: expected String, got {}",
                other.type_name()
            ).into())),
            0,
        )),
    }
}

fn s_split(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "split")?;
    let sep = as_str(&args[1], "split")?;
    let parts: Vec<Value> = if sep.is_empty() {
        // Empty separator → one-char strings (matches `chars`).
        s.chars().map(|c| Value::Str(c.to_string().into())).collect()
    } else {
        s.split(sep).map(|p| Value::Str(p.into())).collect()
    };
    Ok(Value::Array(gc::alloc_array(parts)))
}

fn s_replace(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "replace")?;
    let from = as_str(&args[1], "replace")?;
    let to = as_str(&args[2], "replace")?;
    if from.is_empty() {
        // Match Rust's behavior would be to insert `to` between every
        // char; that's surprising. Return the source unchanged.
        return Ok(Value::Str(s.into()));
    }
    Ok(Value::Str(s.replace(from, to).into()))
}

fn s_contains(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "contains")?;
    let needle = as_str(&args[1], "contains")?;
    Ok(Value::Bool(s.contains(needle)))
}

/// Byte index of the first occurrence, or -1 if not found.
/// (Bytes, not chars — fine for ASCII; consistent with `len`-via-`#`
/// where `#'café' == 5` because tigr counts bytes.)
fn s_index_of(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "index_of")?;
    let needle = as_str(&args[1], "index_of")?;
    match s.find(needle) {
        Some(i) => Ok(Value::Int(i as i64)),
        None => Ok(Value::Int(-1)),
    }
}

fn s_lower(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "lower")?;
    Ok(Value::Str(s.to_lowercase().into()))
}

fn s_upper(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "upper")?;
    Ok(Value::Str(s.to_uppercase().into()))
}

fn s_starts_with(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "starts_with")?;
    let prefix = as_str(&args[1], "starts_with")?;
    Ok(Value::Bool(s.starts_with(prefix)))
}

fn s_ends_with(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "ends_with")?;
    let suffix = as_str(&args[1], "ends_with")?;
    Ok(Value::Bool(s.ends_with(suffix)))
}

fn s_trim(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "trim")?;
    Ok(Value::Str(s.trim().into()))
}

fn s_trim_start(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "trim_start")?;
    Ok(Value::Str(s.trim_start().into()))
}

fn s_trim_end(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "trim_end")?;
    Ok(Value::Str(s.trim_end().into()))
}

fn s_repeat(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "repeat")?;
    let n = match &args[1] {
        Value::Int(n) if *n >= 0 => *n as usize,
        Value::Int(_) => return Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(
                "String.repeat: negative count".into())),
            0,
        )),
        other => return Err(RuntimeError::new(
            RuntimeErrorKind::Raised(Value::Str(format!(
                "String.repeat: count must be Int, got {}", other.type_name()
            ).into())),
            0,
        )),
    };
    Ok(Value::Str(s.repeat(n).into()))
}

fn s_chars(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "chars")?;
    let parts: Vec<Value> = s
        .chars()
        .map(|c| Value::Str(c.to_string().into()))
        .collect();
    Ok(Value::Array(gc::alloc_array(parts)))
}

// --- v0.13 "String II" — targeted text helpers ----------------------

/// Build a `Value::Array` of strings from an iterator of `&str`.
fn str_array<'a>(it: impl Iterator<Item = &'a str>) -> Value {
    Value::Array(gc::alloc_array(it.map(|p| Value::Str(p.into())).collect()))
}

/// Split on runs of whitespace, dropping empty leading/trailing/inner
/// fields — unlike `split`, which only takes a literal separator.
fn s_words(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "words")?;
    Ok(str_array(s.split_whitespace()))
}

/// Split into lines on `\n` / `\r\n`; a trailing newline does not
/// produce a final empty line (matches Rust's `str::lines`).
fn s_lines(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "lines")?;
    Ok(str_array(s.lines()))
}

/// Split on *any* char present in `delims`. An empty `delims` yields
/// the whole string unsplit. Adjacent delimiters yield empty fields,
/// consistent with `split`.
fn s_split_any(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "split_any")?;
    let delims = as_str(&args[1], "split_any")?;
    if delims.is_empty() {
        return Ok(str_array(std::iter::once(s)));
    }
    let set: Vec<char> = delims.chars().collect();
    Ok(str_array(s.split(|c| set.contains(&c))))
}

/// Byte offsets of every non-overlapping occurrence of `needle`. An
/// empty `needle` yields an empty array. (Bytes, not chars — matches
/// `index_of`.)
fn s_find_all(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "find_all")?;
    let needle = as_str(&args[1], "find_all")?;
    let mut out: Vec<Value> = Vec::new();
    if !needle.is_empty() {
        let mut start = 0;
        while let Some(i) = s[start..].find(needle) {
            let abs = start + i;
            out.push(Value::Int(abs as i64));
            start = abs + needle.len();
        }
    }
    Ok(Value::Array(gc::alloc_array(out)))
}

/// Count non-overlapping occurrences of `needle`. Empty `needle` → 0.
fn s_count(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "count")?;
    let needle = as_str(&args[1], "count")?;
    let n = if needle.is_empty() { 0 } else { s.matches(needle).count() };
    Ok(Value::Int(n as i64))
}

/// Replace only the first occurrence of `from`. An empty `from`
/// returns the source unchanged (mirrors `replace`).
fn s_replace_first(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "replace_first")?;
    let from = as_str(&args[1], "replace_first")?;
    let to = as_str(&args[2], "replace_first")?;
    if from.is_empty() {
        return Ok(Value::Str(s.into()));
    }
    Ok(Value::Str(s.replacen(from, to, 1).into()))
}

/// Reverse by Unicode scalar values (not bytes).
fn s_reverse(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "reverse")?;
    Ok(Value::Str(s.chars().rev().collect::<String>().into()))
}

/// `s` without `prefix` if it starts with it, else `s` unchanged.
fn s_strip_prefix(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "strip_prefix")?;
    let prefix = as_str(&args[1], "strip_prefix")?;
    Ok(Value::Str(s.strip_prefix(prefix).unwrap_or(s).into()))
}

/// `s` without `suffix` if it ends with it, else `s` unchanged.
fn s_strip_suffix(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "strip_suffix")?;
    let suffix = as_str(&args[1], "strip_suffix")?;
    Ok(Value::Str(s.strip_suffix(suffix).unwrap_or(s).into()))
}

/// Uppercase the first char; the rest of the string is left as-is.
fn s_capitalize(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "capitalize")?;
    let mut it = s.chars();
    let out = match it.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + it.as_str(),
    };
    Ok(Value::Str(out.into()))
}

// --- glob matching --------------------------------------------------
//
// `matches_glob(s, pattern)` is a whole-string shell-style match — a
// deliberately small slice of pattern-as-data, *not* a regex engine:
//
//   *        any run of chars, including empty
//   ?        exactly one char
//   [abc]    one char from a set; ranges `a-z`; `[!...]` negates
//   \* \? \[ \\   escape a metacharacter
//
// Matching runs the classic linear two-pointer scan with a single
// backtrack point for the most recent `*` — O(n·m) worst case, no
// recursion, no catastrophic backtracking.

enum GlobTok {
    Lit(char),
    AnyOne,
    Star,
    Class { negated: bool, ranges: Vec<(char, char)> },
}

/// Parse a glob pattern into tokens, or raise on a malformed pattern.
fn glob_parse(pattern: &str) -> Result<Vec<GlobTok>, RuntimeError> {
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < n {
        match chars[i] {
            '*' => {
                toks.push(GlobTok::Star);
                i += 1;
            }
            '?' => {
                toks.push(GlobTok::AnyOne);
                i += 1;
            }
            '\\' => {
                if i + 1 >= n {
                    return Err(fmt_err("matches_glob", "dangling '\\' in pattern"));
                }
                toks.push(GlobTok::Lit(chars[i + 1]));
                i += 2;
            }
            '[' => {
                let mut j = i + 1;
                let negated = j < n && (chars[j] == '!' || chars[j] == '^');
                if negated {
                    j += 1;
                }
                let mut ranges: Vec<(char, char)> = Vec::new();
                let class_start = j;
                loop {
                    if j >= n {
                        return Err(fmt_err(
                            "matches_glob",
                            "unterminated '[' in pattern",
                        ));
                    }
                    // A `]` is a literal only as the very first class
                    // member; otherwise it closes the class.
                    if chars[j] == ']' && j > class_start {
                        j += 1;
                        break;
                    }
                    // `a-z` range: a `-` flanked by two chars.
                    if j + 2 < n && chars[j + 1] == '-' && chars[j + 2] != ']' {
                        ranges.push((chars[j], chars[j + 2]));
                        j += 3;
                    } else {
                        ranges.push((chars[j], chars[j]));
                        j += 1;
                    }
                }
                toks.push(GlobTok::Class { negated, ranges });
                i = j;
            }
            c => {
                toks.push(GlobTok::Lit(c));
                i += 1;
            }
        }
    }
    Ok(toks)
}

/// Does a single non-`Star` token match exactly one char `c`?
fn glob_tok_matches(tok: &GlobTok, c: char) -> bool {
    match tok {
        GlobTok::Lit(l) => *l == c,
        GlobTok::AnyOne => true,
        GlobTok::Class { negated, ranges } => {
            let hit = ranges.iter().any(|(lo, hi)| *lo <= c && c <= *hi);
            hit != *negated
        }
        GlobTok::Star => unreachable!("Star handled by the scanner"),
    }
}

fn s_matches_glob(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = as_str(&args[0], "matches_glob")?;
    let pattern = as_str(&args[1], "matches_glob")?;
    let toks = glob_parse(pattern)?;
    let text: Vec<char> = s.chars().collect();

    let mut ti = 0;
    let mut pi = 0;
    let mut star: Option<(usize, usize)> = None; // (token index, text index)
    while ti < text.len() {
        if pi < toks.len() && matches!(toks[pi], GlobTok::Star) {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < toks.len() && glob_tok_matches(&toks[pi], text[ti]) {
            ti += 1;
            pi += 1;
        } else if let Some((sp, st)) = star {
            // Backtrack: let the last `*` swallow one more char.
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return Ok(Value::Bool(false));
        }
    }
    while pi < toks.len() && matches!(toks[pi], GlobTok::Star) {
        pi += 1;
    }
    Ok(Value::Bool(pi == toks.len()))
}

// --- String.format: a printf-flavoured per-value formatter ----------
//
// `format(value, spec)` renders one value through a small spec
// mini-language:
//
//   spec := [[fill]align][sign]['#'][width][','][.precision][type]
//
// where `align` is `<` `>` `^`, `sign` is `+`, `#` is the alternate
// form (0x/0o/0b prefix), a bare leading `0` in `width` means zero-pad,
// `,` adds thousands grouping, and `type` is one of `s d f e E x X b o`.
// `String.printf` (in `stdlib/String.tg`) reuses this exact language.

/// Raise a `String.<label>: <msg>` runtime error (matches `as_str`).
fn fmt_err(label: &str, msg: &str) -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::Raised(Value::Str(
            format!("String.{label}: {msg}").into(),
        )),
        0,
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Right,
    Center,
}

struct Spec {
    fill: char,
    align: Option<Align>,
    sign_plus: bool,
    alternate: bool,
    zero_pad: bool,
    width: usize,
    grouping: bool,
    precision: Option<usize>,
    ty: Option<char>,
}

/// Parse a spec string left-to-right; an unconsumed tail is an error.
fn parse_spec(spec: &str, label: &str) -> Result<Spec, RuntimeError> {
    let chars: Vec<char> = spec.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut s = Spec {
        fill: ' ',
        align: None,
        sign_plus: false,
        alternate: false,
        zero_pad: false,
        width: 0,
        grouping: false,
        precision: None,
        ty: None,
    };
    let to_align = |c: char| match c {
        '<' => Some(Align::Left),
        '>' => Some(Align::Right),
        '^' => Some(Align::Center),
        _ => None,
    };

    // [fill]align — a fill char is only a fill char when an align
    // follows it; a bare align char applies the default space fill.
    if n >= 2 && to_align(chars[1]).is_some() {
        s.fill = chars[0];
        s.align = to_align(chars[1]);
        i = 2;
    } else if n >= 1 && to_align(chars[0]).is_some() {
        s.align = to_align(chars[0]);
        i = 1;
    }
    // sign
    if i < n && chars[i] == '+' {
        s.sign_plus = true;
        i += 1;
    }
    // alternate form
    if i < n && chars[i] == '#' {
        s.alternate = true;
        i += 1;
    }
    // width — a bare leading `0` (no explicit fill+align) is zero-pad
    if i < n && chars[i] == '0' && s.align.is_none() {
        s.zero_pad = true;
    }
    let mut digits = String::new();
    while i < n && chars[i].is_ascii_digit() {
        digits.push(chars[i]);
        i += 1;
    }
    if !digits.is_empty() {
        s.width = digits.parse().unwrap_or(0);
    }
    // thousands grouping
    if i < n && chars[i] == ',' {
        s.grouping = true;
        i += 1;
    }
    // .precision
    if i < n && chars[i] == '.' {
        i += 1;
        let mut prec = String::new();
        while i < n && chars[i].is_ascii_digit() {
            prec.push(chars[i]);
            i += 1;
        }
        s.precision = Some(prec.parse().unwrap_or(0));
    }
    // type code
    if i < n && matches!(chars[i], 's' | 'd' | 'f' | 'e' | 'E' | 'x' | 'X' | 'b' | 'o') {
        s.ty = Some(chars[i]);
        i += 1;
    }
    if i != n {
        return Err(fmt_err(label, &format!("invalid format spec \"{spec}\"")));
    }
    Ok(s)
}

/// Insert `,` every three digits from the right of a run of digits.
fn group_thousands(digits: &str) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len + len / 3);
    for (idx, c) in chars.iter().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

/// Group the integer part of a numeric string that may carry a `.`.
fn group_decimal(s: &str) -> String {
    match s.find('.') {
        Some(d) => format!("{}{}", group_thousands(&s[..d]), &s[d..]),
        None => group_thousands(s),
    }
}

/// Pad `body` to `s.width` with `s.fill`, honouring align (or `default`).
fn pad_align(body: &str, s: &Spec, default: Align) -> String {
    let len = body.chars().count();
    if len >= s.width {
        return body.to_string();
    }
    let pad = s.width - len;
    let fill = |k: usize| std::iter::repeat_n(s.fill, k).collect::<String>();
    match s.align.unwrap_or(default) {
        Align::Left => format!("{body}{}", fill(pad)),
        Align::Right => format!("{}{body}", fill(pad)),
        Align::Center => {
            let left = pad / 2;
            format!("{}{body}{}", fill(left), fill(pad - left))
        }
    }
}

/// Assemble a numeric result: `sign` + `#`-prefix + digits, with either
/// zero-padding (between the prefix and the digits) or width/align.
fn assemble_numeric(sign: &str, prefix: &str, digits: &str, s: &Spec) -> String {
    if s.zero_pad {
        let fixed = sign.chars().count() + prefix.chars().count() + digits.chars().count();
        let zeros = s.width.saturating_sub(fixed);
        format!("{sign}{prefix}{}{digits}", "0".repeat(zeros))
    } else {
        pad_align(&format!("{sign}{prefix}{digits}"), s, Align::Right)
    }
}

/// Coerce a value to `f64` for the float/exponential types.
fn as_number(value: &Value, ty: char) -> Result<f64, RuntimeError> {
    match value {
        Value::Float(x) => Ok(*x),
        Value::Int(n) => Ok(*n as f64),
        other => Err(fmt_err(
            "format",
            &format!("type '{ty}' expects a number, got {}", other.type_name()),
        )),
    }
}

/// Coerce a value to `i64` for the integer/radix types.
fn as_integer(value: &Value, ty: char) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(n) => Ok(*n),
        Value::Float(x) if x.is_finite() && x.fract() == 0.0 => Ok(*x as i64),
        Value::Float(x) => Err(fmt_err(
            "format",
            &format!("type '{ty}' cannot format the non-integral float {x}"),
        )),
        other => Err(fmt_err(
            "format",
            &format!("type '{ty}' expects a number, got {}", other.type_name()),
        )),
    }
}

fn render_int(value: &Value, s: &Spec, ty: char, radix: u32, upper: bool) -> Result<String, RuntimeError> {
    let n = as_integer(value, ty)?;
    let mut digits = int_to_radix(n.unsigned_abs(), radix);
    if upper {
        digits = digits.to_uppercase();
    }
    if s.grouping && radix == 10 {
        digits = group_thousands(&digits);
    }
    let sign = if n < 0 {
        "-"
    } else if s.sign_plus {
        "+"
    } else {
        ""
    };
    let prefix = if s.alternate {
        match radix {
            16 => "0x",
            8 => "0o",
            2 => "0b",
            _ => "",
        }
    } else {
        ""
    };
    Ok(assemble_numeric(sign, prefix, &digits, s))
}

fn render_float(value: &Value, s: &Spec, ty: char) -> Result<String, RuntimeError> {
    let x = as_number(value, ty)?;
    let prec = s.precision.unwrap_or(6);
    let mut digits = format!("{:.prec$}", x.abs(), prec = prec);
    if s.grouping {
        digits = group_decimal(&digits);
    }
    let sign = if x < 0.0 {
        "-"
    } else if s.sign_plus {
        "+"
    } else {
        ""
    };
    Ok(assemble_numeric(sign, "", &digits, s))
}

fn render_exp(value: &Value, s: &Spec, ty: char, upper: bool) -> Result<String, RuntimeError> {
    let x = as_number(value, ty)?;
    let prec = s.precision.unwrap_or(6);
    let mut digits = format!("{:.prec$e}", x.abs(), prec = prec);
    if upper {
        digits = digits.replace('e', "E");
    }
    let sign = if x < 0.0 {
        "-"
    } else if s.sign_plus {
        "+"
    } else {
        ""
    };
    Ok(assemble_numeric(sign, "", &digits, s))
}

fn render_string_value(value: &Value, s: &Spec) -> Result<String, RuntimeError> {
    let text = match value {
        Value::Str(t) => t.as_ref(),
        other => {
            return Err(fmt_err(
                "format",
                &format!("type 's' expects a String, got {}", other.type_name()),
            ))
        }
    };
    let body: String = match s.precision {
        Some(p) => text.chars().take(p).collect(),
        None => text.to_string(),
    };
    Ok(pad_align(&body, s, Align::Left))
}

/// No type code — render by the value's natural kind, honouring the
/// rest of the spec. Never raises on a type mismatch.
fn render_default(value: &Value, s: &Spec) -> Result<String, RuntimeError> {
    match value {
        Value::Int(_) => render_int(value, s, 'd', 10, false),
        Value::Float(_) => {
            if s.precision.is_some() {
                render_float(value, s, 'f')
            } else {
                // Natural Display ("3.0", "1.5", "-2.25"); keep grouping
                // and the optional `+` sign working.
                let disp = format!("{value}");
                let neg = disp.starts_with('-');
                let mut digits = if neg { disp[1..].to_string() } else { disp };
                if s.grouping {
                    digits = group_decimal(&digits);
                }
                let sign = if neg {
                    "-"
                } else if s.sign_plus {
                    "+"
                } else {
                    ""
                };
                Ok(assemble_numeric(sign, "", &digits, s))
            }
        }
        Value::Str(_) => render_string_value(value, s),
        other => {
            let disp = format!("{other}");
            let body: String = match s.precision {
                Some(p) => disp.chars().take(p).collect(),
                None => disp,
            };
            Ok(pad_align(&body, s, Align::Left))
        }
    }
}

fn s_format(args: &[Value]) -> Result<Value, RuntimeError> {
    let spec_str = as_str(&args[1], "format")?;
    let spec = parse_spec(spec_str, "format")?;
    let out = match spec.ty {
        None => render_default(&args[0], &spec)?,
        Some('s') => render_string_value(&args[0], &spec)?,
        Some('d') => render_int(&args[0], &spec, 'd', 10, false)?,
        Some('x') => render_int(&args[0], &spec, 'x', 16, false)?,
        Some('X') => render_int(&args[0], &spec, 'X', 16, true)?,
        Some('b') => render_int(&args[0], &spec, 'b', 2, false)?,
        Some('o') => render_int(&args[0], &spec, 'o', 8, false)?,
        Some('f') => render_float(&args[0], &spec, 'f')?,
        Some('e') => render_exp(&args[0], &spec, 'e', false)?,
        Some('E') => render_exp(&args[0], &spec, 'E', true)?,
        Some(c) => return Err(fmt_err("format", &format!("unknown type code '{c}'"))),
    };
    Ok(Value::Str(out.into()))
}

// --- tests ----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn call(f: fn(&[Value]) -> Result<Value, RuntimeError>, args: &[Value]) -> Value {
        f(args).expect("native call should succeed")
    }

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    fn strs(v: &Value) -> Vec<String> {
        match v {
            Value::Array(a) => a
                .borrow()
                .iter()
                .map(|e| match e {
                    Value::Str(t) => t.to_string(),
                    other => panic!("expected Str, got {}", other.type_name()),
                })
                .collect(),
            other => panic!("expected Array, got {}", other.type_name()),
        }
    }

    #[test]
    fn words_splits_on_whitespace_runs() {
        assert_eq!(
            strs(&call(s_words, &[s("  foo   bar\t baz\n")])),
            ["foo", "bar", "baz"]
        );
        assert!(strs(&call(s_words, &[s("   ")])).is_empty());
    }

    #[test]
    fn lines_handles_crlf_and_no_trailing_empty() {
        assert_eq!(
            strs(&call(s_lines, &[s("a\nb\r\nc")])),
            ["a", "b", "c"]
        );
        assert_eq!(strs(&call(s_lines, &[s("a\n")])), ["a"]);
    }

    #[test]
    fn split_any_splits_on_each_delim() {
        assert_eq!(
            strs(&call(s_split_any, &[s("a,b;c d"), s(",; ")])),
            ["a", "b", "c", "d"]
        );
        // empty delims → whole string unsplit
        assert_eq!(strs(&call(s_split_any, &[s("abc"), s("")])), ["abc"]);
    }

    #[test]
    fn find_all_reports_byte_offsets() {
        let v = call(s_find_all, &[s("café! café!"), s("café")]);
        match v {
            Value::Array(a) => {
                let got: Vec<i64> = a
                    .borrow()
                    .iter()
                    .map(|e| match e {
                        Value::Int(n) => *n,
                        _ => panic!("expected Int"),
                    })
                    .collect();
                // 'é' is two bytes, so the second match is at byte 7.
                assert_eq!(got, [0, 7]);
            }
            _ => panic!("expected Array"),
        }
        // empty needle → no matches
        assert!(strs(&call(s_find_all, &[s("abc"), s("")])).is_empty());
        // overlap is not double-counted
        let v = call(s_find_all, &[s("aaaa"), s("aa")]);
        if let Value::Array(a) = v {
            assert_eq!(a.borrow().len(), 2);
        }
    }

    #[test]
    fn count_is_non_overlapping() {
        assert!(matches!(call(s_count, &[s("aaaa"), s("aa")]), Value::Int(2)));
        assert!(matches!(call(s_count, &[s("abc"), s("")]), Value::Int(0)));
    }

    #[test]
    fn replace_first_replaces_one() {
        assert_eq!(
            call(s_replace_first, &[s("a-a-a"), s("a"), s("X")]),
            s("X-a-a")
        );
        // empty `from` → unchanged
        assert_eq!(call(s_replace_first, &[s("ab"), s(""), s("X")]), s("ab"));
    }

    #[test]
    fn reverse_is_char_wise() {
        assert_eq!(call(s_reverse, &[s("abç")]), s("çba"));
    }

    #[test]
    fn strip_prefix_suffix() {
        assert_eq!(call(s_strip_prefix, &[s("foobar"), s("foo")]), s("bar"));
        assert_eq!(call(s_strip_prefix, &[s("foobar"), s("xyz")]), s("foobar"));
        assert_eq!(call(s_strip_suffix, &[s("foobar"), s("bar")]), s("foo"));
        assert_eq!(call(s_strip_suffix, &[s("foobar"), s("xyz")]), s("foobar"));
    }

    #[test]
    fn capitalize_uppercases_first_char() {
        assert_eq!(call(s_capitalize, &[s("hello")]), s("Hello"));
        assert_eq!(call(s_capitalize, &[s("")]), s(""));
        assert_eq!(call(s_capitalize, &[s("ABC")]), s("ABC"));
    }

    fn glob(text: &str, pat: &str) -> bool {
        match call(s_matches_glob, &[s(text), s(pat)]) {
            Value::Bool(b) => b,
            _ => panic!("expected Bool"),
        }
    }

    #[test]
    fn glob_literals_and_wildcards() {
        assert!(glob("readme.txt", "*.txt"));
        assert!(!glob("readme.md", "*.txt"));
        assert!(glob("abc", "a?c"));
        assert!(!glob("ac", "a?c"));
        assert!(glob("anything", "*"));
        assert!(glob("", "*"));
        assert!(glob("", ""));
        assert!(!glob("x", ""));
        assert!(glob("a.b.c", "*.*"));
    }

    #[test]
    fn glob_classes_ranges_and_negation() {
        assert!(glob("cat", "[cb]at"));
        assert!(!glob("rat", "[cb]at"));
        assert!(glob("file7", "file[0-9]"));
        assert!(!glob("fileX", "file[0-9]"));
        assert!(glob("xat", "[!cb]at"));
        assert!(!glob("cat", "[!cb]at"));
    }

    #[test]
    fn glob_escapes_metacharacters() {
        assert!(glob("a*b", "a\\*b"));
        assert!(!glob("axb", "a\\*b"));
        assert!(glob("[x]", "\\[x\\]"));
    }

    #[test]
    fn glob_rejects_malformed_patterns() {
        assert!(s_matches_glob(&[s("x"), s("[abc")]).is_err());
        assert!(s_matches_glob(&[s("x"), s("ab\\")]).is_err());
    }

    #[test]
    fn glob_star_backtracks() {
        // The classic case a naive matcher gets wrong.
        assert!(glob("abcabd", "a*bd"));
        assert!(glob("aXbXcXd", "a*b*c*d"));
        assert!(!glob("aXbXcX", "a*b*c*d"));
    }
}
