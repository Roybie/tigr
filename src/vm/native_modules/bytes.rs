//! `import 'Bytes'` — the mutable byte-buffer type and its operations.
//!
//! `Bytes` is a `Value` in its own right (a GC-managed `Vec<u8>`):
//! indexable, `#`-length, `for`-iterable, sliceable with `b[start:]`,
//! and concatenable with `+` / `+=`. This module supplies everything
//! the operators cannot: construction, `String`/`[Int]`/hex/base64
//! conversion, in-place growth, and a named family of fixed-width
//! integer readers and writers for binary-protocol work.
//!
//! Reading multi-byte integers uses self-documenting names —
//! `read_u32_be(buf, offset)`, `write_i16_le(buf, offset, value)` — so a
//! call site states its width and endianness without a magic argument.

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::gc::{self, ArrayKind, BytesKind, GcRef};
use crate::vm::value::{Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        // -- construction --
        ("new",         native("new",         Arity::Range(1, 2), b_new)),
        ("from_array",  native("from_array",  Arity::Exact(1), b_from_array)),
        ("from_string", native("from_string", Arity::Exact(1), b_from_string)),
        ("from_hex",    native("from_hex",    Arity::Exact(1), b_from_hex)),
        ("from_base64", native("from_base64", Arity::Exact(1), b_from_base64)),
        // -- conversion --
        ("to_array",    native("to_array",    Arity::Exact(1), b_to_array)),
        ("to_string",   native("to_string",   Arity::Exact(1), b_to_string)),
        ("to_hex",      native("to_hex",      Arity::Exact(1), b_to_hex)),
        ("to_base64",   native("to_base64",   Arity::Exact(1), b_to_base64)),
        // -- buffer ops --
        ("push",        native("push",        Arity::Exact(2), b_push)),
        ("extend",      native("extend",      Arity::Exact(2), b_extend)),
        ("slice",       native("slice",       Arity::Exact(3), b_slice)),
        ("concat",      native("concat",      Arity::Exact(2), b_concat)),
        // -- integer pack/unpack (named family) --
        ("read_u8",     native("read_u8",     Arity::Exact(2), read_u8)),
        ("read_i8",     native("read_i8",     Arity::Exact(2), read_i8)),
        ("read_u16_be", native("read_u16_be", Arity::Exact(2), read_u16_be)),
        ("read_u16_le", native("read_u16_le", Arity::Exact(2), read_u16_le)),
        ("read_i16_be", native("read_i16_be", Arity::Exact(2), read_i16_be)),
        ("read_i16_le", native("read_i16_le", Arity::Exact(2), read_i16_le)),
        ("read_u32_be", native("read_u32_be", Arity::Exact(2), read_u32_be)),
        ("read_u32_le", native("read_u32_le", Arity::Exact(2), read_u32_le)),
        ("read_i32_be", native("read_i32_be", Arity::Exact(2), read_i32_be)),
        ("read_i32_le", native("read_i32_le", Arity::Exact(2), read_i32_le)),
        ("read_u64_be", native("read_u64_be", Arity::Exact(2), read_u64_be)),
        ("read_u64_le", native("read_u64_le", Arity::Exact(2), read_u64_le)),
        ("read_i64_be", native("read_i64_be", Arity::Exact(2), read_i64_be)),
        ("read_i64_le", native("read_i64_le", Arity::Exact(2), read_i64_le)),
        ("write_u8",    native("write_u8",    Arity::Exact(3), write_u8)),
        ("write_i8",    native("write_i8",    Arity::Exact(3), write_i8)),
        ("write_u16_be", native("write_u16_be", Arity::Exact(3), write_u16_be)),
        ("write_u16_le", native("write_u16_le", Arity::Exact(3), write_u16_le)),
        ("write_i16_be", native("write_i16_be", Arity::Exact(3), write_i16_be)),
        ("write_i16_le", native("write_i16_le", Arity::Exact(3), write_i16_le)),
        ("write_u32_be", native("write_u32_be", Arity::Exact(3), write_u32_be)),
        ("write_u32_le", native("write_u32_le", Arity::Exact(3), write_u32_le)),
        ("write_i32_be", native("write_i32_be", Arity::Exact(3), write_i32_be)),
        ("write_i32_le", native("write_i32_le", Arity::Exact(3), write_i32_le)),
        ("write_u64_be", native("write_u64_be", Arity::Exact(3), write_u64_be)),
        ("write_u64_le", native("write_u64_le", Arity::Exact(3), write_u64_le)),
        ("write_i64_be", native("write_i64_be", Arity::Exact(3), write_i64_be)),
        ("write_i64_le", native("write_i64_le", Arity::Exact(3), write_i64_le)),
    ])
}

