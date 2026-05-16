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
