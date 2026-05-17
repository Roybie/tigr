//! `import 'BigInt'` — the arbitrary-precision integer type and its
//! operations.
//!
//! `BigInt` is a `Value` in its own right (an immutable, `Rc`-managed
//! `num_bigint::BigInt`). It is created **explicitly** — there is no
//! auto-promotion of an overflowing `Int`, so v0.8's catchable
//! `overflow` error is unchanged. Once created, a `BigInt` works with
//! the ordinary operators: `+ - * / % ^^`, unary `-`, and comparisons.
//! An `Int` operand is promoted to `BigInt`; a `Float` operand promotes
//! the `BigInt` to `Float`.
//!
//! `/` is exact-or-raise — `a / b` yields a `BigInt` only when the
//! division is exact, otherwise it raises `inexact_division`. Integer
//! division is `divmod` / `div`.

use std::rc::Rc;
use std::str::FromStr;

use num_bigint::{BigInt as BigIntData, Sign};
use num_integer::Integer;
use num_traits::{Pow, Signed, ToPrimitive, Zero};

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{bigint_to_f64, Arity, Value};

use super::{native, object};

pub fn module() -> Value {
    object(&[
        // -- construction --
        ("new",          native("new",          Arity::Exact(1), b_new)),
        // -- conversion --
        ("to_int",       native("to_int",       Arity::Exact(1), b_to_int)),
        ("to_float",     native("to_float",     Arity::Exact(1), b_to_float)),
        ("to_str_radix", native("to_str_radix", Arity::Exact(2), b_to_str_radix)),
        // -- integer division --
        ("divmod",       native("divmod",       Arity::Exact(2), b_divmod)),
        ("div",          native("div",          Arity::Exact(2), b_div)),
        // -- number theory / sign --
        ("abs",          native("abs",          Arity::Exact(1), b_abs)),
        ("pow",          native("pow",          Arity::Exact(2), b_pow)),
        ("sign",         native("sign",         Arity::Exact(1), b_sign)),
        ("is_negative",  native("is_negative",  Arity::Exact(1), b_is_negative)),
        ("gcd",          native("gcd",          Arity::Exact(2), b_gcd)),
        ("lcm",          native("lcm",          Arity::Exact(2), b_lcm)),
    ])
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Wrap a `num_bigint::BigInt` back into a `Value`.
fn big(n: BigIntData) -> Value {
    Value::BigInt(Rc::new(n))
}

/// A catchable, string-valued error. The VM backfills the call line.
fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

/// A catchable structured parse error — `${kind: 'parse', message}` —
/// so `catch` code and `Test.assert_raises(..., 'parse')` can dispatch
/// on `.kind`.
fn parse_err(msg: String) -> RuntimeError {
    let obj = super::object(&[
        ("kind", Value::Str("parse".into())),
        ("message", Value::Str(msg.into())),
    ]);
    RuntimeError::new(RuntimeErrorKind::Raised(obj), 0)
}

/// Accept either a `BigInt` or an `Int` and yield a `BigInt` — every
/// `BigInt` operation interoperates with plain integers.
fn as_bigint(v: &Value, label: &str) -> Result<BigIntData, RuntimeError> {
    match v {
        Value::BigInt(b) => Ok((**b).clone()),
        Value::Int(n) => Ok(BigIntData::from(*n)),
        other => Err(err(format!(
            "BigInt.{label}: expected a bigint or int, got {}",
            other.type_name()
        ))),
    }
}

fn expect_int(v: &Value, label: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(err(format!(
            "BigInt.{label}: expected an int, got {}",
            other.type_name()
        ))),
    }
}

// ---------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------

/// `new(x)` — build a `BigInt` from an `Int`, a decimal `String`
/// (optional leading sign, surrounding whitespace trimmed), or another
/// `BigInt` (returned unchanged). A malformed string raises a catchable
/// `parse` error.
fn b_new(args: &[Value]) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Int(n) => Ok(big(BigIntData::from(*n))),
        Value::BigInt(_) => Ok(args[0].clone()),
        Value::Str(s) => {
            let t = s.trim();
            BigIntData::from_str(t)
                .map(big)
                .map_err(|_| parse_err(format!("BigInt.new: '{t}' is not a valid integer")))
        }
        other => Err(err(format!(
            "BigInt.new: expected an int or string, got {}",
            other.type_name()
        ))),
    }
}

// ---------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------

/// `to_int(b)` — narrow back to an `Int`, raising the catchable
/// `overflow` error if the value is outside the `i64` range.
fn b_to_int(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = as_bigint(&args[0], "to_int")?;
    match n.to_i64() {
        Some(i) => Ok(Value::Int(i)),
        None => Err(RuntimeError::new(RuntimeErrorKind::Overflow, 0)),
    }
}

/// `to_float(b)` — convert to a `Float`, saturating to `±inf` for a
/// magnitude beyond the float range. Never raises.
fn b_to_float(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = as_bigint(&args[0], "to_float")?;
    Ok(Value::Float(bigint_to_f64(&n)))
}

/// `to_str_radix(b, radix)` — the value as a string in base `radix`
/// (2..=36). Covers the radix printing that `str()` only does for `Int`.
fn b_to_str_radix(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = as_bigint(&args[0], "to_str_radix")?;
    let radix = expect_int(&args[1], "to_str_radix")?;
    if !(2..=36).contains(&radix) {
        return Err(err(format!(
            "BigInt.to_str_radix: radix {radix} out of range 2..=36"
        )));
    }
    Ok(Value::Str(n.to_str_radix(radix as u32).into()))
}

// ---------------------------------------------------------------------
// Integer division
// ---------------------------------------------------------------------

