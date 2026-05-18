//! The `tigr test` subcommand.
//!
//! Discovers test files, runs each through the VM, and sums the
//! suite-result objects they produce. A test file is any `.tg` file
//! whose name ends in `_test.tg`, or any `.tg` file under a `tests/`
//! directory. Each file's final expression is expected to be a
//! `Test.suite(...)` result object `${passed, failed, ...}`, or an
//! array of them; the runner reads the `passed`/`failed` fields. An
//! uncaught error in a file counts as a file-level failure.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
#[cfg(test)]
use std::sync::Arc;

use crate::vm;
use crate::vm::source_map::SourceMap;
use crate::vm::value::Value;

/// Collect the test files reachable from `root`. A file path is
/// returned as-is (so an explicit `tigr test foo.tg` always runs);
/// a directory is walked recursively. `target/` and dot-directories
/// are skipped. Result is sorted and deduplicated.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
    } else if root.is_dir() {
        walk(root, false, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

/// Recurse `dir`, appending matching test files. `in_tests` is true
/// once any ancestor directory was named `tests` — inside such a
/// directory every `.tg` file is a test; elsewhere only `*_test.tg`.
fn walk(dir: &Path, in_tests: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name.starts_with('.') {
                continue;
            }
            walk(&path, in_tests || name == "tests", out);
        } else if name.ends_with(".tg") && (in_tests || name.ends_with("_test.tg")) {
            out.push(path);
        }
    }
}

/// Running totals across every test file.
#[derive(Default)]
struct Totals {
    passed: i64,
    failed: i64,
    /// Files that raised an uncaught error (compile or runtime).
    errored: u32,
}

/// Add the `passed`/`failed` fields of a suite result (or an array of
/// them) into `totals`. Any other value shape is ignored — the file
/// ran fine but produced no suite result.
fn aggregate(value: &Value, totals: &mut Totals) {
    match value {
        Value::Object(o) => {
            let o = o.borrow();
            if let Some(Value::Int(p)) = o.get("passed") {
                totals.passed += p;
            }
            if let Some(Value::Int(f)) = o.get("failed") {
                totals.failed += f;
            }
        }
        Value::Array(a) => {
            for item in a.borrow().iter() {
                aggregate(item, totals);
            }
        }
        _ => {}
    }
}

/// Discover and run tests under `path` (default: the current
/// directory). Returns a failure exit code if any test failed or any
/// file errored.
pub fn run(path: Option<&str>) -> ExitCode {
    let root = Path::new(path.unwrap_or("."));
    let files = discover(root);
    if files.is_empty() {
        eprintln!("tigr test: no test files found under {}", root.display());
        return ExitCode::SUCCESS;
    }
    let plural = if files.len() == 1 { "file" } else { "files" };
    println!("tigr test — {} {plural}\n", files.len());

    let mut totals = Totals::default();
    for file in &files {
        println!("── {}", file.display());
        let sources = Rc::new(RefCell::new(SourceMap::new()));
        match vm::run_file_with_map(file, sources.clone()) {
            Ok((value, _)) => aggregate(&value, &mut totals),
            Err(err) => {
                eprintln!("{}", err.render(&sources.borrow()));
                totals.errored += 1;
            }
        }
        println!();
    }

    let mut summary = format!(
        "{} {plural}, {} passed, {} failed",
        files.len(),
        totals.passed,
        totals.failed,
    );
    if totals.errored > 0 {
        summary.push_str(&format!(", {} errored", totals.errored));
    }
    println!("{summary}");

    if totals.failed > 0 || totals.errored > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a throwaway directory tree under the system temp dir.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tigr_test_runner_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, "true").unwrap();
    }

    #[test]
    fn discover_matches_test_suffix_and_tests_dir() {
        let dir = scratch("discover");
        touch(&dir.join("math_test.tg"));
        touch(&dir.join("plain.tg")); // not a test
        touch(&dir.join("src/util_test.tg"));
        touch(&dir.join("tests/parser.tg")); // under tests/
        touch(&dir.join("tests/nested/lexer.tg"));
        touch(&dir.join("target/build_test.tg")); // skipped
        touch(&dir.join(".hidden/secret_test.tg")); // skipped

        let found = discover(&dir);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"math_test.tg".to_string()));
        assert!(names.contains(&"src/util_test.tg".to_string()));
        assert!(names.contains(&"tests/parser.tg".to_string()));
        assert!(names.contains(&"tests/nested/lexer.tg".to_string()));
        assert!(!names.iter().any(|n| n.contains("plain.tg")));
        assert!(!names.iter().any(|n| n.contains("target")));
        assert!(!names.iter().any(|n| n.contains("hidden")));
        assert_eq!(found.len(), 4);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_single_file_returned_verbatim() {
        let dir = scratch("single");
        let file = dir.join("anything.tg");
        touch(&file);
        assert_eq!(discover(&file), vec![file.clone()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn aggregate_sums_object_and_array() {
        let suite = |p: i64, f: i64| {
            let mut m = indexmap::IndexMap::new();
            m.insert(Arc::from("passed"), Value::Int(p));
            m.insert(Arc::from("failed"), Value::Int(f));
            Value::Object(crate::vm::gc::alloc_object(m))
        };
        let mut totals = Totals::default();
        aggregate(&suite(3, 1), &mut totals);
        aggregate(
            &Value::Array(crate::vm::gc::alloc_array(vec![suite(2, 0), suite(1, 4)])),
            &mut totals,
        );
        assert_eq!(totals.passed, 6);
        assert_eq!(totals.failed, 5);
    }
}
