# `Path`

> Native (Rust) module
> Spec: [LANGUAGE.md §13.2](../../LANGUAGE.md#path-v06)

`Path` manipulates path strings, available without an `import`. Every entry is pure string computation: nothing here touches the filesystem, so the results are deterministic and the same code runs whether or not the path exists. Paths are POSIX-style on every platform — `/` is the one separator and a leading `/` means absolute — so the same logical paths behave identically on Linux, macOS, Windows, and the browser (the native filesystem accepts `/` on Windows too). The only error any entry raises is for a non-`String` argument. For path operations that do read the disk, see [`IO`](io.md).

## Functions

| Function | Summary |
|----------|---------|
| [`join(part1, part2?) -> String`](#joinpart1-part2---string) | Joins path segments into one path, inserting `/` between them. An absolute segment (one starting with `/`) resets the path. |
| [`dirname(path) -> String`](#dirnamepath---string) | Returns the parent-directory portion of `path`. |
| [`basename(path) -> String`](#basenamepath---string) | Returns the final component of `path`, the file or directory name without its parent. |
| [`ext(path) -> String`](#extpath---string) | Returns the file extension of `path`, without the leading dot. |
| [`is_absolute(path) -> Bool`](#is_absolutepath---bool) | Tests whether `path` is absolute — POSIX-style, a leading `/` — on every platform. |


### `join(part1, part2?) -> String`

Joins path segments into one path, inserting `/` between them. An absolute segment (one starting with `/`) resets the path.

- `part1` *(String)*: the first segment. `join` is variadic, so any number of segments may follow.

**Returns:** the joined path as a `String`.
**Raises:** a string error if any argument is not a `String`.

```tigr
print(Path.join('usr', 'local', 'bin'));   // => usr/local/bin
print(Path.join('docs', 'stdlib.md'));     // => docs/stdlib.md
```

### `dirname(path) -> String`

Returns the parent-directory portion of `path`.

- `path` *(String)*: the path to split.

**Returns:** the parent directory as a `String`, or `''` if `path` has no parent component.
**Raises:** a string error if `path` is not a `String`.

```tigr
print(Path.dirname('/usr/local/bin/tigr'));   // => /usr/local/bin
print(Path.dirname('file.txt'));              // =>
```

### `basename(path) -> String`

Returns the final component of `path`, the file or directory name without its parent.

- `path` *(String)*: the path to split.

**Returns:** the final component as a `String`, or `''` if `path` has no final component.
**Raises:** a string error if `path` is not a `String`.

```tigr
print(Path.basename('/usr/local/bin/tigr'));   // => tigr
print(Path.basename('report.pdf'));            // => report.pdf
```

### `ext(path) -> String`

Returns the file extension of `path`, without the leading dot.

- `path` *(String)*: the path to inspect.

**Returns:** the extension as a `String`, or `''` if there is none. A name with multiple dots returns only the last segment.
**Raises:** a string error if `path` is not a `String`.

```tigr
print(Path.ext('archive.tar.gz'));   // => gz
print(Path.ext('README'));           // =>
```

### `is_absolute(path) -> Bool`

Tests whether `path` is absolute — POSIX-style, a leading `/` — on every platform.

- `path` *(String)*: the path to test.

**Returns:** `true` if `path` is absolute, otherwise `false`.
**Raises:** a string error if `path` is not a `String`.

```tigr
print(Path.is_absolute('/etc/hosts'));     // => true
print(Path.is_absolute('docs/file.md'));   // => false
```

## See also

- [LANGUAGE.md §13.2](../../LANGUAGE.md#path-v06): the authoritative spec for `Path`
- [IO](io.md): file operations that read and write the actual filesystem
- [Os](os.md): the working directory and process environment
