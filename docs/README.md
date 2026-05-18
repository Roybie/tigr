# Tigr documentation

This is the navigable reference for Tigr. It sits between two other documents:

- The [README](../README.md) is the overview: what Tigr is, how to install it, and a short tour.
- [LANGUAGE.md](../LANGUAGE.md) is the authoritative spec and compatibility contract. When this reference and LANGUAGE.md disagree, LANGUAGE.md wins.

These pages are the companion in the middle: one page per stdlib module and one per language topic, written to be read and linked rather than scrolled.

## Language

- [Overview](language/overview.md): types, truthiness, and the idea that everything is an expression
- [Bindings and scope](language/bindings.md): `:=` versus `=`, blocks, scopes
- [Expressions](language/expressions.md): arithmetic, comparison, bitwise, the pipe, indexing, spread
- [Strings](language/strings.md): the two string forms, interpolation, formatting, glob matching
- [Collections](language/collections.md): arrays, objects, ranges
- [Control flow](language/control-flow.md): `if`, `while`, `for`, `break`, `continue`, `return`, `match`
- [Functions](language/functions.md): definitions, parameters, closures, tail calls
- [Destructuring](language/destructuring.md): array, object, and nested patterns
- [Modules](language/modules.md): `import`, resolution, caching
- [Errors](language/errors.md): `try`, `catch`, `raise`, error kinds, error rendering
- [Concurrency](language/concurrency.md): `spawn`, channels, `select`, `parallel`
- [Garbage collection](language/gc.md): the collector and the `gc()` builtin
- [Operator precedence](language/operator-precedence.md): the full precedence table

## Standard library

See the [standard library index](stdlib/README.md) for all 22 modules and the global builtins.

## A note on maintenance

These pages are written and maintained by hand. The trade-off is that they can drift from the code: a new stdlib function has to be added both here and to LANGUAGE.md §13. If that drift becomes a recurring problem, the fix is a `tigr doc` generator that reads doc comments from the stdlib sources. That is not built yet, and will not be until the drift is real.
