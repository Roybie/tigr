# `IO`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#io)

`IO` covers file and stdio access, imported with `import 'IO'`. The fallible operations (reading, writing, listing, removing, `stat`) raise a catchable string-valued error when the underlying syscall fails, so wrap them in `try` when the path may be missing or unwritable. The predicate entries (`exists`, `is_dir`, `is_file`) never raise; they just report `false` for a path that does not exist.

The waiting calls (`read_file`, `write_file`, `append_file`, the `_bytes` variants, `list_dir`, `mkdir`, `remove`, and `read_line`) are *blocking* natives: inside a green thread they are offloaded to a worker pool so file IO does not stall the actor's other coroutines (see [concurrency](../language/concurrency.md)). The metadata-only calls (`exists`, `is_dir`, `is_file`, `stat`) stay inline, since offloading them would cost more than it saves.

## Functions

| Function | Summary |
|----------|---------|
| [`read_file(path) -> String`](#read_filepath---string) | Reads the entire file at `path` and returns its contents decoded as UTF-8. |
| [`write_file(path, text) -> null`](#write_filepath-text---null) | Writes `text` to `path`, overwriting any existing file and creating it if absent. |
| [`append_file(path, text) -> null`](#append_filepath-text---null) | Appends `text` to the end of the file at `path`, creating the file if it does not exist. |
| [`read_bytes(path) -> Bytes`](#read_bytespath---bytes) | Reads the entire file at `path` as raw bytes, with no UTF-8 decoding. |
| [`write_bytes(path, bytes) -> null`](#write_bytespath-bytes---null) | Writes a `Bytes` buffer to `path`, overwriting any existing file and creating it if absent. |
| [`append_bytes(path, bytes) -> null`](#append_bytespath-bytes---null) | Appends a `Bytes` buffer to the end of the file at `path`, creating the file if it does not exist. |
| [`exists(path) -> Bool`](#existspath---bool) | Tests whether anything exists at `path`, file or directory. |
| [`list_dir(path) -> Array`](#list_dirpath---array) | Lists the entries of the directory at `path`. |
| [`mkdir(path) -> null`](#mkdirpath---null) | Creates the directory at `path`, along with any missing parent directories. |
| [`remove(path) -> null`](#removepath---null) | Deletes the file at `path`, or, if `path` is a directory, removes it and all its contents recursively. |
| [`is_dir(path) -> Bool`](#is_dirpath---bool) | Tests whether `path` is a directory. |
| [`is_file(path) -> Bool`](#is_filepath---bool) | Tests whether `path` is a regular file. |
| [`stat(path) -> Object`](#statpath---object) | Reads filesystem metadata for `path`. |
| [`read_line() -> String \| null`](#read_line---string--null) | Reads one line from standard input, with the trailing newline stripped. |
| [`eprint(value1, value2?) -> value`](#eprintvalue1-value2---value) | Writes its arguments to standard error, matching `print`'s formatting: each argument in `str` form, space-separated, with a trailing newline. |


### `read_file(path) -> String`

Reads the entire file at `path` and returns its contents decoded as UTF-8.

- `path` *(String)*: the file to read.

**Returns:** the file contents as a `String`.
**Raises:** a string error if the file is missing, unreadable, or not valid UTF-8.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_a.txt', 'hello tigr');
print(IO.read_file('/tmp/tigr_doc_a.txt'));   // => hello tigr
IO.remove('/tmp/tigr_doc_a.txt');
```

### `write_file(path, text) -> null`

Writes `text` to `path`, overwriting any existing file and creating it if absent.

- `path` *(String)*: the file to write.
- `text` *(String)*: the contents to write.

**Returns:** `null`.
**Raises:** a string error if the path cannot be written.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_b.txt', 'first');
IO.write_file('/tmp/tigr_doc_b.txt', 'second');
print(IO.read_file('/tmp/tigr_doc_b.txt'));   // => second
IO.remove('/tmp/tigr_doc_b.txt');
```

### `append_file(path, text) -> null`

Appends `text` to the end of the file at `path`, creating the file if it does not exist.

- `path` *(String)*: the file to append to.
- `text` *(String)*: the contents to append.

**Returns:** `null`.
**Raises:** a string error if the path cannot be opened or written.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_c.txt', 'a');
IO.append_file('/tmp/tigr_doc_c.txt', 'b');
print(IO.read_file('/tmp/tigr_doc_c.txt'));   // => ab
IO.remove('/tmp/tigr_doc_c.txt');
```

### `read_bytes(path) -> Bytes`

Reads the entire file at `path` as raw bytes, with no UTF-8 decoding.

- `path` *(String)*: the file to read.

**Returns:** a `Bytes` buffer of the file contents.
**Raises:** a string error if the file is missing or unreadable.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_d.txt', 'hi');
b := IO.read_bytes('/tmp/tigr_doc_d.txt');
print(#b);   // => 2
IO.remove('/tmp/tigr_doc_d.txt');
```

### `write_bytes(path, bytes) -> null`

Writes a `Bytes` buffer to `path`, overwriting any existing file and creating it if absent.

- `path` *(String)*: the file to write.
- `bytes` *(Bytes)*: the raw bytes to write.

**Returns:** `null`.
**Raises:** a string error if the path cannot be written.

```tigr
IO := import 'IO';
Bytes := import 'Bytes';

IO.write_bytes('/tmp/tigr_doc_e.bin', Bytes.from_string('raw'));
print(IO.read_file('/tmp/tigr_doc_e.bin'));   // => raw
IO.remove('/tmp/tigr_doc_e.bin');
```

### `append_bytes(path, bytes) -> null`

Appends a `Bytes` buffer to the end of the file at `path`, creating the file if it does not exist.

- `path` *(String)*: the file to append to.
- `bytes` *(Bytes)*: the raw bytes to append.

**Returns:** `null`.
**Raises:** a string error if the path cannot be opened or written.

```tigr
IO := import 'IO';
Bytes := import 'Bytes';

IO.write_bytes('/tmp/tigr_doc_f.bin', Bytes.from_string('one'));
IO.append_bytes('/tmp/tigr_doc_f.bin', Bytes.from_string('two'));
print(IO.read_file('/tmp/tigr_doc_f.bin'));   // => onetwo
IO.remove('/tmp/tigr_doc_f.bin');
```

### `exists(path) -> Bool`

Tests whether anything exists at `path`, file or directory.

- `path` *(String)*: the path to test.

**Returns:** `true` if the path exists, otherwise `false`.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_g.txt', 'x');
print(IO.exists('/tmp/tigr_doc_g.txt'));    // => true
IO.remove('/tmp/tigr_doc_g.txt');
print(IO.exists('/tmp/tigr_doc_g.txt'));    // => false
```

### `list_dir(path) -> Array`

Lists the entries of the directory at `path`.

- `path` *(String)*: the directory to list.

**Returns:** an `Array` of `String` entry names. Order is not specified.
**Raises:** a string error if `path` is not a directory or cannot be read.

```tigr
IO := import 'IO';

IO.mkdir('/tmp/tigr_doc_dir');
IO.write_file('/tmp/tigr_doc_dir/a.txt', 'a');
print(IO.list_dir('/tmp/tigr_doc_dir'));   // => [a.txt]
IO.remove('/tmp/tigr_doc_dir');
```

### `mkdir(path) -> null`

Creates the directory at `path`, along with any missing parent directories.

- `path` *(String)*: the directory to create.

**Returns:** `null`.
**Raises:** a string error if the directory cannot be created.

```tigr
IO := import 'IO';

IO.mkdir('/tmp/tigr_doc_mk/nested');
print(IO.is_dir('/tmp/tigr_doc_mk/nested'));   // => true
IO.remove('/tmp/tigr_doc_mk');
```

### `remove(path) -> null`

Deletes the file at `path`, or, if `path` is a directory, removes it and all its contents recursively.

- `path` *(String)*: the path to delete.

**Returns:** `null`.
**Raises:** a string error if the path does not exist or cannot be removed.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_h.txt', 'x');
IO.remove('/tmp/tigr_doc_h.txt');
print(IO.exists('/tmp/tigr_doc_h.txt'));   // => false
```

### `is_dir(path) -> Bool`

Tests whether `path` is a directory.

- `path` *(String)*: the path to test.

**Returns:** `true` if `path` is a directory, otherwise `false` (including when it does not exist).

```tigr
IO := import 'IO';

IO.mkdir('/tmp/tigr_doc_isd');
print(IO.is_dir('/tmp/tigr_doc_isd'));   // => true
print(IO.is_dir('/tmp/tigr_doc_isd/missing'));   // => false
IO.remove('/tmp/tigr_doc_isd');
```

### `is_file(path) -> Bool`

Tests whether `path` is a regular file.

- `path` *(String)*: the path to test.

**Returns:** `true` if `path` is a regular file, otherwise `false` (including when it does not exist).

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_isf.txt', 'x');
print(IO.is_file('/tmp/tigr_doc_isf.txt'));   // => true
print(IO.is_file('/tmp'));   // => false
IO.remove('/tmp/tigr_doc_isf.txt');
```

### `stat(path) -> Object`

Reads filesystem metadata for `path`.

- `path` *(String)*: the path to inspect.

**Returns:** an `Object` `${size, is_dir, is_file, modified_ms}`. `size` is the byte count, `is_dir` and `is_file` are booleans, and `modified_ms` is the last-modified time in epoch milliseconds (or `null` on platforms that do not report it).
**Raises:** a string error if the path does not exist.

```tigr
IO := import 'IO';

IO.write_file('/tmp/tigr_doc_st.txt', 'hello tigr');
s := IO.stat('/tmp/tigr_doc_st.txt');
print(s.size);      // => 10
print(s.is_file);   // => true
IO.remove('/tmp/tigr_doc_st.txt');
```

### `read_line() -> String | null`

Reads one line from standard input, with the trailing newline stripped.

**Returns:** the line as a `String`, or `null` at end of input.
**Raises:** a string error if stdin cannot be read.

```tigr
IO := import 'IO';

line := IO.read_line();
if line != null { print('got:', line) }
```

### `eprint(value1, value2?) -> value`

Writes its arguments to standard error, matching `print`'s formatting: each argument in `str` form, space-separated, with a trailing newline.

- `value1` *(value)*: the first thing to print. `eprint` is variadic, so any number of arguments may follow.

**Returns:** the last argument, or `null` if called with none.

```tigr
IO := import 'IO';

IO.eprint('warning:', 'disk low');   // written to stderr
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#io): the authoritative spec for `IO`
- [Path](path.md): build and split path strings without touching the filesystem
- [Os](os.md): process environment, working directory, and subprocesses
- [Bytes](bytes.md): the buffer type returned by `read_bytes`
