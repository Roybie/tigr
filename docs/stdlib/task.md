# `Task`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#132-native-modules-v03)

The `Task` module joins spawned actors. A `Task` is the value the `spawn` keyword produces: `spawn fn() { ... }` runs the function as a separate actor and evaluates to a `Task` handle for it. The module's one function, `join`, blocks until that actor finishes and hands its result back. Import it with `Task := import 'Task'`. `type(t)` on a task is `'task'`, and a task is not JSON-serializable.

```tigr
Task := import 'Task';

t := spawn fn() { 6 * 7 };
print(Task.join(t));            // => 42
```

## Functions

### `join(task) -> value`

Blocks until the actor behind `task` finishes, then returns its result. The returned value is deep-copied into the calling actor's heap. If the actor ended in an error instead, `join` re-raises it here so the caller can `try` and `catch` it.

- `task` *(Task)*: the handle of the actor to wait for.

**Returns:** the value the actor's function evaluated to.
**Raises:** whatever the actor raised. A `raise`d value comes back verbatim. A built-in runtime error in the actor comes back as an object `${kind, message, trace, worker}`, where `worker` is `true`. Joining the same task a second time raises a string error.

```tigr
Task := import 'Task';

tasks := for[] (i, 1..=4) { spawn fn() { i * i } };
results := for[] (t, tasks) { Task.join(t) };
print(results);                 // => [1, 4, 9, 16]
```

A `raise` inside the spawned actor surfaces at the `join` call, so a `catch` around `join` binds exactly what the actor raised:

```tigr
Task := import 'Task';

t := spawn fn() { raise 'boom' };
caught := try { Task.join(t) } catch (e) { 'got: ' + e };
print(caught);                  // => got: boom
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#132-native-modules-v03): the authoritative spec for `Task`
- [Net](net.md): sockets cross actor boundaries, so a `spawn`ed actor can handle a connection
- [Errors](../language/errors.md): `try` and `catch`, which catch an actor's re-raised error
