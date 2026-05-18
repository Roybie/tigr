# Errors

Spec: [LANGUAGE.md §9.6](../../LANGUAGE.md#96-try--catch--raise-v03)

Errors in Tigr are ordinary values. You raise one, and you catch one, and the thing that travels between those two points is just a value like any other. `try`, `catch`, and `raise` are all expressions.

## `raise`

`raise expr` aborts the current evaluation and carries `expr`'s value outward. Any value works, not only strings: an integer, an object, an array. The value is stored verbatim with no coercion.

```
raise 'config missing'
raise ${kind: 'db_down', detail: 'connection lost'}
```

An uncaught `raise` exits the program, printing the raised value.

## `try` and `catch`

`try expr` evaluates `expr` and yields its value on success. If `expr` raises, or hits a built-in runtime error, the `try` yields `null` instead.

`try expr catch (e) { handler }` adds a handler. On a raised or runtime error it runs `handler` with the error value bound to `e`, and the whole expression evaluates to the handler's result.

```tigr
risky := fn(x) { 100 / x };
result := try risky(0) catch (e) {
    print('caught kind:', e.kind);   // => caught kind: div_by_zero
    -1
};
print(result);   // => -1
```

The body of `try` parses at `&&` precedence, so `try f(x) || default` binds as `(try f(x)) || default`. That makes the fallback idiom read naturally:

```tigr
n := try num('not a number') || 0;
print(n);   // => 0
```

Wrap the body in parentheses if you want the `||` inside the `try`.

## The structured error object

`catch` binds exactly what was raised. A `raise 'msg'` gives `e` a string, and a `raise ${...}` gives `e` that object.

A **built-in** runtime error is different. Division by zero, a type mismatch, calling a non-function, a failed import: each of these is reified into a structured object with three fields.

- `kind`: a stable snake-case tag identifying the error category.
- `message`: the human-readable text an uncaught error would print, for example `"division by zero"`.
- `line`: the source line the error occurred on.

Because `kind` is stable, a handler can `match` on it and re-raise anything it does not recognize:

```tigr
risky := fn(x) { 100 / x };
classify := fn(x) {
    try risky(x) catch (e) {
        match e.kind {
            'div_by_zero' => 'was zero division',
            _             => raise e,
        }
    }
};
print(classify(0));   // => was zero division
```

A raised object carries whatever fields you put on it, so you can design your own error shapes:

```tigr
got := try (raise ${kind: 'db_down', detail: 'lost'}) catch (e) {
    e.detail
};
print(got);   // => lost
```

## Built-in error kinds

A built-in runtime error reifies with one of these `kind` values:

- `div_by_zero`: integer or float division (or modulo) by zero.
- `type_mismatch`: an operation given a value of the wrong type.
- `index_out_of_bounds`: an array or string index outside its range.
- `arity_mismatch`: a call given the wrong number of arguments where that is checked.
- `not_callable`: calling something that is not a function.
- `invalid_index_type`: indexing with a value that cannot be an index.
- `invalid_key_type`: using a value that cannot be a key.
- `immutable_target`: assigning to something that cannot be mutated.
- `import_failed`: an import that could not be resolved or loaded.
- `overflow`: an integer operation that overflows i64.
- `stack_overflow`: recursion past the VM's call-depth limit.
- `stack_underflow`: an internal stack-balance failure.
- `cycle`: a cyclic structure where one is not allowed, for example `JSON.stringify` of a self-referential value.
- `no_match`: a `match` with no arm matching the subject and no `_` wildcard.

Native stdlib modules such as `Math`, `IO`, `JSON`, and `Path` raise plain string messages, so `catch` binds those as strings rather than structured objects. The one exception is `JSON.stringify` on a circular structure, which raises a structured `cycle` error. The `Net` module is also structured: its failures arrive as `${kind, message}` objects.

## How uncaught errors render

When an error escapes every `try` and reaches the top level, tigr prints a rustc-style block: the error category, the file and line, and the source line itself.

```
error[runtime]: division by zero
 --> examples/error_rendering.tg:6
  |
6 | result := x / y;
  |
```

Lex, parse, and compile errors carry a precise span, so they get an underlined caret matching the span's width:

```
error[parse]: unexpected token `:=`
 --> /tmp/p.tg:2:6
  |
2 | y := := 7;
  |      ^^
```

An error raised inside an imported file renders against that file's source, because the import machinery registers each imported file with the renderer.

An error that escapes every `try` also prints a **stack trace** beneath the snippet, with each active call frame listed innermost first:

```
error[runtime]: division by zero
 --> t6.tg:1
  |
1 | inner := fn(n) { n / 0 };
  |
stack trace (most recent call first):
  inner at t6.tg:1
  compute at t6.tg:2
  <main> at t6.tg:3
```

Frame names come from the binding: `f := fn(){}` shows as `f`, an unbound `fn` shows as `<anonymous>`, and the top-level program shows as `<main>`. Tail calls reuse their frame, so a tail-recursive function appears only once. A single-frame error prints no trace at all.

## See also

- [Control flow](control-flow.md#match): `match`, which raises a catchable `no_match` when no arm fits
- [Functions](functions.md#tail-calls): how `stack_overflow` relates to recursion depth
- [LANGUAGE.md §9.6](../../LANGUAGE.md#96-try--catch--raise-v03): the authoritative spec