// ---------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------

/// A catchable, string-valued error. The VM backfills the call line.
fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

/// A catchable structured decode error — `${kind: 'decode', message}`.
/// Mirrors the shape of a reified built-in error so `catch` code (and
/// `Test.assert_raises(..., 'decode')`) can dispatch on `.kind`.
fn decode_err(msg: String) -> RuntimeError {
    let obj = super::object(&[
        ("kind", Value::Str("decode".into())),
        ("message", Value::Str(msg.into())),
    ]);
    RuntimeError::new(RuntimeErrorKind::Raised(obj), 0)
}

fn expect_bytes(v: &Value, label: &str) -> Result<GcRef<BytesKind>, RuntimeError> {
    match v {
        Value::Bytes(b) => Ok(*b),
        other => Err(err(format!(
            "Bytes.{label}: expected Bytes, got {}",
            other.type_name()
        ))),
    }
}

fn expect_array(v: &Value, label: &str) -> Result<GcRef<ArrayKind>, RuntimeError> {
    match v {
        Value::Array(a) => Ok(*a),
        other => Err(err(format!(
            "Bytes.{label}: expected Array, got {}",
            other.type_name()
        ))),
    }
}

fn expect_str<'a>(v: &'a Value, label: &str) -> Result<&'a str, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(err(format!(
            "Bytes.{label}: expected String, got {}",
            other.type_name()
        ))),
    }
}

fn expect_int(v: &Value, label: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(err(format!(
            "Bytes.{label}: expected Int, got {}",
            other.type_name()
        ))),
    }
}

/// An `Int` argument constrained to a single byte (0..=255).
fn expect_byte(v: &Value, label: &str) -> Result<u8, RuntimeError> {
    match expect_int(v, label)? {
        n if (0..=255).contains(&n) => Ok(n as u8),
        n => Err(err(format!(
            "Bytes.{label}: byte value {n} out of range 0..=255"
        ))),
    }
}

/// Resolve a possibly-negative index against `len` and clamp to `[0, len]`.
fn resolve_clamped(idx: i64, len: usize) -> usize {
    let real = if idx < 0 { idx + len as i64 } else { idx };
    real.clamp(0, len as i64) as usize
}

// ---------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------

/// `new(n)` / `new(n, fill)` — a buffer of `n` bytes, zero- or
/// `fill`-filled.
fn b_new(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = expect_int(&args[0], "new")?;
    if n < 0 {
        return Err(err(format!("Bytes.new: length {n} is negative")));
    }
    let fill = if args.len() == 2 {
        expect_byte(&args[1], "new")?
    } else {
        0
    };
    Ok(Value::Bytes(gc::alloc_bytes(vec![fill; n as usize])))
}

/// `from_array(arr)` — pack an `[Int]` (each 0..=255) into a buffer.
fn b_from_array(args: &[Value]) -> Result<Value, RuntimeError> {
    let arr = expect_array(&args[0], "from_array")?;
    let src = arr.borrow();
    let mut out = Vec::with_capacity(src.len());
    for (i, v) in src.iter().enumerate() {
        match v {
            Value::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
            Value::Int(n) => return Err(err(format!(
                "Bytes.from_array: element {i} = {n} out of range 0..=255"
            ))),
            other => return Err(err(format!(
                "Bytes.from_array: element {i} is {}, expected Int",
                other.type_name()
            ))),
        }
    }
    Ok(Value::Bytes(gc::alloc_bytes(out)))
}

/// `from_string(s)` — the UTF-8 encoding of `s`. Always succeeds.
fn b_from_string(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = expect_str(&args[0], "from_string")?;
    Ok(Value::Bytes(gc::alloc_bytes(s.as_bytes().to_vec())))
}

/// `from_hex(s)` — decode a hex string. ASCII whitespace is ignored.
fn b_from_hex(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = expect_str(&args[0], "from_hex")?;
    let bytes = hex_decode(s).map_err(|e| decode_err(format!("Bytes.from_hex: {e}")))?;
    Ok(Value::Bytes(gc::alloc_bytes(bytes)))
}

/// `from_base64(s)` — decode a standard-alphabet base64 string.
fn b_from_base64(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = expect_str(&args[0], "from_base64")?;
    let bytes = base64_decode(s).map_err(|e| decode_err(format!("Bytes.from_base64: {e}")))?;
    Ok(Value::Bytes(gc::alloc_bytes(bytes)))
}

