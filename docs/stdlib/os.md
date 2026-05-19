# `Os`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#os)

`Os` provides process and environment access, imported with `import 'Os'`. It reads command-line arguments and environment variables, reports the working directory, runs subprocesses, and exits the process. One entry, `args`, is a plain value rather than a function, because command-line arguments do not change while a program runs.

## Functions

### `args -> Array`

The command-line arguments. This is a value, not a function: index it directly with `Os.args[1]`.

**Returns:** an `Array` of `String` arguments, shaped `[interpreter, script, user_arg1, ...]`. The first entry is the interpreter path, the second is the script path, and the rest are arguments passed by the user.

```tigr
Os := import 'Os';

print(type(Os.args));     // => array
print(#Os.args >= 2);     // => true
```

### `env(name) -> String | null`

Reads the value of an environment variable.

- `name` *(String)*: the variable name to look up.

**Returns:** the variable's value as a `String`, or `null` if it is not set.
**Raises:** a string error if `name` is not a `String`.

```tigr
Os := import 'Os';

print(Os.env('TIGR_NOT_SET'));   // => null
home := Os.env('HOME');
print(home != null);             // => true
```

### `cwd() -> String`

Returns the current working directory.

**Returns:** the working directory path as a `String`.
**Raises:** a string error if the working directory cannot be read.

```tigr
Os := import 'Os';

print(type(Os.cwd()));   // => string
```

### `run(cmd, arg1?) -> Object`

Runs a subprocess, waits for it to finish, and captures its output. A non-zero exit status is a normal result reported in the `code` field, not an error.

- `cmd` *(String)*: the command to run.
- `arg1` *(String, optional)*: an argument passed to `cmd`. `run` is variadic, so any number of `String` arguments may follow.

**Returns:** an `Object` `${code, stdout, stderr}`. `code` is the exit status (`-1` if the process was killed by a signal), and `stdout` / `stderr` are the captured output streams as `String`s.
**Raises:** a string error if the process cannot be spawned at all (for example, the command is not found), or if any argument is not a `String`.

`run` is a blocking call. Inside a green thread it is offloaded to a background worker pool, so waiting on the subprocess does not freeze the actor's other coroutines; see [concurrency](../language/concurrency.md). With no other coroutine to run, it executes inline with no overhead.

```tigr
Os := import 'Os';
String := import 'String';

r := Os.run('echo', 'hello', 'world');
print(r.code);                   // => 0
print(String.trim(r.stdout));    // => hello world
```

### `exit(code) -> never`

Exits the process immediately with the given status code. This is a real process exit, so it bypasses `try` and no cleanup runs after it. It does not return.

- `code` *(Int)*: the exit status code.

**Raises:** a string error if `code` is not an `Int`.

```tigr
Os := import 'Os';

// Os.exit(0) would terminate the process here.
print('still running');   // => still running
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#os): the authoritative spec for `Os`
- [IO](io.md): file and stdio operations
- [Path](path.md): build and split path strings
- [Errors](../language/errors.md): `try` and `catch`, which `Os.exit` deliberately bypasses
