# `IO`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#io)

`IO` covers file and stdio access, available without an `import`. It exposes two styles of file IO:

- **Whole-file ops** (`read_file`, `write_file`, `append_file`, and the `_bytes` variants) load or replace a file in one call. Convenient for small files; raises a catchable string-valued error on failure.
- **Streaming handle ops** (`open`, `read`, `read_line`, `read_until`, `read_exact`, `read_all`, `write`, `seek`, `tell`, `close`) process a file incrementally. The only way to handle a file larger than memory. Errors are structured `${kind, message}` (matching `Net`), so `catch (e) { e.kind == 'eof' }` works.

The predicate entries (`exists`, `is_dir`, `is_file`) never raise; they just report `false` for a path that does not exist.

The waiting calls (`read_file`, `write_file`, `append_file`, the `_bytes` variants, `list_dir`, `mkdir`, `remove`, `read_line`, `open`, `read`, `read_exact`, `read_until`, `read_all`, `write`) are *blocking* natives: inside a green thread they are offloaded to a worker pool so file IO does not stall the actor's other coroutines (see [concurrency](../language/concurrency.md)). The metadata-only calls (`exists`, `is_dir`, `is_file`, `stat`) and the no-syscall handle ops (`seek`, `tell`, `close`) stay inline, since offloading them would cost more than it saves.

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
| [`open(path, mode) -> File`](#openpath-mode---file) | Opens `path` and returns a streaming file handle. `mode` is `'r'`, `'w'`, or `'a'`. |
| [`read(file, n) -> Bytes`](#readfile-n---bytes) | Reads up to `n` bytes from `file`. An empty `Bytes` signals end-of-file. |
| [`read_exact(file, n) -> Bytes`](#read_exactfile-n---bytes) | Reads exactly `n` bytes, raising `eof` if the file ends first. |
| [`read_line(file?) -> String \| null`](#read_linefile---string--null) | Reads one line. With no argument, reads from stdin; with a file handle, reads from the file. `null` at EOF. |
| [`read_until(file, byte) -> Bytes \| null`](#read_untilfile-byte---bytes--null) | Reads up to and including the next `byte`. `null` at clean EOF. |
| [`read_all(file) -> Bytes`](#read_allfile---bytes) | Reads every remaining byte. |
| [`write(file, data) -> Int`](#writefile-data---int) | Writes `data` (`Bytes` or `String`) and returns the byte count. |
| [`seek(file, pos) -> null`](#seekfile-pos---null) | Seeks the file to absolute byte offset `pos`. |
| [`tell(file) -> Int`](#tellfile---int) | Reports the current logical position. |
| [`close(file) -> null`](#closefile---null) | Closes the handle. Idempotent. |
| [`eprint(value1, value2?) -> value`](#eprintvalue1-value2---value) | Writes its arguments to standard error, matching `print`'s formatting: each argument in `str` form, space-separated, with a trailing newline. |


### `read_file(path) -> String`

Reads the entire file at `path` and returns its contents decoded as UTF-8.

- `path` *(String)*: the file to read.

**Returns:** the file contents as a `String`.
**Raises:** a string error if the file is missing, unreadable, or not valid UTF-8.

```tigr
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
IO.write_file('/tmp/tigr_doc_h.txt', 'x');
IO.remove('/tmp/tigr_doc_h.txt');
print(IO.exists('/tmp/tigr_doc_h.txt'));   // => false
```

### `is_dir(path) -> Bool`

Tests whether `path` is a directory.

- `path` *(String)*: the path to test.

**Returns:** `true` if `path` is a directory, otherwise `false` (including when it does not exist).

```tigr
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
IO.write_file('/tmp/tigr_doc_st.txt', 'hello tigr');
s := IO.stat('/tmp/tigr_doc_st.txt');
print(s.size);      // => 10
print(s.is_file);   // => true
IO.remove('/tmp/tigr_doc_st.txt');
```

## Streaming file IO

For files too large to fit in memory, open a file handle and read from it incrementally. The pattern mirrors the [`Net`](net.md) socket API.

```tigr
f := IO.open('huge.log', 'r');
while ((line := IO.read_line(f)) != null) {
    if line[0..5] == 'ERROR' { print(line) }
};
IO.close(f);
```

The streaming ops raise **structured errors** of the form `${kind, message}` so a `catch` block can dispatch on `.kind`:

| `kind` | When it happens |
|--------|----------------|
| `io` | The underlying OS call failed (missing file, permission denied, ...). |
| `eof` | `read_exact` could not read the full count before end-of-file. Other reads signal EOF by returning empty `Bytes` or `null`. |
| `closed` | An operation was attempted on a closed handle. |
| `mode` | The op is not allowed for this handle's mode (read on a write-only handle, write on a read-only handle, wrong-type argument). |
| `invalid_mode` | `open` was passed a mode string other than `'r'`, `'w'`, or `'a'`. |
| `decode` | `read_line` decoded invalid UTF-8. |

### `open(path, mode) -> File`

Opens `path` and returns a file handle for streaming reads or writes.

- `path` *(String)*: the file to open.
- `mode` *(String)*: one of `'r'` (open existing for reading), `'w'` (create or truncate for writing), or `'a'` (create or open for appending).

**Returns:** a file handle.
**Raises:** `invalid_mode` if `mode` is anything else; `io` if the file cannot be opened in the requested mode.

```tigr
f := IO.open('/tmp/tigr_doc_open.txt', 'w');
IO.write(f, 'streamed');
IO.close(f);
print(IO.read_file('/tmp/tigr_doc_open.txt'));   // => streamed
IO.remove('/tmp/tigr_doc_open.txt');
```

### `read(file, n) -> Bytes`

Reads up to `n` bytes from `file`. May return fewer bytes than requested; an empty `Bytes` signals end-of-file.

- `file` *(File)*: a handle opened in `'r'` mode.
- `n` *(Int)*: maximum byte count.

**Returns:** a `Bytes` buffer of length `0..=n`.
**Raises:** `mode` if `file` is not opened for reading; `closed` if it has been closed; `io` on a read failure.

```tigr
IO.write_file('/tmp/tigr_doc_read.txt', 'hello world');
f := IO.open('/tmp/tigr_doc_read.txt', 'r');
print(Bytes.to_string(IO.read(f, 5)));   // => hello
IO.close(f);
IO.remove('/tmp/tigr_doc_read.txt');
```

### `read_exact(file, n) -> Bytes`

Reads exactly `n` bytes. Raises `eof` if the file ends before `n` bytes have been read.

- `file` *(File)*: a handle opened in `'r'` mode.
- `n` *(Int)*: exact byte count.

**Returns:** a `Bytes` buffer of length `n`.
**Raises:** `eof` on a short read; otherwise the same errors as `read`.

### `read_line(file?) -> String | null`

Reads one line. With no argument, reads from standard input; with a file handle, reads from the file. The trailing `\n` (and a preceding `\r`, if present) is stripped.

- `file` *(File, optional)*: omit for stdin; pass a handle to read from that file.

**Returns:** the line as a `String`, or `null` at end of input.
**Raises:** with a file handle: `mode`, `closed`, `io`, or `decode` (invalid UTF-8). With no argument: a string error if stdin cannot be read.

```tigr
// File form: stream a large file line by line.
f := IO.open('/etc/hosts', 'r');
while ((line := IO.read_line(f)) != null) {
    if #line > 0 && line[0] != '#' { print(line) }
};
IO.close(f);

// Stdin form (unchanged).
prompt := IO.read_line();
```

### `read_until(file, byte) -> Bytes | null`

Reads up to and including the next occurrence of `byte`. If the file ends before the delimiter is seen, any trailing bytes are returned as a final unterminated chunk; a subsequent call returns `null` at clean EOF.

- `file` *(File)*: a handle opened in `'r'` mode.
- `byte` *(Int)*: the delimiter byte (0..=255).

**Returns:** a `Bytes` buffer (delimiter included), or `null` at EOF.

### `read_all(file) -> Bytes`

Reads every remaining byte from the current position to end-of-file.

- `file` *(File)*: a handle opened in `'r'` mode.

**Returns:** a `Bytes` buffer of the rest of the file.

### `write(file, data) -> Int`

Writes every byte of `data` to `file` and returns the byte count written.

- `file` *(File)*: a handle opened in `'w'` or `'a'` mode.
- `data` *(Bytes or String)*: a `String` is written as its UTF-8 bytes.

**Returns:** the number of bytes written.
**Raises:** `mode` if `file` is not opened for writing; `closed` if it has been closed; `io` on a write failure.

```tigr
f := IO.open('/tmp/tigr_doc_write.bin', 'w');
IO.write(f, 'header\n');
IO.write(f, Bytes.from_array([1, 2, 3]));
IO.close(f);
IO.remove('/tmp/tigr_doc_write.bin');
```

### `seek(file, pos) -> null`

Seeks `file` to absolute byte offset `pos`, measured from the start of the file. Any bytes pre-buffered by a previous `read_line` / `read_until` are discarded so the next read sees the new region.

- `file` *(File)*: any open handle.
- `pos` *(Int)*: a non-negative absolute byte position.

**Returns:** `null`.
**Raises:** `io` on a negative position or a seek failure; `closed` if the handle is closed.

### `tell(file) -> Int`

Reports the current logical read/write position: the file's own cursor minus any bytes pre-buffered by a previous `read_line` / `read_until` that the user has not yet consumed. The number `tell` reports always equals the byte offset of the next `read` / `write`.

- `file` *(File)*: any open handle.

**Returns:** the current position as an `Int`.

### `close(file) -> null`

Closes `file`. Subsequent operations raise `closed`. Calling `close` on an already-closed handle is a no-op (idempotent).

- `file` *(File)*: the handle to close.

**Returns:** `null`.

### `eprint(value1, value2?) -> value`

Writes its arguments to standard error, matching `print`'s formatting: each argument in `str` form, space-separated, with a trailing newline.

- `value1` *(value)*: the first thing to print. `eprint` is variadic, so any number of arguments may follow.

**Returns:** the last argument, or `null` if called with none.

```tigr
IO.eprint('warning:', 'disk low');   // written to stderr
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#io): the authoritative spec for `IO`
- [Path](path.md): build and split path strings without touching the filesystem
- [Os](os.md): process environment, working directory, and subprocesses
- [Bytes](bytes.md): the buffer type returned by `read_bytes`
