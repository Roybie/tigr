//! Integration tests for the v0.2 VM.
//!
//! Each phase has a directory under `examples/v02/phaseN/`. Each `.tg`
//! file there has an expected final value listed below. The tests run
//! the program through the full pipeline and compare.

use crate::vm::run_source;
use crate::vm::run_source_with_map;
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

/// Render `src`'s error against its own SourceMap, panicking if the
/// program succeeded. Returns the multi-line snippet output that the
/// CLI/REPL would print.
fn render_err(src: &str) -> String {
    let (result, sources) = run_source_with_map(src);
    match result {
        Ok(v) => panic!("expected error, got value {v:?}"),
        Err(e) => e.render(&sources.borrow()),
    }
}

// ---- Phase 1: literals, arithmetic, decl/assign, blocks ----

#[test]
fn phase1_arith() {
    let src = "x := 2 + 3 * 4; y := x ^^ 2 / 7; y";
    assert_eq!(run(src), Value::Int(28));
}

#[test]
fn phase1_precedence() {
    assert_eq!(run("1 + 2 * 3 ^^ 2"), Value::Int(19));
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
    let v = run("pi := 3.14; r := 5; pi * r ^^ 2");
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
    match run("2 ^^ 3") {
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

// ---- Phase 2: control flow, scopes, comparison, logical ----

#[test]
fn phase2_while_sum() {
    let src = "i := 0; sum := 0; while i < 10 { sum = sum + i; i = i + 1 }; sum";
    assert_eq!(run(src), Value::Int(45));
}

#[test]
fn phase2_while_zero_iter_returns_null() {
    assert_eq!(run("while false { 42 }"), Value::Null);
}

#[test]
fn phase2_while_returns_last_iter_value() {
    let src = "i := 0; while i < 3 { i = i + 1 }";
    // body's tail is `i = i + 1`, an assignment expr that evaluates to
    // the assigned value. Last iteration writes 3.
    assert_eq!(run(src), Value::Int(3));
}

#[test]
fn phase2_if_takes_then() {
    assert_eq!(run("if true { 10 }"), Value::Int(10));
}

#[test]
fn phase2_if_no_else_falls_through_to_null() {
    assert_eq!(run("if false { 10 }"), Value::Null);
}

#[test]
fn phase2_if_else() {
    assert_eq!(run("if 1 == 2 { 'yes' } else { 'no' }"), Value::Str("no".into()));
}

#[test]
fn phase2_else_if_chain() {
    let src = "x := 2; if x == 1 { 'one' } else if x == 2 { 'two' } else { 'many' }";
    assert_eq!(run(src), Value::Str("two".into()));
}

#[test]
fn phase2_short_circuit_or_returns_operand() {
    // 0 is falsy → `||` returns the right operand
    assert_eq!(run("0 || 'fallback'"), Value::Str("fallback".into()));
}

#[test]
fn phase2_short_circuit_and_returns_operand() {
    assert_eq!(run("'a' && 'b'"), Value::Str("b".into()));
    assert_eq!(run("null && 'b'"), Value::Null);
}

#[test]
fn phase2_short_circuit_or_returns_left_when_truthy() {
    assert_eq!(run("'first' || 'second'"), Value::Str("first".into()));
}

#[test]
fn phase2_not_unary() {
    assert_eq!(run("!false"), Value::Bool(true));
    assert_eq!(run("!0"), Value::Bool(true));
    assert_eq!(run("!''"), Value::Bool(true));
    assert_eq!(run("!1"), Value::Bool(false));
    assert_eq!(run("!'x'"), Value::Bool(false));
}

#[test]
fn phase2_comparison_int_float() {
    assert_eq!(run("1 < 2"), Value::Bool(true));
    assert_eq!(run("2 == 2.0"), Value::Bool(true));
    assert_eq!(run("3 > 2.5"), Value::Bool(true));
    assert_eq!(run("3 != 'three'"), Value::Bool(true));
}

#[test]
fn phase2_scope_value() {
    // spec §4.3 example: a=9, b={c:=20; c * (a += 1)} where a goes to 10, b to 200
    // We don't have += yet, so use `a = a + 1` which evaluates to 10.
    let src = "a := 9; b := { c := 20; c * (a = a + 1) }; b";
    assert_eq!(run(src), Value::Int(200));
}

#[test]
fn phase2_scope_outer_mutated() {
    let src = "a := 9; { c := 20; a = a + 1 }; a";
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase2_scope_inner_var_not_visible() {
    // `c` declared inside the scope is gone afterwards
    let msg = run_err("{ c := 20 }; c");
    assert!(msg.contains("undeclared"), "got {msg}");
}

#[test]
fn phase2_scope_shadows_outer() {
    let src = "x := 1; y := { x := 99; x + 1 }; (y == 100) && (x == 1)";
    assert_eq!(run(src), Value::Bool(true));
}

#[test]
fn phase2_nested_if_in_while() {
    let src = "i := 0; out := 0; while i < 5 {\
                if i == 3 { out = 999 } else { out = i };\
                i = i + 1\
              }; out";
    // last iteration: i=4, out=4
    assert_eq!(run(src), Value::Int(4));
}

#[test]
fn phase2_truthy_int_in_if() {
    assert_eq!(run("if 7 { 'yes' } else { 'no' }"), Value::Str("yes".into()));
    assert_eq!(run("if 0 { 'yes' } else { 'no' }"), Value::Str("no".into()));
}

#[test]
fn phase2_truthy_string_in_if() {
    assert_eq!(run("if 'x' { 1 } else { 2 }"), Value::Int(1));
    assert_eq!(run("if '' { 1 } else { 2 }"), Value::Int(2));
}

// ---- Phase 3: collections, indexing, references, built-ins ----

#[test]
fn phase3_array_literal_and_index() {
    assert_eq!(run("[10, 20, 30][1]"), Value::Int(20));
}

#[test]
fn phase3_array_negative_index() {
    assert_eq!(run("[1, 2, 3][-1]"), Value::Int(3));
    assert_eq!(run("[1, 2, 3][-2]"), Value::Int(2));
}

#[test]
fn phase3_array_oob_is_null() {
    assert_eq!(run("[1, 2][99]"), Value::Null);
    assert_eq!(run("[1, 2][-5]"), Value::Null);
}

#[test]
fn phase3_reference_semantics_alias_mutation() {
    // The canonical reference-semantics test from the plan.
    let src = "arr := [1, 2, 3]; copy := arr; copy[0] = 99; arr[0]";
    assert_eq!(run(src), Value::Int(99));
}

#[test]
fn phase3_object_literal_and_member() {
    let src = "o := ${ name: 'tigr', version: 2 }; o.name";
    assert_eq!(run(src), Value::Str("tigr".into()));
}

#[test]
fn phase3_object_index_with_string_key() {
    let src = "o := ${ 'with space': 99 }; o['with space']";
    assert_eq!(run(src), Value::Int(99));
}

#[test]
fn phase3_object_missing_key_is_null() {
    assert_eq!(run("${}['missing']"), Value::Null);
}

#[test]
fn phase3_object_member_set() {
    let src = "o := ${}; o.color = 'red'; o.color";
    assert_eq!(run(src), Value::Str("red".into()));
}

#[test]
fn phase3_object_reference_semantics() {
    let src = "o := ${ x: 1 }; alias := o; alias.x = 99; o.x";
    assert_eq!(run(src), Value::Int(99));
}

#[test]
fn phase3_empty_collections_are_falsy() {
    assert_eq!(run("if [] { 'no' } else { 'yes' }"), Value::Str("yes".into()));
    assert_eq!(run("if ${} { 'no' } else { 'yes' }"), Value::Str("yes".into()));
    assert_eq!(run("if [1] { 'yes' } else { 'no' }"), Value::Str("yes".into()));
    assert_eq!(run("if ${a:1} { 'yes' } else { 'no' }"), Value::Str("yes".into()));
}

#[test]
fn phase3_string_concat() {
    assert_eq!(run("'hello' + ', ' + 'world'"), Value::Str("hello, world".into()));
}

#[test]
fn phase3_string_index() {
    assert_eq!(run("'hello'[0]"), Value::Str("h".into()));
    assert_eq!(run("'hello'[-1]"), Value::Str("o".into()));
}

#[test]
fn phase3_string_immutable() {
    let msg = run_err("s := 'abc'; s[0] = 'x'");
    assert!(msg.contains("immutable"), "got {msg}");
}

#[test]
fn phase3_length_of_each() {
    assert_eq!(run("#[1, 2, 3, 4]"), Value::Int(4));
    assert_eq!(run("#${a: 1, b: 2}"), Value::Int(2));
    assert_eq!(run("#'hello'"), Value::Int(5));
}

#[test]
fn phase3_array_plus_value_appends_new() {
    // spec §7.1: arr + v appends; arr itself unchanged
    let src = "arr := [1, 2, 3]; arr + 4";
    let v = run(src);
    match v {
        Value::Array(a) => {
            let a = a.borrow();
            assert_eq!(a.len(), 4);
            assert_eq!(a[3], Value::Int(4));
        }
        _ => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase3_array_plus_array_concats() {
    let src = "[1, 2] + [3, 4]";
    let v = run(src);
    match v {
        Value::Array(a) => {
            let a = a.borrow();
            assert_eq!(a.len(), 4);
            assert_eq!(a[0], Value::Int(1));
            assert_eq!(a[3], Value::Int(4));
        }
        _ => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase3_array_plus_does_not_mutate_lhs() {
    let src = "arr := [1, 2, 3]; arr + 4; #arr";
    assert_eq!(run(src), Value::Int(3));
}

#[test]
fn phase3_compound_assign_array_append() {
    // arr += v: rebinds arr to a new array containing v
    let src = "arr := [1, 2]; arr += 3; arr + 0";
    let v = run(src);
    match v {
        Value::Array(a) => {
            let a = a.borrow();
            assert_eq!(a.len(), 4);
            assert_eq!(a[2], Value::Int(3));
        }
        _ => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase3_compound_assign_index() {
    // arr[i] += v: evaluates arr/i once; mutates in place
    let src = "arr := [10, 20, 30]; arr[1] += 5; arr[1]";
    assert_eq!(run(src), Value::Int(25));
}

#[test]
fn phase3_compound_assign_object_member() {
    let src = "o := ${ n: 10 }; o.n *= 3; o.n";
    assert_eq!(run(src), Value::Int(30));
}

#[test]
fn phase3_native_str() {
    assert_eq!(run("str(42)"), Value::Str("42".into()));
    assert_eq!(run("str(null)"), Value::Str("null".into()));
    assert_eq!(run("str(true)"), Value::Str("true".into()));
    assert_eq!(run("str([1, 2])"), Value::Str("[1, 2]".into()));
}

#[test]
fn phase3_native_print_is_value() {
    // print can be stored/passed
    assert_eq!(run("f := print; null"), Value::Null);
}

#[test]
fn phase3_print_returns_last_arg() {
    // mirrors block-tail semantics
    assert_eq!(run("print()"), Value::Null);
    assert_eq!(run("print(42)"), Value::Int(42));
    assert_eq!(run("print('label:', 7)"), Value::Int(7));
    // wrap-and-debug pattern: log a value and pass it through
    assert_eq!(run("x := print('val:', 10); x * 2"), Value::Int(20));
}

#[test]
fn phase3_native_str_arity_error() {
    let msg = run_err("str()");
    assert!(msg.contains("arguments"), "got {msg}");
}

#[test]
fn phase3_print_is_shadowable() {
    // user binding shadows the built-in
    let src = "print := fn_placeholder := 5; print";
    // (Phase 3 doesn't have `fn` value beyond NativeFn; rebinding to an int suffices.)
    assert_eq!(run(src), Value::Int(5));
}

#[test]
fn phase3_object_iteration_order_preserved_in_str() {
    // IndexMap preserves insertion order; str() displays accordingly
    let src = "str(${ first: 1, second: 2, third: 3 })";
    assert_eq!(run(src), Value::Str("${first: 1, second: 2, third: 3}".into()));
}

#[test]
fn phase3_nested_object_member() {
    let src = "o := ${ inner: ${ x: 7 } }; o.inner.x";
    assert_eq!(run(src), Value::Int(7));
}

#[test]
fn phase3_chained_indexing() {
    let src = "matrix := [[1, 2], [3, 4]]; matrix[1][0]";
    assert_eq!(run(src), Value::Int(3));
}

#[test]
fn phase3_call_chained() {
    // returns value of str(str(42)) which is the string "42" (str is idempotent)
    assert_eq!(run("str(str(42))"), Value::Str("42".into()));
}

#[test]
fn phase3_array_eq_structural() {
    assert_eq!(run("[1, 2, 3] == [1, 2, 3]"), Value::Bool(true));
    assert_eq!(run("[1, 2, 3] == [1, 2, 4]"), Value::Bool(false));
}

#[test]
fn phase3_object_eq_structural() {
    assert_eq!(run("${a:1, b:2} == ${a:1, b:2}"), Value::Bool(true));
    assert_eq!(run("${a:1} == ${a:2}"), Value::Bool(false));
}

// ---- Phase 4: functions, closures, return ----

#[test]
fn phase4_simple_call() {
    assert_eq!(run("f := fn() { 42 }; f()"), Value::Int(42));
}

#[test]
fn phase4_call_with_args() {
    assert_eq!(run("add := fn(a, b) { a + b }; add(2, 3)"), Value::Int(5));
}

#[test]
fn phase4_missing_args_become_null() {
    let src = "f := fn(a, b) { if b == null { 'b is null' } else { 'both given' } }; f(1)";
    assert_eq!(run(src), Value::Str("b is null".into()));
}

#[test]
fn phase4_extra_args_dropped() {
    // f expects 1 arg; we pass 3. Extras silently dropped.
    let src = "f := fn(a) { a }; f(99, 'x', 'y')";
    assert_eq!(run(src), Value::Int(99));
}

#[test]
fn phase4_explicit_return() {
    let src = "f := fn(n) { if n < 0 { return 'neg' }; 'pos' }; f(-5)";
    assert_eq!(run(src), Value::Str("neg".into()));
}

#[test]
fn phase4_return_no_value() {
    let src = "f := fn() { return }; f()";
    assert_eq!(run(src), Value::Null);
}

#[test]
fn phase4_recursion_factorial() {
    // f := fn(n) { if n <= 1 { 1 } else { n * f(n-1) } }
    let src = "fact := fn(n) { if n <= 1 { 1 } else { n * fact(n - 1) } }; fact(6)";
    assert_eq!(run(src), Value::Int(720));
}

#[test]
fn phase4_closure_captures_outer() {
    let src = "x := 10; f := fn() { x }; f()";
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase4_closure_captures_after_outer_changes() {
    let src = "x := 1; f := fn() { x }; x = 99; f()";
    assert_eq!(run(src), Value::Int(99));
}

#[test]
fn phase4_counter_closure() {
    // The canonical Phase 4 test: closure over a mutable cell
    let src = "make_counter := fn() { n := 0; fn() { n = n + 1; n } };
               c := make_counter();
               c(); c(); c()";
    assert_eq!(run(src), Value::Int(3));
}

#[test]
fn phase4_two_counters_independent() {
    // each call to make_counter creates a fresh `n` cell
    let src = "make_counter := fn() { n := 0; fn() { n = n + 1; n } };
               a := make_counter();
               b := make_counter();
               a(); a(); a();
               b()";
    assert_eq!(run(src), Value::Int(1));
}

#[test]
fn phase4_nested_closures() {
    // innermost captures from grandparent (`x`) AND parent (`y`)
    let src = "outer := fn(x) { fn(y) { fn(z) { x + y + z } } };
               outer(100)(20)(3)";
    assert_eq!(run(src), Value::Int(123));
}

#[test]
fn phase4_function_as_value_in_array() {
    let src = "fns := [fn(x) { x * 2 }, fn(x) { x + 1 }, fn(x) { x * x }];
               fns[2](5)";
    assert_eq!(run(src), Value::Int(25));
}

#[test]
fn phase4_function_as_value_in_object() {
    let src = "ops := ${ double: fn(x) { x * 2 }, square: fn(x) { x * x } };
               ops.square(7)";
    assert_eq!(run(src), Value::Int(49));
}

#[test]
fn phase4_pass_function_as_arg() {
    let src = "apply := fn(f, x) { f(x) };
               double := fn(n) { n * 2 };
               apply(double, 21)";
    assert_eq!(run(src), Value::Int(42));
}

#[test]
fn phase4_return_function() {
    let src = "adder := fn(n) { fn(x) { x + n } };
               add5 := adder(5);
               add5(10)";
    assert_eq!(run(src), Value::Int(15));
}

#[test]
fn phase4_closure_modifies_captured_after_outer_returned() {
    // The classic "captured local outlives its frame" test.
    // After make_counter() returns, n is no longer on the stack;
    // it must have been heap-promoted via close_upvalues.
    let src = "make_counter := fn() { n := 0; fn() { n = n + 1; n } };
               c := make_counter();
               c() + c() + c() + c()";
    // 1 + 2 + 3 + 4 = 10
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase4_assign_to_builtin_errors() {
    let msg = run_err("print = 5");
    assert!(msg.contains("built-in"), "got {msg}");
}

#[test]
fn phase4_shadow_then_call_outer() {
    // shadow `print` inside a scope; outer scope still has the built-in
    let src = "x := { print := 5; print };
               (x == 5)";
    assert_eq!(run(src), Value::Bool(true));
}

#[test]
fn phase4_recursive_closure_captures_self() {
    // recursion through an upvalue: outer fn refers to itself, inner
    // closure also captures it
    let src = "fib := fn(n) { if n < 2 { n } else { fib(n-1) + fib(n-2) } };
               fib(10)";
    assert_eq!(run(src), Value::Int(55));
}

#[test]
fn phase4_immediately_invoked() {
    // `fn() { 42 }()` — call a function literal directly
    assert_eq!(run("fn() { 42 }()"), Value::Int(42));
}

#[test]
fn phase4_higher_order_with_closure_arg() {
    // map-like: apply a closure over a small array and sum
    let src = "apply_all := fn(arr, f) {
                 i := 0;
                 sum := 0;
                 while i < #arr { sum += f(arr[i]); i += 1 };
                 sum
               };
               apply_all([1, 2, 3, 4], fn(x) { x * x })";
    // 1 + 4 + 9 + 16 = 30
    assert_eq!(run(src), Value::Int(30));
}

// ---- Phase 5: ranges, for, for[], while[], break-with-value ----

#[test]
fn phase5_range_length_exclusive() {
    assert_eq!(run("#(0..10)"), Value::Int(10));
}

#[test]
fn phase5_range_length_inclusive() {
    assert_eq!(run("#(0..=10)"), Value::Int(11));
}

#[test]
fn phase5_range_length_with_step() {
    assert_eq!(run("#(0..10:2)"), Value::Int(5));
    assert_eq!(run("#(0..10:3)"), Value::Int(4));
    assert_eq!(run("#(0..=10:2)"), Value::Int(6));
}

#[test]
fn phase5_range_length_descending() {
    assert_eq!(run("#(10..0)"), Value::Int(10));    // auto step = -1
    assert_eq!(run("#(10..=0)"), Value::Int(11));
    assert_eq!(run("#(10..0:-2)"), Value::Int(5));
}

#[test]
fn phase5_range_length_empty_when_step_wrong_way() {
    assert_eq!(run("#(0..10:-1)"), Value::Int(0));
    assert_eq!(run("#(0..0)"), Value::Int(0));
}

#[test]
fn phase5_range_index() {
    assert_eq!(run("(0..10:2)[1]"), Value::Int(2));
    assert_eq!(run("(0..10)[5]"), Value::Int(5));
    assert_eq!(run("(0..10)[-1]"), Value::Int(9));
}

#[test]
fn phase5_range_truthiness() {
    assert_eq!(run("if 0..0 { 1 } else { 2 }"), Value::Int(2)); // empty → falsy
    assert_eq!(run("if 0..1 { 1 } else { 2 }"), Value::Int(1));
}

#[test]
fn phase5_for_range_sum() {
    let src = "sum := 0; for (i, 0..=10) { sum = sum + i }; sum";
    assert_eq!(run(src), Value::Int(55));
}

#[test]
fn phase5_for_returns_last_iter_value() {
    let src = "for (i, 0..5) { i * i }";
    assert_eq!(run(src), Value::Int(16));
}

#[test]
fn phase5_for_zero_iterations_is_null() {
    assert_eq!(run("for (i, 0..0) { i }"), Value::Null);
}

#[test]
fn phase5_for_array_collect() {
    let src = "for[] (i, 0..5) { i * i }";
    match run(src) {
        Value::Array(a) => {
            let v: Vec<i64> = a.borrow().iter().map(|x| match x {
                Value::Int(n) => *n,
                _ => panic!(),
            }).collect();
            assert_eq!(v, vec![0, 1, 4, 9, 16]);
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_for_array_collect_filters_null() {
    // for[] drops null values (spec §9.2/§9.4)
    let src = "for[] (i, 0..5) { if i % 2 == 0 { i } }";
    match run(src) {
        Value::Array(a) => {
            let v: Vec<i64> = a.borrow().iter().map(|x| match x {
                Value::Int(n) => *n,
                _ => panic!("got non-int in collected array: {x:?}"),
            }).collect();
            assert_eq!(v, vec![0, 2, 4]);
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_for_two_var_range_counter() {
    let src = "out := for[] (n, i, 10..15) { [n, i] }; out[2]";
    match run(src) {
        Value::Array(a) => {
            let inner = a.borrow();
            assert_eq!(inner.len(), 2);
            assert_eq!(inner[0], Value::Int(2));   // counter
            assert_eq!(inner[1], Value::Int(12));  // element
        }
        v => panic!("expected inner array, got {v:?}"),
    }
}

#[test]
fn phase5_for_over_array() {
    let src = "sum := 0; for (x, [10, 20, 30]) { sum = sum + x }; sum";
    assert_eq!(run(src), Value::Int(60));
}

#[test]
fn phase5_for_over_array_two_var() {
    let src = "for[] (i, x, [10, 20, 30]) { [i, x] }";
    match run(src) {
        Value::Array(a) => {
            assert_eq!(a.borrow().len(), 3);
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_for_over_object_one_var_gives_values() {
    let src = "sum := 0; for (v, ${a:1, b:2, c:3}) { sum = sum + v }; sum";
    assert_eq!(run(src), Value::Int(6));
}

#[test]
fn phase5_for_over_object_two_var_gives_key_value() {
    // Insertion-order: a/b/c. Collect keys.
    let src = "keys := for[] (k, v, ${a:1, b:2, c:3}) { k }; keys";
    match run(src) {
        Value::Array(a) => {
            let arr = a.borrow();
            let k = |i: usize| match &arr[i] {
                Value::Str(s) => s.to_string(),
                _ => panic!(),
            };
            assert_eq!(k(0), "a");
            assert_eq!(k(1), "b");
            assert_eq!(k(2), "c");
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_for_over_string() {
    let src = "out := for[] (ch, 'abc') { ch }; out";
    match run(src) {
        Value::Array(a) => {
            let arr = a.borrow();
            assert_eq!(arr.len(), 3);
            let s = |i: usize| match &arr[i] {
                Value::Str(s) => s.to_string(),
                _ => panic!(),
            };
            assert_eq!(s(0), "a");
            assert_eq!(s(1), "b");
            assert_eq!(s(2), "c");
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_for_iter_var_scoped_to_body() {
    // Spec §7.4: "Iteration variables are scoped to the loop body and
    // not visible after."
    let msg = run_err("for (i, 0..3) { i }; i");
    assert!(msg.contains("undeclared"), "got {msg}");
}

#[test]
fn phase5_for_of_closures_fresh_slot() {
    // The §10.4 worked example. Each iteration's `i` must be captured
    // in its own cell.
    let src = "adders := for[] (i, 0..3) { fn(x) { x + i } };
               [adders[0](10), adders[1](10), adders[2](10)]";
    match run(src) {
        Value::Array(a) => {
            let arr = a.borrow();
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Value::Int(10));
            assert_eq!(arr[1], Value::Int(11));
            assert_eq!(arr[2], Value::Int(12));
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_while_array_collects_iteration_values() {
    let src = "i := 0; while[] i < 5 { v := i; i = i + 1; v }";
    match run(src) {
        Value::Array(a) => {
            let v: Vec<i64> = a.borrow().iter().map(|x| match x {
                Value::Int(n) => *n,
                _ => panic!(),
            }).collect();
            assert_eq!(v, vec![0, 1, 2, 3, 4]);
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_break_exits_loop() {
    let src = "i := 0; while i < 100 { if i == 7 { break }; i = i + 1 }; i";
    assert_eq!(run(src), Value::Int(7));
}

#[test]
fn phase5_break_with_value_for() {
    let src = "for (i, 0..100) { if i == 5 { break i * 2 } }";
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase5_break_with_value_while() {
    let src = "i := 0; while true { i = i + 1; if i == 4 { break i + 100 } }";
    assert_eq!(run(src), Value::Int(104));
}

#[test]
fn phase5_chained_break_breaks_two_loops() {
    // Spec §9.4 worked example: chained break with value.
    let src = "for (i, 0..10) {
                   for (j, 0..10) {
                       if i * j == 25 { break (break [i, j]) }
                   }
               }";
    match run(src) {
        Value::Array(a) => {
            let arr = a.borrow();
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], Value::Int(5));
            assert_eq!(arr[1], Value::Int(5));
        }
        v => panic!("expected [5,5], got {v:?}"),
    }
}

#[test]
fn phase5_break_in_for_array_appends_value() {
    // §9.4: break v in for[] / while[] appends v if non-null.
    let src = "for[] (i, 0..10) { if i == 5 { break 99 }; i }";
    match run(src) {
        Value::Array(a) => {
            let v: Vec<i64> = a.borrow().iter().map(|x| match x {
                Value::Int(n) => *n,
                _ => panic!(),
            }).collect();
            // 0,1,2,3,4 collected, then break 99 appends 99, exits.
            assert_eq!(v, vec![0, 1, 2, 3, 4, 99]);
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_break_no_value_in_for_array_appends_nothing() {
    let src = "for[] (i, 0..10) { if i == 3 { break }; i }";
    match run(src) {
        Value::Array(a) => {
            let v: Vec<i64> = a.borrow().iter().map(|x| match x {
                Value::Int(n) => *n,
                _ => panic!(),
            }).collect();
            assert_eq!(v, vec![0, 1, 2]);
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_break_outside_loop_errors() {
    let msg = run_err("break 5");
    assert!(msg.contains("`break` outside"), "got {msg}");
}

#[test]
fn phase5_return_inside_for_closes_upvalues() {
    // Return from a function while inside a for loop. The captured
    // closure should still see the right `i`.
    let src = "f := fn() {
                   captured := null;
                   for (i, 0..5) {
                       if i == 2 { captured = fn() { i } }
                   };
                   captured()
               };
               f()";
    assert_eq!(run(src), Value::Int(2));
}

#[test]
fn phase5_nested_for_returns_inner_array() {
    let src = "for[] (i, 0..3) { for[] (j, 0..3) { i * 10 + j } }";
    match run(src) {
        Value::Array(a) => {
            let outer = a.borrow();
            assert_eq!(outer.len(), 3);
            // first row [0, 1, 2]
            match &outer[0] {
                Value::Array(inner) => {
                    let v: Vec<i64> = inner.borrow().iter().map(|x| match x {
                        Value::Int(n) => *n, _ => panic!(),
                    }).collect();
                    assert_eq!(v, vec![0, 1, 2]);
                }
                _ => panic!(),
            }
        }
        v => panic!("expected array, got {v:?}"),
    }
}

#[test]
fn phase5_range_in_array_via_index() {
    // Ranges don't materialize; indexing yields the right element
    let src = "r := 100..200:10; r[3]";
    assert_eq!(run(src), Value::Int(130));
}

// ---- Phase 6: built-ins, pipe, spread, string interpolation ----

#[test]
fn phase6_int_truncates() {
    assert_eq!(run("int(3.7)"), Value::Int(3));
    assert_eq!(run("int(-3.7)"), Value::Int(-3));
    assert_eq!(run("int('42')"), Value::Int(42));
    assert_eq!(run("int(true)"), Value::Int(1));
}

#[test]
fn phase6_float_coerces() {
    assert_eq!(run("float(3)"), Value::Float(3.0));
    assert_eq!(run("float('3.14')"), Value::Float(3.14));
}

#[test]
fn phase6_num_parses_or_passes_through() {
    assert_eq!(run("num(42)"), Value::Int(42));
    assert_eq!(run("num('3.5')"), Value::Float(3.5));
    assert_eq!(run("num('-10')"), Value::Int(-10));
    assert_eq!(run("num('not a number')"), Value::Null);
}

#[test]
fn phase6_bool_uses_truthiness() {
    assert_eq!(run("bool(0)"), Value::Bool(false));
    assert_eq!(run("bool('hi')"), Value::Bool(true));
    assert_eq!(run("bool([])"), Value::Bool(false));
    assert_eq!(run("bool(${a:1})"), Value::Bool(true));
}

#[test]
fn phase6_floor_ceil() {
    assert_eq!(run("floor(3.7)"), Value::Int(3));
    assert_eq!(run("floor(-3.2)"), Value::Int(-4));
    assert_eq!(run("ceil(3.2)"), Value::Int(4));
    assert_eq!(run("ceil(-3.7)"), Value::Int(-3));
}

#[test]
fn phase6_rand_in_range() {
    // `rand()` returns Float in [0, 1). Sample twice and verify the
    // range; we don't pin a seed so we just sanity-check.
    for _ in 0..20 {
        match run("rand()") {
            Value::Float(x) => assert!(x >= 0.0 && x < 1.0, "got {x}"),
            v => panic!("expected float, got {v:?}"),
        }
    }
}

#[test]
fn phase6_builtins_are_first_class() {
    // Spec §13: built-ins are bindings. Can be stored, passed, etc.
    let src = "f := floor; f(7.9)";
    assert_eq!(run(src), Value::Int(7));
}

#[test]
fn phase6_builtins_shadowable_via_decl() {
    let src = "print := fn(x) { x + 1 }; print(5)";
    assert_eq!(run(src), Value::Int(6));
}

#[test]
fn phase6_pipe_one_arg() {
    let src = "double := fn(x) { x * 2 }; 5 |> double";
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase6_pipe_with_extra_args() {
    let src = "scale := fn(x, k) { x * k }; 5 |> scale(3)";
    assert_eq!(run(src), Value::Int(15));
}

#[test]
fn phase6_pipe_chain() {
    let src = "double := fn(x) { x * 2 };
               plus  := fn(x, k) { x + k };
               1 |> double |> plus(3) |> double";
    // 1 → 2 → 5 → 10
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase6_pipe_into_builtin() {
    assert_eq!(run("3.7 |> floor"), Value::Int(3));
    assert_eq!(run("3.7 |> floor()"), Value::Int(3));
}

#[test]
fn phase6_array_spread_concat() {
    let src = "a := [1, 2]; b := [3, 4]; [...a, ...b]";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 2, 3, 4]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_array_spread_mixed() {
    let src = "rest := [2, 3, 4]; [1, ...rest, 5]";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 2, 3, 4, 5]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_array_spread_range() {
    let src = "[...0..5]";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![0, 1, 2, 3, 4]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_array_spread_string() {
    let src = "[...'abc']";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<String> = arr.borrow().iter().map(|x| match x {
                Value::Str(s) => s.to_string(), _ => panic!(),
            }).collect();
            assert_eq!(v, vec!["a", "b", "c"]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_object_spread_later_wins() {
    // `${...defaults, color: 'red'}` — explicit key overrides spread.
    let src = "defaults := ${color: 'blue', size: 10};
               style := ${...defaults, color: 'red'};
               style.color";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "red"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_object_spread_keeps_other_keys() {
    let src = "defaults := ${color: 'blue', size: 10};
               style := ${...defaults, color: 'red'};
               style.size";
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn phase6_object_spread_preserves_order() {
    let src = "a := ${x:1, y:2};
               merged := ${...a, z:3};
               keys := for[] (k, v, merged) { k };
               keys";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<String> = arr.borrow().iter().map(|x| match x {
                Value::Str(s) => s.to_string(), _ => panic!(),
            }).collect();
            assert_eq!(v, vec!["x", "y", "z"]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_call_spread() {
    let src = "add3 := fn(a, b, c) { a + b + c };
               args := [10, 20, 30];
               add3(...args)";
    assert_eq!(run(src), Value::Int(60));
}

#[test]
fn phase6_call_spread_mixed() {
    let src = "f := fn(a, b, c, d) { [a, b, c, d] };
               mid := [2, 3];
               f(1, ...mid, 4)";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 2, 3, 4]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_simple() {
    let src = "name := 'world'; 'hello, {name}!'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "hello, world!"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_expression() {
    let src = "a := 3; b := 4; 'sum: {a + b}'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "sum: 7"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_index_access() {
    let src = "arr := [10, 20, 30]; 'first: {arr[0]}'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "first: 10"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_escape_brace() {
    // \{ is a literal brace
    let src = "'literal: \\{x}'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "literal: {x}"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_nested_string() {
    // Per spec §8.2: nested strings inside interpolation are allowed.
    let src = "cond := true; '{ if cond { 'yes' } else { 'no' } }'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "yes"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_doubly_nested() {
    // A nested string with its own interpolation
    let src = "x := 7; '{ 'inner={x}' }'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "inner=7"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_string_interp_with_call_and_pipe() {
    // Combine multiple phase-6 features
    let src = "name := 'tigr'; nums := [1, 2, 3]; 'hi {name}, len={#[0, ...nums, 4]}'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "hi tigr, len=5"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase6_str_of_number_no_trailing_dot_on_int() {
    let src = "'{42} vs {3.5}'";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "42 vs 3.5"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase5_pe004() {
    // Largest palindrome product of two 3-digit numbers — Project
    // Euler 4. Same answer as v0.1.
    let src = r#"
        is_palindrome := fn(n) {
            s := str(n);
            i := 0;
            j := #s - 1;
            result := true;
            while i < j {
                if s[i] != s[j] { result = false; i = j } else { i = i + 1; j = j - 1 }
            };
            result
        };
        best := 0;
        for (a, 100..=999) {
            for (b, a..=999) {
                p := a * b;
                if p > best {
                    if is_palindrome(p) { best = p }
                }
            }
        };
        best
    "#;
    assert_eq!(run(src), Value::Int(906609));
}

// ---- Phase 7: destructuring patterns, rest params, imports ----

#[test]
fn phase7_array_pattern_basic() {
    let src = "[a, b, c] := [1, 2, 3]; [a, b, c]";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 2, 3]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_array_pattern_rest() {
    let src = "[head, ...rest] := [1, 2, 3, 4]; [head, #rest, rest[0], rest[2]]";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 3, 2, 4]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_array_pattern_wildcard() {
    let src = "[a, _, c] := [1, 2, 3]; [a, c]";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 3]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_array_pattern_missing_is_null() {
    let src = "[a, b, c] := [1]; [a, b == null, c == null]";
    match run(src) {
        Value::Array(arr) => {
            let inner = arr.borrow();
            assert_eq!(inner[0], Value::Int(1));
            assert_eq!(inner[1], Value::Bool(true));
            assert_eq!(inner[2], Value::Bool(true));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_object_pattern_shorthand() {
    let src = "${name, age} := ${name: 'tigr', age: 0, extra: true};
               [name, age]";
    match run(src) {
        Value::Array(arr) => {
            let inner = arr.borrow();
            match &inner[0] {
                Value::Str(s) => assert_eq!(&**s, "tigr"),
                _ => panic!(),
            }
            assert_eq!(inner[1], Value::Int(0));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_object_pattern_rename() {
    let src = "${name: n, age: a} := ${name: 'tigr', age: 99};
               [n, a]";
    match run(src) {
        Value::Array(arr) => {
            let inner = arr.borrow();
            match &inner[0] {
                Value::Str(s) => assert_eq!(&**s, "tigr"),
                _ => panic!(),
            }
            assert_eq!(inner[1], Value::Int(99));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_object_pattern_rest() {
    let src = "${name, ...rest} := ${name: 'tigr', a: 1, b: 2};
               keys := for[] (k, v, rest) { k };
               keys";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<String> = arr.borrow().iter().map(|x| match x {
                Value::Str(s) => s.to_string(), _ => panic!(),
            }).collect();
            assert_eq!(v, vec!["a", "b"]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_nested_patterns_array_of_objects() {
    let src = "[${name: n1}, ${name: n2}] := [${name: 'a'}, ${name: 'b'}];
               [n1, n2]";
    match run(src) {
        Value::Array(arr) => {
            let inner = arr.borrow();
            match (&inner[0], &inner[1]) {
                (Value::Str(a), Value::Str(b)) => {
                    assert_eq!(&**a, "a");
                    assert_eq!(&**b, "b");
                }
                _ => panic!(),
            }
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_nested_object_of_object() {
    let src = "${user: ${id, name}} := ${user: ${id: 7, name: 'tigr'}};
               [id, name]";
    match run(src) {
        Value::Array(arr) => {
            let inner = arr.borrow();
            assert_eq!(inner[0], Value::Int(7));
            match &inner[1] {
                Value::Str(s) => assert_eq!(&**s, "tigr"),
                _ => panic!(),
            }
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_pattern_in_fn_params_array() {
    let src = "f := fn([a, b]) { a + b };
               f([10, 20])";
    assert_eq!(run(src), Value::Int(30));
}

#[test]
fn phase7_pattern_in_fn_params_object() {
    let src = "f := fn(${name}) { name };
               f(${name: 'tigr', extra: 1})";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "tigr"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_fn_rest_collects_extras() {
    let src = "f := fn(a, ...rest) { [a, #rest, rest[0], rest[1]] };
               f(1, 2, 3, 4)";
    match run(src) {
        Value::Array(arr) => {
            let v: Vec<i64> = arr.borrow().iter().map(|x| match x {
                Value::Int(n) => *n, _ => panic!(),
            }).collect();
            assert_eq!(v, vec![1, 3, 2, 3]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_fn_rest_zero_extras() {
    let src = "f := fn(a, ...rest) { [a, #rest] };
               f(1)";
    match run(src) {
        Value::Array(arr) => {
            let inner = arr.borrow();
            assert_eq!(inner[0], Value::Int(1));
            assert_eq!(inner[1], Value::Int(0));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_fn_rest_with_spread_call() {
    let src = "f := fn(...args) { #args };
               args := [1, 2, 3, 4, 5];
               f(...args)";
    assert_eq!(run(src), Value::Int(5));
}

#[test]
fn phase7_combined_sample() {
    // Plan's worked sample.
    let src = "
        [a, b, ...rest] := [1, 2, 3, 4, 5];
        ${name, age} := ${name: 'tigr', age: 0, extra: true};
        'name={name} a={a} rest_len={#rest}'
    ";
    match run(src) {
        Value::Str(s) => assert_eq!(&*s, "name=tigr a=1 rest_len=3"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn phase7_invalid_pattern_errors() {
    // [1, a] := [...] — 1 isn't a valid pattern element
    let msg = run_err("[1, a] := [1, 2]");
    assert!(msg.contains("pattern") || msg.contains("invalid"), "got {msg}");
}

#[test]
fn phase7_rest_not_last_errors() {
    let msg = run_err("[a, ...rest, b] := [1, 2, 3]");
    assert!(msg.contains("last") || msg.contains("rest"), "got {msg}");
}

// ---- v0.3: try / catch / raise ----

#[test]
fn v03_try_success_no_catch() {
    // No error → try evaluates to the body's value.
    assert_eq!(run("try (1 + 2)"), Value::Int(3));
}

#[test]
fn v03_try_no_catch_swallows_error() {
    // Builtin error (cannot convert) → caught silently as null.
    assert_eq!(run("try num([1, 2])"), Value::Null);
}

#[test]
fn v03_try_catch_binds_error_message() {
    // Catch handler runs with the error message bound to `e`.
    let src = "try (raise 'oops') catch (e) { 'got: ' + e }";
    assert_eq!(run(src), Value::Str("got: oops".into()));
}

#[test]
fn v03_try_catch_runtime_error() {
    // Built-in TypeMismatch reaches the catch with the message
    // produced by RuntimeError::Display ("type mismatch: ...").
    let src = "try int([1]) catch (e) { e }";
    match run(src) {
        Value::Str(s) => assert!(s.contains("int()") || s.contains("type"), "got {s}"),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_try_in_decl() {
    let src = "
        x := try (raise 'fail') catch (e) { 42 };
        x
    ";
    assert_eq!(run(src), Value::Int(42));
}

#[test]
fn v03_raise_from_nested_fn() {
    // Raise inside a function called from try → caught in outer frame.
    let src = "
        boom := fn() { raise 'inner' };
        try boom() catch (e) { 'caught: ' + e }
    ";
    assert_eq!(run(src), Value::Str("caught: inner".into()));
}

#[test]
fn v03_uncaught_raise_propagates() {
    // No try → error surfaces with the raised message.
    let msg = run_err("raise 'bang'");
    assert!(msg.contains("bang"), "got {msg}");
}

#[test]
fn v03_nested_try_inner_catches() {
    let src = "
        try { try (raise 'inner') catch (e) { 'inner-handled:' + e } }
        catch (e2) { 'outer-handled:' + e2 }
    ";
    assert_eq!(run(src), Value::Str("inner-handled:inner".into()));
}

#[test]
fn v03_nested_try_outer_catches_when_inner_reraises() {
    let src = "
        try { try (raise 'a') catch (e) { raise ('re:' + e) } }
        catch (e2) { 'outer:' + e2 }
    ";
    assert_eq!(run(src), Value::Str("outer:re:a".into()));
}

#[test]
fn v03_try_inside_loop_preserves_break() {
    // try inside a for body shouldn't disturb break-with-value.
    let src = "
        for (i, 1..=10) {
            try (if i == 3 { raise 'stop' }) catch (e) {
                break i * 100
            }
        }
    ";
    assert_eq!(run(src), Value::Int(300));
}

#[test]
fn v03_try_in_expression_position() {
    // try composes with || for default-on-error.
    let src = "(try (raise 'no') || 'default') + '!'";
    assert_eq!(run(src), Value::Str("default!".into()));
}

#[test]
fn v03_raise_non_string_coerces() {
    // Non-string raises stringify via Display.
    let src = "try (raise 42) catch (e) { e }";
    assert_eq!(run(src), Value::Str("42".into()));
}

#[test]
fn v03_try_closure_captures() {
    // Catch param can be captured by an inner closure (open upvalue
    // closes correctly when scope ends).
    let src = "
        get_handler := fn() {
            try (raise 'captured') catch (e) { fn() { e } }
        };
        h := get_handler();
        h()
    ";
    assert_eq!(run(src), Value::Str("captured".into()));
}

#[test]
fn v03_try_inside_call_args() {
    // try expressions work as function call arguments.
    let src = "
        identity := fn(x) { x };
        identity(try (raise 'arg') catch (e) { e })
    ";
    assert_eq!(run(src), Value::Str("arg".into()));
}

// ---- v0.3 Phase 5: REPL ----
//
// These exercise the `Repl::eval` API directly — the IO loop in
// `Repl::run` is harder to unit-test cleanly. The session-state
// behaviour (locals persisting, errors not killing state, closures
// sharing upvalues across lines) is what matters; the IO loop just
// pipes stdin to `eval`.

#[test]
fn v03_repl_persists_locals() {
    let mut repl = crate::repl::Repl::new();
    assert_eq!(repl.eval("x := 5").unwrap(), Value::Int(5));
    assert_eq!(repl.eval("x * 2").unwrap(), Value::Int(10));
}

#[test]
fn v03_repl_multiple_declarations() {
    let mut repl = crate::repl::Repl::new();
    repl.eval("a := 3").unwrap();
    repl.eval("b := 4").unwrap();
    assert_eq!(repl.eval("a + b").unwrap(), Value::Int(7));
}

#[test]
fn v03_repl_error_preserves_state() {
    let mut repl = crate::repl::Repl::new();
    repl.eval("keep := 42").unwrap();
    assert!(repl.eval("raise 'boom'").is_err());
    // State must survive the uncaught raise.
    assert_eq!(repl.eval("keep").unwrap(), Value::Int(42));
}

#[test]
fn v03_repl_error_discards_partial_decl() {
    let mut repl = crate::repl::Repl::new();
    // Decl succeeds, then raise — but the line errored mid-way after
    // declaring; the REPL should NOT commit `partial` to its state.
    let _ = repl.eval("partial := 99; raise 'mid'");
    // Referencing `partial` should now be an undeclared variable.
    assert!(repl.eval("partial").is_err());
}

#[test]
fn v03_repl_closures_share_upvalues() {
    let mut repl = crate::repl::Repl::new();
    repl.eval("n := 0").unwrap();
    repl.eval("inc := fn() { n = n + 1; n }").unwrap();
    repl.eval("read := fn() { n }").unwrap();
    assert_eq!(repl.eval("inc()").unwrap(), Value::Int(1));
    assert_eq!(repl.eval("inc()").unwrap(), Value::Int(2));
    // The closure captured at "inc" definition time sees the SAME `n`
    // as the closure captured later at "read" definition.
    assert_eq!(repl.eval("read()").unwrap(), Value::Int(2));
    // Mutating directly is also visible through the closures.
    repl.eval("n = 100").unwrap();
    assert_eq!(repl.eval("read()").unwrap(), Value::Int(100));
}

#[test]
fn v03_repl_counter_closure_persists() {
    let mut repl = crate::repl::Repl::new();
    repl.eval(
        "make := fn() { n := 0; fn() { n = n + 1; n } }"
    ).unwrap();
    repl.eval("c := make()").unwrap();
    assert_eq!(repl.eval("c()").unwrap(), Value::Int(1));
    assert_eq!(repl.eval("c()").unwrap(), Value::Int(2));
    assert_eq!(repl.eval("c()").unwrap(), Value::Int(3));
}

#[test]
fn v03_repl_stdlib_import() {
    let mut repl = crate::repl::Repl::new();
    repl.eval("Array := import 'Array'").unwrap();
    assert_eq!(repl.eval("Array.sum([1, 2, 3])").unwrap(), Value::Int(6));
}

#[test]
fn v03_repl_try_catch_works() {
    let mut repl = crate::repl::Repl::new();
    let v = repl.eval("try (raise 'msg') catch (e) { 'got: ' + e }").unwrap();
    assert_eq!(v, Value::Str("got: msg".into()));
}

#[test]
fn v03_repl_redeclare_shadows() {
    // Re-declaring a name in the REPL adds a NEW local at a new slot.
    // Per spec §4.1, `:=` in the same scope of the same name is a
    // DuplicateDeclaration error — and that's what should happen here
    // (REPL session is one scope).
    let mut repl = crate::repl::Repl::new();
    repl.eval("x := 1").unwrap();
    assert!(repl.eval("x := 2").is_err());
    // Original still works.
    assert_eq!(repl.eval("x").unwrap(), Value::Int(1));
}

// ---- v0.3 Phase 4: source stdlib (Array / String / Math) ----

#[test]
fn v03_array_map() {
    let src = "
        Array := import 'Array';
        Array.map([1, 2, 3], fn(x) { x * x })
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b.len(), 3);
            assert_eq!(b[0], Value::Int(1));
            assert_eq!(b[1], Value::Int(4));
            assert_eq!(b[2], Value::Int(9));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_array_filter() {
    let src = "
        Array := import 'Array';
        Array.filter([1, 2, 3, 4, 5], fn(x) { x % 2 == 0 })
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b.len(), 2);
            assert_eq!(b[0], Value::Int(2));
            assert_eq!(b[1], Value::Int(4));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_array_reduce() {
    let src = "
        Array := import 'Array';
        Array.reduce([1, 2, 3, 4], fn(acc, x) { acc + x }, 0)
    ";
    assert_eq!(run(src), Value::Int(10));
}

#[test]
fn v03_array_reduce_empty_returns_seed() {
    let src = "
        Array := import 'Array';
        Array.reduce([], fn(acc, x) { acc + x }, 42)
    ";
    assert_eq!(run(src), Value::Int(42));
}

#[test]
fn v03_array_find_returns_first_match() {
    let src = "
        Array := import 'Array';
        Array.find([1, 2, 3, 4], fn(x) { x > 2 })
    ";
    assert_eq!(run(src), Value::Int(3));
}

#[test]
fn v03_array_find_no_match_returns_null() {
    let src = "
        Array := import 'Array';
        Array.find([1, 2, 3], fn(x) { x > 99 })
    ";
    assert_eq!(run(src), Value::Null);
}

#[test]
fn v03_array_any_all() {
    let src = "
        Array := import 'Array';
        [
            Array.any([1, 2, 3], fn(x) { x > 2 }),
            Array.any([1, 2, 3], fn(x) { x > 99 }),
            Array.all([2, 4, 6], fn(x) { x % 2 == 0 }),
            Array.all([2, 4, 5], fn(x) { x % 2 == 0 })
        ]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Bool(true));
            assert_eq!(b[1], Value::Bool(false));
            assert_eq!(b[2], Value::Bool(true));
            assert_eq!(b[3], Value::Bool(false));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_array_sort() {
    let src = "
        Array := import 'Array';
        Array.sort([3, 1, 4, 1, 5, 9, 2, 6])
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            let nums: Vec<i64> = b.iter()
                .map(|v| if let Value::Int(n) = v { *n } else { panic!() })
                .collect();
            assert_eq!(nums, vec![1, 1, 2, 3, 4, 5, 6, 9]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_array_uniq() {
    let src = "
        Array := import 'Array';
        Array.uniq([1, 2, 2, 3, 1, 4, 3])
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            let nums: Vec<i64> = b.iter()
                .map(|v| if let Value::Int(n) = v { *n } else { panic!() })
                .collect();
            assert_eq!(nums, vec![1, 2, 3, 4]);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_array_zip_min_length() {
    let src = "
        Array := import 'Array';
        Array.zip([1, 2, 3], ['a', 'b'])
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b.len(), 2);
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_array_join() {
    let src = "
        Array := import 'Array';
        Array.join([1, 2, 3], ', ')
    ";
    assert_eq!(run(src), Value::Str("1, 2, 3".into()));
}

#[test]
fn v03_string_split_join() {
    let src = "
        String := import 'String';
        parts := String.split('a,b,c,d', ',');
        String.join(parts, '-')
    ";
    assert_eq!(run(src), Value::Str("a-b-c-d".into()));
}

#[test]
fn v03_string_replace() {
    let src = "
        String := import 'String';
        String.replace('hello world', 'world', 'tigr')
    ";
    assert_eq!(run(src), Value::Str("hello tigr".into()));
}

#[test]
fn v03_string_predicates() {
    let src = "
        S := import 'String';
        [
            S.contains('hello world', 'world'),
            S.starts_with('hello world', 'hello'),
            S.ends_with('hello world', 'world'),
            S.contains('hello', 'xyz')
        ]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Bool(true));
            assert_eq!(b[1], Value::Bool(true));
            assert_eq!(b[2], Value::Bool(true));
            assert_eq!(b[3], Value::Bool(false));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_string_case() {
    let src = "
        S := import 'String';
        [S.upper('hello'), S.lower('HELLO')]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Str("HELLO".into()));
            assert_eq!(b[1], Value::Str("hello".into()));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_string_trim_pad() {
    let src = "
        S := import 'String';
        [S.trim('  hi  '), S.pad_start('5', 3, '0'), S.pad_end('hi', 5, '.')]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Str("hi".into()));
            assert_eq!(b[1], Value::Str("005".into()));
            assert_eq!(b[2], Value::Str("hi...".into()));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_math_sqrt_pi() {
    let src = "
        Math := import 'Math';
        [Math.sqrt(4), Math.PI > 3.14, Math.PI < 3.15]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Float(2.0));
            assert_eq!(b[1], Value::Bool(true));
            assert_eq!(b[2], Value::Bool(true));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_math_abs_sign_clamp() {
    let src = "
        Math := import 'Math';
        [Math.abs(-7), Math.sign(-3), Math.sign(0), Math.sign(5), Math.clamp(15, 0, 10)]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Int(7));
            assert_eq!(b[1], Value::Int(-1));
            assert_eq!(b[2], Value::Int(0));
            assert_eq!(b[3], Value::Int(1));
            assert_eq!(b[4], Value::Int(10));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_math_min_max() {
    let src = "
        Math := import 'Math';
        [Math.min(3, 7), Math.max(3, 7)]
    ";
    match run(src) {
        Value::Array(a) => {
            let b = a.borrow();
            assert_eq!(b[0], Value::Int(3));
            assert_eq!(b[1], Value::Int(7));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_stdlib_modules_cached() {
    // Same import twice → same object reference (== compares
    // structurally for objects, so this checks identity-of-content
    // at minimum).
    let src = "
        a := import 'Array';
        b := import 'Array';
        a == b
    ";
    assert_eq!(run(src), Value::Bool(true));
}

// ---- v0.3 Phase 3: native modules (IO / Os / Time) ----

#[test]
fn v03_io_write_then_read_roundtrip() {
    let dir = std::env::temp_dir().join(format!(
        "tigr_v03_io_rw_{}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.txt");
    let path_str = path.to_string_lossy().to_string();
    let src = format!(
        "IO := import 'IO';
         IO.write_file('{path}', 'roundtrip');
         IO.read_file('{path}')",
        path = path_str
    );
    assert_eq!(run(&src), Value::Str("roundtrip".into()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v03_io_exists_for_missing_returns_false() {
    let src = "IO := import 'IO'; IO.exists('/definitely/not/a/path/xyz123')";
    assert_eq!(run(src), Value::Bool(false));
}

#[test]
fn v03_io_read_missing_raises_catchable() {
    let src = "
        IO := import 'IO';
        try IO.read_file('/definitely/not/a/path/xyz123')
        catch (e) { 'caught' }
    ";
    assert_eq!(run(src), Value::Str("caught".into()));
}

#[test]
fn v03_io_module_is_cached() {
    // Two `import 'IO'` calls hand back the same Object.
    let src = "
        a := import 'IO';
        b := import 'IO';
        a == b
    ";
    assert_eq!(run(src), Value::Bool(true));
}

#[test]
fn v03_os_args_is_array() {
    // The actual values depend on the test runner; just verify the
    // shape: an Array of strings, length > 0 (always includes argv[0]).
    let src = "
        Os := import 'Os';
        [#Os.args > 0, Os.args[0]]
    ";
    let v = run(src);
    match v {
        Value::Array(arr) => {
            let b = arr.borrow();
            assert_eq!(b[0], Value::Bool(true));
            assert!(matches!(b[1], Value::Str(_)));
        }
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v03_os_env_missing_is_null() {
    let src = "
        Os := import 'Os';
        Os.env('TIGR_DEFINITELY_NOT_SET_VAR_98765')
    ";
    assert_eq!(run(src), Value::Null);
}

#[test]
fn v03_os_env_present() {
    // PATH is essentially always set; if not, this would fail
    // (acceptable for a hobby-lang test suite).
    std::env::set_var("TIGR_TEST_VAR_PRESENT", "yes");
    let src = "
        Os := import 'Os';
        Os.env('TIGR_TEST_VAR_PRESENT')
    ";
    assert_eq!(run(src), Value::Str("yes".into()));
    std::env::remove_var("TIGR_TEST_VAR_PRESENT");
}

#[test]
fn v03_os_cwd_non_empty() {
    let src = "Os := import 'Os'; #Os.cwd() > 0";
    assert_eq!(run(src), Value::Bool(true));
}

#[test]
fn v03_time_now_ms_is_int() {
    let src = "Time := import 'Time'; Time.now_ms() > 0";
    assert_eq!(run(src), Value::Bool(true));
}

#[test]
fn v03_time_now_ms_monotonic() {
    // Two reads — the second should be ≥ the first.
    let src = "
        Time := import 'Time';
        a := Time.now_ms();
        b := Time.now_ms();
        b >= a
    ";
    assert_eq!(run(src), Value::Bool(true));
}

// ---- v0.3 Phase 2: module caching + native dispatch ----

#[test]
fn v03_import_cached_returns_same_object() {
    // Two imports of the same path should yield the same Object
    // (reference shared via cache, not re-evaluated). Mutating one
    // is visible through the other.
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!(
        "tigr_v03_cache_{}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mod_path = dir.join("counter.tg");
    {
        let mut f = std::fs::File::create(&mod_path).unwrap();
        writeln!(f, "${{count: 0}}").unwrap();
    }
    let main_path = dir.join("main.tg");
    {
        let mut f = std::fs::File::create(&main_path).unwrap();
        writeln!(f,
            "m1 := import './counter';
             m2 := import './counter';
             m1.count = 7;
             m2.count").unwrap();
    }
    let value = crate::vm::run_file(&main_path).unwrap();
    assert_eq!(value, Value::Int(7));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v03_import_circular_is_catchable() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!(
        "tigr_v03_cycle_{}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let mut f = std::fs::File::create(dir.join("a.tg")).unwrap();
        writeln!(f, "b := import './b'; 'a-done'").unwrap();
    }
    {
        let mut f = std::fs::File::create(dir.join("b.tg")).unwrap();
        writeln!(f, "a := import './a'; 'b-done'").unwrap();
    }
    let main_path = dir.join("main.tg");
    {
        let mut f = std::fs::File::create(&main_path).unwrap();
        writeln!(f,
            "try import './a' catch (e) {{ if #e > 0 {{ 'caught' }} else {{ e }} }}"
        ).unwrap();
    }
    let value = crate::vm::run_file(&main_path).unwrap();
    assert_eq!(value, Value::Str("caught".into()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v03_import_bare_unknown_is_catchable() {
    // No native module is registered yet; bare-name import should
    // raise a catchable error.
    let src = "try import 'NotARealModule' catch (e) { 'failed' }";
    assert_eq!(run(src), Value::Str("failed".into()));
}

#[test]
fn v03_import_missing_file_is_catchable() {
    let src = "try import './definitely_does_not_exist' catch (e) { 'missing' }";
    assert_eq!(run(src), Value::Str("missing".into()));
}

#[test]
fn phase7_import_file() {
    // Write a temp module and import it.
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!(
        "tigr_phase7_{}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mod_path = dir.join("test_mod.tg");
    {
        let mut f = std::fs::File::create(&mod_path).unwrap();
        writeln!(f, "${{double: fn(x) {{ x * 2 }}, name: 'mod'}}").unwrap();
    }

    let main_path = dir.join("main.tg");
    {
        let mut f = std::fs::File::create(&main_path).unwrap();
        writeln!(f, "m := import './test_mod'; m.double(21)").unwrap();
    }

    let value = crate::vm::run_file(&main_path).unwrap();
    assert_eq!(value, Value::Int(42));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---- v0.4 Phase 1: rendered errors ----

#[test]
fn v04_render_runtime_division_by_zero() {
    let out = render_err("x := 10;\ny := x / 0;\ny");
    assert!(out.contains("error[runtime]: division by zero"), "got:\n{out}");
    assert!(out.contains("<string>:2"), "no filename:line — got:\n{out}");
    assert!(out.contains("y := x / 0"), "missing source line — got:\n{out}");
}

#[test]
fn v04_render_parse_unexpected_token() {
    // `x := := 5` — second `:=` is unexpected.
    let out = render_err("x := := 5");
    assert!(out.contains("error[parse]"), "got:\n{out}");
    assert!(out.contains("<string>:1"), "missing filename:line — got:\n{out}");
    // Caret line should contain at least one `^`.
    assert!(out.lines().any(|l| l.trim_start_matches(' ').starts_with("| ") &&
        l.contains('^')), "missing caret — got:\n{out}");
}

#[test]
fn v04_render_error_in_imported_file() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("tigr_v04_render_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad_path = dir.join("bad.tg");
    {
        let mut f = std::fs::File::create(&bad_path).unwrap();
        // line 1 a comment, line 2 the divide
        writeln!(f, "// inner module").unwrap();
        writeln!(f, "10 / 0").unwrap();
    }
    let main_path = dir.join("main.tg");
    {
        let mut f = std::fs::File::create(&main_path).unwrap();
        writeln!(f, "import './bad'").unwrap();
    }
    let sources = std::rc::Rc::new(std::cell::RefCell::new(
        crate::vm::source_map::SourceMap::new(),
    ));
    let result = crate::vm::run_file_with_map(&main_path, sources.clone());
    let out = match result {
        Ok((v, _)) => panic!("expected error, got {v:?}"),
        Err(e) => e.render(&sources.borrow()),
    };
    assert!(
        out.contains("bad.tg") && out.contains(":2"),
        "expected snippet to point at bad.tg:2 — got:\n{out}",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- v0.4 Phase 4: JSON native module ----

#[test]
fn v04_json_parse_primitives() {
    assert_eq!(run("JSON := import 'JSON'; JSON.parse('null')"), Value::Null);
    assert_eq!(run("JSON := import 'JSON'; JSON.parse('true')"), Value::Bool(true));
    assert_eq!(run("JSON := import 'JSON'; JSON.parse('false')"), Value::Bool(false));
    match run("JSON := import 'JSON'; JSON.parse('0')") {
        Value::Float(x) => assert_eq!(x, 0.0),
        v => panic!("got {v:?}"),
    }
    match run("JSON := import 'JSON'; JSON.parse('-1.5e2')") {
        Value::Float(x) => assert!((x + 150.0).abs() < 1e-9),
        v => panic!("got {v:?}"),
    }
    assert_eq!(
        run(r#"JSON := import 'JSON'; JSON.parse('"hi"')"#),
        Value::Str("hi".into()),
    );
}

#[test]
fn v04_json_parse_array() {
    let v = run("JSON := import 'JSON'; JSON.parse('[1, 2, 3]')");
    let arr = match v {
        Value::Array(a) => a,
        v => panic!("got {v:?}"),
    };
    let arr = arr.borrow();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0], Value::Float(1.0));
    assert_eq!(arr[2], Value::Float(3.0));
}

#[test]
fn v04_json_parse_object_preserves_key_order() {
    // Build via JSON.parse, then walk via for(k, obj) which iterates
    // in insertion order. Tigr `'..'` strings interpolate on `{`, so
    // the literal JSON `{` is escaped as `\{`.
    let v = run(
        r#"JSON := import 'JSON';
           obj := JSON.parse('\{"z": 1, "a": 2, "m": 3}');
           ks := [];
           for (k, _, obj) { ks = ks + [k] };
           ks"#
    );
    let arr = match v {
        Value::Array(a) => a,
        v => panic!("got {v:?}"),
    };
    let arr = arr.borrow();
    assert_eq!(*arr, vec![
        Value::Str("z".into()),
        Value::Str("a".into()),
        Value::Str("m".into()),
    ]);
}

#[test]
fn v04_json_parse_string_escapes() {
    // The `\\` in tigr source produces a literal `\`, which combines
    // with the next char to form a JSON escape. So `\\n` here is
    // tigr-escaped to `\n` (backslash + n), which JSON decodes to a
    // real newline.
    let v = run(
        r#"JSON := import 'JSON';
           JSON.parse('"a\\nb\\tc\\"d\\\\eé"')"#
    );
    assert_eq!(v, Value::Str("a\nb\tc\"d\\eé".into()));
}

#[test]
fn v04_json_parse_malformed_raises_catchable() {
    let v = run(
        r#"JSON := import 'JSON';
           try JSON.parse('\{') catch (e) { 'caught' }"#
    );
    assert_eq!(v, Value::Str("caught".into()));
}

#[test]
fn v04_json_parse_trailing_content_raises() {
    let v = run(
        r#"JSON := import 'JSON';
           try JSON.parse('1 2') catch (e) { 'caught' }"#
    );
    assert_eq!(v, Value::Str("caught".into()));
}

#[test]
fn v04_json_stringify_primitives() {
    assert_eq!(
        run("JSON := import 'JSON'; JSON.stringify(null)"),
        Value::Str("null".into()),
    );
    assert_eq!(
        run("JSON := import 'JSON'; JSON.stringify(true)"),
        Value::Str("true".into()),
    );
    assert_eq!(
        run("JSON := import 'JSON'; JSON.stringify(42)"),
        Value::Str("42".into()),
    );
    // Integer-valued Float keeps `.0` suffix.
    assert_eq!(
        run("JSON := import 'JSON'; JSON.stringify(42.0)"),
        Value::Str("42.0".into()),
    );
    assert_eq!(
        run("JSON := import 'JSON'; JSON.stringify('hi')"),
        Value::Str("\"hi\"".into()),
    );
}

#[test]
fn v04_json_stringify_string_escapes() {
    let v = run(
        r#"JSON := import 'JSON';
           JSON.stringify('a\nb"c\\d')"#
    );
    assert_eq!(v, Value::Str(r#""a\nb\"c\\d""#.into()));
}

#[test]
fn v04_json_stringify_array_object() {
    let v = run(
        "JSON := import 'JSON'; \
         JSON.stringify(${a: 1, b: [2, 3]})"
    );
    assert_eq!(v, Value::Str(r#"{"a":1,"b":[2,3]}"#.into()));
}

#[test]
fn v04_json_stringify_indent_int() {
    let v = run(
        "JSON := import 'JSON'; \
         JSON.stringify(${a: 1, b: 2}, 2)"
    );
    assert_eq!(v, Value::Str("{\n  \"a\": 1,\n  \"b\": 2\n}".into()));
}

#[test]
fn v04_json_stringify_indent_str() {
    let v = run(
        "JSON := import 'JSON'; \
         JSON.stringify([1, 2], '\t')"
    );
    assert_eq!(v, Value::Str("[\n\t1,\n\t2\n]".into()));
}

#[test]
fn v04_json_stringify_function_raises() {
    let v = run(
        "JSON := import 'JSON'; \
         f := fn() { 1 }; \
         try JSON.stringify(f) catch (e) { 'caught' }"
    );
    assert_eq!(v, Value::Str("caught".into()));
}

#[test]
fn v04_json_roundtrip_via_stringify() {
    // Stringify a tigr value, then parse it back. Numbers come back
    // as Float (always), other types preserved structurally.
    let v = run(
        "JSON := import 'JSON'; \
         data := ${a: 1, b: [2, 3], c: 'x'}; \
         s := JSON.stringify(data); \
         back := JSON.parse(s); \
         back.a + back.b[0] + back.b[1]"
    );
    match v {
        Value::Float(x) => assert!((x - 6.0).abs() < 1e-9),
        v => panic!("got {v:?}"),
    }
}

// ---- v0.4 Phase 3: pattern destructuring on `=` and mid-expression ----

#[test]
fn v04_assign_pattern_array() {
    // After `:=`, `[b, a] = [a, b]` swaps via the array-pattern.
    let v = run("[a, b] := [1, 2]; [b, a] = [a, b]; [a, b]");
    assert_eq!(v, Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        vec![Value::Int(2), Value::Int(1)]
    ))));
}

#[test]
fn v04_assign_pattern_object() {
    let v = run("${x, y} := ${x: 1, y: 2}; ${x, y} = ${x: 10, y: 20}; x + y");
    assert_eq!(v, Value::Int(30));
}

#[test]
fn v04_assign_pattern_returns_rhs() {
    // `[a, b] = rhs` evaluates to rhs (just like `x = 5` does).
    let v = run("a := 0; b := 0; r := ([a, b] = [3, 4]); r");
    assert_eq!(v, Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        vec![Value::Int(3), Value::Int(4)]
    ))));
}

#[test]
fn v04_assign_pattern_undeclared_errors() {
    let msg = run_err("[a, b] = [1, 2]");
    assert!(msg.contains("undeclared") || msg.contains("UndeclaredAssign"),
        "got {msg}");
}

#[test]
fn v04_assign_pattern_compound_op_errors() {
    // `+=` with a pattern is a parse error per spec §11.4.
    let msg = run_err("[a, b] := [1, 2]; [a, b] += [10, 20]");
    assert!(msg.contains("invalid") || msg.contains("parse"),
        "got {msg}");
}

#[test]
fn v04_assign_pattern_nested() {
    let v = run("a := 0; b := 0; c := 0; \
                 [a, [b, c]] = [1, [2, 3]]; \
                 a + b + c");
    assert_eq!(v, Value::Int(6));
}

#[test]
fn v04_assign_pattern_rest() {
    let v = run("a := 0; rest := []; \
                 [a, ...rest] = [1, 2, 3, 4]; \
                 a + #rest");
    assert_eq!(v, Value::Int(1 + 3));
}

#[test]
fn v04_pattern_decl_in_expr_position() {
    // Mid-expression `:=` with a pattern hoists the leaves and the
    // expression evaluates to the source rhs (matches `x := 5`'s
    // "returns the bound value" convention, generalized).
    let v = run("arr := ([a, b] := [3, 4]); a + b + #arr");
    assert_eq!(v, Value::Int(3 + 4 + 2));
}

#[test]
fn v04_pattern_decl_mid_expr_used_in_arith() {
    let v = run("base := 100; \
                 sum := base + ([x, y] := [10, 20])[0] + x + y; \
                 sum");
    assert_eq!(v, Value::Int(100 + 10 + 10 + 20));
}

#[test]
fn v04_pattern_decl_in_for_iter() {
    // Mid-expression `:=` inside the for-iter expression — the new
    // names live in the iter's scope (gone after the loop). What we
    // CAN check: the iter expression's value was computed correctly
    // from the destructured source.
    let v = run("c := 0; \
                 for (i, 0..([upper, _] := [3, 99])[0]) { c = c + 1 }; \
                 c");
    assert_eq!(v, Value::Int(3));
}

#[test]
fn v04_pattern_decl_with_rest_mid_expr() {
    let v = run("obj := (${head, ...rest} := ${head: 'h', a: 1, b: 2}); \
                 (#rest * 10) + (if head == 'h' { 1 } else { 0 })");
    assert_eq!(v, Value::Int(20 + 1));
}

// ---- v0.4 Phase 2: number-literal extensions ----

#[test]
fn v04_num_hex() {
    assert_eq!(run("0xFF"), Value::Int(255));
    assert_eq!(run("0xff"), Value::Int(255));
    assert_eq!(run("0xCAFEBABE"), Value::Int(0xCAFEBABE));
    assert_eq!(run("0x0"), Value::Int(0));
}

#[test]
fn v04_num_binary() {
    assert_eq!(run("0b1010"), Value::Int(10));
    assert_eq!(run("0B1111_0000"), Value::Int(0xF0));
    assert_eq!(run("0b0"), Value::Int(0));
}

#[test]
fn v04_num_octal() {
    assert_eq!(run("0o755"), Value::Int(0o755));
    assert_eq!(run("0o10"), Value::Int(8));
}

#[test]
fn v04_num_underscore_separator() {
    assert_eq!(run("1_000_000"), Value::Int(1_000_000));
    match run("3.141_592") {
        Value::Float(x) => assert!((x - 3.141_592).abs() < 1e-9),
        v => panic!("got {v:?}"),
    }
    assert_eq!(run("0xFF_FF"), Value::Int(0xFFFF));
    assert_eq!(run("0b1010_1010"), Value::Int(0xAA));
}

#[test]
fn v04_num_scientific() {
    match run("1e6") {
        Value::Float(x) => assert!((x - 1_000_000.0).abs() < 1e-9),
        v => panic!("got {v:?}"),
    }
    match run("2.5e-3") {
        Value::Float(x) => assert!((x - 0.0025).abs() < 1e-12),
        v => panic!("got {v:?}"),
    }
    match run("1E+9") {
        Value::Float(x) => assert!((x - 1e9).abs() < 1e-3),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v04_num_leading_dot() {
    match run(".5") {
        Value::Float(x) => assert!((x - 0.5).abs() < 1e-12),
        v => panic!("got {v:?}"),
    }
    match run(".25e2") {
        Value::Float(x) => assert!((x - 25.0).abs() < 1e-9),
        v => panic!("got {v:?}"),
    }
}

#[test]
fn v04_num_trailing_dot_lexes_as_int_dot() {
    // `5.` is `Int(5) Dot` — `5.method` style member access. As an
    // expression on its own this is a *parse* error, not a lex error.
    let msg = run_err("5.");
    assert!(msg.contains("parse error") || msg.contains("error[parse]"), "got {msg}");
}

#[test]
fn v04_num_underscore_rejected_forms() {
    for src in ["5_", "5__5", "0x_FF", "0b_10", "0o_7"] {
        let msg = run_err(src);
        assert!(
            msg.contains("lex error") || msg.contains("error[lex]"),
            "expected lex error for {src:?}, got {msg}",
        );
    }
}

#[test]
fn v04_num_overflow_is_lex_error() {
    let msg = run_err("0xFFFFFFFFFFFFFFFF");
    assert!(msg.contains("out of range"), "got {msg}");

    let msg = run_err("99999999999999999999");
    assert!(msg.contains("out of range"), "got {msg}");
}

#[test]
fn v04_num_range_with_new_literals() {
    // Range from hex to hex.
    let v = run("r := 0xFE..0x100; #r");
    assert_eq!(v, Value::Int(2));

    // Underscores inside range bounds.
    let v = run("c := 0; for (i, 0..1_000) { c = c + 1 }; c");
    assert_eq!(v, Value::Int(1000));
}

#[test]
fn v04_num_range_op_still_works() {
    // Make sure the new lexer didn't break `1..10` and `1..=5`.
    assert_eq!(run("#(1..5)"), Value::Int(4));
    assert_eq!(run("#(1..=5)"), Value::Int(5));
}

#[test]
fn v04_render_repl_uses_repl_filename() {
    let mut repl = crate::repl::Repl::new();
    let err = match repl.eval("1 / 0") {
        Ok(v) => panic!("expected error, got {v:?}"),
        Err(e) => e,
    };
    let out = err.render(&repl.sources());
    assert!(out.contains("<repl:"), "expected <repl:N> filename — got:\n{out}");
    assert!(out.contains("error[runtime]: division by zero"), "got:\n{out}");
}

// ---- v0.5: type() builtin ----

#[test]
fn v05_type_of_scalars() {
    assert_eq!(run("type(1)"), Value::Str("int".into()));
    assert_eq!(run("type(1.0)"), Value::Str("float".into()));
    assert_eq!(run("type('x')"), Value::Str("string".into()));
    assert_eq!(run("type(true)"), Value::Str("bool".into()));
    assert_eq!(run("type(null)"), Value::Str("null".into()));
}

#[test]
fn v05_type_of_collections() {
    assert_eq!(run("type([1, 2])"), Value::Str("array".into()));
    assert_eq!(run("type(${a: 1})"), Value::Str("object".into()));
    assert_eq!(run("type(0..3)"), Value::Str("range".into()));
}

#[test]
fn v05_type_of_callables_is_function() {
    // Both user closures and native builtins report "function".
    assert_eq!(run("type(fn(x) { x })"), Value::Str("function".into()));
    assert_eq!(run("type(print)"), Value::Str("function".into()));
}

#[test]
fn v05_type_arity_errors() {
    assert!(run_err("type()").contains("arity") || !run_err("type()").is_empty());
    assert!(!run_err("type(1, 2)").is_empty());
}

// ---- v0.5: bitwise operators ----

#[test]
fn v05_bitwise_basic() {
    assert_eq!(run("6 & 3"), Value::Int(2));
    assert_eq!(run("6 | 3"), Value::Int(7));
    assert_eq!(run("5 ^ 3"), Value::Int(6));
    assert_eq!(run("~0"), Value::Int(-1));
    assert_eq!(run("~5"), Value::Int(-6));
}

#[test]
fn v05_bitwise_shifts() {
    assert_eq!(run("1 << 4"), Value::Int(16));
    assert_eq!(run("256 >> 2"), Value::Int(64));
    // `>>` is arithmetic — sign-preserving.
    assert_eq!(run("-8 >> 1"), Value::Int(-4));
}

#[test]
fn v05_pow_moved_to_double_caret() {
    match run("2 ^^ 10") {
        Value::Float(x) => assert!((x - 1024.0).abs() < 1e-9, "got {x}"),
        v => panic!("expected float, got {v:?}"),
    }
}

#[test]
fn v05_bitwise_precedence() {
    // `|` < `^` < `&`, all below comparison: `1 == 1 & 1` is `1 == (1 & 1)`.
    assert_eq!(run("1 == 1 & 1"), Value::Bool(true));
    // shift below additive: `1 + 1 << 2` == `(1 + 1) << 2` == 8.
    assert_eq!(run("1 + 1 << 2"), Value::Int(8));
    // shift above multiplicative: `2 << 1 * 2` is `2 << (1 * 2)` == 8.
    assert_eq!(run("2 << 1 * 2"), Value::Int(8));
    // `&` binds tighter than `|`: `1 | 2 & 0` == `1 | (2 & 0)` == 1.
    assert_eq!(run("1 | 2 & 0"), Value::Int(1));
}

#[test]
fn v05_bitwise_type_errors() {
    assert!(!run_err("1.5 & 2").is_empty());
    assert!(!run_err("'x' | 1").is_empty());
    assert!(!run_err("~true").is_empty());
}

#[test]
fn v05_shift_out_of_range_raises() {
    // Must raise, not panic.
    assert!(!run_err("1 << 64").is_empty());
    assert!(!run_err("1 << -1").is_empty());
    assert!(!run_err("1 >> 99").is_empty());
}

#[test]
fn v05_bare_amp_and_pipe_lex() {
    // Bare `&` / `|` were lex errors before v0.5; now they tokenize.
    assert_eq!(run("12 & 10 | 1"), Value::Int(9));
}

// ---- v0.5: match expression ----

#[test]
fn v05_match_literal() {
    assert_eq!(
        run("match 1 { 0 => 'a', 1 => 'b', _ => 'c' }"),
        Value::Str("b".into()),
    );
}

#[test]
fn v05_match_non_exhaustive_is_null() {
    assert_eq!(run("match 9 { 1 => 'a', 2 => 'b' }"), Value::Null);
}

#[test]
fn v05_match_wildcard_and_binding() {
    assert_eq!(run("match 99 { _ => 'any' }"), Value::Str("any".into()));
    assert_eq!(run("match 7 { x => x * 2 }"), Value::Int(14));
}

#[test]
fn v05_match_negative_literal() {
    assert_eq!(
        run("match -1 { -1 => 'negone', _ => 'other' }"),
        Value::Str("negone".into()),
    );
}

#[test]
fn v05_match_range() {
    let g = "fn(s) { match s { 90..=100 => 'A', 80..=89 => 'B', _ => 'F' } }";
    assert_eq!(run(&format!("({g})(95)")), Value::Str("A".into()));
    assert_eq!(run(&format!("({g})(85)")), Value::Str("B".into()));
    assert_eq!(run(&format!("({g})(50)")), Value::Str("F".into()));
}

#[test]
fn v05_match_range_exclusive() {
    assert_eq!(run("match 10 { 0..10 => 'in', _ => 'out' }"), Value::Str("out".into()));
    assert_eq!(run("match 9 { 0..10 => 'in', _ => 'out' }"), Value::Str("in".into()));
}

#[test]
fn v05_match_range_non_number_falls_through() {
    // A string subject must not raise against a range pattern.
    assert_eq!(
        run("match 'x' { 0..10 => 'n', _ => 'other' }"),
        Value::Str("other".into()),
    );
}

#[test]
fn v05_match_array_exact_and_mismatch() {
    assert_eq!(run("match [1, 2] { [a, b] => a + b, _ => -1 }"), Value::Int(3));
    assert_eq!(
        run("match [1, 2, 3] { [a, b] => 'two', _ => 'other' }"),
        Value::Str("other".into()),
    );
}

#[test]
fn v05_match_array_rest() {
    assert_eq!(run("match [1, 2, 3, 4] { [h, ...t] => h + #t }"), Value::Int(4));
}

#[test]
fn v05_match_array_on_non_array_falls_through() {
    assert_eq!(
        run("match 5 { [a, b] => 'arr', _ => 'scalar' }"),
        Value::Str("scalar".into()),
    );
}

#[test]
fn v05_match_object() {
    let src = "area := fn(s) {
        match s {
            ${kind: 'rect', w, h} => w * h,
            ${kind: 'square', side} => side * side,
            _ => 0
        }
    };
    area(${kind: 'rect', w: 3, h: 4})";
    assert_eq!(run(src), Value::Int(12));
}

#[test]
fn v05_match_object_missing_literal_field_fails() {
    assert_eq!(
        run("match ${w: 1} { ${kind: 'rect'} => 'rect', _ => 'no' }"),
        Value::Str("no".into()),
    );
}

#[test]
fn v05_match_object_on_non_object_falls_through() {
    assert_eq!(
        run("match 5 { ${a} => 'obj', _ => 'no' }"),
        Value::Str("no".into()),
    );
}

#[test]
fn v05_match_nested_pattern() {
    assert_eq!(
        run("match ${pt: [3, 4]} { ${pt: [x, y]} => x * x + y * y, _ => -1 }"),
        Value::Int(25),
    );
}

#[test]
fn v05_match_guard() {
    let f = "fn(n) { match n { x if x < 0 => 'neg', _ => 'ok' } }";
    assert_eq!(run(&format!("({f})(-5)")), Value::Str("neg".into()));
    // Guard fails → fall through to the next arm.
    assert_eq!(run(&format!("({f})(5)")), Value::Str("ok".into()));
}

#[test]
fn v05_match_or_pattern() {
    assert_eq!(run("match 2 { 1 | 2 | 3 => 'low', _ => 'high' }"), Value::Str("low".into()));
    assert_eq!(run("match 9 { 1 | 2 | 3 => 'low', _ => 'high' }"), Value::Str("high".into()));
}

#[test]
fn v05_match_or_pattern_binding_is_compile_error() {
    // A binding alternative inside an or-pattern is rejected.
    assert!(!run_err("match 1 { a | 2 => 0, _ => 0 }").is_empty());
}

#[test]
fn v05_match_subject_evaluated_once() {
    let src = "count := 0;
               inc := fn() { count = count + 1; count };
               match inc() { _ => null };
               count";
    assert_eq!(run(src), Value::Int(1));
}

#[test]
fn v05_match_body_is_scope() {
    assert_eq!(run("match 5 { n => { d := n * 2; d + 1 } }"), Value::Int(11));
}

#[test]
fn v05_match_is_a_value() {
    assert_eq!(run("x := match 1 { 1 => 10, _ => 0 }; x + 5"), Value::Int(15));
}

#[test]
fn v05_match_trailing_comma_optional() {
    assert_eq!(run("match 1 { 1 => 'a', _ => 'b', }"), Value::Str("a".into()));
    assert_eq!(run("match 1 { 1 => 'a', _ => 'b' }"), Value::Str("a".into()));
}

#[test]
fn v05_match_arm_binding_captured_by_closure() {
    // A closure created in an arm body captures the pattern binding;
    // the per-arm Unwind must close that upvalue, not discard it.
    let src = "f := match [10] { [x] => fn() { x + 1 }, _ => fn() { 0 } };
               f()";
    assert_eq!(run(src), Value::Int(11));
}

#[test]
fn v05_match_in_loop_with_break() {
    // `match` nested in a loop; an arm can drive `break`.
    let src = "for (i, 0..10) { match i { 3 => break (i * 100), _ => i } }";
    assert_eq!(run(src), Value::Int(300));
}
