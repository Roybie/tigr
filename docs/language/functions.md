# Functions

Spec: [LANGUAGE.md §10](../../LANGUAGE.md#10-functions)

A function in Tigr is a value created by an `fn` expression. It can be bound to a name, passed as an argument, returned from another function, or stored in a collection. There is no separate function-declaration syntax: every function is an expression.

## Defining and calling

`fn(params) { body }` evaluates to a closure. Bind it with `:=` to give it a name, or call it on the spot.

```tigr
add := fn(a, b) { a + b };
print(add(2, 3));        // => 5

print(fn() { 100 }());   // => 100
```

The body is a block, and a block evaluates to its last expression, so a function with no explicit `return` still produces a value. `return` is available for early exit and is itself an expression (see [Control flow](control-flow.md#return)).

An `fn` initializer can see its own binding name, so direct recursion needs no forward declaration:

```tigr
fact := fn(n) { if n <= 1 { 1 } else { n * fact(n - 1) } };
print(fact(6));   // => 720
```

Mutual recursion uses the forward-declaration idiom: declare the name first as `g := null`, then assign the real function once the other name is in scope.

## Parameters

Tigr offers four kinds of parameter, and they combine freely.

**Positional.** `fn(a, b, c) { ... }` takes arguments by position. A missing argument binds to `null`, and extra arguments past the last parameter are dropped silently.

**Rest.** A final `...name` parameter collects every remaining argument into an array, which may be empty. Only one rest parameter is allowed, and it must come last.

```tigr
length := fn(...args) { #args };
print(length());          // => 0
print(length(1, 2, 3));   // => 3
```

**Destructuring patterns.** Any parameter can be a pattern instead of a plain name, so a function can pull fields out of an object or elements out of an array at the call boundary. See [Destructuring](destructuring.md) for the pattern forms.

```tigr
greet := fn(${name, age}) { 'hi {name}, age {age}' };
print(greet(${name: 'tigr', age: 0}));   // => hi tigr, age 0
```

**Default values.** A plain identifier parameter can carry a default with `=`. The default fills in when that argument slot is `null`, which covers both an omitted argument and one explicitly passed as `null`.

```tigr
scale := fn(x, factor = 2) { x * factor };
print(scale(10));         // => 20  (default used)
print(scale(10, 5));      // => 50
print(scale(10, null));   // => 20  (explicit null also triggers it)
```

A default only fires on `null`. Passing `0`, `false`, or `''` keeps that value as-is. Defaults are evaluated left to right and only when needed, and a later default may reference an earlier parameter:

```
fn(a, b = a + 1) { ... }
```

A default is allowed only on a plain identifier parameter. It cannot be attached to a destructuring pattern or to the rest parameter.

## Closures

A function captures the bindings of its enclosing scope by reference. Capture happens at the point where the `fn` expression appears, not at the point of call, so the function carries its lexical environment with it.

Because capture is by reference, a closure can read and mutate a variable from an outer scope after that scope has otherwise finished:

```tigr
make_counter := fn() {
    n := 0;
    fn() { n += 1 }
};
c := make_counter();
print(c());   // => 1
print(c());   // => 2
```

Each iteration of a `for` loop opens a fresh scope for its loop variables, so a closure built inside the loop captures that iteration's value independently of the others:

```tigr
adders := for[] (i, 0..3) { fn(x) { x + i } };
print(adders[0](10));   // => 10
print(adders[1](10));   // => 11
print(adders[2](10));   // => 12
```

## Method-style calls

`obj.method(args)` is plain index followed by call: it reads the `method` field off `obj`, then calls the resulting function. Tigr does not pass an implicit `this`, so a function stored in an object field has no special access to the object it lives in.

```tigr
counter := ${ n: 0, bump: fn() { 1 } };
print(counter.bump());   // => 1
```

When you want the receiver passed as the first argument, use the pipe operator. `obj |> method(args)` rewrites to `method(obj, args)`, which is the idiomatic way to chain stdlib calls:

```tigr
double := fn(x) { x * 2 };
print([1, 2, 3] |> Array.map(double));   // => [2, 4, 6]
```

## Tail calls

A call in **tail position** reuses the current call frame instead of pushing a new one. A tail-recursive function therefore runs in constant frame space, to any depth.

A call is in tail position when its result is directly the result of the enclosing function. That includes a call sitting in the branches of an `if`, the arms of a `match`, or the tail expression of a block, as long as those are themselves the function's result.

```tigr
sum := fn(n, acc) {
    if n <= 0 { acc } else { sum(n - 1, acc + n) }
};
print(sum(1000000, 0));   // => 500000500000
```

A call is **not** in tail position if its result is used further. `n * fact(n - 1)` feeds the call into `*`, and `1 + sum(n - 1)` feeds it into `+`, so both push a new frame. Calls inside a `try` body, inside a `&&` or `||` operand, or inside a loop body are likewise never tail calls.

Call depth is bounded. Recursion that genuinely nests past the VM's limit raises a catchable `stack_overflow` error rather than crashing the process:

```
try deepNonTailRecursion() catch (e) { e.kind }   // 'stack_overflow'
```

To make deep recursion of a non-tail shape work, rewrite it in the accumulator style shown for `sum` above.

## See also

- [Control flow](control-flow.md): `return`, `if`, `match`, and the loops a closure can capture inside
- [Destructuring](destructuring.md): the pattern forms a parameter can use
- [Errors](errors.md): `stack_overflow` and the other errors a call can raise
- [LANGUAGE.md §10](../../LANGUAGE.md#10-functions): the authoritative spec
