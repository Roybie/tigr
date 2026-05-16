//! `import 'DateTime'` — UTC calendar date/time.
//!
//! Everything is **UTC**: `std` ships no timezone database and we keep
//! the dependency set minimal (no `chrono`). Calendar conversion uses
//! Howard Hinnant's `days`<->`civil` algorithm.
//!
//! A "components object" is `${year, month, day, hour, minute, second,
//! ms, weekday, yearday}` — `month` is 1-12, `weekday` is 0=Sunday,
//! `yearday` is the 1-based day of the year.

use std::rc::Rc;

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{Arity, Value};

use super::{native, object};

const MS_PER_DAY: i64 = 86_400_000;

pub fn module() -> Value {
    object(&[
        ("now",     native("now",     Arity::Exact(0), now)),
        ("from_ms", native("from_ms", Arity::Exact(1), from_ms)),
        ("to_ms",   native("to_ms",   Arity::Exact(1), to_ms)),
        ("format",  native("format",  Arity::Exact(2), format_fn)),
        ("parse",   native("parse",   Arity::Exact(1), parse)),
    ])
}

fn raise(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

// ---- Hinnant civil <-> days-since-epoch ----

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

struct Parts {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    ms: i64,
    weekday: i64,
    yearday: i64,
}

fn parts_from_ms(epoch_ms: i64) -> Parts {
    let days = epoch_ms.div_euclid(MS_PER_DAY);
    let tod = epoch_ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    Parts {
        year,
        month,
        day,
        hour: tod / 3_600_000,
        minute: (tod / 60_000) % 60,
        second: (tod / 1000) % 60,
        ms: tod % 1000,
        weekday: (days + 4).rem_euclid(7), // 1970-01-01 was a Thursday
        yearday: days - days_from_civil(year, 1, 1) + 1,
    }
}

fn parts_object(p: &Parts) -> Value {
    object(&[
        ("year",    Value::Int(p.year)),
        ("month",   Value::Int(p.month)),
        ("day",     Value::Int(p.day)),
        ("hour",    Value::Int(p.hour)),
        ("minute",  Value::Int(p.minute)),
        ("second",  Value::Int(p.second)),
        ("ms",      Value::Int(p.ms)),
        ("weekday", Value::Int(p.weekday)),
        ("yearday", Value::Int(p.yearday)),
    ])
}

fn now(_args: &[Value]) -> Result<Value, RuntimeError> {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| raise(format!("DateTime.now: {e}")))?;
    Ok(parts_object(&parts_from_ms(d.as_millis() as i64)))
}

fn from_ms(args: &[Value]) -> Result<Value, RuntimeError> {
    let ms = expect_int(&args[0], "from_ms")?;
    Ok(parts_object(&parts_from_ms(ms)))
}

fn to_ms(args: &[Value]) -> Result<Value, RuntimeError> {
    let map = match &args[0] {
        Value::Object(o) => o.borrow(),
        other => {
            return Err(raise(format!(
                "DateTime.to_ms: expected Object, got {}",
                other.type_name()
            )))
        }
    };
    let year = field(&map, "year", 1970)?;
    let month = field(&map, "month", 1)?;
    let day = field(&map, "day", 1)?;
    let hour = field(&map, "hour", 0)?;
    let minute = field(&map, "minute", 0)?;
    let second = field(&map, "second", 0)?;
    let ms = field(&map, "ms", 0)?;
    let days = days_from_civil(year, month, day);
    Ok(Value::Int(
        days * MS_PER_DAY + hour * 3_600_000 + minute * 60_000 + second * 1000 + ms,
    ))
}

fn format_fn(args: &[Value]) -> Result<Value, RuntimeError> {
    let ms = expect_int(&args[0], "format")?;
    let fmt = match &args[1] {
        Value::Str(s) => s,
        other => {
            return Err(raise(format!(
                "DateTime.format: expected String format, got {}",
                other.type_name()
            )))
        }
    };
    let p = parts_from_ms(ms);
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{:04}", p.year)),
            Some('m') => out.push_str(&format!("{:02}", p.month)),
            Some('d') => out.push_str(&format!("{:02}", p.day)),
            Some('H') => out.push_str(&format!("{:02}", p.hour)),
            Some('M') => out.push_str(&format!("{:02}", p.minute)),
            Some('S') => out.push_str(&format!("{:02}", p.second)),
            Some('j') => out.push_str(&format!("{:03}", p.yearday)),
            Some('%') => out.push('%'),
            Some(other) => {
                return Err(raise(format!(
                    "DateTime.format: unknown directive %{other}"
                )))
            }
            None => {
                return Err(raise(
                    "DateTime.format: trailing `%` in format string".into(),
                ))
            }
        }
    }
    Ok(Value::Str(out.into()))
}

fn parse(args: &[Value]) -> Result<Value, RuntimeError> {
    let s = match &args[0] {
        Value::Str(s) => s.trim(),
        other => {
            return Err(raise(format!(
                "DateTime.parse: expected String, got {}",
                other.type_name()
            )))
        }
    };
    // ISO-8601: `YYYY-MM-DD` then optionally `(T| )HH:MM:SS[.fff]`.
    let err = |s: &str| raise(format!("DateTime.parse: invalid ISO-8601 datetime {s:?}"));
    let num = |slice: &str| slice.parse::<i64>().ok();
    if !s.is_ascii() || s.len() < 10 {
        return Err(err(s));
    }
    if &s[4..5] != "-" || &s[7..8] != "-" {
        return Err(err(s));
    }
    let year = num(&s[0..4]).ok_or_else(|| err(s))?;
    let month = num(&s[5..7]).ok_or_else(|| err(s))?;
    let day = num(&s[8..10]).ok_or_else(|| err(s))?;
    let (mut hour, mut minute, mut second, mut millis) = (0, 0, 0, 0);
    if s.len() > 10 {
        let sep = &s[10..11];
        if sep != "T" && sep != " " {
            return Err(err(s));
        }
        let t = &s[11..];
        if t.len() < 8 || &t[2..3] != ":" || &t[5..6] != ":" {
            return Err(err(s));
        }
        hour = num(&t[0..2]).ok_or_else(|| err(s))?;
        minute = num(&t[3..5]).ok_or_else(|| err(s))?;
        second = num(&t[6..8]).ok_or_else(|| err(s))?;
        if t.len() > 8 {
            if &t[8..9] != "." {
                return Err(err(s));
            }
            let frac = &t[9..];
            if frac.is_empty()
                || frac.len() > 3
                || !frac.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(err(s));
            }
            let mut f = frac.to_string();
            while f.len() < 3 {
                f.push('0'); // pad to milliseconds
            }
            millis = num(&f).ok_or_else(|| err(s))?;
        }
    }
    let days = days_from_civil(year, month, day);
    Ok(Value::Int(
        days * MS_PER_DAY
            + hour * 3_600_000
            + minute * 60_000
            + second * 1000
            + millis,
    ))
}

fn expect_int(v: &Value, label: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(raise(format!(
            "DateTime.{label}: expected Int, got {}",
            other.type_name()
        ))),
    }
}

fn field(map: &IndexMap<Rc<str>, Value>, key: &str, default: i64) -> Result<i64, RuntimeError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(raise(format!(
            "DateTime.to_ms: field {key:?} must be an Int, got {}",
            other.type_name()
        ))),
    }
}
