# Destructuring

Spec: [LANGUAGE.md §11](../../LANGUAGE.md#11-destructuring)

Destructuring pulls values out of an array or object by writing a pattern in place of a single name. A pattern can appear on the left of `:=` to declare new bindings, on the left of `=` to reassign existing ones, and as a function parameter. A leaf with no matching value binds to `null`.

These patterns are **irrefutable**: they always bind, filling in `null` where the source falls short. That is the opposite of the refutable patterns used by `match`, which are allowed to fail and fall through (see [Control flow](control-flow.md#match)).

## Array patterns

An array pattern names each position. A `_` skips a position without binding it, and a final `...rest` collects whatever is left into an array.

```tigr
[a, b, c] := [1, 2, 3];
print(a, b, c);             // => 1 2 3

[head, ...rest] := [10, 20, 30, 40];
print(head, rest);          // => 10 [20, 30, 40]

[x, _, z] := [1, 2, 3];
print(x, z);                // => 1 3

[m, n] := [99];
print(m, n);                // => 99 null
```

## Object patterns

An object pattern names keys. The shorthand `${name}` binds `name` to `obj.name`. The `${name: n}` form renames, binding the value at key `name` to `n`. A final `...rest` collects the remaining keys into a new object.

```tigr
person := ${name: 'tigr', age: 7};

${name, age} := person;
print(name, age);   // => tigr 7

${name: who} := person;
print(who);         // => tigr
```

## Nested patterns

Array and object patterns nest to any depth, so you can reach into a structure in one step.

```tigr
response := ${user: ${id: 5, name: 'r'}};
${user: ${id, name}} := response;
print(id, name);   // => 5 r
```

## Reassigning with `=`

A pattern also works on the left of plain `=`. Every leaf of the pattern must already be declared, otherwise it is a compile-time error. This makes a swap or a bulk update concise:

```tigr
a := 1;
b := 2;
[b, a] = [a, b];
print(a, b);   // => 2 1
```

Compound assignment forms such as `+=` are not allowed with patterns. Only plain `:=` and `=` accept them.

## Mid-expression declarations

A pattern `:=` can appear inside a larger expression, not only as a standalone statement. Each leaf is hoisted to a stable slot at scope entry, so the surrounding operation cannot clobber it. The `:=` expression itself evaluates to the source right-hand side.

```tigr
arr := ([a, b] := [3, 4]);
print(arr, a, b);   // => [3, 4] 3 4

n := 5 + ([c, d] := [10, 20])[0];
print(n, c, d);     // => 15 10 20
```

The same applies anywhere an expression can sit: inside a `for` iterable expression, inside function-call arguments, and so on.

## See also

- [Functions](functions.md): destructuring patterns as parameters
- [Control flow](control-flow.md#match): refutable patterns in `match`, which may fail
- [LANGUAGE.md §11](../../LANGUAGE.md#11-destructuring): the authoritative spec
