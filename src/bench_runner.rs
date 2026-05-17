//! The `tigr bench` subcommand.
//!
//! Discovers benchmark files (every `.tg` under `bench/` by default,
//! or an explicit path), runs each one repeatedly, and reports the
//! min / mean wall time. `min` is the stable metric for before/after
//! comparison of an optimization — it is the run least disturbed by
//! scheduler noise.
//!
//! Timing is measured *inside* the already-started process, so it
//! covers lex+parse+compile+run but NOT process startup — exactly the
//! work v0.12's constant folding and peephole pass affect.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Instant;

use crate::vm;
use crate::vm::source_map::SourceMap;

/// Untimed runs before measurement, to warm caches.
const WARMUP: u32 = 2;
/// Hard cap on timed runs per file.
const MAX_ITERS: usize = 50;
/// Always do at least this many timed runs, even for slow files.
const MIN_ITERS: usize = 5;
/// Stop timing a file once this many seconds of timed runs elapse
/// (provided `MIN_ITERS` is met).
const TIME_BUDGET: f64 = 1.0;

/// Collect benchmark files reachable from `root`: a file is returned
/// as-is, a directory is walked recursively for `.tg` files. `target/`
/// and dot-directories are skipped. Sorted and deduplicated.
fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
    } else if root.is_dir() {
        walk(root, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
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
            walk(&path, out);
        } else if name.ends_with(".tg") {
            out.push(path);
        }
    }
}

/// Run one file once; return its wall time in seconds, or `None` if it
/// raised an uncaught error (which is printed).
fn run_once(file: &Path) -> Option<f64> {
    let sources = Rc::new(RefCell::new(SourceMap::new()));
    let start = Instant::now();
    let result = vm::run_file_with_map(file, sources.clone());
    let elapsed = start.elapsed().as_secs_f64();
    match result {
        Ok(_) => Some(elapsed),
        Err(err) => {
            eprintln!("{}", err.render(&sources.borrow()));
            None
        }
    }
}

/// Discover and run benchmarks under `path` (default: `bench/`).
pub fn run(path: Option<&str>) -> ExitCode {
    let root = Path::new(path.unwrap_or("bench"));
    let files = discover(root);
    if files.is_empty() {
        eprintln!("tigr bench: no .tg files found under {}", root.display());
        return ExitCode::SUCCESS;
    }
    let plural = if files.len() == 1 { "file" } else { "files" };
    println!("tigr bench — {} {plural}\n", files.len());

    let mut errored = 0u32;
    for file in &files {
        let label = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.display().to_string());

        // Warmup — abort this file if it errors.
        if (0..WARMUP).any(|_| run_once(file).is_none()) {
            println!("  {label:28}  ERRORED");
            errored += 1;
            continue;
        }

        // Timed runs, until the time budget or the iteration cap.
        let mut times = Vec::new();
        let overall = Instant::now();
        let mut failed = false;
        while times.len() < MAX_ITERS {
            match run_once(file) {
                Some(t) => times.push(t),
                None => {
                    failed = true;
                    break;
                }
            }
            if times.len() >= MIN_ITERS && overall.elapsed().as_secs_f64() >= TIME_BUDGET {
                break;
            }
        }
        if failed {
            println!("  {label:28}  ERRORED");
            errored += 1;
            continue;
        }

        let n = times.len();
        let min = times.iter().copied().fold(f64::INFINITY, f64::min);
        let mean = times.iter().sum::<f64>() / n as f64;
        println!(
            "  {label:28}  min {:9.3} ms   mean {:9.3} ms   ({n} runs)",
            min * 1000.0,
            mean * 1000.0,
        );
    }

    if errored > 0 {
        println!("\n{errored} file(s) errored");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
