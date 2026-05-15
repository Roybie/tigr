//! Tigr CLI.
//!
//! Usage: `tigr [<file.tg> [args...]]`. With no file argument we
//! launch the interactive REPL (v0.3 Phase 5).
//!
//! The `--legacy` flag is reserved for re-enabling the v0.1 tree-walking
//! interpreter once `src/v01/` is wired back into the build.

use std::cell::RefCell;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;

use vm::source_map::SourceMap;

mod repl;
mod v01;
mod vm;

#[cfg(test)]
mod tests;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut filename: Option<&str> = None;
    let mut legacy = false;
    // First non-flag arg is the script. Anything after that is
    // program-level args (visible from tigr via `Os.args`).
    for arg in args.iter().skip(1) {
        if filename.is_some() {
            break;
        }
        match arg.as_str() {
            "--legacy" => legacy = true,
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other => filename = Some(other),
        }
    }
    if filename.is_none() && !legacy {
        // No script and not legacy mode → enter the REPL.
        let mut repl = repl::Repl::new();
        return match repl.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("tigr: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let Some(filename) = filename else {
        print_usage();
        return ExitCode::FAILURE;
    };
    if legacy {
        eprintln!(
            "tigr: --legacy mode is not currently wired in. \
             See src/v01/mod.rs for restoration instructions."
        );
        return ExitCode::FAILURE;
    }
    let sources = Rc::new(RefCell::new(SourceMap::new()));
    match vm::run_file_with_map(Path::new(filename), sources.clone()) {
        Ok((value, _)) => {
            println!("{value:?}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{}", err.render(&sources.borrow()));
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("usage: tigr [<file.tg> [args...]]");
    eprintln!("       tigr                       (interactive REPL)");
    eprintln!("       tigr --legacy <file.tg>    (v0.1 interpreter; not currently wired)");
}
