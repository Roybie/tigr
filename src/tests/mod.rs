//! Integration tests for the v0.2 VM.
//!
//! Each phase has a directory under `examples/v02/phaseN/`. Each `.tg`
//! file there has an expected final value listed below. The tests run
//! the program through the full pipeline and compare.

use crate::vm::run_source;
use crate::vm::value::Value;

fn run(src: &str) -> Value {
    run_source(src).unwrap_or_else(|e| panic!("error: {e}"))
}

fn run_err(src: &str) -> String {
    match run_source(src) {
        Ok(v) => panic!("expected error, got value {v:?}"),
        Err(e) => format!("{e}"),
    }
}

// ---- Phase 1: literals, arithmetic, decl/assign, blocks ----

#[test]
fn phase1_arith() {
    let src = "x := 2 + 3 * 4; y := x ^ 2 / 7; y";
    assert_eq!(run(src), Value::Int(28));
}

#[test]
fn phase1_precedence() {
    assert_eq!(run("1 + 2 * 3 ^ 2"), Value::Int(19));
}

#[test]
fn phase1_decl_then_use() {
    assert_eq!(run("x := 5; x * x"), Value::Int(25));
}

#[test]
fn phase1_block_value() {
    assert_eq!(run("(a := 1; b := a + 1; b * 2)"), Value::Int(4));
}

#[test]
fn phase1_block_trailing_semi_is_null() {
    assert_eq!(run("(a := 1; b := 2;)"), Value::Null);
}

#[test]
fn phase1_floats() {
    let v = run("pi := 3.14; r := 5; pi * r ^ 2");
    match v {
        Value::Float(x) => assert!((x - 78.5).abs() < 1e-9, "got {x}"),
        _ => panic!("expected float, got {v:?}"),
    }
}

#[test]
fn phase1_int_div_returns_int_when_even() {
    assert_eq!(run("28 / 7"), Value::Int(4));
}

#[test]
fn phase1_int_div_returns_float_when_uneven() {
    match run("10 / 3") {
        Value::Float(x) => assert!((x - 10.0 / 3.0).abs() < 1e-9, "got {x}"),
        v => panic!("expected float, got {v:?}"),
    }
}

#[test]
fn phase1_pow_always_float() {
    match run("2 ^ 3") {
        Value::Float(x) => assert!((x - 8.0).abs() < 1e-9, "got {x}"),
        v => panic!("expected float, got {v:?}"),
    }
}

#[test]
fn phase1_assign() {
    assert_eq!(run("x := 1; x = x + 10; x = x * 2; x"), Value::Int(22));
}

#[test]
fn phase1_unary_neg() {
    assert_eq!(run("x := 10; -x + -3"), Value::Int(-13));
}

#[test]
fn phase1_assign_to_undeclared_errors() {
    let msg = run_err("x = 5");
    assert!(msg.contains("undeclared"), "got {msg}");
}

#[test]
fn phase1_decl_duplicate_errors() {
    let msg = run_err("x := 1; x := 2");
    assert!(msg.contains("already declared"), "got {msg}");
}

#[test]
fn phase1_decl_is_expression() {
    // `:=` evaluates to the assigned value
    assert_eq!(run("(x := 5) * 2"), Value::Int(10));
}

#[test]
fn phase1_empty_block_is_null() {
    assert_eq!(run("()"), Value::Null);
}

#[test]
fn phase1_string_literal() {
    match run("'hello'") {
        Value::Str(s) => assert_eq!(&*s, "hello"),
        v => panic!("expected string, got {v:?}"),
    }
}

#[test]
fn phase1_bool_and_null_literals() {
    assert_eq!(run("true"), Value::Bool(true));
    assert_eq!(run("false"), Value::Bool(false));
    assert_eq!(run("null"), Value::Null);
}

#[test]
fn phase1_modulo() {
    assert_eq!(run("17 % 5"), Value::Int(2));
}