// ---------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------

/// `to_array(b)` — the buffer as an `[Int]`, one element per byte.
fn b_to_array(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "to_array")?;
    let out: Vec<Value> = buf.borrow().iter().map(|&b| Value::Int(b as i64)).collect();
    Ok(Value::Array(gc::alloc_array(out)))
}

/// `to_string(b)` — decode the buffer as UTF-8. Raises a catchable
/// `decode` error if the bytes are not valid UTF-8.
fn b_to_string(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "to_string")?;
    let bytes = buf.borrow().clone();
    match String::from_utf8(bytes) {
        Ok(s) => Ok(Value::Str(s.into())),
        Err(e) => Err(decode_err(format!(
            "Bytes.to_string: invalid UTF-8 at byte {}",
            e.utf8_error().valid_up_to()
        ))),
    }
}

/// `to_hex(b)` — lower-case hex, two digits per byte, no separators.
fn b_to_hex(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "to_hex")?;
    Ok(Value::Str(hex_encode(&buf.borrow()).into()))
}

/// `to_base64(b)` — standard-alphabet base64 with `=` padding.
fn b_to_base64(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "to_base64")?;
    Ok(Value::Str(base64_encode(&buf.borrow()).into()))
}

// ---------------------------------------------------------------------
// Buffer ops
// ---------------------------------------------------------------------

/// `push(b, byte)` — append one byte in place. Returns `b`.
fn b_push(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "push")?;
    let byte = expect_byte(&args[1], "push")?;
    buf.borrow_mut().push(byte);
    Ok(args[0].clone())
}

/// `extend(b, other)` — append every byte of `other` in place. Returns
/// `b`. `other` is snapshotted first so `extend(b, b)` is safe.
fn b_extend(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "extend")?;
    let other = expect_bytes(&args[1], "extend")?;
    let items: Vec<u8> = other.borrow().clone();
    buf.borrow_mut().extend(items);
    Ok(args[0].clone())
}

/// `slice(b, start, end)` — a new buffer of `b[start..end]`. Negative
/// indices count from the end; bounds are clamped.
fn b_slice(args: &[Value]) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], "slice")?;
    let start = expect_int(&args[1], "slice")?;
    let end = expect_int(&args[2], "slice")?;
    let src = buf.borrow();
    let s = resolve_clamped(start, src.len());
    let e = resolve_clamped(end, src.len()).max(s);
    Ok(Value::Bytes(gc::alloc_bytes(src[s..e].to_vec())))
}

/// `concat(a, b)` — a new buffer holding `a` followed by `b`.
fn b_concat(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = expect_bytes(&args[0], "concat")?;
    let b = expect_bytes(&args[1], "concat")?;
    let mut out: Vec<u8> = a.borrow().clone();
    out.extend(b.borrow().iter().copied());
    Ok(Value::Bytes(gc::alloc_bytes(out)))
}

// ---------------------------------------------------------------------
// Integer pack/unpack
// ---------------------------------------------------------------------

/// Read a `width`-byte integer at `offset`. `signed` selects two's-
/// complement interpretation; `be` selects big-endian byte order.
fn read_int(
    args: &[Value],
    label: &str,
    width: usize,
    signed: bool,
    be: bool,
) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], label)?;
    let offset = expect_int(&args[1], label)?;
    let bytes = buf.borrow();
    if offset < 0 {
        return Err(err(format!("Bytes.{label}: negative offset {offset}")));
    }
    let off = offset as usize;
    if off > bytes.len() || bytes.len() - off < width {
        return Err(err(format!(
            "Bytes.{label}: offset {offset} + {width} bytes out of bounds (buffer length {})",
            bytes.len()
        )));
    }
    let slice = &bytes[off..off + width];
    let mut acc: u64 = 0;
    if be {
        for &b in slice {
            acc = (acc << 8) | b as u64;
        }
    } else {
        for &b in slice.iter().rev() {
            acc = (acc << 8) | b as u64;
        }
    }
    let bits = width * 8;
    let value: i64 = if signed {
        if bits == 64 {
            acc as i64
        } else if acc & (1u64 << (bits - 1)) != 0 {
            (acc as i64) - (1i64 << bits)
        } else {
            acc as i64
        }
    } else {
        if width == 8 && acc > i64::MAX as u64 {
            // The value does not fit a signed 64-bit Int — the same
            // condition v0.8 arithmetic reports as a catchable overflow.
            return Err(RuntimeError::new(RuntimeErrorKind::Overflow, 0));
        }
        acc as i64
    };
    Ok(Value::Int(value))
}