/// `divmod(a, b)` — `[quotient, remainder]`, truncating toward zero
/// (the remainder takes the sign of `a`). Raises `div_by_zero` if
/// `b == 0`.
fn b_divmod(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = as_bigint(&args[0], "divmod")?;
    let b = as_bigint(&args[1], "divmod")?;
    if b.is_zero() {
        return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, 0));
    }
    let (q, r) = a.div_rem(&b);
    Ok(Value::Array(crate::vm::gc::alloc_array(vec![big(q), big(r)])))
}

/// `div(a, b)` — the truncating integer quotient. Raises `div_by_zero`
/// if `b == 0`.
fn b_div(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = as_bigint(&args[0], "div")?;
    let b = as_bigint(&args[1], "div")?;
    if b.is_zero() {
        return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, 0));
    }
    Ok(big(a / b))
}

// ---------------------------------------------------------------------
// Number theory / sign
// ---------------------------------------------------------------------

/// `abs(b)` — the absolute value.
fn b_abs(args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(big(as_bigint(&args[0], "abs")?.abs()))
}

/// `pow(base, exp)` — `base` raised to a non-negative integer `exp`,
/// exactly. A negative exponent raises (a fraction is not a `BigInt`).
fn b_pow(args: &[Value]) -> Result<Value, RuntimeError> {
    let base = as_bigint(&args[0], "pow")?;
    let exp = expect_int(&args[1], "pow")?;
    if exp < 0 {
        return Err(err(format!(
            "BigInt.pow: exponent {exp} is negative"
        )));
    }
    Ok(big(Pow::pow(&base, exp as u64)))
}

/// `sign(b)` — `-1`, `0`, or `1` as an `Int`.
fn b_sign(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = as_bigint(&args[0], "sign")?;
    Ok(Value::Int(match n.sign() {
        Sign::Minus => -1,
        Sign::NoSign => 0,
        Sign::Plus => 1,
    }))
}

/// `is_negative(b)` — `true` for a value below zero.
fn b_is_negative(args: &[Value]) -> Result<Value, RuntimeError> {
    let n = as_bigint(&args[0], "is_negative")?;
    Ok(Value::Bool(n.sign() == Sign::Minus))
}

/// `gcd(a, b)` — the greatest common divisor (always non-negative).
fn b_gcd(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = as_bigint(&args[0], "gcd")?;
    let b = as_bigint(&args[1], "gcd")?;
    Ok(big(a.gcd(&b)))
}

/// `lcm(a, b)` — the least common multiple.
fn b_lcm(args: &[Value]) -> Result<Value, RuntimeError> {
    let a = as_bigint(&args[0], "lcm")?;
    let b = as_bigint(&args[1], "lcm")?;
    Ok(big(a.lcm(&b)))
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn b(n: i64) -> Value {
        big(BigIntData::from(n))
    }
    fn as_big(v: &Value) -> BigIntData {
        match v {
            Value::BigInt(x) => (**x).clone(),
            _ => panic!("expected BigInt, got {v:?}"),
        }
    }

    #[test]
    fn new_from_int_string_bigint() {
        assert_eq!(as_big(&b_new(&[Value::Int(42)]).unwrap()), BigIntData::from(42));
        assert_eq!(
            as_big(&b_new(&[Value::Str("  -1000000000000000000000  ".into())]).unwrap()),
            BigIntData::from_str("-1000000000000000000000").unwrap()
        );
        let big1 = b(7);
        assert_eq!(as_big(&b_new(&[big1.clone()]).unwrap()), BigIntData::from(7));
    }

    #[test]
    fn new_rejects_malformed_string() {
        let e = b_new(&[Value::Str("12x3".into())]).unwrap_err();
        match e.kind {
            RuntimeErrorKind::Raised(Value::Object(o)) => {
                assert_eq!(o.borrow().get("kind").cloned(), Some(Value::Str("parse".into())));
            }
            other => panic!("expected a structured parse error, got {other:?}"),
        }
    }

    #[test]
    fn to_int_overflows_outside_i64() {
        // i64::MAX + 1 cannot narrow back.
        let huge = b_new(&[Value::Str("9223372036854775808".into())]).unwrap();
        assert!(matches!(
            b_to_int(&[huge]).unwrap_err().kind,
            RuntimeErrorKind::Overflow
        ));
        assert_eq!(b_to_int(&[b(123)]).unwrap(), Value::Int(123));
    }

    #[test]
    fn divmod_and_div() {
        let dm = b_divmod(&[b(17), b(5)]).unwrap();
        match dm {
            Value::Array(a) => {
                let a = a.borrow();
                assert_eq!(as_big(&a[0]), BigIntData::from(3));
                assert_eq!(as_big(&a[1]), BigIntData::from(2));
            }
            _ => panic!("expected an array"),
        }
        assert_eq!(as_big(&b_div(&[b(17), b(5)]).unwrap()), BigIntData::from(3));
        assert!(matches!(
            b_div(&[b(1), b(0)]).unwrap_err().kind,
            RuntimeErrorKind::DivisionByZero
        ));
    }

    #[test]
    fn pow_gcd_sign() {
        assert_eq!(
            as_big(&b_pow(&[b(2), Value::Int(64)]).unwrap()),
            BigIntData::from_str("18446744073709551616").unwrap()
        );
        assert!(b_pow(&[b(2), Value::Int(-1)]).is_err());
        assert_eq!(as_big(&b_gcd(&[b(48), b(18)]).unwrap()), BigIntData::from(6));
        assert_eq!(b_sign(&[b(-9)]).unwrap(), Value::Int(-1));
        assert_eq!(b_is_negative(&[b(-9)]).unwrap(), Value::Bool(true));
    }
}
