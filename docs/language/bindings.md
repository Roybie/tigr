# Bindings and scope

Spec: [LANGUAGE.md §4](../../LANGUAGE.md#4-bindings-and-scope)

A binding attaches a name to a value. Tigr keeps declaration and assignment as two separate operators, so the difference between introducing a name and changing one is always visible in the source.

## `:=` declares, `=` assigns

- `:=` declares a new binding in the current scope. It shadows any outer binding of the same name.
- `=` assigns to the nearest enclosing binding of that name. It is an error if no such binding exists.

```
foo := 10;     // declare 'foo' in this scope
foo = 20;      // assign to the existing 'foo'
bar = 5;       // ERROR: 'bar' was never declared
```

The compound assignment operators `+=`, `-=`, `*=`, `/=`, and `%=` follow the same rule as `=`: they need an existing binding. On an array target, `+=` mutates the array in place rather than rebinding the name; see [Collections](collections.md).

Both `:=` and `=` are expressions, and each evaluates to the value that was assigned. That lets you declare names inside a larger expression:

```tigr
result := (x := 5) + (y := 7);
print(result);   // => 12
```

A mid-expression `:=` works as expected. The local is hoisted to a stable slot when its scope is entered, so the surrounding operation cannot clobber it part-way through.

A few positions declare bindings implicitly, with no `:=` written:

- function parameters
- `for` iteration variables, both the index and the element variable
- names introduced by a destructuring pattern on the left of `:=`

## Blocks and scopes

A block is a `;`-separated sequence of expressions, with an optional trailing `;`. The block's value is its last expression's value, or `null` if it ends in `;`.

```tigr
b := (a := 1; n := a + 1; n * 2);
print(b);   // => 4
```

```
(a := 1; b := 2;)   // evaluates to null because of the trailing ;
```

A scope is a block wrapped in `{ }`. It follows the same rules and also opens a fresh lexical scope. Names introduced inside with `:=` (or as parameters or for-variables) are not visible after the closing `}`. Mutations to outer bindings, made with `=`, do persist.

```tigr
a := 9;
b := { c := 20; c * (a = a + 1) };
print(a);   // => 10   (the inner 'a = a + 1' updated the outer binding)
print(b);   // => 200  (20 * 10)
// 'c' is out of scope here
```

## Closures capture by reference

A function captures the lexical environment of its definition site. Captured variables are held by reference, so mutating one inside a closure updates the binding it came from. That is what makes a counter possible:

```tigr
make_counter := fn() {
    n := 0;
    fn() { n = n + 1; n }
};
c := make_counter();
print(c());   // => 1
print(c());   // => 2
```

Each call to `make_counter` creates its own `n`, so two counters built this way do not share state.

## See also

- [Overview](overview.md): why everything is an expression
- [Destructuring](destructuring.md): patterns that bind several names at once
- [Functions](functions.md): parameters, calls, and closures in depth
- [Collections](collections.md): how `+=` mutates arrays in place
- [LANGUAGE.md §4](../../LANGUAGE.md#4-bindings-and-scope): the authoritative spec