/// Write `value` as a `width`-byte integer at `offset`, in place.
/// Returns the buffer. Raises if `value` does not fit the field.
fn write_int(
    args: &[Value],
    label: &str,
    width: usize,
    signed: bool,
    be: bool,
) -> Result<Value, RuntimeError> {
    let buf = expect_bytes(&args[0], label)?;
    let offset = expect_int(&args[1], label)?;
    let value = expect_int(&args[2], label)?;
    let bits = width * 8;
    if signed {
        if bits < 64 {
            let min = -(1i64 << (bits - 1));
            let max = (1i64 << (bits - 1)) - 1;
            if value < min || value > max {
                return Err(err(format!(
                    "Bytes.{label}: value {value} does not fit a signed {width}-byte field"
                )));
            }
        }
    } else {
        if value < 0 {
            return Err(err(format!(
                "Bytes.{label}: value {value} is negative — use a signed writer"
            )));
        }
        if bits < 64 {
            let max = (1i64 << bits) - 1;
            if value > max {
                return Err(err(format!(
                    "Bytes.{label}: value {value} does not fit an unsigned {width}-byte field"
                )));
            }
        }
    }
    let acc = value as u64;
    let mut bytes = buf.borrow_mut();
    if offset < 0 {
        return Err(err(format!("Bytes.{label}: negative offset {offset}")));
    }
    let off = offset as usize;
    if off > bytes.len() || bytes.len() - off < width {
        return Err(err(format!(
            "Bytes.{label}: offset {offset} + {width} bytes out of bounds (buffer length {})",
            bytes.len()
        )));
    }
    for i in 0..width {
        let shift = if be { (width - 1 - i) * 8 } else { i * 8 };
        bytes[off + i] = ((acc >> shift) & 0xff) as u8;
    }
    drop(bytes);
    Ok(args[0].clone())
}

/// Generate the named `read_*` / `write_*` pair for one width/sign/
/// endianness combination. Each is a thin wrapper over the cores above.
macro_rules! int_fns {
    ($($rname:ident, $wname:ident, $width:expr, $signed:expr, $be:expr;)*) => {
        $(
            fn $rname(args: &[Value]) -> Result<Value, RuntimeError> {
                read_int(args, stringify!($rname), $width, $signed, $be)
            }
            fn $wname(args: &[Value]) -> Result<Value, RuntimeError> {
                write_int(args, stringify!($wname), $width, $signed, $be)
            }
        )*
    };
}

int_fns! {
    read_u8,     write_u8,     1, false, true;
    read_i8,     write_i8,     1, true,  true;
    read_u16_be, write_u16_be, 2, false, true;
    read_u16_le, write_u16_le, 2, false, false;
    read_i16_be, write_i16_be, 2, true,  true;
    read_i16_le, write_i16_le, 2, true,  false;
    read_u32_be, write_u32_be, 4, false, true;
    read_u32_le, write_u32_le, 4, false, false;
    read_i32_be, write_i32_be, 4, true,  true;
    read_i32_le, write_i32_le, 4, true,  false;
    read_u64_be, write_u64_be, 8, false, true;
    read_u64_le, write_u64_le, 8, false, false;
    read_i64_be, write_i64_be, 8, true,  true;
    read_i64_le, write_i64_le, 8, true,  false;
}

// ---------------------------------------------------------------------
// Hex / base64 codecs (hand-rolled — no external crate)
// ---------------------------------------------------------------------

fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let cleaned: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(format!("odd number of hex digits ({})", cleaned.len()));
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for pair in cleaned.chunks(2) {
        let hi = nibble(pair[0])
            .ok_or_else(|| format!("invalid hex digit {:?}", pair[0] as char))?;
        let lo = nibble(pair[1])
            .ok_or_else(|| format!("invalid hex digit {:?}", pair[1] as char))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

const B64: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(B64[(b0 >> 2) as usize] as char);
        out.push(B64[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(b2 & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let cleaned: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if cleaned.len() % 4 != 0 {
        return Err(format!(
            "length {} is not a multiple of 4",
            cleaned.len()
        ));
    }
    let chunks = cleaned.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (ci, chunk) in cleaned.chunks(4).enumerate() {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        if pad > 0 && ci + 1 != chunks {
            return Err("'=' padding before the final group".to_string());
        }
        if pad > 2 || chunk[0] == b'=' || chunk[1] == b'=' {
            return Err("misplaced '=' padding".to_string());
        }
        if chunk[2] == b'=' && chunk[3] != b'=' {
            return Err("misplaced '=' padding".to_string());
        }
        let mut q = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            if c != b'=' {
                q[i] = value(c)
                    .ok_or_else(|| format!("invalid base64 character {:?}", c as char))?;
            }
        }
        out.push((q[0] << 2) | (q[1] >> 4));
        if pad < 2 {
            out.push((q[1] << 4) | (q[2] >> 2));
        }
        if pad < 1 {
            out.push((q[2] << 6) | q[3]);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(v: Vec<u8>) -> Value {
        Value::Bytes(gc::alloc_bytes(v))
    }
    fn as_bytes(v: &Value) -> Vec<u8> {
        match v {
            Value::Bytes(b) => b.borrow().clone(),
            _ => panic!("expected Bytes, got {v:?}"),
        }
    }

    #[test]
    fn hex_roundtrip() {
        let data = vec![0x00, 0xde, 0xad, 0xbe, 0xef, 0xff];
        assert_eq!(hex_encode(&data), "00deadbeefff");
        assert_eq!(hex_decode("00deadbeefff").unwrap(), data);
        // whitespace is ignored, case is accepted
        assert_eq!(hex_decode("DE AD\tBE\nEF").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert!(hex_decode("abc").is_err()); // odd length
        assert!(hex_decode("zz").is_err()); // bad digit
    }

    #[test]
    fn base64_roundtrip() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let enc = base64_encode(s.as_bytes());
            assert_eq!(base64_decode(&enc).unwrap(), s.as_bytes());
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert!(base64_decode("abc").is_err()); // not a multiple of 4
        assert!(base64_decode("****").is_err()); // bad chars
    }

    #[test]
    fn read_unsigned_endianness() {
        let buf = bytes(vec![0x12, 0x34, 0x56, 0x78]);
        let off = Value::Int(0);
        assert_eq!(read_u16_be(&[buf.clone(), off.clone()]).unwrap(), Value::Int(0x1234));
        assert_eq!(read_u16_le(&[buf.clone(), off.clone()]).unwrap(), Value::Int(0x3412));
        assert_eq!(read_u32_be(&[buf.clone(), off.clone()]).unwrap(), Value::Int(0x12345678));
        assert_eq!(read_u32_le(&[buf, off]).unwrap(), Value::Int(0x78563412));
    }

    #[test]
    fn read_signed_sign_extends() {
        let buf = bytes(vec![0xff, 0xff]);
        assert_eq!(read_i8(&[buf.clone(), Value::Int(0)]).unwrap(), Value::Int(-1));
        assert_eq!(read_i16_be(&[buf, Value::Int(0)]).unwrap(), Value::Int(-1));
    }

    #[test]
    fn read_out_of_bounds_raises() {
        let buf = bytes(vec![0x01, 0x02]);
        assert!(read_u32_be(&[buf.clone(), Value::Int(0)]).is_err());
        assert!(read_u8(&[buf, Value::Int(-1)]).is_err());
    }

    #[test]
    fn read_u64_above_int_max_overflows() {
        let buf = bytes(vec![0xff; 8]);
        let e = read_u64_be(&[buf, Value::Int(0)]).unwrap_err();
        assert!(
            matches!(e.kind, RuntimeErrorKind::Overflow),
            "expected Overflow, got {:?}",
            e.kind
        );
    }

    #[test]
    fn write_roundtrips_read() {
        let buf = bytes(vec![0; 4]);
        write_u32_be(&[buf.clone(), Value::Int(0), Value::Int(0x0a0b0c0d)]).unwrap();
        assert_eq!(as_bytes(&buf), vec![0x0a, 0x0b, 0x0c, 0x0d]);
        assert_eq!(read_u32_be(&[buf, Value::Int(0)]).unwrap(), Value::Int(0x0a0b0c0d));
    }

    #[test]
    fn write_out_of_range_raises() {
        let buf = bytes(vec![0; 2]);
        assert!(write_u8(&[buf.clone(), Value::Int(0), Value::Int(256)]).is_err());
        assert!(write_u16_be(&[buf.clone(), Value::Int(0), Value::Int(-1)]).is_err());
        assert!(write_i8(&[buf, Value::Int(0), Value::Int(200)]).is_err());
    }
}
